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

use crate::export::scene_wiring::{
    MaterializedComponentSlot, MaterializedComponentTarget, MaterializedSceneCollection,
};
use crate::processing::scene::{SceneRelationship, SceneRelationshipTarget, SceneTransformValues};
use crate::resources::animation::converter::RENDERER_CLASS_IDS;
use crate::resources::glb::{
    attach_plan_geometry, GlbBuildLimits, GlbError, GlbLevelLimits, GlbMeshExport,
};
use crate::resources::glb_level::{
    build_scene_document, build_scene_plan_with_limits, GlbLevelAttachedMesh,
    GlbLevelLocalTransform, GlbLevelMesh, GlbLevelPlan, GlbSourceMesh, SceneCollectionGraph,
};
use crate::resources::mesh::MeshView;
use crate::resources::packed_bit_vector::PackedBitVectorLimits;
use crate::resources::sub_mesh_data::IndexFormat;
use crate::resources::vertex_data_blob::{EndianType, UnityVersion};
use crate::serialized_file::object::SerializedValue;
use crate::serialized_file::object_workspace::ObjectWorkspace;
use crate::serialized_file::workspace::LoadedSerializedFile;

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
    /// The collection carries geometry the plan did not resolve, so the GLB would have no meshes.
    ///
    /// **This is a refusal, not a warning.** [`MaterializedSceneGraphView::mesh`] answers `None` for
    /// every Transform because the MeshFilter's mesh reference is not materialized yet, so a plan
    /// built from a real collection has **no mesh nodes at all** and the export would write a file
    /// with a node graph and no geometry - which looks like a successful scene export and is not one.
    /// Refusing turns that into a reported failure. The check is a `MeshFilter` component on any
    /// GameObject: a GameObject with no MeshFilter has no geometry to lose.
    GeometryNotResolved {
        /// How many GameObjects carry a `MeshFilter`.
        mesh_filter_count: usize,
    },
}

impl core::fmt::Display for SceneGlbExportError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Level(error) => write!(formatter, "scene hierarchy walk failed: {error}"),
            Self::Glb(error) => write!(formatter, "scene GLB encoding failed: {error}"),
            Self::NoNodes => write!(formatter, "the scene hierarchy has no nodes to export"),
            Self::GeometryNotResolved { mesh_filter_count } => write!(
                formatter,
                "{mesh_filter_count} GameObjects carry a MeshFilter but the plan resolved no mesh, \
                 so the export would carry a node graph with no geometry"
            ),
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
    /// The mesh and renderer facts the caller resolved, indexed by Transform position.
    ///
    /// The materialized collection records each GameObject's component slots but not the fields of
    /// those components, so the MeshFilter's mesh reference and the Renderer's batching fields are
    /// read by the caller and handed in here. The geometry decision itself stays in
    /// `glb_level::attached_mesh_for`, so this view does not restate it. An empty slice means the
    /// caller resolved nothing, which `export_scene_glb` refuses for a collection with MeshFilters.
    attached_meshes: &'a [Option<GlbLevelMesh>],
}

impl<'a> MaterializedSceneGraphView<'a> {
    /// Index a materialized collection by Transform position, with caller-resolved geometry.
    pub fn new(
        collection: &'a MaterializedSceneCollection,
        attached_meshes: &'a [Option<GlbLevelMesh>],
    ) -> Self {
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
            attached_meshes,
        }
    }

    /// Selects the transform at `index`, narrowed to the two value shapes the walk reads.
    fn values(&self, index: usize) -> Option<&SceneTransformValues> {
        self.collection.transforms.get(index)?.values.as_ref()
    }

    /// How many GameObjects in this collection carry a `MeshFilter` (class 33).
    ///
    /// A `MeshFilter` is what gives a GameObject geometry, so this is the count of GameObjects that
    /// would lose geometry if the mesh reference were not resolved. It is a property of the
    /// materialized collection, which is why the check lives here rather than in the walk.
    fn mesh_filter_count(&self) -> usize {
        self.collection
            .game_objects
            .iter()
            .filter(|object| {
                object
                    .components
                    .iter()
                    .any(|slot| slot.class_id == Some(MESH_FILTER_CLASS_ID))
            })
            .count()
    }
}

/// Class-33 `MeshFilter`, the component that attaches a mesh to a GameObject.
const MESH_FILTER_CLASS_ID: i32 = 33;

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

    fn mesh(&self, index: usize) -> Option<GlbLevelAttachedMesh> {
        // Geometry selection depends on the MeshFilter's and Renderer's fields, which the
        // materialized collection does not carry, so the caller resolves them per Transform and
        // supplies them here. The dynamic/static decision is the walk's, not the caller's.
        crate::resources::glb_level::attached_mesh_for(self.attached_meshes.get(index)?.as_ref()?)
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

/// What resolving a collection's per-Transform geometry needs.
pub struct SceneMeshResolution<'a> {
    /// The decoded object payloads.
    pub objects: &'a ObjectWorkspace,
    /// The workspace files, for each file's Unity version and endianness.
    pub files: &'a [LoadedSerializedFile],
    /// The packed-vector decode bounds the mesh reader uses.
    pub limits: PackedBitVectorLimits,
}

/// Resolve each Transform's mesh and renderer facts from the workspace.
///
/// This is the caller's half of `GlbLevelBuilder.AddGameObjectToScene`'s component gates: a
/// `MeshFilter` whose `m_Mesh` resolves to a mesh that `IsSet` and yields `MeshData`, plus a
/// `Renderer` for geometry to be attached at all. The materialized collection records the component
/// slots but not their fields, which is why the payloads are read here rather than in the scene pass.
///
/// **A mesh referenced from another file is not resolved and yields no facts.** The reference reaches
/// such a mesh through the bundle; this path does not, and returning `None` keeps the node without
/// geometry instead of inventing one. That is a caller-visible gap rather than a silent one.
pub fn resolve_scene_meshes(
    collection: &MaterializedSceneCollection,
    context: &SceneMeshResolution<'_>,
) -> ResolvedSceneMeshes {
    let mut attached = Vec::with_capacity(collection.transforms.len());
    let mut meshes: Vec<(GlbSourceMesh, crate::resources::mesh_data::MeshData)> = Vec::new();
    for transform in &collection.transforms {
        let resolved = resolve_one(collection, transform, context);
        // The reference caches one `MeshData` per `IMesh`, so two nodes sharing a mesh share the
        // entry rather than each carrying a copy.
        if let Some((facts, mesh_data)) = &resolved {
            if let Some(source) = facts.source {
                if !meshes.iter().any(|(candidate, _)| *candidate == source) {
                    meshes.push((source, mesh_data.clone()));
                }
            }
        }
        attached.push(resolved.map(|(facts, _)| facts));
    }
    ResolvedSceneMeshes { attached, meshes }
}

/// The per-Transform facts and the per-mesh geometry one collection resolves to.
pub struct ResolvedSceneMeshes {
    /// The facts the walk needs, indexed by Transform position.
    pub attached: Vec<Option<GlbLevelMesh>>,
    /// The decoded geometry, one entry per distinct source mesh.
    pub meshes: Vec<(GlbSourceMesh, crate::resources::mesh_data::MeshData)>,
}

/// One Transform's facts, or `None` when the source's gates do not attach geometry.
fn resolve_one(
    collection: &MaterializedSceneCollection,
    transform: &crate::export::scene_wiring::MaterializedTransform,
    context: &SceneMeshResolution<'_>,
) -> Option<(GlbLevelMesh, crate::resources::mesh_data::MeshData)> {
    let game_object_index = local_object_index(transform.game_object.as_ref()?)?;
    let game_object = collection
        .game_objects
        .iter()
        .find(|object| object.identity.object_index == game_object_index)?;

    // The MeshFilter's mesh, resolved through its own payload.
    let mesh_filter = game_object
        .components
        .iter()
        .find(|slot| slot.class_id == Some(MESH_FILTER_CLASS_ID))?;
    let mesh_filter_index = component_object_index(mesh_filter)?;
    let mesh_filter_root = payload(context, collection.file_index, mesh_filter_index)?;
    let (file_id, path_id) = pptr_field(mesh_filter_root, "m_Mesh")?;
    if file_id != 0 {
        // Another file's mesh needs the bundle, which this path does not walk.
        return None;
    }
    let mesh_index = find_object_by_path_id(context, collection.file_index, path_id)?;
    let mesh_root = payload(context, collection.file_index, mesh_index)?;
    let file = context.files.get(collection.file_index)?;
    let version = file
        .metadata
        .prefix
        .unity_version_text
        .as_deref()
        .and_then(UnityVersion::parse)?;
    let endian = if file.metadata.prefix.body_endian == crate::io::binary::Endian::Little {
        EndianType::Little
    } else {
        EndianType::Big
    };
    // The Renderer decides whether geometry is attached at all, and how it is batched.
    let renderer = game_object.components.iter().find(|slot| {
        slot.class_id
            .is_some_and(|class_id| RENDERER_CLASS_IDS.contains(&class_id))
    })?;
    let renderer_root = component_object_index(renderer)
        .and_then(|index| payload(context, collection.file_index, index));

    attached_mesh_facts(
        mesh_root,
        renderer_root,
        version,
        endian,
        context.limits,
        GlbSourceMesh { file_id, path_id },
    )
}

/// The mesh and renderer facts of one GameObject, from the two payloads.
///
/// Split from the workspace lookup so the gates are testable from synthetic payloads: the mesh must
/// `IsSet` and yield `MeshData` **before** the Renderer is consulted, because the source's `&&`
/// chain checks the mesh first and a GameObject whose mesh is unset attaches nothing even when a
/// Renderer is present.
pub fn attached_mesh_facts(
    mesh_root: &SerializedValue,
    renderer_root: Option<&SerializedValue>,
    version: UnityVersion,
    endian: EndianType,
    limits: PackedBitVectorLimits,
    source: GlbSourceMesh,
) -> Option<(GlbLevelMesh, crate::resources::mesh_data::MeshData)> {
    let view = MeshView::from_value(mesh_root, version, endian).ok()?;
    if !crate::resources::mesh::is_set(
        view.compressed_mesh
            .as_ref()
            .is_some_and(|mesh| mesh.is_set()),
        view.vertex_data
            .as_ref()
            .map_or(0, |data| data.vertex_count),
        view.vertex_data.as_ref().map_or(0, |data| data.data.len()),
        view.stream_data.as_ref().map(|info| info.is_set()),
    ) {
        return None;
    }
    let mesh_data = view.read_mesh_data(limits).ok()??;
    // A Renderer is required for geometry to be attached at all.
    let renderer_root = renderer_root?;

    let (has_static_batch_info, static_batch_sub_mesh_count, static_batch_info) =
        match field(renderer_root, "m_StaticBatchInfo") {
            Some(value) => {
                let first = field(value, "firstSubMesh")
                    .and_then(int_field)
                    .unwrap_or(0);
                let count = field(value, "subMeshCount")
                    .and_then(int_field)
                    .unwrap_or(0);
                let count_u32 = u32::try_from(count).unwrap_or(0);
                let first_u32 = u32::try_from(first).unwrap_or(0);
                (true, count_u32, Some((first_u32, count_u32)))
            }
            None => (false, 0, None),
        };
    let subset_indices = field(renderer_root, "m_SubsetIndices").and_then(|value| match value {
        SerializedValue::Array(items) => Some(
            items
                .iter()
                .filter_map(int_field)
                .filter_map(|value| u32::try_from(value).ok())
                .collect::<Vec<u32>>(),
        ),
        _ => None,
    });

    let facts = GlbLevelMesh {
        sub_mesh_count: mesh_data.sub_meshes.len(),
        is_16_bit_indices: mesh_data.index_format() == IndexFormat::UInt16,
        mesh_type: Some(crate::resources::glb::get_mesh_type(&mesh_data)),
        has_renderer: true,
        has_static_batch_info,
        static_batch_sub_mesh_count,
        has_subset_indices: subset_indices.is_some(),
        subset_indices_count: subset_indices.as_ref().map_or(0, Vec::len),
        subset_indices,
        static_batch_info,
        source: Some(source),
    };
    Some((facts, mesh_data))
}

/// The local object index a Transform's GameObject relationship resolves to.
fn local_object_index(relationship: &SceneRelationship) -> Option<usize> {
    match &relationship.target {
        SceneRelationshipTarget::LocalObject { object_index } => Some(*object_index),
        _ => None,
    }
}

/// The local object index a component slot names.
fn component_object_index(slot: &MaterializedComponentSlot) -> Option<usize> {
    match &slot.target {
        MaterializedComponentTarget::Source { target, .. } => match target {
            SceneRelationshipTarget::LocalObject { object_index } => Some(*object_index),
            _ => None,
        },
        MaterializedComponentTarget::Generated { .. } => None,
    }
}

/// A decoded object's payload root.
fn payload<'a>(
    context: &'a SceneMeshResolution<'a>,
    file_index: usize,
    object_index: usize,
) -> Option<&'a SerializedValue> {
    context
        .objects
        .get(file_index, object_index)
        .and_then(|object| object.payload.as_ref().ok())
        .map(|payload| &payload.root)
}

/// The object table index of the object with `path_id` in `file_index`.
fn find_object_by_path_id(
    context: &SceneMeshResolution<'_>,
    file_index: usize,
    path_id: i64,
) -> Option<usize> {
    context
        .objects
        .objects_for_file(file_index)
        .iter()
        .find(|object| object.path_id == path_id)
        .map(|object| object.object_index)
}

/// A uniquely named field of a decoded payload.
fn field<'a>(root: &'a SerializedValue, name: &str) -> Option<&'a SerializedValue> {
    root.unique_field_in_base_chain(name).ok().flatten()
}

/// A decoded `PPtr` field's `(file_id, path_id)`.
fn pptr_field(root: &SerializedValue, name: &str) -> Option<(i32, i64)> {
    match field(root, name)? {
        SerializedValue::PPtr { file_id, path_id } => Some((*file_id, *path_id)),
        _ => None,
    }
}

/// A decoded integer field as `i64`.
fn int_field(value: &SerializedValue) -> Option<i64> {
    match value {
        SerializedValue::I8(value) => Some(i64::from(*value)),
        SerializedValue::U8(value) => Some(i64::from(*value)),
        SerializedValue::I16(value) => Some(i64::from(*value)),
        SerializedValue::U16(value) => Some(i64::from(*value)),
        SerializedValue::I32(value) => Some(i64::from(*value)),
        SerializedValue::U32(value) => Some(i64::from(*value)),
        SerializedValue::I64(value) => Some(*value),
        SerializedValue::U64(value) => i64::try_from(*value).ok(),
        _ => None,
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
    /// The decoded geometry, keyed by the mesh object each entry came from.
    ///
    /// The reference builds these on demand and caches them **per `IMesh`**, so two nodes sharing a
    /// mesh share the data. Keying by the source mesh rather than by node position is what makes that
    /// cache expressible: a node asks for the mesh it came from, and a node whose mesh was not
    /// decoded is skipped with a warning rather than silently given another node's geometry.
    pub meshes: &'a [(GlbSourceMesh, crate::resources::mesh_data::MeshData)],
    /// The mesh and renderer facts the caller resolved, indexed by Transform position.
    ///
    /// Empty means nothing was resolved; a collection whose GameObjects carry a `MeshFilter` is then
    /// refused rather than exported with no geometry.
    pub attached_meshes: &'a [Option<GlbLevelMesh>],
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
    let view = MaterializedSceneGraphView::new(input.collection, input.attached_meshes);
    let plan = build_scene_plan_with_limits(&view, input.is_scene, input.level_limits)?;
    if plan.nodes.is_empty() {
        return Err(SceneGlbExportError::NoNodes);
    }
    // A plan with no mesh nodes for a collection that has MeshFilters is a geometry-less export,
    // not a scene with nothing in it. The caller resolves each Transform's geometry and supplies it
    // through `attached_meshes`; supplying none is the state the gap takes, and refusing keeps it
    // from being written as a successful scene.
    if plan.nodes.iter().all(|node| node.mesh.is_none()) {
        let mesh_filter_count = view.mesh_filter_count();
        if mesh_filter_count > 0 {
            return Err(SceneGlbExportError::GeometryNotResolved { mesh_filter_count });
        }
    }
    export_plan_as_glb(&plan, input.meshes, input.glb_limits)
}

/// Encode an already-built plan as a GLB.
///
/// Split out from [`export_scene_glb`] so a caller that built its plan elsewhere - for instance
/// from a prefab hierarchy - can use the same encoder.
pub fn export_plan_as_glb(
    plan: &GlbLevelPlan,
    meshes: &[(GlbSourceMesh, crate::resources::mesh_data::MeshData)],
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
