//! `VertexDataBlob` packing and unpacking.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions`' `VertexDataBlob`,
//! `VertexDataExtensions`, `MeshHelper` and `ChannelInfoExtensions`. The vertex data of a mesh
//! is a set of interleaved streams: each stream is a contiguous block aligned to sixteen bytes,
//! each vertex inside a stream occupies a fixed stride, and each channel describes its offset
//! inside that stride. This module reproduces both directions (build from [`MeshData`] and parse
//! back) for the version branches found in the frozen baseline.

use crate::numerics::{BoneWeight4, Color32, ColorFloat, Vec2, Vec3, Vec4};

/// The stream alignment used by Unity vertex data.
const VERTEX_STREAM_ALIGN: i64 = 16;

/// A Unity version with the three numeric components used by the vertex-data branches.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct UnityVersion {
    /// Major component (`5`, `2017`, `2018`, ...).
    pub major: u32,
    /// Minor component.
    pub minor: u32,
    /// Patch component.
    pub patch: u32,
}

impl UnityVersion {
    /// Parse the numeric prefix of a version string, ignoring a trailing release suffix.
    pub fn parse(value: &str) -> Option<Self> {
        let mut parts = value.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch_text = parts.next()?;
        let patch: String = patch_text
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        let patch = patch.parse().ok()?;
        Some(Self {
            major,
            minor,
            patch,
        })
    }

    /// `version.LessThan(major)`.
    pub fn less_than(self, major: u32) -> bool {
        (self.major, self.minor) < (major, 0)
    }

    /// `version.LessThan(major, minor)`.
    pub fn less_than_at(self, major: u32, minor: u32) -> bool {
        (self.major, self.minor) < (major, minor)
    }

    /// `version.GreaterThanOrEquals(major)`.
    pub fn at_least(self, major: u32) -> bool {
        (self.major, self.minor) >= (major, 0)
    }

    /// `version.GreaterThanOrEquals(major, minor)`.
    pub fn at_least_at(self, major: u32, minor: u32) -> bool {
        (self.major, self.minor) >= (major, minor)
    }
}

/// The byte order of a vertex-data blob.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EndianType {
    /// Little endian.
    Little,
    /// Big endian.
    Big,
}

/// A failure while packing or parsing vertex data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VertexDataError {
    /// A channel format does not map to a vertex format for the version.
    UnsupportedFormat {
        /// The raw format byte.
        format: u8,
        /// The Unity version.
        version: UnityVersion,
    },
    /// A channel referenced a stream that does not exist.
    MissingStream {
        /// The channel index.
        channel: usize,
        /// The stream index.
        stream: usize,
    },
    /// A source layout had no channels and no streams.
    MissingLayout,
    /// A float array length was not a multiple of its dimension.
    InvalidFloatLength {
        /// The array length.
        length: usize,
        /// The dimension.
        dimension: usize,
    },
    /// Component bytes were not divisible by the component byte size.
    NotDivisible {
        /// The byte length.
        length: usize,
        /// The component byte size.
        size: usize,
    },
    /// The blob ended before a component could be read.
    TruncatedData {
        /// The offset required.
        offset: usize,
        /// The byte count required.
        length: usize,
        /// The total available byte count.
        data_length: usize,
    },
}

impl core::fmt::Display for VertexDataError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnsupportedFormat { format, version } => write!(
                formatter,
                "channel format {format} is not valid for Unity {}.{}.{}",
                version.major, version.minor, version.patch
            ),
            Self::MissingStream { channel, stream } => write!(
                formatter,
                "channel {channel} references stream {stream}, which does not exist"
            ),
            Self::MissingLayout => {
                write!(formatter, "vertex data has neither channels nor streams")
            }
            Self::InvalidFloatLength { length, dimension } => write!(
                formatter,
                "float array length {length} is not divisible by dimension {dimension}"
            ),
            Self::NotDivisible { length, size } => write!(
                formatter,
                "component bytes length {length} is not divisible by size {size}"
            ),
            Self::TruncatedData {
                offset,
                length,
                data_length,
            } => write!(
                formatter,
                "vertex data needs bytes {offset}..{} but only {data_length} are available",
                offset + length
            ),
        }
    }
}

impl std::error::Error for VertexDataError {}

/// The decoded component format of a vertex channel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VertexFormat {
    /// 32-bit float.
    Float,
    /// 16-bit float.
    Float16,
    /// 8-bit unsigned normalized.
    UNorm8,
    /// 8-bit signed normalized.
    SNorm8,
    /// 16-bit unsigned normalized.
    UNorm16,
    /// 16-bit signed normalized.
    SNorm16,
    /// 8-bit unsigned integer.
    UInt8,
    /// 8-bit signed integer.
    SInt8,
    /// 16-bit unsigned integer.
    UInt16,
    /// 16-bit signed integer.
    SInt16,
    /// 32-bit unsigned integer.
    UInt32,
    /// 32-bit signed integer.
    SInt32,
}

impl VertexFormat {
    /// `MeshHelper.GetFormatSize`.
    pub fn get_format_size(self) -> usize {
        match self {
            Self::Float | Self::UInt32 | Self::SInt32 => 4,
            Self::Float16 | Self::UNorm16 | Self::SNorm16 | Self::UInt16 | Self::SInt16 => 2,
            Self::UNorm8 | Self::SNorm8 | Self::UInt8 | Self::SInt8 => 1,
        }
    }

    /// `MeshHelper.IsIntFormat`.
    pub fn is_int_format(self) -> bool {
        (self as u8) >= (Self::UInt8 as u8)
    }
}

/// `MeshHelper.ToVertexFormat`.
pub fn to_vertex_format(
    format: u8,
    version: UnityVersion,
) -> Result<VertexFormat, VertexDataError> {
    if version.less_than(2017) {
        match format {
            0 => Ok(VertexFormat::Float),
            1 => Ok(VertexFormat::Float16),
            2 => Ok(VertexFormat::UNorm8),
            3 => Ok(VertexFormat::UInt8),
            4 => Ok(VertexFormat::UInt32),
            _ => Err(VertexDataError::UnsupportedFormat { format, version }),
        }
    } else {
        map_2017_format(format, version)
    }
}

fn map_2017_format(format: u8, version: UnityVersion) -> Result<VertexFormat, VertexDataError> {
    match format {
        0 => Ok(VertexFormat::Float),
        1 => Ok(VertexFormat::Float16),
        2 => Ok(VertexFormat::UNorm8),
        3 => Ok(VertexFormat::SNorm8),
        4 => Ok(VertexFormat::UNorm16),
        5 => Ok(VertexFormat::SNorm16),
        6 => Ok(VertexFormat::UInt8),
        7 => Ok(VertexFormat::SInt8),
        8 => Ok(VertexFormat::UInt16),
        9 => Ok(VertexFormat::SInt16),
        10 => Ok(VertexFormat::UInt32),
        11 => Ok(VertexFormat::SInt32),
        _ => Err(VertexDataError::UnsupportedFormat { format, version }),
    }
}

/// `MeshHelper.GetChannelFormat_Color`: the color channel format byte for a version.
pub fn get_channel_format_color(version: UnityVersion) -> u8 {
    // All three version branches return kChannelFormatColor/kVertexFormatColor/kVertexFormatUNorm8,
    // which are all value 2.
    let _ = version;
    2
}

/// One channel of a vertex stream.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ChannelInfo {
    /// Raw dimension byte.
    pub dimension: u8,
    /// Offset inside the stream.
    pub offset: u8,
    /// Raw format byte.
    pub format: u8,
    /// Stream index.
    pub stream: u8,
}

impl ChannelInfo {
    /// `ChannelInfoExtensions.GetDataDimension`.
    pub fn get_data_dimension(self) -> u8 {
        self.dimension & 0b0000_1111
    }

    /// `ChannelInfoExtensions.SetDataDimension`.
    pub fn set_data_dimension(&mut self, value: u8) {
        self.dimension = (self.dimension & 0b1111_0000) | (value & 0b0000_1111);
    }

    /// `ChannelInfoExtensions.IsSet`.
    pub fn is_set(self) -> bool {
        self.dimension > 0
    }
}

/// One stream of a vertex blob.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StreamInfo {
    /// Bitset of channels present in this stream.
    pub channel_mask: u32,
    /// Byte offset of the stream in the blob.
    pub offset: u32,
    /// Effective stride (`Stride_Byte` or `Stride_UInt32`).
    pub stride: u32,
    /// Whether the stride is stored as a 32-bit value rather than a byte.
    pub stride_is_u32: bool,
    /// Divider operation.
    pub divider_op: u8,
    /// Frequency.
    pub frequency: u8,
}

impl StreamInfo {
    /// `StreamInfoExtensions.GetStride`.
    pub fn get_stride(self) -> u32 {
        self.stride
    }
}

/// The decoded mesh channels.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshData {
    /// Vertices.
    pub vertices: Vec<Vec3>,
    /// Normals.
    pub normals: Option<Vec<Vec3>>,
    /// Tangents.
    pub tangents: Option<Vec<Vec4>>,
    /// Colors.
    pub colors: Option<Vec<ColorFloat>>,
    /// UV0.
    pub uv0: Option<Vec<Vec2>>,
    /// UV1.
    pub uv1: Option<Vec<Vec2>>,
    /// UV2.
    pub uv2: Option<Vec<Vec2>>,
    /// UV3.
    pub uv3: Option<Vec<Vec2>>,
    /// UV4.
    pub uv4: Option<Vec<Vec2>>,
    /// UV5.
    pub uv5: Option<Vec<Vec2>>,
    /// UV6.
    pub uv6: Option<Vec<Vec2>>,
    /// UV7.
    pub uv7: Option<Vec<Vec2>>,
    /// Bone weights and indices.
    pub skin: Option<Vec<BoneWeight4>>,
}

impl MeshData {
    fn has_normals(&self) -> bool {
        self.normals
            .as_ref()
            .is_some_and(|values| values.len() == self.vertices.len())
    }

    fn has_tangents(&self) -> bool {
        self.tangents
            .as_ref()
            .is_some_and(|values| values.len() == self.vertices.len())
    }

    fn has_colors(&self) -> bool {
        self.colors
            .as_ref()
            .is_some_and(|values| values.len() == self.vertices.len())
    }

    fn has_uv(&self, index: usize) -> bool {
        self.uv(index)
            .is_some_and(|values| values.len() == self.vertices.len())
    }

    pub(crate) fn uv(&self, index: usize) -> Option<&Vec<Vec2>> {
        match index {
            0 => self.uv0.as_ref(),
            1 => self.uv1.as_ref(),
            2 => self.uv2.as_ref(),
            3 => self.uv3.as_ref(),
            4 => self.uv4.as_ref(),
            5 => self.uv5.as_ref(),
            6 => self.uv6.as_ref(),
            7 => self.uv7.as_ref(),
            _ => None,
        }
    }
}

/// A packed vertex-data blob.
#[derive(Clone, Debug, PartialEq)]
pub struct VertexDataBlob {
    /// Channels.
    pub channels: Vec<ChannelInfo>,
    /// Streams.
    pub streams: Vec<StreamInfo>,
    /// Raw blob bytes.
    pub data: Vec<u8>,
    /// Vertex count.
    pub vertex_count: i32,
    /// Unity version.
    pub version: UnityVersion,
    /// Byte order.
    pub endian: EndianType,
}

impl VertexDataBlob {
    /// `VertexDataBlob.Create(IVertexData, version, endian)` from explicit channels and streams.
    pub fn from_parts(
        channels: Option<Vec<ChannelInfo>>,
        streams: Option<Vec<StreamInfo>>,
        data: Vec<u8>,
        vertex_count: u32,
        version: UnityVersion,
        endian: EndianType,
    ) -> Result<Self, VertexDataError> {
        let (channels, streams) = match channels {
            Some(channels) => {
                let streams = match streams {
                    Some(streams) => streams,
                    None => convert_channels_to_streams(&channels, vertex_count, version)?,
                };
                (channels, streams)
            }
            None => {
                let streams = streams.ok_or(VertexDataError::MissingLayout)?;
                let channels = convert_streams_to_channels(&streams, version)?;
                (channels, streams)
            }
        };
        Ok(Self {
            channels,
            streams,
            data,
            vertex_count: vertex_count as i32,
            version,
            endian,
        })
    }

    /// `VertexDataBlob.Create(MeshData, version, endian)`.
    pub fn create_from_mesh_data(
        mesh_data: &MeshData,
        version: UnityVersion,
        endian: EndianType,
    ) -> Result<Self, VertexDataError> {
        if mesh_data.vertices.is_empty() {
            return Ok(Self {
                channels: Vec::new(),
                streams: Vec::new(),
                data: Vec::new(),
                vertex_count: 0,
                version,
                endian,
            });
        }

        let mut data = Vec::new();
        let mut channels = Vec::new();
        let vertex_count = mesh_data.vertices.len();

        if version.at_least(2018) {
            // kShaderChannelVertex
            channels.push(float_channel(3, 0));
            write_vec3_array(&mut data, &mesh_data.vertices, endian);
            write_alignment(&mut data);

            // kShaderChannelNormal
            if mesh_data.has_normals() {
                channels.push(float_channel(3, 1));
                write_vec3_array(&mut data, mesh_data.normals.as_ref().unwrap(), endian);
                write_alignment(&mut data);
            } else {
                channels.push(float_channel(0, 1));
            }

            // kShaderChannelTangent
            if mesh_data.has_tangents() {
                channels.push(float_channel(4, 2));
                write_vec4_array(&mut data, mesh_data.tangents.as_ref().unwrap(), endian);
                write_alignment(&mut data);
            } else {
                channels.push(float_channel(0, 2));
            }

            // kShaderChannelColor
            if mesh_data.has_colors() {
                channels.push(color_channel(4, 3));
                for color in mesh_data.colors.as_ref().unwrap() {
                    write_color32(&mut data, Color32::from(*color));
                }
                write_alignment(&mut data);
            } else {
                channels.push(float_channel(0, 3));
            }

            // kShaderChannelTexCoord0..7
            for index in 0..8 {
                let stream = 4 + index as u8;
                if mesh_data.has_uv(index) {
                    channels.push(float_channel(2, stream));
                    write_vec2_array(&mut data, mesh_data.uv(index).unwrap(), endian);
                    write_alignment(&mut data);
                } else {
                    channels.push(float_channel(0, stream));
                }
            }

            // kShaderChannelBlendWeight / kShaderChannelBlendIndices (2018.2+)
            if version.at_least_at(2018, 2) {
                channels.push(float_channel(0, 12));
                channels.push(float_channel(0, 13));
            }
        } else {
            // kShaderChannelVertex
            channels.push(float_channel(3, 0));
            write_vec3_array(&mut data, &mesh_data.vertices, endian);
            write_alignment(&mut data);

            // kShaderChannelNormal
            if mesh_data.has_normals() {
                channels.push(float_channel(3, 1));
                write_vec3_array(&mut data, mesh_data.normals.as_ref().unwrap(), endian);
                write_alignment(&mut data);
            } else {
                channels.push(float_channel(0, 1));
            }

            // kShaderChannelColor
            if mesh_data.has_colors() {
                channels.push(color_channel(1, 2));
                for color in mesh_data.colors.as_ref().unwrap() {
                    write_color32(&mut data, Color32::from(*color));
                }
                write_alignment(&mut data);
            } else {
                channels.push(float_channel(0, 2));
            }

            // kShaderChannelTexCoord0
            if mesh_data.has_uv(0) {
                channels.push(float_channel(2, 3));
                write_vec2_array(&mut data, mesh_data.uv(0).unwrap(), endian);
                write_alignment(&mut data);
            } else {
                channels.push(float_channel(0, 3));
            }

            // kShaderChannelTexCoord1
            if mesh_data.has_uv(1) {
                channels.push(float_channel(2, 4));
                write_vec2_array(&mut data, mesh_data.uv(1).unwrap(), endian);
                write_alignment(&mut data);
            } else {
                channels.push(float_channel(0, 4));
            }

            if version.less_than(5) {
                // kShaderChannelTangent
                if mesh_data.has_tangents() {
                    channels.push(float_channel(4, 5));
                    write_vec4_array(&mut data, mesh_data.tangents.as_ref().unwrap(), endian);
                    write_alignment(&mut data);
                } else {
                    channels.push(float_channel(0, 5));
                }
            } else {
                // kShaderChannelTexCoord2
                if mesh_data.has_uv(2) {
                    channels.push(float_channel(2, 5));
                    write_vec2_array(&mut data, mesh_data.uv(2).unwrap(), endian);
                    write_alignment(&mut data);
                } else {
                    channels.push(float_channel(0, 5));
                }

                // kShaderChannelTexCoord3
                if mesh_data.has_uv(3) {
                    channels.push(float_channel(2, 6));
                    write_vec2_array(&mut data, mesh_data.uv(3).unwrap(), endian);
                    write_alignment(&mut data);
                } else {
                    channels.push(float_channel(0, 6));
                }

                // kShaderChannelTangent
                if mesh_data.has_tangents() {
                    channels.push(float_channel(4, 7));
                    write_vec4_array(&mut data, mesh_data.tangents.as_ref().unwrap(), endian);
                    write_alignment(&mut data);
                } else {
                    channels.push(float_channel(0, 7));
                }
            }
        }

        let streams = convert_channels_to_streams(&channels, vertex_count as u32, version)?;
        Ok(Self {
            channels,
            streams,
            data,
            vertex_count: vertex_count as i32,
            version,
            endian,
        })
    }

    /// `VertexDataBlob.ToMeshData` / `ReadData`.
    pub fn read_data(&self) -> Result<Option<MeshData>, VertexDataError> {
        let (mesh, vertices_set, _) = self.read_decoded_data()?;
        Ok(vertices_set.then_some(mesh))
    }

    /// The channel arrays returned by `VertexDataBlob.ReadData`, including when its boolean
    /// return is false because the position channel is absent. `MeshExtensions.ReadData` ignores
    /// that boolean and merges these arrays with `CompressedMesh` channels.
    pub(crate) fn read_channels_for_mesh_merge(&self) -> Result<Option<MeshData>, VertexDataError> {
        let (mesh, vertices_set, channel_data_set) = self.read_decoded_data()?;
        Ok((vertices_set || channel_data_set).then_some(mesh))
    }

    fn read_decoded_data(&self) -> Result<(MeshData, bool, bool), VertexDataError> {
        if self.data.is_empty() {
            return Ok((MeshData::default(), false, false));
        }
        let vertex_count =
            usize::try_from(self.vertex_count).map_err(|_| VertexDataError::TruncatedData {
                offset: 0,
                length: 0,
                data_length: self.data.len(),
            })?;

        let mut mesh = MeshData::default();
        let mut vertices_set = false;
        let mut channel_data_set = false;

        for (chn, channel) in self.channels.iter().enumerate() {
            let dimension = get_channel_dimension(*channel, chn, self.version)?;
            if dimension == 0 {
                continue;
            }
            let stream = self.streams.get(channel.stream as usize).ok_or(
                VertexDataError::MissingStream {
                    channel: chn,
                    stream: channel.stream as usize,
                },
            )?;
            if !get_bit(stream.channel_mask, chn) {
                continue;
            }

            let format = to_vertex_format(channel.format, self.version)?;
            let component_byte_size = format.get_format_size();
            let dimension = dimension as usize;
            let mut component_bytes = vec![0u8; vertex_count * dimension * component_byte_size];
            for vertex in 0..vertex_count {
                let vertex_offset = stream.offset as usize
                    + channel.offset as usize
                    + stream.get_stride() as usize * vertex;
                for d in 0..dimension {
                    let component_offset = vertex_offset + component_byte_size * d;
                    let destination = component_byte_size * (vertex * dimension + d);
                    if component_offset + component_byte_size > self.data.len() {
                        return Err(VertexDataError::TruncatedData {
                            offset: component_offset,
                            length: component_byte_size,
                            data_length: self.data.len(),
                        });
                    }
                    component_bytes[destination..destination + component_byte_size]
                        .copy_from_slice(
                            &self.data[component_offset..component_offset + component_byte_size],
                        );
                }
            }

            if self.endian == EndianType::Big && component_byte_size > 1 {
                swap_components(&mut component_bytes, component_byte_size)?;
            }

            if format.is_int_format() {
                let components = bytes_to_int_array(&component_bytes, format)?;
                channel_data_set |=
                    self.assign_int_channel(&mut mesh, chn, dimension, &components, vertex_count)?;
            } else {
                let components = bytes_to_float_array(&component_bytes, format)?;
                channel_data_set |= self.assign_float_channel(
                    &mut mesh,
                    chn,
                    dimension,
                    &components,
                    vertex_count,
                )?;
            }
            if chn == 0 {
                vertices_set = true;
            }
        }

        Ok((mesh, vertices_set, channel_data_set))
    }

    fn assign_float_channel(
        &self,
        mesh: &mut MeshData,
        chn: usize,
        dimension: usize,
        components: &[f32],
        vertex_count: usize,
    ) -> Result<bool, VertexDataError> {
        let channel_was_assigned = if self.version.at_least(2018) {
            match chn {
                0 => {
                    mesh.vertices = float_array_to_vector3(components, dimension)?;
                    true
                }
                1 => {
                    mesh.normals = Some(float_array_to_vector3(components, dimension)?);
                    true
                }
                2 => {
                    mesh.tangents = Some(float_array_to_vector4(components, dimension)?);
                    true
                }
                3 => {
                    mesh.colors = Some(float_array_to_color_float(components)?);
                    true
                }
                4 => {
                    mesh.uv0 = Some(float_array_to_vector2(components, dimension)?);
                    true
                }
                5 => {
                    mesh.uv1 = Some(float_array_to_vector2(components, dimension)?);
                    true
                }
                6 => {
                    mesh.uv2 = Some(float_array_to_vector2(components, dimension)?);
                    true
                }
                7 => {
                    mesh.uv3 = Some(float_array_to_vector2(components, dimension)?);
                    true
                }
                8 => {
                    mesh.uv4 = Some(float_array_to_vector2(components, dimension)?);
                    true
                }
                9 => {
                    mesh.uv5 = Some(float_array_to_vector2(components, dimension)?);
                    true
                }
                10 => {
                    mesh.uv6 = Some(float_array_to_vector2(components, dimension)?);
                    true
                }
                11 => {
                    mesh.uv7 = Some(float_array_to_vector2(components, dimension)?);
                    true
                }
                12 => {
                    let skin = skin_slot(mesh, vertex_count);
                    for i in 0..skin.len() {
                        for j in 0..dimension {
                            skin[i].weights[j] = components[i * dimension + j];
                        }
                    }
                    true
                }
                13 => false, // Indices are integer and handled elsewhere.
                _ => false,
            }
        } else {
            match chn {
                0 => {
                    mesh.vertices = float_array_to_vector3(components, dimension)?;
                    true
                }
                1 => {
                    mesh.normals = Some(float_array_to_vector3(components, dimension)?);
                    true
                }
                2 => {
                    mesh.colors = Some(float_array_to_color_float(components)?);
                    true
                }
                3 => {
                    mesh.uv0 = Some(float_array_to_vector2(components, dimension)?);
                    true
                }
                4 => {
                    mesh.uv1 = Some(float_array_to_vector2(components, dimension)?);
                    true
                }
                5 => {
                    if self.version.at_least(5) {
                        mesh.uv2 = Some(float_array_to_vector2(components, dimension)?);
                        true
                    } else {
                        mesh.tangents = Some(float_array_to_vector4(components, dimension)?);
                        true
                    }
                }
                6 => {
                    mesh.uv3 = Some(float_array_to_vector2(components, dimension)?);
                    true
                }
                7 => {
                    mesh.tangents = Some(float_array_to_vector4(components, dimension)?);
                    true
                }
                _ => false,
            }
        };
        Ok(channel_was_assigned)
    }

    fn assign_int_channel(
        &self,
        mesh: &mut MeshData,
        chn: usize,
        dimension: usize,
        components: &[i32],
        vertex_count: usize,
    ) -> Result<bool, VertexDataError> {
        // The only integer channel with vertex-data semantics is the 2018.2+ bone-index channel.
        if self.version.at_least_at(2018, 2) && chn == 13 {
            let skin = skin_slot(mesh, vertex_count);
            for i in 0..skin.len() {
                for j in 0..dimension {
                    skin[i].indices[j] = components[i * dimension + j];
                }
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// `VertexDataBlob.MakeLittleEndian`.
    pub fn make_little_endian(&self) -> Result<Self, VertexDataError> {
        if self.endian == EndianType::Little {
            return Ok(self.clone());
        }
        let mesh_data = self.read_data()?.unwrap_or_default();
        Self::create_from_mesh_data(&mesh_data, self.version, EndianType::Little)
    }

    /// `VertexDataBlob.GetStreamStride`.
    pub fn get_stream_stride(&self, stream: usize) -> i32 {
        self.streams[stream].get_stride() as i32
    }

    /// `VertexDataBlob.GetStreamSize`.
    pub fn get_stream_size(&self, stream: usize) -> i32 {
        self.get_stream_stride(stream) * self.vertex_count
    }

    /// `VertexDataBlob.GetStreamOffset`.
    pub fn get_stream_offset(&self, stream: usize) -> i32 {
        let mut offset = 0i32;
        for i in 0..stream {
            offset += self.get_stream_size(i);
            offset =
                (offset + (VERTEX_STREAM_ALIGN as i32 - 1)) & !(VERTEX_STREAM_ALIGN as i32 - 1);
        }
        offset
    }
}

/// `VertexDataBlob.SupportsUV2`.
pub fn supports_uv2(version: UnityVersion) -> bool {
    version.at_least(5)
}

/// `VertexDataBlob.SupportsUV3`.
pub fn supports_uv3(version: UnityVersion) -> bool {
    version.at_least(5)
}

/// `VertexDataBlob.SupportsUV4`.
pub fn supports_uv4(version: UnityVersion) -> bool {
    version.at_least(2018)
}

/// `VertexDataBlob.SupportsUV5`.
pub fn supports_uv5(version: UnityVersion) -> bool {
    version.at_least(2018)
}

/// `VertexDataBlob.SupportsUV6`.
pub fn supports_uv6(version: UnityVersion) -> bool {
    version.at_least(2018)
}

/// `VertexDataBlob.SupportsUV7`.
pub fn supports_uv7(version: UnityVersion) -> bool {
    version.at_least(2018)
}

/// `VertexDataBlob.SupportsSkin`.
pub fn supports_skin(version: UnityVersion) -> bool {
    version.at_least_at(2018, 2)
}

fn skin_slot(mesh: &mut MeshData, vertex_count: usize) -> &mut Vec<BoneWeight4> {
    if mesh.skin.is_none() {
        mesh.skin = Some(vec![BoneWeight4::default(); vertex_count]);
    }
    mesh.skin.as_mut().unwrap()
}

fn float_channel(dimension: u8, stream: u8) -> ChannelInfo {
    // `CreateChannelInfoForFloatVector` always resolves to channel format 0 for every version.
    ChannelInfo {
        dimension,
        offset: 0,
        format: 0,
        stream,
    }
}

fn color_channel(dimension: u8, stream: u8) -> ChannelInfo {
    ChannelInfo {
        dimension,
        offset: 0,
        format: 2,
        stream,
    }
}

fn write_alignment(data: &mut Vec<u8>) {
    let position = data.len();
    let align = VERTEX_STREAM_ALIGN as usize - position % VERTEX_STREAM_ALIGN as usize;
    if align != VERTEX_STREAM_ALIGN as usize {
        data.resize(data.len() + align, 0);
    }
}

fn write_f32(data: &mut Vec<u8>, value: f32, endian: EndianType) {
    match endian {
        EndianType::Little => data.extend_from_slice(&value.to_le_bytes()),
        EndianType::Big => data.extend_from_slice(&value.to_be_bytes()),
    }
}

fn write_vec2(data: &mut Vec<u8>, value: Vec2, endian: EndianType) {
    write_f32(data, value.x, endian);
    write_f32(data, value.y, endian);
}

fn write_vec3(data: &mut Vec<u8>, value: Vec3, endian: EndianType) {
    write_f32(data, value.x, endian);
    write_f32(data, value.y, endian);
    write_f32(data, value.z, endian);
}

fn write_vec4(data: &mut Vec<u8>, value: Vec4, endian: EndianType) {
    write_f32(data, value.x, endian);
    write_f32(data, value.y, endian);
    write_f32(data, value.z, endian);
    write_f32(data, value.w, endian);
}

fn write_vec2_array(data: &mut Vec<u8>, values: &[Vec2], endian: EndianType) {
    for value in values {
        write_vec2(data, *value, endian);
    }
}

fn write_vec3_array(data: &mut Vec<u8>, values: &[Vec3], endian: EndianType) {
    for value in values {
        write_vec3(data, *value, endian);
    }
}

fn write_vec4_array(data: &mut Vec<u8>, values: &[Vec4], endian: EndianType) {
    for value in values {
        write_vec4(data, *value, endian);
    }
}

fn write_color32(data: &mut Vec<u8>, value: Color32) {
    data.extend_from_slice(&[value.r, value.g, value.b, value.a]);
}

fn convert_channels_to_streams(
    channels: &[ChannelInfo],
    vertex_count: u32,
    version: UnityVersion,
) -> Result<Vec<StreamInfo>, VertexDataError> {
    if channels.is_empty() {
        return Ok(Vec::new());
    }
    let stream_count = channels
        .iter()
        .map(|channel| channel.stream as usize)
        .max()
        .map_or(0, |value| value + 1);
    let mut streams = Vec::with_capacity(stream_count);
    let mut offset: i64 = 0;
    for stream in 0..stream_count {
        let mut channel_mask = 0u32;
        let mut stride = 0u32;
        for (chn, channel) in channels.iter().enumerate() {
            if channel.stream as usize == stream {
                let dimension = get_channel_dimension(*channel, chn, version)?;
                if dimension > 0 {
                    channel_mask |= 1u32 << chn;
                    stride += dimension as u32
                        * to_vertex_format(channel.format, version)?.get_format_size() as u32;
                }
            }
        }
        streams.push(StreamInfo {
            channel_mask,
            offset: offset as u32,
            stride: (stride as u8) as u32,
            stride_is_u32: false,
            divider_op: 0,
            frequency: 0,
        });
        offset += vertex_count as i64 * stride as i64;
        offset = align_stream(offset);
    }
    Ok(streams)
}

fn convert_streams_to_channels(
    streams: &[StreamInfo],
    version: UnityVersion,
) -> Result<Vec<ChannelInfo>, VertexDataError> {
    let mut channels = Vec::with_capacity(6);
    for (s, stream) in streams.iter().enumerate() {
        let mut offset: u8 = 0;
        for i in 0..6usize {
            if !get_bit(stream.channel_mask, i) {
                continue;
            }
            let mut channel = ChannelInfo {
                dimension: 0,
                offset,
                format: 0,
                stream: s as u8,
            };
            match i {
                0 | 1 => {
                    channel.format = 0;
                    channel.set_data_dimension(3);
                }
                2 => {
                    channel.format = 2;
                    channel.set_data_dimension(4);
                }
                3 | 4 => {
                    channel.format = 0;
                    channel.set_data_dimension(2);
                }
                5 => {
                    channel.format = 0;
                    channel.set_data_dimension(4);
                }
                _ => {}
            }
            offset = offset.wrapping_add(
                (channel.get_data_dimension() as usize
                    * to_vertex_format(channel.format, version)?.get_format_size())
                    as u8,
            );
            channels.push(channel);
        }
    }
    Ok(channels)
}

fn align_stream(offset: i64) -> i64 {
    (offset + (VERTEX_STREAM_ALIGN - 1)) & !(VERTEX_STREAM_ALIGN - 1)
}

fn get_channel_dimension(
    channel: ChannelInfo,
    chn: usize,
    version: UnityVersion,
) -> Result<u8, VertexDataError> {
    if version.less_than(2018) && chn == 2 && channel.format == 2 {
        Ok(4)
    } else {
        Ok(channel.get_data_dimension())
    }
}

fn get_bit(mask: u32, index: usize) -> bool {
    (mask & (1u32 << index)) != 0
}

fn swap_components(bytes: &mut [u8], size: usize) -> Result<(), VertexDataError> {
    if size <= 1 {
        return Ok(());
    }
    if !bytes.len().is_multiple_of(size) {
        return Err(VertexDataError::NotDivisible {
            length: bytes.len(),
            size,
        });
    }
    for chunk in bytes.chunks_exact_mut(size) {
        chunk.reverse();
    }
    Ok(())
}

fn bytes_to_float_array(bytes: &[u8], format: VertexFormat) -> Result<Vec<f32>, VertexDataError> {
    let size = format.get_format_size();
    if !bytes.len().is_multiple_of(size) {
        return Err(VertexDataError::NotDivisible {
            length: bytes.len(),
            size,
        });
    }
    let mut result = Vec::with_capacity(bytes.len() / size);
    for chunk in bytes.chunks_exact(size) {
        let value = match format {
            VertexFormat::Float => f32::from_le_bytes(chunk.try_into().unwrap()),
            VertexFormat::Float16 => half_to_f32(u16::from_le_bytes(chunk.try_into().unwrap())),
            VertexFormat::UNorm8 => chunk[0] as f32 / 255.0,
            VertexFormat::SNorm8 => ((chunk[0] as i8) as f32 / 127.0).max(-1.0),
            VertexFormat::UNorm16 => u16::from_le_bytes(chunk.try_into().unwrap()) as f32 / 65535.0,
            VertexFormat::SNorm16 => {
                (i16::from_le_bytes(chunk.try_into().unwrap()) as f32 / 32767.0).max(-1.0)
            }
            _ => 0.0,
        };
        result.push(value);
    }
    Ok(result)
}

fn bytes_to_int_array(bytes: &[u8], format: VertexFormat) -> Result<Vec<i32>, VertexDataError> {
    let size = format.get_format_size();
    if !bytes.len().is_multiple_of(size) {
        return Err(VertexDataError::NotDivisible {
            length: bytes.len(),
            size,
        });
    }
    let mut result = Vec::with_capacity(bytes.len() / size);
    for chunk in bytes.chunks_exact(size) {
        let value = match format {
            VertexFormat::UInt8 | VertexFormat::SInt8 => chunk[0] as i32,
            VertexFormat::UInt16 | VertexFormat::SInt16 => {
                i16::from_le_bytes(chunk.try_into().unwrap()) as i32
            }
            VertexFormat::UInt32 | VertexFormat::SInt32 => {
                i32::from_le_bytes(chunk.try_into().unwrap())
            }
            _ => 0,
        };
        result.push(value);
    }
    Ok(result)
}

fn float_array_to_vector2(input: &[f32], dimension: usize) -> Result<Vec<Vec2>, VertexDataError> {
    validate_dimension(input, dimension)?;
    let mut result = Vec::with_capacity(input.len() / dimension);
    for i in 0..input.len() / dimension {
        let base = dimension * i;
        result.push(match dimension {
            1 => Vec2::new(input[i], 0.0),
            2 => Vec2::new(input[base], input[base + 1]),
            3 => Vec2::new(input[base], input[base + 1]),
            _ => Vec2::new(input[base], input[base + 1]),
        });
    }
    Ok(result)
}

fn float_array_to_vector3(input: &[f32], dimension: usize) -> Result<Vec<Vec3>, VertexDataError> {
    validate_dimension(input, dimension)?;
    let mut result = Vec::with_capacity(input.len() / dimension);
    for i in 0..input.len() / dimension {
        let base = dimension * i;
        result.push(match dimension {
            1 => Vec3 {
                x: input[i],
                y: 0.0,
                z: 0.0,
            },
            2 => Vec3 {
                x: input[base],
                y: input[base + 1],
                z: 0.0,
            },
            3 => Vec3 {
                x: input[base],
                y: input[base + 1],
                z: input[base + 2],
            },
            _ => Vec3 {
                x: input[base],
                y: input[base + 1],
                z: input[base + 2],
            },
        });
    }
    Ok(result)
}

fn float_array_to_vector4(input: &[f32], dimension: usize) -> Result<Vec<Vec4>, VertexDataError> {
    validate_dimension(input, dimension)?;
    let mut result = Vec::with_capacity(input.len() / dimension);
    for i in 0..input.len() / dimension {
        let base = dimension * i;
        result.push(match dimension {
            1 => Vec4::new(input[i], 0.0, 0.0, 0.0),
            2 => Vec4::new(input[base], input[base + 1], 0.0, 0.0),
            3 => Vec4::new(input[base], input[base + 1], input[base + 2], 0.0),
            _ => Vec4::new(
                input[base],
                input[base + 1],
                input[base + 2],
                input[base + 3],
            ),
        });
    }
    Ok(result)
}

fn float_array_to_color_float(input: &[f32]) -> Result<Vec<ColorFloat>, VertexDataError> {
    if !input.len().is_multiple_of(4) {
        return Err(VertexDataError::InvalidFloatLength {
            length: input.len(),
            dimension: 4,
        });
    }
    let mut result = Vec::with_capacity(input.len() / 4);
    for i in 0..input.len() / 4 {
        let base = 4 * i;
        result.push(ColorFloat::new(
            input[base],
            input[base + 1],
            input[base + 2],
            input[base + 3],
        ));
    }
    Ok(result)
}

fn validate_dimension(input: &[f32], dimension: usize) -> Result<(), VertexDataError> {
    if !(1..=4).contains(&dimension) || !input.len().is_multiple_of(dimension) {
        return Err(VertexDataError::InvalidFloatLength {
            length: input.len(),
            dimension,
        });
    }
    Ok(())
}

fn half_to_f32(bits: u16) -> f32 {
    let sign = (bits >> 15) & 1;
    let exponent = (bits >> 10) & 0x1F;
    let mantissa = bits & 0x3FF;
    let sign_factor = if sign == 1 { -1.0f32 } else { 1.0 };
    match exponent {
        0 => {
            if mantissa == 0 {
                sign_factor * 0.0
            } else {
                sign_factor * (mantissa as f32) * 2f32.powi(-24)
            }
        }
        0x1F => {
            if mantissa == 0 {
                sign_factor * f32::INFINITY
            } else {
                f32::NAN
            }
        }
        _ => sign_factor * (1.0 + mantissa as f32 / 1024.0) * 2f32.powi(exponent as i32 - 15),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn triangle_mesh() -> MeshData {
        let vertices = vec![
            Vec3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            Vec3 {
                x: 1.0,
                y: 0.0,
                z: 0.0,
            },
            Vec3 {
                x: 0.0,
                y: 1.0,
                z: 0.0,
            },
        ];
        let normals = vec![
            Vec3 {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
            Vec3 {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
            Vec3 {
                x: 0.0,
                y: 0.0,
                z: 1.0,
            },
        ];
        let tangents = vec![
            Vec4::new(1.0, 0.0, 0.0, 1.0),
            Vec4::new(1.0, 0.0, 0.0, 1.0),
            Vec4::new(1.0, 0.0, 0.0, 1.0),
        ];
        let colors = vec![
            ColorFloat::new(1.0, 0.0, 0.0, 1.0),
            ColorFloat::new(0.0, 1.0, 0.0, 1.0),
            ColorFloat::new(0.0, 0.0, 1.0, 1.0),
        ];
        let uv = vec![
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(0.0, 1.0),
        ];
        MeshData {
            vertices,
            normals: Some(normals),
            tangents: Some(tangents),
            colors: Some(colors),
            uv0: Some(uv.clone()),
            uv1: Some(uv.clone()),
            uv2: Some(uv.clone()),
            uv3: Some(uv.clone()),
            uv4: Some(uv.clone()),
            uv5: Some(uv.clone()),
            uv6: Some(uv.clone()),
            uv7: Some(uv),
            skin: None,
        }
    }

    fn hex(bytes: &[u8]) -> String {
        let mut text = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            text.push_str(&format!("{byte:02X}"));
        }
        text
    }

    fn uv_supported(version: UnityVersion, index: usize) -> bool {
        match index {
            2 => supports_uv2(version),
            3 => supports_uv3(version),
            4 => supports_uv4(version),
            5 => supports_uv5(version),
            6 => supports_uv6(version),
            _ => supports_uv7(version),
        }
    }

    fn uv_value(mesh: &MeshData, index: usize) -> &Option<Vec<Vec2>> {
        match index {
            2 => &mesh.uv2,
            3 => &mesh.uv3,
            4 => &mesh.uv4,
            5 => &mesh.uv5,
            6 => &mesh.uv6,
            _ => &mesh.uv7,
        }
    }

    fn decoded_uv(mesh: &MeshData, index: usize) -> &Option<Vec<Vec2>> {
        match index {
            2 => &mesh.uv2,
            3 => &mesh.uv3,
            4 => &mesh.uv4,
            5 => &mesh.uv5,
            6 => &mesh.uv6,
            _ => &mesh.uv7,
        }
    }

    /// Byte-for-byte differential against the reference `VertexDataBlob.Create(MeshData, ...)`
    /// (probe `Temp/opencode/vertexprobe`), plus a `ReadData` round trip on the same blobs.
    #[test]
    fn create_and_read_match_the_reference_runtime() {
        let fixture = include_str!("vertex_data_blob_fixture.txt");
        let mesh = triangle_mesh();
        let mut checked = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts.len(), 5, "malformed fixture line");
            let version = UnityVersion::parse(parts[0]).expect("version");
            let endian = match parts[1] {
                "LittleEndian" => EndianType::Little,
                "BigEndian" => EndianType::Big,
                other => panic!("unknown endian {other}"),
            };

            let blob = VertexDataBlob::create_from_mesh_data(&mesh, version, endian).unwrap();
            let channels: Vec<String> = blob
                .channels
                .iter()
                .map(|channel| {
                    format!(
                        "{}:{}:{}:{}",
                        channel.dimension, channel.offset, channel.format, channel.stream
                    )
                })
                .collect();
            assert_eq!(channels.join(","), parts[2], "channels for {line}");
            let streams: Vec<String> = blob
                .streams
                .iter()
                .map(|stream| {
                    format!(
                        "{}:{}:{}",
                        stream.channel_mask, stream.offset, stream.stride
                    )
                })
                .collect();
            assert_eq!(streams.join(","), parts[3], "streams for {line}");
            assert_eq!(hex(&blob.data), parts[4], "data for {line}");

            let decoded = blob.read_data().unwrap().expect("vertices present");
            assert_eq!(decoded.vertices, mesh.vertices, "vertices for {line}");
            assert_eq!(decoded.normals, mesh.normals, "normals for {line}");
            assert_eq!(decoded.tangents, mesh.tangents, "tangents for {line}");
            assert_eq!(decoded.colors, mesh.colors, "colors for {line}");
            assert_eq!(decoded.uv0, mesh.uv0, "uv0 for {line}");
            assert_eq!(decoded.uv1, mesh.uv1, "uv1 for {line}");
            for index in 2..8 {
                if uv_supported(version, index) {
                    assert_eq!(
                        decoded_uv(&decoded, index),
                        uv_value(&mesh, index),
                        "uv{index} for {line}"
                    );
                } else {
                    assert!(
                        decoded_uv(&decoded, index).is_none(),
                        "uv{index} for {line}"
                    );
                }
            }
            if supports_skin(version) {
                assert_eq!(decoded.skin, mesh.skin, "skin for {line}");
            } else {
                assert!(decoded.skin.is_none(), "skin for {line}");
            }
            checked += 1;
        }
        assert_eq!(checked, 12);
    }

    /// `ConvertStreamsToChannels` transcription for a stream with all six pre-2018 channels.
    #[test]
    fn convert_streams_to_channels_uses_interleaved_offsets() {
        let version = UnityVersion::parse("5.0.0f1").unwrap();
        let streams = vec![StreamInfo {
            channel_mask: 0b11_1111,
            offset: 0,
            stride: 60,
            stride_is_u32: false,
            divider_op: 0,
            frequency: 0,
        }];
        let channels = convert_streams_to_channels(&streams, version).unwrap();
        let rendered: Vec<(u8, u8, u8, u8)> = channels
            .iter()
            .map(|channel| {
                (
                    channel.dimension,
                    channel.offset,
                    channel.format,
                    channel.stream,
                )
            })
            .collect();
        assert_eq!(
            rendered,
            vec![
                (3, 0, 0, 0),
                (3, 12, 0, 0),
                (4, 24, 2, 0),
                (2, 28, 0, 0),
                (2, 36, 0, 0),
                (4, 44, 0, 0),
            ]
        );
    }

    #[test]
    fn int_and_norm_conversions_match_the_shared_read_quirks() {
        // `BytesToIntArray` zero-extends 8-bit values and sign-extends 16-bit values
        // regardless of the signedness flag: the signed/unsigned pairs share one read.
        assert_eq!(
            bytes_to_int_array(&[0xFF], VertexFormat::SInt8).unwrap(),
            vec![255]
        );
        assert_eq!(
            bytes_to_int_array(&[0xFF], VertexFormat::UInt8).unwrap(),
            vec![255]
        );
        assert_eq!(
            bytes_to_int_array(&[0x00, 0x80], VertexFormat::UInt16).unwrap(),
            vec![-32768]
        );
        assert_eq!(
            bytes_to_int_array(&[0x00, 0x80], VertexFormat::SInt16).unwrap(),
            vec![-32768]
        );
        assert_eq!(
            bytes_to_int_array(&[0xFF, 0xFF, 0xFF, 0xFF], VertexFormat::UInt32).unwrap(),
            vec![-1]
        );
        // Signed-normalized extremes clamp to exactly `-1`, matching the `Max(..., -1f)`.
        assert_eq!(
            bytes_to_float_array(&[0x80], VertexFormat::SNorm8).unwrap(),
            vec![-1.0]
        );
        assert_eq!(
            bytes_to_float_array(&[0x81], VertexFormat::SNorm8).unwrap(),
            vec![-1.0]
        );
        assert_eq!(
            bytes_to_float_array(&[0x7F], VertexFormat::SNorm8).unwrap(),
            vec![1.0]
        );
        assert_eq!(
            bytes_to_float_array(&[0xFF], VertexFormat::UNorm8).unwrap(),
            vec![1.0]
        );
        assert_eq!(
            bytes_to_float_array(&[0x00, 0x80], VertexFormat::SNorm16).unwrap(),
            vec![-1.0]
        );
        assert_eq!(
            bytes_to_float_array(&[0xFF, 0xFF], VertexFormat::UNorm16).unwrap(),
            vec![1.0]
        );
    }
}
