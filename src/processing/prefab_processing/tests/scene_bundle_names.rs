use super::*;
use crate::serialized_file::object::SerializedValue;
use std::collections::HashMap;

#[test]
fn scene_hierarchy_bundle_name_stays_unset_without_a_serialized_bundle() {
    assert_eq!(
        scene_hierarchy_asset_bundle_name(false, "level0", None),
        WorkspaceSceneHierarchyBundleName::Unset
    );
    // The bundle lookup gates everything: even a resolved asset name is
    // ignored when no SerializedBundle was selected.
    assert_eq!(
        scene_hierarchy_asset_bundle_name(false, "level0", Some("scenes")),
        WorkspaceSceneHierarchyBundleName::Unset
    );
}

#[test]
fn scene_hierarchy_bundle_name_falls_back_to_the_bundle_name() {
    assert_eq!(
        scene_hierarchy_asset_bundle_name(true, "level0", None),
        WorkspaceSceneHierarchyBundleName::BundleName("level0".to_owned())
    );
}

#[test]
fn scene_hierarchy_bundle_name_prefers_the_asset_bundle_asset() {
    assert_eq!(
        scene_hierarchy_asset_bundle_name(true, "level0", Some("scenes")),
        WorkspaceSceneHierarchyBundleName::AssetName("scenes".to_owned())
    );
    // The resolved name wins even when it differs from the bundle name.
    assert_eq!(
        scene_hierarchy_asset_bundle_name(true, "shared", Some("shared")),
        WorkspaceSceneHierarchyBundleName::AssetName("shared".to_owned())
    );
}

fn bundle_source(bundle_index: usize, bundle_name: &str) -> WorkspaceSceneBundleSource {
    WorkspaceSceneBundleSource {
        bundle_index,
        bundle_name: bundle_name.to_owned(),
        is_serialized_bundle: true,
    }
}

fn asset_fact(
    resolved_name: &str,
    has_streamed_scene_flag: bool,
    is_streamed_scene_asset_bundle: bool,
) -> WorkspaceBundleAssetNameFact {
    WorkspaceBundleAssetNameFact {
        resolved_name: resolved_name.to_owned(),
        has_streamed_scene_flag,
        is_streamed_scene_asset_bundle,
    }
}

fn asset_bundle_root_with_name(name: &str) -> SerializedValue {
    SerializedValue::Struct(vec![crate::serialized_file::object::SerializedFieldValue {
        name: "m_AssetBundleName_R".to_owned(),
        value: SerializedValue::StringBytes(name.as_bytes().to_vec()),
    }])
}

#[test]
fn scene_bundle_plan_composes_traversal_fetch_order_and_get_asset_bundle_name() {
    // The resolved names below come from the real `get_asset_bundle_name`
    // resolver, not invented strings: an explicit serialized name wins, an
    // empty serialized name falls back to the owning bundle name.
    let resolved_explicit = crate::processing::asset_bundle::get_asset_bundle_name(
        &asset_bundle_root_with_name("scenes"),
        "level0",
    )
    .unwrap();
    assert_eq!(resolved_explicit, "scenes");
    let resolved_fallback = crate::processing::asset_bundle::get_asset_bundle_name(
        &asset_bundle_root_with_name(""),
        "level0",
    )
    .unwrap();
    assert_eq!(resolved_fallback, "level0");

    // Scene 0 has two collections: the first collection belongs to bundle 7
    // and the second to bundle 9. Source `FirstOrDefault` must select bundle 7.
    // Scene 1 has no serialized bundle and stays unset. Scene 2 selects a
    // bundle without assets and falls back to the bundle name.
    let scene_collections = vec![vec![0usize, 1usize], vec![2usize], vec![3usize]];
    let file_bundles = vec![
        Some(bundle_source(7, "level0")),
        Some(bundle_source(9, "other")),
        None,
        Some(bundle_source(11, "empty")),
    ];
    let mut fetch_order = HashMap::new();
    // Bundle 7 holds two assets in FetchAssets() order: the first wins even
    // though the second sorts earlier. Bundle 11 has no entry at all.
    fetch_order.insert(
        7usize,
        vec![
            asset_fact(&resolved_explicit, false, false),
            asset_fact("aaa-first-sorted-but-second", false, false),
        ],
    );
    fetch_order.insert(11usize, Vec::new());
    let assignments = plan_workspace_scene_hierarchy_bundle_names(
        &scene_collections,
        &file_bundles,
        &fetch_order,
        WorkspaceSceneHierarchyBundlePlanLimits::default(),
    )
    .unwrap();
    assert_eq!(assignments.len(), 3);
    assert_eq!(assignments[0].scene_index, 0);
    assert_eq!(assignments[0].effective_asset_bundle_name, None);
    assert_eq!(
        assignments[0].name,
        WorkspaceSceneHierarchyBundleName::AssetName("scenes".to_owned())
    );
    assert_eq!(assignments[0].streamed_scene_assert_warning, None);
    assert_eq!(
        assignments[1].name,
        WorkspaceSceneHierarchyBundleName::Unset
    );
    assert_eq!(assignments[1].effective_asset_bundle_name, None);
    assert_eq!(
        assignments[2].name,
        WorkspaceSceneHierarchyBundleName::BundleName("empty".to_owned())
    );
    assert_eq!(assignments[2].effective_asset_bundle_name, None);
}

#[test]
fn scene_bundle_plan_reports_the_streamed_scene_debug_assert_without_changing_the_name() {
    let scene_collections = vec![vec![0usize]];
    let file_bundles = vec![Some(bundle_source(3, "level0"))];
    let mut fetch_order = HashMap::new();
    // The source Debug.Assert fires when the version carries
    // IsStreamedSceneAssetBundle but the value is false. Release builds
    // continue with the resolved name; Rust records an explicit warning.
    fetch_order.insert(3usize, vec![asset_fact("scenes", true, false)]);
    let assignments = plan_workspace_scene_hierarchy_bundle_names(
        &scene_collections,
        &file_bundles,
        &fetch_order,
        WorkspaceSceneHierarchyBundlePlanLimits::default(),
    )
    .unwrap();
    assert_eq!(assignments.len(), 1);
    assert_eq!(
        assignments[0].name,
        WorkspaceSceneHierarchyBundleName::AssetName("scenes".to_owned())
    );
    let warning = assignments[0]
        .streamed_scene_assert_warning
        .as_ref()
        .expect("assert warning is recorded");
    assert!(warning.contains("IsStreamedSceneAssetBundle=false"));
    assert!(warning.contains("release continues"));

    // A streamed true value and a version without the field stay silent.
    let mut silent = HashMap::new();
    silent.insert(3usize, vec![asset_fact("scenes", true, true)]);
    let assignments = plan_workspace_scene_hierarchy_bundle_names(
        &scene_collections,
        &file_bundles,
        &silent,
        WorkspaceSceneHierarchyBundlePlanLimits::default(),
    )
    .unwrap();
    assert_eq!(assignments[0].streamed_scene_assert_warning, None);
}

#[test]
fn scene_bundle_plan_rejects_invalid_indexes_and_enforces_limits() {
    // Unknown collection file indexes fail instead of selecting a bundle.
    let error = plan_workspace_scene_hierarchy_bundle_names(
        &[vec![5usize]],
        &[Some(bundle_source(1, "level0"))],
        &HashMap::new(),
        WorkspaceSceneHierarchyBundlePlanLimits::default(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("outside 1 files"));

    // Limits fail closed with explicit resources.
    let error = plan_workspace_scene_hierarchy_bundle_names(
        &[vec![0usize], vec![0usize]],
        &[Some(bundle_source(1, "level0"))],
        &HashMap::new(),
        WorkspaceSceneHierarchyBundlePlanLimits {
            max_scenes: 1,
            ..WorkspaceSceneHierarchyBundlePlanLimits::default()
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("scene hierarchy bundle scenes"));
}
