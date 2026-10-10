//! File-scoped GameObject and Transform relationship recovery.
//!
//! Also ports the `SceneHierarchyObject` assembly decisions: the `Create` asset-sort
//! dispatch, the `FetchDependencies` edge order, and the `GetRoots` root filter over
//! caller-reported `IsRoot` facts. Live interface checks, pointer
//! resolution, generated `SceneAsset` allocation, and `SetMainAsset` plumbing stay
//! caller-side; repeated `SceneRoots` assignments overwrite, so callers keep the last.
//! The root facts arrive post-resolution, so this filter never invents roots from
//! unresolved links: the caller reports only whether each father pointer is null.

include!("scene_models_and_errors.rs");
include!("scene_graph_accessors.rs");
include!("scene_graph_builder.rs");

#[cfg(test)]
mod tests {
    include!("scene_tests_01.rs");
    include!("scene_tests_02.rs");
}
