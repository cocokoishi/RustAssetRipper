use std::collections::HashMap;

use super::*;

/// Candidate asset-bundle name that the source processor tries to assign to a scene hierarchy.
///
/// Mirrors the `PrefabProcessor.Process` scene pass
/// (`AssetRipper/Source/AssetRipper.Processing/Prefabs/PrefabProcessor.cs`,
/// lines 28-48): the first `SerializedBundle` among the scene collections
/// supplies the candidate; its first `IAssetBundle` asset's `GetAssetBundleName`
/// wins when present, otherwise the bundle name is used. With no `SerializedBundle`
/// the source does not attempt an assignment. `GeneratedObjectBase.AssetBundleName`
/// has a no-op setter, so the effective generated hierarchy property remains null.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceSceneHierarchyBundleName {
    /// No `SerializedBundle` among the scene collections: no assignment candidate.
    Unset,
    /// A `SerializedBundle` without an `IAssetBundle` asset: `bundle.Name`.
    BundleName(String),
    /// The first `IAssetBundle` asset's `GetAssetBundleName()`.
    AssetName(String),
}

/// Selects the candidate that `PrefabProcessor` attempts to assign to the hierarchy.
///
/// `has_serialized_bundle` records the `FirstOrDefault(b => b is SerializedBundle)`
/// lookup and gates everything, matching the nested `if (bundle is not null)`.
/// `bundle_name` is that bundle's `Name`; `asset_bundle_name` carries the first
/// `IAssetBundle` asset's resolved `GetAssetBundleName()` (`Some` when such an
/// asset exists, `None` otherwise). Collection traversal order, the
/// `FetchAssets` order, and the `GetAssetBundleName` resolution itself (see
/// `asset_bundle.rs`) stay caller-side, as does the debug-only streamed-scene
/// assertion, which is intentionally not modeled. The generated `SceneHierarchyObject` inherits
/// a no-op `AssetBundleName` setter from `GeneratedObjectBase`, so the candidate is discarded and
/// the effective property remains unset.
pub fn scene_hierarchy_asset_bundle_name(
    has_serialized_bundle: bool,
    bundle_name: &str,
    asset_bundle_name: Option<&str>,
) -> WorkspaceSceneHierarchyBundleName {
    if !has_serialized_bundle {
        return WorkspaceSceneHierarchyBundleName::Unset;
    }
    match asset_bundle_name {
        Some(name) => WorkspaceSceneHierarchyBundleName::AssetName(name.to_owned()),
        None => WorkspaceSceneHierarchyBundleName::BundleName(bundle_name.to_owned()),
    }
}

/// One source bundle as seen by the `PrefabProcessor` scene pass.
///
/// `bundle_index` is the workspace container-bundle identity. `bundle_name`
/// is that bundle's `Name` (`FileContainer.Name` after
/// `SpecialFileNames.FixFileIdentifier`). `is_serialized_bundle` records the
/// `b is SerializedBundle` filter from
/// `scene.Collections.Select(c => c.Bundle).FirstOrDefault(b => b is SerializedBundle)`:
/// only serialized container bundles participate; standalone files and generated
/// processed collections never supply a hierarchy bundle name.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceSceneBundleSource {
    pub bundle_index: usize,
    pub bundle_name: String,
    pub is_serialized_bundle: bool,
}

/// One `IAssetBundle` asset in `Bundle.FetchAssets()` order.
///
/// `resolved_name` must already be the `AssetBundleExtensions.GetAssetBundleName()`
/// result for that asset (non-empty serialized `AssetBundleName_R` wins, otherwise
/// the owning bundle name), resolved with `asset_bundle::get_asset_bundle_name`.
/// The list for each bundle must follow `Bundle.FetchAssets()`:
/// direct collections in order, then child bundles recursively in source entry
/// order (`AssetRipper/Source/AssetRipper.Assets/Bundles/Bundle.cs`, lines 368-384).
/// `has_streamed_scene_flag` records `Has_IsStreamedSceneAssetBundle()` and
/// `is_streamed_scene_asset_bundle` records `IsStreamedSceneAssetBundle`; together
/// they replay the scene-pass `Debug.Assert`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceBundleAssetNameFact {
    pub resolved_name: String,
    pub has_streamed_scene_flag: bool,
    pub is_streamed_scene_asset_bundle: bool,
}

/// Per-scene hierarchy bundle assignment from the integrated scene pass.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceSceneHierarchyBundleAssignment {
    pub scene_index: usize,
    /// Candidate value that `PrefabProcessor` passes to the generated hierarchy setter.
    pub name: WorkspaceSceneHierarchyBundleName,
    /// Effective `SceneHierarchyObject.AssetBundleName` read-back value. The generated hierarchy
    /// inherits a no-op setter from `GeneratedObjectBase`, so the source leaves this unset.
    pub effective_asset_bundle_name: Option<String>,
    /// Present when the selected asset carries the streamed-scene field with a
    /// false value, which trips the source `Debug.Assert` in debug builds while
    /// release builds continue with the resolved name. Rust always continues with
    /// the resolved name and records this explicit warning instead of asserting.
    pub streamed_scene_assert_warning: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceSceneHierarchyBundlePlanLimits {
    pub max_scenes: usize,
    pub max_collections_per_scene: usize,
    pub max_bundle_name_bytes: usize,
    pub max_assets_per_bundle: usize,
}

impl Default for WorkspaceSceneHierarchyBundlePlanLimits {
    fn default() -> Self {
        Self {
            max_scenes: 1_000_000,
            max_collections_per_scene: 1_000_000,
            max_bundle_name_bytes: 16 * 1024 * 1024,
            max_assets_per_bundle: 1_000_000,
        }
    }
}

/// Plan every scene hierarchy's asset-bundle name in source order.
///
/// `scene_collection_file_indices` holds one entry per scene in
/// `GameBundle.Scenes` order; each inner list is that scene's
/// `SceneDefinition.Collections` order. `file_bundles` is indexed by workspace
/// file index; `Some` carries the owning bundle source and `None` means the file
/// has no `SerializedBundle` (standalone input). `bundle_assets_in_fetch_order`
/// maps a bundle index to its `IAssetBundle` facts in `FetchAssets()` order.
///
/// For each scene the first collection whose bundle is a serialized bundle wins
/// (`FirstOrDefault`), then that bundle's first `IAssetBundle` asset wins
/// (`FetchAssets().OfType<IAssetBundle>().FirstOrDefault()`): its resolved
/// `GetAssetBundleName()` becomes the candidate `AssetName`, while a bundle without such an
/// asset falls back to the candidate `bundle.Name`. A scene without any serialized bundle
/// stays `Unset`. All candidates are discarded by `GeneratedObjectBase.AssetBundleName`'s
/// no-op setter, leaving the effective property unset. The streamed-scene `Debug.Assert` becomes
/// an explicit warning; it never changes the candidate and never fails the plan.
pub fn plan_workspace_scene_hierarchy_bundle_names(
    scene_collection_file_indices: &[Vec<usize>],
    file_bundles: &[Option<WorkspaceSceneBundleSource>],
    bundle_assets_in_fetch_order: &HashMap<usize, Vec<WorkspaceBundleAssetNameFact>>,
    limits: WorkspaceSceneHierarchyBundlePlanLimits,
) -> Result<Vec<WorkspaceSceneHierarchyBundleAssignment>, WorkspacePrefabProcessingError> {
    if scene_collection_file_indices.len() > limits.max_scenes {
        return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
            resource: "scene hierarchy bundle scenes",
            requested: scene_collection_file_indices.len(),
            limit: limits.max_scenes,
        });
    }
    let mut assignments = Vec::new();
    assignments
        .try_reserve_exact(scene_collection_file_indices.len())
        .map_err(|_| WorkspacePrefabProcessingError::AllocationFailed {
            resource: "scene hierarchy bundle assignments",
            requested: scene_collection_file_indices.len(),
        })?;
    for (scene_index, collection_file_indices) in scene_collection_file_indices.iter().enumerate() {
        if collection_file_indices.len() > limits.max_collections_per_scene {
            return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
                resource: "scene hierarchy bundle collections",
                requested: collection_file_indices.len(),
                limit: limits.max_collections_per_scene,
            });
        }
        let mut selected_bundle: Option<&WorkspaceSceneBundleSource> = None;
        for &file_index in collection_file_indices {
            let bundle = file_bundles.get(file_index).ok_or_else(|| {
                WorkspacePrefabProcessingError::WorkspaceOperationFailed {
                    operation: "scene hierarchy bundle selection",
                    message: format!(
                        "scene {scene_index} collection file {file_index} is outside {} files",
                        file_bundles.len()
                    ),
                }
            })?;
            if let Some(source) = bundle {
                if source.bundle_name.len() > limits.max_bundle_name_bytes {
                    return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
                        resource: "scene hierarchy bundle name bytes",
                        requested: source.bundle_name.len(),
                        limit: limits.max_bundle_name_bytes,
                    });
                }
                if source.is_serialized_bundle {
                    selected_bundle = Some(source);
                    break;
                }
            }
        }
        let Some(bundle) = selected_bundle else {
            assignments.push(WorkspaceSceneHierarchyBundleAssignment {
                scene_index,
                name: WorkspaceSceneHierarchyBundleName::Unset,
                effective_asset_bundle_name: None,
                streamed_scene_assert_warning: None,
            });
            continue;
        };
        let assets = bundle_assets_in_fetch_order.get(&bundle.bundle_index);
        let Some(assets) = assets else {
            assignments.push(WorkspaceSceneHierarchyBundleAssignment {
                scene_index,
                name: WorkspaceSceneHierarchyBundleName::BundleName(bundle.bundle_name.clone()),
                effective_asset_bundle_name: None,
                streamed_scene_assert_warning: None,
            });
            continue;
        };
        if assets.len() > limits.max_assets_per_bundle {
            return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
                resource: "scene hierarchy bundle assets",
                requested: assets.len(),
                limit: limits.max_assets_per_bundle,
            });
        }
        let Some(first) = assets.first() else {
            assignments.push(WorkspaceSceneHierarchyBundleAssignment {
                scene_index,
                name: WorkspaceSceneHierarchyBundleName::BundleName(bundle.bundle_name.clone()),
                effective_asset_bundle_name: None,
                streamed_scene_assert_warning: None,
            });
            continue;
        };
        if first.resolved_name.len() > limits.max_bundle_name_bytes {
            return Err(WorkspacePrefabProcessingError::ResourceLimitExceeded {
                resource: "scene hierarchy asset bundle name bytes",
                requested: first.resolved_name.len(),
                limit: limits.max_bundle_name_bytes,
            });
        }
        let warning = if first.has_streamed_scene_flag && !first.is_streamed_scene_asset_bundle {
            Some(format!(
                "scene {scene_index} bundle '{}' asset '{:?}' carries IsStreamedSceneAssetBundle=false; source Debug.Assert would fire in debug builds, release continues with the resolved name",
                bundle.bundle_name, first.resolved_name
            ))
        } else {
            None
        };
        assignments.push(WorkspaceSceneHierarchyBundleAssignment {
            scene_index,
            name: WorkspaceSceneHierarchyBundleName::AssetName(first.resolved_name.clone()),
            effective_asset_bundle_name: None,
            streamed_scene_assert_warning: warning,
        });
    }
    Ok(assignments)
}
