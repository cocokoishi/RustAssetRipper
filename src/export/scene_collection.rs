//! Ordered export-ID assignment for one Scene YAML collection.
//!
//! The ID-assignment order runs over the unfiltered hierarchy membership, while the YAML
//! display order runs over the exportable selection below before the `Compare` display
//! sort (`scene_member_order.rs`); selection therefore stays a separate step here rather
//! than folded into either order.

use std::collections::HashSet;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::processing::prefab_processing::{
    WorkspaceProcessedCollectionKind, WorkspaceProcessedPrefabAssetKind,
    WorkspaceProcessedPrefabAssetPlan,
};
use crate::processing::scene_collections::scene_manager_kind;
use crate::processing::scene_hierarchy::{
    WorkspaceSceneHierarchyMembershipError, WorkspaceSceneHierarchyMembershipIssue,
    WorkspaceSceneHierarchyMembershipLimits, WorkspaceSceneHierarchyScene,
    WorkspaceSceneObjectIdentity,
};
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

/// Build the Scene ID plan directly from one source SceneDefinition hierarchy.
///
/// This adapter refuses incomplete or ambiguously classified membership. It converts the
/// hierarchy's source category groups to export identities, concatenates them in baseline
/// `SceneHierarchyObject.Assets` order, then assigns fileIDs using the requested Unity version.
pub fn plan_workspace_scene_export_ids(
    scene: &WorkspaceSceneHierarchyScene,
    unity_version: SceneUnityVersion,
    membership_limits: WorkspaceSceneHierarchyMembershipLimits,
    order_limits: SceneHierarchyAssetOrderLimits,
    plan_limits: SceneExportIdPlanLimits,
) -> Result<Vec<SceneExportIdAssignment>, WorkspaceSceneExportPlanError> {
    plan_workspace_scene_export_ids_internal(
        scene,
        None,
        unity_version,
        membership_limits,
        order_limits,
        plan_limits,
    )
}

/// Build the Scene ID plan including PrefabProcessor-generated Transforms attached to this
/// SceneDefinition. This follows `SceneDefinition.Assets` collection order: source collections
/// first, then its appended `SceneGeneratedAssets` collection and the generated components in
/// that collection's insertion order. `SceneHierarchyObject.Create` sees its generated SceneAsset
/// while iterating `SceneDefinition.Assets` but does not classify it into the hierarchy asset
/// groups, so it is deliberately not assigned a scene-file ID here.
pub fn plan_workspace_scene_export_ids_with_processed_assets(
    scene: &WorkspaceSceneHierarchyScene,
    scene_index: usize,
    processed_assets: &WorkspaceProcessedPrefabAssetPlan,
    unity_version: SceneUnityVersion,
    membership_limits: WorkspaceSceneHierarchyMembershipLimits,
    order_limits: SceneHierarchyAssetOrderLimits,
    plan_limits: SceneExportIdPlanLimits,
) -> Result<Vec<SceneExportIdAssignment>, WorkspaceSceneExportPlanError> {
    plan_workspace_scene_export_ids_internal(
        scene,
        Some((scene_index, processed_assets)),
        unity_version,
        membership_limits,
        order_limits,
        plan_limits,
    )
}

fn plan_workspace_scene_export_ids_internal(
    scene: &WorkspaceSceneHierarchyScene,
    processed_assets: Option<(usize, &WorkspaceProcessedPrefabAssetPlan)>,
    unity_version: SceneUnityVersion,
    membership_limits: WorkspaceSceneHierarchyMembershipLimits,
    order_limits: SceneHierarchyAssetOrderLimits,
    plan_limits: SceneExportIdPlanLimits,
) -> Result<Vec<SceneExportIdAssignment>, WorkspaceSceneExportPlanError> {
    let membership = scene
        .asset_membership(membership_limits)
        .map_err(WorkspaceSceneExportPlanError::Membership)?;
    if !membership.is_complete() {
        let mut issues = Vec::new();
        issues
            .try_reserve_exact(membership.issues().len())
            .map_err(|_| WorkspaceSceneExportPlanError::AllocationFailed {
                resource: "incomplete Scene hierarchy membership diagnostics",
                requested: membership.issues().len(),
            })?;
        issues.extend_from_slice(membership.issues());
        return Err(WorkspaceSceneExportPlanError::IncompleteMembership { issues });
    }

    let game_objects = workspace_identities_to_export_assets(membership.game_objects())?;
    let mut components = workspace_identities_to_export_assets(membership.components())?;
    if let Some((scene_index, processed_assets)) = processed_assets {
        let generated_components = processed_scene_generated_components(
            &membership,
            scene_index,
            processed_assets,
            order_limits,
        )?;
        let requested = components
            .len()
            .checked_add(generated_components.len())
            .ok_or(WorkspaceSceneExportPlanError::AssetOrder(
                SceneHierarchyAssetOrderError::AssetCountOverflow,
            ))?;
        if requested > order_limits.max_assets {
            return Err(WorkspaceSceneExportPlanError::AssetOrder(
                SceneHierarchyAssetOrderError::AssetLimitExceeded {
                    requested,
                    limit: order_limits.max_assets,
                },
            ));
        }
        components
            .try_reserve(generated_components.len())
            .map_err(|_| WorkspaceSceneExportPlanError::AllocationFailed {
                resource: "Scene generated component export identities",
                requested,
            })?;
        components.extend(generated_components);
    }
    let prefab_instances = workspace_identities_to_export_assets(membership.prefab_instances())?;
    let managers = workspace_identities_to_export_assets(membership.managers())?;
    let scene_roots = membership
        .scene_roots()
        .map(workspace_identity_to_export_asset)
        .transpose()?;
    let assets = order_scene_hierarchy_assets(
        SceneHierarchyAssetGroups {
            game_objects: &game_objects,
            components: &components,
            prefab_instances: &prefab_instances,
            managers: &managers,
            scene_roots,
        },
        order_limits,
    )
    .map_err(WorkspaceSceneExportPlanError::AssetOrder)?;
    plan_scene_export_ids(&assets, unity_version, plan_limits)
        .map_err(WorkspaceSceneExportPlanError::IdPlan)
}

fn processed_scene_generated_components(
    membership: &crate::processing::scene_hierarchy::WorkspaceSceneHierarchyMembership,
    scene_index: usize,
    processed_assets: &WorkspaceProcessedPrefabAssetPlan,
    limits: SceneHierarchyAssetOrderLimits,
) -> Result<Vec<ExportAssetIdentity>, WorkspaceSceneExportPlanError> {
    if !processed_assets.is_complete() {
        return Err(
            WorkspaceSceneExportPlanError::ProcessedAssetPlanIncomplete {
                reason: processed_assets.incomplete_reason().map(str::to_owned),
            },
        );
    }

    let mut scene_collection = None;
    for collection in processed_assets.collections() {
        if !matches!(
            &collection.kind,
            WorkspaceProcessedCollectionKind::SceneGeneratedAssets {
                scene_index: candidate_scene_index
            } if *candidate_scene_index == scene_index
        ) {
            continue;
        }
        if scene_collection.replace(collection).is_some() {
            return Err(
                WorkspaceSceneExportPlanError::DuplicateSceneGeneratedCollection { scene_index },
            );
        }
    }
    let scene_collection = scene_collection
        .ok_or(WorkspaceSceneExportPlanError::MissingSceneGeneratedCollection { scene_index })?;

    let mut scene_game_objects = HashSet::new();
    scene_game_objects
        .try_reserve(membership.game_objects().len())
        .map_err(|_| WorkspaceSceneExportPlanError::AllocationFailed {
            resource: "Scene GameObject identity index for generated components",
            requested: membership.game_objects().len(),
        })?;
    scene_game_objects.extend(membership.game_objects().iter().copied());

    let generated_component_count = scene_collection
        .assets
        .iter()
        .filter(|asset| {
            matches!(
                &asset.kind,
                WorkspaceProcessedPrefabAssetKind::GeneratedTransform { .. }
            )
        })
        .count();
    if generated_component_count > limits.max_assets {
        return Err(WorkspaceSceneExportPlanError::AssetOrder(
            SceneHierarchyAssetOrderError::AssetLimitExceeded {
                requested: generated_component_count,
                limit: limits.max_assets,
            },
        ));
    }
    let mut generated_components = Vec::new();
    generated_components
        .try_reserve_exact(generated_component_count)
        .map_err(|_| WorkspaceSceneExportPlanError::AllocationFailed {
            resource: "Scene generated component export identities",
            requested: generated_component_count,
        })?;

    for (object_index, asset) in scene_collection.assets.iter().enumerate() {
        if asset.identity.collection_index != scene_collection.collection_index {
            return Err(
                WorkspaceSceneExportPlanError::ProcessedAssetCollectionIndexMismatch {
                    scene_index,
                    collection_index: scene_collection.collection_index,
                    asset_index: object_index,
                    asset_collection_index: asset.identity.collection_index,
                },
            );
        }
        match &asset.kind {
            WorkspaceProcessedPrefabAssetKind::GeneratedTransform { game_object, .. } => {
                if asset.identity.class_id != 4 {
                    return Err(
                        WorkspaceSceneExportPlanError::GeneratedTransformClassMismatch {
                            scene_index,
                            collection_index: scene_collection.collection_index,
                            asset_index: object_index,
                            actual_class_id: asset.identity.class_id,
                        },
                    );
                }
                if !scene_game_objects.contains(game_object) {
                    return Err(
                        WorkspaceSceneExportPlanError::GeneratedTransformTargetOutsideScene {
                            scene_index,
                            collection_index: scene_collection.collection_index,
                            asset_index: object_index,
                            game_object: *game_object,
                        },
                    );
                }
                generated_components.push(ExportAssetIdentity {
                    collection: ExportAssetCollectionIdentity::ProcessedCollection {
                        collection_index: scene_collection.collection_index,
                    },
                    object_index,
                    path_id: asset.identity.path_id,
                    class_id: asset.identity.class_id,
                });
            }
            WorkspaceProcessedPrefabAssetKind::SceneHierarchy {
                scene_index: asset_scene_index,
            } if *asset_scene_index == scene_index && asset.identity.class_id == 1032 => {}
            _ => {
                return Err(
                    WorkspaceSceneExportPlanError::UnsupportedSceneGeneratedAsset {
                        scene_index,
                        collection_index: scene_collection.collection_index,
                        asset_index: object_index,
                    },
                );
            }
        }
    }
    Ok(generated_components)
}

fn workspace_identities_to_export_assets(
    identities: &[WorkspaceSceneObjectIdentity],
) -> Result<Vec<ExportAssetIdentity>, WorkspaceSceneExportPlanError> {
    let mut assets = Vec::new();
    assets.try_reserve_exact(identities.len()).map_err(|_| {
        WorkspaceSceneExportPlanError::AllocationFailed {
            resource: "Scene hierarchy export identities",
            requested: identities.len(),
        }
    })?;
    for &identity in identities {
        assets.push(workspace_identity_to_export_asset(identity)?);
    }
    Ok(assets)
}

fn workspace_identity_to_export_asset(
    identity: WorkspaceSceneObjectIdentity,
) -> Result<ExportAssetIdentity, WorkspaceSceneExportPlanError> {
    let Some(class_id) = identity.class_id else {
        return Err(WorkspaceSceneExportPlanError::MissingClassId { identity });
    };
    Ok(ExportAssetIdentity {
        collection: ExportAssetCollectionIdentity::SerializedFile {
            file_index: identity.file_index,
        },
        object_index: identity.object_index,
        path_id: identity.path_id,
        class_id,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceSceneExportPlanError {
    Membership(WorkspaceSceneHierarchyMembershipError),
    IncompleteMembership {
        issues: Vec<WorkspaceSceneHierarchyMembershipIssue>,
    },
    MissingClassId {
        identity: WorkspaceSceneObjectIdentity,
    },
    ProcessedAssetPlanIncomplete {
        reason: Option<String>,
    },
    MissingSceneGeneratedCollection {
        scene_index: usize,
    },
    DuplicateSceneGeneratedCollection {
        scene_index: usize,
    },
    ProcessedAssetCollectionIndexMismatch {
        scene_index: usize,
        collection_index: usize,
        asset_index: usize,
        asset_collection_index: usize,
    },
    GeneratedTransformClassMismatch {
        scene_index: usize,
        collection_index: usize,
        asset_index: usize,
        actual_class_id: i32,
    },
    GeneratedTransformTargetOutsideScene {
        scene_index: usize,
        collection_index: usize,
        asset_index: usize,
        game_object: WorkspaceSceneObjectIdentity,
    },
    UnsupportedSceneGeneratedAsset {
        scene_index: usize,
        collection_index: usize,
        asset_index: usize,
    },
    AssetOrder(SceneHierarchyAssetOrderError),
    IdPlan(SceneExportIdPlanError),
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
}

impl Display for WorkspaceSceneExportPlanError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Membership(error) => {
                write!(f, "could not build Scene hierarchy membership: {error}")
            }
            Self::IncompleteMembership { issues } => write!(
                f,
                "Scene hierarchy membership has {} unresolved classification/scan issue(s)",
                issues.len()
            ),
            Self::MissingClassId { identity } => write!(
                f,
                "Scene hierarchy asset {}:{} has no class ID for export planning",
                identity.file_index, identity.object_index
            ),
            Self::ProcessedAssetPlanIncomplete { reason } => {
                f.write_str("processed asset plan is incomplete")?;
                if let Some(reason) = reason {
                    write!(f, ": {reason}")?;
                }
                Ok(())
            }
            Self::MissingSceneGeneratedCollection { scene_index } => write!(
                f,
                "processed asset plan has no generated collection for scene {scene_index}"
            ),
            Self::DuplicateSceneGeneratedCollection { scene_index } => write!(
                f,
                "processed asset plan has multiple generated collections for scene {scene_index}"
            ),
            Self::ProcessedAssetCollectionIndexMismatch {
                scene_index,
                collection_index,
                asset_index,
                asset_collection_index,
            } => write!(
                f,
                "scene {scene_index} generated collection {collection_index} asset {asset_index} claims collection {}",
                asset_collection_index
            ),
            Self::GeneratedTransformClassMismatch {
                scene_index,
                collection_index,
                asset_index,
                actual_class_id,
            } => write!(
                f,
                "scene {scene_index} generated collection {collection_index} Transform asset {asset_index} has class ID {actual_class_id}"
            ),
            Self::GeneratedTransformTargetOutsideScene {
                scene_index,
                collection_index,
                asset_index,
                game_object,
            } => write!(
                f,
                "scene {scene_index} generated collection {collection_index} Transform asset {asset_index} targets GameObject {}:{} outside the scene",
                game_object.file_index, game_object.object_index
            ),
            Self::UnsupportedSceneGeneratedAsset {
                scene_index,
                collection_index,
                asset_index,
            } => write!(
                f,
                "scene {scene_index} generated collection {collection_index} contains unsupported hierarchy asset {asset_index}"
            ),
            Self::AssetOrder(error) => write!(f, "could not order Scene hierarchy assets: {error}"),
            Self::IdPlan(error) => write!(f, "could not assign Scene export IDs: {error}"),
            Self::AllocationFailed {
                resource,
                requested,
            } => {
                write!(f, "could not allocate {requested} entries for {resource}")
            }
        }
    }
}

impl Error for WorkspaceSceneExportPlanError {}

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SceneYamlAssetOrderLimits {
    pub max_assets: usize,
}

impl Default for SceneYamlAssetOrderLimits {
    fn default() -> Self {
        Self {
            max_assets: 20_000_000,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SceneYamlAssetOrderError {
    AssetLimitExceeded { count: usize, limit: usize },
    AllocationFailed { requested: usize },
}

impl Display for SceneYamlAssetOrderError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AssetLimitExceeded { count, limit } => write!(
                f,
                "Scene YAML collection has {count} assets, exceeding limit {limit}"
            ),
            Self::AllocationFailed { requested } => write!(
                f,
                "could not allocate a Scene YAML ordering for {requested} assets"
            ),
        }
    }
}

impl Error for SceneYamlAssetOrderError {}

/// Apply the stable ordering used by `SceneExportCollection.Compare` to already selected,
/// hierarchy-ordered Scene assignments. LevelGameManager assets sort first by class ID; all
/// other pairs compare equal and therefore retain their input order.
pub fn order_scene_yaml_assignments(
    assignments_in_hierarchy_order: &[SceneExportIdAssignment],
    limits: SceneYamlAssetOrderLimits,
) -> Result<Vec<SceneExportIdAssignment>, SceneYamlAssetOrderError> {
    if assignments_in_hierarchy_order.len() > limits.max_assets {
        return Err(SceneYamlAssetOrderError::AssetLimitExceeded {
            count: assignments_in_hierarchy_order.len(),
            limit: limits.max_assets,
        });
    }
    let mut ordered = Vec::new();
    ordered
        .try_reserve_exact(assignments_in_hierarchy_order.len())
        .map_err(|_| SceneYamlAssetOrderError::AllocationFailed {
            requested: assignments_in_hierarchy_order.len(),
        })?;
    ordered.extend_from_slice(assignments_in_hierarchy_order);
    ordered.sort_by(|left, right| {
        let left_is_manager = scene_manager_kind(Some(left.asset.class_id)).is_some();
        let right_is_manager = scene_manager_kind(Some(right.asset.class_id)).is_some();
        match (left_is_manager, right_is_manager) {
            (true, true) => left.asset.class_id.cmp(&right.asset.class_id),
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            (false, false) => std::cmp::Ordering::Equal,
        }
    });
    Ok(ordered)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SceneExportableAssetLimits {
    pub max_assets: usize,
}

impl Default for SceneExportableAssetLimits {
    fn default() -> Self {
        Self {
            max_assets: 20_000_000,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SceneExportableAssetError {
    AssetLimitExceeded {
        requested: usize,
        limit: usize,
    },
    HiddenLimitExceeded {
        requested: usize,
        limit: usize,
    },
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
}

impl Display for SceneExportableAssetError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AssetLimitExceeded { requested, limit } => write!(
                f,
                "Scene hierarchy has {requested} members, exceeding limit {limit}"
            ),
            Self::HiddenLimitExceeded { requested, limit } => write!(
                f,
                "Scene hierarchy has {requested} hidden assets, exceeding limit {limit}"
            ),
            Self::AllocationFailed {
                resource,
                requested,
            } => {
                write!(f, "could not allocate {requested} entries for {resource}")
            }
        }
    }
}

impl Error for SceneExportableAssetError {}

/// `GameObjectHierarchyObject.ExportableAssets` for the scene path: hierarchy members
/// minus hidden assets, in hierarchy order.
///
/// The scene collection snapshots `componentArray` from this selection before the
/// `Compare` display sort. Hidden membership compares by object key (collection scope
/// plus object-table index), matching the prefab path's hidden index; the source
/// reference-equality check and this key check agree because one key identifies one
/// object instance. Members sharing a hidden key are all removed; the source `Where`
/// likewise never deduplicates.
pub fn select_scene_exportable_assets(
    members_in_hierarchy_order: &[ExportAssetIdentity],
    hidden_assets: &[ExportAssetObjectKey],
    limits: SceneExportableAssetLimits,
) -> Result<Vec<ExportAssetIdentity>, SceneExportableAssetError> {
    if members_in_hierarchy_order.len() > limits.max_assets {
        return Err(SceneExportableAssetError::AssetLimitExceeded {
            requested: members_in_hierarchy_order.len(),
            limit: limits.max_assets,
        });
    }
    if hidden_assets.len() > limits.max_assets {
        return Err(SceneExportableAssetError::HiddenLimitExceeded {
            requested: hidden_assets.len(),
            limit: limits.max_assets,
        });
    }
    let mut hidden = HashSet::new();
    hidden.try_reserve(hidden_assets.len()).map_err(|_| {
        SceneExportableAssetError::AllocationFailed {
            resource: "Scene hidden-asset index",
            requested: hidden_assets.len(),
        }
    })?;
    hidden.extend(hidden_assets.iter().copied());
    let mut exportable = Vec::new();
    exportable
        .try_reserve_exact(members_in_hierarchy_order.len())
        .map_err(|_| SceneExportableAssetError::AllocationFailed {
            resource: "Scene exportable hierarchy assets",
            requested: members_in_hierarchy_order.len(),
        })?;
    for member in members_in_hierarchy_order {
        if !hidden.contains(&member.object_key()) {
            exportable.push(*member);
        }
    }
    Ok(exportable)
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

    #[test]
    fn scene_yaml_order_moves_managers_first_and_stably_sorts_manager_class_ids() {
        let assignments = [
            SceneExportIdAssignment {
                asset: source(0, 0, 10, 1),
                file_id: 10,
            },
            SceneExportIdAssignment {
                asset: source(0, 1, 11, 196),
                file_id: 11,
            },
            SceneExportIdAssignment {
                asset: source(0, 2, 12, 4),
                file_id: 12,
            },
            SceneExportIdAssignment {
                asset: source(0, 3, 13, 104),
                file_id: 13,
            },
            SceneExportIdAssignment {
                asset: source(0, 4, 14, 29),
                file_id: 14,
            },
            SceneExportIdAssignment {
                asset: source(0, 5, 15, 157),
                file_id: 15,
            },
        ];

        let ordered =
            order_scene_yaml_assignments(&assignments, SceneYamlAssetOrderLimits::default())
                .unwrap();
        assert_eq!(
            ordered
                .iter()
                .map(|entry| entry.file_id)
                .collect::<Vec<_>>(),
            [14, 13, 15, 11, 10, 12]
        );
    }

    #[test]
    fn scene_yaml_order_preserves_same_class_manager_and_nonmanager_input_order() {
        let assignments = [
            SceneExportIdAssignment {
                asset: source(0, 0, 10, 1),
                file_id: 10,
            },
            SceneExportIdAssignment {
                asset: source(0, 1, 11, 4),
                file_id: 11,
            },
            SceneExportIdAssignment {
                asset: source(0, 2, 12, 104),
                file_id: 12,
            },
            SceneExportIdAssignment {
                asset: source(0, 3, 13, 104),
                file_id: 13,
            },
        ];

        let ordered =
            order_scene_yaml_assignments(&assignments, SceneYamlAssetOrderLimits::default())
                .unwrap();
        assert_eq!(
            ordered
                .iter()
                .map(|entry| entry.file_id)
                .collect::<Vec<_>>(),
            [12, 13, 10, 11]
        );
        assert!(matches!(
            order_scene_yaml_assignments(&assignments, SceneYamlAssetOrderLimits { max_assets: 3 }),
            Err(SceneYamlAssetOrderError::AssetLimitExceeded { count: 4, limit: 3 })
        ));
    }

    #[test]
    fn scene_exportable_selection_keeps_order_and_removes_hidden() {
        let members = [
            source(0, 0, 10, 1),
            source(0, 1, 11, 4),
            source(0, 2, 12, 104),
            source(0, 3, 13, 1),
        ];
        // Hidden keys remove their members; unknown keys are ignored.
        let hidden = [members[1].object_key(), source(7, 7, 70, 4).object_key()];
        assert_eq!(
            select_scene_exportable_assets(
                &members,
                &hidden,
                SceneExportableAssetLimits::default()
            ),
            Ok(vec![members[0], members[2], members[3]])
        );
        // No hidden assets keeps every member in hierarchy order.
        assert_eq!(
            select_scene_exportable_assets(&members, &[], SceneExportableAssetLimits::default()),
            Ok(members.to_vec())
        );
    }

    #[test]
    fn scene_exportable_selection_rejects_overruns() {
        let members = [source(0, 0, 10, 1), source(0, 1, 11, 4)];
        assert_eq!(
            select_scene_exportable_assets(
                &members,
                &[],
                SceneExportableAssetLimits { max_assets: 1 }
            ),
            Err(SceneExportableAssetError::AssetLimitExceeded {
                requested: 2,
                limit: 1,
            })
        );
        let hidden = [members[0].object_key(), members[1].object_key()];
        assert_eq!(
            select_scene_exportable_assets(
                &members[..1],
                &hidden,
                SceneExportableAssetLimits { max_assets: 1 }
            ),
            Err(SceneExportableAssetError::HiddenLimitExceeded {
                requested: 2,
                limit: 1,
            })
        );
    }
}
