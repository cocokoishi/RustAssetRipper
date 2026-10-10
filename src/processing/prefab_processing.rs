//! Identity-level reconstruction of AssetRipper's workspace PrefabProcessor order.

include!("prefab_processing_plans.rs");
include!("prefab_asset_materialization.rs");

#[cfg(test)]
mod tests {
    include!("prefab_processing_tests_01.rs");
}
