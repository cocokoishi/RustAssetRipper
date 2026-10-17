//! `GlbModelExporter`: the scene-level GLB export entry.
//!
//! The reference's `AssetRipper.Export.PrimaryContent/Models/GlbModelExporter.cs` is small but
//! load-bearing. `TryCreateCollection` switches on `asset.MainAsset` and accepts a
//! `SceneHierarchyObject` or a `PrefabHierarchyObject`; `ExportModel` then calls
//! `GlbLevelBuilder.Build(assets, isScene)` - `isScene` true for the scene case, false for the
//! prefab case - and writes the result through `GlbWriter.TryWrite`, reporting a write failure
//! through the logger rather than throwing.
//!
//! This module supplies the port's input adapter: it presents one [`MaterializedSceneCollection`]
//! as a [`SceneCollectionGraph`], which is what `build_scene_plan` walks, and then composes the
//! three already-verified stages into a GLB:
//!
//! ```text
//! MaterializedSceneCollection -> build_scene_plan -> build_scene_document
//!                             -> attach_plan_geometry -> SceneDocument::into_glb
//! ```
//!
//! **What is deliberately not here.** Attaching geometry needs each GameObject's decoded
//! `MeshData`, which the caller holds: the reference reaches it through
//! `BuildParameters.TryGetOrMakeMeshData`, which builds a `MeshData` from an `IMesh` and caches it.
//! That cache and the `MeshData` construction already live in this workspace's mesh pipeline, so
//! the caller passes the per-node meshes in rather than this module rebuilding them. A node whose
//! mesh could not be built is skipped with a warning, exactly as the reference skips one whose
//! `TryGetOrMakeMeshData` returns false.

use crate::export::scene_wiring::MaterializedSceneCollection;
use crate::processing::scene::{SceneRelationshipTarget, SceneTransformValues};
use crate::resources::glb::{
    attach_plan_geometry, GlbBuildLimits, GlbError, GlbLevelLimits, GlbMeshExport,
};
use crate::resources::glb_level::{
    build_scene_document, build_scene_plan_with_limits, GlbLevelAttachedMesh,
    GlbLevelLocalTransform, GlbLevelPlan, SceneCollectionGraph,
};

/// Why a scene-level GLB export could not be produced.
#[derive(Clone, Debug, PartialEq)]
pub enum SceneGlbExportError {
    /// The hierarchy walk failed.
    Level(GlbError),
    /// Geometry could not be encoded.
    Glb(GlbError),
    /// The plan produced no nodes, so there is nothing to export.
    ///
    /// The reference writes an empty `SceneBuilder` in this case; the port reports it instead so a
    /// caller can distinguish "no geometry" from "an empty file was written".
    NoNodes,
}

impl core::fmt::Display for SceneGlbExportError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Level(error) => write!(formatter, "scene hierarchy walk failed: {error}"),
            Self::Glb(error) => write!(formatter, "scene GLB encoding failed: {error}"),
            Self::NoNodes => write!(formatter, "the scene hierarchy has no nodes to export"),
        }
    }
}

impl std::error::Error for SceneGlbExportError {}

impl From<GlbError> for SceneGlbExportError {
    fn from(error: GlbError) -> Self {
        Self::Level(error)
    }
}

/// The GLB bytes plus the audit trail a scene export produces.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneGlbExport {
    /// The complete GLB file bytes.
    pub bytes: Vec<u8>,
    /// Non-fatal observations, in a deterministic order.
    pub warnings: Vec<String>,
    /// The number of nodes the plan emitted.
    pub node_count: usize,
    /// The number of nodes that contributed geometry.
    pub mesh_count: usize,
}

/// A materialized scene collection presented as a [`SceneCollectionGraph`].
///
/// A child/father edge in the materialized graph is a resolved `SceneRelationship`; the walk
/// needs indexes into the collection's `transforms`, so the relationships are turned into indexes
/// once here instead of on every visit.
pub struct MaterializedSceneGraphView<'a> {
    collection: &'a MaterializedSceneCollection,
    /// For each Transform, the index of its father Transform when one resolves locally.
    fathers: Vec<Option<usize>>,
    /// For each Transform, the indexes of its children in source order.
    children: Vec<Vec<usize>>,
}

impl<'a> MaterializedSceneGraphView<'a> {
    /// Index a materialized collection by Transform position.
    pub fn new(collection: &'a MaterializedSceneCollection) -> Self {
        // A relationship resolves to a local object index; the Transform that owns that object is
        // the edge target.
        let target_index = |relationship: &crate::processing::scene::SceneRelationship| {
            match &relationship.target {
                SceneRelationshipTarget::LocalObject { object_index } => collection
                    .transforms
                    .iter()
                    .position(|transform| transform.identity.object_index == *object_index),
                // A cross-file or unresolved father is not a node in this collection's walk; the
                // reference's `Father_C4P` would resolve through the bundle, so such an edge
                // cannot be represented here and is reported by the caller's issue list instead.
                _ => None,
            }
        };

        let mut fathers = Vec::with_capacity(collection.transforms.len());
        let mut children = vec![Vec::new(); collection.transforms.len()];
        for (position, transform) in collection.transforms.iter().enumerate() {
            let father = transform.father.as_ref().and_then(target_index);
            // A self-edge would make the walk non-terminating; the visit set stops it either way,
            // but the edge is not a hierarchy edge and is dropped.
            let father = father.filter(|index| *index != position);
            fathers.push(father);
        }
        for (position, father) in fathers.iter().enumerate() {
            if let Some(father) = father {
                children[*father].push(position);
            }
        }
        Self {
            collection,
            fathers,
            children,
        }
    }

    /// Selects the transform at `index`, narrowed to the two value shapes the walk reads.
    fn values(&self, index: usize) -> Option<&SceneTransformValues> {
        self.collection.transforms.get(index)?.values.as_ref()
    }
}

/// Unity's default local transform, which the reference's `Transformation` identity uses when a
/// Transform carries no serialized values.
const IDENTITY_TRANSLATION: [f32; 3] = [0.0, 0.0, 0.0];
const IDENTITY_ROTATION: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
const IDENTITY_SCALE: [f32; 3] = [1.0, 1.0, 1.0];

impl SceneCollectionGraph for MaterializedSceneGraphView<'_> {
    fn len(&self) -> usize {
        self.collection.transforms.len()
    }

    fn name(&self, index: usize) -> Option<&str> {
        let transform = self.collection.transforms.get(index)?;
        // `NodeBuilder(gameObject.Name)` reads the name off the Transform's GameObject.
        let game_object = transform.game_object.as_ref()?;
        let object_index = match &game_object.target {
            SceneRelationshipTarget::LocalObject { object_index } => *object_index,
            _ => return None,
        };
        self.collection
            .game_objects
            .iter()
            .find(|object| object.identity.object_index == object_index)
            .and_then(|object| object.name.as_deref())
    }

    fn father(&self, index: usize) -> Option<usize> {
        self.fathers.get(index).copied().flatten()
    }

    fn children(&self, index: usize) -> &[usize] {
        self.children.get(index).map_or(&[], Vec::as_slice)
    }

    fn local(&self, index: usize) -> Option<GlbLevelLocalTransform> {
        // A Transform carries no resolved component only when the workspace never decoded one for
        // it. This view still reports the identity in that case: `transform.ToTransformation()`
        // reads the generated properties, and an unset `LocalPosition_C4` is the zero vector with
        // an identity rotation and unit scale, not "no transform". A Transform the walk never
        // visits contributes nothing either way.
        self.collection.transforms.get(index)?;
        let values = self.values(index);
        let position = values
            .and_then(|values| values.local_position)
            .map_or(IDENTITY_TRANSLATION, |value| [value.x, value.y, value.z]);
        let rotation = values
            .and_then(|values| values.local_rotation)
            .map_or(IDENTITY_ROTATION, |value| {
                [value.x, value.y, value.z, value.w]
            });
        let scale = values
            .and_then(|values| values.local_scale)
            .map_or(IDENTITY_SCALE, |value| [value.x, value.y, value.z]);
        let rotated = crate::resources::glb::to_gltf_quaternion(crate::numerics::Quat {
            x: rotation[0],
            y: rotation[1],
            z: rotation[2],
            w: rotation[3],
        });
        let translated = crate::resources::glb::to_gltf_vector3(crate::numerics::Vec3 {
            x: position[0],
            y: position[1],
            z: position[2],
        });
        Some(GlbLevelLocalTransform {
            // Scaling is the same in both coordinate systems, so it is carried through unchanged.
            scale,
            rotation: [rotated.x, rotated.y, rotated.z, rotated.w],
            translation: [translated.x, translated.y, translated.z],
        })
    }

    fn mesh(&self, _index: usize) -> Option<GlbLevelAttachedMesh> {
        // Geometry selection depends on the Renderer's fields, which the caller resolves; see the
        // module note. A plan from this view therefore has no mesh selections, and the caller
        // supplies geometry through `SceneGlbExportInput::meshes` instead.
        None
    }

    fn has_game_object(&self, index: usize) -> bool {
        self.collection
            .transforms
            .get(index)
            .is_some_and(|transform| {
                transform.game_object.as_ref().is_some_and(|relationship| {
                    matches!(
                        relationship.target,
                        SceneRelationshipTarget::LocalObject { .. }
                    )
                })
            })
    }
}

/// The inputs a scene export needs beyond the collection itself.
pub struct SceneGlbExportInput<'a> {
    /// The collection to walk.
    pub collection: &'a MaterializedSceneCollection,
    /// `BuildParameters.IsScene`.
    ///
    /// **A production caller must pass `false` to match the frozen baseline.** In the source this
    /// value is dead: `GlbLevelBuilder.Build(assets, isScene)` has exactly one caller,
    /// `GlbModelExporter.ExportModel`, which has exactly one caller - the `GlbModelExporter`
    /// multi-asset `Export` - and that hard-codes `isScene: false`. Neither
    /// `GlbSceneModelExportCollection` nor `GlbPrefabModelExportCollection` overrides
    /// `ExportInner`, so a scene hierarchy reaches the same path and also gets `false`. The flag
    /// has one effect: whether a **root** node writes its local transform
    /// (`parentNode is not null || IsScene`). Keeping it as a parameter rather than hard-coding
    /// `false` preserves the reference-verified `Build` semantics on both arms, which the
    /// fixture-backed tests exercise; it does not mean the shipped product ever sets it.
    pub is_scene: bool,
    /// Per-node decoded geometry, indexed by node position in the plan.
    ///
    /// The reference builds these on demand and caches them; the caller does the same, because
    /// `MeshData` construction needs the mesh pipeline rather than the scene graph.
    pub meshes: &'a [Option<crate::resources::mesh_data::MeshData>],
    /// Bounds for the walk.
    pub level_limits: GlbLevelLimits,
    /// Bounds for the GLB container.
    pub glb_limits: GlbBuildLimits,
}

/// Export one materialized scene collection as a GLB, mirroring `GlbModelExporter.ExportModel`.
///
/// `PruneGraph`: the walk starts at fatherless Transforms, so a Transform whose father is not in
/// this collection is treated as a root. The reference reaches such a father through the bundle;
/// a collection-scoped export cannot, and the caller's issue list records it.
pub fn export_scene_glb(
    input: SceneGlbExportInput<'_>,
) -> Result<SceneGlbExport, SceneGlbExportError> {
    let view = MaterializedSceneGraphView::new(input.collection);
    let plan = build_scene_plan_with_limits(&view, input.is_scene, input.level_limits)?;
    if plan.nodes.is_empty() {
        return Err(SceneGlbExportError::NoNodes);
    }
    export_plan_as_glb(&plan, input.meshes, input.glb_limits)
}

/// Encode an already-built plan as a GLB.
///
/// Split out from [`export_scene_glb`] so a caller that built its plan elsewhere - for instance
/// from a prefab hierarchy - can use the same encoder.
pub fn export_plan_as_glb(
    plan: &GlbLevelPlan,
    meshes: &[Option<crate::resources::mesh_data::MeshData>],
    limits: GlbBuildLimits,
) -> Result<SceneGlbExport, SceneGlbExportError> {
    let mut document = build_scene_document(plan);
    let warnings = attach_plan_geometry(&mut document, plan, meshes, limits)
        .map_err(SceneGlbExportError::Glb)?;
    let mesh_count = plan.nodes.iter().filter(|node| node.mesh.is_some()).count();
    let bytes = document.into_glb();
    Ok(SceneGlbExport {
        bytes,
        warnings,
        node_count: plan.nodes.len(),
        mesh_count,
    })
}

/// The GLB bytes as a `GlbMeshExport`-shaped result, for callers that share that type.
///
/// The scene export has no single mesh, so `emitted_source_indices` is empty and the bytes are the
/// whole document.
pub fn as_mesh_export(export: SceneGlbExport) -> GlbMeshExport {
    GlbMeshExport {
        bytes: export.bytes,
        emitted_source_indices: Vec::new(),
        warnings: export.warnings,
    }
}

#[cfg(test)]
mod tests;
