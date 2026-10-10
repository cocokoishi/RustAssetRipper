use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::serialized_file::object::SerializedValue;

use super::*;
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
    PayloadOverrideIdentityMismatch {
        object_index: usize,
        expected_path_id: i64,
        actual_object_index: usize,
        actual_path_id: i64,
    },
}

impl ReferenceGraphBuildError {
    pub(super) fn allocation(resource: &'static str, requested: usize) -> Self {
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
            Self::PayloadOverrideIdentityMismatch {
                object_index,
                expected_path_id,
                actual_object_index,
                actual_path_id,
            } => write!(
                f,
                "payload override for object {object_index} PathID {expected_path_id} instead names object {actual_object_index} PathID {actual_path_id}"
            ),
        }
    }
}

impl Error for ReferenceGraphBuildError {}

#[derive(Debug)]
pub(super) enum WalkIssue {
    FieldPathLimitExceeded {
        parent_field_path: String,
        component_bytes: usize,
    },
    EdgeLimitExceeded {
        field_path: String,
    },
    ManagedObjectLimitExceeded {
        field_path: String,
    },
    ManagedReferenceLimitExceeded {
        field_path: String,
    },
    ManagedRegistryScopeLimitExceeded {
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

#[derive(Default)]
pub(super) struct LocalManagedReferenceGraph {
    pub(super) objects: Vec<SerializedManagedObject>,
    pub(super) references: Vec<SerializedManagedReference>,
}

#[derive(Clone, Copy)]
struct ManagedReferenceScanContext {
    source_object_index: usize,
    source_path_id: i64,
    limits: ReferenceGraphLimits,
}

struct PendingManagedReference {
    field_path: String,
    registry_scope_id: Option<usize>,
    reference_id: i64,
    indexed: bool,
}

pub(super) fn collect_managed_reference_graph(
    root: &SerializedValue,
    field_path: &mut String,
    source_object_index: usize,
    source_path_id: i64,
    limits: ReferenceGraphLimits,
) -> Result<LocalManagedReferenceGraph, WalkIssue> {
    fn register_scope(
        field_path: &str,
        registry_field_name: Option<&str>,
        scope_paths: &mut Vec<String>,
        limits: ReferenceGraphLimits,
    ) -> Result<usize, WalkIssue> {
        let mut scope_path = copy_field_path(field_path)?;
        if let Some(registry_field_name) = registry_field_name {
            let separator = !scope_path.is_empty();
            append_path_component(
                &mut scope_path,
                registry_field_name,
                separator,
                limits.max_field_path_bytes,
            )?;
        }
        if scope_paths.len() >= limits.max_managed_registry_scopes {
            return Err(WalkIssue::ManagedRegistryScopeLimitExceeded {
                field_path: scope_path,
            });
        }
        scope_paths
            .try_reserve(1)
            .map_err(|_| WalkIssue::AllocationFailed {
                resource: "managed-reference registry scopes",
                requested: 1,
            })?;
        let scope_id = scope_paths.len();
        scope_paths.push(scope_path);
        Ok(scope_id)
    }

    fn visit(
        value: &SerializedValue,
        field_path: &mut String,
        inherited_scope_id: Option<usize>,
        graph: &mut LocalManagedReferenceGraph,
        pending: &mut Vec<PendingManagedReference>,
        scope_paths: &mut Vec<String>,
        context: ManagedReferenceScanContext,
    ) -> Result<(), WalkIssue> {
        let source_object_index = context.source_object_index;
        let source_path_id = context.source_path_id;
        let limits = context.limits;
        match value {
            SerializedValue::Struct(fields) => {
                // A struct's direct Registry field owns the ID table for all of that
                // struct's fields and the payloads stored in that table. Structs without
                // one inherit the nearest enclosing registry scope.
                let active_scope_id = fields
                    .iter()
                    .find(|field| {
                        matches!(&field.value, SerializedValue::ManagedReferenceRegistry(_))
                    })
                    .map(|field| register_scope(field_path, Some(&field.name), scope_paths, limits))
                    .transpose()?
                    .or(inherited_scope_id);
                for field in fields {
                    let old_length = append_path_component(
                        field_path,
                        &field.name,
                        !field_path.is_empty(),
                        limits.max_field_path_bytes,
                    )?;
                    let nested = visit(
                        &field.value,
                        field_path,
                        active_scope_id,
                        graph,
                        pending,
                        scope_paths,
                        context,
                    );
                    field_path.truncate(old_length);
                    nested?;
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
                    let nested = visit(
                        value,
                        field_path,
                        inherited_scope_id,
                        graph,
                        pending,
                        scope_paths,
                        context,
                    );
                    field_path.truncate(old_length);
                    nested?;
                }
            }
            SerializedValue::ManagedReferenceId {
                reference_id,
                indexed,
            } => {
                if pending.len() >= limits.max_edges {
                    return Err(WalkIssue::ManagedReferenceLimitExceeded {
                        field_path: copy_field_path(field_path)?,
                    });
                }
                pending
                    .try_reserve(1)
                    .map_err(|_| WalkIssue::AllocationFailed {
                        resource: "managed-reference ID edges",
                        requested: 1,
                    })?;
                pending.push(PendingManagedReference {
                    field_path: copy_field_path(field_path)?,
                    registry_scope_id: inherited_scope_id,
                    reference_id: *reference_id,
                    indexed: *indexed,
                });
            }
            SerializedValue::ManagedReferenceRegistry(registry) => {
                let registry_scope_id = match inherited_scope_id {
                    Some(scope_id) => scope_id,
                    None => register_scope(field_path, None, scope_paths, limits)?,
                };
                let registry_scope_path = copy_owned_string(
                    scope_paths.get(registry_scope_id).ok_or_else(|| {
                        WalkIssue::AllocationFailed {
                            resource: "managed-reference scope lookup",
                            requested: registry_scope_id.saturating_add(1),
                        }
                    })?,
                    "managed-reference scope path",
                )?;
                let registry_field_path = copy_field_path(field_path)?;
                let registry_parent_length = append_path_component(
                    field_path,
                    "RefIds",
                    !field_path.is_empty(),
                    limits.max_field_path_bytes,
                )?;
                let registry_field_length = field_path.len();
                for (entry_index, entry) in registry.entries.iter().enumerate() {
                    if graph.objects.len() >= limits.max_objects {
                        let current = copy_field_path(field_path)?;
                        field_path.truncate(registry_field_length);
                        return Err(WalkIssue::ManagedObjectLimitExceeded {
                            field_path: current,
                        });
                    }
                    graph
                        .objects
                        .try_reserve(1)
                        .map_err(|_| WalkIssue::AllocationFailed {
                            resource: "managed-reference registry objects",
                            requested: 1,
                        })?;
                    let class_name = copy_owned_string(&entry.class_name, "managed class name")?;
                    let namespace_name =
                        copy_owned_string(&entry.namespace_name, "managed namespace name")?;
                    let assembly_name =
                        copy_owned_string(&entry.assembly_name, "managed assembly name")?;
                    graph.objects.push(SerializedManagedObject {
                        source_object_index,
                        source_path_id,
                        registry_scope_id: Some(registry_scope_id),
                        registry_scope_path: Some(copy_owned_string(
                            &registry_scope_path,
                            "managed-reference scope path",
                        )?),
                        registry_field_path: copy_owned_string(
                            &registry_field_path,
                            "managed-reference registry field path",
                        )?,
                        registry_entry_index: entry_index,
                        reference_id: entry.reference_id,
                        class_name,
                        namespace_name,
                        assembly_name,
                        has_content: matches!(
                            &entry.data,
                            super::super::managed_registry::ManagedRegistryEntryData::Content(_)
                        ),
                    });

                    let entry_component = format!("[{entry_index}]");
                    append_path_component(
                        field_path,
                        &entry_component,
                        false,
                        limits.max_field_path_bytes,
                    )?;
                    append_path_component(field_path, "data", true, limits.max_field_path_bytes)?;
                    let nested = match &entry.data {
                        super::super::managed_registry::ManagedRegistryEntryData::Empty => Ok(()),
                        super::super::managed_registry::ManagedRegistryEntryData::Content(data) => {
                            visit(
                                data,
                                field_path,
                                Some(registry_scope_id),
                                graph,
                                pending,
                                scope_paths,
                                context,
                            )
                        }
                    };
                    field_path.truncate(registry_field_length);
                    nested?;
                }
                field_path.truncate(registry_parent_length);
            }
            _ => {}
        }
        Ok(())
    }

    let mut graph = LocalManagedReferenceGraph::default();
    let mut pending = Vec::new();
    let mut scope_paths = Vec::new();
    let context = ManagedReferenceScanContext {
        source_object_index,
        source_path_id,
        limits,
    };
    visit(
        root,
        field_path,
        None,
        &mut graph,
        &mut pending,
        &mut scope_paths,
        context,
    )?;

    let mut identifiers = HashMap::<(usize, i64), (usize, usize)>::new();
    identifiers
        .try_reserve(graph.objects.len())
        .map_err(|_| WalkIssue::AllocationFailed {
            resource: "managed-reference identifier index",
            requested: graph.objects.len(),
        })?;
    for (managed_object_index, object) in graph.objects.iter().enumerate() {
        if matches!(
            object.reference_id,
            super::super::managed_registry::UNKNOWN_REFERENCE_ID
                | super::super::managed_registry::NULL_REFERENCE_ID
        ) {
            continue;
        }
        let Some(registry_scope_id) = object.registry_scope_id else {
            continue;
        };
        identifiers
            .entry((registry_scope_id, object.reference_id))
            .and_modify(|(_, count)| *count = count.saturating_add(1))
            .or_insert((managed_object_index, 1));
    }
    graph
        .references
        .try_reserve_exact(pending.len())
        .map_err(|_| WalkIssue::AllocationFailed {
            resource: "managed-reference resolved edges",
            requested: pending.len(),
        })?;
    for edge in pending {
        let resolution = match edge.reference_id {
            super::super::managed_registry::NULL_REFERENCE_ID => {
                SerializedManagedReferenceResolution::Null
            }
            super::super::managed_registry::UNKNOWN_REFERENCE_ID => {
                SerializedManagedReferenceResolution::Unknown
            }
            reference_id => match edge.registry_scope_id.and_then(|scope_id| {
                identifiers
                    .get(&(scope_id, reference_id))
                    .copied()
                    .map(|resolved| (scope_id, resolved))
            }) {
                None => SerializedManagedReferenceResolution::Missing { reference_id },
                Some((registry_scope_id, (managed_object_index, 1))) => {
                    SerializedManagedReferenceResolution::Object {
                        registry_scope_id,
                        managed_object_index,
                        registry_entry_index: graph.objects[managed_object_index]
                            .registry_entry_index,
                    }
                }
                Some((_, (_, match_count))) => SerializedManagedReferenceResolution::Ambiguous {
                    reference_id,
                    match_count,
                },
            },
        };
        graph.references.push(SerializedManagedReference {
            source_object_index,
            source_path_id,
            field_path: edge.field_path,
            registry_scope_id: edge.registry_scope_id,
            reference_id: edge.reference_id,
            indexed: edge.indexed,
            resolution,
        });
    }
    Ok(graph)
}

fn copy_owned_string(value: &str, resource: &'static str) -> Result<String, WalkIssue> {
    let mut copied = String::new();
    copied
        .try_reserve_exact(value.len())
        .map_err(|_| WalkIssue::AllocationFailed {
            resource,
            requested: value.len(),
        })?;
    copied.push_str(value);
    Ok(copied)
}

pub(super) fn collect_references(
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
        SerializedValue::ManagedReferenceRegistry(registry) => {
            let registry_parent_length = append_path_component(
                field_path,
                "RefIds",
                !field_path.is_empty(),
                limits.max_field_path_bytes,
            )?;
            let registry_path_length = field_path.len();
            for (entry_index, entry) in registry.entries.iter().enumerate() {
                let entry_component = format!("[{entry_index}]");
                append_path_component(
                    field_path,
                    &entry_component,
                    false,
                    limits.max_field_path_bytes,
                )?;
                append_path_component(field_path, "data", true, limits.max_field_path_bytes)?;
                let nested = match &entry.data {
                    super::super::managed_registry::ManagedRegistryEntryData::Empty => Ok(()),
                    super::super::managed_registry::ManagedRegistryEntryData::Content(value) => {
                        collect_references(
                            value,
                            field_path,
                            object_index,
                            source_path_id,
                            output,
                            issues,
                            limits,
                        )
                    }
                };
                field_path.truncate(registry_path_length);
                nested?;
            }
            field_path.truncate(registry_parent_length);
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
        SerializedValue::Raw { bytes, .. } if bytes.is_empty() => {}
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
