//! `VertexDataExtensions` channel resolution, stream metrics, and vertex/skin reads.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/VertexDataExtensions.cs`. A vertex-data view
//! exposes the two serialized layouts: an explicit channel list (2017 and later, and some 4.x) or
//! four fixed streams (very old versions). `GetChannel` resolves a shader channel to a concrete
//! offset either by indexing the channel list or by walking the matching stream.

use crate::numerics::{BoneWeight4, Vec3};

use super::shader_channel::{
    channel_info_get_stride, get_dimention, get_stride, get_vertex_format, to_channel, to_format,
    to_shader_channel4, ShaderChannel, ShaderChannelError,
};
use super::vertex_data_blob::{ChannelInfo, StreamInfo, UnityVersion};

const VERTEX_STREAM_ALIGN: i32 = 16;

/// A borrow of the serialized vertex-data fields needed by the extension methods.
#[derive(Clone, Copy, Debug)]
pub struct VertexDataView<'a> {
    /// `Has_Channels()`: the explicit channel list.
    pub channels: Option<&'a [ChannelInfo]>,
    /// `Has_Streams()`: the stream list for versions that serialize it.
    pub streams: Option<&'a [StreamInfo]>,
    /// `Has_Streams_0_()`: the four fixed streams for very old versions.
    pub streams_0: Option<[StreamInfo; 4]>,
    /// Raw vertex data bytes.
    pub data: &'a [u8],
    /// Vertex count.
    pub vertex_count: u32,
    /// `CurrentChannels_Int32` when the version serializes it as a signed value.
    pub current_channels_int32: Option<i32>,
    /// `CurrentChannels_UInt32` for versions without the signed field.
    pub current_channels_uint32: u32,
}

/// A failure while resolving or reading vertex data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VertexDataExtensionError {
    /// The view had neither channels nor streams.
    MissingLayout,
    /// A stream index fell outside the stream list.
    MissingStream {
        /// The stream index.
        stream: usize,
    },
    /// A channel index fell outside the channel list.
    ChannelOutOfRange {
        /// The requested index.
        index: usize,
        /// The available count.
        count: usize,
    },
    /// `GenerateVertices` found no vertex channel and the version does not allow it.
    UnsetVertexChannel,
    /// `GenerateSkin` requires the explicit channel list.
    NotImplementedSkin,
    /// The data ended before a component could be read.
    Truncated {
        /// The byte offset required.
        offset: usize,
        /// The byte count required.
        length: usize,
    },
    /// A shader channel or format could not be resolved.
    ShaderChannel(ShaderChannelError),
}

impl core::fmt::Display for VertexDataExtensionError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MissingLayout => {
                write!(formatter, "vertex data has neither channels nor streams")
            }
            Self::MissingStream { stream } => {
                write!(formatter, "vertex stream {stream} is missing")
            }
            Self::ChannelOutOfRange { index, count } => write!(
                formatter,
                "vertex channel {index} is outside the {count} available channels"
            ),
            Self::UnsetVertexChannel => write!(formatter, "the vertex channel is unset"),
            Self::NotImplementedSkin => {
                write!(
                    formatter,
                    "a skin channel requires the explicit channel list"
                )
            }
            Self::Truncated { offset, length } => write!(
                formatter,
                "vertex data ended before byte {}",
                offset + length
            ),
            Self::ShaderChannel(source) => write!(formatter, "{source}"),
        }
    }
}

impl std::error::Error for VertexDataExtensionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ShaderChannel(source) => Some(source),
            _ => None,
        }
    }
}

impl From<ShaderChannelError> for VertexDataExtensionError {
    fn from(value: ShaderChannelError) -> Self {
        Self::ShaderChannel(value)
    }
}

/// `VertexDataExtensions.GetStreams`: the explicit streams, or the four fixed streams, or `None`.
pub fn get_streams(view: &VertexDataView<'_>) -> Option<Vec<StreamInfo>> {
    view.streams
        .map(<[StreamInfo]>::to_vec)
        .or_else(|| view.streams_0.map(|streams| streams.to_vec()))
}

/// `VertexDataExtensions.IsSet`: vertices exist and either inline data or a set streaming
/// info backs them. `streaming_info_is_set` is `None` when the caller has no streaming info;
/// otherwise it answers `StreamingInfoExtensions.IsSet` (a non-empty streaming path).
pub fn is_set(vertex_count: u32, data_len: usize, streaming_info_is_set: Option<bool>) -> bool {
    vertex_count > 0 && (data_len > 0 || streaming_info_is_set == Some(true))
}

fn has_streams_invariant(view: &VertexDataView<'_>) -> bool {
    view.streams.is_some() || view.streams_0.is_some()
}

/// `VertexDataExtensions.GetStreamsInvariant`.
pub fn get_streams_invariant(view: &VertexDataView<'_>) -> Vec<StreamInfo> {
    if let Some(streams) = view.streams {
        streams.to_vec()
    } else if let Some(streams_0) = view.streams_0 {
        streams_0.to_vec()
    } else {
        Vec::new()
    }
}

/// `VertexDataExtensions.GetCurrentChannels`.
pub fn get_current_channels(view: &VertexDataView<'_>) -> u32 {
    match view.current_channels_int32 {
        Some(value) => value as u32,
        None => view.current_channels_uint32,
    }
}

/// The value that `VertexDataExtensions.SetCurrentChannels` would store in the signed or unsigned
/// field, depending on which the version serializes.
pub fn set_current_channels(
    int32_present: bool,
    uint32_slot: &mut u32,
    int32_slot: &mut i32,
    value: u32,
) {
    if int32_present {
        *int32_slot = value as i32;
    } else {
        *uint32_slot = value;
    }
}

/// `VertexDataExtensions.GetChannel`.
pub fn get_channel(
    view: &VertexDataView<'_>,
    version: UnityVersion,
    channel: ShaderChannel,
) -> Result<ChannelInfo, VertexDataExtensionError> {
    if let Some(channels) = view.channels {
        let index = to_channel(channel, version)? as usize;
        return channels
            .get(index)
            .copied()
            .ok_or(VertexDataExtensionError::ChannelOutOfRange {
                index,
                count: channels.len(),
            });
    }

    let streams = get_streams(view).ok_or(VertexDataExtensionError::MissingLayout)?;
    let channel4 = to_shader_channel4(channel)?;
    let mut result = ChannelInfo::default();
    if let Some((stream_index, stream)) = streams
        .iter()
        .enumerate()
        .find(|(_, stream)| bit_set(stream.channel_mask, channel4))
    {
        let mut offset: u8 = 0;
        for i in 0..channel4 {
            if bit_set(stream.channel_mask, i) {
                let stride = super::shader_channel::shader_channel4_to_shader_channel(i)
                    .ok_or(ShaderChannelError::UnsupportedChannel { channel: i })
                    .map(|channel| get_stride(channel, version))??;
                offset = offset.wrapping_add(stride);
            }
        }
        result = ChannelInfo {
            dimension: get_dimention(channel, version)?,
            offset,
            format: to_format(get_vertex_format(channel, version)?, version)?,
            stream: stream_index as u8,
        };
    }
    Ok(result)
}

/// `VertexDataExtensions.GetStreamStride`.
pub fn get_stream_stride(
    view: &VertexDataView<'_>,
    version: UnityVersion,
    stream: usize,
) -> Result<i32, VertexDataExtensionError> {
    if has_streams_invariant(view) {
        let streams = get_streams_invariant(view);
        let entry = streams
            .get(stream)
            .ok_or(VertexDataExtensionError::MissingStream { stream })?;
        Ok(entry.get_stride() as i32)
    } else {
        let channels = view
            .channels
            .ok_or(VertexDataExtensionError::MissingLayout)?;
        let mut total = 0i32;
        for channel in channels {
            if channel.is_set() && channel.stream as usize == stream {
                total += i32::from(channel_info_get_stride(
                    channel.format,
                    channel.get_data_dimension(),
                    version,
                )?);
            }
        }
        Ok(total)
    }
}

/// `VertexDataExtensions.GetStreamSize`.
pub fn get_stream_size(
    view: &VertexDataView<'_>,
    version: UnityVersion,
    stream: usize,
) -> Result<i32, VertexDataExtensionError> {
    Ok(get_stream_stride(view, version, stream)? * view.vertex_count as i32)
}

/// `VertexDataExtensions.GetStreamOffset`.
pub fn get_stream_offset(
    view: &VertexDataView<'_>,
    version: UnityVersion,
    stream: usize,
) -> Result<i32, VertexDataExtensionError> {
    let mut offset = 0i32;
    for i in 0..stream {
        offset += get_stream_size(view, version, i)?;
        offset = (offset + (VERTEX_STREAM_ALIGN - 1)) & !(VERTEX_STREAM_ALIGN - 1);
    }
    Ok(offset)
}

/// Unity 5.6.0 allows an unset vertex channel instead of throwing.
fn allow_unset_vertex_channel(version: UnityVersion) -> bool {
    version.major == 5 && version.minor == 6 && version.patch == 0
}

/// `VertexDataExtensions.GenerateVertices`.
pub fn generate_vertices(
    view: &VertexDataView<'_>,
    version: UnityVersion,
    first_vertex: u32,
    vertex_count: u32,
) -> Result<Vec<Vec3>, VertexDataExtensionError> {
    let channel = get_channel(view, version, ShaderChannel::Vertex)?;
    if !channel.is_set() {
        if allow_unset_vertex_channel(version) {
            return Ok(Vec::new());
        }
        return Err(VertexDataExtensionError::UnsetVertexChannel);
    }

    let stream = channel.stream as usize;
    let stream_stride = get_stream_stride(view, version, stream)? as usize;
    let stream_offset = get_stream_offset(view, version, stream)? as usize;
    let count = vertex_count as usize;
    let mut vertices = Vec::new();
    vertices
        .try_reserve_exact(count)
        .map_err(|_| VertexDataExtensionError::MissingLayout)?;
    for vertex in 0..count {
        let base = stream_offset
            + first_vertex as usize * stream_stride
            + channel.offset as usize
            + vertex * stream_stride;
        vertices.push(Vec3 {
            x: read_f32(view.data, base)?,
            y: read_f32(view.data, base + 4)?,
            z: read_f32(view.data, base + 8)?,
        });
    }
    Ok(vertices)
}

/// `VertexDataExtensions.GenerateSkin`.
pub fn generate_skin(
    view: &VertexDataView<'_>,
    version: UnityVersion,
) -> Result<Vec<BoneWeight4>, VertexDataExtensionError> {
    let channels = view
        .channels
        .ok_or(VertexDataExtensionError::NotImplementedSkin)?;
    let weight_channel =
        channels
            .get(12)
            .copied()
            .ok_or(VertexDataExtensionError::ChannelOutOfRange {
                index: 12,
                count: channels.len(),
            })?;
    let index_channel =
        channels
            .get(13)
            .copied()
            .ok_or(VertexDataExtensionError::ChannelOutOfRange {
                index: 13,
                count: channels.len(),
            })?;
    if !weight_channel.is_set() {
        return Ok(Vec::new());
    }

    let weight_stride = stream_sum(channels, weight_channel.stream, version)?;
    let weight_offset = get_stream_offset(view, version, weight_channel.stream as usize)?;
    let index_stride = stream_sum(channels, index_channel.stream, version)?;
    let index_offset = get_stream_offset(view, version, index_channel.stream as usize)?;

    let weight_count = usize::from(weight_channel.get_data_dimension()).min(4);
    let index_count = usize::from(index_channel.get_data_dimension()).min(4);
    let count = view.vertex_count as usize;
    let mut skin = Vec::new();
    skin.try_reserve_exact(count)
        .map_err(|_| VertexDataExtensionError::MissingLayout)?;
    for vertex in 0..count {
        let mut weights = [0f32; 4];
        let weight_base = weight_offset as usize
            + vertex * weight_stride as usize
            + weight_channel.offset as usize;
        for (i, weight) in weights.iter_mut().take(weight_count).enumerate() {
            *weight = read_f32(view.data, weight_base + i * 4)?;
        }
        let mut indices = [0i32; 4];
        let index_base =
            index_offset as usize + vertex * index_stride as usize + index_channel.offset as usize;
        for (i, index) in indices.iter_mut().take(index_count).enumerate() {
            *index = read_i32(view.data, index_base + i * 4)?;
        }
        skin.push(BoneWeight4 { weights, indices });
    }
    Ok(skin)
}

fn stream_sum(
    channels: &[ChannelInfo],
    stream: u8,
    version: UnityVersion,
) -> Result<i32, VertexDataExtensionError> {
    let mut total = 0i32;
    for channel in channels {
        if channel.stream == stream {
            total += i32::from(channel_info_get_stride(
                channel.format,
                channel.get_data_dimension(),
                version,
            )?);
        }
    }
    Ok(total)
}

fn bit_set(mask: u32, index: u8) -> bool {
    (mask & (1u32 << index)) != 0
}

fn read_f32(data: &[u8], offset: usize) -> Result<f32, VertexDataExtensionError> {
    let bytes = data
        .get(offset..offset + 4)
        .ok_or(VertexDataExtensionError::Truncated { offset, length: 4 })?;
    Ok(f32::from_le_bytes(bytes.try_into().unwrap()))
}

fn read_i32(data: &[u8], offset: usize) -> Result<i32, VertexDataExtensionError> {
    let bytes = data
        .get(offset..offset + 4)
        .ok_or(VertexDataExtensionError::Truncated { offset, length: 4 })?;
    Ok(i32::from_le_bytes(bytes.try_into().unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channels_for(version: &str) -> Vec<ChannelInfo> {
        let index_format: u8 = match version {
            "2019.1.0f1" => 10,
            "5.0.0f1" | "4.0.0f1" | "3.5.0f1" => 0,
            _ => 11,
        };
        let definitions: [(u8, u8, u8, u8); 14] = [
            (3, 0, 0, 0),
            (3, 12, 0, 0),
            (4, 24, 0, 0),
            (4, 0, 2, 1),
            (2, 0, 0, 2),
            (0, 0, 0, 0),
            (0, 0, 0, 0),
            (0, 0, 0, 0),
            (0, 0, 0, 0),
            (0, 0, 0, 0),
            (0, 0, 0, 0),
            (0, 0, 0, 0),
            (4, 0, 0, 3),
            (4, 16, index_format, 3),
        ];
        definitions
            .iter()
            .map(|(dimension, offset, format, stream)| ChannelInfo {
                dimension: *dimension,
                offset: *offset,
                format: *format,
                stream: *stream,
            })
            .collect()
    }

    fn write_f32s(data: &mut [u8], offset: usize, values: &[f32]) {
        for (i, value) in values.iter().enumerate() {
            data[offset + i * 4..offset + i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
    }

    fn write_i32s(data: &mut [u8], offset: usize, values: &[i32]) {
        for (i, value) in values.iter().enumerate() {
            data[offset + i * 4..offset + i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
    }

    fn build_data() -> Vec<u8> {
        let mut data = vec![0u8; 192];
        write_f32s(&mut data, 0, &[0.0, 1.0, 2.0]);
        write_f32s(&mut data, 12, &[0.0, 0.0, 1.0]);
        write_f32s(&mut data, 24, &[0.0, 0.0, 0.0, 1.0]);
        write_f32s(&mut data, 40, &[3.0, 4.0, 5.0]);
        write_f32s(&mut data, 52, &[0.0, 0.0, 1.0]);
        write_f32s(&mut data, 64, &[0.0, 0.0, 0.0, 1.0]);
        data[80..84].copy_from_slice(&[255, 0, 0, 255]);
        data[84..88].copy_from_slice(&[0, 255, 0, 255]);
        write_f32s(&mut data, 96, &[0.0, 1.0]);
        write_f32s(&mut data, 104, &[2.0, 3.0]);
        write_f32s(&mut data, 112, &[0.5, 0.25, 0.125, 0.125]);
        write_i32s(&mut data, 128, &[1, 2, 3, 4]);
        write_f32s(&mut data, 144, &[0.1, 0.2, 0.3, 0.4]);
        write_i32s(&mut data, 160, &[5, 6, 7, 8]);
        data
    }

    type Layout = (
        Option<Vec<ChannelInfo>>,
        Option<Vec<StreamInfo>>,
        Option<[StreamInfo; 4]>,
    );

    fn layout(version: &str) -> Layout {
        match version {
            "3.5.0f1" => (None, None, Some([StreamInfo::default(); 4])),
            "4.0.0f1" => (Some(channels_for(version)), Some(Vec::new()), None),
            _ => (Some(channels_for(version)), None, None),
        }
    }

    fn make_view<'a>(
        channels: Option<&'a [ChannelInfo]>,
        streams: Option<&'a [StreamInfo]>,
        streams_0: Option<[StreamInfo; 4]>,
        data: &'a [u8],
    ) -> VertexDataView<'a> {
        VertexDataView {
            channels,
            streams,
            streams_0,
            data,
            vertex_count: 2,
            current_channels_int32: None,
            current_channels_uint32: 0,
        }
    }

    fn assert_channel(
        line: &str,
        result: Result<ChannelInfo, VertexDataExtensionError>,
        fields: &[&str],
    ) {
        if fields[0] == "ERR" {
            assert!(result.is_err(), "{line}");
            return;
        }
        assert_eq!(fields[0], "OK", "{line}");
        let channel = result.unwrap();
        assert_eq!(
            i32::from(channel.stream),
            fields[1].parse::<i32>().unwrap(),
            "{line}"
        );
        assert_eq!(
            i32::from(channel.offset),
            fields[2].parse::<i32>().unwrap(),
            "{line}"
        );
        assert_eq!(
            i32::from(channel.format),
            fields[3].parse::<i32>().unwrap(),
            "{line}"
        );
        assert_eq!(
            i32::from(channel.dimension),
            fields[4].parse::<i32>().unwrap(),
            "{line}"
        );
    }

    /// Byte-for-byte differential against the reference `VertexDataExtensions` (probe
    /// `Temp/opencode/vdprobe`).
    #[test]
    fn vertex_data_extensions_match_the_reference_runtime() {
        let fixture = include_str!("vertex_data_extensions_fixture.txt");
        let data = build_data();
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            match parts[0] {
                "CH" => {
                    let version = UnityVersion::parse(parts[1]).unwrap();
                    let channel = ShaderChannel::from_index(parts[2].parse().unwrap()).unwrap();
                    let (channels, streams, streams_0) = layout(parts[1]);
                    let view = make_view(channels.as_deref(), streams.as_deref(), streams_0, &data);
                    assert_channel(line, get_channel(&view, version, channel), &parts[3..]);
                }
                "SPATH" => {
                    let version = UnityVersion::parse("3.5.0f1").unwrap();
                    let channel = ShaderChannel::from_index(parts[1].parse().unwrap()).unwrap();
                    let mut streams_0 = [StreamInfo::default(); 4];
                    streams_0[0].channel_mask = 0b11;
                    streams_0[0].stride = 24;
                    streams_0[1].channel_mask = 0b100;
                    streams_0[1].stride = 4;
                    let view = make_view(None, None, Some(streams_0), &data);
                    assert_channel(line, get_channel(&view, version, channel), &parts[2..]);
                }
                "VERT" => {
                    let version = UnityVersion::parse(parts[1]).unwrap();
                    let first: u32 = parts[2].parse().unwrap();
                    let count: u32 = parts[3].parse().unwrap();
                    let (channels, streams, streams_0) = layout(parts[1]);
                    let view = make_view(channels.as_deref(), streams.as_deref(), streams_0, &data);
                    let result = generate_vertices(&view, version, first, count);
                    if parts[4] == "ERR" {
                        assert!(result.is_err(), "{line}");
                    } else {
                        let expected: Vec<u32> = parts[5]
                            .split(',')
                            .map(|value| value.parse().unwrap())
                            .collect();
                        let actual: Vec<u32> = result
                            .unwrap()
                            .iter()
                            .flat_map(|v| [v.x.to_bits(), v.y.to_bits(), v.z.to_bits()])
                            .collect();
                        assert_eq!(actual, expected, "{line}");
                    }
                }
                "SKIN" => {
                    let version = UnityVersion::parse(parts[1]).unwrap();
                    let (channels, streams, streams_0) = layout(parts[1]);
                    let view = make_view(channels.as_deref(), streams.as_deref(), streams_0, &data);
                    let result = generate_skin(&view, version);
                    if parts[2] == "ERR" {
                        assert!(result.is_err(), "{line}");
                    } else {
                        let skin = result.unwrap();
                        let expected_rows: Vec<&str> = parts[3].split(';').collect();
                        assert_eq!(skin.len(), expected_rows.len(), "{line}");
                        for (row, expected) in skin.iter().zip(expected_rows) {
                            let mut halves = expected.split('/');
                            let weights: Vec<u32> = halves
                                .next()
                                .unwrap()
                                .split(',')
                                .map(|value| value.parse().unwrap())
                                .collect();
                            let indices: Vec<i32> = halves
                                .next()
                                .unwrap()
                                .split(',')
                                .map(|value| value.parse().unwrap())
                                .collect();
                            assert_eq!(row.weights.map(f32::to_bits).to_vec(), weights, "{line}");
                            assert_eq!(row.indices.to_vec(), indices, "{line}");
                        }
                    }
                }
                "METRIC" => {
                    let version = UnityVersion::parse(parts[1]).unwrap();
                    let stream: usize = parts[2].parse().unwrap();
                    let (channels, streams, streams_0) = layout(parts[1]);
                    let view = make_view(channels.as_deref(), streams.as_deref(), streams_0, &data);
                    let stride = get_stream_stride(&view, version, stream);
                    let size = get_stream_size(&view, version, stream);
                    let offset = get_stream_offset(&view, version, stream);
                    if parts[3] == "ERR" {
                        assert!(
                            stride.is_err() || size.is_err() || offset.is_err(),
                            "{line}"
                        );
                    } else {
                        assert_eq!(stride.unwrap(), parts[3].parse::<i32>().unwrap(), "{line}");
                        assert_eq!(size.unwrap(), parts[4].parse::<i32>().unwrap(), "{line}");
                        assert_eq!(offset.unwrap(), parts[5].parse::<i32>().unwrap(), "{line}");
                    }
                }
                other => panic!("unknown fixture row {other}"),
            }
            rows += 1;
        }
        assert_eq!(rows, 140);
    }

    #[test]
    fn current_channels_prefers_the_signed_field_when_present() {
        let data: [u8; 0] = [];
        let view = VertexDataView {
            channels: None,
            streams: None,
            streams_0: None,
            data: &data,
            vertex_count: 0,
            current_channels_int32: Some(-1),
            current_channels_uint32: 0,
        };
        assert_eq!(get_current_channels(&view), u32::MAX);

        let mut uint32_slot = 0u32;
        let mut int32_slot = 0i32;
        set_current_channels(true, &mut uint32_slot, &mut int32_slot, 0xABCD);
        assert_eq!(uint32_slot, 0);
        assert_eq!(int32_slot, 0xABCD);
        set_current_channels(false, &mut uint32_slot, &mut int32_slot, 0x1234);
        assert_eq!(uint32_slot, 0x1234);
    }

    /// `VertexDataExtensions.IsSet`: a zero vertex count is never set; otherwise inline
    /// data or a set streaming info is required.
    #[test]
    fn is_set_matches_the_source_branches() {
        assert!(!is_set(0, 0, None));
        assert!(!is_set(0, 16, None));
        assert!(!is_set(0, 16, Some(true)));
        assert!(is_set(2, 16, None));
        assert!(is_set(2, 16, Some(false)));
        assert!(is_set(2, 0, Some(true)));
        assert!(!is_set(2, 0, Some(false)));
        assert!(!is_set(2, 0, None));
    }
}
