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
//!
//! [`build_scene_plan`] adapts a materialized scene collection to this plan, which is the step the
//! reference performs in `GlbModelExporter.ExportModel` when it hands
//! `SceneHierarchyObject.Assets` to `GlbLevelBuilder.Build`.

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
    /// Which mesh object this geometry came from.
    ///
    /// The walk records the facts about a mesh, not the mesh itself, so without this a caller that
    /// wants to supply the decoded geometry cannot say which node wants which mesh. The reference
    /// holds the `IMesh` objects directly and never needs the identity; a caller-side walk does.
    pub source: Option<GlbSourceMesh>,
}

/// The mesh object a node's geometry came from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GlbSourceMesh {
    /// The reference's file table index; `0` is this file.
    pub file_id: i32,
    /// The mesh object's PathID.
    pub path_id: i64,
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
    /// Which mesh object this geometry came from, for a caller supplying the decoded geometry.
    pub source: Option<GlbSourceMesh>,
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
    attached_mesh_for(transform.mesh.as_ref()?)
}

/// The geometry decision for a GameObject's resolved mesh and renderer facts.
///
/// Split out from [`attached_mesh`] so a caller that resolved the facts itself - the scene-level
/// export does, because the materialized collection carries component slots and not their fields -
/// gets the same decision rather than a second copy of it. The source attaches geometry only when a
/// Renderer is present even if a mesh resolved, and the dynamic/static choice reads the Renderer's
/// batching fields.
pub fn attached_mesh_for(mesh: &GlbLevelMesh) -> Option<GlbLevelAttachedMesh> {
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
        source: mesh.source,
        global_inverse_transform: GlbLevelAccumulatedTransform::IDENTITY,
        global_transform: GlbLevelAccumulatedTransform::IDENTITY,
    })
}

/// Turn a plan into a multi-node glTF scene document.
///
/// This is the second half of `GlbLevelBuilder.Build`: the plan decides the node graph, and this
/// emits it. Each node carries its local transform when the plan recorded one, and the scene
/// lists the roots the plan found.
///
/// Geometry is not attached here: a plan node records the *sub-mesh selection* a renderer asks
/// for, and turning that into primitives needs the decoded `MeshData` the caller holds. Nodes
/// with a mesh therefore emit a placeholder `mesh` reference the caller fills by appending to
/// `meshes`, keeping the node/accessor bookkeeping in one place.
pub fn build_scene_document(plan: &GlbLevelPlan) -> super::glb::SceneDocument {
    let mut document = super::glb::SceneDocument {
        nodes: Vec::with_capacity(plan.nodes.len()),
        meshes: Vec::new(),
        accessors: Vec::new(),
        buffer_views: Vec::new(),
        bin: Vec::new(),
        scene_roots: Vec::new(),
    };

    // Children are recorded by parent index, so a node's `children` array needs the full graph.
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); plan.nodes.len()];
    for (index, node) in plan.nodes.iter().enumerate() {
        if let Some(parent) = node.parent {
            children[parent].push(index);
        } else {
            document.scene_roots.push(index);
        }
    }

    for (index, node) in plan.nodes.iter().enumerate() {
        let mut fields: Vec<String> = vec![format!(
            "\"name\":\"{}\"",
            super::glb::escape_json(&node.name)
        )];
        if let Some(local) = node.local_transform {
            fields.push(super::glb::node_transform_json(
                local.translation,
                local.rotation,
                local.scale,
            ));
        }
        if !children[index].is_empty() {
            let list = children[index]
                .iter()
                .map(|child| child.to_string())
                .collect::<Vec<_>>()
                .join(",");
            fields.push(format!("\"children\":[{list}]"));
        }
        if node.mesh.is_some() {
            // The caller appends the mesh and rewrites this index; a plan alone has no geometry.
            fields.push("\"mesh\":0".to_owned());
        }
        document.nodes.push(format!("{{{}}}", fields.join(",")));
    }
    document
}

/// Build a plan from a materialized scene collection.
///
/// This is the port of the step the reference performs in `GlbModelExporter.ExportModel`: it
/// hands `SceneHierarchyObject.Assets` to `GlbLevelBuilder.Build`, which walks each asset back to
/// its root GameObject and emits the hierarchy. Here the caller supplies the already-materialized
/// graph, so the walk is over workspace identities rather than over `IUnityObjectBase`s.
///
/// The reference reaches a root through `IGameObject.GetRoot()` - following `Father_C4P` to the
/// top - and deduplicates by the editor extensions it exported, so a second asset belonging to an
/// already-exported hierarchy contributes nothing. This reproduces both: only transforms with no
/// father start a walk, and each is visited once.
///
/// `is_scene` is `BuildParameters.IsScene`, which decides whether a root node carries its local
/// transform.
///
/// A GameObject with no name contributes an empty node name, which the reference also produces:
/// `NodeBuilder(gameObject.Name)` with an unset name writes an empty string.
pub fn build_scene_plan<G: SceneCollectionGraph>(
    collection: &G,
    is_scene: bool,
) -> Result<GlbLevelPlan, GlbError> {
    build_scene_plan_with_limits(collection, is_scene, GlbLevelLimits::default())
}

/// `build_scene_plan` with an explicit bound on emitted nodes.
pub fn build_scene_plan_with_limits<G: SceneCollectionGraph>(
    collection: &G,
    is_scene: bool,
    limits: GlbLevelLimits,
) -> Result<GlbLevelPlan, GlbError> {
    // Each Transform is visited once, matching the reference's `exportedAssets` set.
    let mut visited = vec![false; collection.len()];
    let mut plan = GlbLevelPlan::default();
    for index in 0..collection.len() {
        if visited[index] || !is_hierarchy_root(collection, index) {
            continue;
        }
        visit_transform(
            collection,
            index,
            None,
            is_scene,
            limits,
            &mut visited,
            &mut plan,
        )?;
    }
    Ok(plan)
}

/// `GameObjectExtensions.GetRoot` decides whether walking up from this Transform stops here.
///
/// The source climbs while `parent.Father_C4P` is non-null **and** `parent.GameObject_C4P` is
/// non-null, and returns the last Transform that satisfied that. So a Transform whose father has
/// no GameObject is itself a hierarchy root: the climb stops one step below it even though the
/// father pointer still names a Transform. Treating such a Transform as an ordinary child instead
/// would leave its own subtree unreachable, because the null-GameObject father emits nothing and
/// never descends.
fn is_hierarchy_root<G: SceneCollectionGraph>(collection: &G, index: usize) -> bool {
    let mut current = index;
    // The climb is over untrusted input, so it is bounded rather than trusted to terminate: a
    // father cycle that never reaches a root must not hang the walk.
    for _ in 0..collection.len() {
        let Some(father) = collection.father(current) else {
            return true;
        };
        if father == current {
            // A self-edge cannot be climbed past; treat it as a root rather than looping.
            return true;
        }
        if !collection.has_game_object(father) {
            // The climb stops at `current`, which is therefore the root when we started there.
            return current == index;
        }
        current = father;
    }
    // A cycle with no root: report the starting Transform as a root so it is still visited once.
    current == index
}

/// The read-only view of a materialized scene collection this walk needs.
///
/// A trait keeps the walk independent of the export crate's concrete types, so it can be tested
/// against the reference fixture without building a whole scene.
pub trait SceneCollectionGraph {
    /// The number of Transforms in the collection.
    fn len(&self) -> usize;

    /// Whether the collection holds no Transforms.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The GameObject name for a Transform, or `None` when it has no GameObject.
    fn name(&self, index: usize) -> Option<&str>;

    /// The father Transform index, or `None` when this is a root.
    fn father(&self, index: usize) -> Option<usize>;

    /// The child Transform indices, in source order.
    fn children(&self, index: usize) -> &[usize];

    /// The local transform values, or `None` when the Transform carries no resolved values.
    fn local(&self, index: usize) -> Option<GlbLevelLocalTransform>;

    /// The geometry this Transform's GameObject asks for, or `None` when it carries none.
    fn mesh(&self, index: usize) -> Option<GlbLevelAttachedMesh>;

    /// Whether the Transform has a GameObject at all.
    ///
    /// The reference returns early when `transform.GameObject_C4P` is null, so such a Transform
    /// emits no node - but it still counts as visited.
    fn has_game_object(&self, index: usize) -> bool;
}

#[allow(clippy::too_many_arguments)]
fn visit_transform<G: SceneCollectionGraph>(
    collection: &G,
    index: usize,
    parent: Option<usize>,
    is_scene: bool,
    limits: GlbLevelLimits,
    visited: &mut [bool],
    plan: &mut GlbLevelPlan,
) -> Result<(), GlbError> {
    if visited.get(index).copied().unwrap_or(true) {
        return Ok(());
    }
    visited[index] = true;
    if !collection.has_game_object(index) {
        // `AddGameObjectToScene` returns before emitting anything when the GameObject is null.
        return Ok(());
    }
    if plan.nodes.len() >= limits.max_nodes {
        return Err(GlbError::LevelNodeLimitExceeded {
            count: plan.nodes.len().saturating_add(1),
            limit: limits.max_nodes,
        });
    }

    // A Transform's node carries `transform.GameObject_C4P.Name`, which may legitimately be empty.
    let name = collection.name(index).unwrap_or("").to_owned();
    // The source writes the local transform only for a child of something, or for a scene build.
    let local_transform = (parent.is_some() || is_scene)
        .then(|| collection.local(index))
        .flatten();
    let node_index = plan.nodes.len();
    plan.nodes.push(GlbLevelNode {
        name,
        parent,
        local_transform,
        mesh: collection.mesh(index),
    });

    for child in collection.children(index) {
        visit_transform(
            collection,
            *child,
            Some(node_index),
            is_scene,
            limits,
            visited,
            plan,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test graph over the fixture hierarchy, so the scene adapter is checked against the same
    /// reference rows as the direct plan path.
    struct FixtureGraph {
        names: Vec<&'static str>,
        fathers: Vec<Option<usize>>,
        children: Vec<Vec<usize>>,
        locals: Vec<Option<GlbLevelLocalTransform>>,
        has_go: Vec<bool>,
    }

    impl FixtureGraph {
        /// Root(0) -> Child(1) -> Grandchild(2), plus parentless Second(3).
        fn new() -> Self {
            Self {
                names: vec!["Root", "Child", "Grandchild", "Second"],
                fathers: vec![None, Some(0), Some(1), None],
                children: vec![vec![1], vec![2], Vec::new(), Vec::new()],
                locals: vec![None; 4],
                has_go: vec![true; 4],
            }
        }

        /// The reference's own emitted transforms for the scene run.
        fn with_reference_locals() -> Self {
            let mut graph = Self::new();
            graph.locals = [
                (
                    [-1.0, 2.0, 3.0],
                    [0.0, -0.707_106_77, -0.0, 0.707_106_77],
                    [2.0, 2.0, 2.0],
                ),
                (
                    [-4.0, 5.0, 6.0],
                    [0.5, -0.0, -0.0, 0.866_025_4],
                    [1.0, 1.0, 1.0],
                ),
                (
                    [-7.0, 8.0, 9.0],
                    [0.0, -0.0, -0.707_106_77, 0.707_106_77],
                    [1.0, 1.0, 1.0],
                ),
                ([1.0, -2.0, -3.0], [0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 1.0]),
            ]
            .into_iter()
            .map(|(translation, rotation, scale)| {
                Some(GlbLevelLocalTransform {
                    scale,
                    rotation,
                    translation,
                })
            })
            .collect();
            graph
        }
    }

    impl SceneCollectionGraph for FixtureGraph {
        fn len(&self) -> usize {
            self.names.len()
        }
        fn name(&self, index: usize) -> Option<&str> {
            self.names.get(index).copied()
        }
        fn father(&self, index: usize) -> Option<usize> {
            self.fathers.get(index).copied().flatten()
        }
        fn children(&self, index: usize) -> &[usize] {
            self.children.get(index).map_or(&[], Vec::as_slice)
        }
        fn local(&self, index: usize) -> Option<GlbLevelLocalTransform> {
            self.locals.get(index).copied().flatten()
        }
        fn mesh(&self, _index: usize) -> Option<GlbLevelAttachedMesh> {
            None
        }
        fn has_game_object(&self, index: usize) -> bool {
            self.has_go.get(index).copied().unwrap_or(false)
        }
    }

    /// The adapter walks the hierarchy the reference rows describe.
    #[test]
    fn scene_plan_adapter_walks_roots_in_source_order() {
        let graph = FixtureGraph::with_reference_locals();
        let plan = build_scene_plan(&graph, true).expect("the plan builds");
        let names: Vec<&str> = plan.nodes.iter().map(|node| node.name.as_str()).collect();
        // Depth-first in source order, exactly as the reference emitted.
        assert_eq!(names, ["Root", "Child", "Grandchild", "Second"]);
        assert_eq!(plan.nodes[0].parent, None);
        assert_eq!(plan.nodes[1].parent, Some(0));
        assert_eq!(plan.nodes[2].parent, Some(1));
        assert_eq!(plan.nodes[3].parent, None);
        // The scene rows from the fixture, reproduced through the adapter.
        let root = plan.nodes[0]
            .local_transform
            .expect("a scene root is transformed");
        assert_eq!(root.translation, [-1.0, 2.0, 3.0]);
        assert_eq!(root.scale, [2.0, 2.0, 2.0]);
        assert_eq!(root.rotation, [0.0, -0.707_106_77, -0.0, 0.707_106_77]);

        // An asset build leaves roots untransformed, even with the same graph.
        let asset = build_scene_plan(&graph, false).expect("the plan builds");
        assert!(
            asset.nodes[0].local_transform.is_none(),
            "an asset root is untransformed"
        );
        assert!(
            asset.nodes[1].local_transform.is_some(),
            "a child is transformed in both modes"
        );

        // A graph that resolved no values produces nodes with no transforms at all.
        let bare = build_scene_plan(&FixtureGraph::new(), true).expect("the plan builds");
        assert!(bare.nodes.iter().all(|node| node.local_transform.is_none()));
    }

    /// A Transform whose father has no GameObject is itself a hierarchy root.
    ///
    /// `GameObjectExtensions.GetRoot` climbs while the parent has a GameObject, so the climb stops
    /// one step **below** a GameObject-less father. Child's GameObject is null here, which makes
    /// Grandchild its own root - and Child itself emits nothing, because `AddGameObjectToScene`
    /// returns before its child loop. Getting this wrong leaves Grandchild's subtree unreachable,
    /// since the GameObject-less father never descends.
    #[test]
    fn scene_plan_adapter_treats_a_child_of_a_gameobject_less_father_as_a_root() {
        let mut graph = FixtureGraph::new();
        graph.has_go = vec![true, false, true, true];
        let plan = build_scene_plan(&graph, true).expect("the plan builds");
        let names: Vec<&str> = plan.nodes.iter().map(|node| node.name.as_str()).collect();
        // Child emits nothing and does not descend, but Grandchild is its own root.
        assert_eq!(names, ["Root", "Grandchild", "Second"]);
        assert_eq!(plan.nodes[0].parent, None);
        assert_eq!(plan.nodes[1].parent, None);
        assert_eq!(plan.nodes[2].parent, None);
    }

    /// The adapter's node bound is enforced like the direct path's.
    #[test]
    fn scene_plan_adapter_enforces_the_node_limit() {
        let graph = FixtureGraph::new();
        let error = build_scene_plan_with_limits(&graph, true, GlbLevelLimits { max_nodes: 2 })
            .expect_err("the limit is enforced");
        assert_eq!(
            error,
            GlbError::LevelNodeLimitExceeded { count: 3, limit: 2 }
        );
    }

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
                        "node {} parent",
                        fields[2]
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
                    let local = node.local_transform.unwrap_or(GlbLevelLocalTransform {
                        scale: [1.0, 1.0, 1.0],
                        rotation: [0.0, 0.0, 0.0, 1.0],
                        translation: [0.0, 0.0, 0.0],
                    });
                    assert_eq!(
                        local.translation.as_slice(),
                        expected_translation.as_slice(),
                        "node {} translation",
                        fields[2]
                    );
                    assert_eq!(
                        local.rotation.as_slice(),
                        expected_rotation.as_slice(),
                        "node {} rotation",
                        fields[2]
                    );
                    assert_eq!(
                        local.scale.as_slice(),
                        expected_scale.as_slice(),
                        "node {} scale",
                        fields[2]
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

    /// The emitted document reproduces the reference's node graph.
    ///
    /// Same fixture as the plan test, but checked on the serialized glTF rather than the plan: the
    /// node names and order, the parent/child edges through `children`, the scene roots, and the
    /// presence or absence of a local transform per node.
    #[test]
    fn scene_document_reproduces_the_reference_node_graph() {
        fn leaf(name: &str, position: [f32; 3]) -> GlbLevelTransform {
            GlbLevelTransform {
                game_object_name: name.to_owned(),
                local_position: position,
                local_rotation: [0.0, 0.0, 0.0, 1.0],
                local_scale: [1.0, 1.0, 1.0],
                children: Vec::new(),
                mesh: None,
            }
        }
        let plan = build_level_plan(
            &[
                GlbLevelTransform {
                    game_object_name: "Root".to_owned(),
                    local_position: [1.0, 2.0, 3.0],
                    local_rotation: [0.0, 0.707_106_77, 0.0, 0.707_106_77],
                    local_scale: [2.0, 2.0, 2.0],
                    children: vec![leaf("Child", [4.0, 5.0, 6.0])],
                    mesh: None,
                },
                leaf("Second", [-1.0, -2.0, -3.0]),
            ],
            true,
        )
        .expect("the plan builds");
        let document = build_scene_document(&plan);
        let bytes = document.into_glb();

        // A well-formed GLB header, as the single-mesh export also produces.
        assert_eq!(&bytes[0..4], b"glTF");
        assert_eq!(
            u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            2
        );
        let declared = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
        assert_eq!(
            declared,
            bytes.len(),
            "the declared length matches the body"
        );
        let text = json_chunk(&bytes);
        let text = text.as_str();

        // The reference emitted Root, Child, Grandchild, Second; this fixture has Root, Child,
        // Second, and the names appear in that order.
        let root = text.find("\"name\":\"Root\"").expect("Root is emitted");
        let child = text.find("\"name\":\"Child\"").expect("Child is emitted");
        let second = text.find("\"name\":\"Second\"").expect("Second is emitted");
        assert!(
            root < child && child < second,
            "emission order is source order"
        );

        // Both parentless nodes are scene roots.
        assert!(text.contains("\"scenes\":[{\"nodes\":[0,2]}]"), "{text}");
        // Root is index 0 and Child index 1, so Root lists Child.
        assert!(text.contains("\"children\":[1]"), "{text}");
        // A scene build transforms its roots, so Root carries the converted transform:
        // position (1,2,3) -> (-1,2,3), and the +90 degree Y rotation -> -0.70710677 about Y.
        assert!(
            text.contains("\"translation\":[-1.0,2.0,3.0]"),
            "the root is converted: {text}"
        );
        assert!(
            text.contains("\"rotation\":[0.0,-0.70710677,-0.0,0.70710677]"),
            "the axis flip reaches the document: {text}"
        );
        // Scale is untouched.
        assert!(text.contains("\"scale\":[2.0,2.0,2.0]"), "{text}");
    }

    /// An asset-shaped plan leaves its roots untransformed in the document too.
    #[test]
    fn asset_document_leaves_roots_untransformed() {
        let root = GlbLevelTransform {
            game_object_name: "Root".to_owned(),
            local_position: [1.0, 2.0, 3.0],
            local_rotation: [0.0, 0.0, 0.0, 1.0],
            local_scale: [2.0, 2.0, 2.0],
            children: vec![GlbLevelTransform {
                game_object_name: "Child".to_owned(),
                local_position: [4.0, 5.0, 6.0],
                local_rotation: [0.0, 0.0, 0.0, 1.0],
                local_scale: [1.0, 1.0, 1.0],
                children: Vec::new(),
                mesh: None,
            }],
            mesh: None,
        };
        let plan = build_level_plan(&[root], false).expect("the plan builds");
        let bytes = build_scene_document(&plan).into_glb();
        let text = json_chunk(&bytes);
        // Root is the first node and has no transform; Child does have one.
        let root_node = text
            .split("\"nodes\":[")
            .nth(1)
            .and_then(|rest| rest.split("},").next())
            .expect("the nodes array is emitted");
        assert!(
            !root_node.contains("translation"),
            "an asset root is untransformed: {root_node}"
        );
        assert!(
            text.contains("\"translation\":[-4.0,5.0,6.0]"),
            "the child is converted: {text}"
        );
    }

    /// The JSON chunk of a GLB, which is the part a test compares against text.
    ///
    /// The file is a binary container: after the 12-byte header comes the JSON chunk header, then
    /// the JSON, then the binary chunk, so the whole buffer is not valid UTF-8.
    fn json_chunk(bytes: &[u8]) -> String {
        let json_length = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
        String::from_utf8(bytes[20..20 + json_length].to_vec()).expect("the JSON chunk is UTF-8")
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
            source: None,
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
