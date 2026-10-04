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
use crate::serialized_file::reference_graph::ObjectReferenceGraph;
use crate::serialized_file::references::PPtrResolution;
use crate::serialized_file::workspace::SerializedFileWorkspace;

use super::scene_collections::{
    extract_asset_bundle_container_references, AssetBundleContainerDecodeError,
    AssetBundleContainerReference, SceneBundleMappingLimits,
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
        let byte_count = export_path_metadata_bytes(&records)?;
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
    pub assigned_collection_directories: usize,
    pub metadata_record_count: usize,
    pub metadata_string_bytes: usize,
}

struct WorkspaceAssetBundlePathApplyState<'a> {
    records: &'a mut Vec<WorkspaceAssetExportPath>,
    record_index: &'a mut HashMap<(usize, usize), usize>,
    metadata_bytes: &'a mut usize,
    operation_count: &'a mut usize,
    report: &'a mut WorkspaceAssetExportPathReport,
    limits: WorkspaceAssetExportPathLimits,
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
    mut export_paths: WorkspaceAssetExportPathTable,
    limits: WorkspaceAssetExportPathLimits,
) -> Result<
    (
        WorkspaceAssetExportPathTable,
        WorkspaceAssetExportPathReport,
    ),
    WorkspaceAssetBundlePathError,
> {
    if !bundle_plan.is_complete() {
        return Err(WorkspaceAssetBundlePathError::IncompleteNamePlan {
            issue_count: bundle_plan.issues.len(),
        });
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
    for assignment in &bundle_plan.assignments {
        operation_count = checked_path_operation_count(operation_count, limits)?;
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

        let original_path = bundle_container_original_path(
            &reference.mapping.reference.path,
            &bundle.asset_name,
            mode,
        )?;
        let original_directory = original_path
            .as_deref()
            .map(original_path_directory)
            .transpose()?
            .flatten();
        let bundle_name = try_copy_export_path_string(&bundle.asset_name)?;
        let index = ensure_export_path_record(
            &mut export_paths.records,
            &mut record_index,
            assignment.target_file_index,
            assignment.target_object_index,
            assignment.target_path_id,
            limits.max_records,
        )?;
        let current_record = &export_paths.records[index];
        let effective_path = original_path
            .as_ref()
            .or(current_record.original_path.as_ref());
        let effective_directory = if original_path.is_some() {
            original_directory.as_ref()
        } else {
            current_record.original_directory.as_ref()
        };
        let new_record_bytes = optional_string_bytes(Some(&bundle_name))
            .checked_add(optional_string_bytes(effective_path))
            .and_then(|bytes| bytes.checked_add(optional_string_bytes(effective_directory)))
            .ok_or(WorkspaceAssetBundlePathError::MetadataByteCountOverflow)?;
        let old_record_bytes = export_path_record_bytes(&export_paths.records[index])?;
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
        let record = &mut export_paths.records[index];
        record.asset_bundle_name = Some(bundle_name);
        if let Some(path) = original_path {
            record.original_path = Some(path);
            record.original_directory = original_directory;
            report.assigned_original_paths = report.assigned_original_paths.checked_add(1).ok_or(
                WorkspaceAssetBundlePathError::CountOverflow {
                    resource: "assigned OriginalPaths",
                },
            )?;
        }
        report.assigned_bundle_names = report.assigned_bundle_names.checked_add(1).ok_or(
            WorkspaceAssetBundlePathError::CountOverflow {
                resource: "assigned AssetBundleNames",
            },
        )?;
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
            let current_record = &state.records[record_index_value];
            let new_record_bytes = optional_string_bytes(current_record.asset_bundle_name.as_ref())
                .checked_add(optional_string_bytes(current_record.original_path.as_ref()))
                .and_then(|bytes| bytes.checked_add(directory.len()))
                .ok_or(WorkspaceAssetBundlePathError::MetadataByteCountOverflow)?;
            let old_record_bytes = export_path_record_bytes(&state.records[record_index_value])?;
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
            state.records[record_index_value].original_directory = Some(directory);
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

fn bundle_container_original_path(
    container_path: &str,
    bundle_name: &str,
    mode: BundledAssetsExportMode,
) -> Result<Option<String>, WorkspaceAssetBundlePathError> {
    let mut asset_path = ensure_path_not_rooted(container_path)?;
    if asset_path.is_empty() {
        return Ok(None);
    }
    match mode {
        BundledAssetsExportMode::GroupByAssetType => Ok(None),
        BundledAssetsExportMode::DirectExport => ensure_starts_with_assets(&asset_path).map(Some),
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
            join_project_path(&["Assets/AssetBundles", bundle_name, &asset_path]).map(Some)
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

fn original_path_directory(path: &str) -> Result<Option<String>, WorkspaceAssetBundlePathError> {
    let Some(separator_index) = path.rfind(['/', '\\']) else {
        return Ok(None);
    };
    if separator_index == 0 {
        return Ok(None);
    }
    try_copy_export_path_string(&path[..separator_index]).map(Some)
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
        get_asset_bundle_name, AssetBundleContainerTarget, AssetBundleNameDecodeError,
        BundledAssetsExportMode,
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
            Some(expected_group_path)
        );
        assert_eq!(
            bundle_container_original_path(
                "Textures/wood.mat",
                "Avatars",
                BundledAssetsExportMode::GroupByAssetType,
            )
            .unwrap(),
            None
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
