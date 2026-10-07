//! Per-SerializedFile object identity indexing and TypeTree-backed PPtr collection.

use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use super::file::{SerializedFile, SerializedFileError};
use super::metadata::{FileIdentifier, SerializedFileMetadata, TypeTree};
use super::object::{ObjectDecodeLimits, SerializedValue};
use super::references::PPtrResolution;

type ExternalDependencyResolver<'a> = dyn FnMut(usize, &FileIdentifier) -> Option<usize> + 'a;

#[derive(Clone, Copy)]
struct ReferenceSource<'a> {
    file_index: usize,
    metadata: &'a SerializedFileMetadata,
    graph: &'a ObjectReferenceGraph,
}

/// Bounds for constructing a per-file reference scan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReferenceGraphLimits {
    pub object_decode: ObjectDecodeLimits,
    pub max_objects: usize,
    pub max_edges: usize,
    pub max_issues: usize,
    pub max_field_path_bytes: usize,
}

impl Default for ReferenceGraphLimits {
    fn default() -> Self {
        Self {
            object_decode: ObjectDecodeLimits::default(),
            max_objects: 4_000_000,
            max_edges: 1_000_000,
            max_issues: 100_000,
            max_field_path_bytes: 1024 * 1024,
        }
    }
}

/// One typed PPtr found in a decoded serialized object.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SerializedObjectReference {
    pub source_object_index: usize,
    pub source_path_id: i64,
    /// Field path within the decoded TypeTree value; arrays include zero-based indexes.
    pub field_path: String,
    pub file_id: i32,
    pub path_id: i64,
}

/// A reason a reference scan did not decode every recognized reference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReferenceGraphIssue {
    ObjectDecodeFailed {
        object_index: usize,
        path_id: i64,
        error: SerializedFileError,
    },
    UnparsedTrailingPayload {
        object_index: usize,
        path_id: i64,
        byte_count: usize,
    },
    UnparsedRawValue {
        object_index: usize,
        path_id: i64,
        field_path: String,
        byte_count: usize,
    },
    FieldPathLimitExceeded {
        object_index: usize,
        path_id: i64,
        parent_field_path: String,
        component_bytes: usize,
        limit: usize,
    },
    EdgeLimitExceeded {
        object_index: usize,
        path_id: i64,
        field_path: String,
        limit: usize,
    },
}

/// A per-file index and scan of TypeTree-decoded PPtr values.
///
/// External FileIDs are classified against the SerializedFile's external table. This does not
/// load external files, resolve their PathIDs, construct runtime asset objects, or claim that
/// undecodable object payloads have no references. Such payloads remain explicit in `issues`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectReferenceGraph {
    object_count: usize,
    attempted_objects: usize,
    /// Sorted by PathID, then source object index, so duplicate identities remain visible.
    path_id_index: Vec<(i64, usize)>,
    references: Vec<SerializedObjectReference>,
    issues: Vec<ReferenceGraphIssue>,
}

impl ObjectReferenceGraph {
    pub fn build(file: &SerializedFile<'_>) -> Result<Self, ReferenceGraphBuildError> {
        Self::build_with_limits(file, ReferenceGraphLimits::default())
    }

    pub fn build_with_limits(
        file: &SerializedFile<'_>,
        limits: ReferenceGraphLimits,
    ) -> Result<Self, ReferenceGraphBuildError> {
        let object_count = file.metadata.objects.len();
        if object_count > limits.max_objects {
            return Err(ReferenceGraphBuildError::ObjectCountLimitExceeded {
                object_count,
                limit: limits.max_objects,
            });
        }
        let mut path_id_index = Vec::new();
        path_id_index
            .try_reserve_exact(object_count)
            .map_err(|_| ReferenceGraphBuildError::allocation("PathID index", object_count))?;
        path_id_index.extend(
            file.metadata
                .objects
                .iter()
                .enumerate()
                .map(|(index, object)| (object.path_id, index)),
        );
        path_id_index.sort_unstable();

        let mut graph = Self {
            object_count,
            attempted_objects: 0,
            path_id_index,
            references: Vec::new(),
            issues: Vec::new(),
        };

        for (object_index, object) in file.metadata.objects.iter().enumerate() {
            graph.attempted_objects += 1;
            let payload =
                match file.decode_object_payload_with_limits(object_index, &limits.object_decode) {
                    Ok(payload) => payload,
                    Err(error) => {
                        graph.push_issue(
                            ReferenceGraphIssue::ObjectDecodeFailed {
                                object_index,
                                path_id: object.path_id,
                                error,
                            },
                            limits.max_issues,
                        )?;
                        continue;
                    }
                };
            if !payload.trailing_bytes.is_empty() {
                graph.push_issue(
                    ReferenceGraphIssue::UnparsedTrailingPayload {
                        object_index,
                        path_id: object.path_id,
                        byte_count: payload.trailing_bytes.len(),
                    },
                    limits.max_issues,
                )?;
            }

            let mut field_path = String::new();
            match collect_references(
                &payload.root,
                &mut field_path,
                object_index,
                object.path_id,
                &mut graph.references,
                &mut graph.issues,
                limits,
            ) {
                Ok(()) => {}
                Err(WalkIssue::FieldPathLimitExceeded {
                    parent_field_path,
                    component_bytes,
                }) => {
                    graph.push_issue(
                        ReferenceGraphIssue::FieldPathLimitExceeded {
                            object_index,
                            path_id: object.path_id,
                            parent_field_path,
                            component_bytes,
                            limit: limits.max_field_path_bytes,
                        },
                        limits.max_issues,
                    )?;
                }
                Err(WalkIssue::EdgeLimitExceeded { field_path }) => {
                    graph.push_issue(
                        ReferenceGraphIssue::EdgeLimitExceeded {
                            object_index,
                            path_id: object.path_id,
                            field_path,
                            limit: limits.max_edges,
                        },
                        limits.max_issues,
                    )?;
                    break;
                }
                Err(WalkIssue::AllocationFailed {
                    resource,
                    requested,
                }) => {
                    return Err(ReferenceGraphBuildError::allocation(resource, requested));
                }
                Err(WalkIssue::IssueLimitExceeded {
                    object_index,
                    path_id,
                    limit,
                }) => {
                    return Err(ReferenceGraphBuildError::IssueLimitExceeded {
                        object_index,
                        path_id,
                        limit,
                    });
                }
            }
        }

        Ok(graph)
    }

    pub fn object_count(&self) -> usize {
        self.object_count
    }

    /// Number of object payloads attempted, including payloads that produced a decode issue.
    pub fn attempted_object_count(&self) -> usize {
        self.attempted_objects
    }

    pub fn references(&self) -> &[SerializedObjectReference] {
        &self.references
    }

    pub fn issues(&self) -> &[ReferenceGraphIssue] {
        &self.issues
    }

    /// True when this object's payload and every field visible to the reference scan were parsed.
    pub fn object_scan_is_complete(&self, object_index: usize) -> bool {
        object_index < self.object_count
            && object_index < self.attempted_objects
            && !self
                .issues
                .iter()
                .any(|issue| reference_issue_object_index(issue) == object_index)
    }

    /// Returns every object-table index with this PathID, preserving duplicate identities.
    pub fn object_indices_for_path_id(
        &self,
        path_id: i64,
    ) -> impl ExactSizeIterator<Item = usize> + '_ {
        let first = self
            .path_id_index
            .partition_point(|(candidate, _)| *candidate < path_id);
        let end = self
            .path_id_index
            .partition_point(|(candidate, _)| *candidate <= path_id);
        self.path_id_index[first..end]
            .iter()
            .map(|(_, object_index)| *object_index)
    }

    /// Returns the PPtr edges recorded for one source object in field traversal order.
    pub fn references_from_object(
        &self,
        object_index: usize,
    ) -> impl Iterator<Item = &SerializedObjectReference> {
        let first = self
            .references
            .partition_point(|reference| reference.source_object_index < object_index);
        let end = self
            .references
            .partition_point(|reference| reference.source_object_index <= object_index);
        self.references[first..end].iter()
    }

    /// True only when every object's TypeTree payload was decoded and every recognized PPtr was
    /// collected within the configured bounds.
    pub fn is_complete(&self) -> bool {
        self.path_id_index.len() == self.object_count
            && self.attempted_objects == self.object_count
            && self.issues.is_empty()
    }

    /// Classifies an edge using this graph's PathID index and the source one-based external table.
    pub fn resolve_pointer<'a>(
        &self,
        metadata: &'a SerializedFileMetadata,
        file_id: i32,
        path_id: i64,
    ) -> PPtrResolution<'a> {
        if path_id == 0 {
            return PPtrResolution::Null;
        }

        if file_id == 0 {
            let first = self
                .path_id_index
                .partition_point(|(candidate, _)| *candidate < path_id);
            let end = self
                .path_id_index
                .partition_point(|(candidate, _)| *candidate <= path_id);
            return match end - first {
                0 => PPtrResolution::MissingLocalObject { path_id },
                1 => PPtrResolution::LocalObject {
                    object_index: self.path_id_index[first].1,
                },
                match_count => PPtrResolution::AmbiguousLocalObject {
                    path_id,
                    match_count,
                },
            };
        }

        let Some(external_index) = external_table_index(file_id, metadata.externals.len()) else {
            return PPtrResolution::InvalidFileId {
                file_id,
                external_count: metadata.externals.len(),
            };
        };
        let identifier = &metadata.externals[external_index];
        PPtrResolution::ExternalDependency {
            file_id,
            external_index,
            path_id,
            identifier,
        }
    }

    fn push_issue(
        &mut self,
        issue: ReferenceGraphIssue,
        maximum_issues: usize,
    ) -> Result<(), ReferenceGraphBuildError> {
        let (object_index, path_id) = match &issue {
            ReferenceGraphIssue::ObjectDecodeFailed {
                object_index,
                path_id,
                ..
            }
            | ReferenceGraphIssue::UnparsedTrailingPayload {
                object_index,
                path_id,
                ..
            }
            | ReferenceGraphIssue::UnparsedRawValue {
                object_index,
                path_id,
                ..
            }
            | ReferenceGraphIssue::FieldPathLimitExceeded {
                object_index,
                path_id,
                ..
            }
            | ReferenceGraphIssue::EdgeLimitExceeded {
                object_index,
                path_id,
                ..
            } => (*object_index, *path_id),
        };
        if self.issues.len() >= maximum_issues {
            return Err(ReferenceGraphBuildError::IssueLimitExceeded {
                object_index,
                path_id,
                limit: maximum_issues,
            });
        }
        self.issues
            .try_reserve(1)
            .map_err(|_| ReferenceGraphBuildError::allocation("reference scan issues", 1))?;
        self.issues.push(issue);
        Ok(())
    }
}

fn reference_issue_object_index(issue: &ReferenceGraphIssue) -> usize {
    match issue {
        ReferenceGraphIssue::ObjectDecodeFailed { object_index, .. }
        | ReferenceGraphIssue::UnparsedTrailingPayload { object_index, .. }
        | ReferenceGraphIssue::UnparsedRawValue { object_index, .. }
        | ReferenceGraphIssue::FieldPathLimitExceeded { object_index, .. }
        | ReferenceGraphIssue::EdgeLimitExceeded { object_index, .. } => *object_index,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LocalPathIdResolution {
    Unique(usize),
    Missing,
    Ambiguous(usize),
}

/// `Bundle.ResolveCollection` engine-resource fallback: when the fixed dependency
/// name has no loaded file, these spellings alias each other before the dependency
/// counts as missing. Resolver-driven builds delegate this fallback to the caller
/// resolver; it applies here to the flat name join only.
fn engine_resource_alias(normalized_name: &str) -> Option<&'static str> {
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

/// `AssetCollection.TryGetDependency` file-index gate over caller facts.
///
/// PPtr file 0 addresses the local file (callers handle it before this gate);
/// positive file `n` addresses external slot `n - 1`. Negative, shifted, and
/// past-the-table IDs have no slot: callers map the miss to their invalid-ID
/// outcome, mirroring the source null return. A returned index is always
/// in-bounds for a table of `external_count` entries.
fn external_table_index(file_id: i32, external_count: usize) -> Option<usize> {
    let index = file_id.checked_sub(1)?;
    let index = usize::try_from(index).ok()?;
    (index < external_count).then_some(index)
}

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReferenceGraphBuildError {
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    ObjectCountLimitExceeded {
        object_count: usize,
        limit: usize,
    },
    IssueLimitExceeded {
        object_index: usize,
        path_id: i64,
        limit: usize,
    },
}

impl ReferenceGraphBuildError {
    fn allocation(resource: &'static str, requested: usize) -> Self {
        Self::AllocationFailed {
            resource,
            requested,
        }
    }
}

impl Display for ReferenceGraphBuildError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AllocationFailed { resource, requested } => write!(
                f,
                "could not allocate {requested} entries for the {resource} reference graph"
            ),
            Self::ObjectCountLimitExceeded { object_count, limit } => write!(
                f,
                "SerializedFile has {object_count} objects, exceeding reference graph limit {limit}"
            ),
            Self::IssueLimitExceeded {
                object_index,
                path_id,
                limit,
            } => write!(
                f,
                "reference graph issue limit {limit} exceeded at object {object_index} PathID {path_id}"
            ),
        }
    }
}

impl Error for ReferenceGraphBuildError {}

#[derive(Debug)]
enum WalkIssue {
    FieldPathLimitExceeded {
        parent_field_path: String,
        component_bytes: usize,
    },
    EdgeLimitExceeded {
        field_path: String,
    },
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    IssueLimitExceeded {
        object_index: usize,
        path_id: i64,
        limit: usize,
    },
}

fn collect_references(
    value: &SerializedValue,
    field_path: &mut String,
    object_index: usize,
    source_path_id: i64,
    output: &mut Vec<SerializedObjectReference>,
    issues: &mut Vec<ReferenceGraphIssue>,
    limits: ReferenceGraphLimits,
) -> Result<(), WalkIssue> {
    match value {
        SerializedValue::Struct(fields) => {
            for field in fields {
                let old_length = append_path_component(
                    field_path,
                    &field.name,
                    !field_path.is_empty(),
                    limits.max_field_path_bytes,
                )?;
                let nested_result = collect_references(
                    &field.value,
                    field_path,
                    object_index,
                    source_path_id,
                    output,
                    issues,
                    limits,
                );
                field_path.truncate(old_length);
                nested_result?;
            }
        }
        SerializedValue::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                let component = format!("[{index}]");
                let old_length = append_path_component(
                    field_path,
                    &component,
                    false,
                    limits.max_field_path_bytes,
                )?;
                let nested_result = collect_references(
                    value,
                    field_path,
                    object_index,
                    source_path_id,
                    output,
                    issues,
                    limits,
                );
                field_path.truncate(old_length);
                nested_result?;
            }
        }
        SerializedValue::PPtr { file_id, path_id } => {
            if output.len() >= limits.max_edges {
                return Err(WalkIssue::EdgeLimitExceeded {
                    field_path: copy_field_path(field_path)?,
                });
            }
            output
                .try_reserve(1)
                .map_err(|_| WalkIssue::AllocationFailed {
                    resource: "reference edges",
                    requested: 1,
                })?;
            output.push(SerializedObjectReference {
                source_object_index: object_index,
                source_path_id,
                field_path: copy_field_path(field_path)?,
                file_id: *file_id,
                path_id: *path_id,
            });
        }
        SerializedValue::Raw { bytes, .. } => {
            if issues.len() >= limits.max_issues {
                return Err(WalkIssue::IssueLimitExceeded {
                    object_index,
                    path_id: source_path_id,
                    limit: limits.max_issues,
                });
            }
            issues
                .try_reserve(1)
                .map_err(|_| WalkIssue::AllocationFailed {
                    resource: "reference scan issues",
                    requested: 1,
                })?;
            issues.push(ReferenceGraphIssue::UnparsedRawValue {
                object_index,
                path_id: source_path_id,
                field_path: copy_field_path(field_path)?,
                byte_count: bytes.len(),
            });
        }
        _ => {}
    }
    Ok(())
}

fn append_path_component(
    path: &mut String,
    component: &str,
    add_separator: bool,
    maximum_bytes: usize,
) -> Result<usize, WalkIssue> {
    let old_length = path.len();
    let component_bytes = component
        .len()
        .checked_add(usize::from(add_separator))
        .ok_or(WalkIssue::AllocationFailed {
            resource: "field path",
            requested: usize::MAX,
        })?;
    let new_length =
        old_length
            .checked_add(component_bytes)
            .ok_or(WalkIssue::AllocationFailed {
                resource: "field path",
                requested: usize::MAX,
            })?;
    if new_length > maximum_bytes {
        return Err(WalkIssue::FieldPathLimitExceeded {
            parent_field_path: copy_field_path(path)?,
            component_bytes,
        });
    }
    path.try_reserve(component_bytes)
        .map_err(|_| WalkIssue::AllocationFailed {
            resource: "field path",
            requested: component_bytes,
        })?;
    if add_separator && !path.is_empty() {
        path.push('.');
    }
    path.push_str(component);
    Ok(old_length)
}

fn copy_field_path(path: &str) -> Result<String, WalkIssue> {
    let mut copy = String::new();
    copy.try_reserve_exact(path.len())
        .map_err(|_| WalkIssue::AllocationFailed {
            resource: "field path copy",
            requested: path.len(),
        })?;
    copy.push_str(path);
    Ok(copy)
}

#[cfg(test)]
mod tests {
    use super::{
        collect_references, MultiFileObjectIdentity, MultiFileObjectReference,
        MultiFilePPtrResolution, MultiFileReferenceGraph, MultiFileReferenceGraphBuildError,
        MultiFileReferenceGraphLimits, NamedObjectReferenceGraph, ObjectReferenceGraph,
        ReferenceGraphLimits, SerializedObjectReference, WalkIssue,
    };
    use crate::io::binary::Endian;
    use crate::processing::asset_bundle::{
        map_asset_bundle_container, resolve_asset_bundle_container_reference,
        AssetBundleContainerTarget,
    };
    use crate::processing::scene_collections::{
        AssetBundleContainerReference, SceneBundleMappingLimits,
    };
    use crate::serialized_file::metadata::{
        FileIdentifier, ObjectInfo, SerializedFileMetadata, SerializedType, TypeTree, TypeTreeNode,
    };
    use crate::serialized_file::metadata_prefix::SerializedFileMetadataPrefix;
    use crate::serialized_file::object::{SerializedFieldValue, SerializedValue};
    use crate::serialized_file::references::PPtrResolution;

    fn metadata(path_ids: &[i64]) -> SerializedFileMetadata {
        SerializedFileMetadata {
            prefix: SerializedFileMetadataPrefix {
                metadata_offset: 0,
                metadata_size: 0,
                body_endian: Endian::Little,
                unity_version_text: None,
                target_platform: None,
                has_type_tree: false,
                consumed_bytes: 0,
            },
            types: Vec::new(),
            long_file_id: None,
            objects: path_ids.iter().map(|path_id| object(*path_id)).collect(),
            script_types: Vec::new(),
            externals: vec![external("shared.assets"), external("scene.assets")],
            reference_types: Vec::new(),
            user_information: None,
            trailing_metadata: Vec::new(),
            consumed_bytes: 0,
        }
    }

    fn object(path_id: i64) -> ObjectInfo {
        ObjectInfo {
            path_id,
            byte_start: 0,
            byte_size: 0,
            serialized_type_index: None,
            raw_type_id: 0,
            class_id: None,
            script_type_index: None,
            is_destroyed: None,
            is_stripped: None,
            resolved_type_index: None,
        }
    }

    fn external(path: &str) -> FileIdentifier {
        FileIdentifier {
            asset_path: Some(path.to_owned()),
            guid: None,
            file_type: None,
            path_name_origin: path.to_owned(),
        }
    }

    #[test]
    fn external_file_ids_address_table_slots_like_try_get_dependency() {
        use super::external_table_index;
        // File 1 addresses slot 0; higher files shift by one like Dependencies.
        assert_eq!(external_table_index(1, 2), Some(0));
        assert_eq!(external_table_index(2, 2), Some(1));
        // File 0 is the local file (handled before the gate); negative,
        // shifted, and past-the-table IDs have no slot.
        assert_eq!(external_table_index(0, 2), None);
        assert_eq!(external_table_index(-1, 2), None);
        assert_eq!(external_table_index(i32::MIN, 2), None);
        assert_eq!(external_table_index(3, 2), None);
        assert_eq!(external_table_index(1, 0), None);
        assert_eq!(external_table_index(i32::MAX, 0), None);
    }

    fn empty_graph(path_ids: &[i64]) -> ObjectReferenceGraph {
        let mut path_id_index: Vec<_> = path_ids
            .iter()
            .enumerate()
            .map(|(index, path_id)| (*path_id, index))
            .collect();
        path_id_index.sort_unstable();
        ObjectReferenceGraph {
            object_count: path_ids.len(),
            attempted_objects: path_ids.len(),
            path_id_index,
            references: Vec::new(),
            issues: Vec::new(),
        }
    }

    #[test]
    fn asset_bundle_container_pptrs_resolve_in_the_owning_file_scope() {
        let owner_metadata = metadata(&[10, 20]);
        let graph = empty_graph(&[10, 20]);
        let reference = |file_id, path_id| AssetBundleContainerReference {
            path: "Assets/entry.asset".to_owned(),
            file_id,
            path_id,
        };
        assert_eq!(
            resolve_asset_bundle_container_reference(&reference(0, 20), &owner_metadata, &graph),
            AssetBundleContainerTarget::LocalObject { object_index: 1 }
        );
        assert_eq!(
            resolve_asset_bundle_container_reference(&reference(0, 0), &owner_metadata, &graph),
            AssetBundleContainerTarget::Null
        );
        assert_eq!(
            resolve_asset_bundle_container_reference(&reference(2, 99), &owner_metadata, &graph),
            AssetBundleContainerTarget::SharedFile {
                file_id: 2,
                external_index: 1,
                path_id: 99,
            }
        );
        assert_eq!(
            resolve_asset_bundle_container_reference(&reference(0, 99), &owner_metadata, &graph),
            AssetBundleContainerTarget::MissingLocalObject { path_id: 99 }
        );
        assert_eq!(
            resolve_asset_bundle_container_reference(
                &reference(0, 10),
                &metadata(&[10, 10]),
                &empty_graph(&[10, 10]),
            ),
            AssetBundleContainerTarget::AmbiguousLocalObject {
                path_id: 10,
                match_count: 2,
            }
        );
        assert_eq!(
            resolve_asset_bundle_container_reference(&reference(3, 99), &owner_metadata, &graph),
            AssetBundleContainerTarget::InvalidFileId {
                file_id: 3,
                external_count: 2,
            }
        );
    }

    #[test]
    fn asset_bundle_container_map_applies_name_rules_and_preserves_target_kinds() {
        let metadata = metadata(&[10, 20]);
        let graph = empty_graph(&[10, 20]);
        let container_entry = |path: &str, file_id, path_id| {
            SerializedValue::Struct(vec![
                SerializedFieldValue {
                    name: "first".to_owned(),
                    value: SerializedValue::StringBytes(path.as_bytes().to_vec()),
                },
                SerializedFieldValue {
                    name: "second".to_owned(),
                    value: SerializedValue::Struct(vec![SerializedFieldValue {
                        name: "asset".to_owned(),
                        value: SerializedValue::PPtr { file_id, path_id },
                    }]),
                },
            ])
        };
        let root = SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "m_AssetBundleName_R".to_owned(),
                value: SerializedValue::StringBytes(b"character.bundle".to_vec()),
            },
            SerializedFieldValue {
                name: "m_Container".to_owned(),
                value: SerializedValue::Array(vec![
                    container_entry("Assets/local.asset", 0, 20),
                    container_entry("Assets/shared.asset", 2, 99),
                    container_entry("Assets/null.asset", 0, 0),
                ]),
            },
        ]);
        let mapping = map_asset_bundle_container(
            &root,
            "source.bundle",
            &metadata,
            &graph,
            SceneBundleMappingLimits::default(),
        )
        .unwrap();

        assert_eq!(mapping.source_name, "character.bundle");
        assert_eq!(mapping.asset_name, "character");
        assert_eq!(
            mapping
                .references
                .iter()
                .map(|entry| entry.target.clone())
                .collect::<Vec<_>>(),
            [
                AssetBundleContainerTarget::LocalObject { object_index: 1 },
                AssetBundleContainerTarget::SharedFile {
                    file_id: 2,
                    external_index: 1,
                    path_id: 99,
                },
                AssetBundleContainerTarget::Null,
            ]
        );
    }

    fn graph_with_reference(
        path_ids: &[i64],
        source_path_id: i64,
        file_id: i32,
        path_id: i64,
    ) -> ObjectReferenceGraph {
        let mut graph = empty_graph(path_ids);
        graph.references.push(SerializedObjectReference {
            source_object_index: 0,
            source_path_id,
            field_path: "m_Target".to_owned(),
            file_id,
            path_id,
        });
        graph
    }

    #[test]
    fn multi_file_reference_graph_exposes_an_objects_resolved_type_tree() {
        let type_tree = TypeTree {
            nodes: vec![TypeTreeNode {
                level: 0,
                version: 1,
                type_flags: 0,
                type_string_offset: None,
                name_string_offset: None,
                type_name: Some("Transform".to_owned()),
                name: Some(String::new()),
                byte_size: -1,
                index: 0,
                meta_flags: 0,
                reference_type_hash: None,
            }],
            string_buffer: Vec::new(),
        };
        let mut source_metadata = metadata(&[10]);
        source_metadata.types.push(SerializedType {
            raw_type_id: 4,
            type_id_for_hash: 4,
            is_stripped_type: false,
            script_type_index: -1,
            script_id: None,
            old_type_hash: None,
            type_tree: Some(type_tree.clone()),
            type_dependencies: Vec::new(),
        });
        source_metadata.objects[0].resolved_type_index = Some(0);
        let source_graph = empty_graph(&[10]);
        let files = [NamedObjectReferenceGraph {
            file_name: "scene.assets",
            metadata: &source_metadata,
            graph: &source_graph,
        }];
        let references = MultiFileReferenceGraph::build(&files).unwrap();

        assert_eq!(references.object_type_tree(0, 0), Some(&type_tree));
        assert_eq!(references.object_type_tree(0, 1), None);
        assert_eq!(references.object_type_tree(1, 0), None);
    }

    #[test]
    fn indexes_duplicate_ids_and_classifies_local_external_and_null_edges() {
        let metadata = metadata(&[101, 202, 101]);
        let graph = empty_graph(&[101, 202, 101]);
        assert_eq!(
            graph.object_indices_for_path_id(101).collect::<Vec<_>>(),
            [0, 2]
        );
        assert_eq!(
            graph.object_indices_for_path_id(202).collect::<Vec<_>>(),
            [1]
        );
        assert_eq!(graph.object_indices_for_path_id(303).count(), 0);
        assert_eq!(
            graph.resolve_pointer(&metadata, 0, 202),
            PPtrResolution::LocalObject { object_index: 1 }
        );
        assert_eq!(
            graph.resolve_pointer(&metadata, 0, 101),
            PPtrResolution::AmbiguousLocalObject {
                path_id: 101,
                match_count: 2
            }
        );
        assert_eq!(
            graph.resolve_pointer(&metadata, 0, 303),
            PPtrResolution::MissingLocalObject { path_id: 303 }
        );
        assert_eq!(graph.resolve_pointer(&metadata, 2, 0), PPtrResolution::Null);
        assert_eq!(
            graph.resolve_pointer(&metadata, 2, 404),
            PPtrResolution::ExternalDependency {
                file_id: 2,
                external_index: 1,
                path_id: 404,
                identifier: &metadata.externals[1],
            }
        );
        assert_eq!(
            graph.resolve_pointer(&metadata, -1, 404),
            PPtrResolution::InvalidFileId {
                file_id: -1,
                external_count: 2,
            }
        );
    }

    #[test]
    fn joins_unique_external_file_names_and_resolves_target_path_ids() {
        let source_metadata = metadata(&[11]);
        let source_graph = graph_with_reference(&[11], 11, 1, 22);
        let target_metadata = metadata(&[22]);
        let target_graph = empty_graph(&[22]);
        let files = [
            NamedObjectReferenceGraph {
                file_name: "root/main.assets",
                metadata: &source_metadata,
                graph: &source_graph,
            },
            NamedObjectReferenceGraph {
                file_name: "SHARED.ASSETS",
                metadata: &target_metadata,
                graph: &target_graph,
            },
        ];

        let graph = MultiFileReferenceGraph::build(&files).unwrap();
        assert_eq!(graph.file_count(), 2);
        assert_eq!(graph.normalized_file_names()[1], "shared.assets");
        assert_eq!(
            source_graph
                .references_from_object(0)
                .map(|reference| reference.field_path.as_str())
                .collect::<Vec<_>>(),
            ["m_Target"]
        );
        assert_eq!(source_graph.references_from_object(1).count(), 0);
        assert_eq!(
            graph
                .candidate_file_indices("library/Shared.Assets")
                .collect::<Vec<_>>(),
            [1]
        );
        assert_eq!(
            graph.references(),
            [MultiFileObjectReference {
                source_file_index: 0,
                source_object_index: 0,
                source_path_id: 11,
                field_path: "m_Target",
                file_id: 1,
                path_id: 22,
                resolution: MultiFilePPtrResolution::Object {
                    target_file_index: 1,
                    target_object_index: 0,
                },
            }]
        );
        assert!(graph.is_complete());
    }

    #[test]
    fn exposes_file_scoped_object_identity_and_incoming_outgoing_edges() {
        let source_metadata = metadata(&[11, 12, 13]);
        let mut source_graph = empty_graph(&[11, 12, 13]);
        source_graph.references = vec![
            SerializedObjectReference {
                source_object_index: 0,
                source_path_id: 11,
                field_path: "m_External".to_owned(),
                file_id: 1,
                path_id: 22,
            },
            SerializedObjectReference {
                source_object_index: 1,
                source_path_id: 12,
                field_path: "m_Local".to_owned(),
                file_id: 0,
                path_id: 13,
            },
            SerializedObjectReference {
                source_object_index: 2,
                source_path_id: 13,
                field_path: "m_External".to_owned(),
                file_id: 1,
                path_id: 22,
            },
        ];
        let target_metadata = metadata(&[22]);
        let target_graph = empty_graph(&[22]);
        let files = [
            NamedObjectReferenceGraph {
                file_name: "root/main.assets",
                metadata: &source_metadata,
                graph: &source_graph,
            },
            NamedObjectReferenceGraph {
                file_name: "SHARED.ASSETS",
                metadata: &target_metadata,
                graph: &target_graph,
            },
        ];

        let graph = MultiFileReferenceGraph::build(&files).unwrap();
        assert_eq!(
            graph.object_identity(1, 0),
            Some(MultiFileObjectIdentity {
                file_index: 1,
                normalized_file_name: "shared.assets",
                object_index: 0,
                path_id: 22,
            })
        );
        assert_eq!(graph.object_identity(1, 1), None);
        assert_eq!(
            graph
                .references_from_object(0, 1)
                .map(|reference| reference.field_path)
                .collect::<Vec<_>>(),
            ["m_Local"]
        );
        assert_eq!(
            graph
                .references_to_object(1, 0)
                .map(|reference| (reference.source_file_index, reference.source_object_index))
                .collect::<Vec<_>>(),
            [(0, 0), (0, 2)]
        );
        assert_eq!(graph.references_from_object(4, 0).count(), 0);
    }

    #[test]
    fn missing_dependencies_report_each_unresolved_identifier_once() {
        let source_metadata = metadata(&[11]);
        let mut source_graph = graph_with_reference(&[11], 11, 1, 22);
        // A second PPtr shares external slot 0; a third uses slot 1.
        source_graph.references.push(SerializedObjectReference {
            source_object_index: 0,
            source_path_id: 11,
            field_path: "m_Other".to_owned(),
            file_id: 1,
            path_id: 23,
        });
        source_graph.references.push(SerializedObjectReference {
            source_object_index: 0,
            source_path_id: 11,
            field_path: "m_Third".to_owned(),
            file_id: 2,
            path_id: 24,
        });
        let unrelated_metadata = metadata(&[22]);
        let unrelated_graph = empty_graph(&[22]);
        let files = [
            NamedObjectReferenceGraph {
                file_name: "main.assets",
                metadata: &source_metadata,
                graph: &source_graph,
            },
            NamedObjectReferenceGraph {
                file_name: "different.assets",
                metadata: &unrelated_metadata,
                graph: &unrelated_graph,
            },
        ];
        let graph = MultiFileReferenceGraph::build(&files).unwrap();
        // Slot 0 is shared by two references but reported once, in identifier order.
        assert_eq!(graph.missing_dependency_indexes(0), [0, 1]);
        assert_eq!(
            graph.dependency_identifier(0, 0).unwrap().fixed_path_name(),
            "shared.assets"
        );
        assert_eq!(
            graph.dependency_identifier(0, 1).unwrap().fixed_path_name(),
            "scene.assets"
        );
        // Files without missing dependencies report nothing; neither do unknown files.
        assert_eq!(graph.missing_dependency_indexes(1), Vec::<usize>::new());
        assert_eq!(graph.missing_dependency_indexes(7), Vec::<usize>::new());
    }

    #[test]
    fn missing_and_ambiguous_external_files_remain_explicit() {
        let source_metadata = metadata(&[11]);
        let source_graph = graph_with_reference(&[11], 11, 1, 22);
        let unrelated_metadata = metadata(&[22]);
        let unrelated_graph = empty_graph(&[22]);
        let missing_files = [
            NamedObjectReferenceGraph {
                file_name: "main.assets",
                metadata: &source_metadata,
                graph: &source_graph,
            },
            NamedObjectReferenceGraph {
                file_name: "different.assets",
                metadata: &unrelated_metadata,
                graph: &unrelated_graph,
            },
        ];
        let missing = MultiFileReferenceGraph::build(&missing_files).unwrap();
        assert_eq!(
            missing.references()[0].resolution,
            MultiFilePPtrResolution::MissingDependency {
                external_index: 0,
                path_id: 22,
            }
        );
        assert_eq!(
            missing
                .dependency_identifier(0, 0)
                .unwrap()
                .fixed_path_name(),
            "shared.assets"
        );
        assert!(!missing.is_complete());

        let duplicate_files = [
            NamedObjectReferenceGraph {
                file_name: "main.assets",
                metadata: &source_metadata,
                graph: &source_graph,
            },
            NamedObjectReferenceGraph {
                file_name: "shared.assets",
                metadata: &unrelated_metadata,
                graph: &unrelated_graph,
            },
            NamedObjectReferenceGraph {
                file_name: "LIBRARY/SHARED.ASSETS",
                metadata: &unrelated_metadata,
                graph: &unrelated_graph,
            },
        ];
        let ambiguous = MultiFileReferenceGraph::build(&duplicate_files).unwrap();
        assert_eq!(
            ambiguous.references()[0].resolution,
            MultiFilePPtrResolution::AmbiguousDependency {
                external_index: 0,
                path_id: 22,
                candidate_count: 2,
            }
        );
        assert_eq!(
            ambiguous
                .candidate_file_indices("shared.assets")
                .collect::<Vec<_>>(),
            [1, 2]
        );
        assert!(!ambiguous.is_complete());
    }

    fn metadata_with_external(path_ids: &[i64], external_path: &str) -> SerializedFileMetadata {
        let mut source_metadata = metadata(path_ids);
        source_metadata.externals = vec![external(external_path)];
        source_metadata
    }

    fn named_file<'a>(
        file_name: &'a str,
        metadata: &'a SerializedFileMetadata,
        graph: &'a ObjectReferenceGraph,
    ) -> NamedObjectReferenceGraph<'a> {
        NamedObjectReferenceGraph {
            file_name,
            metadata,
            graph,
        }
    }

    #[test]
    fn engine_resource_alias_fallback_matches_the_bundle_resolver() {
        // The alias table itself mirrors `Bundle.ResolveCollection` exactly.
        assert_eq!(
            super::engine_resource_alias("unity default resources"),
            Some("unity_default_resources")
        );
        assert_eq!(
            super::engine_resource_alias("unity_default_resources"),
            Some("unity default resources")
        );
        assert_eq!(
            super::engine_resource_alias("unity builtin extra"),
            Some("unity_builtin_extra")
        );
        assert_eq!(
            super::engine_resource_alias("unity_builtin_extra"),
            Some("unity builtin extra")
        );
        assert_eq!(super::engine_resource_alias("shared.assets"), None);

        // `library/unity default resources` resolves into a loaded
        // `unity_default_resources` file the exact-name join would miss.
        let source_metadata = metadata_with_external(&[11], "library/unity default resources");
        let source_graph = graph_with_reference(&[11], 11, 1, 22);
        let target_metadata = metadata(&[22]);
        let target_graph = empty_graph(&[22]);
        let files = [
            named_file("main.assets", &source_metadata, &source_graph),
            named_file("unity_default_resources", &target_metadata, &target_graph),
        ];
        let graph = MultiFileReferenceGraph::build(&files).unwrap();
        assert_eq!(
            graph.references()[0].resolution,
            MultiFilePPtrResolution::Object {
                target_file_index: 1,
                target_object_index: 0,
            }
        );
        assert!(graph.is_complete());

        // The reverse spelling and the builtin-extra pair behave the same way.
        let source_metadata = metadata_with_external(&[11], "resources/unity builtin extra");
        let source_graph = graph_with_reference(&[11], 11, 1, 22);
        let files = [
            named_file("main.assets", &source_metadata, &source_graph),
            named_file(
                "Library/unity_builtin_extra",
                &target_metadata,
                &target_graph,
            ),
        ];
        let graph = MultiFileReferenceGraph::build(&files).unwrap();
        assert_eq!(
            graph.references()[0].resolution,
            MultiFilePPtrResolution::Object {
                target_file_index: 1,
                target_object_index: 0,
            }
        );

        // An exact-name hit wins over the alias: the alias file is not consulted.
        let exact_metadata = metadata(&[99]);
        let exact_graph = empty_graph(&[99]);
        let source_metadata = metadata_with_external(&[11], "library/unity default resources");
        let source_graph = graph_with_reference(&[11], 11, 1, 22);
        let files = [
            named_file("main.assets", &source_metadata, &source_graph),
            named_file("unity default resources", &exact_metadata, &exact_graph),
            named_file("unity_default_resources", &target_metadata, &target_graph),
        ];
        let graph = MultiFileReferenceGraph::build(&files).unwrap();
        assert_eq!(
            graph.references()[0].resolution,
            MultiFilePPtrResolution::MissingExternalObject {
                target_file_index: 1,
                path_id: 22,
            }
        );

        // Duplicate alias spellings stay ambiguous instead of merging.
        let files = [
            named_file("main.assets", &source_metadata, &source_graph),
            named_file("unity_default_resources", &target_metadata, &target_graph),
            named_file(
                "ARCHIVE:/UNITY_DEFAULT_RESOURCES",
                &target_metadata,
                &target_graph,
            ),
        ];
        let graph = MultiFileReferenceGraph::build(&files).unwrap();
        assert_eq!(
            graph.references()[0].resolution,
            MultiFilePPtrResolution::AmbiguousDependency {
                external_index: 0,
                path_id: 22,
                candidate_count: 2,
            }
        );
        assert!(!graph.is_complete());
    }

    #[test]
    fn cross_file_graph_distinguishes_missing_target_objects_and_limits() {
        let source_metadata = metadata(&[11]);
        let source_graph = graph_with_reference(&[11], 11, 1, 404);
        let target_metadata = metadata(&[22]);
        let target_graph = empty_graph(&[22]);
        let files = [
            NamedObjectReferenceGraph {
                file_name: "main.assets",
                metadata: &source_metadata,
                graph: &source_graph,
            },
            NamedObjectReferenceGraph {
                file_name: "shared.assets",
                metadata: &target_metadata,
                graph: &target_graph,
            },
        ];

        let graph = MultiFileReferenceGraph::build(&files).unwrap();
        assert_eq!(
            graph.references()[0].resolution,
            MultiFilePPtrResolution::MissingExternalObject {
                target_file_index: 1,
                path_id: 404,
            }
        );
        assert!(matches!(
            MultiFileReferenceGraph::build_with_limits(
                &files,
                MultiFileReferenceGraphLimits {
                    max_files: 1,
                    ..MultiFileReferenceGraphLimits::default()
                }
            ),
            Err(MultiFileReferenceGraphBuildError::FileCountLimitExceeded {
                file_count: 2,
                limit: 1
            })
        ));
        assert!(matches!(
            MultiFileReferenceGraph::build_with_limits(
                &files,
                MultiFileReferenceGraphLimits {
                    max_references: 0,
                    ..MultiFileReferenceGraphLimits::default()
                }
            ),
            Err(
                MultiFileReferenceGraphBuildError::ReferenceCountLimitExceeded {
                    reference_count: 1,
                    limit: 0
                }
            )
        ));
        assert!(matches!(
            MultiFileReferenceGraph::build_with_limits(
                &files,
                MultiFileReferenceGraphLimits {
                    max_total_file_name_bytes: 10,
                    ..MultiFileReferenceGraphLimits::default()
                }
            ),
            Err(
                MultiFileReferenceGraphBuildError::TotalFileNameBytesLimitExceeded {
                    byte_count: 11,
                    limit: 10
                }
            )
        ));
    }

    #[test]
    fn walks_nested_structures_and_arrays_with_stable_field_paths() {
        let pointer = |path_id| SerializedValue::PPtr {
            file_id: 0,
            path_id,
        };
        let value = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "m_Items".to_owned(),
            value: SerializedValue::Array(vec![SerializedValue::Struct(vec![
                SerializedFieldValue {
                    name: "m_Target".to_owned(),
                    value: pointer(42),
                },
            ])]),
        }]);
        let mut references = Vec::new();
        let mut issues = Vec::new();
        collect_references(
            &value,
            &mut String::new(),
            3,
            88,
            &mut references,
            &mut issues,
            ReferenceGraphLimits::default(),
        )
        .unwrap();
        assert_eq!(references.len(), 1);
        assert_eq!(references[0].source_object_index, 3);
        assert_eq!(references[0].source_path_id, 88);
        assert_eq!(references[0].field_path, "m_Items[0].m_Target");
        assert_eq!(references[0].file_id, 0);
        assert_eq!(references[0].path_id, 42);
        assert!(issues.is_empty());
    }

    #[test]
    fn edge_and_field_path_limits_are_explicit() {
        let pointer = SerializedValue::PPtr {
            file_id: 0,
            path_id: 42,
        };
        let mut references = Vec::new();
        let mut issues = Vec::new();
        assert!(matches!(
            collect_references(
                &pointer,
                &mut String::new(),
                0,
                1,
                &mut references,
                &mut issues,
                ReferenceGraphLimits {
                    max_edges: 0,
                    ..ReferenceGraphLimits::default()
                }
            ),
            Err(WalkIssue::EdgeLimitExceeded { .. })
        ));

        let named_pointer = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "target".to_owned(),
            value: pointer,
        }]);
        assert!(matches!(
            collect_references(
                &named_pointer,
                &mut String::new(),
                0,
                1,
                &mut references,
                &mut issues,
                ReferenceGraphLimits {
                    max_field_path_bytes: 3,
                    ..ReferenceGraphLimits::default()
                }
            ),
            Err(WalkIssue::FieldPathLimitExceeded { .. })
        ));
    }

    #[test]
    fn raw_type_tree_leaves_are_reported_instead_of_claimed_as_scanned() {
        let value = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "m_Unknown".to_owned(),
            value: SerializedValue::Raw {
                type_name: "UnknownPointerShape".to_owned(),
                bytes: vec![1, 2, 3, 4],
            },
        }]);
        let mut references = Vec::new();
        let mut issues = Vec::new();
        collect_references(
            &value,
            &mut String::new(),
            2,
            77,
            &mut references,
            &mut issues,
            ReferenceGraphLimits::default(),
        )
        .unwrap();
        assert!(references.is_empty());
        assert_eq!(
            issues,
            [super::ReferenceGraphIssue::UnparsedRawValue {
                object_index: 2,
                path_id: 77,
                field_path: "m_Unknown".to_owned(),
                byte_count: 4,
            }]
        );
    }

    /// File-level PPtr/external differential against the reference dump
    /// committed in `pptr_probe_fixture.txt` (probe `Temp/opencode/pptrprobe`,
    /// isolated reference bins, SDK 10.0.401). File A carries two objects
    /// with external, local, local-back, and null PPtrs plus one external
    /// table entry; file B is the external target. The per-file graphs must
    /// expose the same non-null edges, the external table must match on GUID
    /// words/type/path, and the multi-file join must resolve every edge
    /// (null PPtrs are field values, not graph edges, on both sides).
    #[test]
    fn pptr_values_external_table_and_join_reproduce_the_reference_dump() {
        use crate::serialized_file::file::SerializedFile;

        fn decode_hex(hex: &str) -> Vec<u8> {
            assert!(hex.len().is_multiple_of(2), "odd hex length");
            hex.as_bytes()
                .chunks(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect()
        }

        let fixture = include_str!("pptr_probe_fixture.txt");
        let mut file_a = Vec::new();
        let mut file_b = Vec::new();
        let mut expected_edges: Vec<(i64, String, i32, i64)> = Vec::new();
        let mut expected_guid = String::new();
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(hex) = line.strip_prefix("SFAHEX|") {
                file_a = decode_hex(hex);
            } else if let Some(hex) = line.strip_prefix("SFBHEX|") {
                file_b = decode_hex(hex);
            } else if let Some(row) = line.strip_prefix("PPTR|") {
                let parts: Vec<&str> = row.split('|').collect();
                assert_eq!(parts.len(), 5, "bad PPTR {row}");
                let file_id: i32 = parts[3].parse().unwrap();
                let path_id: i64 = parts[4].parse().unwrap();
                expected_edges.push((
                    parts[1].parse().unwrap(),
                    parts[2].to_owned(),
                    file_id,
                    path_id,
                ));
            } else if let Some(row) = line.strip_prefix("PEXT|") {
                let parts: Vec<&str> = row.split('|').collect();
                assert_eq!(parts.len(), 5, "bad PEXT {row}");
                expected_guid = parts[2].to_owned();
                assert_eq!(parts[3], "Internal");
                assert_eq!(parts[4], "other.assets");
            } else {
                panic!("unknown fixture row {line}");
            }
        }
        assert!(!file_a.is_empty() && !file_b.is_empty());

        let parsed_a = SerializedFile::parse(&file_a).unwrap();
        let parsed_b = SerializedFile::parse(&file_b).unwrap();
        assert_eq!(parsed_a.metadata.objects.len(), 2);
        assert_eq!(parsed_b.metadata.objects.len(), 1);
        assert_eq!(parsed_b.metadata.objects[0].path_id, 100);

        let external = &parsed_a.metadata.externals;
        assert_eq!(external.len(), 1);
        assert_eq!(external[0].file_type, Some(0));
        assert_eq!(external[0].path_name_origin, "other.assets");
        let guid_words = external[0].guid.unwrap();
        let mut guid_bytes = Vec::new();
        for word in guid_words {
            guid_bytes.extend_from_slice(&word.to_le_bytes());
        }
        let guid_hex: String = guid_bytes.iter().map(|b| format!("{b:02X}")).collect();
        assert_eq!(guid_hex, expected_guid);

        let graph_a = ObjectReferenceGraph::build(&parsed_a).unwrap();
        let mut edges: Vec<(i64, String, i32, i64)> = graph_a
            .references()
            .iter()
            .map(|reference| {
                (
                    reference.source_path_id,
                    reference.field_path.clone(),
                    reference.file_id,
                    reference.path_id,
                )
            })
            .collect();
        edges.sort();
        expected_edges.sort();
        assert_eq!(edges, expected_edges);
        let graph_b = ObjectReferenceGraph::build(&parsed_b).unwrap();
        assert!(graph_b.references().is_empty());

        let files = [
            NamedObjectReferenceGraph {
                file_name: "main.assets",
                metadata: &parsed_a.metadata,
                graph: &graph_a,
            },
            NamedObjectReferenceGraph {
                file_name: "other.assets",
                metadata: &parsed_b.metadata,
                graph: &graph_b,
            },
        ];
        let joined = MultiFileReferenceGraph::build(&files).unwrap();
        assert_eq!(joined.file_count(), 2);
        let mut resolutions: Vec<(i64, String, i32, i64, usize, usize)> = Vec::new();
        let mut nulls = 0;
        for reference in joined.references() {
            match reference.resolution {
                MultiFilePPtrResolution::Object {
                    target_file_index,
                    target_object_index,
                } => resolutions.push((
                    reference.source_path_id,
                    reference.field_path.to_owned(),
                    reference.file_id,
                    reference.path_id,
                    target_file_index,
                    target_object_index,
                )),
                MultiFilePPtrResolution::Null => {
                    assert_eq!(reference.file_id, 0);
                    assert_eq!(reference.path_id, 0);
                    nulls += 1;
                }
                ref other => panic!("unresolved {other:?}"),
            }
        }
        assert_eq!(nulls, 1);
        resolutions.sort();
        assert_eq!(
            resolutions,
            [
                (8001, "m_Friend".to_owned(), 0, 8002, 0, 1),
                (8001, "m_Target".to_owned(), 1, 100, 1, 0),
                (8002, "m_Friend".to_owned(), 0, 8001, 0, 0),
            ]
        );
        assert!(joined.is_complete());
    }
}
