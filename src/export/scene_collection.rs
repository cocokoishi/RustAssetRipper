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

/// Object identity is collection scope plus object-table index; PathID and class ID are attributes.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ExportAssetObjectKey {
    pub collection: ExportAssetCollectionIdentity,
    pub object_index: usize,
}

impl ExportAssetIdentity {
    pub const fn object_key(self) -> ExportAssetObjectKey {
        ExportAssetObjectKey {
            collection: self.collection,
            object_index: self.object_index,
        }
    }
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

/// The ordered membership groups exposed by the baseline `SceneHierarchyObject.Assets`.
///
/// The hierarchy's ID assignment order concatenates these groups; it is distinct from the
/// separately sorted YAML document order used by `SceneExportCollection.ExportableAssets`.
#[derive(Clone, Copy, Debug)]
pub struct SceneHierarchyAssetGroups<'a> {
    pub game_objects: &'a [ExportAssetIdentity],
    pub components: &'a [ExportAssetIdentity],
    pub prefab_instances: &'a [ExportAssetIdentity],
    pub managers: &'a [ExportAssetIdentity],
    pub scene_roots: Option<ExportAssetIdentity>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SceneHierarchyAssetOrderLimits {
    pub max_assets: usize,
}

impl Default for SceneHierarchyAssetOrderLimits {
    fn default() -> Self {
        Self {
            max_assets: 20_000_000,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SceneHierarchyAssetOrderError {
    AssetCountOverflow,
    AssetLimitExceeded { requested: usize, limit: usize },
    AllocationFailed { requested: usize },
}

impl Display for SceneHierarchyAssetOrderError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AssetCountOverflow => f.write_str("Scene hierarchy asset count overflowed"),
            Self::AssetLimitExceeded { requested, limit } => write!(
                f,
                "Scene hierarchy has {requested} assets, exceeding limit {limit}"
            ),
            Self::AllocationFailed { requested } => write!(
                f,
                "could not allocate an ordered Scene hierarchy list for {requested} assets"
            ),
        }
    }
}

impl Error for SceneHierarchyAssetOrderError {}

/// Build the ID-assignment sequence exposed by `SceneHierarchyObject.Assets`.
///
/// Membership group order follows `GameObjectHierarchyObject.Assets`, then the Scene-specific
/// manager and optional `SceneRoots` entries. The baseline applies its YAML display comparator
/// separately, so this helper does not sort, deduplicate, or omit hidden assets.
pub fn order_scene_hierarchy_assets(
    groups: SceneHierarchyAssetGroups<'_>,
    limits: SceneHierarchyAssetOrderLimits,
) -> Result<Vec<ExportAssetIdentity>, SceneHierarchyAssetOrderError> {
    let mut requested = groups
        .game_objects
        .len()
        .checked_add(groups.components.len())
        .and_then(|count| count.checked_add(groups.prefab_instances.len()))
        .and_then(|count| count.checked_add(groups.managers.len()))
        .ok_or(SceneHierarchyAssetOrderError::AssetCountOverflow)?;
    if groups.scene_roots.is_some() {
        requested = requested
            .checked_add(1)
            .ok_or(SceneHierarchyAssetOrderError::AssetCountOverflow)?;
    }
    if requested > limits.max_assets {
        return Err(SceneHierarchyAssetOrderError::AssetLimitExceeded {
            requested,
            limit: limits.max_assets,
        });
    }

    let mut ordered = Vec::new();
    ordered
        .try_reserve_exact(requested)
        .map_err(|_| SceneHierarchyAssetOrderError::AllocationFailed { requested })?;
    ordered.extend_from_slice(groups.game_objects);
    ordered.extend_from_slice(groups.components);
    ordered.extend_from_slice(groups.prefab_instances);
    ordered.extend_from_slice(groups.managers);
    ordered.extend(groups.scene_roots);
    Ok(ordered)
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
        if !identities.insert(asset.object_key()) {
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
    fn scene_hierarchy_asset_order_concatenates_baseline_membership_groups() {
        let game_objects = [source(0, 0, 10, 1), source(0, 1, 11, 1)];
        let components = [source(0, 2, 20, 4)];
        let prefab_instances = [processed(1, 0, 1, 1001)];
        let managers = [source(0, 3, 30, 3)];
        let scene_roots = processed(1, 1, 2, 1660057539);

        let ordered = order_scene_hierarchy_assets(
            SceneHierarchyAssetGroups {
                game_objects: &game_objects,
                components: &components,
                prefab_instances: &prefab_instances,
                managers: &managers,
                scene_roots: Some(scene_roots),
            },
            SceneHierarchyAssetOrderLimits::default(),
        )
        .unwrap();

        assert_eq!(
            ordered,
            [
                game_objects[0],
                game_objects[1],
                components[0],
                prefab_instances[0],
                managers[0],
                scene_roots,
            ]
        );
    }

    #[test]
    fn scene_hierarchy_asset_order_allows_absent_scene_roots_and_enforces_limit() {
        let game_objects = [source(0, 0, 10, 1)];
        let empty = [];
        let groups = SceneHierarchyAssetGroups {
            game_objects: &game_objects,
            components: &empty,
            prefab_instances: &empty,
            managers: &empty,
            scene_roots: None,
        };

        assert_eq!(
            order_scene_hierarchy_assets(groups, SceneHierarchyAssetOrderLimits::default()),
            Ok(game_objects.to_vec())
        );
        assert_eq!(
            order_scene_hierarchy_assets(groups, SceneHierarchyAssetOrderLimits { max_assets: 0 }),
            Err(SceneHierarchyAssetOrderError::AssetLimitExceeded {
                requested: 1,
                limit: 0,
            })
        );
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

        let conflicting_metadata = source(1, 0, 23, 4);
        assert!(matches!(
            plan_scene_export_ids(
                &[asset, conflicting_metadata],
                version("2022.3.1f1"),
                SceneExportIdPlanLimits::default()
            ),
            Err(SceneExportIdPlanError::DuplicateAssetIdentity { asset: duplicate })
                if duplicate == conflicting_metadata
        ));
    }
}
