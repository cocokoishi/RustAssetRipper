use super::*;
use crate::io::binary::Endian;
use crate::processing::prefab_processing::processed_assets::{
    add_generated_constructor_pointer, add_processed_asset, add_processed_collection,
    add_processed_source_file_dependency,
};
use crate::processing::scene_hierarchy::{
    default_workspace_generated_transform_values, WorkspaceSceneObjectIdentity,
};
use crate::serialized_file::metadata::{
    SerializedFileMetadata, SerializedType, TypeTree, TypeTreeNode,
};
use crate::serialized_file::metadata_prefix::SerializedFileMetadataPrefix;

const TRANSFORM_CLASS_ID: i32 = 4;
pub(in crate::processing::prefab_processing) fn empty_plan() -> WorkspaceProcessedPrefabAssetPlan {
    WorkspaceProcessedPrefabAssetPlan {
        complete: true,
        incomplete_reason: None,
        collections: Vec::new(),
        component_additions: Vec::new(),
        component_list_overlays: Vec::new(),
        component_overlay_slot_count: 0,
        constructor_pointers: Vec::new(),
        dependency_entry_count: 0,
        source_dependency_appends: Vec::new(),
        modeled_animator_controller_dependency_append_count: 0,
        deferred_source_dependency_file_count: 0,
        asset_count: 0,
        total_collection_name_bytes: 0,
    }
}

pub(in crate::processing::prefab_processing) fn tree_node(
    level: u8,
    name: &str,
    type_name: &str,
    byte_size: i32,
) -> TypeTreeNode {
    TypeTreeNode {
        level,
        version: 1,
        type_flags: 0,
        type_string_offset: None,
        name_string_offset: None,
        type_name: Some(type_name.to_owned()),
        name: Some(name.to_owned()),
        byte_size,
        index: 0,
        meta_flags: 0,
        reference_type_hash: None,
    }
}

pub(in crate::processing::prefab_processing) fn transform_metadata(
    include_euler_hint: bool,
    include_root_order: bool,
) -> SerializedFileMetadata {
    let mut nodes = vec![
        tree_node(0, "", "Transform", -1),
        tree_node(1, "Base", "Component", -1),
        tree_node(2, "m_GameObject", "PPtr<GameObject>", 12),
        tree_node(3, "m_FileID", "SInt32", 4),
        tree_node(3, "m_PathID", "SInt64", 8),
        tree_node(1, "m_Father", "PPtr<Transform>", 12),
        tree_node(2, "m_FileID", "SInt32", 4),
        tree_node(2, "m_PathID", "SInt64", 8),
        tree_node(1, "m_Children", "vector", -1),
        tree_node(2, "Array", "Array", -1),
        tree_node(3, "size", "int", 4),
        tree_node(3, "data", "PPtr<Transform>", 12),
        tree_node(4, "m_FileID", "SInt32", 4),
        tree_node(4, "m_PathID", "SInt64", 8),
    ];
    if include_root_order {
        nodes.push(tree_node(1, "m_RootOrder", "int", 4));
    }
    if include_euler_hint {
        nodes.extend([
            tree_node(1, "m_LocalEulerAnglesHint", "Vector3f", -1),
            tree_node(2, "x", "float", 4),
            tree_node(2, "y", "float", 4),
            tree_node(2, "z", "float", 4),
        ]);
    }
    nodes.extend([
        tree_node(1, "m_LocalPosition", "Vector3f", -1),
        tree_node(2, "x", "float", 4),
        tree_node(2, "y", "float", 4),
        tree_node(2, "z", "float", 4),
        tree_node(1, "m_LocalRotation", "Quaternionf", -1),
        tree_node(2, "x", "float", 4),
        tree_node(2, "y", "float", 4),
        tree_node(2, "z", "float", 4),
        tree_node(2, "w", "float", 4),
        tree_node(1, "m_LocalScale", "Vector3f", -1),
        tree_node(2, "x", "float", 4),
        tree_node(2, "y", "float", 4),
        tree_node(2, "z", "float", 4),
    ]);
    SerializedFileMetadata {
        prefix: SerializedFileMetadataPrefix {
            metadata_offset: 0,
            metadata_size: 0,
            body_endian: Endian::Little,
            unity_version_text: Some("2019.4.31f1".to_owned()),
            target_platform: Some(0),
            has_type_tree: true,
            consumed_bytes: 0,
        },
        types: vec![SerializedType {
            raw_type_id: TRANSFORM_CLASS_ID,
            type_id_for_hash: TRANSFORM_CLASS_ID,
            is_stripped_type: false,
            script_type_index: -1,
            script_id: None,
            old_type_hash: None,
            type_tree: Some(TypeTree {
                nodes,
                string_buffer: Vec::new(),
            }),
            type_dependencies: Vec::new(),
        }],
        long_file_id: None,
        objects: Vec::new(),
        script_types: Vec::new(),
        externals: Vec::new(),
        reference_types: Vec::new(),
        user_information: None,
        trailing_metadata: Vec::new(),
        consumed_bytes: 0,
    }
}

pub(in crate::processing::prefab_processing) fn generated_transform_plan() -> (
    WorkspaceProcessedPrefabAssetPlan,
    WorkspaceProcessedAssetIdentity,
    WorkspaceSceneObjectIdentity,
) {
    let mut plan = empty_plan();
    let limits = WorkspaceProcessedPrefabAssetPlanLimits::default();
    let collection_index = add_processed_collection(
        &mut plan,
        WorkspaceProcessedCollectionKind::MissingPrefabTransforms,
        "Missing Prefab Transforms",
        limits,
    )
    .unwrap();
    let game_object = WorkspaceSceneObjectIdentity {
        file_index: 3,
        object_index: 7,
        path_id: 99,
        class_id: Some(1),
    };
    let asset = add_processed_asset(
        &mut plan,
        collection_index,
        TRANSFORM_CLASS_ID,
        WorkspaceProcessedPrefabAssetKind::GeneratedTransform {
            game_object,
            default_values: default_workspace_generated_transform_values(),
        },
        limits,
    )
    .unwrap();
    let file_id = add_processed_source_file_dependency(
        &mut plan,
        collection_index,
        game_object.file_index,
        limits,
    )
    .unwrap();
    add_generated_constructor_pointer(
        &mut plan,
        WorkspaceGeneratedConstructorPointer {
            source_asset: asset,
            field: WorkspaceGeneratedPointerField::TransformGameObject,
            file_id,
            path_id: game_object.path_id,
            target: game_object,
        },
        limits,
    )
    .unwrap();
    (plan, asset, game_object)
}
