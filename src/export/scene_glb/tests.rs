//! Tests for the scene-level GLB export entry.
//!
//! The fixtures build small [`MaterializedSceneCollection`]s directly, so the export path is
//! exercised over the port's real types rather than over a stand-in.

use super::*;
use crate::export::scene_wiring::{MaterializedGameObject, MaterializedTransform};
use crate::processing::scene::{
    SceneQuaternion, SceneRelationship, SceneRelationshipKind, SceneTransformValues, SceneVector3,
};
use crate::processing::scene_hierarchy::WorkspaceSceneObjectIdentity;
use crate::resources::glb_level::GlbLevelLocalTransform;

fn identity(object_index: usize) -> WorkspaceSceneObjectIdentity {
    WorkspaceSceneObjectIdentity {
        file_index: 0,
        object_index,
        path_id: 100 + object_index as i64,
        class_id: None,
    }
}

/// A relationship that targets a local object.
fn to_local(object_index: usize) -> SceneRelationship {
    SceneRelationship {
        source_object_index: 0,
        source_path_id: 0,
        kind: SceneRelationshipKind::TransformParent,
        field_path: "m_Father".to_owned(),
        file_id: 0,
        path_id: 100 + object_index as i64,
        target: SceneRelationshipTarget::LocalObject { object_index },
    }
}

/// A Transform with the given GameObject object index and an optional father.
fn transform(
    object_index: usize,
    game_object: usize,
    father: Option<usize>,
    values: Option<SceneTransformValues>,
) -> MaterializedTransform {
    MaterializedTransform {
        identity: identity(object_index),
        game_object: Some(to_local(game_object)),
        father: father.map(to_local),
        children: Vec::new(),
        values,
        rect_values: None,
        generated: false,
    }
}

fn game_object(object_index: usize, name: &str) -> MaterializedGameObject {
    MaterializedGameObject {
        identity: identity(object_index),
        name: Some(name.to_owned()),
        name_bytes: None,
        is_active: Some(true),
        components: Vec::new(),
        generated_transform: None,
    }
}

fn local(
    position: (f32, f32, f32),
    rotation: (f32, f32, f32, f32),
    scale: (f32, f32, f32),
) -> SceneTransformValues {
    SceneTransformValues {
        local_position: Some(SceneVector3 {
            x: position.0,
            y: position.1,
            z: position.2,
        }),
        local_rotation: Some(SceneQuaternion {
            x: rotation.0,
            y: rotation.1,
            z: rotation.2,
            w: rotation.3,
        }),
        local_scale: Some(SceneVector3 {
            x: scale.0,
            y: scale.1,
            z: scale.2,
        }),
        root_order: Some(0),
        local_euler_angles_hint: None,
        rect_transform: None,
    }
}

/// Root(10/11) -> Child(20/21) -> Grandchild(30/31), plus parentless Second(40/41).
///
/// The pair is `(GameObject object index, Transform object index)`. A Transform's `m_Father`
/// names the parent **Transform**, not its GameObject, so the father arguments below are the
/// parent Transform indexes.
fn hierarchy() -> MaterializedSceneCollection {
    MaterializedSceneCollection {
        file_index: 0,
        file_name: "scene.unity".to_owned(),
        objects: Vec::new(),
        game_objects: vec![
            game_object(10, "Root"),
            game_object(20, "Child"),
            game_object(30, "Grandchild"),
            game_object(40, "Second"),
        ],
        transforms: vec![
            // Root: father of nothing.
            transform(
                11,
                10,
                None,
                Some(local(
                    (1.0, 2.0, 3.0),
                    (0.0, 0.707_106_77, 0.0, 0.707_106_77),
                    (2.0, 2.0, 2.0),
                )),
            ),
            // Child: father is Root's Transform (11).
            transform(
                21,
                20,
                Some(11),
                Some(local(
                    (4.0, 5.0, 6.0),
                    (0.5, 0.0, 0.0, 0.866_025_4),
                    (1.0, 1.0, 1.0),
                )),
            ),
            // Grandchild: father is Child's Transform (21).
            transform(
                31,
                30,
                Some(21),
                Some(local(
                    (7.0, 8.0, 9.0),
                    (0.0, 0.0, 0.707_106_77, 0.707_106_77),
                    (1.0, 1.0, 1.0),
                )),
            ),
            // Second: a second root.
            transform(
                41,
                40,
                None,
                Some(local(
                    (-1.0, -2.0, -3.0),
                    (0.0, 0.0, 0.0, 1.0),
                    (1.0, 1.0, 1.0),
                )),
            ),
        ],
        issues: Vec::new(),
    }
}

/// The JSON chunk of a GLB, which is the part a test compares as text.
fn json_chunk(bytes: &[u8]) -> String {
    let length = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
    String::from_utf8(bytes[20..20 + length].to_vec()).expect("the JSON chunk is UTF-8")
}

#[test]
fn the_view_resolves_names_edges_and_local_transforms() {
    let collection = hierarchy();
    let view = MaterializedSceneGraphView::new(&collection, &[]);

    assert_eq!(view.len(), 4);
    // Transform order matches the collection, and each names its GameObject.
    assert_eq!(view.name(0), Some("Root"));
    assert_eq!(view.name(1), Some("Child"));
    assert_eq!(view.name(2), Some("Grandchild"));
    assert_eq!(view.name(3), Some("Second"));

    // Father edges resolve through the GameObject relationship, not the Transform object index.
    assert_eq!(view.father(0), None);
    assert_eq!(view.father(1), Some(0));
    assert_eq!(view.father(2), Some(1));
    assert_eq!(view.father(3), None);
    assert_eq!(view.children(0), [1]);
    assert_eq!(view.children(1), [2]);
    assert!(view.children(2).is_empty());
    assert!(view.children(3).is_empty());

    // Every Transform here has a GameObject.
    assert!((0..4).all(|index| view.has_game_object(index)));

    // Position converts to glTF space and the rotation takes the axis flip; scale is unchanged.
    let root = view.local(0).expect("the root has values");
    assert_eq!(root.translation, [-1.0, 2.0, 3.0]);
    assert_eq!(root.rotation, [0.0, -0.707_106_77, -0.0, 0.707_106_77]);
    assert_eq!(root.scale, [2.0, 2.0, 2.0]);
}

#[test]
fn a_transform_without_values_reads_back_as_the_identity() {
    let mut collection = hierarchy();
    collection.transforms[0].values = None;
    let view = MaterializedSceneGraphView::new(&collection, &[]);
    // A Transform with no resolved values still reads back, as the source's identity
    // `Transformation` does, so the node is emitted with an identity local transform.
    let local = view.local(0).expect("a missing component still reads back");
    assert_eq!(local.translation, [0.0, 0.0, 0.0]);
    assert_eq!(local.rotation, [0.0, 0.0, 0.0, 1.0]);
    assert_eq!(local.scale, [1.0, 1.0, 1.0]);
}

#[test]
fn a_transform_without_a_game_object_reports_no_name_and_no_game_object() {
    let mut collection = hierarchy();
    collection.transforms[1].game_object = None;
    let view = MaterializedSceneGraphView::new(&collection, &[]);
    assert_eq!(view.name(1), None);
    assert!(!view.has_game_object(1));

    // `GetRoot` climbs while the parent HAS a GameObject, so its climb stops at a parent whose
    // GameObject is null. With Child's GameObject gone, Grandchild's climb ends at itself, so
    // Grandchild is emitted as a root even though its father pointer still names Child. Child
    // itself emits nothing, because `AddGameObjectToScene` returns before its child loop.
    let plan = build_scene_plan_with_limits(&view, true, GlbLevelLimits::default())
        .expect("the walk completes");
    let names: Vec<&str> = plan.nodes.iter().map(|node| node.name.as_str()).collect();
    assert_eq!(names, ["Root", "Grandchild", "Second"]);
    // Root emitted no child, and both survivors are roots.
    assert_eq!(plan.nodes[0].parent, None);
    assert_eq!(plan.nodes[1].parent, None);
    assert_eq!(plan.nodes[2].parent, None);
}

#[test]
fn a_self_edge_is_not_a_hierarchy_edge() {
    let mut collection = hierarchy();
    // A Transform that names itself as its father must not become its own parent.
    collection.transforms[0].father = Some(to_local(10));
    let view = MaterializedSceneGraphView::new(&collection, &[]);
    assert_eq!(view.father(0), None, "a self-edge is dropped");
    let plan = build_scene_plan_with_limits(&view, true, GlbLevelLimits::default())
        .expect("the walk completes");
    assert_eq!(plan.nodes.len(), 4, "every Transform is still visited once");
}

#[test]
fn the_export_emits_a_glb_whose_document_matches_the_hierarchy() {
    let collection = hierarchy();
    let meshes: Vec<Option<crate::resources::mesh_data::MeshData>> = vec![None; 4];
    let export = export_scene_glb(SceneGlbExportInput {
        collection: &collection,
        attached_meshes: &[],
        is_scene: true,
        meshes: &meshes,
        level_limits: GlbLevelLimits::default(),
        glb_limits: GlbBuildLimits::default(),
    })
    .expect("the export succeeds");

    assert_eq!(export.node_count, 4);
    assert_eq!(export.mesh_count, 0, "this fixture has no renderers");
    // No mesh selections, so no geometry warnings either.
    assert!(export.warnings.is_empty(), "{:?}", export.warnings);

    assert_eq!(&export.bytes[0..4], b"glTF");
    assert_eq!(
        u32::from_le_bytes([
            export.bytes[4],
            export.bytes[5],
            export.bytes[6],
            export.bytes[7]
        ]),
        2
    );
    let declared = u32::from_le_bytes([
        export.bytes[8],
        export.bytes[9],
        export.bytes[10],
        export.bytes[11],
    ]) as usize;
    assert_eq!(declared, export.bytes.len());

    let json = json_chunk(&export.bytes);
    // Both parentless Transforms are scene roots, in source order.
    assert!(json.contains("\"scenes\":[{\"nodes\":[0,3]}]"), "{json}");
    // Root lists Child, and the names are in source order.
    assert!(json.contains("\"children\":[1]"), "{json}");
    let root = json.find("\"name\":\"Root\"").expect("Root is emitted");
    let second = json.find("\"name\":\"Second\"").expect("Second is emitted");
    assert!(root < second, "{json}");
    // The scene build converted the root.
    assert!(json.contains("\"translation\":[-1.0,2.0,3.0]"), "{json}");
}

#[test]
fn an_asset_export_leaves_roots_untransformed() {
    let collection = hierarchy();
    let meshes: Vec<Option<crate::resources::mesh_data::MeshData>> = vec![None; 4];
    let export = export_scene_glb(SceneGlbExportInput {
        collection: &collection,
        attached_meshes: &[],
        is_scene: false,
        meshes: &meshes,
        level_limits: GlbLevelLimits::default(),
        glb_limits: GlbBuildLimits::default(),
    })
    .expect("the export succeeds");
    let json = json_chunk(&export.bytes);
    // Root is the first node and carries no transform; Child still does.
    assert!(
        !json.contains("\"name\":\"Root\",\"translation\""),
        "{json}"
    );
    assert!(json.contains("\"translation\":[-4.0,5.0,6.0]"), "{json}");
}

#[test]
fn an_empty_hierarchy_reports_no_nodes_rather_than_writing_an_empty_file() {
    let collection = MaterializedSceneCollection {
        file_index: 0,
        file_name: "empty.unity".to_owned(),
        objects: Vec::new(),
        game_objects: Vec::new(),
        transforms: Vec::new(),
        issues: Vec::new(),
    };
    let error = export_scene_glb(SceneGlbExportInput {
        collection: &collection,
        attached_meshes: &[],
        is_scene: true,
        meshes: &[],
        level_limits: GlbLevelLimits::default(),
        glb_limits: GlbBuildLimits::default(),
    })
    .expect_err("an empty hierarchy is reported");
    assert_eq!(error, SceneGlbExportError::NoNodes);
    assert!(error.to_string().contains("no nodes"));
}

/// A collection with geometry the view cannot resolve is refused, not written as a scene.
///
/// `MaterializedSceneGraphView::mesh` answers `None` for every Transform because the MeshFilter's
/// mesh reference is not materialized yet, so a real scene's plan has no mesh nodes. Without this
/// check the export would succeed with a node graph and no geometry - a file that looks like a scene
/// export and is not one. The `MeshFilter` count is what distinguishes that from a scene that simply
/// has nothing to draw.
#[test]
fn a_collection_with_mesh_filters_and_no_resolved_mesh_is_refused() {
    use crate::export::scene_wiring::MaterializedComponentSlot;
    use crate::export::scene_wiring::MaterializedComponentTarget;
    use crate::processing::scene::SceneRelationshipTarget;

    // The existing helpers give a GameObject and a Transform; only the MeshFilter slot is added.
    let mut collection = hierarchy();
    let mut with_filter = game_object(50, "WithGeometry");
    with_filter.components.push(MaterializedComponentSlot {
        source_array_index: 0,
        class_id: Some(33),
        target: MaterializedComponentTarget::Source {
            relationship_index: 0,
            file_id: 0,
            path_id: 51,
            target: SceneRelationshipTarget::LocalObject { object_index: 51 },
        },
    });
    collection.game_objects.push(with_filter);

    let error = export_scene_glb(SceneGlbExportInput {
        collection: &collection,
        attached_meshes: &[],
        is_scene: true,
        meshes: &[],
        level_limits: GlbLevelLimits::default(),
        glb_limits: GlbBuildLimits::default(),
    })
    .expect_err("a geometry-less scene export is refused");
    assert_eq!(
        error,
        SceneGlbExportError::GeometryNotResolved {
            mesh_filter_count: 1
        }
    );
    assert!(error.to_string().contains("no geometry"), "{error}");

    // The same hierarchy without a MeshFilter still exports: it genuinely has nothing to draw.
    let without_geometry = hierarchy();
    export_scene_glb(SceneGlbExportInput {
        collection: &without_geometry,
        attached_meshes: &[],
        is_scene: true,
        meshes: &[],
        level_limits: GlbLevelLimits::default(),
        glb_limits: GlbBuildLimits::default(),
    })
    .expect("a scene with no MeshFilter has no geometry to lose");
}

/// Caller-supplied geometry turns the refusal into an export that carries a mesh node.
///
/// This is the half the port was missing: the view could report no mesh for any Transform, so a real
/// scene exported a node graph with no geometry. The caller now resolves each Transform's geometry
/// from the MeshFilter and Renderer fields and supplies it, and the node records the mesh object it
/// came from so the caller can supply the decoded `MeshData` for that node.
#[test]
fn supplied_geometry_is_attached_to_the_plan() {
    use crate::export::scene_wiring::MaterializedComponentSlot;
    use crate::export::scene_wiring::MaterializedComponentTarget;
    use crate::processing::scene::SceneRelationshipTarget;
    use crate::resources::glb_level::{
        GlbLevelAccumulatedTransform, GlbLevelAttachedMesh, GlbSourceMesh,
    };

    let mut collection = hierarchy();
    let mut with_filter = game_object(50, "WithGeometry");
    with_filter.components.push(MaterializedComponentSlot {
        source_array_index: 0,
        class_id: Some(33),
        target: MaterializedComponentTarget::Source {
            relationship_index: 0,
            file_id: 0,
            path_id: 51,
            target: SceneRelationshipTarget::LocalObject { object_index: 51 },
        },
    });
    collection.game_objects.push(with_filter);
    collection.transforms.push(transform(51, 50, None, None));

    let mesh = GlbLevelAttachedMesh {
        dynamic: false,
        subset_indices: vec![0],
        is_16_bit_indices: true,
        mesh_type: None,
        source: Some(GlbSourceMesh {
            file_id: 0,
            path_id: 7,
        }),
        global_inverse_transform: GlbLevelAccumulatedTransform::IDENTITY,
        global_transform: GlbLevelAccumulatedTransform::IDENTITY,
    };
    let mut attached: Vec<Option<GlbLevelAttachedMesh>> = vec![None; collection.transforms.len()];
    let position = collection.transforms.len() - 1;
    attached[position] = Some(mesh);
    let meshes: Vec<Option<crate::resources::mesh_data::MeshData>> =
        vec![None; collection.transforms.len()];

    let export = export_scene_glb(SceneGlbExportInput {
        collection: &collection,
        is_scene: true,
        attached_meshes: &attached,
        meshes: &meshes,
        level_limits: GlbLevelLimits::default(),
        glb_limits: GlbBuildLimits::default(),
    })
    .expect("supplying the geometry removes the refusal");
    assert_eq!(export.mesh_count, 1);

    let json = json_chunk(&export.bytes);
    assert!(json.contains("WithGeometry"), "{json}");
    assert!(
        json.contains("\"mesh\":"),
        "the node carries a mesh reference: {json}"
    );
}

#[test]
fn the_node_limit_reaches_the_export() {
    let collection = hierarchy();
    let meshes: Vec<Option<crate::resources::mesh_data::MeshData>> = vec![None; 4];
    let error = export_scene_glb(SceneGlbExportInput {
        collection: &collection,
        attached_meshes: &[],
        is_scene: true,
        meshes: &meshes,
        level_limits: GlbLevelLimits { max_nodes: 2 },
        glb_limits: GlbBuildLimits::default(),
    })
    .expect_err("the limit is enforced");
    assert!(matches!(error, SceneGlbExportError::Level(_)), "{error:?}");
}

#[test]
fn a_plan_without_geometry_still_encodes() {
    // The encoder is usable without a collection, which is what a prefab hierarchy would do.
    let plan = GlbLevelPlan {
        nodes: vec![crate::resources::glb_level::GlbLevelNode {
            name: "Solo".to_owned(),
            parent: None,
            local_transform: Some(GlbLevelLocalTransform {
                scale: [1.0, 1.0, 1.0],
                rotation: [0.0, 0.0, 0.0, 1.0],
                translation: [0.0, 0.0, 0.0],
            }),
            mesh: None,
        }],
        warnings: Vec::new(),
    };
    let export =
        export_plan_as_glb(&plan, &[], GlbBuildLimits::default()).expect("the plan encodes");
    assert_eq!(export.node_count, 1);
    let json = json_chunk(&export.bytes);
    assert!(json.contains("\"name\":\"Solo\""), "{json}");
    assert!(json.contains("\"scenes\":[{\"nodes\":[0]}]"), "{json}");

    // And it converts to the shared export shape the per-asset exporters return.
    let mesh_export = as_mesh_export(export);
    assert_eq!(&mesh_export.bytes[0..4], b"glTF");
    assert!(mesh_export.emitted_source_indices.is_empty());
}
