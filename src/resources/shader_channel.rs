//! Shader channel and vertex format enums and extensions.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions.Enums.Shader.ShaderChannel` and
//! `.VertexFormat`, plus `ChannelInfoExtensions.GetVertexFormat`/`GetStride` and the `Dimension`
//! nibble rules (`IsSet`/`GetDataDimension`/`SetDataDimension`). The shader channel
//! set and its numeric mapping changed at Unity 5 and again at 2018.1, and the vertex format
//! encoding changed at 2017.1 and 2019.1; both are reproduced here for the frozen baseline.

use super::vertex_data_blob::UnityVersion;

/// `ShaderChannel`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShaderChannel {
    /// Position.
    Vertex,
    /// Normal.
    Normal,
    /// Tangent.
    Tangent,
    /// Vertex color.
    Color,
    /// UV0.
    Uv0,
    /// UV1.
    Uv1,
    /// UV2.
    Uv2,
    /// UV3.
    Uv3,
    /// UV4.
    Uv4,
    /// UV5.
    Uv5,
    /// UV6.
    Uv6,
    /// UV7.
    Uv7,
    /// Blend weight.
    SkinWeight,
    /// Blend indices.
    SkinBoneIndex,
}

impl ShaderChannel {
    /// The enum ordinal.
    pub const fn index(self) -> u8 {
        self as u8
    }

    /// Build from the enum ordinal.
    pub const fn from_index(value: u8) -> Option<Self> {
        Some(match value {
            0 => Self::Vertex,
            1 => Self::Normal,
            2 => Self::Tangent,
            3 => Self::Color,
            4 => Self::Uv0,
            5 => Self::Uv1,
            6 => Self::Uv2,
            7 => Self::Uv3,
            8 => Self::Uv4,
            9 => Self::Uv5,
            10 => Self::Uv6,
            11 => Self::Uv7,
            12 => Self::SkinWeight,
            13 => Self::SkinBoneIndex,
            _ => return None,
        })
    }
}

/// The five-value `Enums.Shader.VertexFormat.VertexFormat`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ShaderVertexFormat {
    /// Float.
    Float = 0,
    /// Float16.
    Float16 = 1,
    /// Color.
    Color = 2,
    /// Byte.
    Byte = 3,
    /// Int.
    Int = 4,
}

/// A shader-channel or vertex-format lookup failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ShaderChannelError {
    /// The channel cannot be converted for the version.
    UnsupportedChannel {
        /// The channel ordinal.
        channel: u8,
    },
    /// The format byte has no mapping for the version.
    UnsupportedFormat {
        /// The raw format byte.
        format: u8,
        /// The Unity version.
        version: UnityVersion,
    },
}

impl core::fmt::Display for ShaderChannelError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnsupportedChannel { channel } => {
                write!(formatter, "unsupported shader channel {channel}")
            }
            Self::UnsupportedFormat { format, version } => write!(
                formatter,
                "unsupported vertex format {format} for Unity {}.{}.{}",
                version.major, version.minor, version.patch
            ),
        }
    }
}

impl std::error::Error for ShaderChannelError {}

/// `ShaderChannelExtensions.ShaderChannel2018Relevant`.
pub fn shader_channel_2018_relevant(version: UnityVersion) -> bool {
    version.at_least(2018)
}

/// `ShaderChannelExtensions.ShaderChannel5Relevant`.
pub fn shader_channel_5_relevant(version: UnityVersion) -> bool {
    version.at_least(5)
}

/// `ShaderChannelExtensions.GetChannelCount`.
pub fn get_channel_count(version: UnityVersion) -> usize {
    if shader_channel_2018_relevant(version) {
        14
    } else if shader_channel_5_relevant(version) {
        8
    } else {
        6
    }
}

/// `ShaderChannelExtensions.GetVertexFormat`.
pub fn get_vertex_format(
    channel: ShaderChannel,
    version: UnityVersion,
) -> Result<ShaderVertexFormat, ShaderChannelError> {
    match channel {
        ShaderChannel::Vertex | ShaderChannel::Normal | ShaderChannel::Tangent => {
            Ok(ShaderVertexFormat::Float)
        }
        ShaderChannel::Color => {
            if vertex_format_2019_relevant(version) {
                Ok(ShaderVertexFormat::Byte)
            } else {
                Ok(ShaderVertexFormat::Color)
            }
        }
        ShaderChannel::Uv0
        | ShaderChannel::Uv1
        | ShaderChannel::Uv2
        | ShaderChannel::Uv3
        | ShaderChannel::Uv4
        | ShaderChannel::Uv5
        | ShaderChannel::Uv6
        | ShaderChannel::Uv7 => Ok(ShaderVertexFormat::Float),
        ShaderChannel::SkinWeight => Ok(ShaderVertexFormat::Float),
        ShaderChannel::SkinBoneIndex => Ok(ShaderVertexFormat::Int),
    }
}

/// `ShaderChannelExtensions.GetDimention`.
pub fn get_dimention(
    channel: ShaderChannel,
    version: UnityVersion,
) -> Result<u8, ShaderChannelError> {
    match channel {
        ShaderChannel::Vertex | ShaderChannel::Normal => Ok(3),
        ShaderChannel::Tangent => Ok(4),
        ShaderChannel::Color => Ok(if shader_channel_5_relevant(version) {
            4
        } else {
            1
        }),
        ShaderChannel::Uv0
        | ShaderChannel::Uv1
        | ShaderChannel::Uv2
        | ShaderChannel::Uv3
        | ShaderChannel::Uv4
        | ShaderChannel::Uv5
        | ShaderChannel::Uv6
        | ShaderChannel::Uv7 => Ok(2),
        ShaderChannel::SkinWeight | ShaderChannel::SkinBoneIndex => {
            Err(ShaderChannelError::UnsupportedChannel {
                channel: channel.index(),
            })
        }
    }
}

/// `ShaderChannelExtensions.GetStride`.
pub fn get_stride(channel: ShaderChannel, version: UnityVersion) -> Result<u8, ShaderChannelError> {
    let format = get_vertex_format(channel, version)?;
    let dimention = get_dimention(channel, version)?;
    Ok(calculate_stride(format, version, dimention))
}

/// `ShaderChannelExtensions.HasChannel`.
pub fn has_channel(channel: ShaderChannel, version: UnityVersion) -> bool {
    if shader_channel_2018_relevant(version) {
        true
    } else if shader_channel_5_relevant(version) {
        (channel.index()) <= ShaderChannel::Uv3.index()
    } else {
        (channel.index()) <= ShaderChannel::Uv1.index()
    }
}

/// `ShaderChannelExtensions.ToChannel`.
pub fn to_channel(channel: ShaderChannel, version: UnityVersion) -> Result<u8, ShaderChannelError> {
    if shader_channel_2018_relevant(version) {
        to_shader_channel2018(channel)
    } else if shader_channel_5_relevant(version) {
        to_shader_channel5(channel)
    } else {
        to_shader_channel4(channel)
    }
}

/// `ShaderChannelExtensions.ToShaderChannel4`.
pub fn to_shader_channel4(channel: ShaderChannel) -> Result<u8, ShaderChannelError> {
    match channel {
        ShaderChannel::Vertex => Ok(0),
        ShaderChannel::Normal => Ok(1),
        ShaderChannel::Color => Ok(2),
        ShaderChannel::Uv0 => Ok(3),
        ShaderChannel::Uv1 => Ok(4),
        ShaderChannel::Tangent => Ok(5),
        _ => Err(ShaderChannelError::UnsupportedChannel {
            channel: channel.index(),
        }),
    }
}

/// `ShaderChannelExtensions.ToShaderChannel5`.
pub fn to_shader_channel5(channel: ShaderChannel) -> Result<u8, ShaderChannelError> {
    match channel {
        ShaderChannel::Vertex => Ok(0),
        ShaderChannel::Normal => Ok(1),
        ShaderChannel::Color => Ok(2),
        ShaderChannel::Uv0 => Ok(3),
        ShaderChannel::Uv1 => Ok(4),
        ShaderChannel::Uv2 => Ok(5),
        ShaderChannel::Uv3 => Ok(6),
        ShaderChannel::Tangent => Ok(7),
        _ => Err(ShaderChannelError::UnsupportedChannel {
            channel: channel.index(),
        }),
    }
}

/// `ShaderChannelExtensions.ToShaderChannel2018`.
pub fn to_shader_channel2018(channel: ShaderChannel) -> Result<u8, ShaderChannelError> {
    Ok(channel.index())
}

/// `ShaderChannelV4Extensions.ToShaderChannel`.
pub fn shader_channel4_to_shader_channel(value: u8) -> Option<ShaderChannel> {
    Some(match value {
        0 => ShaderChannel::Vertex,
        1 => ShaderChannel::Normal,
        2 => ShaderChannel::Color,
        3 => ShaderChannel::Uv0,
        4 => ShaderChannel::Uv1,
        5 => ShaderChannel::Tangent,
        _ => return None,
    })
}

/// `ShaderChannelExtensions.ToSemantic`.
pub fn to_semantic(channel: ShaderChannel) -> &'static str {
    match channel {
        ShaderChannel::Vertex => "POSITION",
        ShaderChannel::Normal => "NORMAL",
        ShaderChannel::Tangent => "TANGENT",
        ShaderChannel::Color => "COLOR",
        ShaderChannel::Uv0 => "TEXCOORD0",
        ShaderChannel::Uv1 => "TEXCOORD1",
        ShaderChannel::Uv2 => "TEXCOORD2",
        ShaderChannel::Uv3 => "TEXCOORD3",
        ShaderChannel::Uv4 => "TEXCOORD4",
        ShaderChannel::Uv5 => "TEXCOORD5",
        ShaderChannel::Uv6 => "TEXCOORD6",
        ShaderChannel::Uv7 => "TEXCOORD7",
        ShaderChannel::SkinWeight => "BLENDWEIGHT",
        ShaderChannel::SkinBoneIndex => "BLENDINDICES",
    }
}

/// `VertexFormatExtensions.VertexFormat2019Relevant`.
pub fn vertex_format_2019_relevant(version: UnityVersion) -> bool {
    version.at_least(2019)
}

/// `VertexFormatExtensions.VertexFormat2017Relevant`.
pub fn vertex_format_2017_relevant(version: UnityVersion) -> bool {
    version.at_least(2017)
}

/// `VertexFormatExtensions.CalculateStride`.
pub fn calculate_stride(format: ShaderVertexFormat, version: UnityVersion, dimention: u8) -> u8 {
    get_size(format, version) * dimention
}

/// `VertexFormatExtensions.GetSize`.
pub fn get_size(format: ShaderVertexFormat, version: UnityVersion) -> u8 {
    match format {
        ShaderVertexFormat::Float => 4,
        ShaderVertexFormat::Float16 => 2,
        ShaderVertexFormat::Color => {
            if shader_channel_5_relevant(version) {
                1
            } else {
                4
            }
        }
        ShaderVertexFormat::Byte => 1,
        ShaderVertexFormat::Int => 4,
    }
}

/// `VertexFormatExtensions.ToFormat`.
pub fn to_format(
    format: ShaderVertexFormat,
    version: UnityVersion,
) -> Result<u8, ShaderChannelError> {
    if vertex_format_2019_relevant(version) {
        to_vertex_format2019(format)
    } else if vertex_format_2017_relevant(version) {
        to_vertex_format2017(format)
    } else {
        to_vertex_channel_format(format)
    }
}

/// `VertexFormatExtensions.ToVertexChannelFormat`.
pub fn to_vertex_channel_format(format: ShaderVertexFormat) -> Result<u8, ShaderChannelError> {
    match format {
        ShaderVertexFormat::Float => Ok(0),
        ShaderVertexFormat::Float16 => Ok(1),
        ShaderVertexFormat::Color => Ok(2),
        ShaderVertexFormat::Byte | ShaderVertexFormat::Int => {
            Err(ShaderChannelError::UnsupportedChannel {
                channel: format as u8,
            })
        }
    }
}

/// `VertexFormatExtensions.ToVertexFormat2017`.
pub fn to_vertex_format2017(format: ShaderVertexFormat) -> Result<u8, ShaderChannelError> {
    match format {
        ShaderVertexFormat::Float => Ok(0),
        ShaderVertexFormat::Float16 => Ok(1),
        ShaderVertexFormat::Color => Ok(2),
        ShaderVertexFormat::Byte => Ok(7),
        ShaderVertexFormat::Int => Ok(11),
    }
}

/// `VertexFormatExtensions.ToVertexFormat2019`.
pub fn to_vertex_format2019(format: ShaderVertexFormat) -> Result<u8, ShaderChannelError> {
    match format {
        ShaderVertexFormat::Float => Ok(0),
        ShaderVertexFormat::Float16 => Ok(1),
        ShaderVertexFormat::Color | ShaderVertexFormat::Byte => Ok(2),
        ShaderVertexFormat::Int => Ok(10),
    }
}

/// `ChannelInfoExtensions.GetVertexFormat`.
pub fn channel_info_get_vertex_format(
    format: u8,
    version: UnityVersion,
) -> Result<ShaderVertexFormat, ShaderChannelError> {
    if vertex_format_2019_relevant(version) {
        vertex_format2019_to_shader_format(format, version)
    } else if vertex_format_2017_relevant(version) {
        vertex_format2017_to_shader_format(format, version)
    } else {
        vertex_channel_format_to_shader_format(format, version)
    }
}

fn vertex_channel_format_to_shader_format(
    format: u8,
    version: UnityVersion,
) -> Result<ShaderVertexFormat, ShaderChannelError> {
    match format {
        0 => Ok(ShaderVertexFormat::Float),
        1 => Ok(ShaderVertexFormat::Float16),
        2 => Ok(ShaderVertexFormat::Color),
        _ => Err(ShaderChannelError::UnsupportedFormat { format, version }),
    }
}

fn vertex_format2017_to_shader_format(
    format: u8,
    version: UnityVersion,
) -> Result<ShaderVertexFormat, ShaderChannelError> {
    match format {
        0 => Ok(ShaderVertexFormat::Float),
        1 => Ok(ShaderVertexFormat::Float16),
        2 => Ok(ShaderVertexFormat::Color),
        3 => Ok(ShaderVertexFormat::Byte),
        11 => Ok(ShaderVertexFormat::Int),
        _ => Err(ShaderChannelError::UnsupportedFormat { format, version }),
    }
}

fn vertex_format2019_to_shader_format(
    format: u8,
    version: UnityVersion,
) -> Result<ShaderVertexFormat, ShaderChannelError> {
    match format {
        0 => Ok(ShaderVertexFormat::Float),
        1 => Ok(ShaderVertexFormat::Float16),
        2 => Ok(ShaderVertexFormat::Byte),
        10 => Ok(ShaderVertexFormat::Int),
        _ => Err(ShaderChannelError::UnsupportedFormat { format, version }),
    }
}

/// `ChannelInfoExtensions.GetStride`.
pub fn channel_info_get_stride(
    format: u8,
    data_dimension: u8,
    version: UnityVersion,
) -> Result<u8, ShaderChannelError> {
    let vertex_format = channel_info_get_vertex_format(format, version)?;
    Ok(calculate_stride(vertex_format, version, data_dimension))
}

/// `ChannelInfoExtensions.IsSet`: the channel carries data when `Dimension` is nonzero.
pub const fn channel_info_is_set(dimension: u8) -> bool {
    dimension > 0
}

/// `ChannelInfoExtensions.GetDataDimension`: the low nibble of `Dimension`.
pub const fn channel_info_data_dimension(dimension: u8) -> u8 {
    dimension & 0b0000_1111
}

/// `ChannelInfoExtensions.SetDataDimension`: replace the low nibble of `Dimension`
/// and keep the high nibble, masking the value exactly like the source.
pub const fn channel_info_set_data_dimension(dimension: u8, value: u8) -> u8 {
    (dimension & 0b1111_0000) | (value & 0b0000_1111)
}

#[cfg(test)]
mod tests {
    use super::*;

    const THROW: i32 = i32::MIN;

    fn int_of(value: Result<i32, ShaderChannelError>) -> i32 {
        value.unwrap_or(THROW)
    }

    fn cell_matches(cell: &str, value: Result<u8, ShaderChannelError>) -> bool {
        match value {
            Ok(actual) => cell.parse::<u8>() == Ok(actual),
            Err(_) => cell == "X",
        }
    }

    /// Byte-for-byte differential against the reference shader-channel and vertex-format
    /// extensions (probe `Temp/opencode/shaderprobe`).
    #[test]
    fn shader_channel_layer_matches_the_reference_runtime() {
        let fixture = include_str!("shader_channel_fixture.txt");
        let mut channels = 0;
        let mut formats = 0;
        let mut infos = 0;
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
                    assert_eq!(
                        get_channel_count(version),
                        parts[3].parse::<usize>().unwrap(),
                        "{line}"
                    );
                    assert_eq!(
                        int_of(get_vertex_format(channel, version).map(|f| f as i32)),
                        parts[4].parse::<i32>().unwrap(),
                        "{line}"
                    );
                    assert_eq!(
                        int_of(get_dimention(channel, version).map(i32::from)),
                        parts[5].parse::<i32>().unwrap(),
                        "{line}"
                    );
                    assert_eq!(
                        int_of(get_stride(channel, version).map(i32::from)),
                        parts[6].parse::<i32>().unwrap(),
                        "{line}"
                    );
                    assert_eq!(has_channel(channel, version), parts[7] == "True", "{line}");
                    assert_eq!(
                        int_of(to_channel(channel, version).map(i32::from)),
                        parts[8].parse::<i32>().unwrap(),
                        "{line}"
                    );
                    assert!(
                        cell_matches(parts[9], to_shader_channel4(channel)),
                        "{line}"
                    );
                    assert!(
                        cell_matches(parts[10], to_shader_channel5(channel)),
                        "{line}"
                    );
                    assert!(
                        cell_matches(parts[11], to_shader_channel2018(channel)),
                        "{line}"
                    );
                    assert_eq!(to_semantic(channel), parts[12], "{line}");
                    channels += 1;
                }
                "FMT" => {
                    let version = UnityVersion::parse(parts[1]).unwrap();
                    let format = shader_format_from_index(parts[2].parse().unwrap());
                    assert_eq!(
                        i32::from(get_size(format, version)),
                        parts[3].parse::<i32>().unwrap(),
                        "{line}"
                    );
                    assert_eq!(
                        int_of(to_format(format, version).map(i32::from)),
                        parts[4].parse::<i32>().unwrap(),
                        "{line}"
                    );
                    assert!(
                        cell_matches(parts[5], to_vertex_format2017(format)),
                        "{line}"
                    );
                    assert!(
                        cell_matches(parts[6], to_vertex_format2019(format)),
                        "{line}"
                    );
                    assert!(
                        cell_matches(parts[7], to_vertex_channel_format(format)),
                        "{line}"
                    );
                    let strides: Vec<i32> = parts[8]
                        .split(',')
                        .map(|value| value.parse().unwrap())
                        .collect();
                    for (index, expected) in strides.iter().enumerate() {
                        let dimension = index as u8 + 1;
                        assert_eq!(
                            i32::from(calculate_stride(format, version, dimension)),
                            *expected,
                            "{line}"
                        );
                    }
                    formats += 1;
                }
                "INFO" => {
                    let version = UnityVersion::parse(parts[1]).unwrap();
                    let format: u8 = parts[2].parse().unwrap();
                    let dimension: u8 = parts[3].parse().unwrap();
                    assert_eq!(
                        channel_info_data_dimension(dimension),
                        parts[4].parse::<u8>().unwrap(),
                        "{line}"
                    );
                    assert_eq!(
                        int_of(channel_info_get_vertex_format(format, version).map(|f| f as i32)),
                        parts[5].parse::<i32>().unwrap(),
                        "{line}"
                    );
                    assert_eq!(
                        int_of(
                            channel_info_get_stride(format, dimension & 0b1111, version)
                                .map(i32::from)
                        ),
                        parts[6].parse::<i32>().unwrap(),
                        "{line}"
                    );
                    infos += 1;
                }
                other => panic!("unknown fixture row {other}"),
            }
        }
        assert_eq!(channels, 84);
        assert_eq!(formats, 30);
        assert_eq!(infos, 312);
    }

    fn shader_format_from_index(value: u8) -> ShaderVertexFormat {
        match value {
            0 => ShaderVertexFormat::Float,
            1 => ShaderVertexFormat::Float16,
            2 => ShaderVertexFormat::Color,
            3 => ShaderVertexFormat::Byte,
            _ => ShaderVertexFormat::Int,
        }
    }

    #[test]
    fn channel_info_dimension_nibbles_follow_the_source_masks() {
        // `IsSet` keys on the whole byte, not the data nibble.
        assert!(!channel_info_is_set(0x00));
        assert!(channel_info_is_set(0x03));
        assert!(channel_info_is_set(0xF0));
        // The data dimension is the low nibble.
        assert_eq!(channel_info_data_dimension(0x00), 0);
        assert_eq!(channel_info_data_dimension(0x03), 3);
        assert_eq!(channel_info_data_dimension(0xF3), 3);
        assert_eq!(channel_info_data_dimension(0xFF), 15);
        // Setting replaces the low nibble and keeps the high one.
        assert_eq!(channel_info_set_data_dimension(0x00, 3), 0x03);
        assert_eq!(channel_info_set_data_dimension(0xF0, 3), 0xF3);
        assert_eq!(channel_info_set_data_dimension(0xAB, 0), 0xA0);
        assert_eq!(channel_info_set_data_dimension(0xAB, 0x1C), 0xAC);
        assert_eq!(channel_info_set_data_dimension(0xFF, 0xFF), 0xFF);
    }
}
