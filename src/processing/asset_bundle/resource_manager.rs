use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::processing::scene_collections::{
    extract_resource_manager_container_references, SceneBundleMappingLimits,
};
use crate::serialized_file::collection_workspace::WorkspaceAssetCollections;
use crate::serialized_file::object_workspace::ObjectWorkspace;
use crate::serialized_file::reference_graph::{
    MultiFilePPtrResolution, MultiFileReferenceGraphLimits,
};
use crate::serialized_file::workspace::SerializedFileWorkspace;

use super::bundle_names::workspace_object_class_id;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceResourceManagerPathLimits {
    pub max_resource_managers: usize,
    pub max_total_container_entries: usize,
    pub max_assignments: usize,
    pub max_total_path_bytes: usize,
    pub max_issues: usize,
    pub container: SceneBundleMappingLimits,
    pub reference_graph: MultiFileReferenceGraphLimits,
}

impl Default for WorkspaceResourceManagerPathLimits {
    fn default() -> Self {
        Self {
            max_resource_managers: 1_000_000,
            max_total_container_entries: 1_000_000,
            max_assignments: 1_000_000,
            max_total_path_bytes: 256 * 1024 * 1024,
            max_issues: 100_000,
            container: SceneBundleMappingLimits::default(),
            reference_graph: MultiFileReferenceGraphLimits::default(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceResourceManagerPathAssignment {
    pub source_collection_index: usize,
    pub source_file_index: usize,
    pub source_object_index: usize,
    pub source_path_id: i64,
    pub container_entry_index: usize,
    pub path: String,
    pub file_id: i32,
    pub target_file_index: usize,
    pub target_object_index: usize,
    pub target_path_id: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceResourceManagerPathIssueKind {
    ObjectDecode(String),
    ContainerDecode(String),
    ReferenceScanIncomplete,
    PointerAbsentFromReferenceGraph {
        entry_index: usize,
    },
    UnresolvedPointer {
        entry_index: usize,
        resolution: MultiFilePPtrResolution,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceResourceManagerPathIssue {
    pub source_file_index: usize,
    pub source_object_index: usize,
    pub source_path_id: i64,
    pub kind: WorkspaceResourceManagerPathIssueKind,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceResourceManagerPathPlan {
    /// Resolved non-null Container targets, in GameBundle collection/object/entry order.
    pub assignments: Vec<WorkspaceResourceManagerPathAssignment>,
    pub issues: Vec<WorkspaceResourceManagerPathIssue>,
    pub resource_manager_count: usize,
    pub container_entry_count: usize,
}

impl WorkspaceResourceManagerPathPlan {
    pub fn is_complete(&self) -> bool {
        self.issues.is_empty()
    }
}

/// Plan `ResourceManager.Container` path assignments without changing decoded source values.
pub fn plan_workspace_resource_manager_paths(
    workspace: &SerializedFileWorkspace,
    objects: &ObjectWorkspace,
    collections: &WorkspaceAssetCollections,
    limits: WorkspaceResourceManagerPathLimits,
) -> Result<WorkspaceResourceManagerPathPlan, WorkspaceResourceManagerPathPlanError> {
    let references = workspace
        .reference_graph(limits.reference_graph)
        .map_err(
            |source| WorkspaceResourceManagerPathPlanError::ReferenceGraphBuild {
                message: source.to_string(),
            },
        )?;
    let mut plan = WorkspaceResourceManagerPathPlan::default();
    let mut total_path_bytes = 0usize;
    for collection in collections.collections() {
        if workspace
            .files()
            .get(collection.source_file_index)
            .is_none()
        {
            return Err(
                WorkspaceResourceManagerPathPlanError::WorkspaceIndexOutOfBounds {
                    resource: "ResourceManager source file",
                    index: collection.source_file_index,
                    count: workspace.files().len(),
                },
            );
        }
        let source_objects = objects
            .objects()
            .get(collection.object_range.clone())
            .ok_or(
                WorkspaceResourceManagerPathPlanError::ObjectRangeOutOfBounds {
                    start: collection.object_range.start,
                    end: collection.object_range.end,
                    object_count: objects.objects().len(),
                },
            )?;
        for object in source_objects {
            if object.file_index != collection.source_file_index {
                return Err(
                    WorkspaceResourceManagerPathPlanError::ObjectFileIdentityMismatch {
                        expected_file_index: collection.source_file_index,
                        actual_file_index: object.file_index,
                    },
                );
            }
            if workspace_object_class_id(object) != Some(147) {
                continue;
            }
            plan.resource_manager_count = plan.resource_manager_count.checked_add(1).ok_or(
                WorkspaceResourceManagerPathPlanError::CountOverflow {
                    resource: "ResourceManager objects",
                },
            )?;
            if plan.resource_manager_count > limits.max_resource_managers {
                return Err(WorkspaceResourceManagerPathPlanError::CountLimitExceeded {
                    resource: "ResourceManager objects",
                    count: plan.resource_manager_count,
                    limit: limits.max_resource_managers,
                });
            }
            let payload = match &object.payload {
                Ok(payload) => payload,
                Err(error) => {
                    push_resource_manager_path_issue(
                        &mut plan,
                        WorkspaceResourceManagerPathIssue {
                            source_file_index: object.file_index,
                            source_object_index: object.object_index,
                            source_path_id: object.path_id,
                            kind: WorkspaceResourceManagerPathIssueKind::ObjectDecode(
                                error.to_string(),
                            ),
                        },
                        limits.max_issues,
                    )?;
                    continue;
                }
            };
            let entries = match extract_resource_manager_container_references(
                &payload.root,
                limits.container,
            ) {
                Ok(entries) => entries,
                Err(error) => {
                    push_resource_manager_path_issue(
                        &mut plan,
                        WorkspaceResourceManagerPathIssue {
                            source_file_index: object.file_index,
                            source_object_index: object.object_index,
                            source_path_id: object.path_id,
                            kind: WorkspaceResourceManagerPathIssueKind::ContainerDecode(
                                error.to_string(),
                            ),
                        },
                        limits.max_issues,
                    )?;
                    continue;
                }
            };
            plan.container_entry_count = plan
                .container_entry_count
                .checked_add(entries.len())
                .ok_or(WorkspaceResourceManagerPathPlanError::CountOverflow {
                    resource: "ResourceManager Container entries",
                })?;
            if plan.container_entry_count > limits.max_total_container_entries {
                return Err(WorkspaceResourceManagerPathPlanError::CountLimitExceeded {
                    resource: "ResourceManager Container entries",
                    count: plan.container_entry_count,
                    limit: limits.max_total_container_entries,
                });
            }
            let graph_scan_complete = references
                .object_scan_is_complete(object.file_index, object.object_index)
                .unwrap_or(false);
            if !graph_scan_complete {
                push_resource_manager_path_issue(
                    &mut plan,
                    WorkspaceResourceManagerPathIssue {
                        source_file_index: object.file_index,
                        source_object_index: object.object_index,
                        source_path_id: object.path_id,
                        kind: WorkspaceResourceManagerPathIssueKind::ReferenceScanIncomplete,
                    },
                    limits.max_issues,
                )?;
                continue;
            }
            let graph_edges = references.references();
            let object_identity = (object.file_index, object.object_index);
            let edge_start = graph_edges.partition_point(|reference| {
                (reference.source_file_index, reference.source_object_index) < object_identity
            });
            let edge_end = graph_edges.partition_point(|reference| {
                (reference.source_file_index, reference.source_object_index) <= object_identity
            });
            let object_edges = graph_edges.get(edge_start..edge_end).ok_or(
                WorkspaceResourceManagerPathPlanError::CountOverflow {
                    resource: "ResourceManager reference slice",
                },
            )?;
            let mut reference_index = HashMap::<(i32, i64), usize>::new();
            reference_index
                .try_reserve(object_edges.len())
                .map_err(
                    |_| WorkspaceResourceManagerPathPlanError::AllocationFailed {
                        resource: "ResourceManager PPtr lookup",
                        requested: object_edges.len(),
                    },
                )?;
            for (edge_index, edge) in object_edges.iter().enumerate() {
                reference_index
                    .entry((edge.file_id, edge.path_id))
                    .or_insert(edge_index);
            }
            for (entry_index, entry) in entries.into_iter().enumerate() {
                total_path_bytes = total_path_bytes.checked_add(entry.path.len()).ok_or(
                    WorkspaceResourceManagerPathPlanError::CountOverflow {
                        resource: "ResourceManager Container path bytes",
                    },
                )?;
                if total_path_bytes > limits.max_total_path_bytes {
                    return Err(
                        WorkspaceResourceManagerPathPlanError::PathByteLimitExceeded {
                            byte_count: total_path_bytes,
                            limit: limits.max_total_path_bytes,
                        },
                    );
                }
                let Some(edge_index) = reference_index
                    .get(&(entry.file_id, entry.path_id))
                    .copied()
                else {
                    if entry.path_id == 0 {
                        continue;
                    }
                    push_resource_manager_path_issue(
                        &mut plan,
                        WorkspaceResourceManagerPathIssue {
                            source_file_index: object.file_index,
                            source_object_index: object.object_index,
                            source_path_id: object.path_id,
                            kind: WorkspaceResourceManagerPathIssueKind::PointerAbsentFromReferenceGraph {
                                entry_index,
                            },
                        },
                        limits.max_issues,
                    )?;
                    continue;
                };
                let reference = object_edges.get(edge_index).ok_or(
                    WorkspaceResourceManagerPathPlanError::CountOverflow {
                        resource: "ResourceManager PPtr lookup index",
                    },
                )?;
                match &reference.resolution {
                    MultiFilePPtrResolution::Null => {}
                    MultiFilePPtrResolution::Object {
                        target_file_index,
                        target_object_index,
                    } => {
                        let target = objects
                            .get(*target_file_index, *target_object_index)
                            .ok_or(
                                WorkspaceResourceManagerPathPlanError::WorkspaceIndexOutOfBounds {
                                    resource: "ResourceManager target object",
                                    index: *target_object_index,
                                    count: objects.objects().len(),
                                },
                            )?;
                        if target.path_id != entry.path_id {
                            return Err(
                                WorkspaceResourceManagerPathPlanError::TargetObjectIdentityMismatch {
                                    file_index: *target_file_index,
                                    object_index: *target_object_index,
                                    expected_path_id: entry.path_id,
                                    actual_path_id: target.path_id,
                                },
                            );
                        }
                        let requested = plan.assignments.len().checked_add(1).ok_or(
                            WorkspaceResourceManagerPathPlanError::CountOverflow {
                                resource: "ResourceManager path assignments",
                            },
                        )?;
                        if requested > limits.max_assignments {
                            return Err(
                                WorkspaceResourceManagerPathPlanError::CountLimitExceeded {
                                    resource: "ResourceManager path assignments",
                                    count: requested,
                                    limit: limits.max_assignments,
                                },
                            );
                        }
                        plan.assignments.try_reserve(1).map_err(|_| {
                            WorkspaceResourceManagerPathPlanError::AllocationFailed {
                                resource: "ResourceManager path assignments",
                                requested,
                            }
                        })?;
                        plan.assignments
                            .push(WorkspaceResourceManagerPathAssignment {
                                source_collection_index: collection.collection_index,
                                source_file_index: object.file_index,
                                source_object_index: object.object_index,
                                source_path_id: object.path_id,
                                container_entry_index: entry_index,
                                path: entry.path,
                                file_id: entry.file_id,
                                target_file_index: *target_file_index,
                                target_object_index: *target_object_index,
                                target_path_id: target.path_id,
                            });
                    }
                    resolution => {
                        push_resource_manager_path_issue(
                            &mut plan,
                            WorkspaceResourceManagerPathIssue {
                                source_file_index: object.file_index,
                                source_object_index: object.object_index,
                                source_path_id: object.path_id,
                                kind: WorkspaceResourceManagerPathIssueKind::UnresolvedPointer {
                                    entry_index,
                                    resolution: resolution.clone(),
                                },
                            },
                            limits.max_issues,
                        )?;
                    }
                }
            }
        }
    }
    Ok(plan)
}

fn push_resource_manager_path_issue(
    plan: &mut WorkspaceResourceManagerPathPlan,
    issue: WorkspaceResourceManagerPathIssue,
    limit: usize,
) -> Result<(), WorkspaceResourceManagerPathPlanError> {
    if plan.issues.len() >= limit {
        return Err(WorkspaceResourceManagerPathPlanError::IssueLimitExceeded {
            count: plan.issues.len().saturating_add(1),
            limit,
        });
    }
    plan.issues.try_reserve(1).map_err(|_| {
        WorkspaceResourceManagerPathPlanError::AllocationFailed {
            resource: "ResourceManager path issues",
            requested: plan.issues.len().saturating_add(1),
        }
    })?;
    plan.issues.push(issue);
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceResourceManagerPathPlanError {
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    CountOverflow {
        resource: &'static str,
    },
    CountLimitExceeded {
        resource: &'static str,
        count: usize,
        limit: usize,
    },
    PathByteLimitExceeded {
        byte_count: usize,
        limit: usize,
    },
    IssueLimitExceeded {
        count: usize,
        limit: usize,
    },
    ReferenceGraphBuild {
        message: String,
    },
    WorkspaceIndexOutOfBounds {
        resource: &'static str,
        index: usize,
        count: usize,
    },
    ObjectRangeOutOfBounds {
        start: usize,
        end: usize,
        object_count: usize,
    },
    ObjectFileIdentityMismatch {
        expected_file_index: usize,
        actual_file_index: usize,
    },
    TargetObjectIdentityMismatch {
        file_index: usize,
        object_index: usize,
        expected_path_id: i64,
        actual_path_id: i64,
    },
}

impl Display for WorkspaceResourceManagerPathPlanError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AllocationFailed { resource, requested } => {
                write!(formatter, "could not allocate {requested} entries for {resource}")
            }
            Self::CountOverflow { resource } => write!(formatter, "{resource} count overflows"),
            Self::CountLimitExceeded {
                resource,
                count,
                limit,
            } => write!(formatter, "{resource} count {count} exceeds limit {limit}"),
            Self::PathByteLimitExceeded { byte_count, limit } => write!(
                formatter,
                "ResourceManager Container paths use {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::IssueLimitExceeded { count, limit } => write!(
                formatter,
                "ResourceManager issue count {count} exceeds limit {limit}"
            ),
            Self::ReferenceGraphBuild { message } => {
                write!(formatter, "could not build ResourceManager PPtr resolution graph: {message}")
            }
            Self::WorkspaceIndexOutOfBounds {
                resource,
                index,
                count,
            } => write!(formatter, "{resource} index {index} is outside workspace count {count}"),
            Self::ObjectRangeOutOfBounds {
                start,
                end,
                object_count,
            } => write!(
                formatter,
                "ResourceManager source object range {start}..{end} is outside {object_count} objects"
            ),
            Self::ObjectFileIdentityMismatch {
                expected_file_index,
                actual_file_index,
            } => write!(
                formatter,
                "ResourceManager source object belongs to file {actual_file_index}; collection belongs to file {expected_file_index}"
            ),
            Self::TargetObjectIdentityMismatch {
                file_index,
                object_index,
                expected_path_id,
                actual_path_id,
            } => write!(
                formatter,
                "ResourceManager target {file_index}:{object_index} has PathID {actual_path_id}, expected {expected_path_id}"
            ),
        }
    }
}

impl Error for WorkspaceResourceManagerPathPlanError {}
