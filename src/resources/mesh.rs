//! `Mesh` (class 43) field reading and processed-data assembly.
//!
//! Reads the serialized mesh fields the source's `MeshExtensions.ReadData` path consumes —
//! `m_VertexData` (`m_VertexCount`/`m_Channels`/`m_Streams`/`m_DataSize`), `m_CompressedMesh`,
//! `m_IndexBuffer`, `m_IndexFormat`, `m_SubMeshes`, `m_BindPose`, and `m_Skin` — then runs the
//! vertex-data-blob and compressed-mesh decoders and merges them into a [`MeshData`].
//!
//! Field-name basis: `m_VertexData`/`m_DataSize` are already used by the streamed-asset path; the
//! `SubMesh` children use Unity's serialized TypeTree names (`firstByte`, `indexCount`, `topology`,
//! `baseVertex`, `firstVertex`, `vertexCount`, `localAABB`, `m_IsTriStrip`/`isTriStrip`). The
//! generated `ISubMesh`/`IBoneWeights4`/`IStreamInfo`/`IChannelInfo`/`ICompressedMesh` property
//! names (probed from the shipped `AssetRipper.SourceGenerated.dll`) combined with the
//! `Pass002_RenameSubnodes` naming rule (`data[0]` becomes `data_0_`, then `m_`-prefixed and
//! capitalized) imply the `weight[0]`/`boneIndex[0]` lane names and the `channelMask`/`stride`/
//! `dividerOp`/`frequency` stream names. A residual `m_`-prefix ambiguity remains for sub-struct
//! children (for example `isTriStrip` versus `m_IsTriStrip`, both of which are accepted); a
//! payload whose sub-mesh children match neither produces an explicit error rather than an
//! empty sub-mesh list.

use std::error::Error;
use std::fmt::{self, Display};

use crate::numerics::BoneWeight4;
use crate::serialized_file::object::{SerializedValue, SerializedValueLookupError};

use super::compressed_mesh::{decompress, CompressedMesh, CompressedMeshError};
use super::mesh_data::{MeshData, MeshParts};
use super::packed_bit_vector::{PackedBitVector, PackedBitVectorLimits};
use super::packed_float_vector::PackedBitVectorSingle;
use super::sub_mesh_data::{Bounds, IndexFormat, SubMeshData, SubMeshView};
use super::vertex_data_blob::{
    ChannelInfo, EndianType, StreamInfo, UnityVersion, VertexDataBlob, VertexDataError,
};
use crate::numerics::Vec3;

/// A failure while reading `Mesh` fields or assembling the mesh data.
#[derive(Debug)]
pub enum MeshViewError {
    /// A field lookup found duplicate names along the base chain.
    FieldLookup {
        /// The field being read.
        field: &'static str,
        /// The underlying lookup failure.
        source: SerializedValueLookupError,
    },
    /// A required field is absent.
    MissingField {
        /// The required field name.
        field: &'static str,
    },
    /// A field is present but has the wrong value kind.
    WrongType {
        /// The field being read.
        field: &'static str,
        /// The value kind actually decoded.
        actual: &'static str,
    },
    /// A negative count or size.
    NegativeValue {
        /// The offending field.
        field: &'static str,
    },
    /// The vertex-data blob could not be built or read.
    VertexData(VertexDataError),
    /// The compressed mesh could not be decompressed.
    CompressedMesh(CompressedMeshError),
}

impl From<VertexDataError> for MeshViewError {
    fn from(error: VertexDataError) -> Self {
        Self::VertexData(error)
    }
}

impl From<CompressedMeshError> for MeshViewError {
    fn from(error: CompressedMeshError) -> Self {
        Self::CompressedMesh(error)
    }
}

impl Display for MeshViewError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FieldLookup { field, source } => {
                write!(formatter, "could not read Mesh field '{field}': {source}")
            }
            Self::MissingField { field } => {
                write!(formatter, "Mesh is missing required field '{field}'")
            }
            Self::WrongType { field, actual } => write!(
                formatter,
                "Mesh field '{field}' has kind {actual}, expected a different kind"
            ),
            Self::NegativeValue { field } => {
                write!(formatter, "Mesh field '{field}' is negative")
            }
            Self::VertexData(error) => write!(formatter, "{error}"),
            Self::CompressedMesh(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for MeshViewError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::FieldLookup { source, .. } => Some(source),
            Self::VertexData(error) => Some(error),
            Self::CompressedMesh(error) => Some(error),
            _ => None,
        }
    }
}

/// The serialized `m_VertexData` fields.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VertexDataFields {
    /// `m_VertexCount`.
    pub vertex_count: u32,
    /// `m_Channels`, when the version serializes an explicit list.
    pub channels: Option<Vec<ChannelInfo>>,
    /// `m_Streams`: a serialized stream list, or Unity 3.5's four fixed `m_Streams[0..3]`.
    pub streams: Option<Vec<StreamInfo>>,
    /// `m_DataSize`.
    pub data: Vec<u8>,
}

/// The serialized `Mesh` fields.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshView {
    /// The Unity version used for channel/stream layout.
    pub version: UnityVersion,
    /// The file endianness.
    pub endian: EndianType,
    /// `m_VertexData`, when present.
    pub vertex_data: Option<VertexDataFields>,
    /// `m_CompressedMesh`, when present and non-default.
    pub compressed_mesh: Option<CompressedMesh>,
    /// `m_IndexBuffer`.
    pub index_buffer: Vec<u8>,
    /// `m_IndexFormat` (`0` = 16-bit, `1` = 32-bit); absent means 16-bit.
    pub index_format: Option<i32>,
    /// `m_SubMeshes`.
    pub sub_meshes: Vec<SubMeshView>,
    /// `m_BindPose`.
    pub bind_pose: Option<Vec<[[f32; 4]; 4]>>,
    /// `m_Skin`.
    pub skin: Option<Vec<BoneWeight4>>,
}

impl MeshView {
    /// Reads the export-relevant fields from a decoded `Mesh` payload.
    pub fn from_value(
        root: &SerializedValue,
        version: UnityVersion,
        endian: EndianType,
    ) -> Result<Self, MeshViewError> {
        let vertex_data = read_vertex_data(root)?;
        let compressed_mesh = read_compressed_mesh(root)?;
        let index_buffer = read_bytes(root, &["m_IndexBuffer"])?.unwrap_or_default();
        let index_format = read_i32(root, &["m_IndexFormat"])?;
        let sub_meshes = read_sub_meshes(root)?;
        let bind_pose = read_bind_pose(root)?;
        let skin = read_skin(root)?;
        Ok(Self {
            version,
            endian,
            vertex_data,
            compressed_mesh,
            index_buffer,
            index_format,
            sub_meshes,
            bind_pose,
            skin,
        })
    }

    /// The resolved index format (`m_IndexFormat`, or 16-bit when absent).
    pub fn resolved_index_format(&self) -> IndexFormat {
        match self.index_format {
            Some(1) => IndexFormat::UInt32,
            _ => IndexFormat::UInt16,
        }
    }

    /// `MeshExtensions.GetProcessedIndexBuffer`: decode `m_IndexBuffer` with the file endianness.
    pub fn processed_index_buffer(&self) -> Vec<u32> {
        let format = self.resolved_index_format();
        let size = format.size() as usize;
        let count = self.index_buffer.len() / size;
        let mut result = Vec::with_capacity(count);
        for index in 0..count {
            let bytes = &self.index_buffer[index * size..index * size + size];
            result.push(match format {
                IndexFormat::UInt16 => u32::from(read_u16(bytes, self.endian)),
                IndexFormat::UInt32 => read_u32_le_or_be(bytes, self.endian),
            });
        }
        result
    }

    /// `MeshExtensions.ReadData` + `MeshData.TryMakeFromMesh`.
    pub fn read_mesh_data(
        &self,
        limits: PackedBitVectorLimits,
    ) -> Result<Option<MeshData>, MeshViewError> {
        let blob = match &self.vertex_data {
            Some(vertex_data) => VertexDataBlob::from_parts(
                vertex_data.channels.clone(),
                vertex_data.streams.clone(),
                vertex_data.data.clone(),
                vertex_data.vertex_count,
                self.version,
                self.endian,
            )?
            .read_data()?,
            None => None,
        };
        let compressed = match &self.compressed_mesh {
            Some(mesh) => decompress(mesh, limits)?,
            None => Default::default(),
        };
        let format = self.resolved_index_format();
        let sub_meshes = self
            .sub_meshes
            .iter()
            .map(|view| SubMeshData::create(view, format))
            .collect();
        Ok(MeshData::try_make_from_parts(MeshParts {
            blob,
            compressed,
            mesh_bind_pose: self.bind_pose.clone(),
            mesh_index_buffer: self.processed_index_buffer(),
            mesh_skin: self.skin.clone(),
            sub_meshes,
        }))
    }
}

fn read_u16(bytes: &[u8], endian: EndianType) -> u16 {
    let pair = [bytes[0], bytes[1]];
    match endian {
        EndianType::Little => u16::from_le_bytes(pair),
        EndianType::Big => u16::from_be_bytes(pair),
    }
}

fn read_u32_le_or_be(bytes: &[u8], endian: EndianType) -> u32 {
    let quad = [bytes[0], bytes[1], bytes[2], bytes[3]];
    match endian {
        EndianType::Little => u32::from_le_bytes(quad),
        EndianType::Big => u32::from_be_bytes(quad),
    }
}

fn serialized_kind(value: &SerializedValue) -> &'static str {
    match value {
        SerializedValue::Bool(_) => "bool",
        SerializedValue::I8(_) => "SInt8",
        SerializedValue::U8(_) => "UInt8",
        SerializedValue::I16(_) => "SInt16",
        SerializedValue::U16(_) => "UInt16",
        SerializedValue::Char(_) => "char",
        SerializedValue::I32(_) => "SInt32",
        SerializedValue::U32(_) => "UInt32",
        SerializedValue::I64(_) => "SInt64",
        SerializedValue::U64(_) => "UInt64",
        SerializedValue::F32(_) => "float",
        SerializedValue::F64(_) => "double",
        SerializedValue::StringBytes(_) => "string bytes",
        SerializedValue::Bytes(_) => "byte data",
        SerializedValue::Array(_) => "array",
        SerializedValue::Struct(_) => "struct",
        SerializedValue::PPtr { .. } => "PPtr",
        SerializedValue::Raw { .. } => "raw data",
    }
}

fn lookup<'a>(
    root: &'a SerializedValue,
    name: &'static str,
) -> Result<Option<&'a SerializedValue>, MeshViewError> {
    root.unique_field_in_base_chain(name)
        .map_err(|source| MeshViewError::FieldLookup {
            field: name,
            source,
        })
}

fn read_i32(root: &SerializedValue, names: &[&'static str]) -> Result<Option<i32>, MeshViewError> {
    for name in names {
        if let Some(value) = lookup(root, name)? {
            return match value {
                SerializedValue::I32(value) => Ok(Some(*value)),
                SerializedValue::U32(value) => Ok(Some(*value as i32)),
                other => Err(MeshViewError::WrongType {
                    field: name,
                    actual: serialized_kind(other),
                }),
            };
        }
    }
    Ok(None)
}

fn read_u32(root: &SerializedValue, names: &[&'static str]) -> Result<Option<u32>, MeshViewError> {
    match read_i32(root, names)? {
        Some(value) if value >= 0 => Ok(Some(value as u32)),
        Some(_) => Err(MeshViewError::NegativeValue { field: names[0] }),
        None => Ok(None),
    }
}

fn read_u8(root: &SerializedValue, names: &[&'static str]) -> Result<Option<u8>, MeshViewError> {
    for name in names {
        if let Some(value) = lookup(root, name)? {
            return match value {
                SerializedValue::U8(value) => Ok(Some(*value)),
                SerializedValue::I32(value) => Ok(Some(*value as u8)),
                other => Err(MeshViewError::WrongType {
                    field: name,
                    actual: serialized_kind(other),
                }),
            };
        }
    }
    Ok(None)
}

fn read_f32(root: &SerializedValue, names: &[&'static str]) -> Result<Option<f32>, MeshViewError> {
    for name in names {
        if let Some(value) = lookup(root, name)? {
            return match value {
                SerializedValue::F32(value) => Ok(Some(*value)),
                other => Err(MeshViewError::WrongType {
                    field: name,
                    actual: serialized_kind(other),
                }),
            };
        }
    }
    Ok(None)
}

fn read_bytes(
    root: &SerializedValue,
    names: &[&'static str],
) -> Result<Option<Vec<u8>>, MeshViewError> {
    for name in names {
        if let Some(value) = lookup(root, name)? {
            return match value {
                SerializedValue::Bytes(bytes) => Ok(Some(bytes.clone())),
                SerializedValue::Array(values) => {
                    let mut bytes = Vec::with_capacity(values.len());
                    for element in values {
                        match element {
                            SerializedValue::U8(byte) => bytes.push(*byte),
                            other => {
                                return Err(MeshViewError::WrongType {
                                    field: name,
                                    actual: serialized_kind(other),
                                })
                            }
                        }
                    }
                    Ok(Some(bytes))
                }
                other => Err(MeshViewError::WrongType {
                    field: name,
                    actual: serialized_kind(other),
                }),
            };
        }
    }
    Ok(None)
}

fn read_array<'a>(
    root: &'a SerializedValue,
    names: &[&'static str],
) -> Result<Option<&'a [SerializedValue]>, MeshViewError> {
    for name in names {
        if let Some(value) = lookup(root, name)? {
            return match value {
                SerializedValue::Array(values) => Ok(Some(values.as_slice())),
                other => Err(MeshViewError::WrongType {
                    field: name,
                    actual: serialized_kind(other),
                }),
            };
        }
    }
    Ok(None)
}

fn read_vertex_data(root: &SerializedValue) -> Result<Option<VertexDataFields>, MeshViewError> {
    let Some(value) = lookup(root, "m_VertexData")? else {
        return Ok(None);
    };
    if !matches!(value, SerializedValue::Struct(_)) {
        return Err(MeshViewError::WrongType {
            field: "m_VertexData",
            actual: serialized_kind(value),
        });
    }
    let vertex_count = read_u32(value, &["m_VertexCount"])?.unwrap_or(0);
    let channels = match read_array(value, &["m_Channels"])? {
        Some(values) if !values.is_empty() => Some(read_channels(values)?),
        _ => None,
    };
    let streams = match read_array(value, &["m_Streams"])? {
        Some(values) if !values.is_empty() => Some(read_streams(values)?),
        // Unity 3.5 serializes four fixed `m_Streams[0..3]` StreamInfo structs instead of a
        // list; the reference reads them through `GetStreams`/`Has_Streams_0_`.
        _ => read_fixed_streams(value)?,
    };
    let data = read_bytes(value, &["m_DataSize"])?.unwrap_or_default();
    Ok(Some(VertexDataFields {
        vertex_count,
        channels,
        streams,
        data,
    }))
}

fn read_channels(values: &[SerializedValue]) -> Result<Vec<ChannelInfo>, MeshViewError> {
    let mut channels = Vec::with_capacity(values.len());
    for value in values {
        let dimension = read_u8(value, &["dimension"])?
            .ok_or(MeshViewError::MissingField { field: "dimension" })?;
        let offset =
            read_u8(value, &["offset"])?.ok_or(MeshViewError::MissingField { field: "offset" })?;
        let format =
            read_u8(value, &["format"])?.ok_or(MeshViewError::MissingField { field: "format" })?;
        let stream =
            read_u8(value, &["stream"])?.ok_or(MeshViewError::MissingField { field: "stream" })?;
        channels.push(ChannelInfo {
            dimension,
            offset,
            format,
            stream,
        });
    }
    Ok(channels)
}

fn read_streams(values: &[SerializedValue]) -> Result<Vec<StreamInfo>, MeshViewError> {
    let mut streams = Vec::with_capacity(values.len());
    for value in values {
        let channel_mask = read_u32(value, &["channelMask"])?.unwrap_or(0);
        let offset = read_u32(value, &["offset"])?.unwrap_or(0);
        // The generated `Stride_Byte`/`Stride_UInt32` properties imply the TypeTree field is
        // `stride`; the decoded value kind selects the storage width. `Align` is not needed to
        // locate data and is ignored here.
        let (stride, stride_is_u32) = read_stride(value)?;
        let divider_op = read_u8(value, &["dividerOp"])?.unwrap_or(0);
        // The generated `Frequency` property is `UInt16`; values above a byte are reported
        // rather than truncated.
        let frequency = match lookup(value, "frequency")? {
            None => 0,
            Some(SerializedValue::U8(value)) => *value,
            Some(SerializedValue::U16(value)) => {
                u8::try_from(*value).map_err(|_| MeshViewError::WrongType {
                    field: "frequency",
                    actual: "a UInt16 above 255",
                })?
            }
            Some(SerializedValue::I32(value)) => {
                u8::try_from(*value).map_err(|_| MeshViewError::WrongType {
                    field: "frequency",
                    actual: "an out-of-range integer",
                })?
            }
            Some(other) => {
                return Err(MeshViewError::WrongType {
                    field: "frequency",
                    actual: serialized_kind(other),
                })
            }
        };
        streams.push(StreamInfo {
            channel_mask,
            offset,
            stride,
            stride_is_u32,
            divider_op,
            frequency,
        });
    }
    Ok(streams)
}

/// Unity 3.5's four fixed `m_Streams[0..3]` StreamInfo structs.
///
/// Their children are `channelMask`/`offset`/`stride`/`align` (all `UInt32`); there is no
/// `dividerOp`/`frequency`, so those default to zero exactly as the reference's `SetValues` would
/// write them. `align` only pads the stream and is not needed to locate data.
fn read_fixed_streams(value: &SerializedValue) -> Result<Option<Vec<StreamInfo>>, MeshViewError> {
    const FIXED_STREAMS: [&str; 4] = [
        "m_Streams[0]",
        "m_Streams[1]",
        "m_Streams[2]",
        "m_Streams[3]",
    ];
    let mut streams = Vec::with_capacity(4);
    for name in FIXED_STREAMS {
        let Some(field) = lookup(value, name)? else {
            return Ok(None);
        };
        let channel_mask = read_u32(field, &["channelMask"])?.unwrap_or(0);
        let offset = read_u32(field, &["offset"])?.unwrap_or(0);
        let (stride, stride_is_u32) = read_stride(field)?;
        streams.push(StreamInfo {
            channel_mask,
            offset,
            stride,
            stride_is_u32,
            divider_op: 0,
            frequency: 0,
        });
    }
    Ok(Some(streams))
}

fn read_stride(root: &SerializedValue) -> Result<(u32, bool), MeshViewError> {
    let Some(value) = lookup(root, "stride")? else {
        return Ok((0, false));
    };
    match value {
        SerializedValue::U8(value) => Ok((u32::from(*value), false)),
        SerializedValue::U32(value) => Ok((*value, true)),
        SerializedValue::I32(value) if *value >= 0 => Ok((*value as u32, true)),
        SerializedValue::I32(_) => Err(MeshViewError::NegativeValue { field: "stride" }),
        other => Err(MeshViewError::WrongType {
            field: "stride",
            actual: serialized_kind(other),
        }),
    }
}

fn read_packed_bit_vector(
    value: &SerializedValue,
    names: &[&'static str],
) -> Result<PackedBitVector, MeshViewError> {
    let num_items = read_u32(value, names)?.unwrap_or(0);
    let bit_size = read_u8(value, &["m_BitSize"])?.unwrap_or(0);
    let data = read_bytes(value, &["m_Data"])?.unwrap_or_default();
    Ok(PackedBitVector {
        num_items,
        bit_size,
        data,
    })
}

fn read_packed_float_vector(
    value: &SerializedValue,
    names: &[&'static str],
) -> Result<PackedBitVectorSingle, MeshViewError> {
    let start = read_f32(value, &["m_Start"])?.unwrap_or(0.0);
    let range = read_f32(value, &["m_Range"])?.unwrap_or(0.0);
    let num_items = read_u32(value, names)?.unwrap_or(0);
    let bit_size = read_u8(value, &["m_BitSize"])?.unwrap_or(0);
    let data = read_bytes(value, &["m_Data"])?.unwrap_or_default();
    Ok(PackedBitVectorSingle {
        start,
        range,
        num_items,
        bit_size,
        data,
    })
}

fn read_compressed_mesh(root: &SerializedValue) -> Result<Option<CompressedMesh>, MeshViewError> {
    let Some(value) = lookup(root, "m_CompressedMesh")? else {
        return Ok(None);
    };
    if !matches!(value, SerializedValue::Struct(_)) {
        return Err(MeshViewError::WrongType {
            field: "m_CompressedMesh",
            actual: serialized_kind(value),
        });
    }
    let field = |name: &'static str| -> Result<&SerializedValue, MeshViewError> {
        lookup(value, name)?.ok_or(MeshViewError::MissingField { field: name })
    };
    // `m_BindPoses`, `m_Colors`, `m_FloatColors`, and `m_UVInfo` are version-gated: the real
    // class-43 TypeTrees carry `m_BindPoses`/`m_Colors` only on Unity 3.5-4.x and
    // `m_FloatColors`/`m_UVInfo` only on Unity 5.0+, so an absent field is a gate that is off, not
    // a missing required field. `src/resources/mesh_typetree_fixture.txt` pins which versions have
    // which.
    let optional_packed_float =
        |name: &'static str| -> Result<PackedBitVectorSingle, MeshViewError> {
            match lookup(value, name)? {
                Some(field_value) => read_packed_float_vector(field_value, &["m_NumItems"]),
                None => Ok(PackedBitVectorSingle::default()),
            }
        };
    let optional_packed_int = |name: &'static str| -> Result<PackedBitVector, MeshViewError> {
        match lookup(value, name)? {
            Some(field_value) => read_packed_bit_vector(field_value, &["m_NumItems"]),
            None => Ok(PackedBitVector::default()),
        }
    };
    let mesh = CompressedMesh {
        vertices: read_packed_float_vector(field("m_Vertices")?, &["m_NumItems"])?,
        normals: read_packed_float_vector(field("m_Normals")?, &["m_NumItems"])?,
        tangents: read_packed_float_vector(field("m_Tangents")?, &["m_NumItems"])?,
        weights: read_packed_bit_vector(field("m_Weights")?, &["m_NumItems"])?,
        bone_indices: read_packed_bit_vector(field("m_BoneIndices")?, &["m_NumItems"])?,
        normal_signs: read_packed_bit_vector(field("m_NormalSigns")?, &["m_NumItems"])?,
        tangent_signs: read_packed_bit_vector(field("m_TangentSigns")?, &["m_NumItems"])?,
        triangles: read_packed_bit_vector(field("m_Triangles")?, &["m_NumItems"])?,
        uv: read_packed_float_vector(field("m_UV")?, &["m_NumItems"])?,
        uv_info: read_u32(value, &["m_UVInfo"])?.unwrap_or(0),
        has_uv_info: lookup(value, "m_UVInfo")?.is_some(),
        bind_poses: optional_packed_float("m_BindPoses")?,
        has_bind_poses: lookup(value, "m_BindPoses")?.is_some(),
        float_colors: optional_packed_float("m_FloatColors")?,
        has_float_colors: lookup(value, "m_FloatColors")?.is_some(),
        colors: optional_packed_int("m_Colors")?,
        has_colors: lookup(value, "m_Colors")?.is_some(),
    };
    if !mesh.is_set() && mesh.uv.num_items == 0 && mesh.triangles.num_items == 0 {
        return Ok(None);
    }
    Ok(Some(mesh))
}

fn read_sub_meshes(root: &SerializedValue) -> Result<Vec<SubMeshView>, MeshViewError> {
    let Some(values) = read_array(root, &["m_SubMeshes"])? else {
        return Ok(Vec::new());
    };
    let mut sub_meshes = Vec::with_capacity(values.len());
    for value in values {
        let base_vertex = read_u32(value, &["baseVertex"])?.unwrap_or(0);
        let first_byte = read_u32(value, &["firstByte"])?.unwrap_or(0);
        let first_vertex = read_u32(value, &["firstVertex"])?.unwrap_or(0);
        let index_count = read_u32(value, &["indexCount"])?.unwrap_or(0);
        let triangle_count = read_u32(value, &["triangleCount"])?.unwrap_or(0);
        let vertex_count = read_u32(value, &["vertexCount"])?.unwrap_or(0);
        let topology = read_i32(value, &["topology"])?;
        let is_tri_strip = read_i32(value, &["m_IsTriStrip", "isTriStrip"])?.unwrap_or(0) as u32;
        let local_bounds = read_aabb(value)?;
        sub_meshes.push(SubMeshView {
            base_vertex,
            first_byte,
            first_vertex,
            index_count,
            triangle_count,
            vertex_count,
            topology,
            is_tri_strip,
            local_bounds,
        });
    }
    Ok(sub_meshes)
}

fn read_aabb(value: &SerializedValue) -> Result<Bounds, MeshViewError> {
    let Some(aabb) = lookup(value, "localAABB")? else {
        return Ok(Bounds::default());
    };
    let center = read_vec3(aabb, &["m_Center"])?.unwrap_or_default();
    let extent = read_vec3(aabb, &["m_Extent"])?.unwrap_or_default();
    Ok(Bounds { center, extent })
}

fn read_vec3(
    value: &SerializedValue,
    names: &[&'static str],
) -> Result<Option<Vec3>, MeshViewError> {
    for name in names {
        if let Some(field) = lookup(value, name)? {
            let x = read_f32(field, &["x"])?.unwrap_or(0.0);
            let y = read_f32(field, &["y"])?.unwrap_or(0.0);
            let z = read_f32(field, &["z"])?.unwrap_or(0.0);
            return Ok(Some(Vec3 { x, y, z }));
        }
    }
    Ok(None)
}

fn read_bind_pose(root: &SerializedValue) -> Result<Option<Vec<[[f32; 4]; 4]>>, MeshViewError> {
    let Some(values) = read_array(root, &["m_BindPose"])? else {
        return Ok(None);
    };
    let mut matrices = Vec::with_capacity(values.len());
    for value in values {
        let mut matrix = [[0.0f32; 4]; 4];
        // `Matrix4x4f` stores the row-major `E00..E33` fields.
        for (row, row_fields) in matrix.iter_mut().enumerate() {
            for (column, cell) in row_fields.iter_mut().enumerate() {
                let field_name = matrix_field_name(row, column);
                *cell = read_f32(value, &[field_name])?.unwrap_or(0.0);
            }
        }
        matrices.push(matrix);
    }
    Ok(Some(matrices))
}

fn matrix_field_name(row: usize, column: usize) -> &'static str {
    const NAMES: [[&str; 4]; 4] = [
        ["E00", "E01", "E02", "E03"],
        ["E10", "E11", "E12", "E13"],
        ["E20", "E21", "E22", "E23"],
        ["E30", "E31", "E32", "E33"],
    ];
    NAMES[row][column]
}

fn read_skin(root: &SerializedValue) -> Result<Option<Vec<BoneWeight4>>, MeshViewError> {
    let Some(values) = read_array(root, &["m_Skin"])? else {
        return Ok(None);
    };
    let mut skin = Vec::with_capacity(values.len());
    for value in values {
        let mut weights = [0.0f32; 4];
        let mut indices = [0i32; 4];
        for (lane, weight) in weights.iter_mut().enumerate() {
            *weight = read_f32(value, &[weight_field_name(lane)])?.unwrap_or(0.0);
        }
        for (lane, index) in indices.iter_mut().enumerate() {
            *index = read_i32(value, &[index_field_name(lane)])?.unwrap_or(0);
        }
        skin.push(BoneWeight4 { weights, indices });
    }
    Ok(Some(skin))
}

fn weight_field_name(lane: usize) -> &'static str {
    const NAMES: [&str; 4] = ["weight[0]", "weight[1]", "weight[2]", "weight[3]"];
    NAMES[lane]
}

fn index_field_name(lane: usize) -> &'static str {
    const NAMES: [&str; 4] = [
        "boneIndex[0]",
        "boneIndex[1]",
        "boneIndex[2]",
        "boneIndex[3]",
    ];
    NAMES[lane]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serialized_file::object::SerializedFieldValue;

    fn field(name: &str, value: SerializedValue) -> SerializedFieldValue {
        SerializedFieldValue {
            name: name.to_owned(),
            value,
        }
    }

    fn version() -> UnityVersion {
        UnityVersion::parse("2019.4.0f1").unwrap()
    }

    fn make_view(root: &SerializedValue) -> MeshView {
        MeshView::from_value(root, version(), EndianType::Little).unwrap()
    }

    #[test]
    fn index_buffer_decodes_with_the_resolved_format() {
        let root = SerializedValue::Struct(vec![
            field("m_IndexFormat", SerializedValue::I32(0)),
            field("m_IndexBuffer", SerializedValue::Bytes(vec![1, 0, 2, 0])),
        ]);
        let view = make_view(&root);
        assert_eq!(view.resolved_index_format(), IndexFormat::UInt16);
        assert_eq!(view.processed_index_buffer(), vec![1, 2]);

        let root = SerializedValue::Struct(vec![
            field("m_IndexFormat", SerializedValue::I32(1)),
            field(
                "m_IndexBuffer",
                SerializedValue::Bytes(vec![1, 0, 0, 0, 2, 0, 0, 0]),
            ),
        ]);
        let view = make_view(&root);
        assert_eq!(view.resolved_index_format(), IndexFormat::UInt32);
        assert_eq!(view.processed_index_buffer(), vec![1, 2]);
    }

    #[test]
    fn absent_index_format_defaults_to_16_bit() {
        let root = SerializedValue::Struct(vec![field(
            "m_IndexBuffer",
            SerializedValue::Bytes(vec![7, 0]),
        )]);
        assert_eq!(
            make_view(&root).resolved_index_format(),
            IndexFormat::UInt16
        );
    }

    #[test]
    fn empty_mesh_yields_no_mesh_data() {
        let root = SerializedValue::Struct(vec![field("m_Name", SerializedValue::Bytes(vec![]))]);
        let view = make_view(&root);
        assert!(view
            .read_mesh_data(PackedBitVectorLimits::default())
            .unwrap()
            .is_none());
    }

    #[test]
    fn sub_mesh_fields_are_read() {
        let aabb = SerializedValue::Struct(vec![
            field(
                "m_Center",
                SerializedValue::Struct(vec![
                    field("x", SerializedValue::F32(1.0)),
                    field("y", SerializedValue::F32(2.0)),
                    field("z", SerializedValue::F32(3.0)),
                ]),
            ),
            field(
                "m_Extent",
                SerializedValue::Struct(vec![
                    field("x", SerializedValue::F32(0.5)),
                    field("y", SerializedValue::F32(0.5)),
                    field("z", SerializedValue::F32(0.5)),
                ]),
            ),
        ]);
        let sub_mesh = SerializedValue::Struct(vec![
            field("firstByte", SerializedValue::U32(4)),
            field("indexCount", SerializedValue::U32(6)),
            field("topology", SerializedValue::I32(0)),
            field("baseVertex", SerializedValue::U32(0)),
            field("firstVertex", SerializedValue::U32(0)),
            field("vertexCount", SerializedValue::U32(3)),
            field("localAABB", aabb),
        ]);
        let root = SerializedValue::Struct(vec![field(
            "m_SubMeshes",
            SerializedValue::Array(vec![sub_mesh]),
        )]);
        let view = make_view(&root);
        assert_eq!(view.sub_meshes.len(), 1);
        let sub_mesh = view.sub_meshes[0];
        assert_eq!(sub_mesh.first_byte, 4);
        assert_eq!(sub_mesh.get_first_index(true), 2);
        assert_eq!(sub_mesh.index_count, 6);
        assert_eq!(sub_mesh.local_bounds.center.x, 1.0);
        assert_eq!(sub_mesh.local_bounds.extent.z, 0.5);
    }

    #[test]
    fn zero_item_compressed_mesh_is_treated_as_absent() {
        let packed = SerializedValue::Struct(vec![
            field("m_Start", SerializedValue::F32(0.0)),
            field("m_Range", SerializedValue::F32(0.0)),
            field("m_NumItems", SerializedValue::U32(0)),
            field("m_BitSize", SerializedValue::U8(0)),
            field("m_Data", SerializedValue::Bytes(Vec::new())),
        ]);
        let root = SerializedValue::Struct(vec![field(
            "m_CompressedMesh",
            SerializedValue::Struct(vec![
                field("m_Vertices", packed.clone()),
                field("m_Normals", packed.clone()),
                field("m_Tangents", packed.clone()),
                field("m_UV", packed.clone()),
                field("m_BindPoses", packed.clone()),
                field("m_FloatColors", packed.clone()),
                field(
                    "m_Weights",
                    SerializedValue::Struct(vec![
                        field("m_NumItems", SerializedValue::U32(0)),
                        field("m_BitSize", SerializedValue::U8(0)),
                        field("m_Data", SerializedValue::Bytes(Vec::new())),
                    ]),
                ),
                field(
                    "m_BoneIndices",
                    SerializedValue::Struct(vec![
                        field("m_NumItems", SerializedValue::U32(0)),
                        field("m_BitSize", SerializedValue::U8(0)),
                        field("m_Data", SerializedValue::Bytes(Vec::new())),
                    ]),
                ),
                field(
                    "m_NormalSigns",
                    SerializedValue::Struct(vec![
                        field("m_NumItems", SerializedValue::U32(0)),
                        field("m_BitSize", SerializedValue::U8(0)),
                        field("m_Data", SerializedValue::Bytes(Vec::new())),
                    ]),
                ),
                field(
                    "m_TangentSigns",
                    SerializedValue::Struct(vec![
                        field("m_NumItems", SerializedValue::U32(0)),
                        field("m_BitSize", SerializedValue::U8(0)),
                        field("m_Data", SerializedValue::Bytes(Vec::new())),
                    ]),
                ),
                field(
                    "m_Triangles",
                    SerializedValue::Struct(vec![
                        field("m_NumItems", SerializedValue::U32(0)),
                        field("m_BitSize", SerializedValue::U8(0)),
                        field("m_Data", SerializedValue::Bytes(Vec::new())),
                    ]),
                ),
                field(
                    "m_Colors",
                    SerializedValue::Struct(vec![
                        field("m_NumItems", SerializedValue::U32(0)),
                        field("m_BitSize", SerializedValue::U8(0)),
                        field("m_Data", SerializedValue::Bytes(Vec::new())),
                    ]),
                ),
            ]),
        )]);
        assert!(make_view(&root).compressed_mesh.is_none());
    }

    /// Generated-API evidence for the serialized field-name mapping.
    ///
    /// The fixture is the property dump of the shipped `AssetRipper.SourceGenerated.dll`
    /// (probe `Temp/opencode/meshprops`). Combined with the `Pass002_RenameSubnodes` naming rule
    /// (`data[0]` becomes `data_0_`, then `m_`-prefixed and capitalized), these property names
    /// imply the serialized TypeTree names this reader uses.
    #[test]
    fn generated_api_fixture_supports_the_field_name_mapping() {
        let fixture = include_str!("mesh_generated_api_fixture.txt");
        let mut sections: std::collections::HashMap<String, Vec<String>> =
            std::collections::HashMap::new();
        let mut current = String::new();
        for line in fixture.lines() {
            let line = line.trim();
            if let Some(name) = line.strip_prefix("### ") {
                current = name.split(" => ").next().unwrap_or(name).to_owned();
            } else if !line.is_empty() && !current.is_empty() {
                sections
                    .entry(current.clone())
                    .or_default()
                    .push(line.to_owned());
            }
        }
        let properties = |section: &str| -> Vec<String> {
            sections
                .get(section)
                .cloned()
                .unwrap_or_default()
                .iter()
                .filter_map(|line| line.split_whitespace().nth(1).map(str::to_owned))
                .collect()
        };

        let sub_mesh = properties("AssetRipper.SourceGenerated.Subclasses.SubMesh.ISubMesh");
        for expected in [
            "BaseVertex",
            "FirstByte",
            "FirstVertex",
            "IndexCount",
            "IsTriStrip",
            "LocalAABB",
            "Topology",
            "TriangleCount",
            "VertexCount",
        ] {
            assert!(sub_mesh.contains(&expected.to_owned()), "{expected}");
        }

        let bone_weights =
            properties("AssetRipper.SourceGenerated.Subclasses.BoneWeights4.IBoneWeights4");
        for expected in [
            "BoneIndex_0_",
            "BoneIndex_1_",
            "BoneIndex_2_",
            "BoneIndex_3_",
            "Weight_0_",
            "Weight_1_",
            "Weight_2_",
            "Weight_3_",
        ] {
            assert!(bone_weights.contains(&expected.to_owned()), "{expected}");
        }

        let stream_info =
            properties("AssetRipper.SourceGenerated.Subclasses.StreamInfo.IStreamInfo");
        for expected in [
            "ChannelMask",
            "DividerOp",
            "Frequency",
            "Offset",
            "Stride_Byte",
            "Stride_UInt32",
        ] {
            assert!(stream_info.contains(&expected.to_owned()), "{expected}");
        }

        let channel_info =
            properties("AssetRipper.SourceGenerated.Subclasses.ChannelInfo.IChannelInfo");
        for expected in ["Dimension", "Format", "Offset", "Stream"] {
            assert!(channel_info.contains(&expected.to_owned()), "{expected}");
        }

        let mesh = properties("AssetRipper.SourceGenerated.Classes.ClassID_43.IMesh");
        for expected in [
            "BindPose",
            "CompressedMesh",
            "IndexBuffer",
            "IndexFormat",
            "Skin",
            "StreamData",
            "SubMeshes",
            "VertexData",
        ] {
            assert!(mesh.contains(&expected.to_owned()), "{expected}");
        }
    }

    /// One node of a real class-43 release TypeTree.
    #[derive(Clone, Debug)]
    struct TypeTreeNode {
        level: usize,
        type_name: String,
        name: String,
    }

    /// Parses `src/resources/mesh_typetree_fixture.txt` into per-version node lists.
    fn typetree_fixture() -> std::collections::HashMap<String, Vec<TypeTreeNode>> {
        let fixture = include_str!("mesh_typetree_fixture.txt");
        let mut versions: std::collections::HashMap<String, Vec<TypeTreeNode>> =
            std::collections::HashMap::new();
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            if parts[0] == "TREE" {
                versions.entry(parts[1].to_owned()).or_default();
                continue;
            }
            assert_eq!(parts[0], "NODE", "{line}");
            assert_eq!(parts.len(), 11, "{line}");
            versions
                .entry(parts[1].to_owned())
                .or_default()
                .push(TypeTreeNode {
                    level: parts[2].parse().unwrap(),
                    type_name: parts[3].to_owned(),
                    name: parts[4].to_owned(),
                });
        }
        versions
    }

    /// The names of the direct children of the node whose `(level, name)` matches.
    ///
    /// Several levels reuse the name `data` (string bytes, the `SubMesh` element, the
    /// `ChannelInfo` element), so callers that need a later one must pass its `type_name`.
    fn children_of(
        nodes: &[TypeTreeNode],
        level: usize,
        name: &str,
        type_name: Option<&str>,
    ) -> Vec<String> {
        let parent = nodes
            .iter()
            .position(|node| {
                node.level == level
                    && node.name == name
                    && type_name.is_none_or(|expected| node.type_name == expected)
            })
            .unwrap_or_else(|| panic!("no node at level {level} named {name}"));
        let parent_level = nodes[parent].level;
        nodes[parent + 1..]
            .iter()
            .take_while(|node| node.level > parent_level)
            .filter(|node| node.level == parent_level + 1)
            .map(|node| node.name.clone())
            .collect()
    }

    fn node_at<'a>(
        nodes: &'a [TypeTreeNode],
        level: usize,
        name: &str,
        type_name: &str,
    ) -> &'a TypeTreeNode {
        nodes
            .iter()
            .find(|node| node.level == level && node.name == name && node.type_name == type_name)
            .unwrap_or_else(|| panic!("no node at level {level} named {name} of type {type_name}"))
    }

    /// The reader's serialized names must match the real Unity class-43 TypeTrees.
    ///
    /// `src/resources/mesh_typetree_fixture.txt` is the release-node dump of the reference's
    /// `SourceTpk` for Unity 3.5.0a0, 5.0.0a0, 2019.0.0a0, and 6000.6.0a5 (probe
    /// `Temp/opencode/tpkprobe`). It pins the field names, the `SubMesh` child order, the
    /// `ChannelInfo` children, and which version carries each `CompressedMesh` gate field.
    #[test]
    fn real_class43_typetrees_match_the_reader_field_names() {
        let versions = typetree_fixture();
        for version in ["3.5.0a0", "5.0.0a0", "2019.0.0a0", "6000.6.0a5"] {
            let nodes = versions
                .get(version)
                .unwrap_or_else(|| panic!("the fixture has {version}"));
            assert!(nodes.len() > 100, "{version} has a full tree");

            // The reader's top-level names. `m_IndexFormat` exists only on Unity
            // 2017.3+; older files are always 16-bit, which is also the reader's default.
            for field in [
                "m_Name",
                "m_SubMeshes",
                "m_BindPose",
                "m_IndexBuffer",
                "m_VertexData",
                "m_CompressedMesh",
            ] {
                assert!(
                    nodes
                        .iter()
                        .any(|node| node.level == 1 && node.name == field),
                    "{version} has a level-1 {field}"
                );
            }
            let has_index_format = nodes
                .iter()
                .any(|node| node.level == 1 && node.name == "m_IndexFormat");
            assert_eq!(
                has_index_format,
                version == "2019.0.0a0" || version == "6000.6.0a5",
                "{version} m_IndexFormat gate"
            );

            // The `SubMesh` element children, in the order the reader walks them.
            let sub_mesh_children = children_of(nodes, 3, "data", Some("SubMesh"));
            assert_eq!(
                node_at(nodes, 3, "data", "SubMesh").type_name,
                "SubMesh",
                "{version} sub-mesh element type"
            );
            // Unity 3.5 has `isTriStrip`+`triangleCount` instead of `topology`;
            // `baseVertex` was added in Unity 2017.3.
            let mut expected: Vec<&str> = vec!["firstByte", "indexCount"];
            if version == "3.5.0a0" {
                expected.extend(["isTriStrip", "triangleCount"]);
            } else {
                expected.push("topology");
                if version != "5.0.0a0" {
                    expected.push("baseVertex");
                }
            }
            expected.extend(["firstVertex", "vertexCount", "localAABB"]);
            assert_eq!(sub_mesh_children, expected, "{version} SubMesh children");
            assert!(
                nodes
                    .iter()
                    .any(|node| node.level == 5 && node.name == "m_Center"),
                "{version} AABB centre"
            );

            // The `VertexData` children: Unity 3.5 has four fixed `m_Streams[0..3]`
            // and no `m_Channels`; Unity 5.0+ serializes `m_Channels` instead, and
            // Unity 2019+ additionally drops `m_CurrentChannels`.
            let vertex_data_children = children_of(nodes, 1, "m_VertexData", None);
            for field in ["m_VertexCount", "m_DataSize"] {
                assert!(
                    vertex_data_children.iter().any(|name| name == field),
                    "{version} VertexData has {field}"
                );
            }
            let has_channels = vertex_data_children.iter().any(|name| name == "m_Channels");
            assert_eq!(
                has_channels,
                version != "3.5.0a0",
                "{version} m_Channels gate"
            );
            let has_fixed_streams = (0..4).all(|index| {
                vertex_data_children
                    .iter()
                    .any(|name| name == &format!("m_Streams[{index}]"))
            });
            assert_eq!(
                has_fixed_streams,
                version == "3.5.0a0",
                "{version} fixed m_Streams gate"
            );
            if has_channels {
                let channel_children = children_of(nodes, 4, "data", Some("ChannelInfo"));
                assert_eq!(
                    node_at(nodes, 4, "data", "ChannelInfo").type_name,
                    "ChannelInfo",
                    "{version} channel element type"
                );
                for field in ["stream", "offset", "format", "dimension"] {
                    assert!(
                        channel_children.iter().any(|name| name == field),
                        "{version} ChannelInfo has {field}"
                    );
                }
            }

            // The `CompressedMesh` gate fields, and the packed-vector children the reader reads.
            let compressed_children = children_of(nodes, 1, "m_CompressedMesh", None);
            let packed_children = children_of(nodes, 2, "m_Vertices", None);
            for field in ["m_NumItems", "m_Range", "m_Start", "m_Data", "m_BitSize"] {
                assert!(
                    packed_children.iter().any(|name| name == field),
                    "{version} PackedBitVector has {field}"
                );
            }
            for field in [
                "m_Vertices",
                "m_UV",
                "m_Normals",
                "m_Tangents",
                "m_Weights",
                "m_NormalSigns",
                "m_TangentSigns",
                "m_BoneIndices",
                "m_Triangles",
            ] {
                assert!(
                    compressed_children.iter().any(|name| name == field),
                    "{version} CompressedMesh has {field}"
                );
            }
            // The four version gates: `m_BindPoses`/`m_Colors` are Unity 3.5-4.x only, and
            // `m_FloatColors`/`m_UVInfo` are Unity 5.0+ only.
            let pre_unity_5 = version == "3.5.0a0";
            assert_eq!(
                compressed_children.iter().any(|name| name == "m_BindPoses"),
                pre_unity_5,
                "{version} m_BindPoses gate"
            );
            assert_eq!(
                compressed_children.iter().any(|name| name == "m_Colors"),
                pre_unity_5,
                "{version} m_Colors gate"
            );
            assert_eq!(
                compressed_children
                    .iter()
                    .any(|name| name == "m_FloatColors"),
                !pre_unity_5,
                "{version} m_FloatColors gate"
            );
            assert_eq!(
                compressed_children.iter().any(|name| name == "m_UVInfo"),
                !pre_unity_5,
                "{version} m_UVInfo gate"
            );
        }
    }

    /// A Unity 5+ compressed mesh has no `m_BindPoses` or `m_Colors`.
    #[test]
    fn a_unity_5_compressed_mesh_reads_with_the_pre_five_gates_off() {
        let root = SerializedValue::Struct(vec![field(
            "m_CompressedMesh",
            SerializedValue::Struct(vec![
                packed_float("m_Vertices", 0),
                packed_float("m_Normals", 0),
                packed_float("m_Tangents", 0),
                packed_float("m_UV", 0),
                packed_int("m_Weights", 0),
                packed_int("m_BoneIndices", 0),
                packed_int("m_NormalSigns", 0),
                packed_int("m_TangentSigns", 0),
                packed_int("m_Triangles", 3),
                field("m_UVInfo", SerializedValue::U32(0)),
                packed_float("m_FloatColors", 0),
            ]),
        )]);
        let view = make_view(&root);
        let mesh = view.compressed_mesh.expect("the mesh is set");
        assert!(!mesh.has_bind_poses, "Unity 5 has no m_BindPoses");
        assert!(!mesh.has_colors, "Unity 5 has no m_Colors");
        assert!(mesh.has_float_colors);
        assert!(mesh.has_uv_info);
    }

    /// A Unity 3.5/4.x compressed mesh has no `m_FloatColors` or `m_UVInfo`.
    #[test]
    fn a_pre_unity_5_compressed_mesh_reads_with_the_unity_5_gates_off() {
        let root = SerializedValue::Struct(vec![field(
            "m_CompressedMesh",
            SerializedValue::Struct(vec![
                packed_float("m_Vertices", 0),
                packed_float("m_Normals", 0),
                packed_float("m_Tangents", 0),
                packed_float("m_UV", 0),
                packed_int("m_Weights", 0),
                packed_int("m_BoneIndices", 0),
                packed_int("m_NormalSigns", 0),
                packed_int("m_TangentSigns", 0),
                packed_int("m_Triangles", 3),
                packed_float("m_BindPoses", 0),
                packed_int("m_Colors", 0),
            ]),
        )]);
        let view = make_view(&root);
        let mesh = view.compressed_mesh.expect("the mesh is set");
        assert!(mesh.has_bind_poses, "Unity 3.5 has m_BindPoses");
        assert!(mesh.has_colors, "Unity 3.5 has m_Colors");
        assert!(!mesh.has_float_colors, "Unity 3.5 has no m_FloatColors");
        assert!(!mesh.has_uv_info, "Unity 3.5 has no m_UVInfo");
        assert_eq!(mesh.uv_info, 0);
    }

    /// Unity 3.5's four fixed `m_Streams[0..3]` are read when there is no `m_Channels`.
    #[test]
    fn unity_3_5_fixed_streams_are_read_without_channels() {
        fn fixed_stream(channel_mask: u32, offset: u32, stride: u32) -> SerializedValue {
            SerializedValue::Struct(vec![
                field("channelMask", SerializedValue::U32(channel_mask)),
                field("offset", SerializedValue::U32(offset)),
                field("stride", SerializedValue::U32(stride)),
                field("align", SerializedValue::U32(0)),
            ])
        }
        let root = SerializedValue::Struct(vec![field(
            "m_VertexData",
            SerializedValue::Struct(vec![
                field("m_VertexCount", SerializedValue::U32(3)),
                field("m_CurrentChannels", SerializedValue::U32(3)),
                field("m_Streams[0]", fixed_stream(0b11, 0, 24)),
                field("m_Streams[1]", fixed_stream(0b100, 72, 4)),
                field("m_Streams[2]", fixed_stream(0, 84, 0)),
                field("m_Streams[3]", fixed_stream(0, 84, 0)),
                field("m_DataSize", SerializedValue::Bytes(vec![0; 84])),
            ]),
        )]);
        let view = make_view(&root);
        let vertex_data = view.vertex_data.expect("vertex data is present");
        assert_eq!(vertex_data.vertex_count, 3);
        assert_eq!(vertex_data.channels, None);
        let streams = vertex_data.streams.expect("four fixed streams are read");
        assert_eq!(streams.len(), 4);
        assert_eq!(streams[0].channel_mask, 0b11);
        assert_eq!(streams[0].offset, 0);
        assert_eq!(streams[0].stride, 24);
        assert!(streams[0].stride_is_u32, "3.5 stride is UInt32");
        assert_eq!(streams[1].channel_mask, 0b100);
        assert_eq!(streams[1].stride, 4);
        // `dividerOp`/`frequency` do not exist on 3.5 and stay zero.
        assert_eq!(streams[0].divider_op, 0);
        assert_eq!(streams[0].frequency, 0);
    }

    fn packed_float(name: &str, num_items: u32) -> SerializedFieldValue {
        field(
            name,
            SerializedValue::Struct(vec![
                field("m_NumItems", SerializedValue::U32(num_items)),
                field("m_Range", SerializedValue::F32(0.0)),
                field("m_Start", SerializedValue::F32(0.0)),
                field("m_Data", SerializedValue::Bytes(Vec::new())),
                field("m_BitSize", SerializedValue::U8(0)),
            ]),
        )
    }

    fn packed_int(name: &str, num_items: u32) -> SerializedFieldValue {
        field(
            name,
            SerializedValue::Struct(vec![
                field("m_NumItems", SerializedValue::U32(num_items)),
                field("m_Data", SerializedValue::Bytes(Vec::new())),
                field("m_BitSize", SerializedValue::U8(0)),
            ]),
        )
    }
}
