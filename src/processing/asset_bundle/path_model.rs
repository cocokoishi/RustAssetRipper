use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::serialized_file::object::SerializedValueLookupError;

use super::bundle_names::WorkspaceAssetBundleNamePlan;
use super::resource_manager::WorkspaceResourceManagerPathPlan;
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
    pub(super) records: Vec<WorkspaceAssetExportPath>,
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

#[derive(Clone, Copy, Debug)]
pub struct WorkspaceOriginalPathPlans<'a> {
    pub asset_bundles: &'a WorkspaceAssetBundleNamePlan,
    pub resource_managers: &'a WorkspaceResourceManagerPathPlan,
}

pub(super) struct OriginalPathDetailSlices<'a> {
    pub(super) directory: Option<&'a str>,
    pub(super) name: Option<&'a str>,
    pub(super) extension: Option<&'a str>,
}

pub(super) fn original_path_detail_slices(path: &str) -> OriginalPathDetailSlices<'_> {
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

pub(super) fn export_path_record_bytes(
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

pub(super) fn export_path_metadata_bytes(
    records: &[WorkspaceAssetExportPath],
) -> Result<usize, WorkspaceAssetBundlePathError> {
    records.iter().try_fold(0usize, |total, record| {
        total
            .checked_add(export_path_record_bytes(record)?)
            .ok_or(WorkspaceAssetBundlePathError::MetadataByteCountOverflow)
    })
}

pub(super) fn optional_string_bytes(value: Option<&String>) -> usize {
    value.map_or(0, String::len)
}

pub(super) fn try_copy_export_path_string(
    value: &str,
) -> Result<String, WorkspaceAssetBundlePathError> {
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

pub(super) fn nonempty_string(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.is_empty())
}

pub(super) fn normalize_path_extension(value: Option<String>) -> Option<String> {
    value.and_then(|mut value| {
        if value.starts_with('.') {
            value.remove(0);
        }
        nonempty_string(Some(value))
    })
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
