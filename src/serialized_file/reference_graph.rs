//! Per-SerializedFile object identity indexing and TypeTree-backed PPtr collection.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

use super::file::{SerializedFile, SerializedFileError};
use super::metadata::SerializedFileMetadata;
use super::object::{ObjectDecodeLimits, SerializedValue};
use super::references::PPtrResolution;

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

        let Some(external_index) = file_id
            .checked_sub(1)
            .and_then(|index| usize::try_from(index).ok())
        else {
            return PPtrResolution::InvalidFileId {
                file_id,
                external_count: metadata.externals.len(),
            };
        };
        match metadata.externals.get(external_index) {
            Some(identifier) => PPtrResolution::ExternalDependency {
                file_id,
                external_index,
                path_id,
                identifier,
            },
            None => PPtrResolution::InvalidFileId {
                file_id,
                external_count: metadata.externals.len(),
            },
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
    use super::{collect_references, ObjectReferenceGraph, ReferenceGraphLimits, WalkIssue};
    use crate::io::binary::Endian;
    use crate::serialized_file::metadata::{FileIdentifier, ObjectInfo, SerializedFileMetadata};
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
    fn indexes_duplicate_ids_and_classifies_local_external_and_null_edges() {
        let metadata = metadata(&[101, 202, 101]);
        let graph = empty_graph(&[101, 202, 101]);
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
}
