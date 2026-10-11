//! Bounded mapping from resolved Unity object identities to project-export pointers.

use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::serialized_file::reference_graph::{
    MultiFilePPtrResolution, PredefinedEngineResourceKind,
};

use super::ids::{get_main_export_id, ExportIdError};
use super::prefab_collection::PrefabExportIdAssignment;
use super::scene_collection::{ExportAssetIdentity, ExportAssetObjectKey, SceneExportIdAssignment};

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
    AssetCountOverflow,
    CollectionCountOverflow,
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
    MissingCloneSource {
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
    ZeroCollectionGuid,
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
            Self::AssetCountOverflow => f.write_str("project export asset count overflowed"),
            Self::CollectionCountOverflow => {
                f.write_str("project export collection count overflowed")
            }
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
            Self::MissingCloneSource { asset } => write!(f, "collection-local export copy has no canonical source identity {asset:?}"),
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
            Self::ZeroCollectionGuid => {
                f.write_str("Scene/Prefab export collection GUID must not be zero")
            }
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
    /// The referenced class ID when the typed field provides one; `None` means it is unknown.
    pub expected_class_id: Option<i32>,
    pub resolution: MultiFilePPtrResolution,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProjectPointerRequest {
    Null,
    ResolvedAsset(ExportAssetIdentity),
    PredefinedEngineAsset {
        path_id: i64,
        resource: PredefinedEngineResourceKind,
    },
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
    local_copies: HashMap<(usize, ExportAssetObjectKey), ExportedAssetPointerEntry>,
}

/// Collects Scene and Prefab plans and converts their IDs into one project pointer map.
pub struct ProjectExportPointerMapBuilder {
    limits: ExportPointerMapLimits,
    assets: Vec<ExportedAssetPointerEntry>,
    collections: Vec<ExportCollectionPointerMetadata>,
}

impl ProjectExportPointerMapBuilder {
    pub fn new(limits: ExportPointerMapLimits) -> Self {
        Self {
            limits,
            assets: Vec::new(),
            collections: Vec::new(),
        }
    }

    pub fn register_scene_collection(
        &mut self,
        collection_index: usize,
        guid: [u8; 16],
        assignments: &[SceneExportIdAssignment],
    ) -> Result<(), ExportPointerMapError> {
        if guid == [0; 16] {
            return Err(ExportPointerMapError::ZeroCollectionGuid);
        }
        self.reserve_collection(
            ExportCollectionPointerMetadata {
                collection_index,
                guid,
                asset_type: UnityProjectAssetType::Serialized,
            },
            assignments.len(),
        )?;
        self.assets.extend(
            assignments
                .iter()
                .map(|assignment| ExportedAssetPointerEntry {
                    asset: assignment.asset,
                    file_id: assignment.file_id,
                    collection_index,
                }),
        );
        Ok(())
    }

    pub fn register_prefab_collection(
        &mut self,
        collection_index: usize,
        guid: [u8; 16],
        asset_type: UnityProjectAssetType,
        assignments: &[PrefabExportIdAssignment],
    ) -> Result<(), ExportPointerMapError> {
        if guid == [0; 16] {
            return Err(ExportPointerMapError::ZeroCollectionGuid);
        }
        self.reserve_collection(
            ExportCollectionPointerMetadata {
                collection_index,
                guid,
                asset_type,
            },
            assignments.len(),
        )?;
        self.assets.extend(
            assignments
                .iter()
                .map(|assignment| ExportedAssetPointerEntry {
                    asset: assignment.asset,
                    file_id: assignment.file_id,
                    collection_index,
                }),
        );
        Ok(())
    }

    pub fn build(self) -> Result<ProjectExportPointerMap, ExportPointerMapError> {
        ProjectExportPointerMap::new(&self.assets, &self.collections, self.limits)
    }

    fn reserve_collection(
        &mut self,
        metadata: ExportCollectionPointerMetadata,
        additional_assets: usize,
    ) -> Result<(), ExportPointerMapError> {
        let requested_collections = self
            .collections
            .len()
            .checked_add(1)
            .ok_or(ExportPointerMapError::CollectionCountOverflow)?;
        if requested_collections > self.limits.max_collections {
            return Err(ExportPointerMapError::CollectionLimitExceeded {
                requested: requested_collections,
                limit: self.limits.max_collections,
            });
        }
        let requested_assets = self
            .assets
            .len()
            .checked_add(additional_assets)
            .ok_or(ExportPointerMapError::AssetCountOverflow)?;
        if requested_assets > self.limits.max_assets {
            return Err(ExportPointerMapError::AssetLimitExceeded {
                requested: requested_assets,
                limit: self.limits.max_assets,
            });
        }
        self.collections
            .try_reserve(1)
            .map_err(|_| ExportPointerMapError::AllocationFailed {
                resource: "project export collection registry",
                requested: requested_collections,
            })?;
        self.assets.try_reserve(additional_assets).map_err(|_| {
            ExportPointerMapError::AllocationFailed {
                resource: "project export asset registry",
                requested: requested_assets,
            }
        })?;
        self.collections.push(metadata);
        Ok(())
    }
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
            local_copies: HashMap::new(),
        })
    }

    /// Register explicit copies without changing the canonical identity used by
    /// references outside the owning collection. Generic duplicate registration
    /// in `new` remains an error; only this bounded copy plan enables a local alias.
    pub(crate) fn register_local_copies(
        &mut self,
        copies: &[ExportedAssetPointerEntry],
        limits: ExportPointerMapLimits,
    ) -> Result<(), ExportPointerMapError> {
        let requested = self
            .assets
            .len()
            .checked_add(self.local_copies.len())
            .and_then(|count| count.checked_add(copies.len()))
            .ok_or(ExportPointerMapError::AssetCountOverflow)?;
        if requested > limits.max_assets {
            return Err(ExportPointerMapError::AssetLimitExceeded {
                requested,
                limit: limits.max_assets,
            });
        }
        let mut pending = HashMap::new();
        pending
            .try_reserve(copies.len())
            .map_err(|_| ExportPointerMapError::AllocationFailed {
                resource: "collection-local asset copies",
                requested,
            })?;
        for &copy in copies {
            let source = self
                .assets
                .get(&copy.asset.object_key())
                .ok_or(ExportPointerMapError::MissingCloneSource { asset: copy.asset })?;
            if source.asset != copy.asset {
                return Err(ExportPointerMapError::AssetIdentityMetadataMismatch {
                    requested: copy.asset,
                    registered: source.asset,
                });
            }
            if !self.collections.contains_key(&copy.collection_index) {
                return Err(ExportPointerMapError::MissingCollectionMetadata {
                    collection_index: copy.collection_index,
                });
            }
            let key = (copy.collection_index, copy.asset.object_key());
            if source.collection_index == copy.collection_index
                || self.local_copies.contains_key(&key)
                || pending.contains_key(&key)
            {
                return Err(ExportPointerMapError::DuplicateAssetIdentity { asset: copy.asset });
            }
            pending.insert(key, copy);
        }
        self.local_copies.try_reserve(pending.len()).map_err(|_| {
            ExportPointerMapError::AllocationFailed {
                resource: "collection-local asset copies",
                requested,
            }
        })?;
        self.local_copies.extend(pending);
        Ok(())
    }

    pub fn resolve(
        &self,
        request: ProjectPointerRequest,
        current_collection_index: usize,
    ) -> Result<ProjectPointerResolution, ExportPointerMapError> {
        match request {
            ProjectPointerRequest::Null => Ok(ProjectPointerResolution::Null),
            ProjectPointerRequest::PredefinedEngineAsset { path_id, resource } => {
                let words = match resource {
                    PredefinedEngineResourceKind::DefaultResources => {
                        super::guid_policy::ENGINE_DEFAULT_RESOURCES_GUID_WORDS
                    }
                };
                let mut guid = [0; 16];
                for (index, word) in words.iter().enumerate() {
                    guid[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
                }
                Ok(ProjectPointerResolution::Mapped(UnityMetaPointer {
                    file_id: path_id,
                    guid: ExportPointerGuid::Collection(guid),
                    asset_type: UnityProjectAssetType::Internal,
                }))
            }
            ProjectPointerRequest::ResolvedAsset(asset) => {
                let Some(entry) = self
                    .local_copies
                    .get(&(current_collection_index, asset.object_key()))
                    .or_else(|| self.assets.get(&asset.object_key()))
                else {
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
                    MultiFilePPtrResolution::Null
                        | MultiFilePPtrResolution::Object { .. }
                        | MultiFilePPtrResolution::PredefinedEngineAsset { .. }
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
    fn collection_local_copies_preserve_canonical_external_references() {
        let original = asset(0, 0, 10, 1);
        let copy = entry(original, 77, 4);
        let mut pointers = map();
        pointers
            .register_local_copies(&[copy], ExportPointerMapLimits::default())
            .unwrap();
        assert_eq!(
            pointers
                .resolve(ProjectPointerRequest::ResolvedAsset(original), 4)
                .unwrap(),
            ProjectPointerResolution::Mapped(UnityMetaPointer {
                file_id: 77,
                guid: ExportPointerGuid::None,
                asset_type: UnityProjectAssetType::Serialized
            })
        );
        assert_eq!(
            pointers
                .resolve(ProjectPointerRequest::ResolvedAsset(original), 0)
                .unwrap(),
            ProjectPointerResolution::Mapped(UnityMetaPointer {
                file_id: 10,
                guid: ExportPointerGuid::Collection([0x33; 16]),
                asset_type: UnityProjectAssetType::Serialized
            })
        );
        assert!(matches!(
            pointers.register_local_copies(&[copy], ExportPointerMapLimits::default()),
            Err(ExportPointerMapError::DuplicateAssetIdentity { .. })
        ));
        let missing = entry(asset(0, 3, 999, 114), 114000001, 4);
        assert!(matches!(
            pointers.register_local_copies(&[missing], ExportPointerMapLimits::default()),
            Err(ExportPointerMapError::MissingCloneSource { .. })
        ));
        let mut rejected = map();
        let before = rejected
            .resolve(ProjectPointerRequest::ResolvedAsset(original), 4)
            .unwrap();
        assert!(matches!(
            rejected.register_local_copies(&[copy, missing], ExportPointerMapLimits::default()),
            Err(ExportPointerMapError::MissingCloneSource { .. })
        ));
        assert_eq!(
            rejected
                .resolve(ProjectPointerRequest::ResolvedAsset(original), 4)
                .unwrap(),
            before,
            "a rejected copy batch cannot leave a partial reference remapping"
        );
        assert!(matches!(
            map().register_local_copies(
                &[copy],
                ExportPointerMapLimits {
                    max_assets: 2,
                    ..ExportPointerMapLimits::default()
                }
            ),
            Err(ExportPointerMapError::AssetLimitExceeded { .. })
        ));
    }

    #[test]
    fn predefined_engine_asset_pointers_keep_source_path_id_and_guid() {
        let resolution = map()
            .resolve(
                ProjectPointerRequest::PredefinedEngineAsset {
                    path_id: 10202,
                    resource: PredefinedEngineResourceKind::DefaultResources,
                },
                0,
            )
            .unwrap();

        assert_eq!(
            resolution,
            ProjectPointerResolution::Mapped(UnityMetaPointer {
                file_id: 10202,
                guid: ExportPointerGuid::Collection([
                    0, 0, 0, 0, 0, 0, 0, 0, 14, 0, 0, 0, 0, 0, 0, 0,
                ]),
                asset_type: UnityProjectAssetType::Internal,
            })
        );
    }

    #[test]
    fn pointer_map_builder_registers_scene_and_prefab_plans() {
        let scene_asset = asset(0, 0, 10, 1);
        let prefab_asset = asset(1, 0, 20, 1001);
        let prefab_child = asset(1, 1, 21, 1);
        let scene_assignments = [SceneExportIdAssignment {
            asset: scene_asset,
            file_id: 10,
        }];
        let prefab_assignments = [
            PrefabExportIdAssignment {
                asset: prefab_asset,
                file_id: 100_100_000,
                is_main_asset: true,
            },
            PrefabExportIdAssignment {
                asset: prefab_child,
                file_id: 1_000_001,
                is_main_asset: false,
            },
        ];
        let mut builder = ProjectExportPointerMapBuilder::new(ExportPointerMapLimits::default());
        builder
            .register_scene_collection(7, [0x77; 16], &scene_assignments)
            .unwrap();
        builder
            .register_prefab_collection(
                8,
                [0x88; 16],
                UnityProjectAssetType::Meta,
                &prefab_assignments,
            )
            .unwrap();
        let map = builder.build().unwrap();

        assert_eq!(
            map.resolve(ProjectPointerRequest::ResolvedAsset(scene_asset), 8),
            Ok(ProjectPointerResolution::Mapped(UnityMetaPointer {
                file_id: 10,
                guid: ExportPointerGuid::Collection([0x77; 16]),
                asset_type: UnityProjectAssetType::Serialized,
            }))
        );
        assert_eq!(
            map.resolve(ProjectPointerRequest::ResolvedAsset(prefab_child), 8),
            Ok(ProjectPointerResolution::Mapped(UnityMetaPointer {
                file_id: 1_000_001,
                guid: ExportPointerGuid::None,
                asset_type: UnityProjectAssetType::Serialized,
            }))
        );
    }

    #[test]
    fn pointer_map_builder_rejects_zero_guids_and_collection_limits() {
        let assignment = SceneExportIdAssignment {
            asset: asset(0, 0, 10, 1),
            file_id: 10,
        };
        let mut zero_guid = ProjectExportPointerMapBuilder::new(ExportPointerMapLimits::default());
        assert_eq!(
            zero_guid.register_scene_collection(0, [0; 16], &[assignment]),
            Err(ExportPointerMapError::ZeroCollectionGuid)
        );

        let mut limit = ProjectExportPointerMapBuilder::new(ExportPointerMapLimits {
            max_assets: 1,
            max_collections: 0,
        });
        assert_eq!(
            limit.register_scene_collection(0, [0x33; 16], &[assignment]),
            Err(ExportPointerMapError::CollectionLimitExceeded {
                requested: 1,
                limit: 0,
            })
        );
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
            expected_class_id: Some(4),
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
                    expected_class_id: Some(4),
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
