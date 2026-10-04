//! AssetBundle naming and Container pointer resolution.

use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::ops::Range;
use std::str;

use crate::serialized_file::collection_workspace::WorkspaceAssetCollections;
use crate::serialized_file::metadata::SerializedFileMetadata;
use crate::serialized_file::object::{SerializedValue, SerializedValueLookupError};
use crate::serialized_file::object_workspace::{ObjectWorkspace, WorkspaceObject};
use crate::serialized_file::reference_graph::{
    MultiFilePPtrResolution, MultiFileReferenceGraphLimits, ObjectReferenceGraph,
};
use crate::serialized_file::references::PPtrResolution;
use crate::serialized_file::workspace::SerializedFileWorkspace;

use super::scene_collections::{
    extract_asset_bundle_container_references, extract_resource_manager_container_references,
    AssetBundleContainerDecodeError, AssetBundleContainerReference, SceneBundleMappingLimits,
};

/// Resolve `GetAssetBundleName`: a non-empty serialized name wins, otherwise use the owning
/// source Bundle's name. The returned value is the unmodified name, before the processor's
/// `.bundle` suffix removal.
pub fn get_asset_bundle_name(
    root: &SerializedValue,
    source_bundle_name: &str,
) -> Result<String, AssetBundleNameDecodeError> {
    let mut selected = None;
    let aliases = [
        "m_AssetBundleName_R",
        "AssetBundleName_R",
        "m_AssetBundleName",
        "AssetBundleName",
    ];
    for alias in aliases {
        let value = root.unique_field_in_base_chain(alias).map_err(|source| {
            AssetBundleNameDecodeError::FieldLookup {
                field_name: alias,
                source,
            }
        })?;
        if let Some(value) = value {
            if let Some((first_alias, _)) = selected {
                return Err(AssetBundleNameDecodeError::DuplicateAliases {
                    first: first_alias,
                    second: alias,
                });
            }
            selected = Some((alias, value));
        }
    }

    let Some((field_name, SerializedValue::StringBytes(bytes))) = selected else {
        if let Some((field_name, value)) = selected {
            return Err(AssetBundleNameDecodeError::WrongType {
                field_name,
                actual: value_type_name(value),
            });
        }
        return copy_name(source_bundle_name, "source Bundle name");
    };
    let serialized_name =
        str::from_utf8(bytes).map_err(|error| AssetBundleNameDecodeError::InvalidUtf8 {
            field_name,
            valid_up_to: error.valid_up_to(),
        })?;
    if serialized_name.is_empty() {
        copy_name(source_bundle_name, "source Bundle name")
    } else {
        copy_name(serialized_name, "AssetBundleName_R")
    }
}

/// Apply the baseline's AssetBundle path rule to an already selected bundle name.
pub fn asset_bundle_name_for_asset(name: &str) -> Result<String, AssetBundleNameDecodeError> {
    let value = name
        .strip_suffix_case_insensitive(".bundle")
        .unwrap_or(name);
    copy_name(value, "asset bundle name")
}

trait StripSuffixCaseInsensitive {
    fn strip_suffix_case_insensitive(&self, suffix: &str) -> Option<&str>;
}

impl StripSuffixCaseInsensitive for str {
    fn strip_suffix_case_insensitive(&self, suffix: &str) -> Option<&str> {
        let start = self.len().checked_sub(suffix.len())?;
        self.get(start..)
            .filter(|tail| tail.eq_ignore_ascii_case(suffix))
            .map(|_| &self[..start])
    }
}

/// Classification of an AssetBundle Container PPtr using the owning file's PathID graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssetBundleContainerTarget {
    Null,
    LocalObject {
        object_index: usize,
    },
    SharedFile {
        file_id: i32,
        external_index: usize,
        path_id: i64,
    },
    MissingLocalObject {
        path_id: i64,
    },
    AmbiguousLocalObject {
        path_id: i64,
        match_count: usize,
    },
    InvalidFileId {
        file_id: i32,
        external_count: usize,
    },
}

/// Resolve one decoded Container entry against its owning SerializedFile.
pub fn resolve_asset_bundle_container_reference(
    reference: &AssetBundleContainerReference,
    metadata: &SerializedFileMetadata,
    reference_graph: &ObjectReferenceGraph,
) -> AssetBundleContainerTarget {
    let resolution =
        reference_graph.resolve_pointer(metadata, reference.file_id, reference.path_id);
    asset_bundle_container_target(resolution)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetBundleContainerMapping {
    pub reference: AssetBundleContainerReference,
    pub target: AssetBundleContainerTarget,
}

/// Resolved name and ordered Container targets for one decoded AssetBundle object.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetBundleContainerNameMap {
    /// AssetBundleExtensions.GetAssetBundleName result before path normalization.
    pub source_name: String,
    /// Name assigned by OriginalPathProcessor after removing a trailing bundle extension.
    pub asset_name: String,
    pub references: Vec<AssetBundleContainerMapping>,
}

pub fn map_asset_bundle_container(
    root: &SerializedValue,
    source_bundle_name: &str,
    metadata: &SerializedFileMetadata,
    reference_graph: &ObjectReferenceGraph,
    limits: SceneBundleMappingLimits,
) -> Result<AssetBundleContainerNameMap, AssetBundleContainerMapError> {
    let source_name = get_asset_bundle_name(root, source_bundle_name)
        .map_err(AssetBundleContainerMapError::Name)?;
    let asset_name =
        asset_bundle_name_for_asset(&source_name).map_err(AssetBundleContainerMapError::Name)?;
    let container_references = extract_asset_bundle_container_references(root, limits)
        .map_err(AssetBundleContainerMapError::Container)?;
    let mut references = Vec::new();
    references
        .try_reserve_exact(container_references.len())
        .map_err(|_| AssetBundleContainerMapError::AllocationFailed {
            resource: "AssetBundle Container mappings",
            requested: container_references.len(),
        })?;
    for reference in container_references {
        let target =
            resolve_asset_bundle_container_reference(&reference, metadata, reference_graph);
        references.push(AssetBundleContainerMapping { reference, target });
    }
    Ok(AssetBundleContainerNameMap {
        source_name,
        asset_name,
        references,
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceAssetBundleNameLimits {
    pub max_asset_bundles: usize,
    pub max_total_container_entries: usize,
    pub max_total_name_and_path_bytes: usize,
    pub max_issues: usize,
    pub container: SceneBundleMappingLimits,
}

impl Default for WorkspaceAssetBundleNameLimits {
    fn default() -> Self {
        Self {
            max_asset_bundles: 1_000_000,
            max_total_container_entries: 1_000_000,
            max_total_name_and_path_bytes: 256 * 1024 * 1024,
            max_issues: 100_000,
            container: SceneBundleMappingLimits::default(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceAssetBundleNameRecord {
    pub source_file_index: usize,
    pub source_object_index: usize,
    pub source_path_id: i64,
    pub source_name: String,
    pub asset_name: String,
    pub container_references: Range<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceAssetBundleReferenceRecord {
    pub bundle_record_index: usize,
    pub mapping: AssetBundleContainerMapping,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceAssetBundleNameAssignment {
    pub bundle_record_index: usize,
    pub reference_record_index: usize,
    pub target_file_index: usize,
    pub target_object_index: usize,
    pub target_path_id: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceAssetBundleNameIssueKind {
    ObjectDecode(String),
    SourceBundleNameUnavailable,
    MetadataDecode(String),
    UnresolvedContainerTarget(AssetBundleContainerTarget),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceAssetBundleNameIssue {
    pub source_file_index: usize,
    pub source_object_index: usize,
    pub source_path_id: i64,
    pub kind: WorkspaceAssetBundleNameIssueKind,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceAssetBundleNamePlan {
    pub bundles: Vec<WorkspaceAssetBundleNameRecord>,
    /// Entries remain in AssetBundle Container order and source collection traversal order.
    pub container_references: Vec<WorkspaceAssetBundleReferenceRecord>,
    /// Assignments remain in baseline processing order; later assignments to the same target win.
    pub assignments: Vec<WorkspaceAssetBundleNameAssignment>,
    pub issues: Vec<WorkspaceAssetBundleNameIssue>,
}

impl WorkspaceAssetBundleNamePlan {
    pub fn is_complete(&self) -> bool {
        self.issues.is_empty()
    }

    pub fn asset_name_for_assignment(
        &self,
        assignment: &WorkspaceAssetBundleNameAssignment,
    ) -> Option<&str> {
        self.bundles
            .get(assignment.bundle_record_index)
            .map(|bundle| bundle.asset_name.as_str())
    }
}

/// Build ordered, auditable AssetBundleName assignments without mutating input object values.
pub fn plan_workspace_asset_bundle_names(
    workspace: &SerializedFileWorkspace,
    objects: &ObjectWorkspace,
    collections: &WorkspaceAssetCollections,
    limits: WorkspaceAssetBundleNameLimits,
) -> Result<WorkspaceAssetBundleNamePlan, WorkspaceAssetBundleNamePlanError> {
    let mut plan = WorkspaceAssetBundleNamePlan::default();
    let mut total_metadata_bytes = 0usize;
    for collection in collections.collections() {
        let file = workspace.files().get(collection.source_file_index).ok_or(
            WorkspaceAssetBundleNamePlanError::FileIndexOutOfBounds {
                file_index: collection.source_file_index,
                file_count: workspace.files().len(),
            },
        )?;
        let source_objects = objects
            .objects()
            .get(collection.object_range.clone())
            .ok_or(WorkspaceAssetBundleNamePlanError::ObjectRangeOutOfBounds {
                start: collection.object_range.start,
                end: collection.object_range.end,
                object_count: objects.objects().len(),
            })?;
        for object in source_objects {
            if object.file_index != collection.source_file_index {
                return Err(
                    WorkspaceAssetBundleNamePlanError::ObjectFileIdentityMismatch {
                        expected_file_index: collection.source_file_index,
                        actual_file_index: object.file_index,
                    },
                );
            }
            if workspace_object_class_id(object) != Some(142) {
                continue;
            }
            if plan.bundles.len() >= limits.max_asset_bundles {
                return Err(
                    WorkspaceAssetBundleNamePlanError::AssetBundleCountLimitExceeded {
                        count: plan.bundles.len().saturating_add(1),
                        limit: limits.max_asset_bundles,
                    },
                );
            }
            let payload = match &object.payload {
                Err(error) => {
                    push_workspace_asset_bundle_issue(
                        &mut plan,
                        WorkspaceAssetBundleNameIssue {
                            source_file_index: object.file_index,
                            source_object_index: object.object_index,
                            source_path_id: object.path_id,
                            kind: WorkspaceAssetBundleNameIssueKind::ObjectDecode(
                                error.to_string(),
                            ),
                        },
                        limits.max_issues,
                    )?;
                    continue;
                }
                Ok(payload) => payload,
            };
            let source_bundle_name = match file.container_bundle_index {
                None => Some("GameBundle"),
                Some(bundle_index) => workspace
                    .container_bundles()
                    .get(bundle_index)
                    .and_then(|bundle| bundle.name_fixed.as_deref()),
            };
            let fallback_name = source_bundle_name.unwrap_or_default();
            let mapping = match map_asset_bundle_container(
                &payload.root,
                fallback_name,
                &file.metadata,
                &file.reference_graph,
                limits.container,
            ) {
                Ok(mapping) => mapping,
                Err(error) => {
                    push_workspace_asset_bundle_issue(
                        &mut plan,
                        WorkspaceAssetBundleNameIssue {
                            source_file_index: object.file_index,
                            source_object_index: object.object_index,
                            source_path_id: object.path_id,
                            kind: WorkspaceAssetBundleNameIssueKind::MetadataDecode(
                                error.to_string(),
                            ),
                        },
                        limits.max_issues,
                    )?;
                    continue;
                }
            };
            if source_bundle_name.is_none() && mapping.source_name.is_empty() {
                push_workspace_asset_bundle_issue(
                    &mut plan,
                    WorkspaceAssetBundleNameIssue {
                        source_file_index: object.file_index,
                        source_object_index: object.object_index,
                        source_path_id: object.path_id,
                        kind: WorkspaceAssetBundleNameIssueKind::SourceBundleNameUnavailable,
                    },
                    limits.max_issues,
                )?;
            }
            let can_assign_name = source_bundle_name.is_some() || !mapping.source_name.is_empty();
            let entry_count = plan
                .container_references
                .len()
                .checked_add(mapping.references.len())
                .ok_or(WorkspaceAssetBundleNamePlanError::CountOverflow {
                    resource: "AssetBundle Container references",
                })?;
            if entry_count > limits.max_total_container_entries {
                return Err(
                    WorkspaceAssetBundleNamePlanError::ContainerEntryCountLimitExceeded {
                        count: entry_count,
                        limit: limits.max_total_container_entries,
                    },
                );
            }
            let metadata_bytes = mapping
                .source_name
                .len()
                .checked_add(mapping.asset_name.len())
                .and_then(|bytes| {
                    mapping
                        .references
                        .iter()
                        .try_fold(bytes, |total, reference| {
                            total.checked_add(reference.reference.path.len())
                        })
                })
                .ok_or(WorkspaceAssetBundleNamePlanError::CountOverflow {
                    resource: "AssetBundle name and Container path bytes",
                })?;
            total_metadata_bytes = total_metadata_bytes.checked_add(metadata_bytes).ok_or(
                WorkspaceAssetBundleNamePlanError::CountOverflow {
                    resource: "AssetBundle name and Container path bytes",
                },
            )?;
            if total_metadata_bytes > limits.max_total_name_and_path_bytes {
                return Err(
                    WorkspaceAssetBundleNamePlanError::MetadataByteLimitExceeded {
                        byte_count: total_metadata_bytes,
                        limit: limits.max_total_name_and_path_bytes,
                    },
                );
            }
            let bundle_record_index = plan.bundles.len();
            let reference_start = plan.container_references.len();
            plan.container_references
                .try_reserve(mapping.references.len())
                .map_err(|_| WorkspaceAssetBundleNamePlanError::AllocationFailed {
                    resource: "AssetBundle Container reference records",
                    requested: entry_count,
                })?;
            for container_mapping in mapping.references {
                let reference_record_index = plan.container_references.len();
                match &container_mapping.target {
                    AssetBundleContainerTarget::LocalObject { object_index } if can_assign_name => {
                        let target = objects.get(object.file_index, *object_index).ok_or(
                            WorkspaceAssetBundleNamePlanError::TargetObjectIndexOutOfBounds {
                                file_index: object.file_index,
                                object_index: *object_index,
                            },
                        )?;
                        if target.path_id != container_mapping.reference.path_id {
                            return Err(
                                WorkspaceAssetBundleNamePlanError::TargetObjectIdentityMismatch {
                                    file_index: object.file_index,
                                    object_index: *object_index,
                                    expected_path_id: container_mapping.reference.path_id,
                                    actual_path_id: target.path_id,
                                },
                            );
                        }
                        plan.assignments.try_reserve(1).map_err(|_| {
                            WorkspaceAssetBundleNamePlanError::AllocationFailed {
                                resource: "AssetBundleName assignments",
                                requested: plan.assignments.len().saturating_add(1),
                            }
                        })?;
                        plan.assignments.push(WorkspaceAssetBundleNameAssignment {
                            bundle_record_index,
                            reference_record_index,
                            target_file_index: target.file_index,
                            target_object_index: target.object_index,
                            target_path_id: target.path_id,
                        });
                    }
                    AssetBundleContainerTarget::LocalObject { .. } => {}
                    AssetBundleContainerTarget::MissingLocalObject { .. }
                    | AssetBundleContainerTarget::AmbiguousLocalObject { .. }
                    | AssetBundleContainerTarget::InvalidFileId { .. } => {
                        push_workspace_asset_bundle_issue(
                            &mut plan,
                            WorkspaceAssetBundleNameIssue {
                                source_file_index: object.file_index,
                                source_object_index: object.object_index,
                                source_path_id: object.path_id,
                                kind: WorkspaceAssetBundleNameIssueKind::UnresolvedContainerTarget(
                                    container_mapping.target.clone(),
                                ),
                            },
                            limits.max_issues,
                        )?;
                    }
                    AssetBundleContainerTarget::Null
                    | AssetBundleContainerTarget::SharedFile { .. } => {}
                }
                plan.container_references
                    .push(WorkspaceAssetBundleReferenceRecord {
                        bundle_record_index,
                        mapping: container_mapping,
                    });
            }
            let reference_end = plan.container_references.len();
            plan.bundles.try_reserve(1).map_err(|_| {
                WorkspaceAssetBundleNamePlanError::AllocationFailed {
                    resource: "AssetBundle name records",
                    requested: bundle_record_index.saturating_add(1),
                }
            })?;
            plan.bundles.push(WorkspaceAssetBundleNameRecord {
                source_file_index: object.file_index,
                source_object_index: object.object_index,
                source_path_id: object.path_id,
                source_name: mapping.source_name,
                asset_name: mapping.asset_name,
                container_references: reference_start..reference_end,
            });
        }
    }
    Ok(plan)
}

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

#[derive(Clone, Copy, Debug)]
pub struct WorkspaceOriginalPathPlans<'a> {
    pub asset_bundles: &'a WorkspaceAssetBundleNamePlan,
    pub resource_managers: &'a WorkspaceResourceManagerPathPlan,
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

/// Baseline setting that controls the AssetBundle-derived project paths.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BundledAssetsExportMode {
    GroupByAssetType,
    GroupByBundleName,
    #[default]
    DirectExport,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceAssetExportPathLimits {
    pub max_bundles: usize,
    pub max_records: usize,
    pub max_assignments: usize,
    pub max_total_operations: usize,
    pub max_total_metadata_bytes: usize,
}

impl Default for WorkspaceAssetExportPathLimits {
    fn default() -> Self {
        Self {
            max_bundles: 1_000_000,
            max_records: 20_000_000,
            max_assignments: 1_000_000,
            max_total_operations: 20_000_000,
            max_total_metadata_bytes: 512 * 1024 * 1024,
        }
    }
}

/// Export-only path metadata for an object. It is separate from the decoded SerializedValue so
/// source bytes and values remain unchanged while processors update project paths.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceAssetExportPath {
    pub file_index: usize,
    pub object_index: usize,
    pub path_id: i64,
    pub asset_bundle_name: Option<String>,
    pub original_path: Option<String>,
    pub original_directory: Option<String>,
    pub original_name: Option<String>,
    pub original_extension: Option<String>,
    pub override_directory: Option<String>,
    pub override_name: Option<String>,
    pub override_extension: Option<String>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceAssetExportPathTable {
    records: Vec<WorkspaceAssetExportPath>,
}

impl WorkspaceAssetExportPathTable {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn from_records(
        mut records: Vec<WorkspaceAssetExportPath>,
        limits: WorkspaceAssetExportPathLimits,
    ) -> Result<Self, WorkspaceAssetBundlePathError> {
        if records.len() > limits.max_records {
            return Err(WorkspaceAssetBundlePathError::CountLimitExceeded {
                resource: "export path metadata records",
                count: records.len(),
                limit: limits.max_records,
            });
        }
        for record in &mut records {
            record.asset_bundle_name =
                nonempty_string(std::mem::take(&mut record.asset_bundle_name));
            record.original_path = nonempty_string(std::mem::take(&mut record.original_path));
            record.original_directory =
                nonempty_string(std::mem::take(&mut record.original_directory));
            record.original_name = nonempty_string(std::mem::take(&mut record.original_name));
            record.original_extension =
                normalize_path_extension(std::mem::take(&mut record.original_extension));
            record.override_directory =
                nonempty_string(std::mem::take(&mut record.override_directory));
            record.override_name = nonempty_string(std::mem::take(&mut record.override_name));
            record.override_extension =
                normalize_path_extension(std::mem::take(&mut record.override_extension));
        }
        let mut byte_count = export_path_metadata_bytes(&records)?;
        if byte_count > limits.max_total_metadata_bytes {
            return Err(WorkspaceAssetBundlePathError::MetadataByteLimitExceeded {
                byte_count,
                limit: limits.max_total_metadata_bytes,
            });
        }
        for record in &mut records {
            if let Some(path) = record.original_path.as_deref() {
                let details = original_path_detail_slices(path);
                let additional_bytes = (if record.original_directory.is_none() {
                    details.directory.map_or(0, str::len)
                } else {
                    0
                })
                .checked_add(if record.original_name.is_none() {
                    details.name.map_or(0, str::len)
                } else {
                    0
                })
                .and_then(|bytes| {
                    bytes.checked_add(if record.original_extension.is_none() {
                        details.extension.map_or(0, str::len)
                    } else {
                        0
                    })
                })
                .ok_or(WorkspaceAssetBundlePathError::MetadataByteCountOverflow)?;
                byte_count = byte_count
                    .checked_add(additional_bytes)
                    .ok_or(WorkspaceAssetBundlePathError::MetadataByteCountOverflow)?;
                if byte_count > limits.max_total_metadata_bytes {
                    return Err(WorkspaceAssetBundlePathError::MetadataByteLimitExceeded {
                        byte_count,
                        limit: limits.max_total_metadata_bytes,
                    });
                }
                if record.original_directory.is_none() {
                    record.original_directory = details
                        .directory
                        .map(try_copy_export_path_string)
                        .transpose()?;
                }
                if record.original_name.is_none() {
                    record.original_name =
                        details.name.map(try_copy_export_path_string).transpose()?;
                }
                if record.original_extension.is_none() {
                    record.original_extension = details
                        .extension
                        .map(try_copy_export_path_string)
                        .transpose()?;
                }
            }
        }
        records.sort_unstable_by_key(|record| (record.file_index, record.object_index));
        for pair in records.windows(2) {
            if (pair[0].file_index, pair[0].object_index)
                == (pair[1].file_index, pair[1].object_index)
            {
                return Err(WorkspaceAssetBundlePathError::DuplicateMetadataIdentity {
                    file_index: pair[0].file_index,
                    object_index: pair[0].object_index,
                });
            }
        }
        byte_count = export_path_metadata_bytes(&records)?;
        if byte_count > limits.max_total_metadata_bytes {
            return Err(WorkspaceAssetBundlePathError::MetadataByteLimitExceeded {
                byte_count,
                limit: limits.max_total_metadata_bytes,
            });
        }
        Ok(Self { records })
    }

    pub fn records(&self) -> &[WorkspaceAssetExportPath] {
        &self.records
    }

    pub fn get(&self, file_index: usize, object_index: usize) -> Option<&WorkspaceAssetExportPath> {
        self.records
            .binary_search_by_key(&(file_index, object_index), |record| {
                (record.file_index, record.object_index)
            })
            .ok()
            .and_then(|index| self.records.get(index))
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceAssetExportPathReport {
    pub assigned_bundle_names: usize,
    pub assigned_original_paths: usize,
    pub assigned_resource_manager_original_paths: usize,
    pub assigned_collection_directories: usize,
    pub restored_original_names: usize,
    pub case_restoration_unverified: usize,
    pub named_asset_name_unavailable: usize,
    pub initialized_shader_overrides: usize,
    pub metadata_record_count: usize,
    pub metadata_string_bytes: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BundleOriginalPathEffect {
    original_path: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OriginalPathDetails {
    directory: Option<String>,
    name: Option<String>,
    extension: Option<String>,
}

struct OriginalPathDetailSlices<'a> {
    directory: Option<&'a str>,
    name: Option<&'a str>,
    extension: Option<&'a str>,
}

struct WorkspaceAssetBundlePathApplyState<'a> {
    records: &'a mut Vec<WorkspaceAssetExportPath>,
    record_index: &'a mut HashMap<(usize, usize), usize>,
    metadata_bytes: &'a mut usize,
    operation_count: &'a mut usize,
    report: &'a mut WorkspaceAssetExportPathReport,
    limits: WorkspaceAssetExportPathLimits,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkspaceOriginalPathEventKind {
    AssetBundle(usize),
    ResourceManager(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WorkspaceOriginalPathEvent {
    collection_index: usize,
    source_object_index: usize,
    container_entry_index: usize,
    kind: WorkspaceOriginalPathEventKind,
}

fn sort_workspace_original_path_events(events: &mut [WorkspaceOriginalPathEvent]) {
    events.sort_unstable_by_key(|event| {
        (
            event.collection_index,
            event.source_object_index,
            event.container_entry_index,
        )
    });
}

/// Apply the AssetBundle portion of `OriginalPathProcessor` to export-owned path metadata.
///
/// The input name plan must be complete. Bundle Container assignments run in source order, then
/// GroupByBundleName collection directories are filled with the baseline's `??=` behavior.
pub fn apply_workspace_asset_bundle_paths(
    workspace: &SerializedFileWorkspace,
    objects: &ObjectWorkspace,
    collections: &WorkspaceAssetCollections,
    bundle_plan: &WorkspaceAssetBundleNamePlan,
    mode: BundledAssetsExportMode,
    export_paths: WorkspaceAssetExportPathTable,
    limits: WorkspaceAssetExportPathLimits,
) -> Result<
    (
        WorkspaceAssetExportPathTable,
        WorkspaceAssetExportPathReport,
    ),
    WorkspaceAssetBundlePathError,
> {
    apply_workspace_original_paths(
        workspace,
        objects,
        collections,
        WorkspaceOriginalPathPlans {
            asset_bundles: bundle_plan,
            resource_managers: &WorkspaceResourceManagerPathPlan::default(),
        },
        mode,
        export_paths,
        limits,
    )
}

/// Apply ResourceManager and AssetBundle path updates in `GameBundle.FetchAssets()` order.
///
/// The per-object Container order is retained, then GroupByBundleName collection directories
/// are filled after both processors as the baseline does.
pub fn apply_workspace_original_paths(
    workspace: &SerializedFileWorkspace,
    objects: &ObjectWorkspace,
    collections: &WorkspaceAssetCollections,
    plans: WorkspaceOriginalPathPlans<'_>,
    mode: BundledAssetsExportMode,
    mut export_paths: WorkspaceAssetExportPathTable,
    limits: WorkspaceAssetExportPathLimits,
) -> Result<
    (
        WorkspaceAssetExportPathTable,
        WorkspaceAssetExportPathReport,
    ),
    WorkspaceAssetBundlePathError,
> {
    let bundle_plan = plans.asset_bundles;
    let resource_manager_plan = plans.resource_managers;
    if !bundle_plan.is_complete() {
        return Err(WorkspaceAssetBundlePathError::IncompleteNamePlan {
            issue_count: bundle_plan.issues.len(),
        });
    }
    if !resource_manager_plan.is_complete() {
        return Err(
            WorkspaceAssetBundlePathError::IncompleteResourceManagerPlan {
                issue_count: resource_manager_plan.issues.len(),
            },
        );
    }
    if bundle_plan.bundles.len() > limits.max_bundles {
        return Err(WorkspaceAssetBundlePathError::CountLimitExceeded {
            resource: "AssetBundle path source records",
            count: bundle_plan.bundles.len(),
            limit: limits.max_bundles,
        });
    }
    if bundle_plan.assignments.len() > limits.max_assignments {
        return Err(WorkspaceAssetBundlePathError::CountLimitExceeded {
            resource: "AssetBundle path assignments",
            count: bundle_plan.assignments.len(),
            limit: limits.max_assignments,
        });
    }
    if resource_manager_plan.assignments.len() > limits.max_assignments {
        return Err(WorkspaceAssetBundlePathError::CountLimitExceeded {
            resource: "ResourceManager path assignments",
            count: resource_manager_plan.assignments.len(),
            limit: limits.max_assignments,
        });
    }
    let event_count = bundle_plan
        .assignments
        .len()
        .checked_add(resource_manager_plan.assignments.len())
        .ok_or(WorkspaceAssetBundlePathError::CountOverflow {
            resource: "OriginalPath processor events",
        })?;
    if event_count > limits.max_total_operations {
        return Err(WorkspaceAssetBundlePathError::CountLimitExceeded {
            resource: "OriginalPath processor events",
            count: event_count,
            limit: limits.max_total_operations,
        });
    }
    let mut events = Vec::new();
    events.try_reserve_exact(event_count).map_err(|_| {
        WorkspaceAssetBundlePathError::AllocationFailed {
            resource: "OriginalPath processor events",
            requested: event_count,
        }
    })?;
    for (assignment_index, assignment) in bundle_plan.assignments.iter().enumerate() {
        let bundle = bundle_plan
            .bundles
            .get(assignment.bundle_record_index)
            .ok_or(WorkspaceAssetBundlePathError::InvalidNamePlan {
                resource: "AssetBundle record index",
                index: assignment.bundle_record_index,
            })?;
        let collection_index = collections
            .collection_index_for_file(bundle.source_file_index)
            .ok_or(WorkspaceAssetBundlePathError::InvalidWorkspaceIdentity {
                resource: "AssetBundle source collection",
                index: bundle.source_file_index,
            })?;
        let container_entry_index = assignment
            .reference_record_index
            .checked_sub(bundle.container_references.start)
            .filter(|index| *index < bundle.container_references.len())
            .ok_or(WorkspaceAssetBundlePathError::InvalidNamePlan {
                resource: "AssetBundle Container assignment order",
                index: assignment.reference_record_index,
            })?;
        events.push(WorkspaceOriginalPathEvent {
            collection_index,
            source_object_index: bundle.source_object_index,
            container_entry_index,
            kind: WorkspaceOriginalPathEventKind::AssetBundle(assignment_index),
        });
    }
    for (assignment_index, assignment) in resource_manager_plan.assignments.iter().enumerate() {
        let actual_collection_index = collections
            .collection_index_for_file(assignment.source_file_index)
            .ok_or(WorkspaceAssetBundlePathError::InvalidWorkspaceIdentity {
                resource: "ResourceManager source collection",
                index: assignment.source_file_index,
            })?;
        if actual_collection_index != assignment.source_collection_index {
            return Err(WorkspaceAssetBundlePathError::InvalidWorkspaceIdentity {
                resource: "ResourceManager source collection order",
                index: assignment.source_file_index,
            });
        }
        events.push(WorkspaceOriginalPathEvent {
            collection_index: assignment.source_collection_index,
            source_object_index: assignment.source_object_index,
            container_entry_index: assignment.container_entry_index,
            kind: WorkspaceOriginalPathEventKind::ResourceManager(assignment_index),
        });
    }
    sort_workspace_original_path_events(&mut events);
    if export_paths.records.len() > limits.max_records {
        return Err(WorkspaceAssetBundlePathError::CountLimitExceeded {
            resource: "export path metadata records",
            count: export_paths.records.len(),
            limit: limits.max_records,
        });
    }
    let mut metadata_bytes = export_path_metadata_bytes(&export_paths.records)?;
    if metadata_bytes > limits.max_total_metadata_bytes {
        return Err(WorkspaceAssetBundlePathError::MetadataByteLimitExceeded {
            byte_count: metadata_bytes,
            limit: limits.max_total_metadata_bytes,
        });
    }

    let mut record_index = HashMap::<(usize, usize), usize>::new();
    record_index
        .try_reserve(export_paths.records.len())
        .map_err(|_| WorkspaceAssetBundlePathError::AllocationFailed {
            resource: "export path metadata identity index",
            requested: export_paths.records.len(),
        })?;
    for (index, record) in export_paths.records.iter().enumerate() {
        let object = objects.get(record.file_index, record.object_index).ok_or(
            WorkspaceAssetBundlePathError::WorkspaceIndexOutOfBounds {
                resource: "export metadata object",
                index: record.object_index,
                count: objects.objects().len(),
            },
        )?;
        if object.path_id != record.path_id {
            return Err(WorkspaceAssetBundlePathError::ObjectIdentityMismatch {
                file_index: record.file_index,
                object_index: record.object_index,
            });
        }
        if record_index
            .insert((record.file_index, record.object_index), index)
            .is_some()
        {
            return Err(WorkspaceAssetBundlePathError::DuplicateMetadataIdentity {
                file_index: record.file_index,
                object_index: record.object_index,
            });
        }
    }

    let mut report = WorkspaceAssetExportPathReport::default();
    let mut operation_count = 0usize;
    for event in &events {
        operation_count = checked_path_operation_count(operation_count, limits)?;
        if let WorkspaceOriginalPathEventKind::AssetBundle(assignment_index) = event.kind {
            let assignment = bundle_plan.assignments.get(assignment_index).ok_or(
                WorkspaceAssetBundlePathError::InvalidNamePlan {
                    resource: "AssetBundle assignment index",
                    index: assignment_index,
                },
            )?;
            let bundle = bundle_plan
                .bundles
                .get(assignment.bundle_record_index)
                .ok_or(WorkspaceAssetBundlePathError::InvalidNamePlan {
                    resource: "AssetBundle record index",
                    index: assignment.bundle_record_index,
                })?;
            let reference = bundle_plan
                .container_references
                .get(assignment.reference_record_index)
                .ok_or(WorkspaceAssetBundlePathError::InvalidNamePlan {
                    resource: "Container reference index",
                    index: assignment.reference_record_index,
                })?;
            if !bundle
                .container_references
                .contains(&assignment.reference_record_index)
                || reference.bundle_record_index != assignment.bundle_record_index
                || !matches!(
                    reference.mapping.target,
                    AssetBundleContainerTarget::LocalObject { object_index }
                        if object_index == assignment.target_object_index
                )
                || bundle.source_file_index != assignment.target_file_index
            {
                return Err(WorkspaceAssetBundlePathError::InvalidNamePlan {
                    resource: "AssetBundle assignment identity",
                    index: assignment.reference_record_index,
                });
            }
            let target = objects
                .get(assignment.target_file_index, assignment.target_object_index)
                .ok_or(WorkspaceAssetBundlePathError::WorkspaceIndexOutOfBounds {
                    resource: "AssetBundle assignment target object",
                    index: assignment.target_object_index,
                    count: objects.objects().len(),
                })?;
            if target.path_id != assignment.target_path_id
                || target.path_id != reference.mapping.reference.path_id
            {
                return Err(WorkspaceAssetBundlePathError::ObjectIdentityMismatch {
                    file_index: assignment.target_file_index,
                    object_index: assignment.target_object_index,
                });
            }

            let path_effect = bundle_container_original_path(
                &reference.mapping.reference.path,
                &bundle.asset_name,
                mode,
            )?;
            let bundle_name = try_copy_export_path_string(&bundle.asset_name)?;
            let named_asset_name = if path_effect.is_some() {
                match workspace_named_asset_name(target) {
                    Ok(name) => name,
                    Err(WorkspaceAssetBundlePathError::ObjectPayloadUnavailable { .. }) => {
                        report.named_asset_name_unavailable = report
                            .named_asset_name_unavailable
                            .checked_add(1)
                            .ok_or(WorkspaceAssetBundlePathError::CountOverflow {
                                resource: "unavailable INamed asset names",
                            })?;
                        None
                    }
                    Err(error) => return Err(error),
                }
            } else {
                None
            };
            let is_shader = workspace_object_class_id(target) == Some(48);
            let index = ensure_export_path_record(
                &mut export_paths.records,
                &mut record_index,
                assignment.target_file_index,
                assignment.target_object_index,
                assignment.target_path_id,
                limits.max_records,
            )?;
            let old_record_bytes = export_path_record_bytes(&export_paths.records[index])?;
            let record = &mut export_paths.records[index];
            record.asset_bundle_name = Some(bundle_name);
            if let Some(effect) = path_effect {
                if effect.original_path.is_some() {
                    report.assigned_original_paths = report
                        .assigned_original_paths
                        .checked_add(1)
                        .ok_or(WorkspaceAssetBundlePathError::CountOverflow {
                            resource: "assigned OriginalPaths",
                        })?;
                }
                apply_original_path_postprocessing(
                    record,
                    effect.original_path,
                    named_asset_name.as_deref(),
                    is_shader,
                    &mut report,
                )?;
            }
            let new_record_bytes = export_path_record_bytes(record)?;
            metadata_bytes = metadata_bytes
                .checked_sub(old_record_bytes)
                .and_then(|bytes| bytes.checked_add(new_record_bytes))
                .ok_or(WorkspaceAssetBundlePathError::MetadataByteCountOverflow)?;
            if metadata_bytes > limits.max_total_metadata_bytes {
                return Err(WorkspaceAssetBundlePathError::MetadataByteLimitExceeded {
                    byte_count: metadata_bytes,
                    limit: limits.max_total_metadata_bytes,
                });
            }
            report.assigned_bundle_names = report.assigned_bundle_names.checked_add(1).ok_or(
                WorkspaceAssetBundlePathError::CountOverflow {
                    resource: "assigned AssetBundleNames",
                },
            )?;
        } else {
            let WorkspaceOriginalPathEventKind::ResourceManager(assignment_index) = event.kind
            else {
                continue;
            };
            let assignment = resource_manager_plan
                .assignments
                .get(assignment_index)
                .ok_or(WorkspaceAssetBundlePathError::InvalidNamePlan {
                    resource: "ResourceManager assignment index",
                    index: assignment_index,
                })?;
            if assignment.source_collection_index != event.collection_index
                || assignment.source_object_index != event.source_object_index
                || assignment.container_entry_index != event.container_entry_index
            {
                return Err(WorkspaceAssetBundlePathError::InvalidNamePlan {
                    resource: "ResourceManager assignment order",
                    index: assignment_index,
                });
            }
            let source = objects
                .get(assignment.source_file_index, assignment.source_object_index)
                .ok_or(WorkspaceAssetBundlePathError::WorkspaceIndexOutOfBounds {
                    resource: "ResourceManager assignment source",
                    index: assignment.source_object_index,
                    count: objects.objects().len(),
                })?;
            if source.path_id != assignment.source_path_id
                || workspace_object_class_id(source) != Some(147)
                || assignment.target_path_id == 0
            {
                return Err(WorkspaceAssetBundlePathError::InvalidNamePlan {
                    resource: "ResourceManager source identity",
                    index: assignment_index,
                });
            }
            let target = objects
                .get(assignment.target_file_index, assignment.target_object_index)
                .ok_or(WorkspaceAssetBundlePathError::WorkspaceIndexOutOfBounds {
                    resource: "ResourceManager assignment target",
                    index: assignment.target_object_index,
                    count: objects.objects().len(),
                })?;
            if target.path_id != assignment.target_path_id {
                return Err(WorkspaceAssetBundlePathError::ObjectIdentityMismatch {
                    file_index: assignment.target_file_index,
                    object_index: assignment.target_object_index,
                });
            }
            let resource_path = join_project_path(&["Assets/Resources", &assignment.path])?;
            let current_path = if let Some(index) = record_index
                .get(&(assignment.target_file_index, assignment.target_object_index))
                .copied()
            {
                let current_record = export_paths
                    .records
                    .get(index)
                    .ok_or(WorkspaceAssetBundlePathError::InvalidMetadataIndex { index })?;
                if let Some(current_path) = current_record.original_path.as_deref() {
                    Some(try_copy_export_path_string(current_path)?)
                } else {
                    calculated_original_path(current_record)?
                }
            } else {
                None
            };
            if !resource_manager_path_should_replace(current_path.as_deref(), &resource_path) {
                continue;
            }
            let named_asset_name = match workspace_named_asset_name(target) {
                Ok(name) => name,
                Err(WorkspaceAssetBundlePathError::ObjectPayloadUnavailable { .. }) => {
                    report.named_asset_name_unavailable = report
                        .named_asset_name_unavailable
                        .checked_add(1)
                        .ok_or(WorkspaceAssetBundlePathError::CountOverflow {
                            resource: "unavailable INamed asset names",
                        })?;
                    None
                }
                Err(error) => return Err(error),
            };
            let index = ensure_export_path_record(
                &mut export_paths.records,
                &mut record_index,
                assignment.target_file_index,
                assignment.target_object_index,
                assignment.target_path_id,
                limits.max_records,
            )?;
            let old_record_bytes = export_path_record_bytes(&export_paths.records[index])?;
            let record = &mut export_paths.records[index];
            apply_original_path_postprocessing(
                record,
                Some(resource_path),
                named_asset_name.as_deref(),
                workspace_object_class_id(target) == Some(48),
                &mut report,
            )?;
            let new_record_bytes = export_path_record_bytes(record)?;
            metadata_bytes = metadata_bytes
                .checked_sub(old_record_bytes)
                .and_then(|bytes| bytes.checked_add(new_record_bytes))
                .ok_or(WorkspaceAssetBundlePathError::MetadataByteCountOverflow)?;
            if metadata_bytes > limits.max_total_metadata_bytes {
                return Err(WorkspaceAssetBundlePathError::MetadataByteLimitExceeded {
                    byte_count: metadata_bytes,
                    limit: limits.max_total_metadata_bytes,
                });
            }
            report.assigned_resource_manager_original_paths = report
                .assigned_resource_manager_original_paths
                .checked_add(1)
                .ok_or(WorkspaceAssetBundlePathError::CountOverflow {
                    resource: "assigned ResourceManager OriginalPaths",
                })?;
        }
    }

    if mode == BundledAssetsExportMode::GroupByBundleName {
        let mut state = WorkspaceAssetBundlePathApplyState {
            records: &mut export_paths.records,
            record_index: &mut record_index,
            metadata_bytes: &mut metadata_bytes,
            operation_count: &mut operation_count,
            report: &mut report,
            limits,
        };
        apply_group_by_bundle_name_directories(
            workspace,
            objects,
            collections,
            bundle_plan,
            &mut state,
        )?;
    }

    export_paths
        .records
        .sort_unstable_by_key(|record| (record.file_index, record.object_index));
    report.metadata_record_count = export_paths.records.len();
    report.metadata_string_bytes = metadata_bytes;
    Ok((export_paths, report))
}

fn apply_group_by_bundle_name_directories(
    workspace: &SerializedFileWorkspace,
    objects: &ObjectWorkspace,
    collections: &WorkspaceAssetCollections,
    bundle_plan: &WorkspaceAssetBundleNamePlan,
    state: &mut WorkspaceAssetBundlePathApplyState<'_>,
) -> Result<(), WorkspaceAssetBundlePathError> {
    let mut collection_index_by_file = Vec::new();
    collection_index_by_file
        .try_reserve_exact(workspace.files().len())
        .map_err(|_| WorkspaceAssetBundlePathError::AllocationFailed {
            resource: "collection identity index",
            requested: workspace.files().len(),
        })?;
    collection_index_by_file.resize(workspace.files().len(), None::<usize>);
    for (index, collection) in collections.collections().iter().enumerate() {
        let slot = collection_index_by_file
            .get_mut(collection.source_file_index)
            .ok_or(WorkspaceAssetBundlePathError::WorkspaceIndexOutOfBounds {
                resource: "source collection file",
                index: collection.source_file_index,
                count: workspace.files().len(),
            })?;
        if slot.replace(index).is_some() {
            return Err(WorkspaceAssetBundlePathError::InvalidWorkspaceIdentity {
                resource: "duplicate collection file",
                index: collection.source_file_index,
            });
        }
    }

    let mut bundle_for_collection = Vec::new();
    bundle_for_collection
        .try_reserve_exact(collections.collections().len())
        .map_err(|_| WorkspaceAssetBundlePathError::AllocationFailed {
            resource: "bundle-name collection map",
            requested: collections.collections().len(),
        })?;
    bundle_for_collection.resize(collections.collections().len(), None::<usize>);
    for (bundle_record_index, bundle_record) in bundle_plan.bundles.iter().enumerate() {
        let source_file = workspace
            .files()
            .get(bundle_record.source_file_index)
            .ok_or(WorkspaceAssetBundlePathError::WorkspaceIndexOutOfBounds {
                resource: "AssetBundle source file",
                index: bundle_record.source_file_index,
                count: workspace.files().len(),
            })?;
        let Some(owner_bundle_index) = source_file.container_bundle_index else {
            continue;
        };
        let owner_bundle = workspace
            .container_bundles()
            .get(owner_bundle_index)
            .ok_or(WorkspaceAssetBundlePathError::WorkspaceIndexOutOfBounds {
                resource: "owner Bundle",
                index: owner_bundle_index,
                count: workspace.container_bundles().len(),
            })?;
        for &file_index in &owner_bundle.direct_collection_file_indices {
            let collection_index = collection_index_by_file
                .get(file_index)
                .ok_or(WorkspaceAssetBundlePathError::WorkspaceIndexOutOfBounds {
                    resource: "Bundle collection file",
                    index: file_index,
                    count: collection_index_by_file.len(),
                })?
                .ok_or(WorkspaceAssetBundlePathError::InvalidWorkspaceIdentity {
                    resource: "Bundle collection missing from asset collections",
                    index: file_index,
                })?;
            let collection = collections.collections().get(collection_index).ok_or(
                WorkspaceAssetBundlePathError::WorkspaceIndexOutOfBounds {
                    resource: "asset collection index",
                    index: collection_index,
                    count: collections.collections().len(),
                },
            )?;
            if collection.container_bundle_index != Some(owner_bundle_index) {
                return Err(WorkspaceAssetBundlePathError::InvalidWorkspaceIdentity {
                    resource: "Bundle collection owner mismatch",
                    index: file_index,
                });
            }
            bundle_for_collection[collection_index] = Some(bundle_record_index);
        }
    }

    for (collection_index, bundle_record_index) in bundle_for_collection.into_iter().enumerate() {
        let Some(bundle_record_index) = bundle_record_index else {
            continue;
        };
        let bundle = bundle_plan.bundles.get(bundle_record_index).ok_or(
            WorkspaceAssetBundlePathError::InvalidNamePlan {
                resource: "collection Bundle record index",
                index: bundle_record_index,
            },
        )?;
        let collection = collections.collections().get(collection_index).ok_or(
            WorkspaceAssetBundlePathError::WorkspaceIndexOutOfBounds {
                resource: "asset collection index",
                index: collection_index,
                count: collections.collections().len(),
            },
        )?;
        let source_objects = objects
            .objects()
            .get(collection.object_range.clone())
            .ok_or(WorkspaceAssetBundlePathError::ObjectRangeOutOfBounds {
                start: collection.object_range.start,
                end: collection.object_range.end,
                object_count: objects.objects().len(),
            })?;
        for object in source_objects {
            if object.file_index != collection.source_file_index {
                return Err(WorkspaceAssetBundlePathError::ObjectIdentityMismatch {
                    file_index: object.file_index,
                    object_index: object.object_index,
                });
            }
            let existing_index = state
                .record_index
                .get(&(object.file_index, object.object_index))
                .copied();
            if let Some(index) = existing_index {
                let existing = state
                    .records
                    .get(index)
                    .ok_or(WorkspaceAssetBundlePathError::InvalidMetadataIndex { index })?;
                if existing.original_directory.is_some() {
                    continue;
                }
            }
            let payload = object.payload.as_ref().map_err(|_| {
                WorkspaceAssetBundlePathError::ObjectPayloadUnavailable {
                    file_index: object.file_index,
                    object_index: object.object_index,
                }
            })?;
            if payload.type_name.is_empty() {
                return Err(WorkspaceAssetBundlePathError::ClassNameUnavailable {
                    file_index: object.file_index,
                    object_index: object.object_index,
                });
            }
            *state.operation_count =
                checked_path_operation_count(*state.operation_count, state.limits)?;
            let directory = join_project_path(&[
                "Assets/AssetBundles",
                &bundle.asset_name,
                &payload.type_name,
            ])?;
            let record_index_value = ensure_export_path_record(
                state.records,
                state.record_index,
                object.file_index,
                object.object_index,
                object.path_id,
                state.limits.max_records,
            )?;
            let old_record_bytes = export_path_record_bytes(&state.records[record_index_value])?;
            state.records[record_index_value].original_directory = Some(directory);
            state.records[record_index_value].original_path =
                calculated_original_path(&state.records[record_index_value])?;
            let new_record_bytes = export_path_record_bytes(&state.records[record_index_value])?;
            *state.metadata_bytes = (*state.metadata_bytes)
                .checked_sub(old_record_bytes)
                .and_then(|bytes| bytes.checked_add(new_record_bytes))
                .ok_or(WorkspaceAssetBundlePathError::MetadataByteCountOverflow)?;
            if *state.metadata_bytes > state.limits.max_total_metadata_bytes {
                return Err(WorkspaceAssetBundlePathError::MetadataByteLimitExceeded {
                    byte_count: *state.metadata_bytes,
                    limit: state.limits.max_total_metadata_bytes,
                });
            }
            state.report.assigned_collection_directories = state
                .report
                .assigned_collection_directories
                .checked_add(1)
                .ok_or(WorkspaceAssetBundlePathError::CountOverflow {
                    resource: "collection OriginalDirectories",
                })?;
        }
    }
    Ok(())
}

fn ensure_export_path_record(
    records: &mut Vec<WorkspaceAssetExportPath>,
    record_index: &mut HashMap<(usize, usize), usize>,
    file_index: usize,
    object_index: usize,
    path_id: i64,
    maximum_records: usize,
) -> Result<usize, WorkspaceAssetBundlePathError> {
    if let Some(index) = record_index.get(&(file_index, object_index)) {
        let record = records
            .get(*index)
            .ok_or(WorkspaceAssetBundlePathError::InvalidMetadataIndex { index: *index })?;
        if record.path_id != path_id {
            return Err(WorkspaceAssetBundlePathError::ObjectIdentityMismatch {
                file_index,
                object_index,
            });
        }
        return Ok(*index);
    }
    let requested =
        records
            .len()
            .checked_add(1)
            .ok_or(WorkspaceAssetBundlePathError::CountOverflow {
                resource: "export path metadata records",
            })?;
    if requested > maximum_records {
        return Err(WorkspaceAssetBundlePathError::CountLimitExceeded {
            resource: "export path metadata records",
            count: requested,
            limit: maximum_records,
        });
    }
    records
        .try_reserve(1)
        .map_err(|_| WorkspaceAssetBundlePathError::AllocationFailed {
            resource: "export path metadata records",
            requested,
        })?;
    record_index
        .try_reserve(1)
        .map_err(|_| WorkspaceAssetBundlePathError::AllocationFailed {
            resource: "export path metadata identity index",
            requested,
        })?;
    let index = records.len();
    records.push(WorkspaceAssetExportPath {
        file_index,
        object_index,
        path_id,
        ..WorkspaceAssetExportPath::default()
    });
    record_index.insert((file_index, object_index), index);
    Ok(index)
}

fn checked_path_operation_count(
    count: usize,
    limits: WorkspaceAssetExportPathLimits,
) -> Result<usize, WorkspaceAssetBundlePathError> {
    let requested = count
        .checked_add(1)
        .ok_or(WorkspaceAssetBundlePathError::CountOverflow {
            resource: "AssetBundle path operations",
        })?;
    if requested > limits.max_total_operations {
        return Err(WorkspaceAssetBundlePathError::CountLimitExceeded {
            resource: "AssetBundle path operations",
            count: requested,
            limit: limits.max_total_operations,
        });
    }
    Ok(requested)
}

fn export_path_record_bytes(
    record: &WorkspaceAssetExportPath,
) -> Result<usize, WorkspaceAssetBundlePathError> {
    optional_string_bytes(record.asset_bundle_name.as_ref())
        .checked_add(optional_string_bytes(record.original_path.as_ref()))
        .and_then(|bytes| {
            bytes.checked_add(optional_string_bytes(record.original_directory.as_ref()))
        })
        .and_then(|bytes| bytes.checked_add(optional_string_bytes(record.original_name.as_ref())))
        .and_then(|bytes| {
            bytes.checked_add(optional_string_bytes(record.original_extension.as_ref()))
        })
        .and_then(|bytes| {
            bytes.checked_add(optional_string_bytes(record.override_directory.as_ref()))
        })
        .and_then(|bytes| bytes.checked_add(optional_string_bytes(record.override_name.as_ref())))
        .and_then(|bytes| {
            bytes.checked_add(optional_string_bytes(record.override_extension.as_ref()))
        })
        .ok_or(WorkspaceAssetBundlePathError::MetadataByteCountOverflow)
}

fn export_path_metadata_bytes(
    records: &[WorkspaceAssetExportPath],
) -> Result<usize, WorkspaceAssetBundlePathError> {
    records.iter().try_fold(0usize, |total, record| {
        total
            .checked_add(export_path_record_bytes(record)?)
            .ok_or(WorkspaceAssetBundlePathError::MetadataByteCountOverflow)
    })
}

fn optional_string_bytes(value: Option<&String>) -> usize {
    value.map_or(0, String::len)
}

fn try_copy_export_path_string(value: &str) -> Result<String, WorkspaceAssetBundlePathError> {
    let mut copy = String::new();
    copy.try_reserve_exact(value.len()).map_err(|_| {
        WorkspaceAssetBundlePathError::AllocationFailed {
            resource: "export path metadata string",
            requested: value.len(),
        }
    })?;
    copy.push_str(value);
    Ok(copy)
}

fn nonempty_string(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

fn normalize_path_extension(value: Option<String>) -> Option<String> {
    value.and_then(|mut value| {
        if value.starts_with('.') {
            value.remove(0);
        }
        nonempty_string(Some(value))
    })
}

fn original_path_details(path: &str) -> Result<OriginalPathDetails, WorkspaceAssetBundlePathError> {
    let details = original_path_detail_slices(path);
    Ok(OriginalPathDetails {
        directory: details
            .directory
            .map(try_copy_export_path_string)
            .transpose()?,
        name: details.name.map(try_copy_export_path_string).transpose()?,
        extension: details
            .extension
            .map(try_copy_export_path_string)
            .transpose()?,
    })
}

fn original_path_detail_slices(path: &str) -> OriginalPathDetailSlices<'_> {
    let path_value = std::path::Path::new(path);
    let directory = path_value
        .parent()
        .and_then(std::path::Path::to_str)
        .filter(|value| !value.is_empty());
    let ends_with_separator = path.ends_with('/') || (cfg!(windows) && path.ends_with('\\'));
    let name = if ends_with_separator {
        None
    } else {
        path_value
            .file_stem()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
    };
    let extension = if ends_with_separator {
        None
    } else {
        path_value
            .extension()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
    };
    OriginalPathDetailSlices {
        directory,
        name,
        extension,
    }
}

fn calculated_original_path(
    record: &WorkspaceAssetExportPath,
) -> Result<Option<String>, WorkspaceAssetBundlePathError> {
    if record.original_directory.is_none()
        && record.original_name.is_none()
        && record.original_extension.is_none()
    {
        return Ok(None);
    }
    let extension_length = record
        .original_extension
        .as_ref()
        .map_or(Some(0usize), |extension| extension.len().checked_add(1))
        .ok_or(WorkspaceAssetBundlePathError::MetadataByteCountOverflow)?;
    let name_length = record
        .original_name
        .as_ref()
        .map_or(0, String::len)
        .checked_add(extension_length)
        .ok_or(WorkspaceAssetBundlePathError::MetadataByteCountOverflow)?;
    let mut name = String::new();
    name.try_reserve_exact(name_length).map_err(|_| {
        WorkspaceAssetBundlePathError::AllocationFailed {
            resource: "calculated OriginalPath filename",
            requested: name_length,
        }
    })?;
    if let Some(original_name) = record.original_name.as_deref() {
        name.push_str(original_name);
    }
    if let Some(extension) = record.original_extension.as_deref() {
        name.push('.');
        name.push_str(extension);
    }
    if let Some(directory) = record.original_directory.as_deref() {
        join_project_path(&[directory, &name]).map(Some)
    } else if name.is_empty() {
        Ok(None)
    } else {
        Ok(Some(name))
    }
}

fn resource_manager_path_should_replace(current_path: Option<&str>, candidate_path: &str) -> bool {
    current_path.is_none_or(|current_path| {
        current_path.encode_utf16().count() < candidate_path.encode_utf16().count()
    })
}

fn workspace_named_asset_name(
    object: &WorkspaceObject,
) -> Result<Option<String>, WorkspaceAssetBundlePathError> {
    let payload = object.payload.as_ref().map_err(|_| {
        WorkspaceAssetBundlePathError::ObjectPayloadUnavailable {
            file_index: object.file_index,
            object_index: object.object_index,
        }
    })?;
    let value = payload
        .root
        .unique_field_in_base_chain("m_Name")
        .map_err(
            |source| WorkspaceAssetBundlePathError::NamedAssetFieldLookup {
                file_index: object.file_index,
                object_index: object.object_index,
                source,
            },
        )?;
    let Some(SerializedValue::StringBytes(bytes)) = value else {
        return Ok(None);
    };
    let name = str::from_utf8(bytes).map_err(|error| {
        WorkspaceAssetBundlePathError::InvalidNamedAssetUtf8 {
            file_index: object.file_index,
            object_index: object.object_index,
            valid_up_to: error.valid_up_to(),
        }
    })?;
    try_copy_export_path_string(name).map(Some)
}

fn restore_original_name(
    record: &mut WorkspaceAssetExportPath,
    asset_name: &str,
    report: &mut WorkspaceAssetExportPathReport,
) -> Result<bool, WorkspaceAssetBundlePathError> {
    let Some(original_name) = record.original_name.as_deref() else {
        return Ok(false);
    };
    if original_name == asset_name {
        return Ok(true);
    }
    if original_name.encode_utf16().count() != asset_name.encode_utf16().count() {
        return Ok(false);
    }
    if original_name.is_ascii() && asset_name.is_ascii() {
        if !original_name.eq_ignore_ascii_case(asset_name) {
            return Ok(false);
        }
        record.original_name = Some(try_copy_export_path_string(asset_name)?);
        report.restored_original_names = report.restored_original_names.checked_add(1).ok_or(
            WorkspaceAssetBundlePathError::CountOverflow {
                resource: "restored OriginalNames",
            },
        )?;
        return Ok(true);
    }
    report.case_restoration_unverified = report.case_restoration_unverified.checked_add(1).ok_or(
        WorkspaceAssetBundlePathError::CountOverflow {
            resource: "unverified non-ASCII OriginalName comparisons",
        },
    )?;
    Ok(false)
}

fn initialize_shader_overrides(
    record: &mut WorkspaceAssetExportPath,
    report: &mut WorkspaceAssetExportPathReport,
) -> Result<(), WorkspaceAssetBundlePathError> {
    let mut initialized = false;
    if record.override_directory.is_none() {
        if let Some(value) = record.original_directory.as_deref() {
            record.override_directory = Some(try_copy_export_path_string(value)?);
            initialized = true;
        }
    }
    if record.override_name.is_none() {
        if let Some(value) = record.original_name.as_deref() {
            record.override_name = Some(try_copy_export_path_string(value)?);
            initialized = true;
        }
    }
    if record.override_extension.is_none() {
        if let Some(value) = record.original_extension.as_deref() {
            record.override_extension = Some(try_copy_export_path_string(value)?);
            initialized = true;
        }
    }
    if initialized {
        report.initialized_shader_overrides = report
            .initialized_shader_overrides
            .checked_add(1)
            .ok_or(WorkspaceAssetBundlePathError::CountOverflow {
                resource: "initialized Shader override records",
            })?;
    }
    Ok(())
}

fn apply_original_path_postprocessing(
    record: &mut WorkspaceAssetExportPath,
    path_to_set: Option<String>,
    named_asset_name: Option<&str>,
    is_shader: bool,
    report: &mut WorkspaceAssetExportPathReport,
) -> Result<(), WorkspaceAssetBundlePathError> {
    if let Some(path) = path_to_set {
        let details = original_path_details(&path)?;
        record.original_path = Some(path);
        record.original_directory = details.directory;
        record.original_name = details.name;
        record.original_extension = details.extension;
    }
    let mut path_details_changed = false;
    if let Some(asset_name) = named_asset_name {
        path_details_changed = restore_original_name(record, asset_name, report)?;
    }
    if is_shader {
        initialize_shader_overrides(record, report)?;
    }
    if path_details_changed {
        record.original_path = calculated_original_path(record)?;
    }
    Ok(())
}

fn bundle_container_original_path(
    container_path: &str,
    bundle_name: &str,
    mode: BundledAssetsExportMode,
) -> Result<Option<BundleOriginalPathEffect>, WorkspaceAssetBundlePathError> {
    let mut asset_path = ensure_path_not_rooted(container_path)?;
    if asset_path.is_empty() {
        return Ok(None);
    }
    match mode {
        BundledAssetsExportMode::GroupByAssetType => Ok(Some(BundleOriginalPathEffect {
            original_path: None,
        })),
        BundledAssetsExportMode::DirectExport => {
            ensure_starts_with_assets(&asset_path).map(|original_path| {
                Some(BundleOriginalPathEffect {
                    original_path: Some(original_path),
                })
            })
        }
        BundledAssetsExportMode::GroupByBundleName => {
            if let Some(relative_path) = strip_prefix_ignore_ascii_case(&asset_path, "Assets/") {
                asset_path = try_copy_export_path_string(relative_path)?;
            }
            let mut bundle_prefix = try_copy_export_path_string(bundle_name)?;
            bundle_prefix.try_reserve(1).map_err(|_| {
                WorkspaceAssetBundlePathError::AllocationFailed {
                    resource: "AssetBundle path prefix",
                    requested: bundle_prefix.len().saturating_add(1),
                }
            })?;
            bundle_prefix.push('/');
            if let Some(relative_path) = strip_prefix_ignore_ascii_case(&asset_path, &bundle_prefix)
            {
                asset_path = try_copy_export_path_string(relative_path)?;
            }
            join_project_path(&["Assets/AssetBundles", bundle_name, &asset_path]).map(
                |original_path| {
                    Some(BundleOriginalPathEffect {
                        original_path: Some(original_path),
                    })
                },
            )
        }
    }
}

fn ensure_path_not_rooted(path: &str) -> Result<String, WorkspaceAssetBundlePathError> {
    if !is_path_rooted_for_host(path) {
        return try_copy_export_path_string(path);
    }
    let mut start = 0usize;
    for segment in path.split('/') {
        if segment.eq_ignore_ascii_case("Assets") {
            return try_copy_export_path_string(&path[start..]);
        }
        start = start
            .checked_add(segment.len())
            .and_then(|value| value.checked_add(1))
            .ok_or(WorkspaceAssetBundlePathError::CountOverflow {
                resource: "AssetBundle path segment offset",
            })?;
    }
    Ok(String::new())
}

fn is_path_rooted_for_host(path: &str) -> bool {
    if std::path::Path::new(path).is_absolute() {
        return true;
    }
    if cfg!(windows) {
        let bytes = path.as_bytes();
        path.starts_with('/')
            || path.starts_with('\\')
            || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
    } else {
        false
    }
}

fn ensure_starts_with_assets(path: &str) -> Result<String, WorkspaceAssetBundlePathError> {
    if path.starts_with("Assets/") {
        return try_copy_export_path_string(path);
    }
    if let Some(suffix) = strip_prefix_ignore_ascii_case(path, "Assets/") {
        let mut result = try_copy_export_path_string("Assets/")?;
        result.try_reserve(suffix.len()).map_err(|_| {
            WorkspaceAssetBundlePathError::AllocationFailed {
                resource: "direct AssetBundle path",
                requested: result.len().saturating_add(suffix.len()),
            }
        })?;
        result.push_str(suffix);
        return Ok(result);
    }
    let mut result = try_copy_export_path_string("Assets/")?;
    result.try_reserve(path.len()).map_err(|_| {
        WorkspaceAssetBundlePathError::AllocationFailed {
            resource: "direct AssetBundle path",
            requested: result.len().saturating_add(path.len()),
        }
    })?;
    result.push_str(path);
    Ok(result)
}

fn strip_prefix_ignore_ascii_case<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    value
        .get(..prefix.len())
        .filter(|candidate| candidate.eq_ignore_ascii_case(prefix))
        .map(|_| &value[prefix.len()..])
}

fn join_project_path(parts: &[&str]) -> Result<String, WorkspaceAssetBundlePathError> {
    let separator = std::path::MAIN_SEPARATOR;
    let mut length = 0usize;
    let mut has_content = false;
    let mut previous_ended_with_separator = false;
    for part in parts.iter().copied().filter(|part| !part.is_empty()) {
        if has_content && !previous_ended_with_separator {
            length = length
                .checked_add(separator.len_utf8())
                .ok_or(WorkspaceAssetBundlePathError::MetadataByteCountOverflow)?;
        }
        length = length
            .checked_add(part.len())
            .ok_or(WorkspaceAssetBundlePathError::MetadataByteCountOverflow)?;
        has_content = true;
        previous_ended_with_separator = part.ends_with('/') || part.ends_with('\\');
    }
    let mut result = String::new();
    result.try_reserve_exact(length).map_err(|_| {
        WorkspaceAssetBundlePathError::AllocationFailed {
            resource: "joined AssetBundle path",
            requested: length,
        }
    })?;
    has_content = false;
    previous_ended_with_separator = false;
    for part in parts.iter().copied().filter(|part| !part.is_empty()) {
        if has_content && !previous_ended_with_separator {
            result.push(separator);
        }
        result.push_str(part);
        has_content = true;
        previous_ended_with_separator = part.ends_with('/') || part.ends_with('\\');
    }
    Ok(result)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceAssetBundlePathError {
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
    MetadataByteCountOverflow,
    MetadataByteLimitExceeded {
        byte_count: usize,
        limit: usize,
    },
    IncompleteNamePlan {
        issue_count: usize,
    },
    IncompleteResourceManagerPlan {
        issue_count: usize,
    },
    DuplicateMetadataIdentity {
        file_index: usize,
        object_index: usize,
    },
    InvalidNamePlan {
        resource: &'static str,
        index: usize,
    },
    InvalidMetadataIndex {
        index: usize,
    },
    WorkspaceIndexOutOfBounds {
        resource: &'static str,
        index: usize,
        count: usize,
    },
    InvalidWorkspaceIdentity {
        resource: &'static str,
        index: usize,
    },
    ObjectIdentityMismatch {
        file_index: usize,
        object_index: usize,
    },
    ObjectRangeOutOfBounds {
        start: usize,
        end: usize,
        object_count: usize,
    },
    ObjectPayloadUnavailable {
        file_index: usize,
        object_index: usize,
    },
    NamedAssetFieldLookup {
        file_index: usize,
        object_index: usize,
        source: SerializedValueLookupError,
    },
    InvalidNamedAssetUtf8 {
        file_index: usize,
        object_index: usize,
        valid_up_to: usize,
    },
    ClassNameUnavailable {
        file_index: usize,
        object_index: usize,
    },
}

impl Display for WorkspaceAssetBundlePathError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AllocationFailed {
                resource,
                requested,
            } => write!(
                formatter,
                "could not allocate {requested} entries or bytes for {resource}"
            ),
            Self::CountOverflow { resource } => write!(formatter, "{resource} count overflows"),
            Self::CountLimitExceeded {
                resource,
                count,
                limit,
            } => write!(
                formatter,
                "{resource} count {count} exceeds processing limit {limit}"
            ),
            Self::MetadataByteCountOverflow => {
                formatter.write_str("AssetBundle export metadata byte count overflows")
            }
            Self::MetadataByteLimitExceeded { byte_count, limit } => write!(
                formatter,
                "AssetBundle export metadata uses {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::IncompleteNamePlan { issue_count } => write!(
                formatter,
                "cannot apply AssetBundle paths from a name plan with {issue_count} unresolved issues"
            ),
            Self::IncompleteResourceManagerPlan { issue_count } => write!(
                formatter,
                "cannot apply ResourceManager paths from a plan with {issue_count} unresolved issues"
            ),
            Self::DuplicateMetadataIdentity {
                file_index,
                object_index,
            } => write!(
                formatter,
                "export path metadata contains duplicate object {file_index}:{object_index}"
            ),
            Self::InvalidNamePlan { resource, index } => {
                write!(formatter, "AssetBundle name plan has invalid {resource} {index}")
            }
            Self::InvalidMetadataIndex { index } => {
                write!(formatter, "export path metadata index {index} is invalid")
            }
            Self::WorkspaceIndexOutOfBounds {
                resource,
                index,
                count,
            } => write!(
                formatter,
                "{resource} index {index} is outside workspace count {count}"
            ),
            Self::InvalidWorkspaceIdentity { resource, index } => {
                write!(formatter, "workspace has invalid {resource} at index {index}")
            }
            Self::ObjectIdentityMismatch {
                file_index,
                object_index,
            } => write!(
                formatter,
                "workspace object identity mismatch at {file_index}:{object_index}"
            ),
            Self::ObjectRangeOutOfBounds {
                start,
                end,
                object_count,
            } => write!(
                formatter,
                "workspace object range {start}..{end} is outside {object_count} objects"
            ),
            Self::ObjectPayloadUnavailable {
                file_index,
                object_index,
            } => write!(
                formatter,
                "cannot determine ClassName for object {file_index}:{object_index} because its payload is undecoded"
            ),
            Self::NamedAssetFieldLookup {
                file_index,
                object_index,
                source,
            } => write!(
                formatter,
                "cannot determine whether object {file_index}:{object_index} implements INamed: {source}"
            ),
            Self::InvalidNamedAssetUtf8 {
                file_index,
                object_index,
                valid_up_to,
            } => write!(
                formatter,
                "object {file_index}:{object_index} has invalid UTF-8 in m_Name at byte {valid_up_to}"
            ),
            Self::ClassNameUnavailable {
                file_index,
                object_index,
            } => write!(
                formatter,
                "object {file_index}:{object_index} has no TypeTree ClassName for AssetBundle grouping"
            ),
        }
    }
}

impl Error for WorkspaceAssetBundlePathError {}

fn push_workspace_asset_bundle_issue(
    plan: &mut WorkspaceAssetBundleNamePlan,
    issue: WorkspaceAssetBundleNameIssue,
    limit: usize,
) -> Result<(), WorkspaceAssetBundleNamePlanError> {
    if plan.issues.len() >= limit {
        return Err(WorkspaceAssetBundleNamePlanError::IssueLimitExceeded {
            count: plan.issues.len().saturating_add(1),
            limit,
        });
    }
    plan.issues.try_reserve(1).map_err(|_| {
        WorkspaceAssetBundleNamePlanError::AllocationFailed {
            resource: "AssetBundle name issues",
            requested: plan.issues.len().saturating_add(1),
        }
    })?;
    plan.issues.push(issue);
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceAssetBundleNamePlanError {
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    FileIndexOutOfBounds {
        file_index: usize,
        file_count: usize,
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
    AssetBundleCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    ContainerEntryCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    MetadataByteLimitExceeded {
        byte_count: usize,
        limit: usize,
    },
    CountOverflow {
        resource: &'static str,
    },
    IssueLimitExceeded {
        count: usize,
        limit: usize,
    },
    TargetObjectIndexOutOfBounds {
        file_index: usize,
        object_index: usize,
    },
    TargetObjectIdentityMismatch {
        file_index: usize,
        object_index: usize,
        expected_path_id: i64,
        actual_path_id: i64,
    },
}

impl Display for WorkspaceAssetBundleNamePlanError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AllocationFailed { resource, requested } => write!(
                formatter,
                "could not allocate {requested} entries for {resource}"
            ),
            Self::FileIndexOutOfBounds {
                file_index,
                file_count,
            } => write!(
                formatter,
                "AssetBundle collection file index {file_index} is outside {file_count} files"
            ),
            Self::ObjectRangeOutOfBounds {
                start,
                end,
                object_count,
            } => write!(
                formatter,
                "AssetBundle collection object range {start}..{end} is outside {object_count} decoded objects"
            ),
            Self::ObjectFileIdentityMismatch {
                expected_file_index,
                actual_file_index,
            } => write!(
                formatter,
                "AssetBundle object belongs to file {actual_file_index}; collection belongs to file {expected_file_index}"
            ),
            Self::AssetBundleCountLimitExceeded { count, limit } => write!(
                formatter,
                "AssetBundle count {count} exceeds processing limit {limit}"
            ),
            Self::ContainerEntryCountLimitExceeded { count, limit } => write!(
                formatter,
                "AssetBundle Container entry count {count} exceeds processing limit {limit}"
            ),
            Self::MetadataByteLimitExceeded { byte_count, limit } => write!(
                formatter,
                "AssetBundle names and Container paths use {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::CountOverflow { resource } => {
                write!(formatter, "{resource} count or byte size overflows")
            }
            Self::IssueLimitExceeded { count, limit } => write!(
                formatter,
                "AssetBundle processing issue count {count} exceeds limit {limit}"
            ),
            Self::TargetObjectIndexOutOfBounds {
                file_index,
                object_index,
            } => write!(
                formatter,
                "AssetBundle local target object {file_index}:{object_index} is absent from the object workspace"
            ),
            Self::TargetObjectIdentityMismatch {
                file_index,
                object_index,
                expected_path_id,
                actual_path_id,
            } => write!(
                formatter,
                "AssetBundle target {file_index}:{object_index} has PathID {actual_path_id}, expected {expected_path_id}"
            ),
        }
    }
}

impl Error for WorkspaceAssetBundleNamePlanError {}

fn workspace_object_class_id(object: &WorkspaceObject) -> Option<i32> {
    object
        .class_id
        .map(i32::from)
        .or_else(|| object.resolved_type_index.map(|_| object.raw_type_id))
}

fn asset_bundle_container_target(resolution: PPtrResolution<'_>) -> AssetBundleContainerTarget {
    match resolution {
        PPtrResolution::Null => AssetBundleContainerTarget::Null,
        PPtrResolution::LocalObject { object_index } => {
            AssetBundleContainerTarget::LocalObject { object_index }
        }
        PPtrResolution::ExternalDependency {
            file_id,
            external_index,
            path_id,
            ..
        } => AssetBundleContainerTarget::SharedFile {
            file_id,
            external_index,
            path_id,
        },
        PPtrResolution::MissingLocalObject { path_id } => {
            AssetBundleContainerTarget::MissingLocalObject { path_id }
        }
        PPtrResolution::AmbiguousLocalObject {
            path_id,
            match_count,
        } => AssetBundleContainerTarget::AmbiguousLocalObject {
            path_id,
            match_count,
        },
        PPtrResolution::InvalidFileId {
            file_id,
            external_count,
        } => AssetBundleContainerTarget::InvalidFileId {
            file_id,
            external_count,
        },
    }
}

fn copy_name(value: &str, resource: &'static str) -> Result<String, AssetBundleNameDecodeError> {
    let mut name = String::new();
    name.try_reserve_exact(value.len()).map_err(|_| {
        AssetBundleNameDecodeError::AllocationFailed {
            resource,
            requested: value.len(),
        }
    })?;
    name.push_str(value);
    Ok(name)
}

fn value_type_name(value: &SerializedValue) -> &'static str {
    match value {
        SerializedValue::Bool(_) => "bool",
        SerializedValue::I8(_) => "i8",
        SerializedValue::U8(_) => "u8",
        SerializedValue::I16(_) => "i16",
        SerializedValue::U16(_) => "u16",
        SerializedValue::Char(_) => "char",
        SerializedValue::I32(_) => "i32",
        SerializedValue::U32(_) => "u32",
        SerializedValue::I64(_) => "i64",
        SerializedValue::U64(_) => "u64",
        SerializedValue::F32(_) => "f32",
        SerializedValue::F64(_) => "f64",
        SerializedValue::StringBytes(_) => "string",
        SerializedValue::Bytes(_) => "bytes",
        SerializedValue::Array(_) => "array",
        SerializedValue::Struct(_) => "struct",
        SerializedValue::PPtr { .. } => "PPtr",
        SerializedValue::Raw { .. } => "raw",
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssetBundleNameDecodeError {
    FieldLookup {
        field_name: &'static str,
        source: SerializedValueLookupError,
    },
    DuplicateAliases {
        first: &'static str,
        second: &'static str,
    },
    WrongType {
        field_name: &'static str,
        actual: &'static str,
    },
    InvalidUtf8 {
        field_name: &'static str,
        valid_up_to: usize,
    },
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
}

impl Display for AssetBundleNameDecodeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::FieldLookup { field_name, source } => {
                write!(
                    formatter,
                    "AssetBundle {field_name} lookup failed: {source}"
                )
            }
            Self::DuplicateAliases { first, second } => write!(
                formatter,
                "AssetBundle serialized name has both {first} and {second} fields"
            ),
            Self::WrongType { field_name, actual } => write!(
                formatter,
                "AssetBundle {field_name} has type {actual}; expected string"
            ),
            Self::InvalidUtf8 {
                field_name,
                valid_up_to,
            } => write!(
                formatter,
                "AssetBundle {field_name} is not UTF-8 (valid through byte {valid_up_to})"
            ),
            Self::AllocationFailed {
                resource,
                requested,
            } => write!(
                formatter,
                "could not allocate {requested} bytes for {resource}"
            ),
        }
    }
}

impl Error for AssetBundleNameDecodeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::FieldLookup { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssetBundleContainerMapError {
    Name(AssetBundleNameDecodeError),
    Container(AssetBundleContainerDecodeError),
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
}

impl Display for AssetBundleContainerMapError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Name(source) => Display::fmt(source, formatter),
            Self::Container(source) => Display::fmt(source, formatter),
            Self::AllocationFailed {
                resource,
                requested,
            } => write!(
                formatter,
                "could not allocate {requested} entries for {resource}"
            ),
        }
    }
}

impl Error for AssetBundleContainerMapError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Name(source) => Some(source),
            Self::Container(source) => Some(source),
            Self::AllocationFailed { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        asset_bundle_container_target, asset_bundle_name_for_asset, bundle_container_original_path,
        calculated_original_path, get_asset_bundle_name, initialize_shader_overrides,
        original_path_details, resource_manager_path_should_replace, restore_original_name,
        sort_workspace_original_path_events, AssetBundleContainerTarget,
        AssetBundleNameDecodeError, BundleOriginalPathEffect, BundledAssetsExportMode,
        OriginalPathDetails, WorkspaceAssetExportPath, WorkspaceAssetExportPathReport,
        WorkspaceOriginalPathEvent, WorkspaceOriginalPathEventKind,
    };
    use crate::serialized_file::object::{SerializedFieldValue, SerializedValue};
    use crate::serialized_file::references::PPtrResolution;

    fn field(name: &str, value: SerializedValue) -> SerializedFieldValue {
        SerializedFieldValue {
            name: name.to_owned(),
            value,
        }
    }

    fn root(fields: Vec<SerializedFieldValue>) -> SerializedValue {
        SerializedValue::Struct(fields)
    }

    #[test]
    fn asset_bundle_name_prefers_nonempty_serialized_value_then_uses_bundle_name() {
        assert_eq!(
            get_asset_bundle_name(
                &root(vec![field(
                    "m_AssetBundleName_R",
                    SerializedValue::StringBytes(b"custom.bundle".to_vec()),
                )]),
                "source.bundle",
            )
            .unwrap(),
            "custom.bundle"
        );
        assert_eq!(
            get_asset_bundle_name(
                &root(vec![field(
                    "AssetBundleName_R",
                    SerializedValue::StringBytes(Vec::new()),
                )]),
                "source.bundle",
            )
            .unwrap(),
            "source.bundle"
        );
        assert_eq!(
            get_asset_bundle_name(&root(Vec::new()), "source.bundle").unwrap(),
            "source.bundle"
        );
    }

    #[test]
    fn asset_bundle_name_rejects_conflicting_aliases_and_malformed_values() {
        let conflict = root(vec![
            field(
                "m_AssetBundleName_R",
                SerializedValue::StringBytes(b"one".to_vec()),
            ),
            field(
                "AssetBundleName_R",
                SerializedValue::StringBytes(b"two".to_vec()),
            ),
        ]);
        assert!(matches!(
            get_asset_bundle_name(&conflict, "fallback"),
            Err(AssetBundleNameDecodeError::DuplicateAliases { .. })
        ));
        assert!(matches!(
            get_asset_bundle_name(
                &root(vec![field("m_AssetBundleName_R", SerializedValue::I32(3))]),
                "fallback",
            ),
            Err(AssetBundleNameDecodeError::WrongType { .. })
        ));
        assert!(matches!(
            get_asset_bundle_name(
                &root(vec![field(
                    "m_AssetBundleName_R",
                    SerializedValue::StringBytes(vec![0xff]),
                )]),
                "fallback",
            ),
            Err(AssetBundleNameDecodeError::InvalidUtf8 { .. })
        ));
    }

    #[test]
    fn asset_bundle_asset_name_removes_only_a_trailing_bundle_extension() {
        assert_eq!(
            asset_bundle_name_for_asset("avatars.bundle").unwrap(),
            "avatars"
        );
        assert_eq!(
            asset_bundle_name_for_asset("Avatars.BUNDLE").unwrap(),
            "Avatars"
        );
        assert_eq!(
            asset_bundle_name_for_asset("bundle.avatars").unwrap(),
            "bundle.avatars"
        );
    }

    #[test]
    fn asset_bundle_original_paths_follow_all_export_modes() {
        assert_eq!(
            bundle_container_original_path(
                "Textures/wood.mat",
                "Avatars",
                BundledAssetsExportMode::DirectExport,
            )
            .unwrap()
            .unwrap()
            .original_path
            .as_deref(),
            Some("Assets/Textures/wood.mat")
        );
        assert_eq!(
            bundle_container_original_path(
                "/export/assets/Textures/wood.mat",
                "Avatars",
                BundledAssetsExportMode::DirectExport,
            )
            .unwrap()
            .unwrap()
            .original_path
            .as_deref(),
            Some("Assets/Textures/wood.mat")
        );
        assert_eq!(
            bundle_container_original_path(
                "/export/Outside/wood.mat",
                "Avatars",
                BundledAssetsExportMode::DirectExport,
            )
            .unwrap(),
            None
        );
        let expected_group_path = format!(
            "Assets/AssetBundles{}Avatars{}Textures/wood.mat",
            std::path::MAIN_SEPARATOR,
            std::path::MAIN_SEPARATOR,
        );
        assert_eq!(
            bundle_container_original_path(
                "assets/aVaTaRs/Textures/wood.mat",
                "Avatars",
                BundledAssetsExportMode::GroupByBundleName,
            )
            .unwrap(),
            Some(BundleOriginalPathEffect {
                original_path: Some(expected_group_path),
            })
        );
        assert_eq!(
            bundle_container_original_path(
                "Textures/wood.mat",
                "Avatars",
                BundledAssetsExportMode::GroupByAssetType,
            )
            .unwrap(),
            Some(BundleOriginalPathEffect {
                original_path: None,
            })
        );
    }

    #[test]
    fn original_path_details_split_directory_name_and_extension() {
        assert_eq!(
            original_path_details("Assets/Textures/Wood.Albedo.PNG").unwrap(),
            OriginalPathDetails {
                directory: Some("Assets/Textures".to_owned()),
                name: Some("Wood.Albedo".to_owned()),
                extension: Some("PNG".to_owned()),
            }
        );
        assert_eq!(
            original_path_details("Assets/Textures/README").unwrap(),
            OriginalPathDetails {
                directory: Some("Assets/Textures".to_owned()),
                name: Some("README".to_owned()),
                extension: None,
            }
        );
    }

    #[test]
    fn original_name_restoration_matches_ascii_case_and_utf16_length() {
        let mut record = WorkspaceAssetExportPath {
            original_path: Some("Assets/textures/wood.png".to_owned()),
            original_directory: Some("Assets/textures".to_owned()),
            original_name: Some("wood".to_owned()),
            original_extension: Some("png".to_owned()),
            ..WorkspaceAssetExportPath::default()
        };
        let mut report = WorkspaceAssetExportPathReport::default();
        assert!(restore_original_name(&mut record, "Wood", &mut report).unwrap());
        assert_eq!(record.original_name.as_deref(), Some("Wood"));
        record.original_path = calculated_original_path(&record).unwrap();
        assert_eq!(
            record.original_path.as_deref(),
            Some(format!("Assets/textures{}Wood.png", std::path::MAIN_SEPARATOR).as_str())
        );
        assert_eq!(report.restored_original_names, 1);

        assert!(!restore_original_name(&mut record, "Longer", &mut report).unwrap());
        assert_eq!(report.restored_original_names, 1);
    }

    #[test]
    fn original_name_restoration_reports_non_ascii_case_as_unverified() {
        let mut record = WorkspaceAssetExportPath {
            original_name: Some("über".to_owned()),
            ..WorkspaceAssetExportPath::default()
        };
        let mut report = WorkspaceAssetExportPathReport::default();
        assert!(!restore_original_name(&mut record, "Über", &mut report).unwrap());
        assert_eq!(record.original_name.as_deref(), Some("über"));
        assert_eq!(report.case_restoration_unverified, 1);
    }

    #[test]
    fn shader_override_initialization_preserves_existing_values() {
        let mut record = WorkspaceAssetExportPath {
            original_directory: Some("Assets/Shaders".to_owned()),
            original_name: Some("Surface".to_owned()),
            original_extension: Some("shader".to_owned()),
            override_name: Some("CustomName".to_owned()),
            ..WorkspaceAssetExportPath::default()
        };
        let mut report = WorkspaceAssetExportPathReport::default();
        initialize_shader_overrides(&mut record, &mut report).unwrap();
        assert_eq!(record.override_directory.as_deref(), Some("Assets/Shaders"));
        assert_eq!(record.override_name.as_deref(), Some("CustomName"));
        assert_eq!(record.override_extension.as_deref(), Some("shader"));
        assert_eq!(report.initialized_shader_overrides, 1);

        initialize_shader_overrides(&mut record, &mut report).unwrap();
        assert_eq!(report.initialized_shader_overrides, 1);
    }

    #[test]
    fn resource_manager_path_precedence_uses_utf16_length_and_keeps_first_tie() {
        assert!(resource_manager_path_should_replace(
            None,
            "Assets/Resources/wood"
        ));
        assert!(!resource_manager_path_should_replace(
            Some("Assets/Resources/é"),
            "Assets/Resources/x"
        ));
        assert!(resource_manager_path_should_replace(
            Some("Assets/Resources/wood"),
            "Assets/Resources/inner/wood"
        ));
    }

    #[test]
    fn original_path_events_follow_collection_object_and_container_order() {
        let mut events = [
            WorkspaceOriginalPathEvent {
                collection_index: 1,
                source_object_index: 0,
                container_entry_index: 0,
                kind: WorkspaceOriginalPathEventKind::AssetBundle(0),
            },
            WorkspaceOriginalPathEvent {
                collection_index: 0,
                source_object_index: 3,
                container_entry_index: 1,
                kind: WorkspaceOriginalPathEventKind::ResourceManager(0),
            },
            WorkspaceOriginalPathEvent {
                collection_index: 0,
                source_object_index: 2,
                container_entry_index: 0,
                kind: WorkspaceOriginalPathEventKind::AssetBundle(1),
            },
            WorkspaceOriginalPathEvent {
                collection_index: 0,
                source_object_index: 3,
                container_entry_index: 0,
                kind: WorkspaceOriginalPathEventKind::ResourceManager(1),
            },
        ];

        sort_workspace_original_path_events(&mut events);

        assert_eq!(
            events.map(|event| event.kind),
            [
                WorkspaceOriginalPathEventKind::AssetBundle(1),
                WorkspaceOriginalPathEventKind::ResourceManager(1),
                WorkspaceOriginalPathEventKind::ResourceManager(0),
                WorkspaceOriginalPathEventKind::AssetBundle(0),
            ]
        );
    }

    #[test]
    fn container_pointer_resolution_keeps_null_local_shared_and_invalid_distinct() {
        assert_eq!(
            asset_bundle_container_target(PPtrResolution::Null),
            AssetBundleContainerTarget::Null
        );
        assert_eq!(
            asset_bundle_container_target(PPtrResolution::LocalObject { object_index: 4 }),
            AssetBundleContainerTarget::LocalObject { object_index: 4 }
        );
        assert_eq!(
            asset_bundle_container_target(PPtrResolution::ExternalDependency {
                file_id: 2,
                external_index: 1,
                path_id: 9,
                identifier: &crate::serialized_file::metadata::FileIdentifier {
                    asset_path: None,
                    guid: None,
                    file_type: None,
                    path_name_origin: String::new(),
                },
            }),
            AssetBundleContainerTarget::SharedFile {
                file_id: 2,
                external_index: 1,
                path_id: 9,
            }
        );
        assert_eq!(
            asset_bundle_container_target(PPtrResolution::InvalidFileId {
                file_id: -1,
                external_count: 0,
            }),
            AssetBundleContainerTarget::InvalidFileId {
                file_id: -1,
                external_count: 0,
            }
        );
    }
}
