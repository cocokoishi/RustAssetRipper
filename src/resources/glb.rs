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
}
