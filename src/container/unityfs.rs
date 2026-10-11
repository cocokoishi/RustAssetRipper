use std::borrow::Cow;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io::{self, Read, Seek, SeekFrom, Write};

use crate::compression::lz4_block::{decompress_block, Lz4BlockError};
use crate::compression::lzma::{decompress_block as decompress_lzma_block, LzmaBlockError};
use crate::compression::zstd::{
    decompress_block as decompress_zstd_block, has_frame_magic, ZstdBlockError,
};
use crate::io::binary::{BinaryError, BinaryReader, Endian};
use crate::io::stream::SmartStream;

const UNITY_FS_MAGIC: &str = "UnityFS";
const BLOCK_INFO_AT_END: u32 = 0x80;
const BLOCK_INFO_NEEDS_PADDING: u32 = 0x200;
const BLOCKS_AND_DIRECTORY_COMBINED: u32 = 0x40;
const COMPRESSION_MASK: u32 = 0x3f;
const BLOCK_COMPRESSION_MASK: u16 = 0x3f;
/// `BundleVersion.BF_LargeFilesSupport`: bundles at this version and later align
/// the post-header stream to 16 bytes (`FileStreamBundleFile.Read`).
///
/// Later `BundleVersion` members share this value in the baseline; only the gate
/// itself is asserted here.
pub const BUNDLE_VERSION_LARGE_FILES_SUPPORT: i32 = 7;
/// `NodeFlags.SerializedFile`: the directory-node bit marking SerializedFile entries.
///
/// The baseline stores this bit on every node but never branches on it (entry kinds
/// are recovered downstream by content sniffing instead); it is pinned here so the
/// named bit survives as a caller fact rather than a magic number. The remaining
/// header bits the baseline never branches on (`OldWebPluginCompatibility`,
/// `EncryptionOld`, `EncryptionNew`) stay uninterpreted for the same reason.
pub const NODE_SERIALIZED_FILE_FLAG: u32 = 4;

/// `CompressionType` enum names (`AssetRipper.IO.Files.BundleFiles.CompressionType`)
/// for diagnostics: 0 None, 1 Lzma, 2 Lz4, 3 Lz4HC, 4 Lzham.
///
/// Unknown discriminants read `None`, mirroring the source `default` branch. One
/// deliberate difference is documented, not changed: the baseline renders these names
/// inside its unsupported-compression texts, while `UnityFsErrorKind::UnsupportedCompression`
/// keeps the numeric discriminant so unknown future types stay reportable.
pub const fn compression_type_name(compression: u8) -> Option<&'static str> {
    match compression {
        0 => Some("None"),
        1 => Some("Lzma"),
        2 => Some("Lz4"),
        3 => Some("Lz4HC"),
        4 => Some("Lzham"),
        _ => None,
    }
}

/// Resource bounds applied before allocating decompressed bundle metadata or entries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnityFsLimits {
    pub max_header_string_bytes: usize,
    pub max_metadata_uncompressed_bytes: usize,
    pub max_storage_blocks: usize,
    pub max_directory_nodes: usize,
    pub max_node_path_bytes: usize,
    /// Maximum materialized directory-entry size. This remains finite while allowing
    /// the large SerializedFile entries used by modern Unity projects.
    pub max_entry_bytes: usize,
    pub max_block_uncompressed_bytes: usize,
    pub max_zstd_window_bytes: usize,
}

impl Default for UnityFsLimits {
    fn default() -> Self {
        Self {
            max_header_string_bytes: 4096,
            max_metadata_uncompressed_bytes: 256 * 1024 * 1024,
            max_storage_blocks: 1_000_000,
            max_directory_nodes: 1_000_000,
            max_node_path_bytes: 1024 * 1024,
            max_entry_bytes: 1024 * 1024 * 1024,
            max_block_uncompressed_bytes: 512 * 1024 * 1024,
            max_zstd_window_bytes: 128 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnityFsHeader {
    pub version: i32,
    pub unity_version: String,
    pub unity_revision: String,
    /// Unity's signed 64-bit size field, preserved as a nonnegative value.
    pub declared_size: u64,
    pub compressed_blocks_info_size: u32,
    pub uncompressed_blocks_info_size: u32,
    pub flags: u32,
    pub header_size: usize,
}

impl UnityFsHeader {
    pub fn metadata_compression(&self) -> u8 {
        (self.flags & COMPRESSION_MASK) as u8
    }

    pub fn blocks_info_at_end(&self) -> bool {
        self.flags & BLOCK_INFO_AT_END != 0
    }

    pub fn directory_is_combined(&self) -> bool {
        self.flags & BLOCKS_AND_DIRECTORY_COMBINED != 0
    }

    pub fn blocks_need_padding_at_start(&self) -> bool {
        self.flags & BLOCK_INFO_NEEDS_PADDING != 0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnityFsStorageBlock {
    pub uncompressed_size: u32,
    pub compressed_size: u32,
    pub flags: u16,
}

impl UnityFsStorageBlock {
    pub fn compression_type(&self) -> u8 {
        (self.flags & BLOCK_COMPRESSION_MASK) as u8
    }

    pub fn is_streamed(&self) -> bool {
        self.flags & 0x40 != 0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnityFsNode {
    pub offset: u64,
    pub size: u64,
    pub flags: u32,
    pub path: String,
}

impl UnityFsNode {
    /// Whether the `NodeFlags.SerializedFile` bit is set on this directory entry.
    pub const fn is_serialized_file(&self) -> bool {
        self.flags & NODE_SERIALIZED_FILE_FLAG != 0
    }
}

/// Read-only view of a UnityFS bundle with parsed blocks and directory entries.
#[derive(Debug)]
pub struct UnityFsBundle<'a> {
    input: &'a [u8],
    limits: UnityFsLimits,
    pub header: UnityFsHeader,
    pub uncompressed_data_hash: [u8; 16],
    pub storage_blocks: Vec<UnityFsStorageBlock>,
    pub nodes: Vec<UnityFsNode>,
    pub metadata_offset: usize,
    pub data_offset: usize,
    pub uncompressed_data_size: u64,
}

impl<'a> UnityFsBundle<'a> {
    pub fn parse(input: &'a [u8]) -> Result<Self, UnityFsError> {
        Self::parse_with_limits(input, UnityFsLimits::default())
    }

    pub fn parse_with_limits(input: &'a [u8], limits: UnityFsLimits) -> Result<Self, UnityFsError> {
        let header_layout = parse_header_slice(input, input.len(), &limits)?;
        let compressed_metadata = input
            .get(header_layout.metadata_offset..header_layout.metadata_end)
            .ok_or_else(|| {
                UnityFsError::new(
                    header_layout.metadata_offset,
                    UnityFsErrorKind::InvalidMetadataRange {
                        metadata_offset: header_layout.metadata_offset,
                        metadata_size: header_layout.header.compressed_blocks_info_size,
                        input_size: input.len(),
                    },
                )
            })?;
        let parsed = parse_layout(input.len(), header_layout, compressed_metadata, &limits)?;

        Ok(Self {
            input,
            limits,
            header: parsed.header,
            uncompressed_data_hash: parsed.uncompressed_data_hash,
            storage_blocks: parsed.storage_blocks,
            nodes: parsed.nodes,
            metadata_offset: parsed.metadata_offset,
            data_offset: parsed.data_offset,
            uncompressed_data_size: parsed.uncompressed_data_size,
        })
    }
    /// Read one directory entry. A single uncompressed storage block can be borrowed directly;
    /// entries crossing or using compressed blocks are returned in an owned buffer.
    pub fn read_entry(&self, entry_index: usize) -> Result<Cow<'a, [u8]>, UnityFsError> {
        let entry = self.nodes.get(entry_index).ok_or_else(|| {
            UnityFsError::new(
                self.metadata_offset,
                UnityFsErrorKind::InvalidEntryIndex {
                    entry_index,
                    entry_count: self.nodes.len(),
                },
            )
        })?;
        let entry_size = usize::try_from(entry.size).map_err(|_| {
            UnityFsError::new(
                self.data_offset,
                UnityFsErrorKind::AddressSpaceOverflow("entry size"),
            )
        })?;
        if entry_size > self.limits.max_entry_bytes {
            return Err(UnityFsError::new(
                self.data_offset,
                UnityFsErrorKind::LimitExceeded {
                    resource: "bundle entry",
                    requested: entry.size,
                    limit: self.limits.max_entry_bytes,
                },
            ));
        }
        let entry_start = entry.offset;
        let entry_end = entry_start.checked_add(entry.size).ok_or_else(|| {
            UnityFsError::new(
                self.data_offset,
                UnityFsErrorKind::AddressSpaceOverflow("entry range"),
            )
        })?;

        if entry_size == 0 {
            let empty = self
                .input
                .get(self.data_offset..self.data_offset)
                .ok_or_else(|| {
                    UnityFsError::new(
                        self.data_offset,
                        UnityFsErrorKind::InvalidDataRange {
                            data_offset: self.data_offset,
                            data_end: self.input.len(),
                            input_size: self.input.len(),
                        },
                    )
                })?;
            return Ok(Cow::Borrowed(empty));
        }

        if self.storage_blocks.len() == 1 && self.storage_blocks[0].compression_type() == 0 {
            let start = usize::try_from(entry.offset)
                .ok()
                .and_then(|offset| self.data_offset.checked_add(offset))
                .ok_or_else(|| {
                    UnityFsError::new(
                        self.data_offset,
                        UnityFsErrorKind::AddressSpaceOverflow("borrowed entry range"),
                    )
                })?;
            let end = start.checked_add(entry_size).ok_or_else(|| {
                UnityFsError::new(
                    start,
                    UnityFsErrorKind::AddressSpaceOverflow("borrowed entry range"),
                )
            })?;
            let borrowed = self.input.get(start..end).ok_or_else(|| {
                UnityFsError::new(
                    start,
                    UnityFsErrorKind::InvalidDataRange {
                        data_offset: start,
                        data_end: end,
                        input_size: self.input.len(),
                    },
                )
            })?;
            return Ok(Cow::Borrowed(borrowed));
        }

        let mut output = Vec::new();
        output.try_reserve_exact(entry_size).map_err(|_| {
            UnityFsError::new(
                self.data_offset,
                UnityFsErrorKind::AllocationFailed {
                    resource: "bundle entry",
                    requested: entry_size,
                },
            )
        })?;
        let mut compressed_offset = 0u64;
        let mut block_uncompressed_offset = 0u64;
        for (block_index, block) in self.storage_blocks.iter().enumerate() {
            let block_end = block_uncompressed_offset
                .checked_add(u64::from(block.uncompressed_size))
                .ok_or_else(|| {
                    UnityFsError::new(
                        self.data_offset,
                        UnityFsErrorKind::AddressSpaceOverflow("block output range"),
                    )
                })?;
            if let Some((overlap_start, overlap_end)) =
                entry_block_window(entry_start, entry_end, block_uncompressed_offset, block_end)
            {
                let compressed_start = usize::try_from(compressed_offset)
                    .ok()
                    .and_then(|offset| self.data_offset.checked_add(offset))
                    .ok_or_else(|| {
                        UnityFsError::new(
                            self.data_offset,
                            UnityFsErrorKind::AddressSpaceOverflow("compressed block offset"),
                        )
                    })?;
                let compressed_end = compressed_start
                    .checked_add(block.compressed_size as usize)
                    .ok_or_else(|| {
                        UnityFsError::new(
                            compressed_start,
                            UnityFsErrorKind::AddressSpaceOverflow("compressed block range"),
                        )
                    })?;
                let compressed_bytes = self
                    .input
                    .get(compressed_start..compressed_end)
                    .ok_or_else(|| {
                        UnityFsError::new(
                            compressed_start,
                            UnityFsErrorKind::InvalidDataRange {
                                data_offset: compressed_start,
                                data_end: compressed_end,
                                input_size: self.input.len(),
                            },
                        )
                    })?;
                let local_start = usize::try_from(overlap_start - block_uncompressed_offset)
                    .map_err(|_| {
                        UnityFsError::new(
                            compressed_start,
                            UnityFsErrorKind::AddressSpaceOverflow("entry block offset"),
                        )
                    })?;
                let local_end =
                    usize::try_from(overlap_end - block_uncompressed_offset).map_err(|_| {
                        UnityFsError::new(
                            compressed_start,
                            UnityFsErrorKind::AddressSpaceOverflow("entry block range"),
                        )
                    })?;
                if block.compression_type() == 0 {
                    let part = compressed_bytes
                        .get(local_start..local_end)
                        .ok_or_else(|| {
                            UnityFsError::new(
                                compressed_start,
                                UnityFsErrorKind::InvalidBlockSlice {
                                    block_index,
                                    start: local_start,
                                    end: local_end,
                                    block_size: compressed_bytes.len(),
                                },
                            )
                        })?;
                    output.extend_from_slice(part);
                } else {
                    let decoded = decode_payload(
                        compressed_bytes,
                        block.uncompressed_size as usize,
                        self.limits.max_block_uncompressed_bytes,
                        block.compression_type(),
                        compressed_start,
                        "storage block",
                        Some(self.limits.max_zstd_window_bytes),
                    )?;
                    let part = decoded.get(local_start..local_end).ok_or_else(|| {
                        UnityFsError::new(
                            compressed_start,
                            UnityFsErrorKind::InvalidBlockSlice {
                                block_index,
                                start: local_start,
                                end: local_end,
                                block_size: decoded.len(),
                            },
                        )
                    })?;
                    output.extend_from_slice(part);
                }
            }
            compressed_offset = compressed_offset
                .checked_add(u64::from(block.compressed_size))
                .ok_or_else(|| {
                    UnityFsError::new(
                        self.data_offset,
                        UnityFsErrorKind::AddressSpaceOverflow("compressed block offset"),
                    )
                })?;
            block_uncompressed_offset = block_end;
        }

        if output.len() != entry_size {
            return Err(UnityFsError::new(
                self.data_offset,
                UnityFsErrorKind::EntryOutputSizeMismatch {
                    entry_index,
                    expected: entry_size,
                    actual: output.len(),
                },
            ));
        }
        Ok(Cow::Owned(output))
    }
}

/// UnityFS metadata parsed from a seekable source stream.
///
/// Directory entries remain backed by the original source for the common single-block
/// uncompressed case. Compressed and multi-block entries are materialized one entry at a time.
pub struct UnityFsStreamBundle {
    source: SmartStream,
    limits: UnityFsLimits,
    max_input_bytes: usize,
    pub header: UnityFsHeader,
    pub uncompressed_data_hash: [u8; 16],
    pub storage_blocks: Vec<UnityFsStorageBlock>,
    pub nodes: Vec<UnityFsNode>,
    pub metadata_offset: usize,
    pub data_offset: usize,
    pub uncompressed_data_size: u64,
}

impl UnityFsStreamBundle {
    /// Parse only the bounded UnityFS header and metadata from a seekable source.
    pub fn parse(
        source: &SmartStream,
        limits: UnityFsLimits,
        max_input_bytes: usize,
    ) -> Result<Self, UnityFsError> {
        let source_length = source
            .length()
            .map_err(|error| stream_input_error(0, "UnityFS input length", error))?;
        if source_length > u64::try_from(max_input_bytes).unwrap_or(u64::MAX) {
            return Err(UnityFsError::new(
                0,
                UnityFsErrorKind::LimitExceeded {
                    resource: "UnityFS input",
                    requested: source_length,
                    limit: max_input_bytes,
                },
            ));
        }
        let input_size = usize::try_from(source_length).map_err(|_| {
            UnityFsError::new(
                0,
                UnityFsErrorKind::AddressSpaceOverflow("UnityFS input size"),
            )
        })?;

        let mut reader = source.create_reference();
        let header_prefix = read_header_prefix(&mut reader, input_size, &limits)?;
        let header_layout = parse_header_slice(&header_prefix, input_size, &limits)?;
        let metadata_size = usize::try_from(header_layout.header.compressed_blocks_info_size)
            .map_err(|_| {
                UnityFsError::new(
                    header_layout.metadata_offset,
                    UnityFsErrorKind::AddressSpaceOverflow("compressed metadata size"),
                )
            })?;
        let mut compressed_metadata = Vec::new();
        compressed_metadata
            .try_reserve_exact(metadata_size)
            .map_err(|_| {
                UnityFsError::new(
                    header_layout.metadata_offset,
                    UnityFsErrorKind::AllocationFailed {
                        resource: "compressed blocks metadata",
                        requested: metadata_size,
                    },
                )
            })?;
        compressed_metadata.resize(metadata_size, 0);
        reader
            .seek(SeekFrom::Start(header_layout.metadata_offset as u64))
            .map_err(|error| {
                stream_input_error(
                    header_layout.metadata_offset,
                    "UnityFS metadata seek",
                    error,
                )
            })?;
        reader
            .read_exact(&mut compressed_metadata)
            .map_err(|error| {
                stream_input_error(
                    header_layout.metadata_offset,
                    "UnityFS metadata read",
                    error,
                )
            })?;

        let parsed = parse_layout(input_size, header_layout, &compressed_metadata, &limits)?;
        Ok(Self {
            source: source.create_reference(),
            limits,
            max_input_bytes,
            header: parsed.header,
            uncompressed_data_hash: parsed.uncompressed_data_hash,
            storage_blocks: parsed.storage_blocks,
            nodes: parsed.nodes,
            metadata_offset: parsed.metadata_offset,
            data_offset: parsed.data_offset,
            uncompressed_data_size: parsed.uncompressed_data_size,
        })
    }

    /// Return one entry as a source-backed range when possible, otherwise as a bounded stream.
    pub fn read_entry(&self, entry_index: usize) -> Result<SmartStream, UnityFsError> {
        self.read_entry_with_source_backing(entry_index)
            .map(|(stream, _)| stream)
    }

    /// Return an entry stream and whether it is a range over the input rather than materialized.
    pub fn read_entry_with_source_backing(
        &self,
        entry_index: usize,
    ) -> Result<(SmartStream, bool), UnityFsError> {
        let entry = self.nodes.get(entry_index).ok_or_else(|| {
            UnityFsError::new(
                self.metadata_offset,
                UnityFsErrorKind::InvalidEntryIndex {
                    entry_index,
                    entry_count: self.nodes.len(),
                },
            )
        })?;
        let entry_size = usize::try_from(entry.size).map_err(|_| {
            UnityFsError::new(
                self.data_offset,
                UnityFsErrorKind::AddressSpaceOverflow("entry size"),
            )
        })?;
        if entry_size > self.limits.max_entry_bytes {
            return Err(UnityFsError::new(
                self.data_offset,
                UnityFsErrorKind::LimitExceeded {
                    resource: "bundle entry",
                    requested: entry.size,
                    limit: self.limits.max_entry_bytes,
                },
            ));
        }
        let entry_end = entry.offset.checked_add(entry.size).ok_or_else(|| {
            UnityFsError::new(
                self.data_offset,
                UnityFsErrorKind::AddressSpaceOverflow("entry range"),
            )
        })?;

        if self.storage_blocks.len() == 1 && self.storage_blocks[0].compression_type() == 0 {
            let absolute_offset = u64::try_from(self.data_offset)
                .ok()
                .and_then(|offset| offset.checked_add(entry.offset))
                .ok_or_else(|| {
                    UnityFsError::new(
                        self.data_offset,
                        UnityFsErrorKind::AddressSpaceOverflow("source-backed entry offset"),
                    )
                })?;
            let (stream, source_backed) = self
                .source
                .create_partial_file_backed(absolute_offset, entry.size)
                .map_err(|error| {
                    stream_input_error(
                        usize::try_from(absolute_offset).unwrap_or(usize::MAX),
                        "UnityFS source-backed entry range",
                        error,
                    )
                })?;
            return Ok((stream, source_backed));
        }

        if entry_size == 0 {
            return Ok((SmartStream::from_memory(Vec::new()), false));
        }

        const TEMPORARY_OUTPUT_THRESHOLD: usize = 50 * 1024 * 1024;
        let mut output = if entry_size > TEMPORARY_OUTPUT_THRESHOLD {
            SmartStream::create_temp()
        } else {
            Ok(SmartStream::create_memory())
        }
        .map_err(|error| stream_input_error(self.data_offset, "UnityFS entry output", error))?;
        let mut compressed_offset = 0u64;
        let mut block_uncompressed_offset = 0u64;
        let mut output_size = 0u64;
        for (block_index, block) in self.storage_blocks.iter().enumerate() {
            let block_end = block_uncompressed_offset
                .checked_add(u64::from(block.uncompressed_size))
                .ok_or_else(|| {
                    UnityFsError::new(
                        self.data_offset,
                        UnityFsErrorKind::AddressSpaceOverflow("block output range"),
                    )
                })?;
            if let Some((overlap_start, overlap_end)) = entry_block_window(
                entry.offset,
                entry_end,
                block_uncompressed_offset,
                block_end,
            ) {
                let overlap_length = overlap_end - overlap_start;
                let block_absolute_offset = u64::try_from(self.data_offset)
                    .ok()
                    .and_then(|offset| offset.checked_add(compressed_offset))
                    .ok_or_else(|| {
                        UnityFsError::new(
                            self.data_offset,
                            UnityFsErrorKind::AddressSpaceOverflow("compressed block offset"),
                        )
                    })?;
                let local_start = overlap_start - block_uncompressed_offset;
                if block.compression_type() == 0 {
                    let part_offset =
                        block_absolute_offset
                            .checked_add(local_start)
                            .ok_or_else(|| {
                                UnityFsError::new(
                                    self.data_offset,
                                    UnityFsErrorKind::AddressSpaceOverflow(
                                        "uncompressed entry range",
                                    ),
                                )
                            })?;
                    let mut part = self
                        .source
                        .create_partial_file_backed(part_offset, overlap_length)
                        .map(|(stream, _)| stream)
                        .map_err(|error| {
                            stream_input_error(
                                usize::try_from(part_offset).unwrap_or(usize::MAX),
                                "UnityFS uncompressed block range",
                                error,
                            )
                        })?;
                    io::copy(&mut part, &mut output).map_err(|error| {
                        stream_input_error(
                            usize::try_from(part_offset).unwrap_or(usize::MAX),
                            "UnityFS uncompressed block copy",
                            error,
                        )
                    })?;
                } else {
                    let compressed_size = u64::from(block.compressed_size);
                    let compressed = self
                        .source
                        .create_partial_file_backed(block_absolute_offset, compressed_size)
                        .map(|(stream, _)| stream)
                        .map_err(|error| {
                            stream_input_error(
                                usize::try_from(block_absolute_offset).unwrap_or(usize::MAX),
                                "UnityFS compressed block range",
                                error,
                            )
                        })?;
                    let compressed_bytes =
                        compressed.to_vec(self.max_input_bytes).map_err(|error| {
                            stream_input_error(
                                usize::try_from(block_absolute_offset).unwrap_or(usize::MAX),
                                "UnityFS compressed block read",
                                error,
                            )
                        })?;
                    let decoded = decode_payload(
                        &compressed_bytes,
                        block.uncompressed_size as usize,
                        self.limits.max_block_uncompressed_bytes,
                        block.compression_type(),
                        usize::try_from(block_absolute_offset).unwrap_or(usize::MAX),
                        "storage block",
                        Some(self.limits.max_zstd_window_bytes),
                    )?;
                    let local_start = usize::try_from(local_start).map_err(|_| {
                        UnityFsError::new(
                            self.data_offset,
                            UnityFsErrorKind::AddressSpaceOverflow("entry block offset"),
                        )
                    })?;
                    let local_end = local_start
                        .checked_add(usize::try_from(overlap_length).map_err(|_| {
                            UnityFsError::new(
                                self.data_offset,
                                UnityFsErrorKind::AddressSpaceOverflow("entry block range"),
                            )
                        })?)
                        .ok_or_else(|| {
                            UnityFsError::new(
                                self.data_offset,
                                UnityFsErrorKind::AddressSpaceOverflow("entry block range"),
                            )
                        })?;
                    let part = decoded.get(local_start..local_end).ok_or_else(|| {
                        UnityFsError::new(
                            usize::try_from(block_absolute_offset).unwrap_or(usize::MAX),
                            UnityFsErrorKind::InvalidBlockSlice {
                                block_index,
                                start: local_start,
                                end: local_end,
                                block_size: decoded.len(),
                            },
                        )
                    })?;
                    output.write_all(part).map_err(|error| {
                        stream_input_error(
                            usize::try_from(block_absolute_offset).unwrap_or(usize::MAX),
                            "UnityFS decoded block write",
                            error,
                        )
                    })?;
                }
                output_size = output_size.checked_add(overlap_length).ok_or_else(|| {
                    UnityFsError::new(
                        self.data_offset,
                        UnityFsErrorKind::AddressSpaceOverflow("entry output size"),
                    )
                })?;
            }
            compressed_offset = compressed_offset
                .checked_add(u64::from(block.compressed_size))
                .ok_or_else(|| {
                    UnityFsError::new(
                        self.data_offset,
                        UnityFsErrorKind::AddressSpaceOverflow("compressed block offset"),
                    )
                })?;
            block_uncompressed_offset = block_end;
        }
        if output_size != entry.size {
            return Err(UnityFsError::new(
                self.data_offset,
                UnityFsErrorKind::EntryOutputSizeMismatch {
                    entry_index,
                    expected: entry_size,
                    actual: usize::try_from(output_size).unwrap_or(usize::MAX),
                },
            ));
        }
        output
            .seek(SeekFrom::Start(0))
            .map_err(|error| stream_input_error(self.data_offset, "UnityFS entry rewind", error))?;
        Ok((output, false))
    }
}

#[derive(Debug)]
struct UnityFsHeaderLayout {
    header: UnityFsHeader,
    metadata_offset: usize,
    metadata_end: usize,
}

#[derive(Debug)]
struct UnityFsParsedLayout {
    header: UnityFsHeader,
    uncompressed_data_hash: [u8; 16],
    storage_blocks: Vec<UnityFsStorageBlock>,
    nodes: Vec<UnityFsNode>,
    metadata_offset: usize,
    data_offset: usize,
    uncompressed_data_size: u64,
}

fn parse_header_slice(
    input: &[u8],
    input_size: usize,
    limits: &UnityFsLimits,
) -> Result<UnityFsHeaderLayout, UnityFsError> {
    let mut reader = BinaryReader::new(input, Endian::Big);
    let signature = read_string(
        &mut reader,
        limits.max_header_string_bytes,
        "UnityFS signature",
    )?;
    if signature != UNITY_FS_MAGIC {
        return Err(UnityFsError::new(
            0,
            UnityFsErrorKind::InvalidSignature { signature },
        ));
    }
    let version_offset = reader.offset();
    let version = read_i32(&mut reader, "UnityFS bundle version")?;
    if version < 0 {
        return Err(UnityFsError::new(
            version_offset,
            UnityFsErrorKind::NegativeValue {
                field: "bundle version",
                value: i64::from(version),
            },
        ));
    }
    let unity_version = read_string(
        &mut reader,
        limits.max_header_string_bytes,
        "UnityFS engine version",
    )?;
    let unity_revision = read_string(
        &mut reader,
        limits.max_header_string_bytes,
        "UnityFS engine revision",
    )?;
    let declared_size_offset = reader.offset();
    let declared_size_signed = read_i64(&mut reader, "UnityFS declared size")?;
    let declared_size =
        nonnegative_u64(declared_size_signed, declared_size_offset, "declared size")?;
    let compressed_blocks_info_size = nonnegative_u32(
        read_i32(&mut reader, "compressed blocks-info size")?,
        reader.offset().saturating_sub(4),
        "compressed blocks-info size",
    )?;
    let uncompressed_blocks_info_size = nonnegative_u32(
        read_i32(&mut reader, "uncompressed blocks-info size")?,
        reader.offset().saturating_sub(4),
        "uncompressed blocks-info size",
    )?;
    let flags = read_u32(&mut reader, "UnityFS flags")?;
    let header_size = reader.position();
    if version >= BUNDLE_VERSION_LARGE_FILES_SUPPORT {
        reader
            .align(16)
            .map_err(|error| UnityFsError::from_binary(error, "UnityFS v7+ header alignment"))?;
    }
    let header_data_start = reader.position();
    let blocks_at_end = flags & BLOCK_INFO_AT_END != 0;
    let metadata_offset = if blocks_at_end {
        let offset = declared_size
            .checked_sub(u64::from(compressed_blocks_info_size))
            .ok_or_else(|| {
                UnityFsError::new(
                    declared_size_offset,
                    UnityFsErrorKind::InvalidMetadataRange {
                        metadata_offset: 0,
                        metadata_size: compressed_blocks_info_size,
                        input_size,
                    },
                )
            })?;
        usize::try_from(offset).map_err(|_| {
            UnityFsError::new(
                declared_size_offset,
                UnityFsErrorKind::AddressSpaceOverflow("metadata offset"),
            )
        })?
    } else {
        header_data_start
    };
    let metadata_end = metadata_offset
        .checked_add(compressed_blocks_info_size as usize)
        .ok_or_else(|| {
            UnityFsError::new(
                metadata_offset,
                UnityFsErrorKind::AddressSpaceOverflow("metadata range"),
            )
        })?;
    if metadata_end > input_size {
        return Err(UnityFsError::new(
            metadata_offset,
            UnityFsErrorKind::InvalidMetadataRange {
                metadata_offset,
                metadata_size: compressed_blocks_info_size,
                input_size,
            },
        ));
    }
    if metadata_offset < header_size || (!blocks_at_end && metadata_offset < header_data_start) {
        return Err(UnityFsError::new(
            metadata_offset,
            UnityFsErrorKind::InvalidMetadataRange {
                metadata_offset,
                metadata_size: compressed_blocks_info_size,
                input_size,
            },
        ));
    }
    Ok(UnityFsHeaderLayout {
        header: UnityFsHeader {
            version,
            unity_version,
            unity_revision,
            declared_size,
            compressed_blocks_info_size,
            uncompressed_blocks_info_size,
            flags,
            header_size,
        },
        metadata_offset,
        metadata_end,
    })
}

fn parse_layout(
    input_size: usize,
    header_layout: UnityFsHeaderLayout,
    compressed_metadata: &[u8],
    limits: &UnityFsLimits,
) -> Result<UnityFsParsedLayout, UnityFsError> {
    let UnityFsHeaderLayout {
        header,
        metadata_offset,
        metadata_end,
    } = header_layout;
    if compressed_metadata.len() != header.compressed_blocks_info_size as usize {
        return Err(UnityFsError::new(
            metadata_offset,
            UnityFsErrorKind::InvalidMetadataRange {
                metadata_offset,
                metadata_size: header.compressed_blocks_info_size,
                input_size,
            },
        ));
    }
    let uncompressed_metadata_size = usize::try_from(header.uncompressed_blocks_info_size)
        .map_err(|_| {
            UnityFsError::new(
                metadata_offset,
                UnityFsErrorKind::AddressSpaceOverflow("uncompressed metadata size"),
            )
        })?;
    if uncompressed_metadata_size > limits.max_metadata_uncompressed_bytes {
        return Err(UnityFsError::new(
            metadata_offset,
            UnityFsErrorKind::LimitExceeded {
                resource: "uncompressed blocks metadata",
                requested: uncompressed_metadata_size as u64,
                limit: limits.max_metadata_uncompressed_bytes,
            },
        ));
    }
    let metadata_compression = header.metadata_compression();
    let decoded_metadata = decode_payload(
        compressed_metadata,
        uncompressed_metadata_size,
        limits.max_metadata_uncompressed_bytes,
        metadata_compression,
        metadata_offset,
        "blocks metadata",
        None,
    )?;
    let metadata_base_offset = if metadata_compression == 0 {
        metadata_offset
    } else {
        0
    };
    let metadata = parse_metadata(
        decoded_metadata.as_ref(),
        metadata_base_offset,
        metadata_offset,
        uncompressed_metadata_size,
        header.directory_is_combined(),
        limits,
    )?;

    let blocks_at_end = header.blocks_info_at_end();
    let mut data_offset = if blocks_at_end {
        header.header_size
    } else {
        metadata_end
    };
    if blocks_at_end && header.version >= BUNDLE_VERSION_LARGE_FILES_SUPPORT {
        data_offset = align_offset(data_offset, 16).ok_or_else(|| {
            UnityFsError::new(
                data_offset,
                UnityFsErrorKind::AddressSpaceOverflow("data alignment"),
            )
        })?;
    }
    if header.blocks_need_padding_at_start() {
        data_offset = align_offset(data_offset, 16).ok_or_else(|| {
            UnityFsError::new(
                data_offset,
                UnityFsErrorKind::AddressSpaceOverflow("data padding"),
            )
        })?;
    }
    let data_end = if blocks_at_end {
        metadata_offset
    } else {
        input_size
    };
    if data_offset > data_end {
        return Err(UnityFsError::new(
            data_offset,
            UnityFsErrorKind::InvalidDataRange {
                data_offset,
                data_end,
                input_size,
            },
        ));
    }

    let mut compressed_data_size = 0u64;
    let mut uncompressed_data_size = 0u64;
    for (index, block) in metadata.storage_blocks.iter().enumerate() {
        if block.compression_type() == 0 && block.compressed_size != block.uncompressed_size {
            return Err(UnityFsError::new(
                data_offset,
                UnityFsErrorKind::UncompressedBlockSizeMismatch {
                    block_index: index,
                    compressed_size: block.compressed_size,
                    uncompressed_size: block.uncompressed_size,
                },
            ));
        }
        compressed_data_size = compressed_data_size
            .checked_add(u64::from(block.compressed_size))
            .ok_or_else(|| {
                UnityFsError::new(
                    data_offset,
                    UnityFsErrorKind::AddressSpaceOverflow("compressed block ranges"),
                )
            })?;
        uncompressed_data_size = uncompressed_data_size
            .checked_add(u64::from(block.uncompressed_size))
            .ok_or_else(|| {
                UnityFsError::new(
                    data_offset,
                    UnityFsErrorKind::AddressSpaceOverflow("uncompressed block ranges"),
                )
            })?;
    }
    let data_capacity = u64::try_from(data_end - data_offset).map_err(|_| {
        UnityFsError::new(
            data_offset,
            UnityFsErrorKind::AddressSpaceOverflow("available compressed data size"),
        )
    })?;
    if compressed_data_size > data_capacity {
        return Err(UnityFsError::new(
            data_offset,
            UnityFsErrorKind::CompressedBlocksExceedData {
                compressed_size: compressed_data_size,
                available: data_capacity,
            },
        ));
    }
    validate_nodes(&metadata.nodes, uncompressed_data_size, metadata_offset)?;
    Ok(UnityFsParsedLayout {
        header,
        uncompressed_data_hash: metadata.uncompressed_data_hash,
        storage_blocks: metadata.storage_blocks,
        nodes: metadata.nodes,
        metadata_offset,
        data_offset,
        uncompressed_data_size,
    })
}

fn read_header_prefix(
    stream: &mut SmartStream,
    input_size: usize,
    limits: &UnityFsLimits,
) -> Result<Vec<u8>, UnityFsError> {
    stream
        .seek(SeekFrom::Start(0))
        .map_err(|error| stream_input_error(0, "UnityFS header seek", error))?;
    let mut prefix = Vec::new();
    read_stream_cstring(
        stream,
        &mut prefix,
        input_size,
        limits.max_header_string_bytes,
    )?;
    let version_offset = prefix.len();
    read_stream_append(stream, &mut prefix, input_size, 4, "UnityFS bundle version")?;
    let version_bytes: [u8; 4] = prefix[version_offset..version_offset + 4]
        .try_into()
        .map_err(|_| {
            UnityFsError::new(
                version_offset,
                UnityFsErrorKind::AddressSpaceOverflow("header version"),
            )
        })?;
    let version = i32::from_be_bytes(version_bytes);
    read_stream_cstring(
        stream,
        &mut prefix,
        input_size,
        limits.max_header_string_bytes,
    )?;
    read_stream_cstring(
        stream,
        &mut prefix,
        input_size,
        limits.max_header_string_bytes,
    )?;
    read_stream_append(stream, &mut prefix, input_size, 20, "UnityFS fixed header")?;
    if version >= BUNDLE_VERSION_LARGE_FILES_SUPPORT {
        if let Some(aligned_end) = align_offset(prefix.len(), 16) {
            let remaining = aligned_end.saturating_sub(prefix.len());
            read_stream_append(
                stream,
                &mut prefix,
                input_size,
                remaining,
                "UnityFS header alignment",
            )?;
        }
    }
    Ok(prefix)
}

fn read_stream_cstring(
    stream: &mut SmartStream,
    output: &mut Vec<u8>,
    input_size: usize,
    maximum_length: usize,
) -> Result<(), UnityFsError> {
    let start = output.len();
    loop {
        let offset = output.len();
        if offset >= input_size {
            return Err(stream_input_error(
                offset,
                "UnityFS header string",
                io::Error::new(io::ErrorKind::UnexpectedEof, "unterminated string"),
            ));
        }
        let mut byte = [0u8; 1];
        stream
            .read_exact(&mut byte)
            .map_err(|error| stream_input_error(offset, "UnityFS header string", error))?;
        if byte[0] != 0 && output.len() - start >= maximum_length {
            return Err(UnityFsError::new(
                offset,
                UnityFsErrorKind::LimitExceeded {
                    resource: "UnityFS header string",
                    requested: (output.len() - start + 1) as u64,
                    limit: maximum_length,
                },
            ));
        }
        output.try_reserve(1).map_err(|_| {
            UnityFsError::new(
                offset,
                UnityFsErrorKind::AllocationFailed {
                    resource: "UnityFS header",
                    requested: output.len().saturating_add(1),
                },
            )
        })?;
        output.push(byte[0]);
        if byte[0] == 0 {
            return Ok(());
        }
    }
}

fn read_stream_append(
    stream: &mut SmartStream,
    output: &mut Vec<u8>,
    input_size: usize,
    count: usize,
    context: &'static str,
) -> Result<(), UnityFsError> {
    let start = output.len();
    let available = input_size.saturating_sub(start);
    let requested = count.min(available);
    output.try_reserve(requested).map_err(|_| {
        UnityFsError::new(
            start,
            UnityFsErrorKind::AllocationFailed {
                resource: "UnityFS header",
                requested: start.saturating_add(requested),
            },
        )
    })?;
    let original_len = output.len();
    output.resize(original_len + requested, 0);
    stream
        .read_exact(&mut output[original_len..])
        .map_err(|error| stream_input_error(start, context, error))?;
    if requested < count {
        return Err(stream_input_error(
            start + requested,
            context,
            io::Error::new(io::ErrorKind::UnexpectedEof, "truncated header"),
        ));
    }
    Ok(())
}

fn stream_input_error(offset: usize, context: &'static str, error: io::Error) -> UnityFsError {
    UnityFsError::new(
        offset,
        UnityFsErrorKind::StreamInput {
            context,
            detail: error.to_string(),
        },
    )
}

/// Half-open overlap of one storage block with the entry byte range, if any.
///
/// This is the `BundleFileBlockReader.ReadEntry` block walk as a pure interval: the
/// first block whose end passes the entry start opens the window (the source's
/// `blockDecompressedOffset + UncompressedSize <= entry.Offset` search), and every
/// later block contributes up to the entry end (the source's
/// `Math.Min(blockSize, left)` copy). Adjacent ranges do not overlap, and an empty
/// entry never reaches this kernel because `read_entry` returns early for it.
fn entry_block_window(
    entry_start: u64,
    entry_end: u64,
    block_start: u64,
    block_end: u64,
) -> Option<(u64, u64)> {
    let start = entry_start.max(block_start);
    let end = entry_end.min(block_end);
    (start < end).then_some((start, end))
}

fn parse_metadata(
    bytes: &[u8],
    reader_base_offset: usize,
    physical_offset: usize,
    declared_size: usize,
    has_directory: bool,
    limits: &UnityFsLimits,
) -> Result<ParsedMetadata, UnityFsError> {
    let mut reader = BinaryReader::with_base_offset(bytes, Endian::Big, reader_base_offset)
        .map_err(|error| UnityFsError::from_binary(error, "decoded UnityFS blocks metadata"))?;
    let mut hash = [0u8; 16];
    hash.copy_from_slice(
        reader
            .read_exact(16)
            .map_err(|error| metadata_binary_error(error, physical_offset, reader_base_offset))?,
    );
    let block_count = read_metadata_count(
        &mut reader,
        "storage blocks",
        limits.max_storage_blocks,
        physical_offset,
        reader_base_offset,
    )?;
    let mut blocks = Vec::new();
    blocks.try_reserve_exact(block_count).map_err(|_| {
        UnityFsError::new(
            physical_offset,
            UnityFsErrorKind::AllocationFailed {
                resource: "storage block table",
                requested: block_count,
            },
        )
    })?;
    for _ in 0..block_count {
        let uncompressed_size = read_metadata_u32(
            &mut reader,
            physical_offset,
            reader_base_offset,
            "storage block uncompressed size",
        )?;
        if uncompressed_size as usize > limits.max_block_uncompressed_bytes {
            return Err(metadata_limit_error(
                physical_offset,
                reader.position(),
                "storage block output",
                u64::from(uncompressed_size),
                limits.max_block_uncompressed_bytes,
            ));
        }
        let compressed_size = read_metadata_u32(
            &mut reader,
            physical_offset,
            reader_base_offset,
            "storage block compressed size",
        )?;
        let flags = read_metadata_u16(
            &mut reader,
            physical_offset,
            reader_base_offset,
            "storage block flags",
        )?;
        blocks.push(UnityFsStorageBlock {
            uncompressed_size,
            compressed_size,
            flags,
        });
    }

    let mut nodes = Vec::new();
    if has_directory {
        let node_count = read_metadata_count(
            &mut reader,
            "directory nodes",
            limits.max_directory_nodes,
            physical_offset,
            reader_base_offset,
        )?;
        nodes.try_reserve_exact(node_count).map_err(|_| {
            UnityFsError::new(
                physical_offset,
                UnityFsErrorKind::AllocationFailed {
                    resource: "directory table",
                    requested: node_count,
                },
            )
        })?;
        for _ in 0..node_count {
            let node_offset = read_metadata_i64(
                &mut reader,
                physical_offset,
                reader_base_offset,
                "directory node offset",
            )?;
            let node_size = read_metadata_i64(
                &mut reader,
                physical_offset,
                reader_base_offset,
                "directory node size",
            )?;
            if node_offset < 0 || node_size < 0 {
                return Err(metadata_problem(
                    physical_offset,
                    reader.position().saturating_sub(16),
                    UnityFsErrorKind::NegativeDirectoryRange {
                        offset: node_offset,
                        size: node_size,
                    },
                ));
            }
            let flags = read_metadata_u32(
                &mut reader,
                physical_offset,
                reader_base_offset,
                "directory node flags",
            )?;
            let path = read_metadata_string(
                &mut reader,
                limits.max_node_path_bytes,
                physical_offset,
                reader_base_offset,
                "directory node path",
            )?;
            nodes.push(UnityFsNode {
                offset: node_offset as u64,
                size: node_size as u64,
                flags,
                path,
            });
        }
    }

    let bytes_read = reader.position();
    if !metadata_size_accepts(bytes_read, declared_size, reader_base_offset) {
        return Err(metadata_problem(
            physical_offset,
            bytes_read,
            UnityFsErrorKind::MetadataSizeMismatch {
                bytes_read,
                declared_size,
            },
        ));
    }
    Ok(ParsedMetadata {
        uncompressed_data_hash: hash,
        storage_blocks: blocks,
        nodes,
    })
}

/// `FileStreamBundleFile.ReadMetadata` size rule over caller facts: an exact byte count
/// always passes (the declared size need not be a multiple of 4); otherwise the declared
/// size may include up to three alignment bytes, accepted only when both the metadata
/// base and the declared size are 4-aligned and rounding the bytes read up reaches the
/// declared size (AssetRipper issue #1470: no flag marks such bundles).
fn metadata_size_accepts(
    bytes_read: usize,
    declared_size: usize,
    metadata_base_offset: usize,
) -> bool {
    if bytes_read == declared_size {
        return true;
    }
    let aligned_bytes_read = bytes_read.checked_add(3).map(|value| value & !3);
    metadata_base_offset.is_multiple_of(4)
        && declared_size.is_multiple_of(4)
        && aligned_bytes_read == Some(declared_size)
}

fn validate_nodes(
    nodes: &[UnityFsNode],
    uncompressed_data_size: u64,
    metadata_offset: usize,
) -> Result<(), UnityFsError> {
    for (node_index, node) in nodes.iter().enumerate() {
        let end = node.offset.checked_add(node.size).ok_or_else(|| {
            UnityFsError::new(
                metadata_offset,
                UnityFsErrorKind::AddressSpaceOverflow("directory node range"),
            )
        })?;
        if end > uncompressed_data_size {
            return Err(UnityFsError::new(
                metadata_offset,
                UnityFsErrorKind::NodeOutsideUncompressedData {
                    node_index,
                    offset: node.offset,
                    size: node.size,
                    uncompressed_data_size,
                },
            ));
        }
    }
    Ok(())
}

fn decode_payload<'a>(
    input: &'a [u8],
    expected_size: usize,
    maximum_output_size: usize,
    compression: u8,
    input_offset: usize,
    context: &'static str,
    maximum_zstd_window_size: Option<usize>,
) -> Result<Cow<'a, [u8]>, UnityFsError> {
    if expected_size > maximum_output_size {
        return Err(UnityFsError::new(
            input_offset,
            UnityFsErrorKind::LimitExceeded {
                resource: context,
                requested: expected_size as u64,
                limit: maximum_output_size,
            },
        ));
    }
    match compression {
        0 => {
            if input.len() != expected_size {
                return Err(UnityFsError::new(
                    input_offset,
                    UnityFsErrorKind::UncompressedSizeMismatch {
                        context,
                        expected: expected_size,
                        actual: input.len(),
                    },
                ));
            }
            Ok(Cow::Borrowed(input))
        }
        1 => decompress_lzma_block(input, expected_size, maximum_output_size)
            .map(Cow::Owned)
            .map_err(|error| {
                UnityFsError::new(
                    input_offset.saturating_add(error.input_offset()),
                    UnityFsErrorKind::Lzma { context, error },
                )
            }),
        2 | 3 => decompress_block(input, expected_size, maximum_output_size)
            .map(Cow::Owned)
            .map_err(|error| {
                UnityFsError::new(
                    input_offset.saturating_add(error.input_offset()),
                    UnityFsErrorKind::Lz4 { context, error },
                )
            }),
        compression if compression > 4 && has_frame_magic(input) => {
            match maximum_zstd_window_size {
                Some(maximum_window_size) => decompress_zstd_block(
                    input,
                    expected_size,
                    maximum_output_size,
                    maximum_window_size,
                )
                .map(Cow::Owned)
                .map_err(|error| {
                    UnityFsError::new(input_offset, UnityFsErrorKind::Zstd { context, error })
                }),
                None => Err(UnityFsError::new(
                    input_offset,
                    UnityFsErrorKind::UnsupportedCompression {
                        compression,
                        context,
                    },
                )),
            }
        }
        _ => Err(UnityFsError::new(
            input_offset,
            UnityFsErrorKind::UnsupportedCompression {
                compression,
                context,
            },
        )),
    }
}

fn read_metadata_count(
    reader: &mut BinaryReader<'_>,
    field: &'static str,
    limit: usize,
    physical_offset: usize,
    reader_base_offset: usize,
) -> Result<usize, UnityFsError> {
    let count_offset = reader.offset();
    let count = reader
        .read_i32()
        .map_err(|error| metadata_binary_error(error, physical_offset, reader_base_offset))?;
    if count < 0 {
        return Err(metadata_problem(
            physical_offset,
            count_offset.saturating_sub(reader_base_offset),
            UnityFsErrorKind::NegativeCount { field, count },
        ));
    }
    let count = count as usize;
    if count > limit {
        return Err(metadata_limit_error(
            physical_offset,
            count_offset.saturating_sub(reader_base_offset),
            field,
            count as u64,
            limit,
        ));
    }
    Ok(count)
}

fn read_metadata_u16(
    reader: &mut BinaryReader<'_>,
    physical_offset: usize,
    reader_base_offset: usize,
    field: &'static str,
) -> Result<u16, UnityFsError> {
    reader
        .read_u16()
        .map_err(|error| metadata_binary_error(error, physical_offset, reader_base_offset))
        .map_err(|error| error.with_field_context(field))
}

fn read_metadata_u32(
    reader: &mut BinaryReader<'_>,
    physical_offset: usize,
    reader_base_offset: usize,
    field: &'static str,
) -> Result<u32, UnityFsError> {
    reader
        .read_u32()
        .map_err(|error| metadata_binary_error(error, physical_offset, reader_base_offset))
        .map_err(|error| error.with_field_context(field))
}

fn read_metadata_i64(
    reader: &mut BinaryReader<'_>,
    physical_offset: usize,
    reader_base_offset: usize,
    field: &'static str,
) -> Result<i64, UnityFsError> {
    reader
        .read_i64()
        .map_err(|error| metadata_binary_error(error, physical_offset, reader_base_offset))
        .map_err(|error| error.with_field_context(field))
}

fn read_metadata_string(
    reader: &mut BinaryReader<'_>,
    maximum_length: usize,
    physical_offset: usize,
    reader_base_offset: usize,
    field: &'static str,
) -> Result<String, UnityFsError> {
    let string_offset = reader.offset();
    let value = reader
        .read_null_terminated_utf8(maximum_length)
        .map_err(|error| metadata_binary_error(error, physical_offset, reader_base_offset))
        .map_err(|error| error.with_field_context(field))?;
    let mut result = String::new();
    result.try_reserve_exact(value.len()).map_err(|_| {
        metadata_problem(
            physical_offset,
            string_offset.saturating_sub(reader_base_offset),
            UnityFsErrorKind::AllocationFailed {
                resource: "directory path",
                requested: value.len(),
            },
        )
    })?;
    result.push_str(value);
    Ok(result)
}

fn metadata_binary_error(
    error: BinaryError,
    physical_offset: usize,
    reader_base_offset: usize,
) -> UnityFsError {
    let decoded_offset = error.offset().saturating_sub(reader_base_offset);
    UnityFsError::new(
        physical_offset,
        UnityFsErrorKind::MetadataRead {
            decoded_offset,
            detail: error.to_string(),
        },
    )
}

fn metadata_problem(
    physical_offset: usize,
    decoded_offset: usize,
    kind: UnityFsErrorKind,
) -> UnityFsError {
    UnityFsError::new(
        physical_offset,
        UnityFsErrorKind::MetadataProblem {
            decoded_offset,
            cause: Box::new(kind),
        },
    )
}

fn metadata_limit_error(
    physical_offset: usize,
    decoded_offset: usize,
    resource: &'static str,
    requested: u64,
    limit: usize,
) -> UnityFsError {
    metadata_problem(
        physical_offset,
        decoded_offset,
        UnityFsErrorKind::LimitExceeded {
            resource,
            requested,
            limit,
        },
    )
}

fn read_string(
    reader: &mut BinaryReader<'_>,
    maximum_length: usize,
    context: &'static str,
) -> Result<String, UnityFsError> {
    let value = reader
        .read_null_terminated_utf8(maximum_length)
        .map_err(|error| UnityFsError::from_binary(error, context))?;
    let offset = reader.offset().saturating_sub(value.len() + 1);
    let mut result = String::new();
    result.try_reserve_exact(value.len()).map_err(|_| {
        UnityFsError::new(
            offset,
            UnityFsErrorKind::AllocationFailed {
                resource: context,
                requested: value.len(),
            },
        )
    })?;
    result.push_str(value);
    Ok(result)
}

fn read_i32(reader: &mut BinaryReader<'_>, context: &'static str) -> Result<i32, UnityFsError> {
    reader
        .read_i32()
        .map_err(|error| UnityFsError::from_binary(error, context))
}

fn read_i64(reader: &mut BinaryReader<'_>, context: &'static str) -> Result<i64, UnityFsError> {
    reader
        .read_i64()
        .map_err(|error| UnityFsError::from_binary(error, context))
}

fn read_u32(reader: &mut BinaryReader<'_>, context: &'static str) -> Result<u32, UnityFsError> {
    reader
        .read_u32()
        .map_err(|error| UnityFsError::from_binary(error, context))
}

fn nonnegative_u64(value: i64, offset: usize, field: &'static str) -> Result<u64, UnityFsError> {
    u64::try_from(value)
        .map_err(|_| UnityFsError::new(offset, UnityFsErrorKind::NegativeValue { field, value }))
}

fn nonnegative_u32(value: i32, offset: usize, field: &'static str) -> Result<u32, UnityFsError> {
    u32::try_from(value).map_err(|_| {
        UnityFsError::new(
            offset,
            UnityFsErrorKind::NegativeValue {
                field,
                value: i64::from(value),
            },
        )
    })
}

fn align_offset(offset: usize, alignment: usize) -> Option<usize> {
    let mask = alignment.checked_sub(1)?;
    offset.checked_add(mask).map(|value| value & !mask)
}

#[derive(Debug)]
struct ParsedMetadata {
    uncompressed_data_hash: [u8; 16],
    storage_blocks: Vec<UnityFsStorageBlock>,
    nodes: Vec<UnityFsNode>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnityFsError {
    offset: usize,
    kind: UnityFsErrorKind,
}

impl UnityFsError {
    fn new(offset: usize, kind: UnityFsErrorKind) -> Self {
        Self { offset, kind }
    }

    fn from_binary(error: BinaryError, context: &'static str) -> Self {
        let offset = error.offset();
        Self::new(
            offset,
            UnityFsErrorKind::Binary {
                context,
                detail: error.to_string(),
            },
        )
    }

    fn with_field_context(self, field: &'static str) -> Self {
        Self::new(
            self.offset,
            UnityFsErrorKind::FieldRead {
                field,
                detail: self.kind.to_string(),
            },
        )
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn kind(&self) -> &UnityFsErrorKind {
        &self.kind
    }
}

impl Display for UnityFsError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte offset {}", self.kind, self.offset)
    }
}

impl Error for UnityFsError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UnityFsErrorKind {
    StreamInput {
        context: &'static str,
        detail: String,
    },
    Binary {
        context: &'static str,
        detail: String,
    },
    FieldRead {
        field: &'static str,
        detail: String,
    },
    InvalidSignature {
        signature: String,
    },
    NegativeValue {
        field: &'static str,
        value: i64,
    },
    AddressSpaceOverflow(&'static str),
    LimitExceeded {
        resource: &'static str,
        requested: u64,
        limit: usize,
    },
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    InvalidMetadataRange {
        metadata_offset: usize,
        metadata_size: u32,
        input_size: usize,
    },
    InvalidDataRange {
        data_offset: usize,
        data_end: usize,
        input_size: usize,
    },
    UnsupportedCompression {
        compression: u8,
        context: &'static str,
    },
    Lz4 {
        context: &'static str,
        error: Lz4BlockError,
    },
    Lzma {
        context: &'static str,
        error: LzmaBlockError,
    },
    Zstd {
        context: &'static str,
        error: ZstdBlockError,
    },
    MetadataRead {
        decoded_offset: usize,
        detail: String,
    },
    MetadataProblem {
        decoded_offset: usize,
        cause: Box<UnityFsErrorKind>,
    },
    NegativeCount {
        field: &'static str,
        count: i32,
    },
    MetadataSizeMismatch {
        bytes_read: usize,
        declared_size: usize,
    },
    UncompressedSizeMismatch {
        context: &'static str,
        expected: usize,
        actual: usize,
    },
    UncompressedBlockSizeMismatch {
        block_index: usize,
        compressed_size: u32,
        uncompressed_size: u32,
    },
    CompressedBlocksExceedData {
        compressed_size: u64,
        available: u64,
    },
    NegativeDirectoryRange {
        offset: i64,
        size: i64,
    },
    NodeOutsideUncompressedData {
        node_index: usize,
        offset: u64,
        size: u64,
        uncompressed_data_size: u64,
    },
    InvalidEntryIndex {
        entry_index: usize,
        entry_count: usize,
    },
    InvalidBlockSlice {
        block_index: usize,
        start: usize,
        end: usize,
        block_size: usize,
    },
    EntryOutputSizeMismatch {
        entry_index: usize,
        expected: usize,
        actual: usize,
    },
}

impl Display for UnityFsErrorKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::StreamInput { context, detail } => write!(f, "{context}: {detail}"),
            Self::Binary { context, detail } => write!(f, "{context}: {detail}"),
            Self::FieldRead { field, detail } => write!(f, "{field}: {detail}"),
            Self::InvalidSignature { signature } => {
                write!(f, "expected UnityFS signature, found {signature:?}")
            }
            Self::NegativeValue { field, value } => {
                write!(f, "{field} has negative value {value}")
            }
            Self::AddressSpaceOverflow(field) => write!(f, "{field} overflows the address space"),
            Self::LimitExceeded {
                resource,
                requested,
                limit,
            } => write!(f, "{resource} size {requested} exceeds configured limit {limit}"),
            Self::AllocationFailed { resource, requested } => {
                write!(f, "could not allocate {requested} entries/bytes for {resource}")
            }
            Self::InvalidMetadataRange {
                metadata_offset,
                metadata_size,
                input_size,
            } => write!(
                f,
                "metadata range {metadata_offset}..{} exceeds input size {input_size}",
                metadata_offset.saturating_add(*metadata_size as usize)
            ),
            Self::InvalidDataRange {
                data_offset,
                data_end,
                input_size,
            } => write!(
                f,
                "block data range {data_offset}..{data_end} is invalid for input size {input_size}"
            ),
            Self::UnsupportedCompression { compression, context } => write!(
                f,
                "compression type {compression} is not implemented for {context}"
            ),
            Self::Lz4 { context, error } => write!(f, "LZ4 {context}: {error}"),
            Self::Lzma { context, error } => write!(f, "LZMA {context}: {error}"),
            Self::Zstd { context, error } => write!(f, "Zstandard {context}: {error}"),
            Self::MetadataRead {
                decoded_offset,
                detail,
            } => write!(f, "blocks metadata decoded offset {decoded_offset}: {detail}"),
            Self::MetadataProblem {
                decoded_offset,
                cause,
            } => write!(f, "blocks metadata decoded offset {decoded_offset}: {cause}"),
            Self::NegativeCount { field, count } => {
                write!(f, "{field} count is negative ({count})")
            }
            Self::MetadataSizeMismatch {
                bytes_read,
                declared_size,
            } => write!(
                f,
                "parsed {bytes_read} metadata bytes, declared size is {declared_size}"
            ),
            Self::UncompressedSizeMismatch {
                context,
                expected,
                actual,
            } => write!(
                f,
                "uncompressed {context} has {actual} bytes, expected {expected}"
            ),
            Self::UncompressedBlockSizeMismatch {
                block_index,
                compressed_size,
                uncompressed_size,
            } => write!(
                f,
                "uncompressed block {block_index} declares compressed size {compressed_size} and output size {uncompressed_size}"
            ),
            Self::CompressedBlocksExceedData {
                compressed_size,
                available,
            } => write!(
                f,
                "storage blocks require {compressed_size} compressed bytes, only {available} are available"
            ),
            Self::NegativeDirectoryRange { offset, size } => {
                write!(f, "directory entry has negative offset {offset} or size {size}")
            }
            Self::NodeOutsideUncompressedData {
                node_index,
                offset,
                size,
                uncompressed_data_size,
            } => write!(
                f,
                "directory entry {node_index} range {offset}..{} exceeds uncompressed data size {uncompressed_data_size}",
                offset.saturating_add(*size)
            ),
            Self::InvalidEntryIndex {
                entry_index,
                entry_count,
            } => write!(
                f,
                "directory entry index {entry_index} is outside {entry_count} entries"
            ),
            Self::InvalidBlockSlice {
                block_index,
                start,
                end,
                block_size,
            } => write!(
                f,
                "entry slice {start}..{end} is invalid for block {block_index} of size {block_size}"
            ),
            Self::EntryOutputSizeMismatch {
                entry_index,
                expected,
                actual,
            } => write!(
                f,
                "entry {entry_index} produced {actual} bytes, expected {expected}"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    include!("unityfs_tests_01.rs");
}
