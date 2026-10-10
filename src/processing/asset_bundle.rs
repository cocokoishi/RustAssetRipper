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

include!("asset_bundle_names_and_plans.rs");
include!("asset_bundle_path_application.rs");

#[cfg(test)]
mod tests {
    include!("asset_bundle_tests_01.rs");
}
