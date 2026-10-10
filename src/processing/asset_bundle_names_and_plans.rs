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
