use super::asset_classification::{
    classify_type_tree_component_base, is_game_object_pointer_path,
    is_lighting_data_asset_field_path, normalize_component_candidates, scene_asset_category,
    TypeTreeComponentBaseClassification,
};
use super::relationship_indexes::build_prefab_root_relationship_index;
use super::*;
use crate::processing::prefab_processing::{
    plan_workspace_prefab_processing, plan_workspace_processed_prefab_assets,
    WorkspacePrefabProcessingEvent, WorkspaceProcessedCollectionKind,
    WorkspaceProcessedPrefabAssetKind, WorkspaceProcessedPrefabAssetPlanLimits,
    WorkspaceProcessedPrefabReference, WorkspaceSourceDependencyAppendTarget,
    WorkspaceSourceDependencyPointerField,
};
use crate::processing::scene_collections::{
    SceneDefinitionGuidPlan, SceneDefinitionPathSource, SceneDefinitionPlan,
    SceneGuidGenerationReason, SceneManagerKind,
};
use crate::serialized_file::file::SerializedFile;
use crate::serialized_file::format_version::FormatVersion;
use crate::serialized_file::metadata::TypeTree;
use crate::serialized_file::metadata::TypeTreeNode;
use crate::serialized_file::reference_graph::{
    MultiFileReferenceGraph, NamedObjectReferenceGraph, ObjectReferenceGraph,
};

fn push_generated_scene_type_tree_string(buffer: &mut Vec<u8>, value: &str) -> u32 {
    let offset = u32::try_from(buffer.len()).unwrap();
    buffer.extend_from_slice(value.as_bytes());
    buffer.push(0);
    offset
}

fn put_generated_scene_type_tree_node(
    output: &mut Vec<u8>,
    level: u8,
    type_offset: u32,
    name_offset: u32,
    byte_size: i32,
    index: i32,
) {
    output.extend_from_slice(&1_u16.to_le_bytes());
    output.push(level);
    output.push(0);
    output.extend_from_slice(&type_offset.to_le_bytes());
    output.extend_from_slice(&name_offset.to_le_bytes());
    output.extend_from_slice(&byte_size.to_le_bytes());
    output.extend_from_slice(&index.to_le_bytes());
    output.extend_from_slice(&0_u32.to_le_bytes());
}

fn generated_mesh_renderer_serialized_file() -> Vec<u8> {
    const CLASS_ID: i32 = 23;

    let mut strings = Vec::new();
    let mesh_renderer_type = push_generated_scene_type_tree_string(&mut strings, "MeshRenderer");
    let empty_name = push_generated_scene_type_tree_string(&mut strings, "");
    let renderer_type = push_generated_scene_type_tree_string(&mut strings, "Renderer");
    let component_type = push_generated_scene_type_tree_string(&mut strings, "Component");
    let base_name = push_generated_scene_type_tree_string(&mut strings, "Base");
    let game_object_pointer_type =
        push_generated_scene_type_tree_string(&mut strings, "PPtr<GameObject>");
    let game_object_field = push_generated_scene_type_tree_string(&mut strings, "m_GameObject");
    let int_type = push_generated_scene_type_tree_string(&mut strings, "int");
    let file_id_field = push_generated_scene_type_tree_string(&mut strings, "m_FileID");
    let path_id_field = push_generated_scene_type_tree_string(&mut strings, "m_PathID");

    let mut metadata = b"2019.4.0f1\0".to_vec();
    metadata.extend_from_slice(&0_u32.to_le_bytes());
    metadata.extend_from_slice(&1_i32.to_le_bytes());
    metadata.extend_from_slice(&CLASS_ID.to_le_bytes());
    metadata.extend_from_slice(&6_i32.to_le_bytes());
    metadata.extend_from_slice(&i32::try_from(strings.len()).unwrap().to_le_bytes());
    put_generated_scene_type_tree_node(&mut metadata, 0, mesh_renderer_type, empty_name, -1, 0);
    put_generated_scene_type_tree_node(&mut metadata, 1, renderer_type, base_name, -1, 1);
    put_generated_scene_type_tree_node(&mut metadata, 2, component_type, base_name, -1, 2);
    put_generated_scene_type_tree_node(
        &mut metadata,
        3,
        game_object_pointer_type,
        game_object_field,
        -1,
        3,
    );
    put_generated_scene_type_tree_node(&mut metadata, 4, int_type, file_id_field, 4, 4);
    put_generated_scene_type_tree_node(&mut metadata, 4, int_type, path_id_field, 4, 5);
    metadata.extend_from_slice(&strings);

    metadata.extend_from_slice(&0_u32.to_le_bytes());
    metadata.extend_from_slice(&1_i32.to_le_bytes());
    metadata.extend_from_slice(&9001_i32.to_le_bytes());
    metadata.extend_from_slice(&0_u32.to_le_bytes());
    metadata.extend_from_slice(&8_i32.to_le_bytes());
    metadata.extend_from_slice(&CLASS_ID.to_le_bytes());
    metadata.extend_from_slice(&(CLASS_ID as i16).to_le_bytes());
    metadata.extend_from_slice(&0_u16.to_le_bytes());
    metadata.extend_from_slice(&0_i32.to_le_bytes());
    metadata.push(0);

    let header_size = 20_usize;
    let data_offset = header_size + metadata.len();
    let file_size = data_offset + 8;
    let mut file = Vec::with_capacity(file_size);
    file.extend_from_slice(&i32::try_from(metadata.len()).unwrap().to_be_bytes());
    file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
    file.extend_from_slice(&FormatVersion::Unknown10.raw().to_be_bytes());
    file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
    file.extend_from_slice(&[0, 0, 0, 0]);
    file.extend_from_slice(&metadata);
    file.extend_from_slice(&[0; 8]);
    file
}

fn identity(
    file_index: usize,
    object_index: usize,
    path_id: i64,
    class_id: i32,
) -> WorkspaceSceneObjectIdentity {
    WorkspaceSceneObjectIdentity {
        file_index,
        object_index,
        path_id,
        class_id: Some(class_id),
    }
}

fn relationship(
    source: WorkspaceSceneObjectIdentity,
    kind: SceneRelationshipKind,
    field_path: &str,
    path_id: i64,
    target: SceneRelationshipTarget,
) -> SceneRelationship {
    SceneRelationship {
        source_object_index: source.object_index,
        source_path_id: source.path_id,
        kind,
        field_path: field_path.to_owned(),
        file_id: 0,
        path_id,
        target,
    }
}

fn empty_collection_graph(file_index: usize) -> WorkspaceSceneCollectionHierarchy {
    WorkspaceSceneCollectionHierarchy {
        collection_file_index: file_index,
        scene_objects: Vec::new(),
        relationships: Vec::new(),
        game_object_hierarchies: Vec::new(),
        game_object_component_relationship_indices: Vec::new(),
        prefab_root_relationship_index: Vec::new(),
        transform_hierarchies: Vec::new(),
        transform_parent_relationship_indices: Vec::new(),
        transform_child_relationship_indices: Vec::new(),
        transform_game_object_relationship_indices: Vec::new(),
        graph_issues: Vec::new(),
        scene_assets: Vec::new(),
        lighting_data_asset_field_object_indices: Vec::new(),
        component_candidates: Vec::new(),
        missing_transform_plans: Vec::new(),
        source_graph_complete: true,
        source_reference_scans_complete: true,
        roots: WorkspaceSceneRootDiscovery::default(),
        hierarchy_audit: WorkspaceSceneHierarchyAudit::default(),
    }
}

fn scene_definition_plan(file_index: usize) -> SceneDefinitionPlan {
    SceneDefinitionPlan {
        collection_file_index: file_index,
        collection_name: "scene".to_owned(),
        name: "scene".to_owned(),
        path: "Assets/Scenes/scene".to_owned(),
        path_source: SceneDefinitionPathSource::CollectionNameFallback,
        guid: SceneDefinitionGuidPlan::GenerateAtExport(
            SceneGuidGenerationReason::MissingFromInput,
        ),
    }
}

fn workspace_hierarchy_fixture(cycle: bool) -> WorkspaceSceneHierarchies {
    let root = identity(0, 0, 1, GAME_OBJECT_CLASS_ID);
    let first_transform = identity(0, 1, 2, TRANSFORM_CLASS_ID);
    let last_transform = identity(0, 2, 3, RECT_TRANSFORM_CLASS_ID);
    let renderer = identity(0, 3, 4, 23);
    let child_transform = identity(1, 0, 10, TRANSFORM_CLASS_ID);
    let child_game_object = identity(1, 1, 11, GAME_OBJECT_CLASS_ID);

    let root_relationships = vec![
        relationship(
            root,
            SceneRelationshipKind::GameObjectComponent,
            "m_Component[0]",
            renderer.path_id,
            SceneRelationshipTarget::LocalObject { object_index: 3 },
        ),
        relationship(
            root,
            SceneRelationshipKind::GameObjectComponent,
            "m_Component[1]",
            0,
            SceneRelationshipTarget::Null,
        ),
        relationship(
            root,
            SceneRelationshipKind::GameObjectComponent,
            "m_Component[2]",
            first_transform.path_id,
            SceneRelationshipTarget::LocalObject { object_index: 1 },
        ),
        relationship(
            root,
            SceneRelationshipKind::GameObjectComponent,
            "m_Component[3]",
            last_transform.path_id,
            SceneRelationshipTarget::LocalObject { object_index: 2 },
        ),
        relationship(
            last_transform,
            SceneRelationshipKind::TransformChild,
            "m_Children[0]",
            child_transform.path_id,
            SceneRelationshipTarget::WorkspaceObject {
                file_index: 1,
                object_index: 0,
            },
        ),
        relationship(
            last_transform,
            SceneRelationshipKind::ComponentGameObject,
            "m_GameObject",
            root.path_id,
            SceneRelationshipTarget::LocalObject { object_index: 0 },
        ),
        relationship(
            first_transform,
            SceneRelationshipKind::ComponentGameObject,
            "m_GameObject",
            root.path_id,
            SceneRelationshipTarget::LocalObject { object_index: 0 },
        ),
        relationship(
            first_transform,
            SceneRelationshipKind::TransformParent,
            "m_Father",
            0,
            SceneRelationshipTarget::Null,
        ),
        relationship(
            last_transform,
            SceneRelationshipKind::TransformParent,
            "m_Father",
            0,
            SceneRelationshipTarget::Null,
        ),
    ];
    let root_collection = WorkspaceSceneCollectionHierarchy {
        collection_file_index: 0,
        scene_objects: Vec::new(),
        relationships: root_relationships,
        game_object_hierarchies: vec![WorkspaceSceneGameObjectHierarchy {
            game_object: root,
            component_relationship_start: 0,
            component_relationship_end: 4,
        }],
        game_object_component_relationship_indices: vec![0, 1, 2, 3],
        prefab_root_relationship_index: Vec::new(),
        transform_hierarchies: vec![
            WorkspaceSceneTransformHierarchy {
                transform: first_transform,
                parent_relationship_start: 0,
                parent_relationship_end: 1,
                child_relationship_start: 0,
                child_relationship_end: 0,
                game_object_relationship_start: 0,
                game_object_relationship_end: 1,
            },
            WorkspaceSceneTransformHierarchy {
                transform: last_transform,
                parent_relationship_start: 1,
                parent_relationship_end: 2,
                child_relationship_start: 0,
                child_relationship_end: 1,
                game_object_relationship_start: 1,
                game_object_relationship_end: 1,
            },
        ],
        transform_parent_relationship_indices: vec![7, 8],
        transform_child_relationship_indices: vec![4],
        transform_game_object_relationship_indices: vec![6, 5],
        graph_issues: Vec::new(),
        lighting_data_asset_field_object_indices: Vec::new(),
        scene_assets: vec![WorkspaceSceneAssetIdentity {
            identity: root,
            category: WorkspaceSceneAssetCategory::GameObject,
        }],
        component_candidates: vec![
            WorkspaceSceneComponentCandidate {
                identity: first_transform,
                evidence: WorkspaceSceneComponentEvidence::TransformClassId,
            },
            WorkspaceSceneComponentCandidate {
                identity: last_transform,
                evidence: WorkspaceSceneComponentEvidence::TransformClassId,
            },
            WorkspaceSceneComponentCandidate {
                identity: renderer,
                evidence: WorkspaceSceneComponentEvidence::GameObjectComponentReference,
            },
        ],
        missing_transform_plans: Vec::new(),
        source_graph_complete: true,
        source_reference_scans_complete: true,
        roots: WorkspaceSceneRootDiscovery::default(),
        hierarchy_audit: WorkspaceSceneHierarchyAudit::default(),
    };

    let mut child_relationships = vec![
        relationship(
            child_transform,
            SceneRelationshipKind::ComponentGameObject,
            "m_GameObject",
            child_game_object.path_id,
            SceneRelationshipTarget::LocalObject { object_index: 1 },
        ),
        relationship(
            child_game_object,
            SceneRelationshipKind::GameObjectComponent,
            "m_Component[0]",
            child_transform.path_id,
            SceneRelationshipTarget::LocalObject { object_index: 0 },
        ),
        relationship(
            child_transform,
            SceneRelationshipKind::TransformParent,
            "m_Father",
            first_transform.path_id,
            SceneRelationshipTarget::WorkspaceObject {
                file_index: 0,
                object_index: 1,
            },
        ),
    ];
    let child_transform_child_range = if cycle {
        child_relationships.push(relationship(
            child_transform,
            SceneRelationshipKind::TransformChild,
            "m_Children[0]",
            child_transform.path_id,
            SceneRelationshipTarget::LocalObject { object_index: 0 },
        ));
        0..1
    } else {
        0..0
    };
    let child_transform_child_indexes = if cycle { vec![3] } else { Vec::new() };
    let child_collection = WorkspaceSceneCollectionHierarchy {
        collection_file_index: 1,
        scene_objects: Vec::new(),
        relationships: child_relationships,
        game_object_hierarchies: vec![WorkspaceSceneGameObjectHierarchy {
            game_object: child_game_object,
            component_relationship_start: 0,
            component_relationship_end: 1,
        }],
        game_object_component_relationship_indices: vec![1],
        prefab_root_relationship_index: Vec::new(),
        transform_hierarchies: vec![WorkspaceSceneTransformHierarchy {
            transform: child_transform,
            parent_relationship_start: 0,
            parent_relationship_end: 1,
            child_relationship_start: child_transform_child_range.start,
            child_relationship_end: child_transform_child_range.end,
            game_object_relationship_start: 0,
            game_object_relationship_end: 1,
        }],
        transform_parent_relationship_indices: vec![2],
        transform_child_relationship_indices: child_transform_child_indexes,
        transform_game_object_relationship_indices: vec![0],
        graph_issues: Vec::new(),
        lighting_data_asset_field_object_indices: Vec::new(),
        scene_assets: vec![WorkspaceSceneAssetIdentity {
            identity: child_game_object,
            category: WorkspaceSceneAssetCategory::GameObject,
        }],
        component_candidates: vec![WorkspaceSceneComponentCandidate {
            identity: child_transform,
            evidence: WorkspaceSceneComponentEvidence::TransformClassId,
        }],
        missing_transform_plans: Vec::new(),
        source_graph_complete: true,
        source_reference_scans_complete: true,
        roots: WorkspaceSceneRootDiscovery::default(),
        hierarchy_audit: WorkspaceSceneHierarchyAudit::default(),
    };
    WorkspaceSceneHierarchies {
        scenes: Vec::new(),
        non_scene_collections: vec![root_collection, child_collection],
        collection_graph_index: vec![
            WorkspaceSceneCollectionGraphIndexEntry {
                file_index: 0,
                scene_index: None,
                collection_index: 0,
            },
            WorkspaceSceneCollectionGraphIndexEntry {
                file_index: 1,
                scene_index: None,
                collection_index: 1,
            },
        ],
    }
}

fn prefab_root_workspace(
    target: Option<(i64, SceneRelationshipTarget)>,
) -> (WorkspaceSceneHierarchies, WorkspaceSceneObjectIdentity) {
    let prefab = identity(0, 0, 100, PREFAB_INSTANCE_CLASS_ID);
    let root = identity(0, 1, 200, GAME_OBJECT_CLASS_ID);
    let relationships = target
        .map(|(path_id, target)| {
            vec![relationship(
                prefab,
                SceneRelationshipKind::PrefabInstanceRootGameObject,
                "m_RootGameObject",
                path_id,
                target,
            )]
        })
        .unwrap_or_default();
    let prefab_root_relationship_index = if relationships.is_empty() {
        Vec::new()
    } else {
        vec![WorkspacePrefabRootRelationshipIndexEntry {
            source_object_index: prefab.object_index,
            relationship_index: 0,
        }]
    };
    let collection = WorkspaceSceneCollectionHierarchy {
        collection_file_index: 0,
        scene_objects: Vec::new(),
        relationships,
        game_object_hierarchies: Vec::new(),
        game_object_component_relationship_indices: Vec::new(),
        prefab_root_relationship_index,
        transform_hierarchies: Vec::new(),
        transform_parent_relationship_indices: Vec::new(),
        transform_child_relationship_indices: Vec::new(),
        transform_game_object_relationship_indices: Vec::new(),
        graph_issues: Vec::new(),
        lighting_data_asset_field_object_indices: Vec::new(),
        scene_assets: vec![
            WorkspaceSceneAssetIdentity {
                identity: prefab,
                category: WorkspaceSceneAssetCategory::PrefabInstance,
            },
            WorkspaceSceneAssetIdentity {
                identity: root,
                category: WorkspaceSceneAssetCategory::GameObject,
            },
        ],
        component_candidates: Vec::new(),
        missing_transform_plans: Vec::new(),
        source_graph_complete: true,
        source_reference_scans_complete: true,
        roots: WorkspaceSceneRootDiscovery::default(),
        hierarchy_audit: WorkspaceSceneHierarchyAudit::default(),
    };
    (
        WorkspaceSceneHierarchies {
            scenes: Vec::new(),
            non_scene_collections: vec![collection],
            collection_graph_index: vec![WorkspaceSceneCollectionGraphIndexEntry {
                file_index: 0,
                scene_index: None,
                collection_index: 0,
            }],
        },
        prefab,
    )
}
mod asset_classification;
mod hierarchy_walk;
mod membership;
mod prefab_processing;
mod prefab_roots;
