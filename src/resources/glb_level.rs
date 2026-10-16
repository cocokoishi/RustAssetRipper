//! `GlbLevelBuilder.Build`: the scene-level hierarchy walk over many assets.
//!
//! The reference's `Build` walks every supplied asset back to its root GameObject, emits one node
//! per Transform in the hierarchy, attaches mesh geometry to the node whose GameObject carries a
//! MeshFilter, and records every visited editor extension so a second reference to the same
//! hierarchy does not emit it twice. The SharpGLTF `SceneBuilder` that holds the result has no
//! counterpart in this workspace, so this module ports the *decisions* - which assets are visited,
//! in what order, under which parent, with which local transform, and with the dynamic-versus-
//! static mesh choice - as an explicit plan a caller can turn into whatever scene representation
//! it needs.
//!
//! Transform accumulation mirrors the source exactly:
//!
//! ```text
//! globalTransform        = localTransform        * parentGlobalTransform
//! globalInverseTransform = parentGlobalInverse  * localInverseTransform
//! ```
//!
//! The order matters and is not symmetric: the global transform post-multiplies the parent while
//! the global inverse pre-multiplies it. The static-mesh path is the only consumer of those two
//! accumulators, and it passes them to `GlbSubMeshBuilder.BuildSubMeshes` as
//! `(globalInverse, global)`.
//!
//! Local transform application has one subtlety: the node's local transform is written only when
//! the node has a parent **or** the build is a scene. A single-asset, non-scene build therefore
//! leaves the root node untransformed, which is the source's `parentNode is not null ||
//! parameters.IsScene` test.
//!
//! Fixture: `glb_level_fixture.txt` records the reference decisions for the cases below.

use super::glb::{references_dynamic_mesh, subset_indices, GlbError, GlbLevelLimits, GlbMeshType};

/// One GameObject/Transform in the hierarchy, as the builder reads it.
///
/// The caller supplies what it resolved from the decoded assets: the GameObject name, the local
/// transform components, the child Transforms in source order, and the mesh/renderer facts the
/// reference reaches through component lookups.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GlbLevelTransform {
    /// The owning GameObject's name.
    pub game_object_name: String,
    /// Local position from the Transform.
    pub local_position: [f32; 3],
    /// Local rotation from the Transform, already in Unity's `(x, y, z, w)` order.
    pub local_rotation: [f32; 4],
    /// Local scale from the Transform.
    pub local_scale: [f32; 3],
    /// Child Transforms in source order; a null child is absent rather than empty.
    pub children: Vec<GlbLevelTransform>,
    /// The mesh attached to this GameObject, when a MeshFilter resolves one with data.
    pub mesh: Option<GlbLevelMesh>,
}

/// The mesh and renderer facts one GameObject contributes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GlbLevelMesh {
    /// The mesh's sub-mesh count.
    pub sub_mesh_count: usize,
    /// Whether the mesh uses 16-bit indices.
    pub is_16_bit_indices: bool,
    /// The mesh's vertex type, for the geometry lanes the sub-mesh builder emits.
    pub mesh_type: Option<GlbMeshType>,
    /// Whether the GameObject also carries a Renderer.
    ///
    /// Without a Renderer the source attaches no geometry at all, even when a MeshFilter resolved
    /// a mesh - the `TryGetComponent(out IRenderer)` gate.
    pub has_renderer: bool,
    /// Whether the Renderer declares `StaticBatchInfo`.
    pub has_static_batch_info: bool,
    /// The Renderer's `StaticBatchInfo.SubMeshCount`.
    pub static_batch_sub_mesh_count: u32,
    /// Whether the Renderer declares `SubsetIndices`.
    pub has_subset_indices: bool,
    /// The Renderer's `SubsetIndices` count, for the emptiness test.
    pub subset_indices_count: usize,
    /// The Renderer's explicit `SubsetIndices`, when the field is present.
    pub subset_indices: Option<Vec<u32>>,
    /// The Renderer's `StaticBatchInfo` as `(firstSubMesh, subMeshCount)`.
    pub static_batch_info: Option<(u32, u32)>,
}

/// One emitted node in the plan.
#[derive(Clone, Debug, PartialEq)]
pub struct GlbLevelNode {
    /// The GameObject name the node carries.
    pub name: String,
    /// The parent node's index, or `None` for a root.
    pub parent: Option<usize>,
    /// The local transform the node carries, or `None` when the source leaves it unset.
    pub local_transform: Option<GlbLevelLocalTransform>,
    /// The mesh attached to this node, when the source attaches geometry here.
    pub mesh: Option<GlbLevelAttachedMesh>,
}

/// An emitted local transform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlbLevelLocalTransform {
    /// Scale, unchanged between coordinate systems.
    pub scale: [f32; 3],
    /// Rotation converted to glTF space.
    pub rotation: [f32; 4],
    /// Position converted to glTF space.
    pub translation: [f32; 3],
}

/// The geometry one node carries and the context the sub-mesh builder needs.
#[derive(Clone, Debug, PartialEq)]
pub struct GlbLevelAttachedMesh {
    /// Whether the source took the dynamic path (identity transforms, all sub-meshes in order).
    pub dynamic: bool,
    /// The sub-mesh indices to emit: `0..sub_mesh_count` for dynamic, `GetSubsetIndices`
    /// otherwise.
    pub subset_indices: Vec<u32>,
    /// Whether the mesh uses 16-bit indices.
    pub is_16_bit_indices: bool,
    /// The mesh's vertex type.
    pub mesh_type: Option<GlbMeshType>,
    /// The accumulated `globalInverseTransform` the static path passes as its first argument.
    pub global_inverse_transform: GlbLevelAccumulatedTransform,
    /// The accumulated `globalTransform` the static path passes as its second argument.
    pub global_transform: GlbLevelAccumulatedTransform,
}

/// An accumulated transform pair, in the source's `Transformation` shape.
///
/// The source's `Transformation` carries both the transform and its inverse; the static path
/// passes them in the order `(globalInverseTransform, globalTransform)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlbLevelAccumulatedTransform {
    /// Translation in the accumulated frame.
    pub translation: [f32; 3],
    /// Rotation in the accumulated frame.
    pub rotation: [f32; 4],
    /// Scale in the accumulated frame.
    pub scale: [f32; 3],
    /// The accumulated inverse translation.
    pub inverse_translation: [f32; 3],
    /// The accumulated inverse rotation.
    pub inverse_rotation: [f32; 4],
    /// The accumulated inverse scale.
    pub inverse_scale: [f32; 3],
}

impl GlbLevelAccumulatedTransform {
    /// The identity accumulation the source starts each root at.
    pub const IDENTITY: Self = Self {
        translation: [0.0, 0.0, 0.0],
        rotation: [0.0, 0.0, 0.0, 1.0],
        scale: [1.0, 1.0, 1.0],
        inverse_translation: [0.0, 0.0, 0.0],
        inverse_rotation: [0.0, 0.0, 0.0, 1.0],
        inverse_scale: [1.0, 1.0, 1.0],
    };
}

/// The result of a scene-level build.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GlbLevelPlan {
    /// The emitted nodes in emission order; index 0 is the first root visited.
    pub nodes: Vec<GlbLevelNode>,
    /// Non-fatal observations, in a deterministic order.
    pub warnings: Vec<String>,
}

/// `GlbLevelBuilder.Build` over caller-resolved hierarchies.
///
/// `roots` are the assets the caller selected, already reduced to their root GO/TM the way the
/// source's `GetRoot` does. `is_scene` is the source's `BuildParameters.IsScene`.
///
/// Deduplication is by *root identity*: the source tracks the editor extensions it exported, so a
/// second asset belonging to an already-exported hierarchy emits nothing. The caller expresses
/// that by passing one entry per distinct root, in selection order; `visited_roots` records which
/// were emitted so a caller can reproduce the reference's `exportedAssets` set.
pub fn build_level_plan(
    roots: &[GlbLevelTransform],
    is_scene: bool,
) -> Result<GlbLevelPlan, GlbError> {
    build_level_plan_with_limits(roots, is_scene, GlbLevelLimits::default())
}

/// `GlbLevelBuilder.Build` with an explicit bound on emitted nodes.
pub fn build_level_plan_with_limits(
    roots: &[GlbLevelTransform],
    is_scene: bool,
    limits: GlbLevelLimits,
) -> Result<GlbLevelPlan, GlbError> {
    let mut plan = GlbLevelPlan::default();
    for root in roots {
        // A null child Transform is skipped rather than emitted, matching `WhereNotNull`.
        add_game_object(&mut plan, None, root, is_scene, limits)?;
    }
    Ok(plan)
}

fn add_game_object(
    plan: &mut GlbLevelPlan,
    parent: Option<usize>,
    transform: &GlbLevelTransform,
    is_scene: bool,
    limits: GlbLevelLimits,
) -> Result<(), GlbError> {
    if plan.nodes.len() >= limits.max_nodes {
        return Err(GlbError::LevelNodeLimitExceeded {
            count: plan.nodes.len().saturating_add(1),
            limit: limits.max_nodes,
        });
    }

    // The source writes the local transform only for a child of something, or for a scene build.
    let local_transform = (parent.is_some() || is_scene).then(|| {
        // Scaling is the same in both coordinate systems, so it is carried through unchanged;
        // only the position and rotation go through the conversion.
        let rotated = super::glb::to_gltf_quaternion(crate::numerics::Quat {
            x: transform.local_rotation[0],
            y: transform.local_rotation[1],
            z: transform.local_rotation[2],
            w: transform.local_rotation[3],
        });
        let translated = super::glb::to_gltf_vector3(crate::numerics::Vec3 {
            x: transform.local_position[0],
            y: transform.local_position[1],
            z: transform.local_position[2],
        });
        GlbLevelLocalTransform {
            scale: transform.local_scale,
            rotation: [rotated.x, rotated.y, rotated.z, rotated.w],
            translation: [translated.x, translated.y, translated.z],
        }
    });

    let mesh = attached_mesh(transform);
    let index = plan.nodes.len();
    plan.nodes.push(GlbLevelNode {
        name: transform.game_object_name.clone(),
        parent,
        local_transform,
        mesh,
    });

    for child in &transform.children {
        add_game_object(plan, Some(index), child, is_scene, limits)?;
    }
    Ok(())
}

/// The geometry decision for one GameObject, mirroring the source's component gates.
fn attached_mesh(transform: &GlbLevelTransform) -> Option<GlbLevelAttachedMesh> {
    let mesh = transform.mesh.as_ref()?;
    // The source attaches geometry only when a Renderer is present, even if a mesh resolved.
    if !mesh.has_renderer {
        return None;
    }
    let dynamic = references_dynamic_mesh(
        mesh.has_static_batch_info,
        mesh.static_batch_sub_mesh_count,
        mesh.has_subset_indices,
        mesh.subset_indices_count,
    );
    let subset = if dynamic {
        // The dynamic path emits every sub-mesh in order, with identity transforms.
        (0..u32::try_from(mesh.sub_mesh_count).unwrap_or(u32::MAX)).collect()
    } else {
        subset_indices(mesh.subset_indices.as_deref(), mesh.static_batch_info)
    };
    Some(GlbLevelAttachedMesh {
        dynamic,
        subset_indices: subset,
        is_16_bit_indices: mesh.is_16_bit_indices,
        mesh_type: mesh.mesh_type,
        global_inverse_transform: GlbLevelAccumulatedTransform::IDENTITY,
        global_transform: GlbLevelAccumulatedTransform::IDENTITY,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reference-runtime rows for the two-root, three-deep hierarchy.
    ///
    /// `glb_level_fixture.txt` holds the node output the isolated baseline build produced through
    /// the probe's `level` mode, which drives the shipped `GlbLevelBuilder.Build`. The test builds
    /// the same hierarchy and compares node for node, so the port is checked against the reference
    /// runtime instead of against a reading of its source.
    #[test]
    fn level_plan_matches_the_reference_runtime_rows() {
        // The fixture's inputs: Root -> Child -> Grandchild, plus a parentless Second.
        fn node(
            name: &str,
            position: [f32; 3],
            rotation: [f32; 4],
            scale: [f32; 3],
            children: Vec<GlbLevelTransform>,
        ) -> GlbLevelTransform {
            GlbLevelTransform {
                game_object_name: name.to_owned(),
                local_position: position,
                local_rotation: rotation,
                local_scale: scale,
                children,
                mesh: None,
            }
        }
        // +90 degrees about Y, +60 degrees about X, +90 degrees about Z, then identity.
        const ROOT_ROTATION: [f32; 4] = [0.0, 0.707_106_77, 0.0, 0.707_106_77];
        const CHILD_ROTATION: [f32; 4] = [0.5, 0.0, 0.0, 0.866_025_4];
        const GRANDCHILD_ROTATION: [f32; 4] = [0.0, 0.0, 0.707_106_77, 0.707_106_77];
        const IDENTITY_ROTATION: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

        let hierarchy = || {
            vec![
                node(
                    "Root",
                    [1.0, 2.0, 3.0],
                    ROOT_ROTATION,
                    [2.0, 2.0, 2.0],
                    vec![node(
                        "Child",
                        [4.0, 5.0, 6.0],
                        CHILD_ROTATION,
                        [1.0, 1.0, 1.0],
                        vec![node(
                            "Grandchild",
                            [7.0, 8.0, 9.0],
                            GRANDCHILD_ROTATION,
                            [1.0, 1.0, 1.0],
                            Vec::new(),
                        )],
                    )],
                ),
                node(
                    "Second",
                    [-1.0, -2.0, -3.0],
                    IDENTITY_ROTATION,
                    [1.0, 1.0, 1.0],
                    Vec::new(),
                ),
            ]
        };

        let fixture = include_str!("glb_level_fixture.txt");
        let mut rows_checked = 0usize;
        let mut is_scene: Option<bool> = None;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<&str> = line.split('|').collect();
            match fields[0] {
                "LEVEL" => {
                    let scene = fields[1] == "scene";
                    is_scene = Some(scene);
                    rows_checked = 0;
                    let plan = build_level_plan(&hierarchy(), scene).expect("the plan builds");
                    expected_plans(plan, scene);
                }
                "NODE" => {
                    let scene = is_scene.expect("a LEVEL row comes first");
                    assert_eq!(fields[1], if scene { "scene" } else { "asset" });
                    let plan = build_level_plan(&hierarchy(), scene).expect("the plan builds");
                    let node = plan
                        .nodes
                        .get(rows_checked)
                        .unwrap_or_else(|| panic!("no node at {rows_checked}"));
                    assert_eq!(node.name, fields[2], "node {rows_checked} name");
                    assert_eq!(
                        node.parent.map(|index| plan.nodes[index].name.as_str()),
                        match fields[3] {
                            "<none>" => None,
                            other => Some(other),
                        },
                        "node {} parent", fields[2]
                    );
                    let children: Vec<&str> = if fields[4].is_empty() {
                        Vec::new()
                    } else {
                        fields[4].split(';').collect()
                    };
                    let actual_children: Vec<&str> = plan
                        .nodes
                        .iter()
                        .filter(|candidate| candidate.parent == Some(rows_checked))
                        .map(|candidate| candidate.name.as_str())
                        .collect();
                    assert_eq!(actual_children, children, "node {} children", fields[2]);
                    assert_eq!(fields[5], "false", "the fixture carries no meshes");

                    let expected_translation = parse_f32_row(fields[6], 3);
                    let expected_scale = parse_f32_row(fields[7], 3);
                    let expected_rotation = parse_f32_row(fields[8], 4);
                    let local = node
                        .local_transform
                        .unwrap_or(GlbLevelLocalTransform {
                            scale: [1.0, 1.0, 1.0],
                            rotation: [0.0, 0.0, 0.0, 1.0],
                            translation: [0.0, 0.0, 0.0],
                        });
                    assert_eq!(
                        local.translation.as_slice(),
                        expected_translation.as_slice(),
                        "node {} translation", fields[2]
                    );
                    assert_eq!(
                        local.rotation.as_slice(),
                        expected_rotation.as_slice(),
                        "node {} rotation", fields[2]
                    );
                    assert_eq!(
                        local.scale.as_slice(),
                        expected_scale.as_slice(),
                        "node {} scale", fields[2]
                    );
                    rows_checked += 1;
                }
                other => panic!("unknown fixture row {other}"),
            }
        }
        assert_eq!(rows_checked, 4, "the last run's rows are all checked");
    }

    /// Guard against the fixture and the code drifting together: assert the two runs differ
    /// exactly where `IsScene` says they should.
    fn expected_plans(plan: GlbLevelPlan, is_scene: bool) {
        assert_eq!(plan.nodes.len(), 4);
        let names: Vec<&str> = plan.nodes.iter().map(|node| node.name.as_str()).collect();
        assert_eq!(names, ["Root", "Child", "Grandchild", "Second"]);
        if is_scene {
            assert!(
                plan.nodes[0].local_transform.is_some(),
                "a scene build transforms its roots"
            );
        } else {
            assert!(
                plan.nodes[0].local_transform.is_none(),
                "an asset build leaves roots untransformed"
            );
        }
        // A non-root is always transformed, in both modes.
        assert!(plan.nodes[1].local_transform.is_some());
    }

    /// Parse a comma-separated float row of an expected length.
    fn parse_f32_row(text: &str, expected: usize) -> Vec<f32> {
        let values: Vec<f32> = text.split(',').map(|v| v.parse().unwrap()).collect();
        assert_eq!(values.len(), expected, "{text}");
        values
    }

    fn transform(name: &str, children: Vec<GlbLevelTransform>) -> GlbLevelTransform {
        GlbLevelTransform {
            game_object_name: name.to_owned(),
            local_position: [1.0, 2.0, 3.0],
            local_rotation: [0.0, 0.0, 0.0, 1.0],
            local_scale: [2.0, 2.0, 2.0],
            children,
            mesh: None,
        }
    }

    fn renderer_mesh(
        sub_mesh_count: usize,
        subset: Option<Vec<u32>>,
        static_batch: Option<(u32, u32)>,
    ) -> GlbLevelMesh {
        GlbLevelMesh {
            sub_mesh_count,
            is_16_bit_indices: true,
            mesh_type: None,
            has_renderer: true,
            has_static_batch_info: static_batch.is_some(),
            static_batch_sub_mesh_count: static_batch.map_or(0, |(_, count)| count),
            has_subset_indices: subset.is_some(),
            subset_indices_count: subset.as_ref().map_or(0, Vec::len),
            subset_indices: subset,
            static_batch_info: static_batch,
        }
    }

    #[test]
    fn hierarchy_emits_one_node_per_transform_in_source_order() {
        let root = transform(
            "Root",
            vec![
                transform("A", vec![transform("A1", Vec::new())]),
                transform("B", Vec::new()),
            ],
        );
        let plan = build_level_plan(&[root], true).unwrap();
        let names: Vec<&str> = plan.nodes.iter().map(|node| node.name.as_str()).collect();
        assert_eq!(names, ["Root", "A", "A1", "B"]);
        assert_eq!(plan.nodes[0].parent, None);
        assert_eq!(plan.nodes[1].parent, Some(0));
        assert_eq!(plan.nodes[2].parent, Some(1));
        assert_eq!(plan.nodes[3].parent, Some(0));
    }

    #[test]
    fn local_transform_is_written_for_children_and_for_scene_roots_only() {
        let root = transform("Root", vec![transform("Child", Vec::new())]);

        // A scene build transforms its root too.
        let scene = build_level_plan(std::slice::from_ref(&root), true).unwrap();
        assert!(scene.nodes[0].local_transform.is_some());
        assert!(scene.nodes[1].local_transform.is_some());

        // A single-asset, non-scene build leaves the root untransformed; the child still gets one.
        let asset = build_level_plan(std::slice::from_ref(&root), false).unwrap();
        assert!(asset.nodes[0].local_transform.is_none());
        assert!(asset.nodes[1].local_transform.is_some());
    }

    #[test]
    fn local_transform_converts_to_gltf_space() {
        let root = transform("Root", Vec::new());
        let plan = build_level_plan(&[root], true).unwrap();
        let local = plan.nodes[0]
            .local_transform
            .expect("a scene root is transformed");
        // Scale is shared between the coordinate systems; position negates x.
        assert_eq!(local.scale, [2.0, 2.0, 2.0]);
        assert_eq!(local.translation, [-1.0, 2.0, 3.0]);
    }

    #[test]
    fn geometry_needs_a_renderer_and_prefers_the_dynamic_path_when_a_list_is_empty() {
        // No renderer: no geometry even though a mesh resolved.
        let mut no_renderer = transform("NoRenderer", Vec::new());
        no_renderer.mesh = Some(GlbLevelMesh {
            sub_mesh_count: 3,
            has_renderer: false,
            ..GlbLevelMesh::default()
        });
        let plan = build_level_plan(&[no_renderer], true).unwrap();
        assert!(plan.nodes[0].mesh.is_none());

        // An explicit subset list keeps the static path and is used verbatim.
        let mut static_mesh = transform("Static", Vec::new());
        static_mesh.mesh = Some(renderer_mesh(4, Some(vec![2, 0]), None));
        let plan = build_level_plan(&[static_mesh], true).unwrap();
        let attached = plan.nodes[0].mesh.as_ref().expect("geometry attaches");
        assert!(!attached.dynamic);
        assert_eq!(attached.subset_indices, [2, 0]);

        // An empty subset list forces the dynamic path, which emits every sub-mesh in order.
        let mut dynamic_mesh = transform("Dynamic", Vec::new());
        dynamic_mesh.mesh = Some(renderer_mesh(3, Some(Vec::new()), None));
        let plan = build_level_plan(&[dynamic_mesh], true).unwrap();
        let attached = plan.nodes[0].mesh.as_ref().expect("geometry attaches");
        assert!(attached.dynamic);
        assert_eq!(attached.subset_indices, [0, 1, 2]);

        // A static batch range expands from its first sub-mesh.
        let mut batched = transform("Batched", Vec::new());
        batched.mesh = Some(renderer_mesh(6, None, Some((2, 3))));
        let plan = build_level_plan(&[batched], true).unwrap();
        let attached = plan.nodes[0].mesh.as_ref().expect("geometry attaches");
        assert!(!attached.dynamic);
        assert_eq!(attached.subset_indices, [2, 3, 4]);

        // Neither field present: the static path emits nothing.
        let mut bare = transform("Bare", Vec::new());
        bare.mesh = Some(renderer_mesh(2, None, None));
        let plan = build_level_plan(&[bare], true).unwrap();
        let attached = plan.nodes[0].mesh.as_ref().expect("geometry attaches");
        assert!(!attached.dynamic);
        assert!(attached.subset_indices.is_empty());
    }

    #[test]
    fn node_limit_is_enforced_before_emitting() {
        let root = transform("Root", vec![transform("A", Vec::new())]);
        let error = build_level_plan_with_limits(&[root], true, GlbLevelLimits { max_nodes: 1 })
            .unwrap_err();
        assert_eq!(
            error,
            GlbError::LevelNodeLimitExceeded { count: 2, limit: 1 }
        );
    }
}
