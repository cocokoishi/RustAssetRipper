use crate::serialized_file::file::{SerializedFile, SerializedFileError};
use crate::serialized_file::metadata::SerializedFileMetadata;
use crate::serialized_file::object::{ObjectDecodeError, ObjectDecodeLimits, ParsedObjectPayload};
use crate::serialized_file::references::PPtrResolution;

use super::external_table_index;
use super::walkers::{
    collect_managed_reference_graph, collect_references, ReferenceGraphBuildError, WalkIssue,
};

#[derive(Clone, Copy)]
struct SourceTypeTreeFallback<'a> {
    collection_name: &'a str,
    file_path: &'a str,
    layouts: &'a dyn crate::processing::asset_factory::ClassLayoutSource,
}

/// Bounds for constructing a per-file reference scan.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReferenceGraphLimits {
    pub object_decode: ObjectDecodeLimits,
    pub max_objects: usize,
    pub max_managed_registry_scopes: usize,
    pub max_edges: usize,
    pub max_issues: usize,
    pub max_field_path_bytes: usize,
}

impl Default for ReferenceGraphLimits {
    fn default() -> Self {
        Self {
            object_decode: ObjectDecodeLimits::default(),
            max_objects: 4_000_000,
            max_managed_registry_scopes: 1_000_000,
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

/// One dynamically typed managed object stored in a SerializedFile object's registry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SerializedManagedObject {
    pub source_object_index: usize,
    pub source_path_id: i64,
    /// Stable preorder scope assigned to the struct that owns this registry.
    pub registry_scope_id: Option<usize>,
    /// TypeTree path of the owning struct's registry field.
    pub registry_scope_path: Option<String>,
    /// Path to this specific registry field (distinct if a struct has more than one).
    pub registry_field_path: String,
    /// Zero-based order in this owning object's registry.
    pub registry_entry_index: usize,
    pub reference_id: i64,
    pub class_name: String,
    pub namespace_name: String,
    pub assembly_name: String,
    /// False for registry entries whose source triple carries no decoded payload.
    pub has_content: bool,
}

/// Resolution of a managed-reference ID within its owning serialized object.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SerializedManagedReferenceResolution {
    Null,
    Unknown,
    Object {
        registry_scope_id: usize,
        /// Index into `ObjectReferenceGraph::managed_objects()` for this file object.
        managed_object_index: usize,
        registry_entry_index: usize,
    },
    Missing {
        reference_id: i64,
    },
    Ambiguous {
        reference_id: i64,
        match_count: usize,
    },
}

/// One managed-reference ID edge within an owning serialized object.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SerializedManagedReference {
    pub source_object_index: usize,
    pub source_path_id: i64,
    pub field_path: String,
    /// The lexical registry scope selected by the nearest owning struct.
    pub registry_scope_id: Option<usize>,
    pub reference_id: i64,
    pub indexed: bool,
    pub resolution: SerializedManagedReferenceResolution,
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
    ManagedObjectLimitExceeded {
        object_index: usize,
        path_id: i64,
        field_path: String,
        limit: usize,
    },
    ManagedReferenceLimitExceeded {
        object_index: usize,
        path_id: i64,
        field_path: String,
        limit: usize,
    },
    ManagedRegistryScopeLimitExceeded {
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
    pub(super) object_count: usize,
    pub(super) attempted_objects: usize,
    /// Sorted by PathID, then source object index, so duplicate identities remain visible.
    pub(super) path_id_index: Vec<(i64, usize)>,
    pub(super) references: Vec<SerializedObjectReference>,
    pub(super) managed_objects: Vec<SerializedManagedObject>,
    pub(super) managed_references: Vec<SerializedManagedReference>,
    pub(super) issues: Vec<ReferenceGraphIssue>,
}

impl ObjectReferenceGraph {
    pub fn build(file: &SerializedFile<'_>) -> Result<Self, ReferenceGraphBuildError> {
        Self::build_with_limits(file, ReferenceGraphLimits::default())
    }

    pub fn build_with_limits(
        file: &SerializedFile<'_>,
        limits: ReferenceGraphLimits,
    ) -> Result<Self, ReferenceGraphBuildError> {
        Self::build_internal(file, limits, None, None)
    }

    /// Build a reference graph with source TPK recovery for generated classes and
    /// fallback TypeTreeObjects when a file has no usable TypeTree. Unsupported class
    /// or version cases retain the original decode issue and keep the scan incomplete.
    pub fn build_with_source_type_tree_fallback(
        file: &SerializedFile<'_>,
        limits: ReferenceGraphLimits,
        collection_name: &str,
        file_path: &str,
        layouts: &dyn crate::processing::asset_factory::ClassLayoutSource,
    ) -> Result<Self, ReferenceGraphBuildError> {
        Self::build_internal(
            file,
            limits,
            Some(SourceTypeTreeFallback {
                collection_name,
                file_path,
                layouts,
            }),
            None,
        )
    }

    /// Rebuild a source-fallback scan while substituting caller-decoded payloads for selected
    /// object indexes. Overrides must be sorted by object index and retain the corresponding
    /// object PathID. This is used after assembly-backed MonoBehaviour recovery so the graph
    /// scans the recovered script fields rather than preserving the old unloaded-tail issue.
    pub(crate) fn build_with_source_type_tree_fallback_and_payload_overrides(
        file: &SerializedFile<'_>,
        limits: ReferenceGraphLimits,
        collection_name: &str,
        file_path: &str,
        layouts: &dyn crate::processing::asset_factory::ClassLayoutSource,
        payload_overrides: &[(usize, ParsedObjectPayload)],
    ) -> Result<Self, ReferenceGraphBuildError> {
        Self::build_internal(
            file,
            limits,
            Some(SourceTypeTreeFallback {
                collection_name,
                file_path,
                layouts,
            }),
            Some(payload_overrides),
        )
    }

    /// Build a graph from object payloads supplied one at a time by a caller-owned input source.
    /// This preserves the graph's bounded working set for file-backed SerializedFiles.
    pub(crate) fn build_with_payload_decoder(
        metadata: &SerializedFileMetadata,
        limits: ReferenceGraphLimits,
        decode: impl FnMut(usize) -> Result<DecodedReferenceObject, SerializedFileError>,
    ) -> Result<Self, ReferenceGraphBuildError> {
        Self::build_with_decoder_internal(metadata, limits, None, decode)
    }

    fn build_internal(
        file: &SerializedFile<'_>,
        limits: ReferenceGraphLimits,
        fallback: Option<SourceTypeTreeFallback<'_>>,
        payload_overrides: Option<&[(usize, ParsedObjectPayload)]>,
    ) -> Result<Self, ReferenceGraphBuildError> {
        Self::build_with_decoder_internal(
            &file.metadata,
            limits,
            payload_overrides,
            |object_index| {
                decode_object_payload_with_generated_fallback(
                    file,
                    object_index,
                    &limits.object_decode,
                    fallback,
                )
            },
        )
    }

    fn build_with_decoder_internal(
        metadata: &SerializedFileMetadata,
        limits: ReferenceGraphLimits,
        payload_overrides: Option<&[(usize, ParsedObjectPayload)]>,
        mut decode: impl FnMut(usize) -> Result<DecodedReferenceObject, SerializedFileError>,
    ) -> Result<Self, ReferenceGraphBuildError> {
        let object_count = metadata.objects.len();
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
            metadata
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
            managed_objects: Vec::new(),
            managed_references: Vec::new(),
            issues: Vec::new(),
        };

        for (object_index, object) in metadata.objects.iter().enumerate() {
            graph.attempted_objects += 1;
            let override_payload = payload_overrides.and_then(|overrides| {
                overrides
                    .binary_search_by_key(&object_index, |(index, _)| *index)
                    .ok()
                    .map(|index| &overrides[index].1)
            });
            let decoded_result = if let Some(payload) = override_payload {
                if payload.object_index != object_index || payload.path_id != object.path_id {
                    return Err(ReferenceGraphBuildError::PayloadOverrideIdentityMismatch {
                        object_index,
                        expected_path_id: object.path_id,
                        actual_object_index: payload.object_index,
                        actual_path_id: payload.path_id,
                    });
                }
                Ok(DecodedReferenceObject {
                    payload: payload.clone(),
                    unparsed_tail_bytes: payload.trailing_bytes.len(),
                    incomplete_outcome: None,
                })
            } else {
                decode(object_index)
            };
            let decoded = match decoded_result {
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
            if decoded.unparsed_tail_bytes != 0 {
                graph.push_issue(
                    ReferenceGraphIssue::UnparsedTrailingPayload {
                        object_index,
                        path_id: object.path_id,
                        byte_count: decoded.unparsed_tail_bytes,
                    },
                    limits.max_issues,
                )?;
            }
            if let Some(error) = decoded.incomplete_outcome {
                graph.push_issue(
                    ReferenceGraphIssue::ObjectDecodeFailed {
                        object_index,
                        path_id: object.path_id,
                        error,
                    },
                    limits.max_issues,
                )?;
            }

            let mut field_path = String::new();
            match collect_references(
                &decoded.payload.root,
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
                Err(WalkIssue::ManagedObjectLimitExceeded { field_path }) => {
                    graph.push_issue(
                        ReferenceGraphIssue::ManagedObjectLimitExceeded {
                            object_index,
                            path_id: object.path_id,
                            field_path,
                            limit: limits.max_objects,
                        },
                        limits.max_issues,
                    )?;
                }
                Err(WalkIssue::ManagedReferenceLimitExceeded { field_path }) => {
                    graph.push_issue(
                        ReferenceGraphIssue::ManagedReferenceLimitExceeded {
                            object_index,
                            path_id: object.path_id,
                            field_path,
                            limit: limits.max_edges,
                        },
                        limits.max_issues,
                    )?;
                }
                Err(WalkIssue::ManagedRegistryScopeLimitExceeded { field_path }) => {
                    graph.push_issue(
                        ReferenceGraphIssue::ManagedRegistryScopeLimitExceeded {
                            object_index,
                            path_id: object.path_id,
                            field_path,
                            limit: limits.max_managed_registry_scopes,
                        },
                        limits.max_issues,
                    )?;
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

            let mut managed_limits = limits;
            managed_limits.max_objects = limits
                .max_objects
                .saturating_sub(graph.managed_objects.len());
            managed_limits.max_edges = limits
                .max_edges
                .saturating_sub(graph.managed_references.len());
            let mut managed_field_path = String::new();
            match collect_managed_reference_graph(
                &decoded.payload.root,
                &mut managed_field_path,
                object_index,
                object.path_id,
                managed_limits,
            ) {
                Ok(local) => {
                    graph
                        .managed_objects
                        .try_reserve_exact(local.objects.len())
                        .map_err(|_| {
                            ReferenceGraphBuildError::allocation(
                                "managed reference objects",
                                local.objects.len(),
                            )
                        })?;
                    graph
                        .managed_references
                        .try_reserve_exact(local.references.len())
                        .map_err(|_| {
                            ReferenceGraphBuildError::allocation(
                                "managed reference edges",
                                local.references.len(),
                            )
                        })?;
                    graph.managed_objects.extend(local.objects);
                    graph.managed_references.extend(local.references);
                }
                Err(WalkIssue::FieldPathLimitExceeded {
                    parent_field_path,
                    component_bytes,
                }) => graph.push_issue(
                    ReferenceGraphIssue::FieldPathLimitExceeded {
                        object_index,
                        path_id: object.path_id,
                        parent_field_path,
                        component_bytes,
                        limit: limits.max_field_path_bytes,
                    },
                    limits.max_issues,
                )?,
                Err(WalkIssue::ManagedObjectLimitExceeded { field_path }) => {
                    graph.push_issue(
                        ReferenceGraphIssue::ManagedObjectLimitExceeded {
                            object_index,
                            path_id: object.path_id,
                            field_path,
                            limit: limits.max_objects,
                        },
                        limits.max_issues,
                    )?;
                }
                Err(WalkIssue::ManagedReferenceLimitExceeded { field_path }) => {
                    graph.push_issue(
                        ReferenceGraphIssue::ManagedReferenceLimitExceeded {
                            object_index,
                            path_id: object.path_id,
                            field_path,
                            limit: limits.max_edges,
                        },
                        limits.max_issues,
                    )?;
                }
                Err(WalkIssue::ManagedRegistryScopeLimitExceeded { field_path }) => {
                    graph.push_issue(
                        ReferenceGraphIssue::ManagedRegistryScopeLimitExceeded {
                            object_index,
                            path_id: object.path_id,
                            field_path,
                            limit: limits.max_managed_registry_scopes,
                        },
                        limits.max_issues,
                    )?;
                }
                Err(WalkIssue::AllocationFailed {
                    resource,
                    requested,
                }) => {
                    return Err(ReferenceGraphBuildError::allocation(resource, requested));
                }
                Err(WalkIssue::EdgeLimitExceeded { field_path }) => graph.push_issue(
                    ReferenceGraphIssue::ManagedReferenceLimitExceeded {
                        object_index,
                        path_id: object.path_id,
                        field_path,
                        limit: limits.max_edges,
                    },
                    limits.max_issues,
                )?,
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

    pub fn managed_objects(&self) -> &[SerializedManagedObject] {
        &self.managed_objects
    }

    pub fn managed_references(&self) -> &[SerializedManagedReference] {
        &self.managed_references
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

    /// Returns managed objects registered by one source object, in registry order.
    pub fn managed_objects_from_object(
        &self,
        object_index: usize,
    ) -> impl Iterator<Item = &SerializedManagedObject> {
        let first = self
            .managed_objects
            .partition_point(|object| object.source_object_index < object_index);
        let end = self
            .managed_objects
            .partition_point(|object| object.source_object_index <= object_index);
        self.managed_objects[first..end].iter()
    }

    /// Returns managed-reference ID edges for one source object in field traversal order.
    pub fn managed_references_from_object(
        &self,
        object_index: usize,
    ) -> impl Iterator<Item = &SerializedManagedReference> {
        let first = self
            .managed_references
            .partition_point(|reference| reference.source_object_index < object_index);
        let end = self
            .managed_references
            .partition_point(|reference| reference.source_object_index <= object_index);
        self.managed_references[first..end].iter()
    }

    /// True only when every object's TypeTree payload was decoded and every recognized PPtr was
    /// collected within the configured bounds.
    pub fn is_complete(&self) -> bool {
        self.path_id_index.len() == self.object_count
            && self.attempted_objects == self.object_count
            && self.issues.is_empty()
    }

    /// Whether the scan is complete except for payloads explicitly treated as opaque by the
    /// caller. Only decode/trailing/raw-value issues on those object indices may be excluded;
    /// safety-limit failures and reference-walk failures remain fatal. A `true` result does not
    /// claim that references inside an opaque payload were inspected.
    pub fn is_complete_except_opaque_object_payloads(
        &self,
        mut is_opaque: impl FnMut(usize) -> bool,
    ) -> bool {
        self.path_id_index.len() == self.object_count
            && self.attempted_objects == self.object_count
            && self.issues.iter().all(|issue| match issue {
                ReferenceGraphIssue::ObjectDecodeFailed { object_index, .. }
                | ReferenceGraphIssue::UnparsedTrailingPayload { object_index, .. }
                | ReferenceGraphIssue::UnparsedRawValue { object_index, .. } => {
                    is_opaque(*object_index)
                }
                ReferenceGraphIssue::FieldPathLimitExceeded { .. }
                | ReferenceGraphIssue::EdgeLimitExceeded { .. }
                | ReferenceGraphIssue::ManagedObjectLimitExceeded { .. }
                | ReferenceGraphIssue::ManagedReferenceLimitExceeded { .. }
                | ReferenceGraphIssue::ManagedRegistryScopeLimitExceeded { .. } => false,
            })
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
            }
            | ReferenceGraphIssue::ManagedObjectLimitExceeded {
                object_index,
                path_id,
                ..
            }
            | ReferenceGraphIssue::ManagedReferenceLimitExceeded {
                object_index,
                path_id,
                ..
            }
            | ReferenceGraphIssue::ManagedRegistryScopeLimitExceeded {
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

pub(crate) struct DecodedReferenceObject {
    pub(crate) payload: ParsedObjectPayload,
    pub(crate) unparsed_tail_bytes: usize,
    pub(crate) incomplete_outcome: Option<SerializedFileError>,
}

#[derive(Clone, Copy)]
pub(crate) struct StreamObjectDecodeContext<'a> {
    header: &'a crate::serialized_file::header::SerializedFileHeader,
    metadata: &'a SerializedFileMetadata,
    actual_file_size: usize,
    limits: &'a ObjectDecodeLimits,
    collection_name: &'a str,
    file_path: &'a str,
    layouts: &'a dyn crate::processing::asset_factory::ClassLayoutSource,
}

impl<'a> StreamObjectDecodeContext<'a> {
    pub(crate) fn new(
        header: &'a crate::serialized_file::header::SerializedFileHeader,
        metadata: &'a SerializedFileMetadata,
        actual_file_size: usize,
        limits: &'a ObjectDecodeLimits,
        collection_name: &'a str,
        file_path: &'a str,
        layouts: &'a dyn crate::processing::asset_factory::ClassLayoutSource,
    ) -> Self {
        Self {
            header,
            metadata,
            actual_file_size,
            limits,
            collection_name,
            file_path,
            layouts,
        }
    }
}

pub(crate) fn decode_object_payload_with_generated_fallback_from_data(
    context: &StreamObjectDecodeContext<'_>,
    object_index: usize,
    payload_bytes: &[u8],
) -> Result<DecodedReferenceObject, SerializedFileError> {
    let StreamObjectDecodeContext {
        header,
        metadata,
        actual_file_size,
        limits,
        collection_name,
        file_path,
        layouts,
    } = *context;
    let platform = reference_graph_platform_label(metadata.prefix.target_platform);
    let materialization =
        crate::processing::asset_factory::materialize_object_for_workspace_from_payload(
            header,
            metadata,
            &crate::processing::asset_factory::FactoryFileFacts {
                collection_name,
                file_path,
                platform: &platform,
            },
            layouts,
            object_index,
            payload_bytes,
            limits,
        )
        .map_err(|error| {
            let class_id = metadata
                .objects
                .get(object_index)
                .map(crate::processing::asset_factory::object_read_class_id)
                .unwrap_or_default();
            SerializedFileError::from_object(ObjectDecodeError::factory_object_outcome(
                object_index,
                class_id,
                "factory materialization failure".to_owned(),
                Some(error.to_string()),
            ))
        })?;
    let crate::processing::asset_factory::FactoryObjectMaterialization {
        state,
        payload: factory_payload,
    } = materialization;

    let has_file_type_tree = metadata
        .objects
        .get(object_index)
        .and_then(|object| object.resolved_type_index)
        .and_then(|type_index| metadata.types.get(type_index))
        .and_then(|serialized_type| serialized_type.type_tree.as_ref())
        .is_some_and(|tree| !tree.nodes.is_empty());
    let payload = if state.class_id == crate::processing::asset_factory::MONO_BEHAVIOUR_CLASS_ID
        && has_file_type_tree
    {
        metadata
            .decode_object_payload_from_data_with_limits(
                payload_bytes,
                header,
                object_index,
                actual_file_size,
                limits,
            )
            .map_err(SerializedFileError::from_object)?
    } else if state.kind.is_recoverable() {
        factory_payload.ok_or_else(|| factory_object_outcome_error(&state))?
    } else {
        return Err(factory_object_outcome_error(&state));
    };

    let unparsed_tail_bytes = match state.tail {
        Some(
            crate::processing::asset_factory::ObjectTailOutcome::Exact
            | crate::processing::asset_factory::ObjectTailOutcome::ZeroPadding
            | crate::processing::asset_factory::ObjectTailOutcome::ChineseTextureFields,
        ) => 0,
        Some(crate::processing::asset_factory::ObjectTailOutcome::IncorrectByteCount) | None => {
            payload.trailing_bytes.len()
        }
    };
    let is_fully_materialized = state.is_fully_materialized(Some(&payload));
    let incomplete_outcome = if !is_fully_materialized && unparsed_tail_bytes == 0 {
        Some(factory_object_outcome_error(&state))
    } else {
        None
    };
    Ok(DecodedReferenceObject {
        payload,
        unparsed_tail_bytes,
        incomplete_outcome,
    })
}

fn decode_object_payload_with_generated_fallback(
    file: &SerializedFile<'_>,
    object_index: usize,
    limits: &ObjectDecodeLimits,
    fallback: Option<SourceTypeTreeFallback<'_>>,
) -> Result<DecodedReferenceObject, SerializedFileError> {
    let Some(fallback) = fallback else {
        let payload = file.decode_object_payload_with_limits(object_index, limits)?;
        let unparsed_tail_bytes = payload.trailing_bytes.len();
        return Ok(DecodedReferenceObject {
            payload,
            unparsed_tail_bytes,
            incomplete_outcome: None,
        });
    };

    let platform = reference_graph_platform_label(file.metadata.prefix.target_platform);
    let materialization = crate::processing::asset_factory::materialize_object_for_workspace(
        file,
        &crate::processing::asset_factory::FactoryFileFacts {
            collection_name: fallback.collection_name,
            file_path: fallback.file_path,
            platform: &platform,
        },
        fallback.layouts,
        object_index,
        limits,
    )
    .map_err(|error| {
        let class_id = file
            .metadata
            .objects
            .get(object_index)
            .map(crate::processing::asset_factory::object_read_class_id)
            .unwrap_or_default();
        SerializedFileError::from_object(ObjectDecodeError::factory_object_outcome(
            object_index,
            class_id,
            "factory materialization failure".to_owned(),
            Some(error.to_string()),
        ))
    })?;
    let crate::processing::asset_factory::FactoryObjectMaterialization {
        state,
        payload: factory_payload,
    } = materialization;

    let has_file_type_tree = file
        .metadata
        .objects
        .get(object_index)
        .and_then(|object| object.resolved_type_index)
        .and_then(|type_index| file.metadata.types.get(type_index))
        .and_then(|serialized_type| serialized_type.type_tree.as_ref())
        .is_some_and(|tree| !tree.nodes.is_empty());
    let payload = if state.class_id == crate::processing::asset_factory::MONO_BEHAVIOUR_CLASS_ID
        && has_file_type_tree
    {
        file.decode_object_payload_with_limits(object_index, limits)?
    } else if state.kind.is_recoverable() {
        factory_payload.ok_or_else(|| factory_object_outcome_error(&state))?
    } else {
        return Err(factory_object_outcome_error(&state));
    };

    let unparsed_tail_bytes = match state.tail {
        Some(
            crate::processing::asset_factory::ObjectTailOutcome::Exact
            | crate::processing::asset_factory::ObjectTailOutcome::ZeroPadding
            | crate::processing::asset_factory::ObjectTailOutcome::ChineseTextureFields,
        ) => 0,
        Some(crate::processing::asset_factory::ObjectTailOutcome::IncorrectByteCount) | None => {
            payload.trailing_bytes.len()
        }
    };
    let is_fully_materialized = state.is_fully_materialized(Some(&payload));
    let incomplete_outcome = if !is_fully_materialized && unparsed_tail_bytes == 0 {
        Some(factory_object_outcome_error(&state))
    } else {
        None
    };
    Ok(DecodedReferenceObject {
        payload,
        unparsed_tail_bytes,
        incomplete_outcome,
    })
}

fn factory_object_outcome_error(
    state: &crate::processing::asset_factory::FactoryObjectState,
) -> SerializedFileError {
    let outcome = match &state.kind {
        crate::processing::asset_factory::FactoryAssetKind::Generated(name) => {
            format!("generated class {name} without a complete serialized structure")
        }
        crate::processing::asset_factory::FactoryAssetKind::TypeTreeObject => {
            "TypeTreeObject without a complete serialized structure".to_owned()
        }
        crate::processing::asset_factory::FactoryAssetKind::Unknown => {
            "UnknownObject raw fallback".to_owned()
        }
        crate::processing::asset_factory::FactoryAssetKind::Unreadable => {
            "UnreadableObject raw fallback".to_owned()
        }
    };
    SerializedFileError::from_object(ObjectDecodeError::factory_object_outcome(
        state.object_index,
        state.class_id,
        outcome,
        state.log_message.clone(),
    ))
}

fn reference_graph_platform_label(platform: Option<u32>) -> String {
    match platform {
        Some(crate::serialized_file::object::BUILD_TARGET_NO_TARGET) => "NoTarget".to_owned(),
        Some(19) => "StandaloneWin64Player".to_owned(),
        Some(value) => format!("BuildTarget({value})"),
        None => "Unknown".to_owned(),
    }
}

fn reference_issue_object_index(issue: &ReferenceGraphIssue) -> usize {
    match issue {
        ReferenceGraphIssue::ObjectDecodeFailed { object_index, .. }
        | ReferenceGraphIssue::UnparsedTrailingPayload { object_index, .. }
        | ReferenceGraphIssue::UnparsedRawValue { object_index, .. }
        | ReferenceGraphIssue::FieldPathLimitExceeded { object_index, .. }
        | ReferenceGraphIssue::EdgeLimitExceeded { object_index, .. }
        | ReferenceGraphIssue::ManagedObjectLimitExceeded { object_index, .. }
        | ReferenceGraphIssue::ManagedReferenceLimitExceeded { object_index, .. }
        | ReferenceGraphIssue::ManagedRegistryScopeLimitExceeded { object_index, .. } => {
            *object_index
        }
    }
}
