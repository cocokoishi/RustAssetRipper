//! Ordered export-ID assignment for one Scene YAML collection.

use std::collections::HashSet;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::processing::scene_naming::SceneUnityVersion;

use super::ids::get_pseudo_random_export_id;

/// Collection scope used to distinguish source and processed assets with equal PathIDs.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ExportAssetCollectionIdentity {
    SerializedFile { file_index: usize },
    ProcessedCollection { collection_index: usize },
}

/// Stable identity and class data needed by a Unity project export collection.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ExportAssetIdentity {
    pub collection: ExportAssetCollectionIdentity,
    pub object_index: usize,
    pub path_id: i64,
    pub class_id: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SceneExportIdAssignment {
    pub asset: ExportAssetIdentity,
    pub file_id: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SceneExportIdPlanLimits {
    pub max_assets: usize,
}

impl Default for SceneExportIdPlanLimits {
    fn default() -> Self {
        Self {
            max_assets: 20_000_000,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SceneExportIdPlanError {
    AssetLimitExceeded {
        requested: usize,
        limit: usize,
    },
    DuplicateAssetIdentity {
        asset: ExportAssetIdentity,
    },
    GeneratedAssetSeedOverflow,
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
}

impl Display for SceneExportIdPlanError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AssetLimitExceeded { requested, limit } => write!(
                f,
                "Scene export collection has {requested} assets, exceeding limit {limit}"
            ),
            Self::DuplicateAssetIdentity { asset } => write!(
                f,
                "Scene export collection contains duplicate asset identity {asset:?}"
            ),
            Self::GeneratedAssetSeedOverflow => {
                f.write_str("Scene export generated-asset seed exceeds the source 32-bit range")
            }
            Self::AllocationFailed {
                resource,
                requested,
            } => {
                write!(f, "could not allocate {requested} entries for {resource}")
            }
        }
    }
}

impl Error for SceneExportIdPlanError {}

/// Assign IDs in the exact order consumed by `SceneExportCollection`.
///
/// Serialized-file assets keep their PathID. Processed-collection assets receive a pseudo-random
/// ID whose seed increments only for other processed assets in this scene export collection.
/// Asset order is supplied by the caller and is not sorted or normalized here.
pub fn plan_scene_export_ids(
    assets_in_export_order: &[ExportAssetIdentity],
    unity_version: SceneUnityVersion,
    limits: SceneExportIdPlanLimits,
) -> Result<Vec<SceneExportIdAssignment>, SceneExportIdPlanError> {
    if assets_in_export_order.len() > limits.max_assets {
        return Err(SceneExportIdPlanError::AssetLimitExceeded {
            requested: assets_in_export_order.len(),
            limit: limits.max_assets,
        });
    }

    let mut identities = HashSet::new();
    identities
        .try_reserve(assets_in_export_order.len())
        .map_err(|_| SceneExportIdPlanError::AllocationFailed {
            resource: "Scene export asset identity index",
            requested: assets_in_export_order.len(),
        })?;
    let mut assignments = Vec::new();
    assignments
        .try_reserve_exact(assets_in_export_order.len())
        .map_err(|_| SceneExportIdPlanError::AllocationFailed {
            resource: "Scene export ID assignments",
            requested: assets_in_export_order.len(),
        })?;

    let mut generated_seed = 0i32;
    for &asset in assets_in_export_order {
        if !identities.insert(asset) {
            return Err(SceneExportIdPlanError::DuplicateAssetIdentity { asset });
        }
        let file_id = match asset.collection {
            ExportAssetCollectionIdentity::SerializedFile { .. } => asset.path_id,
            ExportAssetCollectionIdentity::ProcessedCollection { .. } => {
                let file_id =
                    get_pseudo_random_export_id(asset.class_id, generated_seed, unity_version);
                generated_seed = generated_seed
                    .checked_add(1)
                    .ok_or(SceneExportIdPlanError::GeneratedAssetSeedOverflow)?;
                file_id
            }
        };
        assignments.push(SceneExportIdAssignment { asset, file_id });
    }
    Ok(assignments)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(
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

    fn processed(
        collection_index: usize,
        object_index: usize,
        path_id: i64,
        class_id: i32,
    ) -> ExportAssetIdentity {
        ExportAssetIdentity {
            collection: ExportAssetCollectionIdentity::ProcessedCollection { collection_index },
            object_index,
            path_id,
            class_id,
        }
    }

    fn version(value: &str) -> SceneUnityVersion {
        SceneUnityVersion::parse(value).unwrap()
    }

    #[test]
    fn scene_export_ids_preserve_order_and_only_seed_processed_assets() {
        let assets = [
            source(2, 5, 91, 1),
            processed(4, 0, 1, 4),
            source(3, 7, 91, 4),
            processed(4, 1, 2, 1660057539),
        ];

        let plan = plan_scene_export_ids(
            &assets,
            version("2022.3.1f1"),
            SceneExportIdPlanLimits::default(),
        )
        .unwrap();

        assert_eq!(plan.len(), assets.len());
        assert_eq!(
            plan.iter().map(|entry| entry.asset).collect::<Vec<_>>(),
            assets
        );
        assert_eq!(
            plan.iter().map(|entry| entry.file_id).collect::<Vec<_>>(),
            [
                91,
                get_pseudo_random_export_id(4, 0, version("2022.3.1f1")),
                91,
                get_pseudo_random_export_id(1660057539, 1, version("2022.3.1f1")),
            ]
        );
    }

    #[test]
    fn scene_export_id_plan_uses_the_legacy_hash_width_for_old_projects() {
        let assets = [processed(0, 0, 1, 4), processed(0, 1, 2, 4)];

        let plan = plan_scene_export_ids(
            &assets,
            version("5.4.7f1"),
            SceneExportIdPlanLimits::default(),
        )
        .unwrap();

        assert_eq!(
            plan.iter().map(|entry| entry.file_id).collect::<Vec<_>>(),
            [
                get_pseudo_random_export_id(4, 0, version("5.4.7f1")),
                get_pseudo_random_export_id(4, 1, version("5.4.7f1")),
            ]
        );
    }

    #[test]
    fn scene_export_id_plan_rejects_duplicate_assets_and_limit_overruns() {
        let asset = source(1, 0, 22, 1);
        assert!(matches!(
            plan_scene_export_ids(
                &[asset, asset],
                version("2022.3.1f1"),
                SceneExportIdPlanLimits::default()
            ),
            Err(SceneExportIdPlanError::DuplicateAssetIdentity { asset: duplicate })
                if duplicate == asset
        ));
        assert_eq!(
            plan_scene_export_ids(
                &[asset],
                version("2022.3.1f1"),
                SceneExportIdPlanLimits { max_assets: 0 }
            ),
            Err(SceneExportIdPlanError::AssetLimitExceeded {
                requested: 1,
                limit: 0,
            })
        );
    }
}
