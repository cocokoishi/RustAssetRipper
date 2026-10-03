//! Export IDs and membership for one baseline-compatible Prefab collection.

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::processing::scene_naming::SceneUnityVersion;

use super::ids::{get_main_export_id, get_pseudo_random_export_id, ExportIdError};
use super::pointer_map::{ExportPointerGuid, UnityMetaPointer, UnityProjectAssetType};
use super::scene_collection::{ExportAssetIdentity, ExportAssetObjectKey};

pub const PREFAB_INSTANCE_CLASS_ID: i32 = 1001;

/// Whether the Prefab field uses one of the baseline marker interfaces.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrefabAssetRepresentation {
    /// Pre-2018.3 prefab assets are emitted as YAML documents with a NativeFormatImporter.
    LegacyPrefabMarker,
    /// Modern prefab instances are hidden from YAML and use a PrefabImporter.
    PrefabInstanceMarker,
    /// Other IPrefabInstance implementations use a PrefabImporter and are not hidden by the
    /// PrefabHierarchyObject constructor.
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VersionedPrefabAsset {
    pub identity: ExportAssetIdentity,
    /// Baseline GetPseudoRandomExportId reads each asset's own collection version.
    pub unity_version: SceneUnityVersion,
}

/// Categorized input matching PrefabHierarchyObject.Assets and the Prefab collection constructor.
#[derive(Clone, Copy, Debug)]
pub struct PrefabHierarchyAssetGroups<'a> {
    pub game_objects: &'a [VersionedPrefabAsset],
    pub components: &'a [VersionedPrefabAsset],
    pub prefab_instances: &'a [VersionedPrefabAsset],
    pub prefab_asset: VersionedPrefabAsset,
    pub hierarchy_asset: VersionedPrefabAsset,
    pub representation: PrefabAssetRepresentation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrefabExportPlanLimits {
    pub max_assets: usize,
}

impl Default for PrefabExportPlanLimits {
    fn default() -> Self {
        Self {
            max_assets: 20_000_000,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrefabExportIdAssignment {
    pub asset: ExportAssetIdentity,
    pub file_id: i64,
    pub is_main_asset: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrefabExportIdPlan {
    /// Includes the main Prefab asset followed by collection-registered assets in add order.
    pub registered_assets: Vec<PrefabExportIdAssignment>,
    /// Hierarchy order after the baseline HiddenAssets filter; it does not include the hierarchy
    /// helper itself, which is registered for lookup but not serialized as a YAML document.
    pub exportable_assets: Vec<ExportAssetIdentity>,
    pub emit_prefab_asset: bool,
    pub representation: PrefabAssetRepresentation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PrefabExportPlanError {
    AssetCountOverflow,
    AssetLimitExceeded {
        requested: usize,
        limit: usize,
    },
    DuplicateObjectMetadata {
        key: ExportAssetObjectKey,
        first_path_id: i64,
        first_class_id: i32,
        first_version: SceneUnityVersion,
        duplicate_path_id: i64,
        duplicate_class_id: i32,
        duplicate_version: SceneUnityVersion,
    },
    GeneratedAssetSeedOverflow,
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    ExportId(ExportIdError),
    ZeroCollectionGuid,
}

impl Display for PrefabExportPlanError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AssetCountOverflow => f.write_str("Prefab export asset count overflowed"),
            Self::AssetLimitExceeded { requested, limit } => write!(
                f,
                "Prefab export collection has {requested} assets, exceeding limit {limit}"
            ),
            Self::DuplicateObjectMetadata {
                key,
                first_path_id,
                first_class_id,
                first_version,
                duplicate_path_id,
                duplicate_class_id,
                duplicate_version,
            } => write!(
                f,
                "Prefab object key {key:?} has conflicting PathID/class/version metadata: ({first_path_id}, {first_class_id}, {first_version:?}) and ({duplicate_path_id}, {duplicate_class_id}, {duplicate_version:?})"
            ),
            Self::GeneratedAssetSeedOverflow => {
                f.write_str("Prefab generated-asset seed exceeds the source 32-bit range")
            }
            Self::AllocationFailed {
                resource,
                requested,
            } => {
                write!(f, "could not allocate {requested} entries for {resource}")
            }
            Self::ExportId(error) => Display::fmt(error, f),
            Self::ZeroCollectionGuid => {
                f.write_str("Prefab source pointer requires a nonzero export collection GUID")
            }
        }
    }
}

impl Error for PrefabExportPlanError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ExportId(error) => Some(error),
            _ => None,
        }
    }
}

/// Reproduce `PrefabExportCollection` ID registration and hierarchy export order.
///
/// `AssetsExportCollection.AddAssets` first visits hierarchy members in category order, then
/// `PrefabExportCollection` adds the hierarchy helper itself. The main Prefab asset is excluded
/// from generated-ID allocation, as are duplicate object keys; accepted generated assets receive
/// contiguous seeds. The Prefab asset remains the collection's main asset with a class-prefixed ID.
pub fn plan_prefab_export_ids(
    groups: PrefabHierarchyAssetGroups<'_>,
    hidden_assets: &[ExportAssetObjectKey],
    limits: PrefabExportPlanLimits,
) -> Result<PrefabExportIdPlan, PrefabExportPlanError> {
    let requested = groups
        .game_objects
        .len()
        .checked_add(groups.components.len())
        .and_then(|count| count.checked_add(groups.prefab_instances.len()))
        .and_then(|count| count.checked_add(2))
        .ok_or(PrefabExportPlanError::AssetCountOverflow)?;
    if requested > limits.max_assets {
        return Err(PrefabExportPlanError::AssetLimitExceeded {
            requested,
            limit: limits.max_assets,
        });
    }

    if hidden_assets.len() > limits.max_assets {
        return Err(PrefabExportPlanError::AssetLimitExceeded {
            requested: hidden_assets.len(),
            limit: limits.max_assets,
        });
    }
    let mut hidden = HashSet::new();
    hidden.try_reserve(hidden_assets.len()).map_err(|_| {
        PrefabExportPlanError::AllocationFailed {
            resource: "Prefab hidden-asset index",
            requested: hidden_assets.len(),
        }
    })?;
    hidden.extend(hidden_assets.iter().copied());

    let mut identity_by_key = HashMap::new();
    identity_by_key.try_reserve(requested).map_err(|_| {
        PrefabExportPlanError::AllocationFailed {
            resource: "Prefab object identity index",
            requested,
        }
    })?;
    let mut registered_assets = Vec::new();
    registered_assets
        .try_reserve_exact(requested)
        .map_err(|_| PrefabExportPlanError::AllocationFailed {
            resource: "Prefab export ID assignments",
            requested,
        })?;
    let mut exportable_assets = Vec::new();
    exportable_assets
        .try_reserve_exact(requested.saturating_sub(1))
        .map_err(|_| PrefabExportPlanError::AllocationFailed {
            resource: "Prefab exportable hierarchy assets",
            requested: requested.saturating_sub(1),
        })?;

    let main_asset = groups.prefab_asset.identity;
    let main_file_id =
        get_main_export_id(main_asset.class_id, 0).map_err(PrefabExportPlanError::ExportId)?;
    identity_by_key.insert(main_asset.object_key(), groups.prefab_asset);
    registered_assets.push(PrefabExportIdAssignment {
        asset: main_asset,
        file_id: main_file_id,
        is_main_asset: true,
    });

    let mut generated_seed = 0i32;
    for member in groups
        .game_objects
        .iter()
        .chain(groups.components)
        .chain(groups.prefab_instances)
        .chain(std::iter::once(&groups.prefab_asset))
        .chain(std::iter::once(&groups.hierarchy_asset))
    {
        let identity = member.identity;
        let key = identity.object_key();
        if let Some(&first) = identity_by_key.get(&key) {
            if first.identity != identity || first.unity_version != member.unity_version {
                return Err(PrefabExportPlanError::DuplicateObjectMetadata {
                    key,
                    first_path_id: first.identity.path_id,
                    first_class_id: first.identity.class_id,
                    first_version: first.unity_version,
                    duplicate_path_id: identity.path_id,
                    duplicate_class_id: identity.class_id,
                    duplicate_version: member.unity_version,
                });
            }
            continue;
        }
        identity_by_key.insert(key, *member);
        let file_id =
            get_pseudo_random_export_id(identity.class_id, generated_seed, member.unity_version);
        generated_seed = generated_seed
            .checked_add(1)
            .ok_or(PrefabExportPlanError::GeneratedAssetSeedOverflow)?;
        registered_assets.push(PrefabExportIdAssignment {
            asset: identity,
            file_id,
            is_main_asset: false,
        });
    }

    for member in groups
        .game_objects
        .iter()
        .chain(groups.components)
        .chain(groups.prefab_instances)
        .chain(std::iter::once(&groups.prefab_asset))
    {
        if !hidden.contains(&member.identity.object_key()) {
            exportable_assets.push(member.identity);
        }
    }

    debug_assert!(registered_assets.len() <= requested);
    Ok(PrefabExportIdPlan {
        registered_assets,
        exportable_assets,
        emit_prefab_asset: groups.representation == PrefabAssetRepresentation::LegacyPrefabMarker,
        representation: groups.representation,
    })
}

/// Return `PrefabExportCollection.GenerateMetaPtrForPrefab` for this collection.
pub fn generate_prefab_source_pointer(
    collection_guid: [u8; 16],
    representation: PrefabAssetRepresentation,
) -> Result<UnityMetaPointer, PrefabExportPlanError> {
    if collection_guid == [0; 16] {
        return Err(PrefabExportPlanError::ZeroCollectionGuid);
    }
    Ok(UnityMetaPointer {
        file_id: get_main_export_id(PREFAB_INSTANCE_CLASS_ID, 0)
            .map_err(PrefabExportPlanError::ExportId)?,
        guid: ExportPointerGuid::Collection(collection_guid),
        asset_type: if representation == PrefabAssetRepresentation::LegacyPrefabMarker {
            UnityProjectAssetType::Serialized
        } else {
            UnityProjectAssetType::Meta
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::scene_collection::ExportAssetCollectionIdentity;

    fn identity(
        collection_index: usize,
        object_index: usize,
        path_id: i64,
        class_id: i32,
    ) -> ExportAssetIdentity {
        ExportAssetIdentity {
            collection: ExportAssetCollectionIdentity::SerializedFile {
                file_index: collection_index,
            },
            object_index,
            path_id,
            class_id,
        }
    }

    fn member(
        collection_index: usize,
        object_index: usize,
        path_id: i64,
        class_id: i32,
        version: &str,
    ) -> VersionedPrefabAsset {
        VersionedPrefabAsset {
            identity: identity(collection_index, object_index, path_id, class_id),
            unity_version: SceneUnityVersion::parse(version).unwrap(),
        }
    }

    fn groups<'a>(
        game_objects: &'a [VersionedPrefabAsset],
        components: &'a [VersionedPrefabAsset],
        prefab_instances: &'a [VersionedPrefabAsset],
        prefab_asset: VersionedPrefabAsset,
        hierarchy_asset: VersionedPrefabAsset,
        representation: PrefabAssetRepresentation,
    ) -> PrefabHierarchyAssetGroups<'a> {
        PrefabHierarchyAssetGroups {
            game_objects,
            components,
            prefab_instances,
            prefab_asset,
            hierarchy_asset,
            representation,
        }
    }

    #[test]
    fn prefab_export_ids_follow_add_order_and_each_assets_unity_version() {
        let game_objects = [member(0, 0, 10, 1, "2022.3.1f1")];
        let components = [member(0, 1, 20, 4, "5.4.7f1")];
        let prefab_instances = [member(0, 2, 30, 1001, "2022.3.1f1")];
        let prefab = member(0, 3, 40, 1001, "2022.3.1f1");
        let hierarchy = member(1, 0, 1, 1001, "2022.3.1f1");
        let plan = plan_prefab_export_ids(
            groups(
                &game_objects,
                &components,
                &prefab_instances,
                prefab,
                hierarchy,
                PrefabAssetRepresentation::LegacyPrefabMarker,
            ),
            &[],
            PrefabExportPlanLimits::default(),
        )
        .unwrap();

        assert_eq!(plan.registered_assets.len(), 5);
        assert_eq!(plan.registered_assets[0].asset, prefab.identity);
        assert!(plan.registered_assets[0].is_main_asset);
        assert_eq!(
            plan.registered_assets[0].file_id,
            get_main_export_id(PREFAB_INSTANCE_CLASS_ID, 0).unwrap()
        );
        assert_eq!(plan.registered_assets[1].asset, game_objects[0].identity);
        assert_eq!(
            plan.registered_assets[1].file_id,
            get_pseudo_random_export_id(1, 0, game_objects[0].unity_version)
        );
        assert_eq!(plan.registered_assets[2].asset, components[0].identity);
        assert_eq!(
            plan.registered_assets[2].file_id,
            get_pseudo_random_export_id(4, 1, components[0].unity_version)
        );
        assert_eq!(
            plan.registered_assets[3].asset,
            prefab_instances[0].identity
        );
        assert_eq!(plan.registered_assets[4].asset, hierarchy.identity);
        assert_eq!(
            plan.exportable_assets,
            [
                game_objects[0].identity,
                components[0].identity,
                prefab_instances[0].identity,
                prefab.identity
            ]
        );
        assert!(plan.emit_prefab_asset);
    }

    #[test]
    fn prefab_export_plan_skips_hidden_prefab_and_generates_source_pointer_by_representation() {
        let empty = [];
        let prefab = member(0, 0, 40, 1001, "2022.3.1f1");
        let hierarchy = member(1, 0, 1, 1001, "2022.3.1f1");
        let hidden = [prefab.identity.object_key()];
        let plan = plan_prefab_export_ids(
            groups(
                &empty,
                &empty,
                &empty,
                prefab,
                hierarchy,
                PrefabAssetRepresentation::PrefabInstanceMarker,
            ),
            &hidden,
            PrefabExportPlanLimits::default(),
        )
        .unwrap();

        assert_eq!(plan.exportable_assets, []);
        assert!(!plan.emit_prefab_asset);
        assert_eq!(
            generate_prefab_source_pointer([0x55; 16], plan.representation),
            Ok(UnityMetaPointer {
                file_id: get_main_export_id(1001, 0).unwrap(),
                guid: ExportPointerGuid::Collection([0x55; 16]),
                asset_type: UnityProjectAssetType::Meta,
            })
        );
        assert_eq!(
            generate_prefab_source_pointer([0; 16], plan.representation),
            Err(PrefabExportPlanError::ZeroCollectionGuid)
        );
    }

    #[test]
    fn prefab_export_plan_deduplicates_members_without_advancing_seed() {
        let shared = member(0, 0, 10, 1, "2022.3.1f1");
        let game_objects = [shared, shared];
        let components = [member(0, 1, 20, 4, "2022.3.1f1")];
        let empty = [];
        let prefab = member(0, 2, 40, 1001, "2022.3.1f1");
        let hierarchy = member(1, 0, 1, 1001, "2022.3.1f1");
        let plan = plan_prefab_export_ids(
            groups(
                &game_objects,
                &components,
                &empty,
                prefab,
                hierarchy,
                PrefabAssetRepresentation::Other,
            ),
            &[],
            PrefabExportPlanLimits::default(),
        )
        .unwrap();

        assert_eq!(plan.registered_assets.len(), 4);
        assert_eq!(
            plan.registered_assets[1].file_id,
            get_pseudo_random_export_id(1, 0, shared.unity_version)
        );
        assert_eq!(
            plan.registered_assets[2].file_id,
            get_pseudo_random_export_id(4, 1, components[0].unity_version)
        );
        assert!(plan.exportable_assets.contains(&prefab.identity));
        assert!(!plan.emit_prefab_asset);

        let version_conflict = member(0, 0, 10, 1, "5.4.7f1");
        let conflicting_members = [shared, version_conflict];
        assert!(matches!(
            plan_prefab_export_ids(
                groups(
                    &conflicting_members,
                    &empty,
                    &empty,
                    prefab,
                    hierarchy,
                    PrefabAssetRepresentation::Other,
                ),
                &[],
                PrefabExportPlanLimits::default(),
            ),
            Err(PrefabExportPlanError::DuplicateObjectMetadata {
                first_version,
                duplicate_version,
                ..
            }) if first_version == shared.unity_version
                && duplicate_version == version_conflict.unity_version
        ));
    }

    #[test]
    fn prefab_export_plan_enforces_collection_asset_limit() {
        let empty = [];
        let prefab = member(0, 0, 40, 1001, "2022.3.1f1");
        let hierarchy = member(1, 0, 1, 1001, "2022.3.1f1");
        assert!(matches!(
            plan_prefab_export_ids(
                groups(
                    &empty,
                    &empty,
                    &empty,
                    prefab,
                    hierarchy,
                    PrefabAssetRepresentation::Other,
                ),
                &[],
                PrefabExportPlanLimits { max_assets: 1 },
            ),
            Err(PrefabExportPlanError::AssetLimitExceeded {
                requested: 2,
                limit: 1,
            })
        ));
    }
}
