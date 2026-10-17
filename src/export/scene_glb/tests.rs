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
    // No decoded geometry: every node with a mesh selection is skipped with a warning.
    let meshes: Vec<(GlbSourceMesh, crate::resources::mesh_data::MeshData)> = Vec::new();
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
    // No decoded geometry: every node with a mesh selection is skipped with a warning.
    let meshes: Vec<(GlbSourceMesh, crate::resources::mesh_data::MeshData)> = Vec::new();
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
    use crate::resources::glb_level::{GlbLevelMesh, GlbSourceMesh};

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

    // The facts the caller resolved: one sub-mesh, a renderer, and a subset list that names it.
    let mesh = GlbLevelMesh {
        sub_mesh_count: 1,
        is_16_bit_indices: true,
        has_renderer: true,
        has_subset_indices: true,
        subset_indices_count: 1,
        subset_indices: Some(vec![0]),
        source: Some(GlbSourceMesh {
            file_id: 0,
            path_id: 7,
        }),
        ..GlbLevelMesh::default()
    };
    let mut attached: Vec<Option<GlbLevelMesh>> = vec![None; collection.transforms.len()];
    let position = collection.transforms.len() - 1;
    attached[position] = Some(mesh);
    let meshes: Vec<(GlbSourceMesh, crate::resources::mesh_data::MeshData)> = Vec::new();

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

/// The mesh gate runs before the renderer gate, and the small payload readers agree with the fields.
///
/// The positive path - a valid mesh payload yielding `MeshData` - needs a vertex blob fixture that
/// does not exist yet, so it is **not** covered here. What is covered is the ordering the source's
/// `&&` chain encodes and the small readers, which is what a synthetic payload can decide.
#[test]
fn the_mesh_gate_runs_before_the_renderer_gate() {
    use crate::resources::glb_level::GlbSourceMesh;
    use crate::resources::packed_bit_vector::PackedBitVectorLimits;
    use crate::resources::vertex_data_blob::{EndianType, UnityVersion};
    use crate::serialized_file::object::{SerializedFieldValue, SerializedValue};

    let payload = |fields: Vec<(&str, SerializedValue)>| {
        SerializedValue::Struct(
            fields
                .into_iter()
                .map(|(name, value)| SerializedFieldValue {
                    name: name.to_owned(),
                    value,
                })
                .collect(),
        )
    };
    let version = UnityVersion {
        major: 2019,
        minor: 4,
        patch: 40,
    };
    let source = GlbSourceMesh {
        file_id: 0,
        path_id: 7,
    };

    // A mesh payload with no vertex data is not set, so nothing is attached even with a renderer.
    let unset_mesh = payload(vec![]);
    let renderer = payload(vec![(
        "m_SubsetIndices",
        SerializedValue::Array(vec![SerializedValue::U32(0)]),
    )]);
    assert_eq!(
        attached_mesh_facts(
            &unset_mesh,
            Some(&renderer),
            version,
            EndianType::Little,
            PackedBitVectorLimits::default(),
            source,
        ),
        None,
        "an unset mesh attaches nothing"
    );

    // The readers themselves.
    assert_eq!(int_field(&SerializedValue::U32(7)), Some(7));
    assert_eq!(int_field(&SerializedValue::I64(-3)), Some(-3));
    assert_eq!(int_field(&SerializedValue::Bool(true)), None);
    let pptr = payload(vec![(
        "m_Mesh",
        SerializedValue::PPtr {
            file_id: 0,
            path_id: 42,
        },
    )]);
    assert_eq!(pptr_field(&pptr, "m_Mesh"), Some((0, 42)));
    assert_eq!(pptr_field(&pptr, "m_Other"), None);
    assert!(field(&pptr, "m_Mesh").is_some());
}

#[test]
fn the_node_limit_reaches_the_export() {
    let collection = hierarchy();
    // No decoded geometry: every node with a mesh selection is skipped with a warning.
    let meshes: Vec<(GlbSourceMesh, crate::resources::mesh_data::MeshData)> = Vec::new();
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

/// The scene GLB geometry is anchored to a primitive the reference runtime emitted.
///
/// `glb_level_mesh_fixture.txt` holds the primitive output the isolated baseline build produced
/// through the probe's `mesh` mode, which drives the shipped `GlbLevelBuilder.Build` over a
/// GameObject carrying a MeshFilter, a MeshRenderer and one triangle. This test builds the same
/// hierarchy and `MeshData`, replays them through [`export_plan_as_glb`] - the encoder
/// `export_scene_glb` itself calls - and compares the decoded node, mesh, positions and indices
/// against the fixture. Without it the geometry half of the scene export rested on a reading of the
/// reference source rather than on a runtime row.
#[test]
fn scene_geometry_matches_the_reference_runtime_rows() {
    use crate::resources::compressed_mesh::{DecompressedMesh, Vec3 as CompressedVec3};
    use crate::resources::glb_level::{build_level_plan, GlbLevelMesh, GlbLevelTransform};
    use crate::resources::mesh_data::{MeshData, MeshParts};
    use crate::resources::sub_mesh_data::{Bounds, MeshTopology, SubMeshData};

    // The fixture's mesh: three Float32 positions and three 16-bit indices forming one triangle.
    let mesh_data = MeshData::try_make_from_parts(MeshParts {
        blob: None,
        compressed: DecompressedMesh {
            vertices: Some(vec![
                CompressedVec3::new(0.0, 0.0, 0.0),
                CompressedVec3::new(1.0, 0.0, 0.0),
                CompressedVec3::new(0.0, 1.0, 0.0),
            ]),
            processed_index_buffer: Some(vec![0, 1, 2]),
            ..DecompressedMesh::default()
        },
        sub_meshes: vec![SubMeshData {
            base_vertex: 0,
            first_index: 0,
            first_vertex: 0,
            index_count: 3,
            triangle_count: 1,
            vertex_count: 3,
            topology: MeshTopology::TRIANGLES,
            local_bounds: Bounds::default(),
        }],
        mesh_bind_pose: None,
        mesh_index_buffer: Vec::new(),
        mesh_skin: None,
    })
    .expect("the triangle mesh builds");

    // The probe's renderer has an empty `StaticBatchInfo` (`SubMeshCount == 0`), so the reference's
    // `ReferencesDynamicMesh` takes the dynamic arm and emits every sub-mesh in order.
    let source = GlbSourceMesh {
        file_id: 0,
        path_id: 2004,
    };
    let root = GlbLevelTransform {
        game_object_name: "Meshed".to_owned(),
        local_position: [1.0, 0.0, 0.0],
        local_rotation: [0.0, 0.0, 0.0, 1.0],
        local_scale: [1.0, 1.0, 1.0],
        children: Vec::new(),
        mesh: Some(GlbLevelMesh {
            sub_mesh_count: 1,
            is_16_bit_indices: true,
            mesh_type: Some(crate::resources::glb::get_mesh_type(&mesh_data)),
            has_renderer: true,
            has_static_batch_info: true,
            static_batch_sub_mesh_count: 0,
            static_batch_info: Some((0, 0)),
            source: Some(source),
            ..GlbLevelMesh::default()
        }),
    };
    let plan = build_level_plan(&[root], false).expect("the plan builds");
    let meshes = vec![(source, mesh_data)];
    let export =
        export_plan_as_glb(&plan, &meshes, GlbBuildLimits::default()).expect("the plan encodes");
    assert_eq!(export.node_count, 1);
    assert_eq!(export.mesh_count, 1);
    assert!(export.warnings.is_empty(), "{:?}", export.warnings);

    let json = json_chunk(&export.bytes);
    let bin = bin_chunk(&export.bytes);

    // Parse the fixture into the expected rows before comparing, so a malformed fixture is a
    // failure rather than a silently empty expectation.
    let fixture = include_str!("../../resources/glb_level_mesh_fixture.txt");
    let mut expected_positions: Vec<[f32; 3]> = Vec::new();
    let mut expected_indices: Option<Vec<u32>> = None;
    let mut rows = 0usize;
    for line in fixture.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split('|').collect();
        match fields[0] {
            "LIN" => {
                assert_eq!(fields[1], "mesh");
                assert_eq!(fields[2], "2019.4.0f1");
                assert_eq!(fields[3], "scene=False");
            }
            "LNODE" => {
                assert_eq!(fields[2], "Meshed");
                assert_eq!(fields[3], "mesh=True");
            }
            "GMESH" => {
                assert_eq!(fields[2], "0", "the fixture's mesh index");
                assert_eq!(fields[3], "", "the reference mesh carries no name");
                assert_eq!(fields[4], "0", "the fixture's primitive index");
            }
            "GPOS" => {
                assert_eq!(fields[2], expected_positions.len().to_string());
                expected_positions.push([
                    fields[3].parse().expect("a float component"),
                    fields[4].parse().expect("a float component"),
                    fields[5].parse().expect("a float component"),
                ]);
            }
            "GIDX" => {
                expected_indices = Some(
                    fields[2]
                        .split(',')
                        .map(|value| value.parse().expect("an index"))
                        .collect(),
                );
            }
            other => panic!("unknown fixture row {other}"),
        }
        rows += 1;
    }
    assert_eq!(rows, 7, "every fixture row is checked");
    let expected_indices = expected_indices.expect("the fixture carries an index row");

    // The document: one node named Meshed that references the single mesh.
    assert!(json.contains("\"name\":\"Meshed\""), "{json}");
    assert!(json.contains("\"mesh\":0"), "{json}");

    // The primitive's decoded geometry.
    let primitive = single_primitive(&json);
    let position_accessor =
        attribute_accessor(&primitive, "POSITION").expect("POSITION is emitted");
    let index_accessor = json_number(&primitive, "indices").expect("indices are emitted") as usize;
    let positions = accessor_floats(&json, &bin, position_accessor);
    let indices = accessor_indices(&json, &bin, index_accessor);

    // Compare position bits, not values: the conversion of a zero component produces a negative
    // zero, and `-0.0 == 0.0` would hide a missing conversion.
    let decoded: Vec<[f32; 3]> = positions
        .chunks_exact(3)
        .map(|c| [c[0], c[1], c[2]])
        .collect();
    assert_eq!(decoded.len(), expected_positions.len(), "{json}");
    for (got, want) in decoded.iter().zip(&expected_positions) {
        for axis in 0..3 {
            assert_eq!(
                got[axis].to_bits(),
                want[axis].to_bits(),
                "position {got:?} != {want:?}"
            );
        }
    }
    assert_eq!(indices, expected_indices);
}

/// The binary chunk of a GLB, which is the buffer the accessors index into.
fn bin_chunk(bytes: &[u8]) -> Vec<u8> {
    let json_length = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
    let offset = 20 + json_length;
    assert_eq!(&bytes[offset + 4..offset + 8], b"BIN\0");
    let length = u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ]) as usize;
    bytes[offset + 8..offset + 8 + length].to_vec()
}

/// The raw text of a top-level JSON array value.
fn json_array<'a>(json: &'a str, key: &str) -> Option<&'a str> {
    let bytes = json.as_bytes();
    let needle = format!("\"{key}\"");
    let mut object_depth = 0usize;
    let mut array_depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut index = 0usize;

    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }

        match byte {
            b'"' => {
                if object_depth == 1 && array_depth == 0 && json[index..].starts_with(&needle) {
                    let mut cursor = index + needle.len();
                    while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
                        cursor += 1;
                    }
                    if bytes.get(cursor) == Some(&b':') {
                        cursor += 1;
                        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
                            cursor += 1;
                        }
                        if bytes.get(cursor) == Some(&b'[') {
                            let start = cursor + 1;
                            let mut depth = 1usize;
                            let mut array_string = false;
                            let mut array_escape = false;
                            for end in start..bytes.len() {
                                let array_byte = bytes[end];
                                if array_string {
                                    if array_escape {
                                        array_escape = false;
                                    } else if array_byte == b'\\' {
                                        array_escape = true;
                                    } else if array_byte == b'"' {
                                        array_string = false;
                                    }
                                } else {
                                    match array_byte {
                                        b'"' => array_string = true,
                                        b'[' => depth += 1,
                                        b']' => {
                                            depth -= 1;
                                            if depth == 0 {
                                                return Some(&json[start..end]);
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            return None;
                        }
                    }
                }
                in_string = true;
            }
            b'{' => object_depth += 1,
            b'}' => object_depth = object_depth.saturating_sub(1),
            b'[' => array_depth += 1,
            b']' => array_depth = array_depth.saturating_sub(1),
            _ => {}
        }
        index += 1;
    }
    None
}

/// Splits a flat array of JSON objects into its object bodies.
fn json_objects(array: &str) -> Vec<String> {
    array
        .split("},{")
        .map(|part| part.trim_matches(|character| character == '{' || character == '}'))
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The integer value of a flat object field.
fn json_number(object: &str, key: &str) -> Option<i64> {
    let needle = format!("\"{key}\":");
    let start = object.find(&needle)? + needle.len();
    let rest = &object[start..];
    let end = rest
        .find(|character: char| !(character.is_ascii_digit() || character == '-'))
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// The string value of a flat object field.
fn json_string(object: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":\"");
    let start = object.find(&needle)? + needle.len();
    let end = object[start..].find('"')? + start;
    Some(object[start..end].to_owned())
}

/// The single primitive body of the single-mesh JSON layout.
fn single_primitive(json: &str) -> String {
    let anchor = json
        .find("\"primitives\":[")
        .expect("a primitive is emitted");
    let start = anchor + "\"primitives\":[".len();
    let end = json[start..].find(']').expect("the primitive closes") + start;
    json[start..end]
        .trim()
        .trim_matches(|character| character == '{' || character == '}')
        .to_owned()
}

/// The accessor number of a named primitive attribute.
fn attribute_accessor(primitive: &str, name: &str) -> Option<usize> {
    let anchor = primitive.find("\"attributes\":{")? + "\"attributes\":{".len();
    let end = primitive[anchor..].find('}')? + anchor;
    json_number(&primitive[anchor..end], name).map(|value| value as usize)
}

/// The decoded floats of one accessor, in accessor order.
fn accessor_floats(json: &str, bin: &[u8], accessor: usize) -> Vec<f32> {
    let accessors = json_objects(json_array(json, "accessors").expect("accessors"));
    let views = json_objects(json_array(json, "bufferViews").expect("bufferViews"));
    let object = &accessors[accessor];
    let view = &views[json_number(object, "bufferView").expect("a bufferView") as usize];
    let count = json_number(object, "count").expect("a count") as usize;
    let components = match json_string(object, "type").expect("a type").as_str() {
        "VEC2" => 2,
        "VEC3" => 3,
        _ => 4,
    };
    let offset = json_number(view, "byteOffset").expect("a byteOffset") as usize;
    (0..count * components)
        .map(|index| {
            let start = offset + index * 4;
            f32::from_le_bytes(bin[start..start + 4].try_into().unwrap())
        })
        .collect()
}

/// The decoded indices of one accessor.
fn accessor_indices(json: &str, bin: &[u8], accessor: usize) -> Vec<u32> {
    let accessors = json_objects(json_array(json, "accessors").expect("accessors"));
    let views = json_objects(json_array(json, "bufferViews").expect("bufferViews"));
    let object = &accessors[accessor];
    let view = &views[json_number(object, "bufferView").expect("a bufferView") as usize];
    let count = json_number(object, "count").expect("a count") as usize;
    let component_type = json_number(object, "componentType").expect("a componentType");
    let offset = json_number(view, "byteOffset").expect("a byteOffset") as usize;
    (0..count)
        .map(|index| match component_type {
            5123 => {
                let start = offset + index * 2;
                u16::from_le_bytes(bin[start..start + 2].try_into().unwrap()) as u32
            }
            _ => {
                let start = offset + index * 4;
                u32::from_le_bytes(bin[start..start + 4].try_into().unwrap())
            }
        })
        .collect()
}
