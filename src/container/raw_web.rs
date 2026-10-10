//! UnityRaw and UnityWeb bundle parsing.
//!
//! Mirrors `AssetRipper.IO.Files/BundleFiles/RawWeb` (`RawWebBundleHeader.Read` with its
//! version-4/2/3 gates, `RawWebBundleFile.Read` with its header-size check, last-chunk UnityWeb
//! decompression, and the metadata-size check).
//!
//! Deliberate hardening, all explicit errors: declared sizes are validated against limits and the
//! input before slicing, entry ranges are bounds-checked where the source seeks and reads, and
//! directory reads are bounded by the metadata cap instead of scanning unbounded data.

use std::borrow::Cow;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::compression::lzma::{decompress_size_block, LzmaBlockError};
use crate::io::binary::{BinaryError, BinaryReader, Endian};

const RAW_MAGIC: &str = "UnityRaw";
const WEB_MAGIC: &str = "UnityWeb";

/// The legacy bundle layout selected by its signature.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RawWebKind {
    UnityRaw,
    UnityWeb,
}

/// Header fields shared by UnityRaw and UnityWeb bundles.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawWebHeader {
    pub kind: RawWebKind,
    pub version: i32,
    pub unity_version: String,
    pub unity_revision: String,
    pub hash: Option<[u8; 16]>,
    pub crc: Option<u32>,
    pub minimum_streamed_bytes: u32,
    pub header_size: usize,
    pub number_of_scenes_to_download_before_streaming: i32,
    pub scenes: Vec<RawWebScene>,
    pub complete_file_size: Option<u32>,
    pub uncompressed_blocks_info_size: Option<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawWebScene {
    pub compressed_size: u32,
    pub decompressed_size: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawWebEntry {
    pub path: String,
    pub offset: u32,
    pub size: u32,
}

/// Allocation and parsing bounds for legacy bundle headers, metadata, and chunks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawWebLimits {
    pub max_header_string_bytes: usize,
    pub max_header_bytes: usize,
    pub max_scenes: usize,
    pub max_metadata_bytes: usize,
    pub max_nodes: usize,
    pub max_node_path_bytes: usize,
    pub max_entry_bytes: usize,
    pub max_decompressed_chunk_bytes: usize,
}

impl Default for RawWebLimits {
    fn default() -> Self {
        Self {
            max_header_string_bytes: 4096,
            max_header_bytes: 16 * 1024 * 1024,
            max_scenes: 1_000_000,
            max_metadata_bytes: 256 * 1024 * 1024,
            max_nodes: 1_000_000,
            max_node_path_bytes: 1024 * 1024,
            max_entry_bytes: 512 * 1024 * 1024,
            max_decompressed_chunk_bytes: 512 * 1024 * 1024,
        }
    }
}

/// Parsed legacy bundle metadata backed by borrowed raw bytes or an owned Web chunk.
#[derive(Debug)]
pub struct RawWebBundle<'a> {
    pub header: RawWebHeader,
    pub entries: Vec<RawWebEntry>,
    data: Cow<'a, [u8]>,
}

impl<'a> RawWebBundle<'a> {
    pub fn parse(input: &'a [u8]) -> Result<Self, RawWebError> {
        Self::parse_with_limits(input, RawWebLimits::default())
    }

    pub fn parse_with_limits(input: &'a [u8], limits: RawWebLimits) -> Result<Self, RawWebError> {
        let mut header_reader = BinaryReader::new(input, Endian::Big);
        let header = read_header(&mut header_reader, input.len(), limits)?;

        let data = match header.kind {
            RawWebKind::UnityRaw => Cow::Borrowed(&input[header.header_size..]),
            RawWebKind::UnityWeb => {
                let scene = header.scenes.last().ok_or_else(|| {
                    RawWebError::new(header.header_size, RawWebErrorKind::WebBundleHasNoScenes)
                })?;
                let compressed_size = scene.compressed_size as usize;
                let compressed_end =
                    header
                        .header_size
                        .checked_add(compressed_size)
                        .ok_or_else(|| {
                            RawWebError::new(
                                header.header_size,
                                RawWebErrorKind::ChunkRangeOverflow {
                                    offset: header.header_size,
                                    size: compressed_size,
                                },
                            )
                        })?;
                let compressed =
                    input
                        .get(header.header_size..compressed_end)
                        .ok_or_else(|| {
                            RawWebError::new(
                                header.header_size,
                                RawWebErrorKind::ChunkOutOfBounds {
                                    offset: header.header_size,
                                    size: compressed_size,
                                    input_size: input.len(),
                                },
                            )
                        })?;
                Cow::Owned(Self::decode_unityweb_scene(&header, compressed, limits)?)
            }
        };

        Self::parse_header_and_data(header, data, limits)
    }

    pub(crate) fn parse_header_prefix(
        prefix: &[u8],
        input_size: usize,
        limits: RawWebLimits,
    ) -> Result<RawWebHeader, RawWebError> {
        let mut reader = BinaryReader::new(prefix, Endian::Big);
        read_header(&mut reader, input_size, limits)
    }

    pub(crate) fn parse_unityraw_directory_prefix(
        header: &RawWebHeader,
        metadata: &[u8],
        data_size: usize,
        limits: RawWebLimits,
    ) -> Result<Vec<RawWebEntry>, RawWebError> {
        if header.kind != RawWebKind::UnityRaw {
            return Err(RawWebError::new(
                0,
                RawWebErrorKind::InvalidSignature {
                    actual: "UnityWeb".to_owned(),
                },
            ));
        }
        if metadata.len() > limits.max_metadata_bytes {
            return Err(RawWebError::new(
                header.header_size,
                RawWebErrorKind::LimitExceeded {
                    field: "metadata bytes",
                    requested: metadata.len() as u64,
                    limit: limits.max_metadata_bytes,
                },
            ));
        }
        Self::parse_directory_metadata(header, metadata, data_size, limits)
            .map(|(entries, _)| entries)
    }

    pub(crate) fn from_owned_data(
        header: RawWebHeader,
        data: Vec<u8>,
        limits: RawWebLimits,
    ) -> Result<RawWebBundle<'static>, RawWebError> {
        RawWebBundle::<'static>::parse_header_and_data(header, Cow::Owned(data), limits)
    }

    pub(crate) fn decode_unityweb_scene(
        header: &RawWebHeader,
        compressed: &[u8],
        limits: RawWebLimits,
    ) -> Result<Vec<u8>, RawWebError> {
        let scene = Self::last_scene(header)?;
        decompress_size_block(
            compressed,
            scene.decompressed_size as usize,
            limits.max_decompressed_chunk_bytes,
        )
        .map_err(|error| {
            RawWebError::new(
                header.header_size + error.input_offset(),
                RawWebErrorKind::Lzma(error),
            )
        })
    }

    pub(crate) fn last_scene(header: &RawWebHeader) -> Result<RawWebScene, RawWebError> {
        header.scenes.last().copied().ok_or_else(|| {
            RawWebError::new(header.header_size, RawWebErrorKind::WebBundleHasNoScenes)
        })
    }

    pub(crate) fn last_scene_compressed_range(
        header: &RawWebHeader,
        input_size: usize,
    ) -> Result<(usize, usize), RawWebError> {
        let scene = Self::last_scene(header)?;
        let offset = header.header_size;
        let size = scene.compressed_size as usize;
        let end = offset.checked_add(size).ok_or_else(|| {
            RawWebError::new(offset, RawWebErrorKind::ChunkRangeOverflow { offset, size })
        })?;
        if end > input_size {
            return Err(RawWebError::new(
                offset,
                RawWebErrorKind::ChunkOutOfBounds {
                    offset,
                    size,
                    input_size,
                },
            ));
        }
        Ok((offset, end))
    }

    fn parse_header_and_data(
        header: RawWebHeader,
        data: Cow<'a, [u8]>,
        limits: RawWebLimits,
    ) -> Result<Self, RawWebError> {
        let metadata_limit = data.len().min(limits.max_metadata_bytes);
        let metadata = &data[..metadata_limit];
        let (entries, _) = Self::parse_directory_metadata(&header, metadata, data.len(), limits)?;

        Ok(Self {
            header,
            entries,
            data,
        })
    }

    fn parse_directory_metadata(
        header: &RawWebHeader,
        metadata: &[u8],
        data_size: usize,
        limits: RawWebLimits,
    ) -> Result<(Vec<RawWebEntry>, usize), RawWebError> {
        let metadata_base = match header.kind {
            RawWebKind::UnityRaw => header.header_size,
            RawWebKind::UnityWeb => 0,
        };
        let mut metadata_reader =
            BinaryReader::with_base_offset(metadata, Endian::Big, metadata_base)
                .map_err(|error| RawWebError::binary(error, "metadata"))?;
        let entries = read_entries(&mut metadata_reader, data_size, limits)?;
        let metadata_size = metadata_reader.position();
        if let Some(declared_size) = header.uncompressed_blocks_info_size {
            if declared_size > 0 && declared_size as usize != metadata_size {
                return Err(RawWebError::new(
                    metadata_base + metadata_size,
                    RawWebErrorKind::MetadataSizeMismatch {
                        declared: declared_size,
                        actual: metadata_size,
                    },
                ));
            }
        }
        Ok((entries, metadata_size))
    }

    pub fn read_entry(&self, entry_index: usize) -> Result<&[u8], RawWebError> {
        let range = self.entry_range(entry_index)?;
        self.data.get(range.clone()).ok_or_else(|| {
            let entry = &self.entries[entry_index];
            RawWebError::new(
                range.start,
                RawWebErrorKind::EntryOutOfBounds {
                    path: entry.path.clone(),
                    offset: entry.offset,
                    size: entry.size,
                    data_size: self.data.len(),
                },
            )
        })
    }

    /// Consume the bundle and return one entry while retaining borrowed raw data when possible.
    /// UnityWeb entries are slices of an owned decompressed chunk, so those ranges are copied.
    pub fn into_entry(self, entry_index: usize) -> Result<Cow<'a, [u8]>, RawWebError> {
        let range = self.entry_range(entry_index)?;
        match self.data {
            Cow::Borrowed(data) => data.get(range.clone()).map(Cow::Borrowed).ok_or_else(|| {
                let entry = &self.entries[entry_index];
                RawWebError::new(
                    range.start,
                    RawWebErrorKind::EntryOutOfBounds {
                        path: entry.path.clone(),
                        offset: entry.offset,
                        size: entry.size,
                        data_size: data.len(),
                    },
                )
            }),
            Cow::Owned(data) => {
                let Some(entry_data) = data.get(range.clone()) else {
                    let entry = &self.entries[entry_index];
                    return Err(RawWebError::new(
                        range.start,
                        RawWebErrorKind::EntryOutOfBounds {
                            path: entry.path.clone(),
                            offset: entry.offset,
                            size: entry.size,
                            data_size: data.len(),
                        },
                    ));
                };
                if range.start == 0 && range.end == data.len() {
                    Ok(Cow::Owned(data))
                } else {
                    let mut output = Vec::new();
                    output.try_reserve_exact(entry_data.len()).map_err(|_| {
                        RawWebError::new(
                            range.start,
                            RawWebErrorKind::AllocationFailed {
                                field: "entry bytes",
                                requested: entry_data.len(),
                            },
                        )
                    })?;
                    output.extend_from_slice(entry_data);
                    Ok(Cow::Owned(output))
                }
            }
        }
    }

    fn entry_range(&self, entry_index: usize) -> Result<std::ops::Range<usize>, RawWebError> {
        let entry = self.entries.get(entry_index).ok_or_else(|| {
            RawWebError::new(
                0,
                RawWebErrorKind::EntryIndexOutOfRange {
                    index: entry_index,
                    count: self.entries.len(),
                },
            )
        })?;
        let start = entry.offset as usize;
        let end = start.checked_add(entry.size as usize).ok_or_else(|| {
            RawWebError::new(
                start,
                RawWebErrorKind::EntryRangeOverflow {
                    path: entry.path.clone(),
                    offset: entry.offset,
                    size: entry.size,
                },
            )
        })?;
        Ok(start..end)
    }

    pub fn data_size(&self) -> usize {
        self.data.len()
    }
}

fn read_header(
    reader: &mut BinaryReader<'_>,
    input_size: usize,
    limits: RawWebLimits,
) -> Result<RawWebHeader, RawWebError> {
    let signature = reader
        .read_null_terminated_utf8(limits.max_header_string_bytes)
        .map_err(|error| RawWebError::binary(error, "bundle signature"))?;
    let kind = match signature {
        RAW_MAGIC => RawWebKind::UnityRaw,
        WEB_MAGIC => RawWebKind::UnityWeb,
        actual => {
            return Err(RawWebError::new(
                0,
                RawWebErrorKind::InvalidSignature {
                    actual: actual.to_owned(),
                },
            ));
        }
    };

    let version = reader
        .read_i32()
        .map_err(|error| RawWebError::binary(error, "bundle version"))?;
    if version < 0 {
        return Err(RawWebError::new(
            reader.offset() - 4,
            RawWebErrorKind::NegativeVersion { version },
        ));
    }
    let unity_version = reader
        .read_null_terminated_utf8(limits.max_header_string_bytes)
        .map_err(|error| RawWebError::binary(error, "Unity version"))?
        .to_owned();
    let unity_revision = reader
        .read_null_terminated_utf8(limits.max_header_string_bytes)
        .map_err(|error| RawWebError::binary(error, "Unity revision"))?
        .to_owned();

    let (hash, crc) = if version >= 4 {
        let hash: [u8; 16] = reader
            .read_exact(16)
            .map_err(|error| RawWebError::binary(error, "bundle hash"))?
            .try_into()
            .map_err(|_| {
                RawWebError::new(
                    reader.offset().saturating_sub(16),
                    RawWebErrorKind::InternalLengthInvariant {
                        field: "bundle hash",
                    },
                )
            })?;
        let crc = reader
            .read_u32()
            .map_err(|error| RawWebError::binary(error, "bundle CRC"))?;
        (Some(hash), Some(crc))
    } else {
        (None, None)
    };

    let minimum_streamed_bytes = reader
        .read_u32()
        .map_err(|error| RawWebError::binary(error, "minimum streamed bytes"))?;
    let header_size_field_offset = reader.offset();
    let declared_header_size = reader
        .read_i32()
        .map_err(|error| RawWebError::binary(error, "header size"))?;
    let header_size = usize::try_from(declared_header_size).map_err(|_| {
        RawWebError::new(
            header_size_field_offset,
            RawWebErrorKind::InvalidHeaderSize {
                value: declared_header_size,
            },
        )
    })?;
    if header_size > limits.max_header_bytes {
        return Err(RawWebError::new(
            header_size_field_offset,
            RawWebErrorKind::LimitExceeded {
                field: "header bytes",
                requested: header_size as u64,
                limit: limits.max_header_bytes,
            },
        ));
    }
    if header_size > input_size {
        return Err(RawWebError::new(
            header_size_field_offset,
            RawWebErrorKind::ChunkOutOfBounds {
                offset: 0,
                size: header_size,
                input_size,
            },
        ));
    }

    let number_of_scenes_to_download_before_streaming = reader
        .read_i32()
        .map_err(|error| RawWebError::binary(error, "streaming scene count"))?;
    let scene_count_offset = reader.offset();
    let scene_count = reader
        .read_i32()
        .map_err(|error| RawWebError::binary(error, "scene table count"))?;
    let scene_count = checked_count(
        scene_count,
        limits.max_scenes,
        scene_count_offset,
        "scene records",
    )?;
    let mut scenes = Vec::new();
    scenes.try_reserve_exact(scene_count).map_err(|_| {
        RawWebError::new(
            scene_count_offset,
            RawWebErrorKind::AllocationFailed {
                field: "scene records",
                requested: scene_count,
            },
        )
    })?;
    for _ in 0..scene_count {
        let compressed_size = reader
            .read_u32()
            .map_err(|error| RawWebError::binary(error, "scene compressed size"))?;
        let decompressed_size = reader
            .read_u32()
            .map_err(|error| RawWebError::binary(error, "scene decompressed size"))?;
        scenes.push(RawWebScene {
            compressed_size,
            decompressed_size,
        });
    }

    let complete_file_size = if version >= 2 {
        Some(
            reader
                .read_u32()
                .map_err(|error| RawWebError::binary(error, "complete file size"))?,
        )
    } else {
        None
    };
    let uncompressed_blocks_info_size = if version >= 3 {
        Some(
            reader
                .read_u32()
                .map_err(|error| RawWebError::binary(error, "uncompressed metadata size"))?,
        )
    } else {
        None
    };
    reader
        .align(4)
        .map_err(|error| RawWebError::binary(error, "bundle header alignment"))?;
    if reader.position() != header_size {
        return Err(RawWebError::new(
            header_size_field_offset,
            RawWebErrorKind::HeaderSizeMismatch {
                declared: header_size,
                actual: reader.position(),
            },
        ));
    }

    Ok(RawWebHeader {
        kind,
        version,
        unity_version,
        unity_revision,
        hash,
        crc,
        minimum_streamed_bytes,
        header_size,
        number_of_scenes_to_download_before_streaming,
        scenes,
        complete_file_size,
        uncompressed_blocks_info_size,
    })
}

fn checked_count(
    count: i32,
    limit: usize,
    offset: usize,
    field: &'static str,
) -> Result<usize, RawWebError> {
    let count = usize::try_from(count)
        .map_err(|_| RawWebError::new(offset, RawWebErrorKind::NegativeCount { field, count }))?;
    if count > limit {
        return Err(RawWebError::new(
            offset,
            RawWebErrorKind::LimitExceeded {
                field,
                requested: count as u64,
                limit,
            },
        ));
    }
    Ok(count)
}

fn read_entries(
    reader: &mut BinaryReader<'_>,
    data_size: usize,
    limits: RawWebLimits,
) -> Result<Vec<RawWebEntry>, RawWebError> {
    let count_offset = reader.offset();
    let count = reader
        .read_i32()
        .map_err(|error| RawWebError::binary(error, "directory count"))?;
    let count = checked_count(count, limits.max_nodes, count_offset, "directory nodes")?;
    let mut entries = Vec::new();
    entries.try_reserve_exact(count).map_err(|_| {
        RawWebError::new(
            count_offset,
            RawWebErrorKind::AllocationFailed {
                field: "directory nodes",
                requested: count,
            },
        )
    })?;

    for _ in 0..count {
        let path = reader
            .read_null_terminated_utf8(limits.max_node_path_bytes)
            .map_err(|error| RawWebError::binary(error, "directory path"))?
            .to_owned();
        let range_offset = reader.offset();
        let signed_offset = reader
            .read_i32()
            .map_err(|error| RawWebError::binary(error, "directory entry offset"))?;
        let signed_size = reader
            .read_i32()
            .map_err(|error| RawWebError::binary(error, "directory entry size"))?;
        let offset = u32::try_from(signed_offset).map_err(|_| {
            RawWebError::new(
                range_offset,
                RawWebErrorKind::NegativeEntryRange {
                    path: path.clone(),
                    offset: signed_offset,
                    size: signed_size,
                },
            )
        })?;
        let size = u32::try_from(signed_size).map_err(|_| {
            RawWebError::new(
                range_offset + 4,
                RawWebErrorKind::NegativeEntryRange {
                    path: path.clone(),
                    offset: signed_offset,
                    size: signed_size,
                },
            )
        })?;
        if size as usize > limits.max_entry_bytes {
            return Err(RawWebError::new(
                range_offset + 4,
                RawWebErrorKind::LimitExceeded {
                    field: "directory entry bytes",
                    requested: size as u64,
                    limit: limits.max_entry_bytes,
                },
            ));
        }
        let end = (offset as usize)
            .checked_add(size as usize)
            .ok_or_else(|| {
                RawWebError::new(
                    range_offset,
                    RawWebErrorKind::EntryRangeOverflow {
                        path: path.clone(),
                        offset,
                        size,
                    },
                )
            })?;
        if end > data_size {
            return Err(RawWebError::new(
                range_offset,
                RawWebErrorKind::EntryOutOfBounds {
                    path: path.clone(),
                    offset,
                    size,
                    data_size,
                },
            ));
        }
        entries.push(RawWebEntry { path, offset, size });
    }

    reader
        .align(4)
        .map_err(|error| RawWebError::binary(error, "directory alignment"))?;
    Ok(entries)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawWebError {
    offset: usize,
    kind: RawWebErrorKind,
}

impl RawWebError {
    fn new(offset: usize, kind: RawWebErrorKind) -> Self {
        Self { offset, kind }
    }

    fn binary(error: BinaryError, stage: &'static str) -> Self {
        Self::new(
            error.offset(),
            RawWebErrorKind::Binary {
                stage,
                detail: error.to_string(),
            },
        )
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn kind(&self) -> &RawWebErrorKind {
        &self.kind
    }
}

impl Display for RawWebError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} at byte offset {}", self.kind, self.offset)
    }
}

impl Error for RawWebError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RawWebErrorKind {
    Binary {
        stage: &'static str,
        detail: String,
    },
    InvalidSignature {
        actual: String,
    },
    NegativeVersion {
        version: i32,
    },
    InvalidHeaderSize {
        value: i32,
    },
    HeaderSizeMismatch {
        declared: usize,
        actual: usize,
    },
    NegativeCount {
        field: &'static str,
        count: i32,
    },
    LimitExceeded {
        field: &'static str,
        requested: u64,
        limit: usize,
    },
    AllocationFailed {
        field: &'static str,
        requested: usize,
    },
    InternalLengthInvariant {
        field: &'static str,
    },
    WebBundleHasNoScenes,
    ChunkRangeOverflow {
        offset: usize,
        size: usize,
    },
    ChunkOutOfBounds {
        offset: usize,
        size: usize,
        input_size: usize,
    },
    Lzma(LzmaBlockError),
    MetadataSizeMismatch {
        declared: u32,
        actual: usize,
    },
    NegativeEntryRange {
        path: String,
        offset: i32,
        size: i32,
    },
    EntryRangeOverflow {
        path: String,
        offset: u32,
        size: u32,
    },
    EntryOutOfBounds {
        path: String,
        offset: u32,
        size: u32,
        data_size: usize,
    },
    EntryIndexOutOfRange {
        index: usize,
        count: usize,
    },
}

impl Display for RawWebErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Binary { stage, detail } => write!(formatter, "{stage}: {detail}"),
            Self::InvalidSignature { actual } => {
                write!(formatter, "unsupported raw/web bundle signature {actual:?}")
            }
            Self::NegativeVersion { version } => {
                write!(formatter, "bundle version is negative ({version})")
            }
            Self::InvalidHeaderSize { value } => {
                write!(formatter, "invalid negative header size {value}")
            }
            Self::HeaderSizeMismatch { declared, actual } => write!(
                formatter,
                "header consumed {actual} bytes but declares {declared} bytes"
            ),
            Self::NegativeCount { field, count } => {
                write!(formatter, "{field} count is negative ({count})")
            }
            Self::LimitExceeded {
                field,
                requested,
                limit,
            } => write!(
                formatter,
                "{field} value {requested} exceeds configured limit {limit}"
            ),
            Self::AllocationFailed { field, requested } => {
                write!(formatter, "could not allocate {requested} {field}")
            }
            Self::InternalLengthInvariant { field } => {
                write!(
                    formatter,
                    "internal fixed-length invariant failed for {field}"
                )
            }
            Self::WebBundleHasNoScenes => {
                write!(formatter, "UnityWeb bundle has no LZMA scene chunks")
            }
            Self::ChunkRangeOverflow { offset, size } => {
                write!(formatter, "chunk range {offset}+{size} overflows")
            }
            Self::ChunkOutOfBounds {
                offset,
                size,
                input_size,
            } => write!(
                formatter,
                "chunk range {offset}..{} exceeds input size {input_size}",
                offset.saturating_add(*size)
            ),
            Self::Lzma(error) => write!(formatter, "UnityWeb LZMA chunk: {error}"),
            Self::MetadataSizeMismatch { declared, actual } => write!(
                formatter,
                "directory metadata has {actual} bytes but declares {declared} bytes"
            ),
            Self::NegativeEntryRange { path, offset, size } => write!(
                formatter,
                "entry {path:?} has a negative range offset={offset}, size={size}"
            ),
            Self::EntryRangeOverflow { path, offset, size } => {
                write!(formatter, "entry {path:?} range {offset}+{size} overflows")
            }
            Self::EntryOutOfBounds {
                path,
                offset,
                size,
                data_size,
            } => write!(
                formatter,
                "entry {path:?} range {offset}..{} exceeds data size {data_size}",
                u64::from(*offset) + u64::from(*size)
            ),
            Self::EntryIndexOutOfRange { index, count } => {
                write!(formatter, "entry index {index} is outside {count} entries")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{RawWebBundle, RawWebKind, RawWebLimits, RawWebScene};
    use crate::compression::wrappers::compress_gzip;
    use crate::io::filesystem::VirtualFileSystem;
    use crate::scheme::{ContainerMountLimits, FileKind, ParsedFile, SchemeReader, WrapperKind};

    struct Fixture {
        bytes: Vec<u8>,
        header_size_offset: usize,
        complete_size_offset: Option<usize>,
        metadata_size_offset: Option<usize>,
    }

    fn raw_metadata(path: &str, payload: &[u8]) -> Vec<u8> {
        let mut metadata = Vec::new();
        metadata.extend_from_slice(&1i32.to_be_bytes());
        metadata.extend_from_slice(path.as_bytes());
        metadata.push(0);
        let offset_location = metadata.len();
        metadata.extend_from_slice(&0i32.to_be_bytes());
        metadata.extend_from_slice(&(payload.len() as i32).to_be_bytes());
        while !metadata.len().is_multiple_of(4) {
            metadata.push(0);
        }
        let payload_offset = metadata.len() as i32;
        metadata[offset_location..offset_location + 4]
            .copy_from_slice(&payload_offset.to_be_bytes());
        metadata.extend_from_slice(payload);
        metadata
    }

    fn make_header(
        kind: RawWebKind,
        version: i32,
        scenes: &[RawWebScene],
        metadata_size: usize,
    ) -> Fixture {
        let mut bytes = Vec::new();
        let magic = match kind {
            RawWebKind::UnityRaw => "UnityRaw",
            RawWebKind::UnityWeb => "UnityWeb",
        };
        bytes.extend_from_slice(magic.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(&version.to_be_bytes());
        bytes.extend_from_slice(b"5.0.0\0");
        bytes.extend_from_slice(b"5.0.0f1\0");
        if version >= 4 {
            bytes.extend_from_slice(&[0x42; 16]);
            bytes.extend_from_slice(&0x1234_5678u32.to_be_bytes());
        }
        bytes.extend_from_slice(&0x1000u32.to_be_bytes());
        let header_size_offset = bytes.len();
        bytes.extend_from_slice(&0i32.to_be_bytes());
        bytes.extend_from_slice(&(scenes.len() as i32).to_be_bytes());
        bytes.extend_from_slice(&(scenes.len() as i32).to_be_bytes());
        for scene in scenes {
            bytes.extend_from_slice(&scene.compressed_size.to_be_bytes());
            bytes.extend_from_slice(&scene.decompressed_size.to_be_bytes());
        }
        let complete_size_offset = if version >= 2 {
            let offset = bytes.len();
            bytes.extend_from_slice(&0u32.to_be_bytes());
            Some(offset)
        } else {
            None
        };
        let metadata_size_offset = if version >= 3 {
            let offset = bytes.len();
            bytes.extend_from_slice(&(metadata_size as u32).to_be_bytes());
            Some(offset)
        } else {
            None
        };
        while !bytes.len().is_multiple_of(4) {
            bytes.push(0);
        }
        let header_size = bytes.len() as i32;
        bytes[header_size_offset..header_size_offset + 4]
            .copy_from_slice(&header_size.to_be_bytes());
        Fixture {
            bytes,
            header_size_offset,
            complete_size_offset,
            metadata_size_offset,
        }
    }

    fn make_raw_fixture(version: i32, payload: &[u8]) -> Fixture {
        let metadata = raw_metadata("assets/data.bin", payload);
        let metadata_size = metadata.len() - payload.len();
        let mut fixture = make_header(RawWebKind::UnityRaw, version, &[], metadata_size);
        fixture.bytes.extend_from_slice(&metadata);
        if let Some(offset) = fixture.complete_size_offset {
            let size = fixture.bytes.len() as u32;
            fixture.bytes[offset..offset + 4].copy_from_slice(&size.to_be_bytes());
        }
        fixture
    }

    fn make_raw_bundle(version: i32, payload: &[u8]) -> Vec<u8> {
        make_raw_fixture(version, payload).bytes
    }

    #[test]
    fn reads_raw_directory_entries_and_versioned_header_fields() {
        let payload = b"legacy raw resource";
        for version in 1..=8 {
            let input = make_raw_bundle(version, payload);
            let bundle = RawWebBundle::parse(&input).unwrap();
            assert_eq!(bundle.header.kind, RawWebKind::UnityRaw);
            assert_eq!(bundle.header.version, version);
            assert_eq!(bundle.header.hash.is_some(), version >= 4);
            assert_eq!(bundle.header.crc, (version >= 4).then_some(0x1234_5678));
            assert_eq!(bundle.header.complete_file_size.is_some(), version >= 2);
            assert_eq!(
                bundle.header.uncompressed_blocks_info_size.is_some(),
                version >= 3
            );
            assert_eq!(bundle.entries.len(), 1);
            assert_eq!(bundle.entries[0].path, "assets/data.bin");
            assert_eq!(bundle.read_entry(0).unwrap(), payload);
        }
    }

    #[test]
    fn recursively_dispatches_raw_bundle_entries() {
        let nested = compress_gzip(b"nested UnityRaw entry", 1024).unwrap();
        let input = make_raw_bundle(4, &nested);
        let bundle = SchemeReader::read(&input).unwrap();
        assert_eq!(bundle.kind(), FileKind::UnityRaw);

        let entry = bundle.dispatch_entry(0).unwrap();
        assert_eq!(entry.kind(), FileKind::Resource);
        assert_eq!(entry.bytes(), b"nested UnityRaw entry");
        assert_eq!(entry.wrappers()[0].kind, WrapperKind::Gzip);
        assert!(matches!(entry.parse().unwrap(), ParsedFile::Resource(_)));
    }

    #[test]
    fn mounts_unityraw_entries_using_the_parsed_directory_paths() {
        let payload = b"UnityRaw direct mount payload";
        let input = make_raw_bundle(4, payload);
        let dispatched = SchemeReader::read(&input).unwrap();
        let mut filesystem = VirtualFileSystem::new();
        let report = dispatched
            .mount_container_entries(
                &mut filesystem,
                Some("/bundles/raw"),
                ContainerMountLimits::default(),
            )
            .unwrap();

        assert_eq!(report.mounted_entries, 1);
        assert_eq!(report.mounted_bytes, payload.len() as u64);
        assert_eq!(
            filesystem
                .read_all_bytes("/bundles/raw/assets/data.bin", 128)
                .unwrap(),
            payload
        );
    }

    #[test]
    fn decodes_the_last_unity_web_scene_chunk() {
        let payload = compress_gzip(b"nested UnityWeb entry", 1024).unwrap();
        let chunk = raw_metadata("WebData", &payload);
        let mut compressed = Vec::new();
        lzma_rs::lzma_compress(&mut std::io::Cursor::new(&chunk), &mut compressed).unwrap();
        compressed[5..13].copy_from_slice(&(chunk.len() as u64).to_le_bytes());
        let scenes = [
            RawWebScene {
                compressed_size: 1,
                decompressed_size: 1,
            },
            RawWebScene {
                compressed_size: compressed.len() as u32,
                decompressed_size: chunk.len() as u32,
            },
        ];
        let metadata_size = chunk.len() - payload.len();
        let mut fixture = make_header(RawWebKind::UnityWeb, 4, &scenes, metadata_size);
        fixture.bytes.extend_from_slice(&compressed);

        let bundle = RawWebBundle::parse(&fixture.bytes).unwrap();
        assert_eq!(bundle.header.kind, RawWebKind::UnityWeb);
        assert_eq!(bundle.header.scenes.len(), 2);
        assert_eq!(bundle.entries[0].path, "WebData");
        assert_eq!(bundle.read_entry(0).unwrap(), payload);

        let dispatched = SchemeReader::read(&fixture.bytes).unwrap();
        assert_eq!(dispatched.kind(), FileKind::UnityWebBundle);
        let mut filesystem = VirtualFileSystem::new();
        let report = dispatched
            .mount_container_entries(
                &mut filesystem,
                Some("/bundles/web"),
                ContainerMountLimits::default(),
            )
            .unwrap();
        assert_eq!(report.mounted_entries, 1);
        assert_eq!(report.mounted_bytes, payload.len() as u64);
        assert_eq!(
            filesystem
                .read_all_bytes("/bundles/web/WebData", 1024)
                .unwrap(),
            payload
        );

        let entry = dispatched.dispatch_entry(0).unwrap();
        assert_eq!(entry.kind(), FileKind::Resource);
        assert_eq!(entry.bytes(), b"nested UnityWeb entry");
        assert_eq!(entry.wrappers()[0].kind, WrapperKind::Gzip);
        assert!(matches!(entry.parse().unwrap(), ParsedFile::Resource(_)));

        let limits = RawWebLimits {
            max_decompressed_chunk_bytes: chunk.len() - 1,
            ..RawWebLimits::default()
        };
        let limit_error = RawWebBundle::parse_with_limits(&fixture.bytes, limits).unwrap_err();
        assert!(limit_error.to_string().contains("exceeds configured limit"));
    }

    #[test]
    fn enforces_header_metadata_chunk_and_entry_limits() {
        let raw = make_raw_bundle(4, b"entry");

        let limits = RawWebLimits {
            max_header_bytes: 4,
            ..RawWebLimits::default()
        };
        let header_limit = RawWebBundle::parse_with_limits(&raw, limits).unwrap_err();
        assert!(header_limit.to_string().contains("header bytes"));

        let limits = RawWebLimits {
            max_entry_bytes: 4,
            ..RawWebLimits::default()
        };
        let entry_limit = RawWebBundle::parse_with_limits(&raw, limits).unwrap_err();
        assert!(entry_limit.to_string().contains("directory entry bytes"));

        let limits = RawWebLimits {
            max_metadata_bytes: 2,
            ..RawWebLimits::default()
        };
        let metadata_limit = RawWebBundle::parse_with_limits(&raw, limits).unwrap_err();
        assert!(metadata_limit.to_string().contains("directory count"));
    }

    #[test]
    fn rejects_invalid_signature_metadata_size_and_entry_ranges() {
        let bad_signature = RawWebBundle::parse(b"not a Unity bundle\0").unwrap_err();
        assert!(bad_signature.to_string().contains("signature"));

        let empty_web = make_header(RawWebKind::UnityWeb, 4, &[], 0);
        let no_scenes = RawWebBundle::parse(&empty_web.bytes).unwrap_err();
        assert!(no_scenes.to_string().contains("no LZMA scene chunks"));

        let mut wrong_metadata = make_raw_fixture(4, b"data");
        let metadata_size_offset = wrong_metadata.metadata_size_offset.unwrap();
        wrong_metadata.bytes[metadata_size_offset..metadata_size_offset + 4]
            .copy_from_slice(&1u32.to_be_bytes());
        let metadata_error = RawWebBundle::parse(&wrong_metadata.bytes).unwrap_err();
        assert!(metadata_error.to_string().contains("metadata has"));

        let mut outside_entry = make_raw_fixture(4, b"data");
        let header_size = i32::from_be_bytes(
            outside_entry.bytes
                [outside_entry.header_size_offset..outside_entry.header_size_offset + 4]
                .try_into()
                .expect("fixture header field width"),
        ) as usize;
        let range_offset = header_size + 4 + "assets/data.bin".len() + 1;
        outside_entry.bytes[range_offset..range_offset + 4]
            .copy_from_slice(&i32::MAX.to_be_bytes());
        let range_error = RawWebBundle::parse(&outside_entry.bytes).unwrap_err();
        assert!(range_error.to_string().contains("exceeds data size"));
    }

    #[test]
    fn rejects_corrupt_header_fields_with_context() {
        // A negative bundle version fails before any version-gated field is read.
        let negative_version = make_header(RawWebKind::UnityRaw, -1, &[], 0);
        let version_error = RawWebBundle::parse(&negative_version.bytes).unwrap_err();
        assert!(version_error.to_string().contains("is negative (-1)"));

        // A negative declared header size fails at its field offset.
        let mut negative_size = make_raw_fixture(4, b"data");
        negative_size.bytes[negative_size.header_size_offset..negative_size.header_size_offset + 4]
            .copy_from_slice(&(-5i32).to_be_bytes());
        let size_error = RawWebBundle::parse(&negative_size.bytes).unwrap_err();
        assert!(size_error
            .to_string()
            .contains("invalid negative header size -5"));

        // A declared size past the consumed fields fails the header-size check.
        let mut drifted = make_raw_fixture(4, b"data");
        let declared = i32::from_be_bytes(
            drifted.bytes[drifted.header_size_offset..drifted.header_size_offset + 4]
                .try_into()
                .expect("fixture header field width"),
        );
        drifted.bytes.extend_from_slice(&[0; 4]);
        drifted.bytes[drifted.header_size_offset..drifted.header_size_offset + 4]
            .copy_from_slice(&(declared + 4).to_be_bytes());
        let drift_error = RawWebBundle::parse(&drifted.bytes).unwrap_err();
        assert!(drift_error.to_string().contains("but declares"));
    }

    #[test]
    fn headers_and_entries_match_reference_constructed_bundles() {
        fn hex_to_bytes(hex: &str) -> Vec<u8> {
            assert!(hex.len().is_multiple_of(2), "odd hex length for {hex}");
            let mut out = Vec::with_capacity(hex.len() / 2);
            let bytes = hex.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                let hi = match bytes[i] {
                    b'0'..=b'9' => bytes[i] - b'0',
                    b'a'..=b'f' => bytes[i] - b'a' + 10,
                    b'A'..=b'F' => bytes[i] - b'A' + 10,
                    b => panic!("invalid hex byte {b}"),
                };
                let lo = match bytes[i + 1] {
                    b'0'..=b'9' => bytes[i + 1] - b'0',
                    b'a'..=b'f' => bytes[i + 1] - b'a' + 10,
                    b'A'..=b'F' => bytes[i + 1] - b'A' + 10,
                    b => panic!("invalid hex byte {b}"),
                };
                out.push((hi << 4) | lo);
                i += 2;
            }
            out
        }

        fn field<'a>(parts: &[&'a str], key: &str) -> &'a str {
            let prefix = format!("{key}=");
            for part in parts {
                if let Some(v) = part.strip_prefix(&prefix) {
                    return v;
                }
            }
            panic!("missing {key} in {parts:?}");
        }

        // Probe Temp/opencode/legacybundleprobe (Raw/WebBundleHeader.Read and
        // Raw/WebBundleFile.Read over C#-constructed bundles, SDK 10.0.401;
        // Web scene via LzmaCompression): H1-H6 version gates v1/2/3/4/8 and
        // Web v6, R1 Raw entry, W1 Web LZMA entry, M1 corrupt HeaderSize.
        // Nothing here was produced by the Rust writer (Raw/Web Write throws
        // NotImplementedException in the baseline, proven by HW).
        let fixture = include_str!("legacy_bundle_fixture.txt");
        let mut hexes: std::collections::HashMap<String, Vec<u8>> =
            std::collections::HashMap::new();
        let mut reads: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for line in fixture.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            if line.starts_with("LEGACY_HEX|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad LEGACY_HEX {line}");
                hexes.insert(parts[1].to_string(), hex_to_bytes(parts[2]));
            } else if line.starts_with("LEGACY_READ|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad LEGACY_READ {line}");
                reads.insert(parts[1].to_string(), parts[2].to_string());
            }
        }

        // Baseline cannot write Raw/Web bundles; no Rust write unit is owed.
        let hw = reads.get("HW").expect("HW row");
        assert!(hw.contains("NotImplementedException"), "HW refusal {hw}");

        // Version gates reproduce across v1/2/3/4/8 Raw (full bundles with
        // an empty directory); H6 Web v6 header gates are reference-proven
        // here and Rust-covered via W1 below.
        for (case, want_version, want_kind, want_scenes, want_comp, want_unc, want_hash) in [
            ("H1", 1, RawWebKind::UnityRaw, 0, None, None, false),
            ("H2", 2, RawWebKind::UnityRaw, 1, Some(500), None, false),
            ("H3", 3, RawWebKind::UnityRaw, 1, Some(500), Some(4), false),
            ("H4", 4, RawWebKind::UnityRaw, 2, Some(500), Some(4), true),
            ("H5", 8, RawWebKind::UnityRaw, 0, Some(500), Some(4), true),
        ] {
            let bytes = hexes.get(case).unwrap_or_else(|| panic!("missing {case}"));
            let dump = reads.get(case).unwrap_or_else(|| panic!("missing {case}"));
            let parts: Vec<&str> = dump.split('|').collect();
            assert_eq!(field(&parts, "V").parse::<i32>().unwrap(), want_version);
            let bundle = RawWebBundle::parse(bytes)
                .unwrap_or_else(|e| panic!("{case} Rust parse failed: {e:?}"));
            assert_eq!(bundle.header.kind, want_kind, "{case} kind");
            assert_eq!(bundle.header.version, want_version, "{case} version");
            assert_eq!(bundle.header.scenes.len(), want_scenes, "{case} scenes");
            assert_eq!(
                bundle.header.complete_file_size, want_comp,
                "{case} complete"
            );
            assert_eq!(
                bundle.header.uncompressed_blocks_info_size, want_unc,
                "{case} uncomp"
            );
            assert_eq!(bundle.header.hash.is_some(), want_hash, "{case} hash");
            assert_eq!(bundle.header.crc.is_some(), want_hash, "{case} crc");
            assert_eq!(bundle.header.minimum_streamed_bytes, 100);
            assert_eq!(bundle.header.unity_version, "2022.3.1f1");
            assert!(bundle.entries.is_empty(), "{case} empty directory");
        }

        // Raw entry bytes reproduce.
        {
            let bytes = hexes.get("R1").expect("R1 hex");
            let dump = reads.get("R1").expect("R1 dump");
            assert!(
                dump.contains("FILE:a.bin=30313233343536373839"),
                "R1 {dump}"
            );
            let bundle = RawWebBundle::parse(bytes).expect("R1 Rust parse");
            assert_eq!(bundle.entries.len(), 1);
            assert_eq!(bundle.entries[0].path, "a.bin");
            assert_eq!(bundle.read_entry(0).unwrap(), b"0123456789");
        }

        // Web LZMA scene entry bytes reproduce (also covering Web v6 header
        // gates: version, kind, scenes, complete, uncomp, hash).
        {
            let bytes = hexes.get("W1").expect("W1 hex");
            let dump = reads.get("W1").expect("W1 dump");
            assert!(
                dump.contains("FILE:w.bin=4142434445464748494A"),
                "W1 {dump}"
            );
            let bundle = RawWebBundle::parse(bytes).expect("W1 Rust parse");
            assert_eq!(bundle.header.kind, RawWebKind::UnityWeb);
            assert_eq!(bundle.header.version, 6);
            assert_eq!(bundle.header.scenes.len(), 1);
            assert_eq!(bundle.header.complete_file_size, Some(0));
            assert!(bundle.header.hash.is_some());
            assert_eq!(bundle.entries.len(), 1);
            assert_eq!(bundle.read_entry(0).unwrap(), b"ABCDEFGHIJ");
        }

        // Corrupt HeaderSize fails loud on both sides.
        {
            let bytes = hexes.get("M1").expect("M1 hex");
            let dump = reads.get("M1").expect("M1 dump");
            assert!(dump.starts_with("THROW:"), "M1 refusal {dump}");
            assert!(RawWebBundle::parse(bytes).is_err(), "M1 Rust fails loud");
        }
    }
}
