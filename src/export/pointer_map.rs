//! Bounded mapping from resolved Unity object identities to project-export pointers.

use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::serialized_file::reference_graph::MultiFilePPtrResolution;

use super::ids::{get_main_export_id, ExportIdError};
use super::scene_collection::{ExportAssetIdentity, ExportAssetObjectKey};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ExportCollectionPointerMetadata {
    pub collection_index: usize,
    pub guid: [u8; 16],
    pub asset_type: UnityProjectAssetType,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum UnityProjectAssetType {
    Internal,
    Cached,
    Serialized,
    Meta,
}

impl UnityProjectAssetType {
    pub const fn as_unity_value(self) -> i32 {
        match self {
            Self::Internal => 0,
            Self::Cached => 1,
            Self::Serialized => 2,
            Self::Meta => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExportedAssetPointerEntry {
    pub asset: ExportAssetIdentity,
    pub file_id: i64,
    pub collection_index: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExportPointerMapLimits {
    pub max_assets: usize,
    pub max_collections: usize,
}

impl Default for ExportPointerMapLimits {
    fn default() -> Self {
        Self {
            max_assets: 20_000_000,
            max_collections: 1_000_000,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExportPointerMapError {
    AssetLimitExceeded {
        requested: usize,
        limit: usize,
    },
    CollectionLimitExceeded {
        requested: usize,
        limit: usize,
    },
    DuplicateAssetIdentity {
        asset: ExportAssetIdentity,
    },
    AssetIdentityMetadataMismatch {
        requested: ExportAssetIdentity,
        registered: ExportAssetIdentity,
    },
    DuplicateCollectionIdentity {
        collection_index: usize,
    },
    MissingCollectionMetadata {
        collection_index: usize,
    },
    InvalidUnresolvedReferenceStatus,
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    ExportId(ExportIdError),
}

impl Display for ExportPointerMapError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AssetLimitExceeded { requested, limit } => write!(
                f,
                "project export pointer map has {requested} assets, exceeding limit {limit}"
            ),
            Self::CollectionLimitExceeded { requested, limit } => write!(
                f,
                "project export pointer map has {requested} collections, exceeding limit {limit}"
            ),
            Self::DuplicateAssetIdentity { asset } => {
                write!(f, "duplicate project export asset identity {asset:?}")
            }
            Self::AssetIdentityMetadataMismatch {
                requested,
                registered,
            } => write!(
                f,
                "project export object key {requested:?} was requested with metadata different from registered identity {registered:?}"
            ),
            Self::DuplicateCollectionIdentity { collection_index } => write!(
                f,
                "duplicate project export collection identity {collection_index}"
            ),
            Self::MissingCollectionMetadata { collection_index } => write!(
                f,
                "project export asset references collection {collection_index} without metadata"
            ),
            Self::InvalidUnresolvedReferenceStatus => f.write_str(
                "an unresolved project pointer cannot carry a null or uniquely resolved status",
            ),
            Self::AllocationFailed { resource, requested } => {
                write!(f, "could not allocate {requested} entries for {resource}")
            }
            Self::ExportId(error) => Display::fmt(error, f),
        }
    }
}

impl Error for ExportPointerMapError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ExportId(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExportPointerGuid {
    None,
    Collection([u8; 16]),
    MissingReference,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnityMetaPointer {
    pub file_id: i64,
    pub guid: ExportPointerGuid,
    pub asset_type: UnityProjectAssetType,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnresolvedProjectPointer {
    pub source_file_index: usize,
    pub source_object_index: usize,
    pub source_path_id: i64,
    pub field_path: String,
    pub serialized_file_id: i32,
    pub serialized_path_id: i64,
    pub class_id: i32,
    pub resolution: MultiFilePPtrResolution,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProjectPointerRequest {
    Null,
    ResolvedAsset(ExportAssetIdentity),
    UnresolvedInput(UnresolvedProjectPointer),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProjectPointerResolution {
    Null,
    Mapped(UnityMetaPointer),
    MissingExportedTarget {
        asset: ExportAssetIdentity,
        pointer: UnityMetaPointer,
    },
    UnresolvedInput(UnresolvedProjectPointer),
}

/// Lookup table used after input PPtrs have been resolved to file-scoped object identities.
///
/// Missing export membership produces an explicit missing-reference pointer, matching the
/// baseline's `ProjectAssetContainer`. Failed or ambiguous input PPtr resolution remains a
/// separate result and is never converted to null.
pub struct ProjectExportPointerMap {
    assets: HashMap<ExportAssetObjectKey, ExportedAssetPointerEntry>,
    collections: HashMap<usize, ExportCollectionPointerMetadata>,
}

impl ProjectExportPointerMap {
    pub fn new(
        assets: &[ExportedAssetPointerEntry],
        collections: &[ExportCollectionPointerMetadata],
        limits: ExportPointerMapLimits,
    ) -> Result<Self, ExportPointerMapError> {
        if assets.len() > limits.max_assets {
            return Err(ExportPointerMapError::AssetLimitExceeded {
                requested: assets.len(),
                limit: limits.max_assets,
            });
        }
        if collections.len() > limits.max_collections {
            return Err(ExportPointerMapError::CollectionLimitExceeded {
                requested: collections.len(),
                limit: limits.max_collections,
            });
        }

        let mut collection_map = HashMap::new();
        collection_map.try_reserve(collections.len()).map_err(|_| {
            ExportPointerMapError::AllocationFailed {
                resource: "project export collection metadata",
                requested: collections.len(),
            }
        })?;
        for &collection in collections {
            if collection_map
                .insert(collection.collection_index, collection)
                .is_some()
            {
                return Err(ExportPointerMapError::DuplicateCollectionIdentity {
                    collection_index: collection.collection_index,
                });
            }
        }

        let mut asset_map = HashMap::new();
        asset_map.try_reserve(assets.len()).map_err(|_| {
            ExportPointerMapError::AllocationFailed {
                resource: "project export asset pointer map",
                requested: assets.len(),
            }
        })?;
        for &entry in assets {
            if !collection_map.contains_key(&entry.collection_index) {
                return Err(ExportPointerMapError::MissingCollectionMetadata {
                    collection_index: entry.collection_index,
                });
            }
            if asset_map.insert(entry.asset.object_key(), entry).is_some() {
                return Err(ExportPointerMapError::DuplicateAssetIdentity { asset: entry.asset });
            }
        }

        Ok(Self {
            assets: asset_map,
            collections: collection_map,
        })
    }

    pub fn resolve(
        &self,
        request: ProjectPointerRequest,
        current_collection_index: usize,
    ) -> Result<ProjectPointerResolution, ExportPointerMapError> {
        match request {
            ProjectPointerRequest::Null => Ok(ProjectPointerResolution::Null),
            ProjectPointerRequest::ResolvedAsset(asset) => {
                let Some(entry) = self.assets.get(&asset.object_key()) else {
                    return Ok(ProjectPointerResolution::MissingExportedTarget {
                        asset,
                        pointer: UnityMetaPointer {
                            file_id: get_main_export_id(asset.class_id, 0)
                                .map_err(ExportPointerMapError::ExportId)?,
                            guid: ExportPointerGuid::MissingReference,
                            asset_type: UnityProjectAssetType::Meta,
                        },
                    });
                };
                if entry.asset != asset {
                    return Err(ExportPointerMapError::AssetIdentityMetadataMismatch {
                        requested: asset,
                        registered: entry.asset,
                    });
                }
                let collection = self.collections.get(&entry.collection_index).ok_or(
                    ExportPointerMapError::MissingCollectionMetadata {
                        collection_index: entry.collection_index,
                    },
                )?;
                let pointer = if entry.collection_index == current_collection_index {
                    UnityMetaPointer {
                        file_id: entry.file_id,
                        guid: ExportPointerGuid::None,
                        asset_type: UnityProjectAssetType::Serialized,
                    }
                } else {
                    UnityMetaPointer {
                        file_id: entry.file_id,
                        guid: if collection.guid == [0; 16] {
                            ExportPointerGuid::None
                        } else {
                            ExportPointerGuid::Collection(collection.guid)
                        },
                        asset_type: collection.asset_type,
                    }
                };
                Ok(ProjectPointerResolution::Mapped(pointer))
            }
            ProjectPointerRequest::UnresolvedInput(reference) => {
                if matches!(
                    reference.resolution,
                    MultiFilePPtrResolution::Null | MultiFilePPtrResolution::Object { .. }
                ) {
                    return Err(ExportPointerMapError::InvalidUnresolvedReferenceStatus);
                }
                Ok(ProjectPointerResolution::UnresolvedInput(reference))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::scene_collection::ExportAssetCollectionIdentity;

    fn asset(
        file_index: usize,
        object_index: usize,
        path_id: i64,
        class_id: i32,
    ) -> ExportAssetIdentity {
        ExportAssetIdentity {
            collection: ExportAssetCollectionIdentity::SerializedFile { file_index },
            object_index,
            path_id,
            class_id,
        }
    }

    fn collection(
        collection_index: usize,
        guid_byte: u8,
        asset_type: UnityProjectAssetType,
    ) -> ExportCollectionPointerMetadata {
        ExportCollectionPointerMetadata {
            collection_index,
            guid: [guid_byte; 16],
            asset_type,
        }
    }

    fn entry(
        asset: ExportAssetIdentity,
        file_id: i64,
        collection_index: usize,
    ) -> ExportedAssetPointerEntry {
        ExportedAssetPointerEntry {
            asset,
            file_id,
            collection_index,
        }
    }

    fn map() -> ProjectExportPointerMap {
        ProjectExportPointerMap::new(
            &[
                entry(asset(0, 0, 10, 1), 10, 3),
                entry(asset(1, 0, 10, 1), 20, 4),
            ],
            &[
                collection(3, 0x33, UnityProjectAssetType::Serialized),
                collection(4, 0x44, UnityProjectAssetType::Meta),
            ],
            ExportPointerMapLimits::default(),
        )
        .unwrap()
    }

    #[test]
    fn project_pointer_map_distinguishes_null_local_and_external_pointers() {
        let map = map();
        let local = asset(0, 0, 10, 1);
        let external = asset(1, 0, 10, 1);

        assert_eq!(
            map.resolve(ProjectPointerRequest::Null, 3),
            Ok(ProjectPointerResolution::Null)
        );
        assert_eq!(
            map.resolve(ProjectPointerRequest::ResolvedAsset(local), 3),
            Ok(ProjectPointerResolution::Mapped(UnityMetaPointer {
                file_id: 10,
                guid: ExportPointerGuid::None,
                asset_type: UnityProjectAssetType::Serialized,
            }))
        );
        assert_eq!(
            map.resolve(ProjectPointerRequest::ResolvedAsset(external), 3),
            Ok(ProjectPointerResolution::Mapped(UnityMetaPointer {
                file_id: 20,
                guid: ExportPointerGuid::Collection([0x44; 16]),
                asset_type: UnityProjectAssetType::Meta,
            }))
        );
    }

    #[test]
    fn project_pointer_map_keeps_same_path_ids_in_distinct_source_files() {
        let map = map();
        assert_eq!(
            map.resolve(ProjectPointerRequest::ResolvedAsset(asset(0, 0, 10, 1)), 4),
            Ok(ProjectPointerResolution::Mapped(UnityMetaPointer {
                file_id: 10,
                guid: ExportPointerGuid::Collection([0x33; 16]),
                asset_type: UnityProjectAssetType::Serialized,
            }))
        );
    }

    #[test]
    fn project_pointer_map_preserves_missing_exports_and_unresolved_input_causes() {
        let map = map();
        let missing = asset(9, 8, 88, 4);
        assert_eq!(
            map.resolve(ProjectPointerRequest::ResolvedAsset(missing), 3),
            Ok(ProjectPointerResolution::MissingExportedTarget {
                asset: missing,
                pointer: UnityMetaPointer {
                    file_id: get_main_export_id(4, 0).unwrap(),
                    guid: ExportPointerGuid::MissingReference,
                    asset_type: UnityProjectAssetType::Meta,
                },
            })
        );
        let unresolved = MultiFilePPtrResolution::MissingDependency {
            external_index: 2,
            path_id: 88,
        };
        let unresolved_reference = UnresolvedProjectPointer {
            source_file_index: 1,
            source_object_index: 6,
            source_path_id: 73,
            field_path: "m_Material".to_owned(),
            serialized_file_id: 2,
            serialized_path_id: 88,
            class_id: 4,
            resolution: unresolved,
        };
        assert_eq!(
            map.resolve(
                ProjectPointerRequest::UnresolvedInput(unresolved_reference.clone()),
                3
            ),
            Ok(ProjectPointerResolution::UnresolvedInput(
                unresolved_reference
            ))
        );
        assert_eq!(
            map.resolve(
                ProjectPointerRequest::UnresolvedInput(UnresolvedProjectPointer {
                    source_file_index: 1,
                    source_object_index: 6,
                    source_path_id: 73,
                    field_path: "m_Material".to_owned(),
                    serialized_file_id: 2,
                    serialized_path_id: 0,
                    class_id: 4,
                    resolution: MultiFilePPtrResolution::Null,
                }),
                3
            ),
            Err(ExportPointerMapError::InvalidUnresolvedReferenceStatus)
        );
    }

    #[test]
    fn project_pointer_map_rejects_duplicate_and_unregistered_identities() {
        let duplicate = entry(asset(0, 0, 10, 1), 10, 3);
        assert_eq!(
            ProjectExportPointerMap::new(
                &[duplicate, duplicate],
                &[collection(3, 0x33, UnityProjectAssetType::Serialized)],
                ExportPointerMapLimits::default()
            )
            .err(),
            Some(ExportPointerMapError::DuplicateAssetIdentity {
                asset: duplicate.asset,
            })
        );
        assert_eq!(
            ProjectExportPointerMap::new(&[duplicate], &[], ExportPointerMapLimits::default())
                .err(),
            Some(ExportPointerMapError::MissingCollectionMetadata {
                collection_index: 3
            })
        );

        let conflicting_metadata = asset(0, 0, 99, 4);
        assert_eq!(
            map().resolve(
                ProjectPointerRequest::ResolvedAsset(conflicting_metadata),
                3
            ),
            Err(ExportPointerMapError::AssetIdentityMetadataMismatch {
                requested: conflicting_metadata,
                registered: duplicate.asset,
            })
        );
    }
}
