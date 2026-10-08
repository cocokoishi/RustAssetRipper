//! glTF-binary (GLB) mesh export, ported from `AssetRipper.Export.Modules.Models`.
//!
//! The source builds one SharpGLTF `SceneBuilder` per `IMesh`:
//!
//! - `GlbMeshBuilder.Build` creates a `DefaultMaterial`, then for every `SubMeshes` entry calls
//!   `GlbSubMeshBuilder.BuildSubMeshes` with a single-sub-mesh segment and attaches the result to
//!   a `SubMesh_{index}` child node of a root node named after the mesh.
//! - `MeshDataExtensions.GetMeshType` selects which attributes the vertex type carries.
//! - `GlbSubMeshBuilder.BuildSubMesh` rewrites the topology: triangles and quadrangles are emitted
//!   with the vertex order flipped (glTF winds counter-clockwise), triangle strips are
//!   de-stripified with degenerate removal and alternating winding, line strips become line lists,
//!   and quads become two triangles per quad.
//! - `GlbCoordinateConversion` multiplies positions, normals, and tangents by `(-1, 1, 1)` (and the
//!   tangent `w` by `-1`) to move from Unity's left-handed space to glTF's right-handed space.
//!
//! This module reimplements that pipeline and writes the glTF 2.0 JSON plus binary container
//! directly, so no external glTF library is required at runtime.
//!
//! Two deliberate differences from the source, both required by the project's bounds and
//! diagnosability rules:
//!
//! - The source indexes `ProcessedIndexBuffer` without checking the sub-mesh range against the
//!   buffer length. Here an out-of-range sub-mesh is an explicit [`GlbError::IndexOutOfRange`], and
//!   a partial trailing primitive group is reported through [`GlbMeshExport::warnings`] rather than
//!   silently dropped.
//! - A bone index above `u16::MAX` is an explicit [`GlbError::BoneIndexTooLarge`]; the source
//!   stores joints as floats and lets the encoder deal with them, which has no defined outcome
//!   for unrepresentable indices.
//! - `GlbWriter.TryWrite` refuses oversized models with a message instead of succeeding. Here
//!   `GlbBuildLimits::max_output_bytes` (default 2GiB, mirroring the source 2Gb buffer ceiling)
//!   is checked both per buffer view and on the final binary-plus-JSON total, yielding an
//!   explicit [`GlbError::OutputTooLarge`] instead of an allocation failure. The source's
//!   catch-all and out-of-memory branches have no separable Rust counterpart: unexpected
//!   failures already propagate as their own `GlbError` variants with context.
//!
//! Accessor encodings follow the SharpGLTF 1.0.2 writer differentially (probe
//! `Temp/opencode/gltfprobe`): positions, normals, tangents, texture coordinates, and weights
//! are little-endian floats; `COLOR_0` is normalized unsigned bytes with truncating
//! quantization; `JOINTS_0` is unsigned bytes while every joint fits, unsigned shorts
//! otherwise; indices are unsigned shorts while every index fits below the reserved
//! `65535`, unsigned ints otherwise. Skin lanes reproduce `SparseWeight8.Create` (duplicate
//! merge, exact-zero drop), `OrderedByWeight` (strongest first, magnitude-compared, smaller
//! indices on ties), and the `VertexJoints4` renormalization (skipped at exact sums of zero
//! and one, like the source).
//!
//! Vertex deduplication mirrors `VertexBuilder.CreateCompatibleMesh`: vertices with bit-identical
//! attributes share a glTF vertex. Ordering is deterministic (first use within each sub-mesh, then
//! sub-meshes in file order).
//!
//! The per-renderer hierarchy decisions from `GlbLevelBuilder` live here as caller-fact kernels:
//! `references_dynamic_mesh` (the static-vs-dynamic mesh dispatch) and `subset_indices` (the
//! explicit-subset versus static-batch-range selection). Transform accumulation, the material and
//! mesh caches, and the SharpGLTF scene assembly stay caller-side.

use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Display};

use crate::numerics::{Quat, Vec2, Vec3, Vec4};

use super::mesh_data::MeshData;
use super::sub_mesh_data::MeshTopology;

/// The generator string written into the glTF asset block.
const GENERATOR: &str = concat!(
    "AssetRipper ",
    env!("CARGO_PKG_VERSION"),
    " (assetripper-rs)"
);

/// The material name used by `GlbMeshBuilder.Build`.
const DEFAULT_MATERIAL_NAME: &str = "DefaultMaterial";

/// `GlbMeshType`: a flags set describing which vertex attributes a mesh carries.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GlbMeshType(u8);

impl GlbMeshType {
    /// `VertexPosition` (also the `Empty` value).
    pub const POSITION: Self = Self(0);
    /// `VertexPositionNormal`.
    pub const NORMAL: Self = Self(1 << 0);
    /// Tangent presence.
    pub const TANGENT: Self = Self(1 << 1);
    /// `VertexColor1`.
    pub const COLOR1: Self = Self(1 << 2);
    /// `VertexTexture1`.
    pub const TEXTURE1: Self = Self(1 << 3);
    /// `VertexTexture2`.
    pub const TEXTURE2: Self = Self(1 << 4);
    /// More than two texture channels.
    pub const TEXTURE_N: Self = Self(1 << 5);
    /// `VertexJoints4`.
    pub const JOINTS4: Self = Self(1 << 6);

    /// Whether every bit of `other` is present.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// The raw flag bits.
    pub const fn bits(self) -> u8 {
        self.0
    }
}

/// `MeshDataExtensions.GetMeshType`.
pub fn get_mesh_type(mesh: &MeshData) -> GlbMeshType {
    let mut mesh_type = GlbMeshType::POSITION;
    if mesh.has_normals() {
        mesh_type.0 |= if mesh.has_tangents() {
            GlbMeshType::NORMAL.0 | GlbMeshType::TANGENT.0
        } else {
            GlbMeshType::NORMAL.0
        };
    }
    mesh_type.0 |= match mesh.uv_count() {
        0 => 0,
        1 => GlbMeshType::TEXTURE1.0,
        2 => GlbMeshType::TEXTURE2.0,
        _ => GlbMeshType::TEXTURE_N.0,
    };
    if mesh.has_colors() {
        mesh_type.0 |= GlbMeshType::COLOR1.0;
    }
    if mesh.has_skin() {
        mesh_type.0 |= GlbMeshType::JOINTS4.0;
    }
    mesh_type
}

/// `GlbCoordinateConversion.CoordinateSpaceConversionScale`.
pub const COORDINATE_SPACE_CONVERSION_SCALE: Vec3 = Vec3 {
    x: -1.0,
    y: 1.0,
    z: 1.0,
};

/// `GlbCoordinateConversion.TangentSpaceConversionScale`.
pub const TANGENT_SPACE_CONVERSION_SCALE: Vec4 = Vec4 {
    x: -1.0,
    y: 1.0,
    z: 1.0,
    w: -1.0,
};

/// `GlbCoordinateConversion.ToGltfVector3Convert`.
pub fn to_gltf_vector3(value: Vec3) -> Vec3 {
    Vec3 {
        x: value.x * COORDINATE_SPACE_CONVERSION_SCALE.x,
        y: value.y * COORDINATE_SPACE_CONVERSION_SCALE.y,
        z: value.z * COORDINATE_SPACE_CONVERSION_SCALE.z,
    }
}

/// `GlbCoordinateConversion.ToGltfTangentConvert`.
pub fn to_gltf_tangent(value: Vec4) -> Vec4 {
    Vec4 {
        x: value.x * TANGENT_SPACE_CONVERSION_SCALE.x,
        y: value.y * TANGENT_SPACE_CONVERSION_SCALE.y,
        z: value.z * TANGENT_SPACE_CONVERSION_SCALE.z,
        w: value.w * TANGENT_SPACE_CONVERSION_SCALE.w,
    }
}

/// `GlbCoordinateConversion.ToGltfQuaternionConvert`.
pub fn to_gltf_quaternion(value: Quat) -> Quat {
    let from = Vec3 {
        x: value.x,
        y: value.y,
        z: value.z,
    };
    let to = to_gltf_vector3(from);
    Quat {
        x: to.x,
        y: to.y,
        z: to.z,
        w: value.w,
    }
}

/// A failure while exporting a mesh to GLB.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GlbError {
    /// The mesh has no vertices, so there is nothing to write.
    NoVertices,
    /// A sub-mesh uses a topology the exporter does not rewrite.
    UnsupportedTopology {
        /// The zero-based sub-mesh position.
        sub_mesh: usize,
        /// The mesh name supplied by the caller.
        mesh: String,
        /// The raw `MeshTopology` value.
        topology: i32,
    },
    /// A sub-mesh index range leaves the processed index buffer.
    IndexOutOfRange {
        /// The zero-based sub-mesh position.
        sub_mesh: usize,
        /// The mesh name supplied by the caller.
        mesh: String,
        /// The requested index count.
        index_count: u64,
        /// The first index plus the requested count.
        required_length: u64,
        /// The available processed index buffer length.
        available_length: usize,
    },
    /// A position component is not finite, so the required `min`/`max` cannot be expressed.
    NonFinitePosition {
        /// The mesh name supplied by the caller.
        mesh: String,
        /// The source vertex index.
        vertex: u32,
        /// Which component was non-finite (`x`, `y`, or `z`).
        component: &'static str,
    },
    /// A bone index is negative, which cannot be represented in `JOINTS_0`.
    NegativeBoneIndex {
        /// The mesh name supplied by the caller.
        mesh: String,
        /// The source vertex index.
        vertex: u32,
        /// The offending bone index.
        bone_index: i32,
    },
    /// A bone index exceeds `u16::MAX`, which neither `JOINTS_0` encoding can represent.
    BoneIndexTooLarge {
        /// The mesh name supplied by the caller.
        mesh: String,
        /// The source vertex index.
        vertex: u32,
        /// The offending bone index.
        bone_index: i32,
    },
    /// The export exceeds the configured output cap, mirroring the source `GlbWriter`
    /// refusal for models too large to write.
    OutputTooLarge {
        /// The refused byte count (binary chunk plus JSON).
        byte_count: u64,
        /// The configured cap.
        limit: u64,
    },
    /// `GlbTerrainBuilder.Build` divides `1 / (grid - 1)` in integer arithmetic; a grid
    /// edge of one makes the source throw `DivideByZeroException`.
    TerrainUvScaleDivisionByZero {
        /// The serialized `Heightmap.Width`.
        width: i32,
        /// The serialized `Heightmap.Height`.
        height: i32,
        /// The serialized `Heightmap.Resolution`.
        resolution: i32,
    },
    /// The serialized heights list is shorter than the `tw * th` grid the source reads.
    TerrainHeightsTooShort {
        /// `max(Width, Resolution) * max(Height, Resolution)`.
        required: u64,
        /// The supplied heights length.
        available: usize,
    },
    /// A non-square terrain grid reads `float[tw, th]` outside its bounds in the source.
    TerrainGridNotSquare {
        /// `max(Width, Resolution)`.
        grid_width: i32,
        /// `max(Height, Resolution)`.
        grid_height: i32,
    },
    /// A navmesh height mesh has an index count that is not a multiple of three; the source
    /// throws `Exception("Height mesh must be triangles.")`.
    NavMeshIndexNotTriangles {
        /// The zero-based height-mesh position.
        mesh_index: usize,
        /// The offending index count.
        index_count: usize,
    },
    /// A navmesh height-mesh index leaves its vertex list.
    NavMeshIndexOutOfRange {
        /// The zero-based height-mesh position.
        mesh_index: usize,
        /// The position within the index list.
        entry: usize,
        /// The offending index.
        index: i32,
        /// The available vertex count.
        vertex_count: usize,
    },
    /// An emitted source index leaves the vertex list it was assembled from.
    VertexIndexOutOfRange {
        /// The node name supplied by the caller.
        mesh: String,
        /// The offending source index.
        index: u32,
        /// The available vertex count.
        vertex_count: usize,
    },
}

impl Display for GlbError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoVertices => write!(formatter, "mesh has no vertices to export"),
            Self::UnsupportedTopology {
                sub_mesh,
                mesh,
                topology,
            } => write!(
                formatter,
                "mesh '{mesh}' sub-mesh {sub_mesh} uses unsupported MeshTopology {topology}"
            ),
            Self::IndexOutOfRange {
                sub_mesh,
                mesh,
                index_count,
                required_length,
                available_length,
            } => write!(
                formatter,
                "mesh '{mesh}' sub-mesh {sub_mesh} needs {required_length} indices \
                 ({index_count} from first index) but the processed index buffer holds \
                 {available_length}"
            ),
            Self::NonFinitePosition {
                mesh,
                vertex,
                component,
            } => write!(
                formatter,
                "mesh '{mesh}' vertex {vertex} has a non-finite {component} coordinate"
            ),
            Self::NegativeBoneIndex {
                mesh,
                vertex,
                bone_index,
            } => write!(
                formatter,
                "mesh '{mesh}' vertex {vertex} has negative bone index {bone_index}"
            ),
            Self::BoneIndexTooLarge {
                mesh,
                vertex,
                bone_index,
            } => write!(
                formatter,
                "mesh '{mesh}' vertex {vertex} has bone index {bone_index} above u16::MAX"
            ),
            Self::OutputTooLarge { byte_count, limit } => write!(
                formatter,
                "model of {byte_count} bytes exceeds the {limit}-byte GLB output cap and was too large to export"
            ),
            Self::TerrainUvScaleDivisionByZero {
                width,
                height,
                resolution,
            } => write!(
                formatter,
                "terrain heightmap {width}x{height} (resolution {resolution}) has a one-vertex \
                 grid edge, which divides by zero in the source UV scale"
            ),
            Self::TerrainHeightsTooShort {
                required,
                available,
            } => write!(
                formatter,
                "terrain heightmap needs {required} serialized heights but only {available} are present"
            ),
            Self::TerrainGridNotSquare {
                grid_width,
                grid_height,
            } => write!(
                formatter,
                "terrain grid {grid_width}x{grid_height} is not square; the source indexes its \
                 height array out of range"
            ),
            Self::NavMeshIndexNotTriangles {
                mesh_index,
                index_count,
            } => write!(
                formatter,
                "navmesh height mesh {mesh_index} has {index_count} indices; height meshes must be triangles"
            ),
            Self::NavMeshIndexOutOfRange {
                mesh_index,
                entry,
                index,
                vertex_count,
            } => write!(
                formatter,
                "navmesh height mesh {mesh_index} index {entry} is {index}, which leaves the \
                 {vertex_count}-vertex list"
            ),
            Self::VertexIndexOutOfRange {
                mesh,
                index,
                vertex_count,
            } => write!(
                formatter,
                "mesh '{mesh}' emits source index {index}, which leaves the {vertex_count}-vertex list"
            ),
        }
    }
}

impl Error for GlbError {}

/// The result of a GLB export.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GlbMeshExport {
    /// The complete GLB file bytes.
    pub bytes: Vec<u8>,
    /// The emitted source-index sequence for each sub-mesh, in sub-mesh order.
    ///
    /// This is the auditable record of the topology rewrite: it shows exactly which mesh vertices
    /// reached the GLB, in which winding, so a reviewer can compare it against the reference's
    /// `GlbSubMeshBuilder.BuildSubMesh` output without reparsing the binary.
    pub emitted_source_indices: Vec<Vec<u32>>,
    /// Non-fatal observations, in a deterministic order.
    pub warnings: Vec<String>,
}

/// A deduplicated glTF vertex.
#[derive(Clone, Debug, PartialEq)]
struct GlbVertex {
    /// `POSITION` in glTF space.
    position: [f32; 3],
    /// `NORMAL` in glTF space.
    normal: Option<[f32; 3]>,
    /// `TANGENT` in glTF space.
    tangent: Option<[f32; 4]>,
    /// `TEXCOORD_0..N` (already flipped to the glTF convention by the data layer).
    tex_coords: Vec<[f32; 2]>,
    /// `COLOR_0`.
    color: Option<[f32; 4]>,
    /// `JOINTS_0`.
    joints: Option<[u16; 4]>,
    /// `WEIGHTS_0`.
    weights: Option<[f32; 4]>,
}

impl Eq for GlbVertex {}

impl GlbVertex {
    fn bits(value: f32) -> u32 {
        value.to_bits()
    }

    /// A hashable key over the bit patterns of every attribute.
    fn key(&self) -> VertexKey {
        let mut floats = Vec::with_capacity(3 + 3 + 4 + 2 * self.tex_coords.len() + 4 + 4);
        floats.extend(self.position.map(Self::bits));
        if let Some(normal) = self.normal {
            floats.extend(normal.map(Self::bits));
        }
        if let Some(tangent) = self.tangent {
            floats.extend(tangent.map(Self::bits));
        }
        for tex_coord in &self.tex_coords {
            floats.extend(tex_coord.map(Self::bits));
        }
        if let Some(color) = self.color {
            floats.extend(color.map(Self::bits));
        }
        if let Some(weights) = self.weights {
            floats.extend(weights.map(Self::bits));
        }
        VertexKey {
            floats,
            joints: self.joints,
            normal: self.normal.is_some(),
            tangent: self.tangent.is_some(),
            color: self.color.is_some(),
            weights: self.weights.is_some(),
        }
    }
}

/// The hashable identity of a [`GlbVertex`].
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct VertexKey {
    floats: Vec<u32>,
    joints: Option<[u16; 4]>,
    normal: bool,
    tangent: bool,
    color: bool,
    weights: bool,
}

/// Bounds for GLB export.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GlbBuildLimits {
    /// Maximum combined binary-chunk and JSON bytes for one export, mirroring the source
    /// `GlbWriter` 2Gb buffer ceiling with a friendly error instead of an allocation failure.
    pub max_output_bytes: u64,
}

impl Default for GlbBuildLimits {
    fn default() -> Self {
        Self {
            max_output_bytes: 2 * 1024 * 1024 * 1024,
        }
    }
}

/// Accumulates glTF accessors, buffer views, and the binary chunk.
#[derive(Debug)]
struct GlbBuilder {
    bin: Vec<u8>,
    buffer_views: Vec<String>,
    accessors: Vec<String>,
    limit: u64,
}

impl Default for GlbBuilder {
    fn default() -> Self {
        Self::with_limit(GlbBuildLimits::default().max_output_bytes)
    }
}

impl GlbBuilder {
    fn with_limit(limit: u64) -> Self {
        Self {
            bin: Vec::new(),
            buffer_views: Vec::new(),
            accessors: Vec::new(),
            limit,
        }
    }
    /// Appends tightly packed floats as one buffer view and returns its index.
    fn push_float_view(&mut self, values: &[f32]) -> Result<usize, GlbError> {
        let mut bytes = Vec::with_capacity(values.len() * 4);
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        self.push_view(bytes)
    }

    /// Appends tightly packed unsigned bytes as one buffer view and returns its index.
    fn push_ubyte_view(&mut self, values: &[u8]) -> Result<usize, GlbError> {
        self.push_view(values.to_vec())
    }

    /// Appends tightly packed unsigned shorts as one buffer view and returns its index.
    fn push_ushort_view(&mut self, values: &[u16]) -> Result<usize, GlbError> {
        let mut bytes = Vec::with_capacity(values.len() * 2);
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        self.push_view(bytes)
    }

    /// Appends tightly packed unsigned ints as one buffer view and returns its index.
    fn push_uint_view(&mut self, values: &[u32]) -> Result<usize, GlbError> {
        let mut bytes = Vec::with_capacity(values.len() * 4);
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        self.push_view(bytes)
    }

    fn push_view(&mut self, mut bytes: Vec<u8>) -> Result<usize, GlbError> {
        while !bytes.len().is_multiple_of(4) {
            bytes.push(0);
        }
        let offset = self.bin.len();
        let length = bytes.len();
        if offset as u64 + length as u64 > self.limit {
            return Err(GlbError::OutputTooLarge {
                byte_count: offset as u64 + length as u64,
                limit: self.limit,
            });
        }
        self.bin.extend_from_slice(&bytes);
        let index = self.buffer_views.len();
        self.buffer_views.push(format!(
            "{{\"buffer\":0,\"byteOffset\":{offset},\"byteLength\":{length}}}"
        ));
        Ok(index)
    }

    /// Adds a vertex accessor with the given component count.
    ///
    /// Only `VEC2`, `VEC3`, and `VEC4` float accessors are emitted; the count is the number of
    /// scalars divided by the component count, so callers must supply whole vertices.
    fn push_vertex_accessor(
        &mut self,
        values: &[f32],
        kind: &'static str,
    ) -> Result<usize, GlbError> {
        let components = match kind {
            "VEC2" => 2,
            "VEC3" => 3,
            _ => 4,
        };
        let view = self.push_float_view(values)?;
        self.accessors.push(format!(
            "{{\"bufferView\":{view},\"componentType\":5126,\"count\":{},\"type\":\"{kind}\"}}",
            values.len() / components
        ));
        Ok(self.accessors.len() - 1)
    }

    /// Adds a `SCALAR` index accessor, unsigned shorts while every index fits below the
    /// reserved `65535`, unsigned ints otherwise (matching the SharpGLTF writer widths).
    fn push_index_accessor(&mut self, indices: &[u32]) -> Result<usize, GlbError> {
        if indices.iter().all(|index| *index <= 65534) {
            let values: Vec<u16> = indices.iter().map(|index| *index as u16).collect();
            let view = self.push_ushort_view(&values)?;
            self.accessors.push(format!(
                "{{\"bufferView\":{view},\"componentType\":5123,\"count\":{},\"type\":\"SCALAR\"}}",
                indices.len()
            ));
        } else {
            let view = self.push_uint_view(indices)?;
            self.accessors.push(format!(
                "{{\"bufferView\":{view},\"componentType\":5125,\"count\":{},\"type\":\"SCALAR\"}}",
                indices.len()
            ));
        }
        Ok(self.accessors.len() - 1)
    }

    /// Adds the `POSITION` accessor, which the specification requires to carry `min` and `max`.
    fn push_position_accessor(
        &mut self,
        positions: &[[f32; 3]],
        mesh: &str,
    ) -> Result<usize, GlbError> {
        let mut values = Vec::with_capacity(positions.len() * 3);
        for (position_index, position) in positions.iter().enumerate() {
            for (component_index, component) in position.iter().enumerate() {
                if !component.is_finite() {
                    let component = match component_index {
                        0 => "x",
                        1 => "y",
                        _ => "z",
                    };
                    return Err(GlbError::NonFinitePosition {
                        mesh: mesh.to_owned(),
                        vertex: position_index as u32,
                        component,
                    });
                }
                values.push(*component);
            }
        }
        let mut minimum = [f32::INFINITY; 3];
        let mut maximum = [f32::NEG_INFINITY; 3];
        for position in positions {
            for axis in 0..3 {
                minimum[axis] = minimum[axis].min(position[axis]);
                maximum[axis] = maximum[axis].max(position[axis]);
            }
        }
        let view = self.push_float_view(&values)?;
        let minimum = format!(
            "[{},{},{}]",
            format_f32(minimum[0]),
            format_f32(minimum[1]),
            format_f32(minimum[2])
        );
        let maximum = format!(
            "[{},{},{}]",
            format_f32(maximum[0]),
            format_f32(maximum[1]),
            format_f32(maximum[2])
        );
        self.accessors.push(format!(
            "{{\"bufferView\":{view},\"componentType\":5126,\"count\":{},\"type\":\"VEC3\",\
             \"min\":{minimum},\"max\":{maximum}}}",
            positions.len()
        ));
        Ok(self.accessors.len() - 1)
    }

    /// Adds a `COLOR_0` accessor: normalized unsigned bytes with truncating quantization,
    /// matching the SharpGLTF writer.
    fn push_color_accessor(&mut self, colors: &[[f32; 4]]) -> Result<usize, GlbError> {
        let mut values = Vec::with_capacity(colors.len() * 4);
        for color in colors {
            values.extend(quantize_color(*color));
        }
        let view = self.push_ubyte_view(&values)?;
        self.accessors.push(format!(
            "{{\"bufferView\":{view},\"componentType\":5121,\"count\":{},\"normalized\":true,\"type\":\"VEC4\"}}",
            colors.len()
        ));
        Ok(self.accessors.len() - 1)
    }

    /// Adds a `JOINTS_0` accessor: unsigned bytes while every joint fits, unsigned shorts
    /// otherwise (matching the SharpGLTF writer widths).
    fn push_joints_accessor(&mut self, joints: &[[u16; 4]]) -> Result<usize, GlbError> {
        if joints
            .iter()
            .flatten()
            .all(|joint| *joint <= u8::MAX as u16)
        {
            let mut values = Vec::with_capacity(joints.len() * 4);
            for joint in joints {
                values.extend(joint.map(|lane| lane as u8));
            }
            let view = self.push_ubyte_view(&values)?;
            self.accessors.push(format!(
                "{{\"bufferView\":{view},\"componentType\":5121,\"count\":{},\"type\":\"VEC4\"}}",
                joints.len()
            ));
        } else {
            let mut values = Vec::with_capacity(joints.len() * 4);
            for joint in joints {
                values.extend_from_slice(joint);
            }
            let view = self.push_ushort_view(&values)?;
            self.accessors.push(format!(
                "{{\"bufferView\":{view},\"componentType\":5123,\"count\":{},\"type\":\"VEC4\"}}",
                joints.len()
            ));
        }
        Ok(self.accessors.len() - 1)
    }
}

/// Formats a float for glTF JSON, preserving round-trip precision.
fn format_f32(value: f32) -> String {
    // `{}` on `f32` prints the shortest representation that round-trips. Whole numbers are
    // printed without a fractional part, which keeps the output readable and stable.
    let text = format!("{value}");
    if text.contains(['.', 'e', 'E']) {
        text
    } else {
        format!("{text}.0")
    }
}

/// Escapes a string for a glTF JSON document.
fn escape_json(value: &str) -> String {
    let mut result = String::with_capacity(value.len() + 2);
    for character in value.chars() {
        match character {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            '\u{08}' => result.push_str("\\b"),
            '\u{0c}' => result.push_str("\\f"),
            control if (control as u32) < 0x20 => {
                result.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => result.push(other),
        }
    }
    result
}

/// Quantizes one finite color channel to normalized unsigned bytes with C#-style
/// truncation: non-positive values become zero, values at or above one become 255, and the
/// rest truncate `value * 255` (so `0.5` maps to `127`, as the writer emits).
fn quantize_color_channel(value: f32) -> u8 {
    if value <= 0.0 {
        0
    } else if value >= 1.0 {
        u8::MAX
    } else {
        (value * 255.0) as u8
    }
}

/// Quantizes one vertex color: any non-finite channel zeroes the whole color, matching the
/// writer's vector-wide poisoning (probed with infinite and NaN lanes).
fn quantize_color(color: [f32; 4]) -> [u8; 4] {
    if color.iter().all(|value| value.is_finite()) {
        color.map(quantize_color_channel)
    } else {
        [0; 4]
    }
}

/// The glTF primitive mode for a converted topology.
fn glb_mode(topology: MeshTopology) -> Option<u32> {
    Some(match topology {
        MeshTopology::POINTS => 0,
        MeshTopology::LINES | MeshTopology::LINE_STRIP => 1,
        MeshTopology::TRIANGLES | MeshTopology::TRIANGLE_STRIP | MeshTopology::QUADS => 4,
        _ => return None,
    })
}

/// The index group size a topology consumes per emitted primitive.
fn primitive_group_size(topology: MeshTopology) -> u64 {
    match topology {
        MeshTopology::LINES => 2,
        MeshTopology::POINTS => 1,
        _ => 3,
    }
}

/// The number of source indices a topology consumes per emitted primitive.
///
/// Quads consume four source indices but emit two triangles, so their emitted triangle count is
/// `index_count / 4 * 2`; the caller loops on the source index count instead.
fn source_group_size(topology: MeshTopology) -> u64 {
    match topology {
        MeshTopology::LINES => 2,
        MeshTopology::POINTS => 1,
        MeshTopology::QUADS => 4,
        _ => 3,
    }
}

/// Processed skin lanes for one vertex: joints plus weights.
type SkinLanes = (Option<[u16; 4]>, Option<[f32; 4]>);

/// `SparseWeight8.Create` plus `OrderedByWeight` plus the `VertexJoints4` renormalization
/// for one vertex: negative indices fail, duplicate indices merge by summation, exact-zero
/// weights drop out, an empty set stays zero (the source constructor skips renormalization
/// at exact sums of zero and one), other sums scale to one, lanes sort strongest-first with
/// magnitudes compared like `IndexWeight.IsGreaterThan` (smaller indices win ties), and
/// missing lanes pad with joint zero.
fn process_skin_bindings(
    indices: &[i32; 4],
    weights: &[f32; 4],
    mesh_name: &str,
    vertex: u32,
) -> Result<SkinLanes, GlbError> {
    let mut merged: Vec<(i32, f32)> = Vec::with_capacity(4);
    for (&bone_index, &weight) in indices.iter().zip(weights.iter()) {
        if bone_index < 0 {
            return Err(GlbError::NegativeBoneIndex {
                mesh: mesh_name.to_owned(),
                vertex,
                bone_index,
            });
        }
        if weight == 0.0 {
            continue;
        }
        if let Some(slot) = merged.iter_mut().find(|slot| slot.0 == bone_index) {
            slot.1 += weight;
        } else {
            merged.push((bone_index, weight));
        }
    }
    if merged.is_empty() {
        return Ok((Some([0; 4]), Some([0.0; 4])));
    }
    merged.sort_by(|left, right| {
        let left_magnitude = left.1.abs();
        let right_magnitude = right.1.abs();
        if left_magnitude > right_magnitude {
            std::cmp::Ordering::Less
        } else if right_magnitude > left_magnitude {
            std::cmp::Ordering::Greater
        } else if left_magnitude.is_nan() {
            if right_magnitude.is_nan() {
                left.0.cmp(&right.0)
            } else {
                std::cmp::Ordering::Greater
            }
        } else if right_magnitude.is_nan() {
            std::cmp::Ordering::Less
        } else {
            left.0.cmp(&right.0)
        }
    });
    let sum: f32 = merged.iter().map(|lane| lane.1).sum();
    let mut joint_values = [0u16; 4];
    let mut weight_values = [0.0f32; 4];
    for (lane, (bone_index, weight)) in merged.into_iter().enumerate() {
        if bone_index > u16::MAX as i32 {
            return Err(GlbError::BoneIndexTooLarge {
                mesh: mesh_name.to_owned(),
                vertex,
                bone_index,
            });
        }
        joint_values[lane] = bone_index as u16;
        weight_values[lane] = if sum != 0.0 && sum != 1.0 {
            weight / sum
        } else {
            weight
        };
    }
    Ok((Some(joint_values), Some(weight_values)))
}

/// Builds a glTF vertex for `index` from the processed mesh data.
fn build_vertex(mesh: &MeshData, mesh_name: &str, index: u32) -> Result<GlbVertex, GlbError> {
    let mesh_type = get_mesh_type(mesh);
    let position = to_gltf_vector3(mesh.try_get_vertex_at_index(index));
    let normal = if mesh_type.contains(GlbMeshType::NORMAL) {
        let value = mesh.try_get_normal_at_index(index);
        let length = (value.x * value.x + value.y * value.y + value.z * value.z).sqrt();
        let normalized = if length == 0.0 || !length.is_finite() {
            Vec3::default()
        } else {
            Vec3 {
                x: value.x / length,
                y: value.y / length,
                z: value.z / length,
            }
        };
        Some(to_gltf_vector3(normalized))
    } else {
        None
    };
    let tangent = if mesh_type.contains(GlbMeshType::TANGENT) {
        let value = mesh.try_get_tangent_at_index(index);
        let vector = Vec3 {
            x: value.x,
            y: value.y,
            z: value.z,
        };
        let length = (vector.x * vector.x + vector.y * vector.y + vector.z * vector.z).sqrt();
        let normalized = if length == 0.0 || !length.is_finite() {
            Vec3::default()
        } else {
            Vec3 {
                x: vector.x / length,
                y: vector.y / length,
                z: vector.z / length,
            }
        };
        Some(to_gltf_tangent(Vec4 {
            w: if value.w >= 0.0 { 1.0 } else { -1.0 },
            ..Vec4 {
                x: normalized.x,
                y: normalized.y,
                z: normalized.z,
                w: 0.0,
            }
        }))
    } else {
        None
    };
    let mut tex_coords = Vec::new();
    if mesh_type.contains(GlbMeshType::TEXTURE1) {
        tex_coords.push(uv_pair(mesh.try_get_uv0_at_index(index)));
    }
    if mesh_type.contains(GlbMeshType::TEXTURE2) {
        tex_coords.push(uv_pair(mesh.try_get_uv1_at_index(index)));
    }
    if mesh_type.contains(GlbMeshType::TEXTURE_N) {
        let channels = [
            mesh.try_get_uv2_at_index(index),
            mesh.try_get_uv3_at_index(index),
            mesh.try_get_uv4_at_index(index),
            mesh.try_get_uv5_at_index(index),
            mesh.try_get_uv6_at_index(index),
            mesh.try_get_uv7_at_index(index),
        ];
        for channel in channels {
            tex_coords.push(uv_pair(channel));
        }
    }
    let color = if mesh_type.contains(GlbMeshType::COLOR1) {
        let value = mesh.try_get_color_at_index(index);
        Some([value.r, value.g, value.b, value.a])
    } else {
        None
    };
    let (joints, weights) = if mesh_type.contains(GlbMeshType::JOINTS4) {
        let skin = mesh.try_get_skin_at_index(index);
        process_skin_bindings(&skin.indices, &skin.weights, mesh_name, index)?
    } else {
        (None, None)
    };
    Ok(GlbVertex {
        position: [position.x, position.y, position.z],
        normal: normal.map(|value| [value.x, value.y, value.z]),
        tangent: tangent.map(|value| [value.x, value.y, value.z, value.w]),
        tex_coords,
        color,
        joints,
        weights,
    })
}

fn uv_pair(value: Vec2) -> [f32; 2] {
    [value.x, value.y]
}

/// `GlbSubMeshBuilder.BuildSubMesh`: the emitted index sequence for one sub-mesh.
///
/// Returns the deduplicated vertex list, the emitted indices as slots into that list, the glTF
/// primitive mode, and any non-fatal observations.
fn build_primitive(
    mesh: &MeshData,
    mesh_name: &str,
    sub_mesh_index: usize,
) -> Result<Primitive, GlbError> {
    let sub_mesh = &mesh.sub_meshes[sub_mesh_index];
    let topology = sub_mesh.topology;
    let Some(mode) = glb_mode(topology) else {
        return Err(GlbError::UnsupportedTopology {
            sub_mesh: sub_mesh_index,
            mesh: mesh_name.to_owned(),
            topology: topology.0,
        });
    };
    let first_index = sub_mesh.first_index.max(0) as u64;
    let index_count = sub_mesh.index_count.max(0) as u64;
    let available = mesh.processed_index_buffer.len() as u64;
    let required = first_index + index_count;
    if required > available {
        return Err(GlbError::IndexOutOfRange {
            sub_mesh: sub_mesh_index,
            mesh: mesh_name.to_owned(),
            index_count,
            required_length: required,
            available_length: mesh.processed_index_buffer.len(),
        });
    }

    let index_at =
        |offset: u64| -> u32 { mesh.processed_index_buffer[(first_index + offset) as usize] };
    let mut warnings: Vec<String> = Vec::new();
    let mut order: Vec<u32> = Vec::new();
    match topology {
        MeshTopology::TRIANGLES => {
            let mut offset = 0;
            while offset + 3 <= index_count {
                order.push(index_at(offset + 2));
                order.push(index_at(offset + 1));
                order.push(index_at(offset));
                offset += 3;
            }
            if offset != index_count {
                warnings.push(format!(
                    "sub-mesh {sub_mesh_index} of mesh '{mesh_name}' has {index_count} triangle \
                     indices; {} trailing indices were not a complete triangle",
                    index_count - offset
                ));
            }
        }
        MeshTopology::TRIANGLE_STRIP => {
            let mut offset = 0;
            while offset + 3 <= index_count {
                let a = index_at(offset + 2);
                let b = index_at(offset + 1);
                let c = index_at(offset);
                if a == b || a == c || b == c {
                    offset += 1;
                    continue;
                }
                if offset & 1 == 1 {
                    order.push(b);
                    order.push(a);
                    order.push(c);
                } else {
                    order.push(a);
                    order.push(b);
                    order.push(c);
                }
                offset += 1;
            }
            if index_count < 2 {
                warnings.push(format!(
                    "sub-mesh {sub_mesh_index} of mesh '{mesh_name}' is a triangle strip with \
                     {index_count} indices, which cannot form a triangle"
                ));
            }
        }
        MeshTopology::QUADS => {
            let mut offset = 0;
            while offset + 4 <= index_count {
                let a = index_at(offset + 3);
                let b = index_at(offset + 2);
                let c = index_at(offset + 1);
                let d = index_at(offset);
                order.push(a);
                order.push(b);
                order.push(c);
                order.push(a);
                order.push(c);
                order.push(d);
                offset += 4;
            }
            if offset != index_count {
                warnings.push(format!(
                    "sub-mesh {sub_mesh_index} of mesh '{mesh_name}' has {index_count} quad \
                     indices; {} trailing indices were not a complete quadrangle",
                    index_count - offset
                ));
            }
        }
        MeshTopology::LINES => {
            let mut offset = 0;
            while offset + 2 <= index_count {
                order.push(index_at(offset + 1));
                order.push(index_at(offset));
                offset += 2;
            }
            if offset != index_count {
                warnings.push(format!(
                    "sub-mesh {sub_mesh_index} of mesh '{mesh_name}' has {index_count} line \
                     indices; {} trailing indices were not a complete line",
                    index_count - offset
                ));
            }
        }
        MeshTopology::LINE_STRIP => {
            if index_count == 0 {
                warnings.push(format!(
                    "sub-mesh {sub_mesh_index} of mesh '{mesh_name}' is an empty line strip"
                ));
            }
            let mut previous = if index_count > 0 { index_at(0) } else { 0 };
            for offset in 1..index_count {
                let current = index_at(offset);
                order.push(current);
                order.push(previous);
                previous = current;
            }
            if index_count > 0 && index_count < 2 {
                warnings.push(format!(
                    "sub-mesh {sub_mesh_index} of mesh '{mesh_name}' is a line strip with \
                     {index_count} index, which cannot form a line"
                ));
            }
        }
        MeshTopology::POINTS => {
            for offset in 0..index_count {
                order.push(index_at(offset));
            }
        }
        _ => unreachable!("unreachable topology rejected above"),
    }

    let mut vertices: Vec<GlbVertex> = Vec::with_capacity(order.len());
    let mut lookup: HashMap<VertexKey, u32> = HashMap::new();
    let mut indices: Vec<u32> = Vec::with_capacity(order.len());
    for &source_index in &order {
        let vertex = build_vertex(mesh, mesh_name, source_index)?;
        let key = vertex.key();
        let slot = *lookup.entry(key).or_insert_with(|| {
            let slot = vertices.len() as u32;
            vertices.push(vertex);
            slot
        });
        indices.push(slot);
    }
    Ok(Primitive {
        indices,
        source_indices: order,
        vertices,
        mode,
        warnings,
    })
}

/// The emitted form of one sub-mesh.
#[derive(Clone, Debug)]
struct Primitive {
    /// Indices as slots into `vertices`.
    indices: Vec<u32>,
    /// The same sequence expressed as source vertex indices, kept for diagnostics.
    source_indices: Vec<u32>,
    /// The deduplicated vertices.
    vertices: Vec<GlbVertex>,
    /// The glTF primitive mode.
    mode: u32,
    /// Non-fatal observations.
    warnings: Vec<String>,
}

/// Flattens an optional fixed-size per-vertex attribute into an accessor buffer.
///
/// Returns `None` when the attribute is absent on any vertex, matching the source's behaviour of
/// choosing one vertex type for the whole primitive.
fn flatten_optional<const N: usize>(
    vertices: &[GlbVertex],
    select: impl Fn(&GlbVertex) -> Option<[f32; N]>,
) -> Option<Vec<f32>> {
    if vertices.is_empty() {
        return None;
    }
    let mut values = Vec::with_capacity(vertices.len() * N);
    for vertex in vertices {
        values.extend_from_slice(&select(vertex)?[..]);
    }
    Some(values)
}

/// Collects the `JOINTS_0` lanes when every vertex carries skin data.
fn collect_joints(vertices: &[GlbVertex]) -> Option<Vec<[u16; 4]>> {
    if vertices.is_empty() {
        return None;
    }
    vertices
        .iter()
        .map(|vertex| vertex.joints)
        .collect::<Option<Vec<_>>>()
}

/// Exports `mesh` as a glTF 2.0 binary file.
///
/// `name` becomes the root node name, matching `GlbMeshBuilder.Build`, which uses `mesh.Name`.
pub fn build_glb(name: &str, mesh: &MeshData) -> Result<GlbMeshExport, GlbError> {
    build_glb_with_limits(name, mesh, GlbBuildLimits::default())
}

/// Exports `mesh` as a glTF 2.0 binary file, refusing outputs beyond `limits`.
pub fn build_glb_with_limits(
    name: &str,
    mesh: &MeshData,
    limits: GlbBuildLimits,
) -> Result<GlbMeshExport, GlbError> {
    if mesh.vertices.is_empty() {
        return Err(GlbError::NoVertices);
    }
    let mut builder = GlbBuilder::with_limit(limits.max_output_bytes);
    let mut warnings: Vec<String> = Vec::new();
    let mut primitive_jsons: Vec<String> = Vec::new();
    let mut mesh_names: Vec<String> = Vec::new();
    let mut emitted_source_indices: Vec<Vec<u32>> = Vec::new();

    for sub_mesh_index in 0..mesh.sub_meshes.len() {
        let primitive = build_primitive(mesh, name, sub_mesh_index)?;
        warnings.extend(primitive.warnings);
        emitted_source_indices.push(primitive.source_indices);
        let Primitive {
            indices,
            vertices,
            mode,
            ..
        } = primitive;
        let positions: Vec<[f32; 3]> = vertices.iter().map(|vertex| vertex.position).collect();
        let mut attributes = vec![(
            "POSITION".to_owned(),
            builder.push_position_accessor(&positions, name)?,
        )];
        if let Some(values) = flatten_optional(&vertices, |vertex| vertex.normal) {
            attributes.push((
                "NORMAL".to_owned(),
                builder.push_vertex_accessor(&values, "VEC3")?,
            ));
        }
        if let Some(values) = flatten_optional(&vertices, |vertex| vertex.tangent) {
            attributes.push((
                "TANGENT".to_owned(),
                builder.push_vertex_accessor(&values, "VEC4")?,
            ));
        }
        let tex_coord_count = vertices.first().map_or(0, |vertex| vertex.tex_coords.len());
        for channel in 0..tex_coord_count {
            let mut values = Vec::with_capacity(vertices.len() * 2);
            for vertex in &vertices {
                values.extend_from_slice(
                    vertex
                        .tex_coords
                        .get(channel)
                        .copied()
                        .unwrap_or([0.0, 0.0])
                        .as_slice(),
                );
            }
            attributes.push((
                format!("TEXCOORD_{channel}"),
                builder.push_vertex_accessor(&values, "VEC2")?,
            ));
        }
        if let Some(colors) = vertices
            .iter()
            .map(|vertex| vertex.color)
            .collect::<Option<Vec<_>>>()
        {
            attributes.push(("COLOR_0".to_owned(), builder.push_color_accessor(&colors)?));
        }
        if let Some(joints) = collect_joints(&vertices) {
            let mut weights = Vec::with_capacity(vertices.len() * 4);
            for vertex in &vertices {
                weights.extend_from_slice(&vertex.weights.unwrap_or([0.0; 4])[..]);
            }
            attributes.push((
                "JOINTS_0".to_owned(),
                builder.push_joints_accessor(&joints)?,
            ));
            attributes.push((
                "WEIGHTS_0".to_owned(),
                builder.push_vertex_accessor(&weights, "VEC4")?,
            ));
        }
        let index_accessor = builder.push_index_accessor(&indices)?;

        let attribute_json = attributes
            .iter()
            .map(|(name, accessor)| format!("\"{name}\":{accessor}"))
            .collect::<Vec<_>>()
            .join(",");
        primitive_jsons.push(format!(
            "{{\"attributes\":{{{attribute_json}}},\"indices\":{index_accessor},\
             \"material\":0,\"mode\":{mode}}}"
        ));
        mesh_names.push(format!("SubMesh_{sub_mesh_index}"));
    }

    let json = build_json(
        name,
        &mesh_names,
        &primitive_jsons,
        &builder.accessors,
        &builder.buffer_views,
        builder.bin.len(),
    );
    let total = builder.bin.len() as u64 + json.len() as u64;
    if total > limits.max_output_bytes {
        return Err(GlbError::OutputTooLarge {
            byte_count: total,
            limit: limits.max_output_bytes,
        });
    }
    Ok(GlbMeshExport {
        bytes: wrap_glb(&json, &builder.bin),
        emitted_source_indices,
        warnings,
    })
}

fn build_json(
    name: &str,
    mesh_names: &[String],
    primitive_jsons: &[String],
    accessors: &[String],
    buffer_views: &[String],
    bin_length: usize,
) -> String {
    let mut nodes: Vec<String> = Vec::new();
    if mesh_names.is_empty() {
        nodes.push(format!("{{\"name\":\"{}\"}}", escape_json(name)));
    } else {
        let children = (1..=mesh_names.len())
            .map(|index| index.to_string())
            .collect::<Vec<_>>()
            .join(",");
        nodes.push(format!(
            "{{\"name\":\"{}\",\"children\":[{children}]}}",
            escape_json(name)
        ));
        for (index, mesh_name) in mesh_names.iter().enumerate() {
            nodes.push(format!(
                "{{\"name\":\"{}\",\"mesh\":{index}}}",
                escape_json(mesh_name)
            ));
        }
    }
    let meshes = primitive_jsons
        .iter()
        .enumerate()
        .map(|(index, primitives)| {
            format!(
                "{{\"name\":\"{}\",\"primitives\":[{primitives}]}}",
                escape_json(&mesh_names[index])
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let mut json = String::new();
    json.push_str("{\"asset\":{\"version\":\"2.0\",\"generator\":");
    json.push_str(&escape_json(GENERATOR));
    json.push_str("},\"scene\":0,\"scenes\":[{\"nodes\":[0]}]");
    json.push_str(&format!(",\"nodes\":[{}]", nodes.join(",")));
    if !mesh_names.is_empty() {
        json.push_str(&format!(",\"meshes\":[{meshes}]"));
    }
    json.push_str(&format!(
        ",\"materials\":[{{\"name\":\"{DEFAULT_MATERIAL_NAME}\",\"pbrMetallicRoughness\":\
         {{\"baseColorFactor\":[1.0,1.0,1.0,1.0],\"metallicFactor\":1.0,\"roughnessFactor\":1.0}}}}]"
    ));
    json.push_str(&format!(",\"accessors\":[{}]", accessors.join(",")));
    json.push_str(&format!(",\"bufferViews\":[{}]", buffer_views.join(",")));
    json.push_str(&format!(",\"buffers\":[{{\"byteLength\":{bin_length}}}]}}"));
    json
}

/// Wraps a glTF JSON document and binary chunk into a GLB container.
fn wrap_glb(json: &str, bin: &[u8]) -> Vec<u8> {
    const GLB_MAGIC: u32 = 0x4654_6C67;
    const JSON_CHUNK: u32 = 0x4E4F_534A;
    const BIN_CHUNK: u32 = 0x004E_4942;
    const HEADER_LENGTH: usize = 12;
    const CHUNK_HEADER_LENGTH: usize = 8;

    let mut json_bytes = json.as_bytes().to_vec();
    while !json_bytes.len().is_multiple_of(4) {
        json_bytes.push(b' ');
    }
    let mut bin_bytes = bin.to_vec();
    while !bin_bytes.len().is_multiple_of(4) {
        bin_bytes.push(0);
    }
    let total_length = HEADER_LENGTH
        + CHUNK_HEADER_LENGTH
        + json_bytes.len()
        + CHUNK_HEADER_LENGTH
        + bin_bytes.len();
    let mut bytes = Vec::with_capacity(total_length);
    bytes.extend_from_slice(&GLB_MAGIC.to_le_bytes());
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&(total_length as u32).to_le_bytes());
    bytes.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&JSON_CHUNK.to_le_bytes());
    bytes.extend_from_slice(&json_bytes);
    bytes.extend_from_slice(&(bin_bytes.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&BIN_CHUNK.to_le_bytes());
    bytes.extend_from_slice(&bin_bytes);
    bytes
}

/// The number of emitted indices a topology produces for `index_count` source indices.
///
/// Triangle strips are de-stripified into up to `index_count - 2` triangles (degenerates are
/// removed at runtime), line strips into `index_count - 1` lines, and quads into two triangles per
/// four source indices. Exposed for tests and for callers that report progress.
pub fn emitted_index_count(topology: MeshTopology, index_count: u64) -> u64 {
    match topology {
        MeshTopology::TRIANGLES | MeshTopology::LINES | MeshTopology::POINTS => {
            index_count / source_group_size(topology) * primitive_group_size(topology)
        }
        MeshTopology::QUADS => index_count / 4 * 6,
        MeshTopology::TRIANGLE_STRIP => {
            if index_count < 3 {
                0
            } else {
                (index_count - 2) * 3
            }
        }
        MeshTopology::LINE_STRIP => {
            if index_count < 2 {
                0
            } else {
                (index_count - 1) * 2
            }
        }
        _ => 0,
    }
}

/// `GlbLevelBuilder.ReferencesDynamicMesh` over caller facts.
///
/// Like the source (`&&` binds tighter than `||`), an empty static-batch list or an empty
/// subset list each force the dynamic path on their own.
pub fn references_dynamic_mesh(
    has_static_batch_info: bool,
    static_sub_mesh_count: u32,
    has_subset_indices: bool,
    subset_indices_count: usize,
) -> bool {
    has_static_batch_info && static_sub_mesh_count == 0
        || has_subset_indices && subset_indices_count == 0
}

/// `GlbLevelBuilder.GetSubsetIndices` over caller facts.
///
/// Explicit subset indices win; otherwise a static-batch range expands from its first sub-mesh;
/// otherwise the renderer contributes no sub-meshes.
pub fn subset_indices(subset: Option<&[u32]>, static_batch_info: Option<(u32, u32)>) -> Vec<u32> {
    if let Some(explicit) = subset {
        return explicit.to_vec();
    }
    match static_batch_info {
        Some((first_sub_mesh, sub_mesh_count)) => {
            (first_sub_mesh..).take(sub_mesh_count as usize).collect()
        }
        None => Vec::new(),
    }
}

/// `short.MaxValue`, the divisor `GlbTerrainBuilder.GetHeights` applies to every height.
pub const TERRAIN_HEIGHT_DIVISOR: i16 = i16::MAX;

/// Inputs of `GlbTerrainBuilder.Build` (`GlbTerrainBuilder.cs` lines 14-85).
///
/// The caller supplies the values the generated `Heightmap` property surface reads back, which
/// differ by Unity version: the modern subclasses expose `Resolution` and stub `Width`/`Height`
/// to zero, while 5.0 exposes `Width`/`Height` and stubs `Resolution` to zero.
#[derive(Clone, Copy, Debug)]
pub struct TerrainMeshInput<'a> {
    /// `TerrainData.Name`, used as the scene node name.
    pub name: &'a str,
    /// `Heightmap.Width` as the generated property surface reads it.
    pub width: i32,
    /// `Heightmap.Height` as the generated property surface reads it.
    pub height: i32,
    /// `Heightmap.Resolution` as the generated property surface reads it.
    pub resolution: i32,
    /// `Heightmap.Scale` in Unity space.
    pub scale: [f32; 3],
    /// `Heightmap.Heights` in serialized order.
    pub heights: &'a [i16],
}

/// `GlbTerrainBuilder.Build`: one node named after the terrain carrying a single triangulated
/// heightmap mesh with `POSITION` and `TEXCOORD_0`.
pub fn build_terrain_glb(input: &TerrainMeshInput<'_>) -> Result<GlbMeshExport, GlbError> {
    build_terrain_glb_with_limits(input, GlbBuildLimits::default())
}

/// `GlbTerrainBuilder.Build` with a bound on the emitted container size.
///
/// The geometry is a line-for-line port of `AddTerrainToSceneBuilder`: the source's
/// `1 / (grid - 1)` UV scale is **integer** division, so grids of two emit `0`/`1` texture
/// coordinates and every larger grid emits an all-zero `TEXCOORD_0`; the two source failure
/// modes (a one-vertex grid edge dividing by zero, a non-square grid reading `float[tw, th]`
/// out of range) are explicit errors here instead of runtime exceptions.
pub fn build_terrain_glb_with_limits(
    input: &TerrainMeshInput<'_>,
    limits: GlbBuildLimits,
) -> Result<GlbMeshExport, GlbError> {
    // `int tw = Math.Max(terrain.Heightmap.Width, terrain.Heightmap.Resolution);`
    let grid_width = input.width.max(input.resolution);
    // `int th = Math.Max(terrain.Heightmap.Height, terrain.Heightmap.Resolution);`
    let grid_height = input.height.max(input.resolution);
    // `Vector3 meshScale = terrain.Heightmap.Scale.CastToStruct() * new Vector3(-1, 1, 1);`
    let mesh_scale = [-input.scale[0], input.scale[1], input.scale[2]];
    // `Vector2 uvScale = new Vector2(1 / (tw - 1), 1 / (th - 1));` — integer division.
    if grid_width - 1 == 0 || grid_height - 1 == 0 {
        return Err(GlbError::TerrainUvScaleDivisionByZero {
            width: input.width,
            height: input.height,
            resolution: input.resolution,
        });
    }
    let uv_scale = [
        (1i32 / (grid_width - 1)) as f32,
        (1i32 / (grid_height - 1)) as f32,
    ];
    // `int w = th; int h = tw;`
    let w = grid_height;
    let h = grid_width;
    // `GetHeights` reads `heights[x + y * width]` over a `float[tw, th]` array.
    let required = (grid_width.max(0) as u64).saturating_mul(grid_height.max(0) as u64);
    if (input.heights.len() as u64) < required {
        return Err(GlbError::TerrainHeightsTooShort {
            required,
            available: input.heights.len(),
        });
    }
    // The vertex loop reads `tData[x, y]` for `x < th` and `y < tw`: the source only stays in
    // bounds while the grid is square.
    if grid_width != grid_height {
        return Err(GlbError::TerrainGridNotSquare {
            grid_width,
            grid_height,
        });
    }
    let height_at = |x: i32, y: i32| -> f32 {
        input.heights[(x + y * grid_width) as usize] as f32 / f32::from(TERRAIN_HEIGHT_DIVISOR)
    };
    let vertex_count = (w.max(0) as usize).saturating_mul(h.max(0) as usize);
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(vertex_count);
    let mut tex_coords: Vec<[f32; 2]> = Vec::with_capacity(vertex_count);
    for y in 0..h {
        for x in 0..w {
            // `new Vector3(-(startY + y), tData[startX + x, startY + y], startX + x) * meshScale`
            // with both starts at zero. The negation happens on the integer first, so a zero row
            // produces `0.0 * -1.0 = -0.0`.
            let negated_row = (-(y)) as f32;
            positions.push([
                negated_row * mesh_scale[0],
                height_at(x, y) * mesh_scale[1],
                (x as f32) * mesh_scale[2],
            ]);
            tex_coords.push([(x as f32) * uv_scale[0], (y as f32) * uv_scale[1]]);
        }
    }
    let mut order: Vec<u32> = Vec::new();
    for y in 0..h - 1 {
        for x in 0..w - 1 {
            let base = y * w + x;
            order.push(base as u32);
            order.push((base + 1) as u32);
            order.push((base + w) as u32);
            order.push((base + w) as u32);
            order.push((base + 1) as u32);
            order.push((base + w + 1) as u32);
        }
    }
    finish_single_mesh(
        input.name,
        &positions,
        Some(&tex_coords),
        &order,
        TRIANGLES_MODE,
        limits,
    )
}

/// One `HeightMeshData` of a `NavMeshData` asset.
#[derive(Clone, Copy, Debug)]
pub struct NavMeshHeightMesh<'a> {
    /// `HeightMeshData.Indices`, consumed in triples.
    pub indices: &'a [i32],
    /// `HeightMeshData.Vertices`.
    pub vertices: &'a [[f32; 3]],
}

/// `GlbNavMeshExporter.AddAssetToSceneBuilder`: one node named after the asset carrying a single
/// triangulated mesh with only `POSITION`.
///
/// Every triangle is emitted with its winding reversed (`indices[i + 2], indices[i + 1],
/// indices[i]`) and every vertex negated on X, matching `FromVector`. An index count that is not
/// a multiple of three is the source's `Exception("Height mesh must be triangles.")`.
pub fn build_navmesh_glb(
    name: &str,
    height_meshes: &[NavMeshHeightMesh<'_>],
) -> Result<GlbMeshExport, GlbError> {
    build_navmesh_glb_with_limits(name, height_meshes, GlbBuildLimits::default())
}

/// `GlbNavMeshExporter` with a bound on the emitted container size.
pub fn build_navmesh_glb_with_limits(
    name: &str,
    height_meshes: &[NavMeshHeightMesh<'_>],
    limits: GlbBuildLimits,
) -> Result<GlbMeshExport, GlbError> {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut order: Vec<u32> = Vec::new();
    let mut emitted_source_indices: Vec<u32> = Vec::new();
    for (mesh_index, mesh) in height_meshes.iter().enumerate() {
        if !mesh.indices.len().is_multiple_of(3) {
            return Err(GlbError::NavMeshIndexNotTriangles {
                mesh_index,
                index_count: mesh.indices.len(),
            });
        }
        for (entry, &index) in mesh.indices.iter().enumerate() {
            if index < 0 || index as usize >= mesh.vertices.len() {
                return Err(GlbError::NavMeshIndexOutOfRange {
                    mesh_index,
                    entry,
                    index,
                    vertex_count: mesh.vertices.len(),
                });
            }
        }
        for triangle in mesh.indices.chunks_exact(3) {
            // `primitiveBuilder.AddTriangle(FromVector(vertices[i + 2]),
            //  FromVector(vertices[i + 1]), FromVector(vertices[i]))`
            for &index in [triangle[2], triangle[1], triangle[0]].iter() {
                let vertex = mesh.vertices[index as usize];
                positions.push([-vertex[0], vertex[1], vertex[2]]);
                order.push(positions.len() as u32 - 1);
                emitted_source_indices.push(index as u32);
            }
        }
    }
    let mut export = finish_single_mesh(name, &positions, None, &order, TRIANGLES_MODE, limits)?;
    if let Some(source_indices) = export.emitted_source_indices.first_mut() {
        *source_indices = emitted_source_indices;
    }
    Ok(export)
}

/// The glTF primitive mode of a triangle list.
const TRIANGLES_MODE: u32 = 4;

/// Assembles a single-node, single-mesh GLB from caller-supplied source vertices and indices.
///
/// Vertex deduplication matches `VertexBuilder.CreateCompatibleMesh` (bit-identical attributes
/// share a vertex, first appearance wins), so the emitted vertex order is the first-use order.
fn finish_single_mesh(
    node_name: &str,
    positions: &[[f32; 3]],
    tex_coords: Option<&[[f32; 2]]>,
    order: &[u32],
    mode: u32,
    limits: GlbBuildLimits,
) -> Result<GlbMeshExport, GlbError> {
    let primitive = dedupe_source_vertices(node_name, positions, tex_coords, order, mode)?;
    let mut builder = GlbBuilder::with_limit(limits.max_output_bytes);
    let mut primitive_jsons: Vec<String> = Vec::new();
    if !primitive.indices.is_empty() {
        primitive_jsons.push(emit_position_primitive(
            &mut builder,
            node_name,
            &primitive,
        )?);
    }
    let json = build_json_single_mesh(
        node_name,
        &primitive_jsons,
        &builder.accessors,
        &builder.buffer_views,
        builder.bin.len(),
    );
    let total = builder.bin.len() as u64 + json.len() as u64;
    if total > limits.max_output_bytes {
        return Err(GlbError::OutputTooLarge {
            byte_count: total,
            limit: limits.max_output_bytes,
        });
    }
    Ok(GlbMeshExport {
        bytes: wrap_glb(&json, &builder.bin),
        emitted_source_indices: vec![primitive.source_indices],
        warnings: primitive.warnings,
    })
}

/// Deduplicates caller-supplied source vertices into a triangle-list primitive.
fn dedupe_source_vertices(
    mesh_name: &str,
    positions: &[[f32; 3]],
    tex_coords: Option<&[[f32; 2]]>,
    order: &[u32],
    mode: u32,
) -> Result<Primitive, GlbError> {
    let mut vertices: Vec<GlbVertex> = Vec::new();
    let mut lookup: HashMap<VertexKey, u32> = HashMap::new();
    let mut indices: Vec<u32> = Vec::with_capacity(order.len());
    for &source_index in order {
        let Some(position) = positions.get(source_index as usize).copied() else {
            return Err(GlbError::VertexIndexOutOfRange {
                mesh: mesh_name.to_owned(),
                index: source_index,
                vertex_count: positions.len(),
            });
        };
        for (component_index, component) in position.iter().enumerate() {
            if !component.is_finite() {
                return Err(GlbError::NonFinitePosition {
                    mesh: mesh_name.to_owned(),
                    vertex: source_index,
                    component: match component_index {
                        0 => "x",
                        1 => "y",
                        _ => "z",
                    },
                });
            }
        }
        let vertex = GlbVertex {
            position,
            normal: None,
            tangent: None,
            tex_coords: tex_coords
                .and_then(|uvs| uvs.get(source_index as usize))
                .map(|uv| vec![*uv])
                .unwrap_or_default(),
            color: None,
            joints: None,
            weights: None,
        };
        let key = vertex.key();
        let slot = *lookup.entry(key).or_insert_with(|| {
            let slot = vertices.len() as u32;
            vertices.push(vertex);
            slot
        });
        indices.push(slot);
    }
    Ok(Primitive {
        indices,
        source_indices: order.to_vec(),
        vertices,
        mode,
        warnings: Vec::new(),
    })
}

/// Emits one `POSITION` (+ optional `TEXCOORD_0..N`) triangle-list primitive.
fn emit_position_primitive(
    builder: &mut GlbBuilder,
    mesh_name: &str,
    primitive: &Primitive,
) -> Result<String, GlbError> {
    let positions: Vec<[f32; 3]> = primitive
        .vertices
        .iter()
        .map(|vertex| vertex.position)
        .collect();
    let mut attributes = vec![(
        "POSITION".to_owned(),
        builder.push_position_accessor(&positions, mesh_name)?,
    )];
    let tex_coord_count = primitive
        .vertices
        .first()
        .map_or(0, |vertex| vertex.tex_coords.len());
    for channel in 0..tex_coord_count {
        let mut values = Vec::with_capacity(primitive.vertices.len() * 2);
        for vertex in &primitive.vertices {
            values.extend_from_slice(
                vertex
                    .tex_coords
                    .get(channel)
                    .copied()
                    .unwrap_or([0.0, 0.0])
                    .as_slice(),
            );
        }
        attributes.push((
            format!("TEXCOORD_{channel}"),
            builder.push_vertex_accessor(&values, "VEC2")?,
        ));
    }
    let index_accessor = builder.push_index_accessor(&primitive.indices)?;
    let attribute_json = attributes
        .iter()
        .map(|(name, accessor)| format!("\"{name}\":{accessor}"))
        .collect::<Vec<_>>()
        .join(",");
    Ok(format!(
        "{{\"attributes\":{{{attribute_json}}},\"indices\":{index_accessor},\
         \"material\":0,\"mode\":{}}}",
        primitive.mode
    ))
}

/// The glTF JSON document for the single-node builders (terrain and navmesh).
fn build_json_single_mesh(
    node_name: &str,
    primitives: &[String],
    accessors: &[String],
    buffer_views: &[String],
    bin_length: usize,
) -> String {
    let mut json = String::new();
    json.push_str("{\"asset\":{\"version\":\"2.0\",\"generator\":");
    json.push_str(&escape_json(GENERATOR));
    json.push_str("},\"scene\":0,\"scenes\":[{\"nodes\":[0]}]");
    if primitives.is_empty() {
        json.push_str(&format!(
            ",\"nodes\":[{{\"name\":\"{}\"}}]",
            escape_json(node_name)
        ));
    } else {
        json.push_str(&format!(
            ",\"nodes\":[{{\"name\":\"{}\",\"mesh\":0}}]",
            escape_json(node_name)
        ));
        json.push_str(&format!(
            ",\"meshes\":[{{\"name\":\"\",\"primitives\":[{}]}}]",
            primitives.join(",")
        ));
        json.push_str(&format!(
            ",\"materials\":[{{\"name\":\"{DEFAULT_MATERIAL_NAME}\",\"pbrMetallicRoughness\":\
             {{\"baseColorFactor\":[1.0,1.0,1.0,1.0],\"metallicFactor\":1.0,\"roughnessFactor\":1.0}}}}]"
        ));
    }
    json.push_str(&format!(",\"accessors\":[{}]", accessors.join(",")));
    json.push_str(&format!(",\"bufferViews\":[{}]", buffer_views.join(",")));
    json.push_str(&format!(",\"buffers\":[{{\"byteLength\":{bin_length}}}]}}"));
    json
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::numerics::BoneWeight4;
    use crate::resources::sub_mesh_data::SubMeshData;

    fn vec3(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3 { x, y, z }
    }

    /// A `SubMeshData` covering `index_count` indices from `first_index`.
    fn sub_mesh(topology: MeshTopology, first_index: i32, index_count: i32) -> SubMeshData {
        SubMeshData {
            base_vertex: 0,
            first_index,
            first_vertex: 0,
            index_count,
            triangle_count: index_count / 3,
            vertex_count: 0,
            topology,
            local_bounds: Default::default(),
        }
    }

    fn triangle_mesh() -> MeshData {
        MeshData {
            vertices: vec![
                vec3(0.0, 0.0, 0.0),
                vec3(1.0, 0.0, 0.0),
                vec3(0.0, 1.0, 0.0),
            ],
            normals: None,
            tangents: None,
            colors: None,
            uv0: None,
            uv1: None,
            uv2: None,
            uv3: None,
            uv4: None,
            uv5: None,
            uv6: None,
            uv7: None,
            skin: None,
            bind_pose: None,
            processed_index_buffer: vec![0, 1, 2],
            sub_meshes: vec![sub_mesh(MeshTopology::TRIANGLES, 0, 3)],
        }
    }

    #[test]
    fn coordinate_conversion_matches_the_source_scales() {
        let converted = to_gltf_vector3(vec3(1.0, 2.0, 3.0));
        assert_eq!([converted.x, converted.y, converted.z], [-1.0, 2.0, 3.0]);
        let tangent = to_gltf_tangent(Vec4::new(1.0, 2.0, 3.0, 1.0));
        assert_eq!(
            [tangent.x, tangent.y, tangent.z, tangent.w],
            [-1.0, 2.0, 3.0, -1.0]
        );
        let rotation = to_gltf_quaternion(Quat {
            x: 1.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        });
        assert_eq!(
            [rotation.x, rotation.y, rotation.z, rotation.w],
            [-1.0, 0.0, 0.0, 1.0]
        );
    }

    #[test]
    fn mesh_type_follows_the_source_precedence() {
        let mesh = triangle_mesh();
        assert_eq!(get_mesh_type(&mesh).bits(), 0);

        let mut mesh = triangle_mesh();
        mesh.normals = Some(vec![Vec3::default(); 3]);
        assert!(get_mesh_type(&mesh).contains(GlbMeshType::NORMAL));
        assert!(!get_mesh_type(&mesh).contains(GlbMeshType::TANGENT));

        mesh.tangents = Some(vec![Vec4::default(); 3]);
        assert!(get_mesh_type(&mesh).contains(GlbMeshType::TANGENT));

        mesh.colors = Some(vec![
            crate::numerics::ColorFloat::new(1.0, 1.0, 1.0, 1.0);
            3
        ]);
        assert!(get_mesh_type(&mesh).contains(GlbMeshType::COLOR1));

        mesh.skin = Some(vec![BoneWeight4::new(1.0, 0.0, 0.0, 0.0, 0, 0, 0, 0); 3]);
        assert!(get_mesh_type(&mesh).contains(GlbMeshType::JOINTS4));
    }

    #[test]
    fn triangles_are_wound_in_reverse() {
        let mesh = triangle_mesh();
        let exported = build_glb("Tri", &mesh).unwrap();
        assert!(exported.warnings.is_empty());
        assert_eq!(exported.emitted_source_indices, vec![vec![2, 1, 0]]);
        let text = String::from_utf8_lossy(&exported.bytes);
        assert!(text.contains("\"version\":\"2.0\""));
        assert!(text.contains("SubMesh_0"));
        assert!(text.contains("\"mode\":4"));
        // Small meshes use unsigned-short indices, matching the SharpGLTF writer.
        assert!(text.contains("\"componentType\":5123"));
    }

    #[test]
    fn out_of_range_sub_mesh_is_an_error() {
        let mut mesh = triangle_mesh();
        mesh.sub_meshes[0].index_count = 9;
        match build_glb("Bad", &mesh) {
            Err(GlbError::IndexOutOfRange {
                sub_mesh,
                required_length,
                available_length,
                ..
            }) => {
                assert_eq!(sub_mesh, 0);
                assert_eq!(required_length, 9);
                assert_eq!(available_length, 3);
            }
            other => panic!("expected an out-of-range error, got {other:?}"),
        }
    }

    #[test]
    fn partial_trailing_triangle_is_reported() {
        let mut mesh = triangle_mesh();
        mesh.processed_index_buffer = vec![0, 1, 2, 0];
        mesh.sub_meshes[0].index_count = 4;
        let exported = build_glb("Partial", &mesh).unwrap();
        assert_eq!(exported.warnings.len(), 1);
        assert!(exported.warnings[0].contains("trailing indices"));
    }

    #[test]
    fn triangle_strip_is_de_stripified_with_alternating_winding() {
        let mut mesh = triangle_mesh();
        mesh.vertices = (0..5).map(|value| vec3(value as f32, 0.0, 0.0)).collect();
        mesh.processed_index_buffer = vec![0, 1, 2, 3, 4];
        mesh.sub_meshes = vec![sub_mesh(MeshTopology::TRIANGLE_STRIP, 0, 5)];
        let primitive = build_primitive(&mesh, "Strip", 0).unwrap();
        assert_eq!(primitive.source_indices, vec![2, 1, 0, 2, 3, 1, 4, 3, 2]);
        assert_eq!(primitive.vertices.len(), 5);
        assert_eq!(primitive.mode, 4);
        assert!(primitive.warnings.is_empty());
    }

    #[test]
    fn degenerate_strip_triangles_are_skipped() {
        let mut mesh = triangle_mesh();
        mesh.vertices = (0..3).map(|value| vec3(value as f32, 0.0, 0.0)).collect();
        // The (2,2,1) window is degenerate; the (2,1,0) window is not.
        mesh.processed_index_buffer = vec![0, 1, 2, 2];
        mesh.sub_meshes = vec![sub_mesh(MeshTopology::TRIANGLE_STRIP, 0, 4)];
        let primitive = build_primitive(&mesh, "Degenerate", 0).unwrap();
        assert_eq!(primitive.source_indices, vec![2, 1, 0]);
    }

    #[test]
    fn fully_degenerate_strip_emits_nothing() {
        let mut mesh = triangle_mesh();
        mesh.vertices = (0..3).map(|value| vec3(value as f32, 0.0, 0.0)).collect();
        mesh.processed_index_buffer = vec![0, 1, 1, 2];
        mesh.sub_meshes = vec![sub_mesh(MeshTopology::TRIANGLE_STRIP, 0, 4)];
        let primitive = build_primitive(&mesh, "Degenerate", 0).unwrap();
        assert!(primitive.source_indices.is_empty());
    }

    #[test]
    fn quads_emit_two_triangles_each() {
        let mut mesh = triangle_mesh();
        mesh.vertices = (0..4).map(|value| vec3(value as f32, 0.0, 0.0)).collect();
        mesh.processed_index_buffer = vec![0, 1, 2, 3];
        mesh.sub_meshes = vec![sub_mesh(MeshTopology::QUADS, 0, 4)];
        let primitive = build_primitive(&mesh, "Quads", 0).unwrap();
        assert_eq!(primitive.source_indices, vec![3, 2, 1, 3, 1, 0]);
        assert_eq!(primitive.mode, 4);
    }

    #[test]
    fn lines_are_emitted_in_reverse_pair_order() {
        let mut mesh = triangle_mesh();
        mesh.vertices = (0..4).map(|value| vec3(value as f32, 0.0, 0.0)).collect();
        mesh.processed_index_buffer = vec![0, 1, 2, 3];
        mesh.sub_meshes = vec![sub_mesh(MeshTopology::LINES, 0, 4)];
        let primitive = build_primitive(&mesh, "Lines", 0).unwrap();
        assert_eq!(primitive.source_indices, vec![1, 0, 3, 2]);
        assert_eq!(primitive.mode, 1);
    }

    #[test]
    fn line_strip_becomes_a_line_list() {
        let mut mesh = triangle_mesh();
        mesh.vertices = (0..4).map(|value| vec3(value as f32, 0.0, 0.0)).collect();
        mesh.processed_index_buffer = vec![0, 1, 2, 3];
        mesh.sub_meshes = vec![sub_mesh(MeshTopology::LINE_STRIP, 0, 4)];
        let primitive = build_primitive(&mesh, "LineStrip", 0).unwrap();
        assert_eq!(primitive.source_indices, vec![1, 0, 2, 1, 3, 2]);
        assert_eq!(primitive.mode, 1);
    }

    #[test]
    fn points_are_emitted_in_order() {
        let mut mesh = triangle_mesh();
        mesh.vertices = (0..3).map(|value| vec3(value as f32, 0.0, 0.0)).collect();
        mesh.processed_index_buffer = vec![2, 0, 1];
        mesh.sub_meshes = vec![sub_mesh(MeshTopology::POINTS, 0, 3)];
        let primitive = build_primitive(&mesh, "Points", 0).unwrap();
        assert_eq!(primitive.source_indices, vec![2, 0, 1]);
        assert_eq!(primitive.mode, 0);
    }

    #[test]
    fn triangles_are_wound_in_reverse_source_order() {
        let mut mesh = triangle_mesh();
        mesh.processed_index_buffer = vec![0, 1, 2, 2, 1, 3];
        mesh.vertices = (0..4).map(|value| vec3(value as f32, 0.0, 0.0)).collect();
        mesh.sub_meshes = vec![sub_mesh(MeshTopology::TRIANGLES, 0, 6)];
        let primitive = build_primitive(&mesh, "Tris", 0).unwrap();
        assert_eq!(primitive.source_indices, vec![2, 1, 0, 3, 1, 2]);
    }

    #[test]
    fn unsupported_topology_is_an_error() {
        let mut mesh = triangle_mesh();
        mesh.sub_meshes = vec![sub_mesh(MeshTopology(9), 0, 3)];
        match build_glb("Weird", &mesh) {
            Err(GlbError::UnsupportedTopology { topology, .. }) => assert_eq!(topology, 9),
            other => panic!("expected an unsupported topology error, got {other:?}"),
        }
    }

    #[test]
    fn non_finite_positions_are_rejected() {
        let mut mesh = triangle_mesh();
        mesh.vertices[1].x = f32::NAN;
        match build_glb("NaN", &mesh) {
            Err(GlbError::NonFinitePosition { component, .. }) => assert_eq!(component, "x"),
            other => panic!("expected a non-finite position error, got {other:?}"),
        }
    }

    #[test]
    fn empty_mesh_is_rejected() {
        let mut mesh = triangle_mesh();
        mesh.vertices.clear();
        assert_eq!(build_glb("Empty", &mesh), Err(GlbError::NoVertices));
    }

    #[test]
    fn glb_container_is_well_formed() {
        let mesh = triangle_mesh();
        let exported = build_glb("Container", &mesh).unwrap();
        let bytes = &exported.bytes;
        assert_eq!(&bytes[0..4], b"glTF");
        assert_eq!(
            u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
            2,
            "version"
        );
        assert_eq!(
            u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize,
            bytes.len(),
            "total length"
        );
        let json_length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        assert_eq!(&bytes[16..20], b"JSON");
        assert_eq!(json_length % 4, 0, "JSON chunk is 4-byte aligned");
        let bin_header = 20 + json_length;
        assert_eq!(
            &bytes[bin_header + 4..bin_header + 8],
            b"BIN\0",
            "binary chunk type"
        );
        let bin_length =
            u32::from_le_bytes(bytes[bin_header..bin_header + 4].try_into().unwrap()) as usize;
        assert_eq!(bin_length % 4, 0, "binary chunk is 4-byte aligned");
        assert_eq!(bin_header + 8 + bin_length, bytes.len());
    }

    #[test]
    fn emitted_index_counts_match_the_topology_rewrite() {
        assert_eq!(emitted_index_count(MeshTopology::TRIANGLES, 6), 6);
        assert_eq!(emitted_index_count(MeshTopology::TRIANGLES, 7), 6);
        assert_eq!(emitted_index_count(MeshTopology::QUADS, 8), 12);
        assert_eq!(emitted_index_count(MeshTopology::LINES, 4), 4);
        assert_eq!(emitted_index_count(MeshTopology::POINTS, 5), 5);
        assert_eq!(emitted_index_count(MeshTopology::LINE_STRIP, 4), 6);
        assert_eq!(emitted_index_count(MeshTopology::LINE_STRIP, 1), 0);
        assert_eq!(emitted_index_count(MeshTopology::TRIANGLE_STRIP, 5), 9);
        assert_eq!(emitted_index_count(MeshTopology::TRIANGLE_STRIP, 2), 0);
    }

    #[test]
    fn identical_vertices_are_shared() {
        let mut mesh = triangle_mesh();
        mesh.processed_index_buffer = vec![0, 0, 0];
        let exported = build_glb("Shared", &mesh).unwrap();
        let text = String::from_utf8_lossy(&exported.bytes);
        // One shared vertex still needs one accessor entry for POSITION.
        assert_eq!(text.matches("\"POSITION\":").count(), 1);
        assert_eq!(text.matches("\"componentType\":5123").count(), 1);
    }

    #[test]
    fn color_channels_quantize_with_truncation() {
        // Probe values from the SharpGLTF writer: 0.5 becomes 127, not 128.
        assert_eq!(quantize_color_channel(0.0), 0);
        assert_eq!(quantize_color_channel(0.25), 63);
        assert_eq!(quantize_color_channel(0.5), 127);
        assert_eq!(quantize_color_channel(0.999), 254);
        assert_eq!(quantize_color_channel(1.0), 255);
        assert_eq!(quantize_color_channel(1.5), 255);
        assert_eq!(quantize_color_channel(-0.5), 0);
        // Any non-finite channel zeroes the whole color, including finite alpha.
        assert_eq!(quantize_color([0.5, 0.5, 0.5, 1.0]), [127, 127, 127, 255]);
        assert_eq!(quantize_color([f32::INFINITY, 0.5, 0.5, 1.0]), [0, 0, 0, 0]);
        assert_eq!(quantize_color([0.5, 0.5, 0.5, f32::NAN]), [0, 0, 0, 0]);
    }

    #[test]
    fn skin_bindings_follow_the_sparse_weight_rules() {
        // Duplicate indices merge by summation, then renormalize to one.
        assert_eq!(
            process_skin_bindings(&[1, 1, 0, 0], &[0.4, 0.6, 0.0, 0.0], "M", 0).unwrap(),
            (Some([1, 0, 0, 0]), Some([1.0, 0.0, 0.0, 0.0]))
        );
        // Exact-zero weights drop out; sub-unit sums scale up.
        assert_eq!(
            process_skin_bindings(&[0, 1, 2, 3], &[0.3, 0.3, 0.0, 0.0], "M", 0).unwrap(),
            (Some([0, 1, 0, 0]), Some([0.5, 0.5, 0.0, 0.0]))
        );
        // An exact unit sum is left untouched.
        assert_eq!(
            process_skin_bindings(&[2, 0, 0, 0], &[1.0, 0.0, 0.0, 0.0], "M", 0).unwrap(),
            (Some([2, 0, 0, 0]), Some([1.0, 0.0, 0.0, 0.0]))
        );
        // An empty set stays zero, mirroring the skipped renormalization.
        assert_eq!(
            process_skin_bindings(&[5, 6, 0, 0], &[0.0, 0.0, 0.0, 0.0], "M", 0).unwrap(),
            (Some([0, 0, 0, 0]), Some([0.0, 0.0, 0.0, 0.0]))
        );
        // Negative indices fail with context.
        assert_eq!(
            process_skin_bindings(&[-1, 0, 0, 0], &[1.0, 0.0, 0.0, 0.0], "M", 7).unwrap_err(),
            GlbError::NegativeBoneIndex {
                mesh: "M".to_owned(),
                vertex: 7,
                bone_index: -1,
            }
        );
        // Indices beyond the USHORT encoding fail with context.
        assert_eq!(
            process_skin_bindings(&[70000, 0, 0, 0], &[1.0, 0.0, 0.0, 0.0], "M", 7).unwrap_err(),
            GlbError::BoneIndexTooLarge {
                mesh: "M".to_owned(),
                vertex: 7,
                bone_index: 70000,
            }
        );
    }

    #[test]
    fn accessor_widths_follow_the_writer_rules() {
        let mut builder = GlbBuilder::default();
        // Colors are normalized unsigned bytes.
        let color = builder
            .push_color_accessor(&[[1.0, 0.0, 0.5, 0.25]])
            .unwrap();
        assert!(builder.accessors[color].contains("\"componentType\":5121"));
        assert!(builder.accessors[color].contains("\"normalized\":true"));
        // Joints fit in a byte while every lane does, and widen otherwise.
        let small = builder.push_joints_accessor(&[[1, 2, 3, 4]]).unwrap();
        assert!(builder.accessors[small].contains("\"componentType\":5121"));
        assert!(!builder.accessors[small].contains("normalized"));
        let big = builder.push_joints_accessor(&[[300, 0, 0, 0]]).unwrap();
        assert!(builder.accessors[big].contains("\"componentType\":5123"));
        // Indices fit in a short below the reserved 65535.
        let short = builder.push_index_accessor(&[0, 1, 65534]).unwrap();
        assert!(builder.accessors[short].contains("\"componentType\":5123"));
        let long = builder.push_index_accessor(&[0, 1, 65535]).unwrap();
        assert!(builder.accessors[long].contains("\"componentType\":5125"));
    }

    #[test]
    fn oversized_output_is_refused_with_context() {
        let mesh = triangle_mesh();
        let error = build_glb_with_limits(
            "Tri",
            &mesh,
            GlbBuildLimits {
                max_output_bytes: 16,
            },
        )
        .unwrap_err();
        assert!(matches!(error, GlbError::OutputTooLarge { limit: 16, .. }));
        assert!(error.to_string().contains("too large to export"));
        // The default cap is the source 2Gb buffer ceiling.
        assert_eq!(
            GlbBuildLimits::default().max_output_bytes,
            2 * 1024 * 1024 * 1024
        );
        build_glb("Tri", &mesh).unwrap();
    }

    #[test]
    fn json_escaping_is_applied_to_names() {
        let mesh = triangle_mesh();
        let exported = build_glb("a\"b\\c", &mesh).unwrap();
        let text = String::from_utf8_lossy(&exported.bytes);
        assert!(text.contains("a\\\"b\\\\c"));
    }

    #[test]
    fn renderer_mesh_dispatch_matches_the_source_gates() {
        // An empty static-batch list forces the dynamic path.
        assert!(references_dynamic_mesh(true, 0, false, 0));
        // An empty subset list forces the dynamic path.
        assert!(references_dynamic_mesh(false, 5, true, 0));
        // Either empty list wins on its own, even beside a non-empty one.
        assert!(references_dynamic_mesh(true, 0, true, 3));
        assert!(references_dynamic_mesh(true, 2, true, 0));
        // Non-empty lists stay on the static path.
        assert!(!references_dynamic_mesh(true, 2, false, 0));
        assert!(!references_dynamic_mesh(false, 5, true, 3));
        assert!(!references_dynamic_mesh(false, 5, false, 9));
        // Explicit subset indices win over the static-batch range.
        assert_eq!(subset_indices(Some(&[2, 0]), Some((0, 3))), vec![2, 0]);
        // Otherwise the batch range expands from its first sub-mesh.
        assert_eq!(subset_indices(None, Some((1, 3))), vec![1, 2, 3]);
        assert_eq!(subset_indices(None, Some((5, 0))), Vec::<u32>::new());
        // Otherwise the renderer contributes no sub-meshes.
        assert!(subset_indices(None, None).is_empty());
    }

    /// The decoded container of a GLB produced by this module.
    struct DecodedGlb {
        json: String,
        bin: Vec<u8>,
    }

    /// Parses the GLB container independently of the writer (magic, version, chunk framing).
    fn decode_glb(bytes: &[u8]) -> DecodedGlb {
        assert!(bytes.len() >= 20, "GLB is shorter than its header");
        assert_eq!(&bytes[0..4], b"glTF");
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 2);
        assert_eq!(
            u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize,
            bytes.len()
        );
        let json_length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        assert_eq!(&bytes[16..20], b"JSON");
        let json = String::from_utf8(bytes[20..20 + json_length].to_vec()).unwrap();
        let offset = 20 + json_length;
        let mut bin = Vec::new();
        if offset + 8 <= bytes.len() {
            let bin_length =
                u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
            assert_eq!(&bytes[offset + 4..offset + 8], b"BIN\0");
            bin = bytes[offset + 8..offset + 8 + bin_length].to_vec();
        }
        DecodedGlb { json, bin }
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

    /// The decoded floats of one accessor, in accessor order.
    fn accessor_floats(glb: &DecodedGlb, accessor: usize) -> Vec<f32> {
        let accessors = json_objects(json_array(&glb.json, "accessors").unwrap());
        let views = json_objects(json_array(&glb.json, "bufferViews").unwrap());
        let object = &accessors[accessor];
        let view = &views[json_number(object, "bufferView").unwrap() as usize];
        let count = json_number(object, "count").unwrap() as usize;
        let components = match json_string(object, "type").unwrap().as_str() {
            "VEC2" => 2,
            "VEC3" => 3,
            _ => 4,
        };
        let offset = json_number(view, "byteOffset").unwrap() as usize;
        (0..count * components)
            .map(|index| {
                let start = offset + index * 4;
                f32::from_le_bytes(glb.bin[start..start + 4].try_into().unwrap())
            })
            .collect()
    }

    /// The decoded indices of one accessor.
    fn accessor_indices(glb: &DecodedGlb, accessor: usize) -> Vec<u32> {
        let accessors = json_objects(json_array(&glb.json, "accessors").unwrap());
        let views = json_objects(json_array(&glb.json, "bufferViews").unwrap());
        let object = &accessors[accessor];
        let view = &views[json_number(object, "bufferView").unwrap() as usize];
        let count = json_number(object, "count").unwrap() as usize;
        let component_type = json_number(object, "componentType").unwrap();
        let offset = json_number(view, "byteOffset").unwrap() as usize;
        (0..count)
            .map(|index| match component_type {
                5123 => {
                    let start = offset + index * 2;
                    u16::from_le_bytes(glb.bin[start..start + 2].try_into().unwrap()) as u32
                }
                _ => {
                    let start = offset + index * 4;
                    u32::from_le_bytes(glb.bin[start..start + 4].try_into().unwrap())
                }
            })
            .collect()
    }

    /// The single primitive body of the single-mesh JSON layout.
    fn single_primitive(json: &str) -> String {
        let anchor = json.find("\"primitives\":[").unwrap();
        let start = anchor + "\"primitives\":[".len();
        let end = json[start..].find(']').unwrap() + start;
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

    /// The reference exception name a `GlbError` stands in for.
    fn reference_exception(error: &GlbError) -> &'static str {
        match error {
            GlbError::TerrainUvScaleDivisionByZero { .. } => "DivideByZeroException",
            GlbError::TerrainGridNotSquare { .. }
            | GlbError::TerrainHeightsTooShort { .. }
            | GlbError::VertexIndexOutOfRange { .. }
            | GlbError::IndexOutOfRange { .. } => "IndexOutOfRangeException",
            GlbError::NavMeshIndexNotTriangles { .. } => "Exception",
            other => panic!("unmapped reference exception for {other}"),
        }
    }

    /// One parsed fixture case.
    #[derive(Debug, Default)]
    struct BuilderCase {
        tag: String,
        nav: bool,
        version: String,
        name: String,
        width: i32,
        height: i32,
        resolution: i32,
        scale: [f32; 3],
        heights: Vec<i16>,
        height_count: usize,
        read_back: (i32, i32, i32),
        nav_meshes: Vec<(Vec<[f32; 3]>, Vec<i32>)>,
        positions: Vec<[f32; 3]>,
        uvs: Vec<[f32; 2]>,
        indices: Vec<u32>,
        has_uv: bool,
        nodes: Vec<String>,
        empty_nodes: Option<usize>,
        error: Option<String>,
    }

    fn parse_f32(text: &str) -> f32 {
        let value: f32 = text.parse().expect("a probe float");
        if value == 0.0 && text.starts_with('-') {
            -0.0
        } else {
            value
        }
    }

    fn parse_csv_i32(text: &str) -> Vec<i32> {
        text.split(',')
            .filter(|part| !part.is_empty())
            .map(|part| part.parse().expect("a probe integer"))
            .collect()
    }

    /// Round-trips `tools/probes/res-glb` through the production builders.
    ///
    /// Every row was produced by the shipped `GlbTerrainBuilder`/`GlbNavMeshExporter` over real
    /// generated objects; the test asserts the container structure, node names, and the decoded
    /// POSITION/TEXCOORD_0/indices bit-for-bit (including the sign of zero).
    #[test]
    fn glb_builders_match_the_reference_runtime() {
        let fixture = include_str!("glb_builders_fixture.txt");
        let mut cases: Vec<BuilderCase> = Vec::new();
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<&str> = line.split('|').collect();
            match fields[0] {
                "GIN" => {
                    let case = BuilderCase {
                        tag: fields[1].to_owned(),
                        nav: false,
                        version: fields[2].to_owned(),
                        width: fields[3].parse().unwrap(),
                        height: fields[4].parse().unwrap(),
                        resolution: fields[5].parse().unwrap(),
                        scale: [
                            parse_f32(fields[6]),
                            parse_f32(fields[7]),
                            parse_f32(fields[8]),
                        ],
                        name: fields[9].to_owned(),
                        height_count: fields[10].parse().unwrap(),
                        ..BuilderCase::default()
                    };
                    cases.push(case);
                }
                "GHTS" => {
                    let case = cases.last_mut().expect("a GHTS row follows a GIN row");
                    case.heights = fields[2]
                        .split(',')
                        .filter(|part| !part.is_empty())
                        .map(|part| part.parse().expect("a probe height"))
                        .collect();
                    assert_eq!(case.heights.len(), case.height_count, "{line}");
                }
                "GREAD" => {
                    let case = cases.last_mut().expect("a GREAD row follows a GIN row");
                    case.read_back = (
                        fields[2].parse().unwrap(),
                        fields[3].parse().unwrap(),
                        fields[4].parse().unwrap(),
                    );
                }
                "NIN" => {
                    cases.push(BuilderCase {
                        tag: fields[1].to_owned(),
                        nav: true,
                        version: fields[2].to_owned(),
                        name: fields[3].to_owned(),
                        ..BuilderCase::default()
                    });
                }
                "NVTS" => {
                    let case = cases.last_mut().expect("an NVTS row follows a NIN row");
                    let index: usize = fields[2].parse().unwrap();
                    let vertices: Vec<[f32; 3]> = fields[3]
                        .split(';')
                        .filter(|part| !part.is_empty())
                        .map(|vertex| {
                            let components: Vec<f32> = vertex.split(',').map(parse_f32).collect();
                            [components[0], components[1], components[2]]
                        })
                        .collect();
                    while case.nav_meshes.len() <= index {
                        case.nav_meshes.push((Vec::new(), Vec::new()));
                    }
                    case.nav_meshes[index].0 = vertices;
                }
                "NIDX" => {
                    let case = cases.last_mut().expect("an NIDX row follows a NIN row");
                    let index: usize = fields[2].parse().unwrap();
                    let indices = parse_csv_i32(fields[3]);
                    case.nav_meshes[index].1 = indices;
                }
                "GMESH" => {
                    let _ = cases.last();
                }
                "GPOS" => {
                    let case = cases.last_mut().unwrap();
                    let index: usize = fields[2].parse().unwrap();
                    assert_eq!(index, case.positions.len(), "{line}");
                    case.positions.push([
                        parse_f32(fields[3]),
                        parse_f32(fields[4]),
                        parse_f32(fields[5]),
                    ]);
                }
                "GUV" => {
                    let case = cases.last_mut().unwrap();
                    let index: usize = fields[2].parse().unwrap();
                    assert_eq!(index, case.uvs.len(), "{line}");
                    case.has_uv = true;
                    case.uvs.push([parse_f32(fields[3]), parse_f32(fields[4])]);
                }
                "GIDX" => {
                    let case = cases.last_mut().unwrap();
                    case.indices = parse_csv_i32(fields[2])
                        .into_iter()
                        .map(|value| value as u32)
                        .collect();
                }
                "GNODE" => {
                    let case = cases.last_mut().unwrap();
                    case.nodes.push(fields[2].to_owned());
                }
                "GEMPTY" => {
                    let case = cases.last_mut().unwrap();
                    case.empty_nodes = Some(
                        fields[3]
                            .strip_prefix("nodes=")
                            .expect("a nodes= count")
                            .parse()
                            .unwrap(),
                    );
                }
                "GERR" => {
                    let case = cases.last_mut().unwrap();
                    if fields[2] == "inner" {
                        case.error = Some(fields[3].to_owned());
                    }
                }
                other => panic!("unexpected fixture row {other}"),
            }
        }
        assert_eq!(cases.len(), 9, "every probed case is asserted");

        let mut mesh_cases = 0;
        for case in &cases {
            if !case.nav {
                match case.version.as_str() {
                    "2019.4.0f1" => assert_eq!(
                        case.read_back,
                        (0, 0, case.resolution),
                        "{} uses the modern Heightmap stubs",
                        case.tag
                    ),
                    "5.0.0f1" => assert_eq!(
                        case.read_back,
                        (case.width, case.height, 0),
                        "{} uses the legacy Heightmap stubs",
                        case.tag
                    ),
                    version => {
                        panic!("unmapped generated Heightmap behavior for {version}")
                    }
                }
            }
            let result = if case.nav {
                let meshes: Vec<NavMeshHeightMesh<'_>> = case
                    .nav_meshes
                    .iter()
                    .map(|(vertices, indices)| NavMeshHeightMesh { indices, vertices })
                    .collect();
                build_navmesh_glb(&case.name, &meshes)
            } else {
                let input = TerrainMeshInput {
                    name: &case.name,
                    width: case.read_back.0,
                    height: case.read_back.1,
                    resolution: case.read_back.2,
                    scale: case.scale,
                    heights: &case.heights,
                };
                build_terrain_glb(&input)
            };
            match (&case.error, result) {
                (Some(exception), Err(error)) => {
                    assert_eq!(reference_exception(&error), *exception, "{}", case.tag);
                }
                (Some(exception), Ok(_)) => {
                    panic!("{} should fail like the reference {exception}", case.tag)
                }
                (None, Err(error)) => panic!("{} failed: {error}", case.tag),
                (None, Ok(export)) => {
                    let glb = decode_glb(&export.bytes);
                    assert_eq!(
                        json_array(&glb.json, "nodes")
                            .unwrap()
                            .matches("\"name\"")
                            .count(),
                        case.empty_nodes.unwrap_or(1),
                        "{}; GLB JSON: {}",
                        case.tag,
                        glb.json
                    );
                    assert!(glb.json.contains(&format!("\"name\":\"{}\"", case.name)));
                    if case.empty_nodes.is_some() {
                        assert_eq!(export.emitted_source_indices.len(), 1);
                        assert!(export.emitted_source_indices[0].is_empty());
                        assert!(!glb.json.contains("\"meshes\""));
                        assert!(glb.bin.is_empty());
                        continue;
                    }
                    assert_eq!(export.emitted_source_indices.len(), 1);
                    mesh_cases += 1;
                    let primitive = single_primitive(&glb.json);
                    let position_accessor =
                        attribute_accessor(&primitive, "POSITION").expect("a POSITION accessor");
                    let positions = accessor_floats(&glb, position_accessor);
                    assert_eq!(positions.len(), case.positions.len() * 3, "{}", case.tag);
                    for (index, expected) in case.positions.iter().enumerate() {
                        for axis in 0..3 {
                            assert_eq!(
                                positions[index * 3 + axis].to_bits(),
                                expected[axis].to_bits(),
                                "{} POSITION vertex {index} axis {axis}",
                                case.tag
                            );
                        }
                    }
                    assert_eq!(
                        attribute_accessor(&primitive, "TEXCOORD_0").is_some(),
                        case.has_uv,
                        "{}",
                        case.tag
                    );
                    if case.has_uv {
                        let uv_accessor = attribute_accessor(&primitive, "TEXCOORD_0").unwrap();
                        let uvs = accessor_floats(&glb, uv_accessor);
                        assert_eq!(uvs.len(), case.uvs.len() * 2, "{}", case.tag);
                        for (index, expected) in case.uvs.iter().enumerate() {
                            for axis in 0..2 {
                                assert_eq!(
                                    uvs[index * 2 + axis].to_bits(),
                                    expected[axis].to_bits(),
                                    "{} TEXCOORD_0 vertex {index} axis {axis}",
                                    case.tag
                                );
                            }
                        }
                    }
                    let index_accessor =
                        json_number(&primitive, "indices").expect("an index accessor") as usize;
                    assert_eq!(
                        accessor_indices(&glb, index_accessor),
                        case.indices,
                        "{}",
                        case.tag
                    );
                    if case.nav {
                        assert_eq!(
                            export.emitted_source_indices[0]
                                .iter()
                                .map(|value| *value as usize)
                                .collect::<Vec<_>>(),
                            case.positions_source_order(),
                            "{}",
                            case.tag
                        );
                    }
                }
            }
        }
        assert_eq!(
            mesh_cases, 5,
            "four terrain and one navmesh case carry geometry"
        );
    }

    impl BuilderCase {
        /// The source vertex index sequence the reference's AddTriangle calls produced.
        fn positions_source_order(&self) -> Vec<usize> {
            self.nav_meshes
                .iter()
                .flat_map(|(_, indices)| {
                    indices
                        .chunks_exact(3)
                        .flat_map(|triangle| {
                            [triangle[2], triangle[1], triangle[0]]
                                .into_iter()
                                .map(|value| value as usize)
                        })
                        .collect::<Vec<_>>()
                })
                .collect()
        }
    }
}
