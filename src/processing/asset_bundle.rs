//! AssetBundle naming and Container pointer resolution.
//!
//! Ports `OriginalPathProcessor.Process` (both passes), `OriginalPathHelper`, and
//! `AssetBundleExtensions.GetAssetBundleName`
//! (`AssetRipper/Source/AssetRipper.Processing/Scenes/OriginalPathProcessor.cs`,
//! `AssetRipper/Source/AssetRipper.Processing/OriginalPathHelper.cs`,
//! `AssetRipper/Source/AssetRipper.SourceGenerated.Extensions/AssetBundleExtensions.cs`):
//!
//! - Bundle discovery reads the serialized name first and falls back to the owning
//!   Bundle's name (`GameBundle.Name` is literally `"GameBundle"`, matched here); a
//!   trailing `.bundle` is removed case-insensitively.
//! - Shared Container entries (`AssetInfo.Asset.FileID != 0`) resolve to non-local
//!   targets and never receive bundle names or paths, matching the source skip.
//! - ResourceManager paths replace only when the candidate is longer, measured in
//!   UTF-16 units exactly like the source `string.Length` comparison.
//! - GroupByBundleName collection directories apply only when unset (`??=`), and only
//!   for bundles outside the game bundle.
//! - `UndoPathLowercasing` restores ASCII case-insensitive matches; non-ASCII pairs are
//!   conservatively declined and counted (the source invariant-culture comparison has no
//!   workspace equivalent here).
//! - Shader override directory/name/extension fill only when unset (`??=`).
//! - Unresolvable Container targets surface as auditable plan issues instead of the
//!   source's silent skips.
//!
//! Deliberately open (audited, not missing):
//! - `AssetBundleExtensions.HasPathExtension` has zero call sites in the frozen baseline,
//!   so no gate is ported.
//! - The `IsStreamedSceneAssetBundle` `Debug.Assert` in `PrefabProcessor` has no release
//!   behavior; streamed-scene path recovery itself lives in `scene_collections.rs`.

mod bundle_names;
mod path_application;
mod path_model;
mod resource_manager;

pub use self::bundle_names::{
    asset_bundle_name_for_asset, get_asset_bundle_name, map_asset_bundle_container,
    plan_workspace_asset_bundle_names, resolve_asset_bundle_container_reference,
    AssetBundleContainerMapError, AssetBundleContainerMapping, AssetBundleContainerNameMap,
    AssetBundleContainerTarget, AssetBundleNameDecodeError, WorkspaceAssetBundleNameAssignment,
    WorkspaceAssetBundleNameIssue, WorkspaceAssetBundleNameIssueKind,
    WorkspaceAssetBundleNameLimits, WorkspaceAssetBundleNamePlan,
    WorkspaceAssetBundleNamePlanError, WorkspaceAssetBundleNameRecord,
    WorkspaceAssetBundleReferenceRecord,
};
pub use self::path_application::{
    apply_workspace_asset_bundle_paths, apply_workspace_original_paths,
};
pub use self::path_model::{
    BundledAssetsExportMode, WorkspaceAssetBundlePathError, WorkspaceAssetExportPath,
    WorkspaceAssetExportPathLimits, WorkspaceAssetExportPathReport, WorkspaceAssetExportPathTable,
    WorkspaceOriginalPathPlans,
};
pub use self::resource_manager::{
    plan_workspace_resource_manager_paths, WorkspaceResourceManagerPathAssignment,
    WorkspaceResourceManagerPathIssue, WorkspaceResourceManagerPathIssueKind,
    WorkspaceResourceManagerPathLimits, WorkspaceResourceManagerPathPlan,
    WorkspaceResourceManagerPathPlanError,
};

#[cfg(test)]
#[path = "asset_bundle/tests/mod.rs"]
mod tests;
