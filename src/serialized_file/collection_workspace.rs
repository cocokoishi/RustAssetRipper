//! Source-ordered collection records linking SerializedFiles to decoded workspace objects.

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::ops::Range;

use super::object_workspace::ObjectWorkspace;
use super::workspace::{SerializedFileWorkspace, SerializedFileWorkspaceError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceCollectionLimits {
    pub max_collections: usize,
    pub max_objects: usize,
    pub max_external_identifiers: usize,
    pub max_total_collection_name_bytes: usize,
}

impl Default for WorkspaceCollectionLimits {
    fn default() -> Self {
        Self {
            max_collections: 1_000_000,
            max_objects: 20_000_000,
            max_external_identifiers: 5_000_000,
            max_total_collection_name_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceCollectionReport {
    pub collection_count: usize,
    pub object_count: usize,
    pub external_identifier_count: usize,
    pub total_fixed_name_bytes: usize,
}

/// A SerializedFile collection's stable workspace identity and its object slice.
///
/// This records the source `SerializedAssetCollection` boundary and metadata links. It does not
/// materialize generated `IUnityObjectBase` instances or initialize Bundle dependencies.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceAssetCollection {
    /// Position in `SerializedFileWorkspace::source_collection_file_indices()` order.
    pub collection_index: usize,
    pub source_file_index: usize,
    pub fixed_name: String,
    /// Range into `ObjectWorkspace::objects()`; objects remain in workspace file order.
    pub object_range: Range<usize>,
    pub container_bundle_index: Option<usize>,
    pub container_collection_index: Option<usize>,
    pub external_identifier_count: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceAssetCollections {
    collections: Vec<WorkspaceAssetCollection>,
    collection_index_by_file: Vec<Option<usize>>,
    report: WorkspaceCollectionReport,
}

impl WorkspaceAssetCollections {
    pub fn build(
        workspace: &SerializedFileWorkspace,
        objects: &ObjectWorkspace,
        limits: WorkspaceCollectionLimits,
    ) -> Result<Self, Box<WorkspaceCollectionError>> {
        let mut object_ranges = Vec::new();
        object_ranges
            .try_reserve_exact(workspace.files().len())
            .map_err(|_| {
                collection_error(WorkspaceCollectionErrorKind::AllocationFailed {
                    resource: "per-file object ranges",
                    requested: workspace.files().len(),
                })
            })?;

        let mut expected_object_count = 0usize;
        for file in workspace.files() {
            let start = expected_object_count;
            expected_object_count = expected_object_count
                .checked_add(file.metadata.objects.len())
                .ok_or_else(|| {
                    collection_error(WorkspaceCollectionErrorKind::CountOverflow {
                        resource: "workspace objects",
                    })
                })?;
            object_ranges.push(start..expected_object_count);
        }
        if expected_object_count != objects.objects().len()
            || expected_object_count != objects.report().object_count
        {
            return Err(collection_error(
                WorkspaceCollectionErrorKind::ObjectCountMismatch {
                    workspace_count: expected_object_count,
                    decoded_count: objects.objects().len(),
                    reported_count: objects.report().object_count,
                },
            ));
        }
        if expected_object_count > limits.max_objects {
            return Err(collection_error(
                WorkspaceCollectionErrorKind::ObjectCountLimitExceeded {
                    count: expected_object_count,
                    limit: limits.max_objects,
                },
            ));
        }

        for (file_index, range) in object_ranges.iter().enumerate() {
            let source_file = workspace.files().get(file_index).ok_or_else(|| {
                collection_error(WorkspaceCollectionErrorKind::FileIndexOutOfBounds {
                    file_index,
                    file_count: workspace.files().len(),
                })
            })?;
            for (object_index, object) in objects.objects()[range.clone()].iter().enumerate() {
                if object.file_index != file_index || object.object_index != object_index {
                    return Err(collection_error(
                        WorkspaceCollectionErrorKind::ObjectIdentityMismatch {
                            flat_index: range.start + object_index,
                            expected_file_index: file_index,
                            expected_object_index: object_index,
                            actual_file_index: object.file_index,
                            actual_object_index: object.object_index,
                        },
                    ));
                }
            }
            if source_file.metadata.objects.len() != range.len() {
                return Err(collection_error(
                    WorkspaceCollectionErrorKind::ObjectCountMismatch {
                        workspace_count: source_file.metadata.objects.len(),
                        decoded_count: range.len(),
                        reported_count: range.len(),
                    },
                ));
            }
        }

        let source_order = workspace
            .source_collection_file_indices()
            .map_err(|error| {
                collection_error(WorkspaceCollectionErrorKind::SourceTraversal(error))
            })?;
        if source_order.len() > limits.max_collections {
            return Err(collection_error(
                WorkspaceCollectionErrorKind::CollectionCountLimitExceeded {
                    count: source_order.len(),
                    limit: limits.max_collections,
                },
            ));
        }

        let mut collection_index_by_file = Vec::new();
        collection_index_by_file
            .try_reserve_exact(workspace.files().len())
            .map_err(|_| {
                collection_error(WorkspaceCollectionErrorKind::AllocationFailed {
                    resource: "file-to-collection index",
                    requested: workspace.files().len(),
                })
            })?;
        collection_index_by_file.resize(workspace.files().len(), None);

        let mut collections = Vec::new();
        collections
            .try_reserve_exact(source_order.len())
            .map_err(|_| {
                collection_error(WorkspaceCollectionErrorKind::AllocationFailed {
                    resource: "workspace collection records",
                    requested: source_order.len(),
                })
            })?;
        let mut report = WorkspaceCollectionReport {
            collection_count: source_order.len(),
            object_count: expected_object_count,
            ..WorkspaceCollectionReport::default()
        };

        for (collection_index, file_index) in source_order.into_iter().enumerate() {
            let file = workspace.files().get(file_index).ok_or_else(|| {
                collection_error(WorkspaceCollectionErrorKind::FileIndexOutOfBounds {
                    file_index,
                    file_count: workspace.files().len(),
                })
            })?;
            let collection_count = collection_index_by_file.len();
            let assigned = collection_index_by_file
                .get_mut(file_index)
                .ok_or_else(|| {
                    collection_error(WorkspaceCollectionErrorKind::FileIndexOutOfBounds {
                        file_index,
                        file_count: collection_count,
                    })
                })?;
            if assigned.replace(collection_index).is_some() {
                return Err(collection_error(
                    WorkspaceCollectionErrorKind::DuplicateFileCollection { file_index },
                ));
            }

            let fixed_name = crate::io::special_names::fix_file_identifier(&file.file_name);
            report.total_fixed_name_bytes = report
                .total_fixed_name_bytes
                .checked_add(fixed_name.len())
                .ok_or_else(|| {
                    collection_error(WorkspaceCollectionErrorKind::CountOverflow {
                        resource: "fixed collection names",
                    })
                })?;
            if report.total_fixed_name_bytes > limits.max_total_collection_name_bytes {
                return Err(collection_error(
                    WorkspaceCollectionErrorKind::CollectionNameByteLimitExceeded {
                        byte_count: report.total_fixed_name_bytes,
                        limit: limits.max_total_collection_name_bytes,
                    },
                ));
            }
            report.external_identifier_count = report
                .external_identifier_count
                .checked_add(file.metadata.externals.len())
                .ok_or_else(|| {
                    collection_error(WorkspaceCollectionErrorKind::CountOverflow {
                        resource: "external identifiers",
                    })
                })?;
            if report.external_identifier_count > limits.max_external_identifiers {
                return Err(collection_error(
                    WorkspaceCollectionErrorKind::ExternalIdentifierLimitExceeded {
                        count: report.external_identifier_count,
                        limit: limits.max_external_identifiers,
                    },
                ));
            }

            let range = object_ranges.get(file_index).cloned().ok_or_else(|| {
                collection_error(WorkspaceCollectionErrorKind::FileIndexOutOfBounds {
                    file_index,
                    file_count: object_ranges.len(),
                })
            })?;
            collections.push(WorkspaceAssetCollection {
                collection_index,
                source_file_index: file_index,
                fixed_name,
                object_range: range,
                container_bundle_index: file.container_bundle_index,
                container_collection_index: file.container_collection_index,
                external_identifier_count: file.metadata.externals.len(),
            });
        }

        Ok(Self {
            collections,
            collection_index_by_file,
            report,
        })
    }

    pub fn collections(&self) -> &[WorkspaceAssetCollection] {
        &self.collections
    }

    pub fn collection_index_for_file(&self, file_index: usize) -> Option<usize> {
        self.collection_index_by_file
            .get(file_index)
            .copied()
            .flatten()
    }

    pub fn report(&self) -> WorkspaceCollectionReport {
        self.report
    }
}

#[derive(Debug)]
pub struct WorkspaceCollectionError {
    pub kind: WorkspaceCollectionErrorKind,
}

#[derive(Debug)]
pub enum WorkspaceCollectionErrorKind {
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    CountOverflow {
        resource: &'static str,
    },
    CollectionCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    ObjectCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    ExternalIdentifierLimitExceeded {
        count: usize,
        limit: usize,
    },
    CollectionNameByteLimitExceeded {
        byte_count: usize,
        limit: usize,
    },
    ObjectCountMismatch {
        workspace_count: usize,
        decoded_count: usize,
        reported_count: usize,
    },
    ObjectIdentityMismatch {
        flat_index: usize,
        expected_file_index: usize,
        expected_object_index: usize,
        actual_file_index: usize,
        actual_object_index: usize,
    },
    FileIndexOutOfBounds {
        file_index: usize,
        file_count: usize,
    },
    DuplicateFileCollection {
        file_index: usize,
    },
    SourceTraversal(Box<SerializedFileWorkspaceError>),
}

impl Display for WorkspaceCollectionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "workspace asset collection model: {}", self.kind)
    }
}

impl Display for WorkspaceCollectionErrorKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AllocationFailed { resource, requested } => {
                write!(f, "could not allocate {requested} entries for {resource}")
            }
            Self::CountOverflow { resource } => write!(f, "{resource} count overflows"),
            Self::CollectionCountLimitExceeded { count, limit } => write!(
                f,
                "workspace contains {count} collections, exceeding limit {limit}"
            ),
            Self::ObjectCountLimitExceeded { count, limit } => {
                write!(f, "workspace has {count} objects, exceeding limit {limit}")
            }
            Self::ExternalIdentifierLimitExceeded { count, limit } => write!(
                f,
                "workspace has {count} external identifiers, exceeding limit {limit}"
            ),
            Self::CollectionNameByteLimitExceeded { byte_count, limit } => write!(
                f,
                "fixed collection names use {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::ObjectCountMismatch {
                workspace_count,
                decoded_count,
                reported_count,
            } => write!(
                f,
                "object count mismatch: workspace has {workspace_count}, decoded records have {decoded_count}, decode report says {reported_count}"
            ),
            Self::ObjectIdentityMismatch {
                flat_index,
                expected_file_index,
                expected_object_index,
                actual_file_index,
                actual_object_index,
            } => write!(
                f,
                "object record {flat_index} has file/object identity {actual_file_index}/{actual_object_index}, expected {expected_file_index}/{expected_object_index}"
            ),
            Self::FileIndexOutOfBounds {
                file_index,
                file_count,
            } => write!(
                f,
                "file index {file_index} is outside the {file_count} workspace files"
            ),
            Self::DuplicateFileCollection { file_index } => {
                write!(f, "file {file_index} was assigned more than one collection index")
            }
            Self::SourceTraversal(error) => write!(f, "source collection traversal failed: {error}"),
        }
    }
}

impl Error for WorkspaceCollectionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            WorkspaceCollectionErrorKind::SourceTraversal(error) => Some(error.as_ref()),
            _ => None,
        }
    }
}

fn collection_error(kind: WorkspaceCollectionErrorKind) -> Box<WorkspaceCollectionError> {
    Box::new(WorkspaceCollectionError { kind })
}
