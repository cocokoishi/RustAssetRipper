//! `CompressedMesh` geometry decompression ported from `CompressedMeshExtensions`.
//!
//! This ports the channels whose behavior the snapshot fully determines: vertices, normals,
//! tangents, triangle indices, and bone weights. UV channels (`UVInfo`), colors
//! (`ColorFloat`/`Color32`), and bind poses are separate follow-ups.
//!
//! Notable source behaviors reproduced exactly:
//!
//! - `GetNormals`/`GetTangents` reconstruct the third component as `sqrt(1 - x^2 - y^2)`; when
//!   that is negative they fall back to `Vector3.Normalize(x, y, 0)` and overwrite all three
//!   components. A separate sign array flips the reconstructed component, and for tangents a
//!   second sign becomes `w = -1` or `1`.
//! - Weights are 31-scaled integers; the fourth weight is derived from `31 - sum`, and a vertex
//!   whose running sum reaches 31 stops early.
//! - Because the integer bit vector masks `BitSize == 32` to zero, a triangle list containing
//!   `0xFFFFFFFF` decompresses to all zeros — confirmed by the reference probe in
//!   `Temp/opencode/meshprobe`.
//!
//! All reads and writes are bounded by [`PackedBitVectorLimits`].

use std::fmt::{self, Display, Formatter};

use crate::numerics::BoneWeight4;

use super::packed_bit_vector::{PackedBitVector, PackedBitVectorError, PackedBitVectorLimits};
use super::packed_float_vector::{PackedBitVectorSingle, PackedFloatVectorError};

/// A `System.Numerics.Vector2`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    /// `X`.
    pub x: f32,
    /// `Y`.
    pub y: f32,
}

impl Vec2 {
    /// Build from components.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// A `System.Numerics.Vector3`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    /// `X`.
    pub x: f32,
    /// `Y`.
    pub y: f32,
    /// `Z`.
    pub z: f32,
}

impl Vec3 {
    /// Build from components.
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
}

/// A `System.Numerics.Vector4`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec4 {
    /// `X`.
    pub x: f32,
    /// `Y`.
    pub y: f32,
    /// `Z`.
    pub z: f32,
    /// `W`.
    pub w: f32,
}

impl Vec4 {
    /// Build from components.
    pub const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self { x, y, z, w }
    }
}

/// Errors returned by compressed-mesh decoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompressedMeshError {
    /// A packed float vector failed to unpack.
    Float(PackedFloatVectorError),
    /// A packed integer vector failed to pack or unpack.
    Integer(PackedBitVectorError),
    /// The float array length is not a multiple of the element dimension.
    InvalidFloatArrayLength { length: usize, dimension: usize },
    /// The element dimension is outside `1..=4`.
    InvalidDimension { dimension: usize },
    /// A bounded allocation could not be reserved.
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    /// A sign or weight array was shorter than the data required.
    MissingElementData { needed: usize, available: usize },
    /// The two-channel UV fast path requires equal-length channel arrays.
    MismatchedUvLengths { first: usize, second: usize },
    /// A UV channel index is outside `0..8`.
    InvalidUvChannel { channel: usize },
    /// A UV channel dimension is outside `1..=4`.
    InvalidUvDimension { dimension: usize },
}

impl Display for CompressedMeshError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Float(source) => {
                write!(formatter, "compressed mesh float vector error: {source}")
            }
            Self::Integer(source) => {
                write!(formatter, "compressed mesh integer vector error: {source}")
            }
            Self::InvalidFloatArrayLength { length, dimension } => write!(
                formatter,
                "float array length {length} is not a multiple of dimension {dimension}"
            ),
            Self::InvalidDimension { dimension } => {
                write!(formatter, "element dimension {dimension} is outside 1..=4")
            }
            Self::AllocationFailed {
                resource,
                requested,
            } => write!(formatter, "could not allocate {requested} for {resource}"),
            Self::MissingElementData { needed, available } => write!(
                formatter,
                "compressed mesh needed {needed} elements but only {available} were available"
            ),
            Self::MismatchedUvLengths { first, second } => write!(
                formatter,
                "UV channels must be the same length, got {first} and {second}"
            ),
            Self::InvalidUvChannel { channel } => {
                write!(formatter, "UV channel {channel} is outside 0..8")
            }
            Self::InvalidUvDimension { dimension } => {
                write!(formatter, "UV dimension {dimension} is outside 1..=4")
            }
        }
    }
}

impl std::error::Error for CompressedMeshError {}

impl From<PackedFloatVectorError> for CompressedMeshError {
    fn from(source: PackedFloatVectorError) -> Self {
        Self::Float(source)
    }
}

impl From<PackedBitVectorError> for CompressedMeshError {
    fn from(source: PackedBitVectorError) -> Self {
        Self::Integer(source)
    }
}

/// The geometry channels of a `CompressedMesh`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CompressedMesh {
    /// `Vertices`.
    pub vertices: PackedBitVectorSingle,
    /// `Normals`.
    pub normals: PackedBitVectorSingle,
    /// `Tangents`.
    pub tangents: PackedBitVectorSingle,
    /// `Weights`.
    pub weights: PackedBitVector,
    /// `BoneIndices`.
    pub bone_indices: PackedBitVector,
    /// `NormalSigns`.
    pub normal_signs: PackedBitVector,
    /// `TangentSigns`.
    pub tangent_signs: PackedBitVector,
    /// `Triangles`.
    pub triangles: PackedBitVector,
    /// `UV`.
    pub uv: PackedBitVectorSingle,
    /// `UVInfo` (four bits per channel).
    pub uv_info: u32,
    /// `Has_UVInfo()`, supplied by the caller because it is a generated version gate.
    pub has_uv_info: bool,
    /// `BindPoses`.
    pub bind_poses: PackedBitVectorSingle,
    /// `Has_BindPoses()`, a generated gate that is true only before Unity 5.
    pub has_bind_poses: bool,
}

impl CompressedMesh {
    /// `IsSet`.
    pub const fn is_set(&self) -> bool {
        self.vertices.num_items > 0
    }

    /// `GetVertices`.
    pub fn get_vertices(
        &self,
        limits: PackedBitVectorLimits,
    ) -> Result<Vec<Vec3>, CompressedMeshError> {
        let data = self.vertices.unpack_chunked(3, 3, 0, None, limits)?;
        float_array_to_vector3(&data, 3)
    }

    /// `SetVertices`.
    pub fn set_vertices(
        &mut self,
        vertices: &[Vec3],
        limits: PackedBitVectorLimits,
    ) -> Result<(), CompressedMeshError> {
        let floats = vector3_to_float_array(vertices);
        self.vertices.pack(&floats, limits)?;
        Ok(())
    }

    /// `GetNormals`.
    pub fn get_normals(
        &self,
        limits: PackedBitVectorLimits,
    ) -> Result<Vec<Vec3>, CompressedMeshError> {
        let data = self.normals.unpack_chunked(2, 2, 0, None, limits)?;
        let signs = self.normal_signs.unpack_ints(limits)?;
        let count = self.normals.num_items as usize / 2;
        if signs.len() < count {
            return Err(CompressedMeshError::MissingElementData {
                needed: count,
                available: signs.len(),
            });
        }
        let mut normals = Vec::new();
        normals
            .try_reserve_exact(count)
            .map_err(|_| CompressedMeshError::AllocationFailed {
                resource: "decompressed normals",
                requested: count,
            })?;
        for i in 0..count {
            let mut x = data[i * 2];
            let mut y = data[i * 2 + 1];
            let zsqr = 1.0 - x * x - y * y;
            let mut z = if zsqr >= 0.0 {
                zsqr.sqrt()
            } else {
                let normalized = normalize(Vec3::new(x, y, 0.0));
                x = normalized.x;
                y = normalized.y;
                normalized.z
            };
            if signs[i] == 0 {
                z = -z;
            }
            normals.push(Vec3::new(x, y, z));
        }
        Ok(normals)
    }

    /// `SetNormals`.
    pub fn set_normals(
        &mut self,
        normals: &[Vec3],
        limits: PackedBitVectorLimits,
    ) -> Result<(), CompressedMeshError> {
        let (floats, signs) = make_float_and_sign_arrays_vec3(normals);
        self.normals.pack(&floats, limits)?;
        self.normal_signs.pack_uints(&signs, limits)?;
        Ok(())
    }

    /// `GetTangents`.
    pub fn get_tangents(
        &self,
        limits: PackedBitVectorLimits,
    ) -> Result<Vec<Vec4>, CompressedMeshError> {
        let data = self.tangents.unpack_chunked(2, 2, 0, None, limits)?;
        let signs = self.tangent_signs.unpack_ints(limits)?;
        let count = self.tangents.num_items as usize / 2;
        if signs.len() < count * 2 {
            return Err(CompressedMeshError::MissingElementData {
                needed: count * 2,
                available: signs.len(),
            });
        }
        let mut tangents = Vec::new();
        tangents
            .try_reserve_exact(count)
            .map_err(|_| CompressedMeshError::AllocationFailed {
                resource: "decompressed tangents",
                requested: count,
            })?;
        for i in 0..count {
            let mut x = data[i * 2];
            let mut y = data[i * 2 + 1];
            let zsqr = 1.0 - x * x - y * y;
            let mut z = if zsqr >= 0.0 {
                zsqr.sqrt()
            } else {
                let normalized = normalize(Vec3::new(x, y, 0.0));
                x = normalized.x;
                y = normalized.y;
                normalized.z
            };
            if signs[i * 2] == 0 {
                z = -z;
            }
            let w = if signs[i * 2 + 1] == 0 { -1.0 } else { 1.0 };
            tangents.push(Vec4::new(x, y, z, w));
        }
        Ok(tangents)
    }

    /// `SetTangents`.
    pub fn set_tangents(
        &mut self,
        tangents: &[Vec4],
        limits: PackedBitVectorLimits,
    ) -> Result<(), CompressedMeshError> {
        let (floats, signs) = make_float_and_sign_arrays_vec4(tangents);
        self.tangents.pack(&floats, limits)?;
        self.tangent_signs.pack_uints(&signs, limits)?;
        Ok(())
    }

    /// `GetTriangles`.
    pub fn get_triangles(
        &self,
        limits: PackedBitVectorLimits,
    ) -> Result<Vec<u32>, CompressedMeshError> {
        Ok(self.triangles.unpack_uints(limits)?)
    }

    /// `SetTriangles`.
    pub fn set_triangles(
        &mut self,
        triangles: &[u32],
        limits: PackedBitVectorLimits,
    ) -> Result<(), CompressedMeshError> {
        self.triangles.pack_uints(triangles, limits)?;
        Ok(())
    }

    /// `GetWeights`.
    pub fn get_weights(
        &self,
        limits: PackedBitVectorLimits,
    ) -> Result<Vec<BoneWeight4>, CompressedMeshError> {
        let weights = self.weights.unpack_ints(limits)?;
        let bone_indices = self.bone_indices.unpack_ints(limits)?;

        let mut skin = Vec::new();
        skin.try_reserve_exact(self.weights.num_items as usize)
            .map_err(|_| CompressedMeshError::AllocationFailed {
                resource: "decompressed bone weights",
                requested: self.weights.num_items as usize,
            })?;

        let mut bone_index_pos = 0usize;
        let mut j = 0usize;
        let mut sum = 0i32;
        let mut current = BoneWeight4::default();

        for i in 0..self.weights.num_items as usize {
            let weight = *weights
                .get(i)
                .ok_or(CompressedMeshError::MissingElementData {
                    needed: i + 1,
                    available: weights.len(),
                })?;
            current.weights[j] = weight as f32 / 31.0;
            current.indices[j] = *bone_indices.get(bone_index_pos).ok_or(
                CompressedMeshError::MissingElementData {
                    needed: bone_index_pos + 1,
                    available: bone_indices.len(),
                },
            )?;
            bone_index_pos += 1;
            j += 1;
            sum += weight;

            if sum >= 31 {
                for lane in j..4 {
                    current.weights[lane] = 0.0;
                    current.indices[lane] = 0;
                }
                skin.push(current);
                current = BoneWeight4::default();
                j = 0;
                sum = 0;
            } else if j == 3 {
                current.weights[j] = (31 - sum) as f32 / 31.0;
                current.indices[j] = *bone_indices.get(bone_index_pos).ok_or(
                    CompressedMeshError::MissingElementData {
                        needed: bone_index_pos + 1,
                        available: bone_indices.len(),
                    },
                )?;
                bone_index_pos += 1;
                skin.push(current);
                current = BoneWeight4::default();
                j = 0;
                sum = 0;
            }
        }
        Ok(skin)
    }

    /// `SetWeights`.
    pub fn set_weights(
        &mut self,
        skin: &[BoneWeight4],
        limits: PackedBitVectorLimits,
    ) -> Result<(), CompressedMeshError> {
        if skin.is_empty() {
            self.weights = PackedBitVector::default();
            self.bone_indices = PackedBitVector::default();
            return Ok(());
        }
        let mut weight_list = Vec::with_capacity(skin.len() * 3);
        let mut bone_index_list = Vec::with_capacity(skin.len() * 4);
        for bone_weight in skin {
            let mut sum = 0i32;
            for j in 0..4 {
                let weight = (bone_weight.weights[j] * 31.0) as i32;
                sum += weight;
                if j != 3 {
                    weight_list.push(weight);
                }
                bone_index_list.push(bone_weight.indices[j]);
                if sum >= 31 {
                    break;
                }
            }
        }
        self.weights.pack_ints(&weight_list, limits)?;
        self.bone_indices.pack_ints(&bone_index_list, limits)?;
        Ok(())
    }

    /// `GetUV`.
    pub fn get_uv(
        &self,
        limits: PackedBitVectorLimits,
    ) -> Result<[Option<Vec<Vec2>>; 8], CompressedMeshError> {
        let mut result: [Option<Vec<Vec2>>; 8] = Default::default();
        if self.uv.num_items == 0 {
            return Ok(result);
        }
        let vertex_count = self.vertices.num_items as usize / 3;
        if self.has_uv_info && self.uv_info != 0 {
            let mut uv_src_offset = 0usize;
            for (channel, slot) in result.iter_mut().enumerate() {
                *slot = read_channel(
                    &self.uv,
                    self.uv_info,
                    channel,
                    vertex_count,
                    &mut uv_src_offset,
                    limits,
                )?;
            }
        } else {
            let data = self
                .uv
                .unpack_chunked(2, 2, 0, Some(vertex_count), limits)?;
            result[0] = Some(float_array_to_vector2(&data, 2)?);
            if self.uv.num_items as usize >= vertex_count * 4 {
                let data =
                    self.uv
                        .unpack_chunked(2, 2, vertex_count * 2, Some(vertex_count), limits)?;
                result[1] = Some(float_array_to_vector2(&data, 2)?);
            }
        }
        Ok(result)
    }

    /// `SetUV`.
    pub fn set_uv(
        &mut self,
        uv: [&[Vec2]; 8],
        limits: PackedBitVectorLimits,
    ) -> Result<(), CompressedMeshError> {
        let late_channels_empty = uv[2..].iter().all(|channel| channel.is_empty());
        if !self.has_uv_info || late_channels_empty {
            self.uv_info = 0;
            if uv[0].is_empty() {
                self.uv = PackedBitVectorSingle::default();
            } else if uv[1].is_empty() {
                self.pack_uv(uv[0], limits)?;
            } else {
                if uv[0].len() != uv[1].len() {
                    return Err(CompressedMeshError::MismatchedUvLengths {
                        first: uv[0].len(),
                        second: uv[1].len(),
                    });
                }
                let mut concatenated = Vec::with_capacity(uv[0].len() + uv[1].len());
                concatenated.extend_from_slice(uv[0]);
                concatenated.extend_from_slice(uv[1]);
                self.pack_uv(&concatenated, limits)?;
            }
        } else {
            let mut buffer = Vec::new();
            let mut uv_info = 0u32;
            for (channel, data) in uv.iter().enumerate() {
                if !data.is_empty() {
                    uv_info = add_channel_info(uv_info, channel, true, 2)?;
                    buffer.extend_from_slice(data);
                }
            }
            self.pack_uv(&buffer, limits)?;
            self.uv_info = uv_info;
        }
        Ok(())
    }

    fn pack_uv(
        &mut self,
        values: &[Vec2],
        limits: PackedBitVectorLimits,
    ) -> Result<(), CompressedMeshError> {
        let mut floats = Vec::with_capacity(values.len() * 2);
        for value in values {
            floats.push(value.x);
            floats.push(value.y);
        }
        self.uv.pack(&floats, limits)?;
        Ok(())
    }

    /// `GetBindPoses` (Unity 5 and earlier).
    pub fn get_bind_poses(
        &self,
        limits: PackedBitVectorLimits,
    ) -> Result<Vec<[[f32; 4]; 4]>, CompressedMeshError> {
        if !self.has_bind_poses {
            return Ok(Vec::new());
        }
        const MATRIX_FLOATS: usize = 16;
        let count = self.bind_poses.num_items as usize / MATRIX_FLOATS;
        let data =
            self.bind_poses
                .unpack_chunked(MATRIX_FLOATS, MATRIX_FLOATS, 0, Some(count), limits)?;
        let mut result = Vec::new();
        result
            .try_reserve_exact(count)
            .map_err(|_| CompressedMeshError::AllocationFailed {
                resource: "bind poses",
                requested: count,
            })?;
        for i in 0..count {
            let base = i * MATRIX_FLOATS;
            let mut matrix = [[0f32; 4]; 4];
            for (row, values) in matrix.iter_mut().enumerate() {
                for (column, value) in values.iter_mut().enumerate() {
                    *value = data[base + row * 4 + column];
                }
            }
            result.push(transpose(matrix));
        }
        Ok(result)
    }

    /// `SetBindPoses` (Unity 5 and earlier).
    pub fn set_bind_poses(
        &mut self,
        bind_poses: &[[[f32; 4]; 4]],
        limits: PackedBitVectorLimits,
    ) -> Result<(), CompressedMeshError> {
        if !self.has_bind_poses {
            return Ok(());
        }
        if bind_poses.is_empty() {
            self.bind_poses = PackedBitVectorSingle::default();
            return Ok(());
        }
        let mut floats = Vec::with_capacity(bind_poses.len() * 16);
        for matrix in bind_poses {
            let transposed = transpose(*matrix);
            for row in transposed {
                floats.extend_from_slice(&row);
            }
        }
        self.bind_poses.pack(&floats, limits)?;
        Ok(())
    }
}

/// `Vector3.Normalize`, which divides each component by the length.
fn normalize(value: Vec3) -> Vec3 {
    let length_squared = value.x * value.x + value.y * value.y + value.z * value.z;
    let length = length_squared.sqrt();
    Vec3::new(value.x / length, value.y / length, value.z / length)
}

fn vector3_to_float_array(vertices: &[Vec3]) -> Vec<f32> {
    let mut floats = Vec::with_capacity(vertices.len() * 3);
    for vertex in vertices {
        floats.push(vertex.x);
        floats.push(vertex.y);
        floats.push(vertex.z);
    }
    floats
}

/// `MeshHelper.FloatArrayToVector3`.
fn float_array_to_vector3(
    input: &[f32],
    dimension: usize,
) -> Result<Vec<Vec3>, CompressedMeshError> {
    if !(1..=4).contains(&dimension) {
        return Err(CompressedMeshError::InvalidDimension { dimension });
    }
    if !input.len().is_multiple_of(dimension) {
        return Err(CompressedMeshError::InvalidFloatArrayLength {
            length: input.len(),
            dimension,
        });
    }
    let count = input.len() / dimension;
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| CompressedMeshError::AllocationFailed {
            resource: "vector3 array",
            requested: count,
        })?;
    for i in 0..count {
        let base = i * dimension;
        let vertex = match dimension {
            1 => Vec3::new(input[base], 0.0, 0.0),
            2 => Vec3::new(input[base], input[base + 1], 0.0),
            3 => Vec3::new(input[base], input[base + 1], input[base + 2]),
            _ => Vec3::new(input[base], input[base + 1], input[base + 2]),
        };
        result.push(vertex);
    }
    Ok(result)
}

/// `UVInfo.GetChannelInfo`.
fn get_channel_info(value: u32, channel: usize) -> Result<(bool, usize), CompressedMeshError> {
    if channel >= 8 {
        return Err(CompressedMeshError::InvalidUvChannel { channel });
    }
    let bits = (value >> (channel * 4)) & 0xF;
    let exists = bits & 0b0100 != 0;
    let dimension = 1 + (bits & 0b0011) as usize;
    Ok((exists, dimension))
}

/// `UVInfo.AddChannelInfo`.
fn add_channel_info(
    value: u32,
    channel: usize,
    exists: bool,
    dimension: usize,
) -> Result<u32, CompressedMeshError> {
    if channel >= 8 {
        return Err(CompressedMeshError::InvalidUvChannel { channel });
    }
    if !(1..=4).contains(&dimension) {
        return Err(CompressedMeshError::InvalidUvDimension { dimension });
    }
    let shift = (channel * 4) as u32;
    let bits = (if exists { 0b0100u32 } else { 0 }) | (dimension as u32 - 1);
    Ok((value & !(0xFu32 << shift)) | (bits << shift))
}

/// `CompressedMeshExtensions.ReadChannel`.
fn read_channel(
    vector: &PackedBitVectorSingle,
    uv_info: u32,
    channel: usize,
    vertex_count: usize,
    current_offset: &mut usize,
    limits: PackedBitVectorLimits,
) -> Result<Option<Vec<Vec2>>, CompressedMeshError> {
    let (exists, dimension) = get_channel_info(uv_info, channel)?;
    if !exists {
        return Ok(None);
    }
    let data = vector.unpack_chunked(
        dimension,
        dimension,
        *current_offset,
        Some(vertex_count),
        limits,
    )?;
    *current_offset += dimension * vertex_count;
    Ok(Some(float_array_to_vector2(&data, dimension)?))
}

/// `MeshHelper.FloatArrayToVector2`.
fn float_array_to_vector2(
    input: &[f32],
    dimension: usize,
) -> Result<Vec<Vec2>, CompressedMeshError> {
    if !(1..=4).contains(&dimension) {
        return Err(CompressedMeshError::InvalidDimension { dimension });
    }
    if !input.len().is_multiple_of(dimension) {
        return Err(CompressedMeshError::InvalidFloatArrayLength {
            length: input.len(),
            dimension,
        });
    }
    let count = input.len() / dimension;
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| CompressedMeshError::AllocationFailed {
            resource: "vector2 array",
            requested: count,
        })?;
    for i in 0..count {
        let base = i * dimension;
        let value = if dimension == 1 {
            Vec2::new(input[base], 0.0)
        } else {
            Vec2::new(input[base], input[base + 1])
        };
        result.push(value);
    }
    Ok(result)
}

fn transpose(matrix: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut result = [[0f32; 4]; 4];
    for (row, values) in result.iter_mut().enumerate() {
        for (column, value) in values.iter_mut().enumerate() {
            *value = matrix[column][row];
        }
    }
    result
}

fn make_float_and_sign_arrays_vec3(normals: &[Vec3]) -> (Vec<f32>, Vec<u32>) {
    let mut floats = Vec::with_capacity(normals.len() * 2);
    let mut signs = Vec::with_capacity(normals.len());
    for normal in normals {
        let vector = normalize(*normal);
        floats.push(vector.x);
        floats.push(vector.y);
        signs.push(if vector.z < 0.0 { 0 } else { 1 });
    }
    (floats, signs)
}

fn make_float_and_sign_arrays_vec4(tangents: &[Vec4]) -> (Vec<f32>, Vec<u32>) {
    let mut floats = Vec::with_capacity(tangents.len() * 2);
    let mut signs = Vec::with_capacity(tangents.len() * 2);
    for tangent in tangents {
        let vector = normalize(Vec3::new(tangent.x, tangent.y, tangent.z));
        floats.push(vector.x);
        floats.push(vector.y);
        signs.push(if vector.z < 0.0 { 0 } else { 1 });
        signs.push(if tangent.w < 0.0 { 0 } else { 1 });
    }
    (floats, signs)
}

#[cfg(test)]
mod tests {
    use super::{normalize, CompressedMesh, Vec2, Vec3, Vec4};
    use crate::numerics::BoneWeight4;
    use crate::resources::packed_bit_vector::PackedBitVectorLimits;

    fn limits() -> PackedBitVectorLimits {
        PackedBitVectorLimits::default()
    }

    fn vec3_bits(values: &[Vec3]) -> Vec<u32> {
        values
            .iter()
            .flat_map(|value| [value.x.to_bits(), value.y.to_bits(), value.z.to_bits()])
            .collect()
    }

    fn vec4_bits(values: &[Vec4]) -> Vec<u32> {
        values
            .iter()
            .flat_map(|value| {
                [
                    value.x.to_bits(),
                    value.y.to_bits(),
                    value.z.to_bits(),
                    value.w.to_bits(),
                ]
            })
            .collect()
    }

    /// Byte-for-byte differential against the reference `CompressedMeshExtensions`.
    ///
    /// The expected values come from the isolated probe `Temp/opencode/meshprobe`, which built a
    /// real `CompressedMesh` through `CompressedMesh.Create(2019.4.3f1)`, used the `Set*`
    /// methods, and read the `Get*` outputs.
    #[test]
    fn geometry_matches_the_reference_runtime() {
        let mut mesh = CompressedMesh::default();
        mesh.set_vertices(
            &[
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.5, -2.5, 3.25),
                Vec3::new(-0.125, 0.5, 0.75),
            ],
            limits(),
        )
        .unwrap();
        assert_eq!(
            vec3_bits(&mesh.get_vertices(limits()).unwrap()),
            [0, 0, 0, 1069547520, 3223322624, 1078984704, 3187671040, 1056964612, 1061158916]
        );

        let mut mesh = CompressedMesh::default();
        mesh.set_normals(
            &[
                Vec3::new(0.0, 0.0, 1.0),
                Vec3::new(0.0, -0.6, 0.8),
                Vec3::new(0.6, 0.8, 0.0),
                Vec3::new(-1.0, 0.0, 0.0),
            ],
            limits(),
        )
        .unwrap();
        assert_eq!(
            vec3_bits(&mesh.get_normals(limits()).unwrap()),
            [
                0, 0, 1065353216, 0, 3206125978, 1061997773, 1058642332, 1061997772, 0, 3212836864,
                0, 0
            ]
        );

        let mut mesh = CompressedMesh::default();
        mesh.set_tangents(
            &[
                Vec4::new(1.0, 0.0, 0.0, 1.0),
                Vec4::new(0.0, 0.0, -1.0, -1.0),
                Vec4::new(0.6, 0.8, 0.0, 1.0),
            ],
            limits(),
        )
        .unwrap();
        assert_eq!(
            vec4_bits(&mesh.get_tangents(limits()).unwrap()),
            [
                1065353216, 0, 0, 1065353216, 0, 0, 3212836864, 3212836864, 1058642330, 1061997773,
                0, 1065353216
            ]
        );

        let mut mesh = CompressedMesh::default();
        mesh.set_triangles(&[0, 1, 2, 3, 2, 1, 0xFFFF_FFFF], limits())
            .unwrap();
        assert_eq!(mesh.get_triangles(limits()).unwrap(), [0, 0, 0, 0, 0, 0, 0]);

        let mut mesh = CompressedMesh::default();
        mesh.set_weights(
            &[
                BoneWeight4::new(1.0, 0.0, 0.0, 0.0, 3, 0, 0, 0),
                BoneWeight4::new(0.25, 0.25, 0.25, 0.25, 7, 1, 2, 9),
                BoneWeight4::new(0.5, 0.5, 0.0, 0.0, 4, 6, 0, 0),
                BoneWeight4::new(0.1, 0.2, 0.3, 0.4, 5, 8, 1, 2),
            ],
            limits(),
        )
        .unwrap();
        let skin = mesh.get_weights(limits()).unwrap();
        assert_eq!(skin.len(), 4);
        let weight_bits: Vec<u32> = skin
            .iter()
            .flat_map(|weight| weight.weights.map(f32::to_bits))
            .collect();
        assert_eq!(
            weight_bits,
            [
                1065353216, 0, 0, 0, 1046952398, 1046952398, 1046952398, 1051011402, 1056423407,
                1056423407, 0, 1023680776, 1036398988, 1044787596, 1049929001, 1054258605
            ]
        );
        let indices: Vec<i32> = skin.iter().flat_map(|weight| weight.indices).collect();
        assert_eq!(indices, [3, 0, 0, 0, 7, 1, 2, 9, 4, 6, 0, 0, 5, 8, 1, 2]);
    }

    #[test]
    fn normalize_matches_the_reference_runtime() {
        let values = [
            Vec3::new(1.2, 0.1, 0.0),
            Vec3::new(-3.0, 4.0, 0.0),
            Vec3::new(1.0, 1.0, 1.0),
        ];
        let bits: Vec<u32> = values
            .iter()
            .flat_map(|value| {
                let normalized = normalize(*value);
                [
                    normalized.x.to_bits(),
                    normalized.y.to_bits(),
                    normalized.z.to_bits(),
                ]
            })
            .collect();
        assert_eq!(
            bits,
            [
                1065295264, 1034556351, 0, 3206125978, 1061997773, 0, 1058262330, 1058262330,
                1058262330
            ]
        );
    }

    fn vec2_bits(values: &[Vec2]) -> Vec<u32> {
        values
            .iter()
            .flat_map(|value| [value.x.to_bits(), value.y.to_bits()])
            .collect()
    }

    fn three_vertices() -> [Vec3; 3] {
        [
            Vec3::new(0.0, 0.5, 0.25),
            Vec3::new(1.0, 1.5, 1.25),
            Vec3::new(2.0, 2.5, 2.25),
        ]
    }

    /// Byte-for-byte differential against the reference `CompressedMeshExtensions` UV and
    /// bind-pose paths (probe `Temp/opencode/uvprobe`).
    #[test]
    fn uv_and_bind_poses_match_the_reference_runtime() {
        let pair = [
            Vec2::new(0.0, 0.0),
            Vec2::new(0.25, 0.5),
            Vec2::new(1.0, -1.0),
        ];
        let triple = [
            Vec2::new(0.1, 0.2),
            Vec2::new(0.3, -0.4),
            Vec2::new(0.5, 0.6),
        ];
        let empty: [Vec2; 0] = [];

        // Two-channel fast path: UVInfo stays zero.
        let mut mesh = CompressedMesh {
            has_uv_info: true,
            ..CompressedMesh::default()
        };
        mesh.set_vertices(&three_vertices(), limits()).unwrap();
        mesh.set_uv(
            [
                &pair, &triple, &empty, &empty, &empty, &empty, &empty, &empty,
            ],
            limits(),
        )
        .unwrap();
        let uv = mesh.get_uv(limits()).unwrap();
        assert_eq!(mesh.uv_info, 0);
        assert_eq!(
            vec2_bits(uv[0].as_ref().unwrap()),
            [872415232, 872415232, 1048576000, 1056964608, 1065353216, 3212836864]
        );
        assert_eq!(
            vec2_bits(uv[1].as_ref().unwrap()),
            [1036831952, 1045220560, 1050253720, 3201092810, 1056964608, 1058642330]
        );
        assert!(uv[2..].iter().all(Option::is_none));

        // UVInfo path: three channels, each dimension two.
        let mut mesh = CompressedMesh {
            has_uv_info: true,
            ..CompressedMesh::default()
        };
        mesh.set_vertices(&three_vertices(), limits()).unwrap();
        mesh.set_uv(
            [
                &pair, &triple, &triple, &empty, &empty, &empty, &empty, &empty,
            ],
            limits(),
        )
        .unwrap();
        let uv = mesh.get_uv(limits()).unwrap();
        assert_eq!(mesh.uv_info, 1365);
        assert_eq!(
            vec2_bits(uv[0].as_ref().unwrap()),
            [872415232, 872415232, 1048576000, 1056964608, 1065353216, 3212836864]
        );
        assert_eq!(
            vec2_bits(uv[1].as_ref().unwrap()),
            [1036831952, 1045220560, 1050253720, 3201092810, 1056964608, 1058642330]
        );
        assert_eq!(
            vec2_bits(uv[2].as_ref().unwrap()),
            [1036831952, 1045220560, 1050253720, 3201092810, 1056964608, 1058642330]
        );
        assert!(uv[3..].iter().all(Option::is_none));

        // Bind poses (Unity 5 and earlier).
        let mut mesh = CompressedMesh {
            has_bind_poses: true,
            ..CompressedMesh::default()
        };
        let first = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [2.0, 3.0, 4.0, 1.0],
        ];
        let second = [
            [0.5, 0.0, 0.0, 0.0],
            [0.0, 0.25, 0.0, 0.0],
            [0.0, 0.0, 2.0, 0.0],
            [-1.0, 0.0, 0.0, 1.0],
        ];
        mesh.set_bind_poses(&[first, second], limits()).unwrap();
        let poses = mesh.get_bind_poses(limits()).unwrap();
        assert_eq!(poses.len(), 2);
        let bits: Vec<u32> = poses
            .iter()
            .flat_map(|matrix| matrix.iter().flat_map(|row| row.map(f32::to_bits)))
            .collect();
        assert_eq!(
            bits,
            [
                1065353216, 0, 0, 0, 0, 1065353216, 0, 0, 0, 0, 1065353216, 0, 1073741824,
                1077936128, 1082130432, 1065353216, 1056964610, 0, 0, 0, 0, 1048576004, 0, 0, 0, 0,
                1073741824, 0, 3212836864, 0, 0, 1065353216
            ]
        );
    }
}
