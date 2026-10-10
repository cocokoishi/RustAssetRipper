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

use crate::processing::scene_collections::{
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

pub(super) fn workspace_object_class_id(object: &WorkspaceObject) -> Option<i32> {
    object
        .class_id
        .map(i32::from)
        .or_else(|| object.resolved_type_index.map(|_| object.raw_type_id))
}

pub(super) fn asset_bundle_container_target(
    resolution: PPtrResolution<'_>,
) -> AssetBundleContainerTarget {
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
        SerializedValue::ManagedReferenceId { .. } => "managed reference ID",
        SerializedValue::ManagedReferenceRegistry(_) => "managed reference registry",
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
