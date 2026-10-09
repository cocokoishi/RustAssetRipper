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
use crate::io::stream::{SmartStream, SmartStreamType};

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
            max_entry_bytes: 512 * 1024 * 1024,
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
            let stream = self
                .source
                .create_partial(absolute_offset, entry.size)
                .map_err(|error| {
                    stream_input_error(
                        usize::try_from(absolute_offset).unwrap_or(usize::MAX),
                        "UnityFS source-backed entry range",
                        error,
                    )
                })?;
            let source_backed = stream.stream_type() == SmartStreamType::File;
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
                        .create_partial(part_offset, overlap_length)
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
                        .create_partial(block_absolute_offset, compressed_size)
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
    use super::{
        compression_type_name, UnityFsBundle, UnityFsErrorKind, UnityFsLimits, UnityFsNode,
        UnityFsStreamBundle, BUNDLE_VERSION_LARGE_FILES_SUPPORT, NODE_SERIALIZED_FILE_FLAG,
    };
    use crate::compression::wrappers::compress_gzip;
    use crate::io::filesystem::VirtualFileSystem;
    use crate::io::stream::{SmartStream, SmartStreamType};
    use crate::scheme::{
        BatchMountLimits, ContainerMountErrorKind, ContainerMountLimits, FileKind, ParsedFile,
        SchemeReader, SchemeReaderLimits, WrapperKind,
    };

    fn push_i32(output: &mut Vec<u8>, value: i32) {
        output.extend_from_slice(&value.to_be_bytes());
    }

    fn push_i64(output: &mut Vec<u8>, value: i64) {
        output.extend_from_slice(&value.to_be_bytes());
    }

    fn push_u32(output: &mut Vec<u8>, value: u32) {
        output.extend_from_slice(&value.to_be_bytes());
    }

    fn push_cstring(output: &mut Vec<u8>, value: &str) {
        output.extend_from_slice(value.as_bytes());
        output.push(0);
    }

    fn lz4_literal_block(bytes: &[u8]) -> Vec<u8> {
        let mut result = Vec::new();
        result.push((bytes.len().min(15) as u8) << 4);
        if bytes.len() >= 15 {
            let mut remainder = bytes.len() - 15;
            while remainder >= 255 {
                result.push(255);
                remainder -= 255;
            }
            result.push(remainder as u8);
        }
        result.extend_from_slice(bytes);
        result
    }

    fn lzma_raw_block(bytes: &[u8]) -> Vec<u8> {
        let mut framed = Vec::new();
        lzma_rs::lzma_compress(&mut std::io::Cursor::new(bytes), &mut framed).unwrap();
        assert!(framed.len() >= 13);
        let mut raw = framed[..5].to_vec();
        raw.extend_from_slice(&framed[13..]);
        raw
    }

    fn zstd_raw_block(bytes: &[u8]) -> Vec<u8> {
        assert!(bytes.len() <= 255);
        let mut frame = vec![0x28, 0xb5, 0x2f, 0xfd, 0x20, bytes.len() as u8];
        let block_header = ((bytes.len() as u32) << 3) | 1;
        frame.extend_from_slice(&block_header.to_le_bytes()[..3]);
        frame.extend_from_slice(bytes);
        frame
    }

    fn zstd_rle_block(bytes: &[u8]) -> Vec<u8> {
        assert!(!bytes.is_empty() && bytes.len() <= 255);
        assert!(bytes.iter().all(|byte| *byte == bytes[0]));
        let mut frame = vec![0x28, 0xb5, 0x2f, 0xfd, 0x20, bytes.len() as u8];
        let block_header = ((bytes.len() as u32) << 3) | 3;
        frame.extend_from_slice(&block_header.to_le_bytes()[..3]);
        frame.push(bytes[0]);
        frame
    }

    fn make_bundle(
        entry_bytes: &[u8],
        metadata_compression: u8,
        split_data_block: bool,
        flags_extra: u32,
    ) -> Vec<u8> {
        make_bundle_variant(
            entry_bytes,
            metadata_compression,
            split_data_block,
            6,
            flags_extra,
            false,
        )
    }

    fn make_bundle_variant(
        entry_bytes: &[u8],
        metadata_compression: u8,
        split_data_block: bool,
        version: i32,
        flags_extra: u32,
        metadata_at_end: bool,
    ) -> Vec<u8> {
        make_bundle_with_block_compression(
            entry_bytes,
            metadata_compression,
            0,
            split_data_block,
            version,
            flags_extra,
            metadata_at_end,
        )
    }

    fn make_bundle_with_block_compression(
        entry_bytes: &[u8],
        metadata_compression: u8,
        block_compression: u8,
        split_data_block: bool,
        version: i32,
        flags_extra: u32,
        metadata_at_end: bool,
    ) -> Vec<u8> {
        let (first_data, second_data) = if split_data_block {
            (&entry_bytes[..2], &entry_bytes[2..])
        } else {
            (entry_bytes, &[][..])
        };
        let first_compressed = match block_compression {
            0 => first_data.to_vec(),
            1 => lzma_raw_block(first_data),
            2 | 3 => lz4_literal_block(first_data),
            4 | 5 => zstd_raw_block(first_data),
            6 => zstd_rle_block(first_data),
            other => panic!("test helper does not encode compression type {other}"),
        };
        let second_compressed = if split_data_block {
            lz4_literal_block(second_data)
        } else {
            Vec::new()
        };

        let mut metadata = vec![0x5a; 16];
        push_i32(&mut metadata, if split_data_block { 2 } else { 1 });
        metadata.extend_from_slice(&(first_data.len() as u32).to_be_bytes());
        metadata.extend_from_slice(&(first_compressed.len() as u32).to_be_bytes());
        metadata.extend_from_slice(&(block_compression as u16).to_be_bytes());
        if split_data_block {
            metadata.extend_from_slice(&(second_data.len() as u32).to_be_bytes());
            metadata.extend_from_slice(&(second_compressed.len() as u32).to_be_bytes());
            metadata.extend_from_slice(&2u16.to_be_bytes());
        }
        push_i32(&mut metadata, 1);
        push_i64(&mut metadata, 0);
        push_i64(&mut metadata, entry_bytes.len() as i64);
        push_i32(&mut metadata, 4);
        push_cstring(&mut metadata, "data/file.bin");

        let encoded_metadata = match metadata_compression {
            0 => metadata.clone(),
            1 => lzma_raw_block(&metadata),
            2 | 3 => lz4_literal_block(&metadata),
            4 | 5 => zstd_raw_block(&metadata),
            other => panic!("test helper does not encode compression type {other}"),
        };
        let mut header = Vec::new();
        push_cstring(&mut header, "UnityFS");
        push_i32(&mut header, version);
        push_cstring(&mut header, "2022.3.1f1");
        push_cstring(&mut header, "2022.3.1f1");
        let data_start = header.len() + 8 + 4 + 4 + 4;
        let mut body = Vec::new();
        if version >= BUNDLE_VERSION_LARGE_FILES_SUPPORT {
            let padding = (16 - (data_start & 15)) & 15;
            body.resize(padding, 0);
        }
        if metadata_at_end {
            body.extend_from_slice(&first_compressed);
            body.extend_from_slice(&second_compressed);
            body.extend_from_slice(&encoded_metadata);
        } else {
            body.extend_from_slice(&encoded_metadata);
            let after_metadata = data_start + body.len();
            if flags_extra & 0x200 != 0 {
                let padding = (16 - (after_metadata & 15)) & 15;
                body.resize(body.len() + padding, 0);
            }
            body.extend_from_slice(&first_compressed);
            body.extend_from_slice(&second_compressed);
        }
        let total_size = data_start + body.len();
        push_i64(&mut header, total_size as i64);
        push_i32(&mut header, encoded_metadata.len() as i32);
        push_i32(&mut header, metadata.len() as i32);
        let at_end_flag = if metadata_at_end { 0x80 } else { 0 };
        push_u32(
            &mut header,
            (metadata_compression as u32) | 0x40 | flags_extra | at_end_flag,
        );
        header.extend_from_slice(&body);
        header
    }

    /// A metadata-at-start bundle whose flags omit the combined-directory bit: the
    /// baseline `ReadMetadata` then reads blocks only, and the directory stays empty.
    fn make_bare_bundle(entry_bytes: &[u8]) -> Vec<u8> {
        let mut metadata = vec![0x5a; 16];
        push_i32(&mut metadata, 1);
        metadata.extend_from_slice(&(entry_bytes.len() as u32).to_be_bytes());
        metadata.extend_from_slice(&(entry_bytes.len() as u32).to_be_bytes());
        metadata.extend_from_slice(&0u16.to_be_bytes());
        let mut header = Vec::new();
        push_cstring(&mut header, "UnityFS");
        push_i32(&mut header, 6);
        push_cstring(&mut header, "2022.3.1f1");
        push_cstring(&mut header, "2022.3.1f1");
        let data_start = header.len() + 8 + 4 + 4 + 4;
        push_i64(
            &mut header,
            (data_start + metadata.len() + entry_bytes.len()) as i64,
        );
        push_i32(&mut header, metadata.len() as i32);
        push_i32(&mut header, metadata.len() as i32);
        push_u32(&mut header, 0);
        header.extend_from_slice(&metadata);
        header.extend_from_slice(entry_bytes);
        header
    }

    /// The metadata-placement branches the other fixtures never take: a directory-less
    /// bundle parses to an empty node table, and a pre-alignment version with
    /// blocks-info-at-end skips the 16-byte data alignment.
    #[test]
    fn parses_bare_metadata_and_unaligned_end_placement() {
        let input = make_bare_bundle(b"bare entry");
        let bundle = UnityFsBundle::parse(&input).unwrap();
        assert_eq!(bundle.uncompressed_data_hash, [0x5a; 16]);
        assert_eq!(bundle.storage_blocks.len(), 1);
        assert!(bundle.nodes.is_empty());
        assert_eq!(bundle.uncompressed_data_size, 10);
        let error = bundle.read_entry(0).unwrap_err();
        assert!(matches!(
            error.kind(),
            UnityFsErrorKind::InvalidEntryIndex {
                entry_index: 0,
                entry_count: 0
            }
        ));

        let input = make_bundle_variant(b"entry data", 0, false, 6, 0, true);
        let bundle = UnityFsBundle::parse(&input).unwrap();
        assert_eq!(bundle.header.version, 6);
        assert_eq!(bundle.data_offset, bundle.header.header_size);
        assert_eq!(
            bundle.metadata_offset + bundle.header.compressed_blocks_info_size as usize,
            input.len()
        );
        assert_eq!(bundle.read_entry(0).unwrap().as_ref(), b"entry data");
    }

    #[test]
    fn parses_uncompressed_unityfs_metadata_and_borrows_single_block_entry() {
        let input = make_bundle(b"sample asset bytes", 0, false, 0);
        let bundle = UnityFsBundle::parse(&input).unwrap();
        assert_eq!(bundle.header.version, 6);
        assert_eq!(bundle.header.unity_version, "2022.3.1f1");
        assert_eq!(bundle.storage_blocks.len(), 1);
        assert_eq!(bundle.nodes[0].path, "data/file.bin");
        assert_eq!(
            bundle.read_entry(0).unwrap().as_ref(),
            b"sample asset bytes"
        );
        assert!(matches!(
            bundle.read_entry(0).unwrap(),
            std::borrow::Cow::Borrowed(_)
        ));
    }

    #[test]
    fn streamed_unityfs_reader_keeps_single_uncompressed_entry_file_backed() {
        use std::sync::atomic::{AtomicU64, Ordering};

        static NEXT_FIXTURE_ID: AtomicU64 = AtomicU64::new(0);
        let input = make_bundle(b"source-backed entry bytes", 0, false, 0);
        let path = crate::test_support::test_temp_root().join(format!(
            "unityfs-stream-entry-{}-{}.bundle",
            std::process::id(),
            NEXT_FIXTURE_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, &input).unwrap();

        let source = SmartStream::open_read(&path).unwrap();
        let bundle =
            UnityFsStreamBundle::parse(&source, UnityFsLimits::default(), 1024 * 1024).unwrap();
        assert_eq!(bundle.nodes[0].path, "data/file.bin");
        let entry = bundle.read_entry(0).unwrap();
        assert_eq!(entry.stream_type(), SmartStreamType::File);
        assert_eq!(entry.to_vec(1024).unwrap(), b"source-backed entry bytes");

        drop(entry);
        drop(bundle);
        drop(source);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn production_mount_paths_retain_file_backing_for_uncompressed_unityfs_leaves() {
        let row = include_str!("unityfs_stream_entry_source_fixture.txt")
            .lines()
            .find(|line| line.starts_with("UNITYFS_ENTRY_STREAM|case=uncompressed|"))
            .expect("source UnityFS stream row");
        let field = |name: &str| {
            row.split('|')
                .find_map(|part| part.strip_prefix(name))
                .unwrap_or_else(|| panic!("missing source row field {name}"))
        };
        let decode_hex = |hex: &str| {
            assert_eq!(hex.len() % 2, 0, "odd source hex length");
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect::<Vec<_>>()
        };
        let input = decode_hex(field("input="));
        let payload = decode_hex(field("entry="));
        assert_eq!(input.len().to_string(), field("input_length="));
        assert_eq!(payload.len().to_string(), field("entry_length="));
        assert_eq!(field("resource_name="), "data/file.bin");
        assert_eq!(field("backing_stream="), "RandomAccessStream");
        assert_eq!(field("stream_type="), "THROW:InvalidOperationException");
        assert_eq!(field("resources="), "1");
        assert_eq!(field("failed="), "0");

        let directory = crate::test_support::unique_test_directory("unityfs-stream-mount");
        let path = directory.join("input.bundle");
        std::fs::write(&path, &input).unwrap();

        let mut path_filesystem = VirtualFileSystem::new();
        let path_report = SchemeReader::mount_path(
            &path,
            &mut path_filesystem,
            Some("/path-mount"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
        )
        .unwrap();
        assert_eq!(path_report.mounted_entries, 1);
        assert_eq!(path_report.mounted_bytes, payload.len() as u64);
        assert_eq!(path_report.resource_files.len(), 1);
        let path_leaf = "/path-mount/data/file.bin";
        let path_stream = path_filesystem.open_file(path_leaf).unwrap();
        assert_eq!(path_stream.stream_type(), SmartStreamType::File);
        assert_eq!(path_stream.to_vec(1024).unwrap(), payload.as_slice());

        let mut directory_filesystem = VirtualFileSystem::new();
        let directory_report = SchemeReader::mount_directory(
            &directory,
            &mut directory_filesystem,
            Some("/directory-mount"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
            BatchMountLimits::default(),
        )
        .unwrap();
        assert_eq!(directory_report.discovered_inputs, 1);
        assert_eq!(directory_report.completed_inputs, 1);
        assert_eq!(directory_report.mounted_entries, 1);
        assert_eq!(directory_report.mounted_bytes, payload.len() as u64);
        assert_eq!(directory_report.container_trees.len(), 1);
        assert_eq!(directory_report.container_trees[0].resource_files.len(), 1);
        let directory_leaf = "/directory-mount/input.bundle/data/file.bin";
        let directory_stream = directory_filesystem.open_file(directory_leaf).unwrap();
        assert_eq!(directory_stream.stream_type(), SmartStreamType::File);
        assert_eq!(directory_stream.to_vec(1024).unwrap(), payload.as_slice());

        let empty_row = include_str!("unityfs_stream_entry_source_fixture.txt")
            .lines()
            .find(|line| line.starts_with("UNITYFS_ENTRY_STREAM|case=empty|"))
            .expect("source UnityFS empty-entry row");
        let empty_field = |name: &str| {
            empty_row
                .split('|')
                .find_map(|part| part.strip_prefix(name))
                .unwrap_or_else(|| panic!("missing empty-entry source row field {name}"))
        };
        let empty_input = decode_hex(empty_field("input="));
        assert_eq!(empty_field("entry_length="), "0");
        assert_eq!(empty_field("backing_stream="), "RandomAccessStream");
        assert_eq!(
            empty_field("stream_type="),
            "THROW:InvalidOperationException"
        );
        let empty_path = directory.join("empty.bundle");
        std::fs::write(&empty_path, empty_input).unwrap();
        let mut empty_filesystem = VirtualFileSystem::new();
        let empty_report = SchemeReader::mount_path(
            &empty_path,
            &mut empty_filesystem,
            Some("/empty-mount"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
        )
        .unwrap();
        assert_eq!(empty_report.mounted_entries, 1);
        assert_eq!(empty_report.mounted_bytes, 0);
        let empty_stream = empty_filesystem
            .open_file("/empty-mount/data/file.bin")
            .unwrap();
        assert_eq!(empty_stream.stream_type(), SmartStreamType::File);
        assert!(empty_stream.to_vec(0).unwrap().is_empty());

        drop(path_stream);
        drop(path_filesystem);
        drop(directory_stream);
        drop(directory_filesystem);
        drop(empty_stream);
        drop(empty_filesystem);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(empty_path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn production_stream_mount_decodes_source_lz4_and_multiblock_entries() {
        let fixture = include_str!("unityfs_stream_entry_source_fixture.txt");
        let decode_hex = |hex: &str| {
            assert_eq!(hex.len() % 2, 0, "odd source hex length");
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect::<Vec<_>>()
        };

        for case_name in ["lz4", "multiblock"] {
            let row = fixture
                .lines()
                .find(|line| line.starts_with(&format!("UNITYFS_ENTRY_STREAM|case={case_name}|")))
                .unwrap_or_else(|| panic!("missing source UnityFS {case_name} row"));
            let field = |name: &str| {
                row.split('|')
                    .find_map(|part| part.strip_prefix(name))
                    .unwrap_or_else(|| panic!("missing {case_name} source row field {name}"))
            };
            let input = decode_hex(field("input="));
            let expected = decode_hex(field("entry="));
            assert_eq!(input.len().to_string(), field("input_length="));
            assert_eq!(expected.len().to_string(), field("entry_length="));
            assert_eq!(field("resource_name="), "data/file.bin");
            assert_eq!(field("backing_stream="), "MemoryStream");
            assert_eq!(field("stream_type="), "Memory");
            assert_eq!(field("resources="), "1");
            assert_eq!(field("failed="), "0");

            let path = crate::test_support::unique_test_path(
                &format!("unityfs-stream-{case_name}"),
                "bundle",
            );
            std::fs::write(&path, &input).unwrap();
            let mut filesystem = VirtualFileSystem::new();
            let destination = format!("/source-{case_name}");
            let report = SchemeReader::mount_path(
                &path,
                &mut filesystem,
                Some(&destination),
                SchemeReaderLimits::default(),
                ContainerMountLimits::default(),
            )
            .unwrap();
            assert_eq!(report.mounted_entries, 1, "{case_name} mounted entries");
            assert_eq!(
                report.mounted_bytes,
                expected.len() as u64,
                "{case_name} mounted bytes"
            );
            let leaf_path = format!("{destination}/data/file.bin");
            let leaf = filesystem.open_file(&leaf_path).unwrap();
            assert_eq!(
                leaf.stream_type(),
                SmartStreamType::Memory,
                "{case_name} stream"
            );
            assert_eq!(leaf.to_vec(1024).unwrap(), expected, "{case_name} payload");

            drop(leaf);
            drop(filesystem);
            std::fs::remove_file(path).unwrap();
        }
    }

    #[test]
    fn streamed_mount_applies_nested_owned_byte_limit_to_materialized_children() {
        let payload = b"nested payload";
        let inner = make_bundle(payload, 0, false, 0);
        let direct_outer = make_bundle(&inner, 0, false, 0);
        let compressed_outer = make_bundle_with_block_compression(&inner, 0, 2, false, 6, 0, false);
        let directory = crate::test_support::unique_test_directory("unityfs-stream-nested-limit");
        let direct_path = directory.join("direct.bundle");
        let compressed_path = directory.join("compressed.bundle");
        std::fs::write(&direct_path, &direct_outer).unwrap();
        std::fs::write(&compressed_path, &compressed_outer).unwrap();
        let limits = ContainerMountLimits {
            max_active_nested_bytes: 0,
            ..ContainerMountLimits::default()
        };

        let mut direct_filesystem = VirtualFileSystem::new();
        let direct_report = SchemeReader::mount_path(
            &direct_path,
            &mut direct_filesystem,
            Some("/direct"),
            SchemeReaderLimits::default(),
            limits,
        )
        .unwrap();
        assert_eq!(direct_report.mounted_entries, 1);
        let direct_leaf = direct_filesystem
            .read_all_bytes("/direct/data/file.bin/data/file.bin", 1024)
            .unwrap();
        assert_eq!(direct_leaf, payload);
        drop(direct_filesystem);

        let mut compressed_filesystem = VirtualFileSystem::new();
        let error = SchemeReader::mount_path(
            &compressed_path,
            &mut compressed_filesystem,
            Some("/compressed"),
            SchemeReaderLimits::default(),
            limits,
        )
        .unwrap_err();
        assert!(matches!(
            &error.kind,
            ContainerMountErrorKind::ActiveNestedContainerLimitExceeded {
                requested_total,
                limit: 0,
                ..
            } if *requested_total == inner.len() as u64
        ));
        drop(compressed_filesystem);

        std::fs::remove_file(direct_path).unwrap();
        std::fs::remove_file(compressed_path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn streamed_mount_preserves_source_nested_container_topology() {
        let input =
            include_bytes!("../export/managed_reference_project_fixture/nested_reference.bundle");
        let directory = crate::test_support::unique_test_directory("unityfs-stream-topology");
        let path = directory.join("workspace.bundle");
        std::fs::write(&path, input).unwrap();

        let dispatched = SchemeReader::read(input).unwrap();
        let mut slice_filesystem = VirtualFileSystem::new();
        let slice_report = dispatched
            .mount_container_tree_with_source_name(
                &mut slice_filesystem,
                Some("/source"),
                Some("workspace.bundle"),
                ContainerMountLimits::default(),
            )
            .unwrap();

        let mut stream_filesystem = VirtualFileSystem::new();
        let stream_report = SchemeReader::mount_path(
            &path,
            &mut stream_filesystem,
            Some("/source"),
            SchemeReaderLimits::default(),
            ContainerMountLimits::default(),
        )
        .unwrap();
        assert_eq!(
            stream_report.container_scopes,
            slice_report.container_scopes
        );
        assert_eq!(
            stream_report.container_bundles,
            slice_report.container_bundles
        );
        assert_eq!(
            stream_report.serialized_files,
            slice_report.serialized_files
        );
        assert_eq!(stream_report.resource_files, slice_report.resource_files);
        assert_eq!(stream_report.failed_entries, slice_report.failed_entries);
        assert_eq!(stream_report.mounted_entries, slice_report.mounted_entries);
        assert_eq!(stream_report.mounted_bytes, slice_report.mounted_bytes);

        drop(stream_filesystem);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn streamed_unityfs_reader_decodes_compressed_blocks_across_boundaries() {
        for compression in [1, 2, 5] {
            let input = make_bundle_with_block_compression(
                b"abcdefghijklmnop",
                2,
                compression,
                true,
                6,
                0,
                false,
            );
            let source = SmartStream::from_memory(input);
            let bundle =
                UnityFsStreamBundle::parse(&source, UnityFsLimits::default(), 1024 * 1024).unwrap();
            let entry = bundle.read_entry(0).unwrap();
            assert_eq!(entry.to_vec(1024).unwrap(), b"abcdefghijklmnop");
        }
    }

    /// Uncompressed metadata with tolerated 4-byte-alignment slack (the source
    /// `#1470` case): the slack pad is accepted but is not entry data, so block
    /// data still starts after the declared size and the entry reads intact.
    /// Continuing after the consumed bytes instead (as the source fall-through
    /// does) would shift the entry by the slack.
    #[test]
    fn uncompressed_metadata_slack_starts_data_after_declared_size() {
        let entry = b"slack entry payload";
        let mut slack = make_bundle_variant(entry, 0, false, 7, 0, false);
        // The v7 fixture header is 54 bytes plus 10 alignment bytes, so metadata
        // starts at 64; its struct is 68 bytes ending with the `data/file.bin`
        // NUL. Growing the path by one byte makes the struct 69 bytes; three pad
        // bytes bring the region to the declared 72, which the alignment
        // tolerance accepts: (69 + 3) & !3 == 72.
        const METADATA_OFFSET: usize = 64;
        slack.insert(METADATA_OFFSET + 67, b'1');
        slack.splice(METADATA_OFFSET + 69..METADATA_OFFSET + 69, [0, 0, 0]);
        let total_size = i64::from_be_bytes(slack[34..42].try_into().unwrap()) + 4;
        slack[34..42].copy_from_slice(&total_size.to_be_bytes());
        let compressed = i32::from_be_bytes(slack[42..46].try_into().unwrap()) + 4;
        slack[42..46].copy_from_slice(&compressed.to_be_bytes());
        let uncompressed = i32::from_be_bytes(slack[46..50].try_into().unwrap()) + 4;
        slack[46..50].copy_from_slice(&uncompressed.to_be_bytes());

        let bundle = UnityFsBundle::parse(&slack).unwrap();
        assert_eq!(bundle.metadata_offset, METADATA_OFFSET);
        assert_eq!(bundle.data_offset, METADATA_OFFSET + 72);
        assert_eq!(bundle.nodes[0].path, "data/file.bin1");
        assert_eq!(bundle.read_entry(0).unwrap().as_ref(), entry);
    }

    /// The source accepts the four-byte metadata slack but resumes at the number of
    /// bytes consumed by its metadata structs, before the padding. This replay uses a
    /// separately built source-probe input and verifies the production dispatcher
    /// honors the declared metadata region and returns the intact resource bytes.
    #[test]
    fn production_dispatch_skips_reference_metadata_slack_before_entry_data() {
        let fixture = include_str!("unityfs_alignment_slack_fixture.txt");
        let field = |prefix: &str| {
            fixture
                .lines()
                .find_map(|line| line.strip_prefix(prefix))
                .unwrap_or_else(|| panic!("missing fixture field {prefix}"))
        };
        let decode_hex = |hex: &str| {
            assert_eq!(hex.len() % 2, 0);
            hex.as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect::<Vec<_>>()
        };

        let input = decode_hex(field("INPUT_HEX|"));
        let source_tree = field("SOURCE_TREE|");
        let source_bytes = decode_hex(field("SOURCE_ENTRY_HEX|"));
        let expected_bytes = decode_hex(field("RUST_EXPECTED_ENTRY_HEX|"));
        assert!(source_tree.ends_with(&format!(
            "RES:data/file.bin1:{}",
            field("SOURCE_ENTRY_HEX|")
        )));
        assert_eq!(source_bytes, b"\0\0\0slack entry payl");
        assert_eq!(expected_bytes, b"slack entry payload");

        let parsed = UnityFsBundle::parse(&input).unwrap();
        assert_eq!(parsed.header.version, 7);
        assert_eq!(parsed.metadata_offset, 64);
        assert_eq!(parsed.header.uncompressed_blocks_info_size, 72);
        assert_eq!(parsed.data_offset, 136);
        assert_eq!(parsed.read_entry(0).unwrap().as_ref(), expected_bytes);

        let dispatched = SchemeReader::read(&input).unwrap();
        assert_eq!(dispatched.kind(), FileKind::UnityFs);
        let entry = dispatched.dispatch_entry(0).unwrap();
        assert_eq!(entry.kind(), FileKind::Resource);
        assert_eq!(entry.bytes(), expected_bytes);
        assert_ne!(entry.bytes(), source_bytes);
    }

    #[test]
    fn mounts_unityfs_entries_using_the_parsed_node_paths() {
        let payload = b"UnityFS direct mount payload";
        let input = make_bundle(payload, 0, false, 0);
        let dispatched = SchemeReader::read(&input).unwrap();
        let mut filesystem = VirtualFileSystem::new();
        let report = dispatched
            .mount_container_entries(
                &mut filesystem,
                Some("/bundles/unityfs"),
                ContainerMountLimits::default(),
            )
            .unwrap();

        assert_eq!(report.mounted_entries, 1);
        assert_eq!(report.mounted_bytes, payload.len() as u64);
        assert_eq!(
            filesystem
                .read_all_bytes("/bundles/unityfs/data/file.bin", 128)
                .unwrap(),
            payload
        );
    }

    #[test]
    fn recursively_mounts_nested_unityfs_entries() {
        let inner = make_bundle(b"nested UnityFS leaf", 0, false, 0);
        let outer = make_bundle(&inner, 0, false, 0);
        let dispatched = SchemeReader::read(&outer).unwrap();
        let mut filesystem = VirtualFileSystem::new();
        let report = dispatched
            .mount_container_tree(
                &mut filesystem,
                Some("/bundles/tree"),
                ContainerMountLimits::default(),
            )
            .unwrap();

        assert_eq!(report.mounted_entries, 1);
        assert_eq!(report.mounted_bytes, b"nested UnityFS leaf".len() as u64);
        assert_eq!(
            filesystem
                .read_all_bytes("/bundles/tree/data/file.bin/data/file.bin", 128,)
                .unwrap(),
            b"nested UnityFS leaf"
        );
    }

    #[test]
    fn recursively_dispatches_unityfs_entries() {
        let nested = compress_gzip(b"nested UnityFS entry", 1024).unwrap();
        let input = make_bundle(&nested, 0, false, 0);
        let bundle = SchemeReader::read(&input).unwrap();
        assert_eq!(bundle.kind(), FileKind::UnityFs);

        let entry = bundle.dispatch_entry(0).unwrap();
        assert_eq!(entry.kind(), FileKind::Resource);
        assert_eq!(entry.bytes(), b"nested UnityFS entry");
        assert_eq!(entry.wrappers()[0].kind, WrapperKind::Gzip);
        assert!(matches!(entry.parse().unwrap(), ParsedFile::Resource(_)));
    }

    #[test]
    fn decodes_lz4_metadata_and_reassembles_entries_across_storage_blocks() {
        let input = make_bundle(b"abcdefghijklmnop", 2, true, 0);
        let bundle = UnityFsBundle::parse(&input).unwrap();
        assert_eq!(bundle.storage_blocks.len(), 2);
        assert_eq!(bundle.read_entry(0).unwrap().as_ref(), b"abcdefghijklmnop");
    }

    #[test]
    fn decodes_lzma_metadata_and_storage_blocks() {
        let input =
            make_bundle_with_block_compression(b"lzma entry payload", 1, 1, false, 6, 0, false);
        let bundle = UnityFsBundle::parse(&input).unwrap();
        assert_eq!(
            bundle.read_entry(0).unwrap().as_ref(),
            b"lzma entry payload"
        );
    }

    #[test]
    fn decodes_zstd_storage_blocks_by_frame_signature() {
        let entry_bytes = vec![b'Z'; 64];
        let input = make_bundle_with_block_compression(&entry_bytes, 0, 6, false, 6, 0, false);
        let bundle = UnityFsBundle::parse(&input).unwrap();
        assert_eq!(bundle.read_entry(0).unwrap().as_ref(), entry_bytes);
    }

    #[test]
    fn zstd_signature_does_not_override_metadata_or_lzham_dispatch() {
        let metadata = make_bundle(b"entry", 5, false, 0);
        let metadata_error = UnityFsBundle::parse(&metadata).unwrap_err();
        assert!(matches!(
            metadata_error.kind(),
            UnityFsErrorKind::UnsupportedCompression {
                compression: 5,
                context: "blocks metadata"
            }
        ));

        let lzham = make_bundle_with_block_compression(b"entry", 0, 4, false, 6, 0, false);
        let bundle = UnityFsBundle::parse(&lzham).unwrap();
        let lzham_error = bundle.read_entry(0).unwrap_err();
        assert!(matches!(
            lzham_error.kind(),
            UnityFsErrorKind::UnsupportedCompression {
                compression: 4,
                context: "storage block"
            }
        ));
    }

    #[test]
    fn handles_generation_seven_alignment_and_blocks_info_at_end() {
        let input = make_bundle_variant(b"entry data", 2, false, 7, 0, true);
        let bundle = UnityFsBundle::parse(&input).unwrap();
        assert_eq!(bundle.header.version, 7);
        assert_eq!(bundle.data_offset % 16, 0);
        assert_eq!(
            bundle.metadata_offset + bundle.header.compressed_blocks_info_size as usize,
            input.len()
        );
        assert_eq!(bundle.read_entry(0).unwrap().as_ref(), b"entry data");
    }

    #[test]
    fn honors_padding_flag_before_block_data() {
        let input = make_bundle_variant(b"entry data", 0, false, 6, 0x200, false);
        let bundle = UnityFsBundle::parse(&input).unwrap();
        assert_eq!(bundle.data_offset % 16, 0);
        assert_eq!(bundle.read_entry(0).unwrap().as_ref(), b"entry data");
    }

    #[test]
    fn validates_directory_ranges_and_configured_metadata_bounds() {
        let input = make_bundle(b"short", 0, false, 0);
        let mut malformed = input.clone();
        let parsed = UnityFsBundle::parse(&input).unwrap();
        let entry_offset_field = parsed.metadata_offset
            + parsed.header.compressed_blocks_info_size as usize
            - 8
            - 4
            - "data/file.bin".len()
            - 1;
        malformed[entry_offset_field..entry_offset_field + 8]
            .copy_from_slice(&100i64.to_be_bytes());
        let error = UnityFsBundle::parse(&malformed).unwrap_err();
        assert!(matches!(
            error.kind(),
            UnityFsErrorKind::NodeOutsideUncompressedData { .. }
        ));

        let limits = UnityFsLimits {
            max_metadata_uncompressed_bytes: 1,
            ..UnityFsLimits::default()
        };
        let error = UnityFsBundle::parse_with_limits(&input, limits).unwrap_err();
        assert!(matches!(
            error.kind(),
            UnityFsErrorKind::LimitExceeded {
                resource: "uncompressed blocks metadata",
                ..
            }
        ));
    }

    #[test]
    fn bundle_flag_and_compression_names_match_the_source_enums() {
        assert_eq!(NODE_SERIALIZED_FILE_FLAG, 4);
        // The named compression discriminants resolve; anything else reads unknown,
        // mirroring the source `default` branch.
        assert_eq!(compression_type_name(0), Some("None"));
        assert_eq!(compression_type_name(1), Some("Lzma"));
        assert_eq!(compression_type_name(2), Some("Lz4"));
        assert_eq!(compression_type_name(3), Some("Lz4HC"));
        assert_eq!(compression_type_name(4), Some("Lzham"));
        assert_eq!(compression_type_name(5), None);
        assert_eq!(compression_type_name(255), None);
    }

    #[test]
    fn directory_node_serialized_file_bit_follows_the_flag_word() {
        let node = UnityFsNode {
            offset: 0,
            size: 8,
            flags: NODE_SERIALIZED_FILE_FLAG,
            path: "level0".to_owned(),
        };
        assert!(node.is_serialized_file());
        // Other flag bits alone do not mark a SerializedFile.
        let other = UnityFsNode {
            flags: 0x40,
            ..node.clone()
        };
        assert!(!other.is_serialized_file());
        let combined = UnityFsNode {
            flags: 0x40 | NODE_SERIALIZED_FILE_FLAG,
            ..node.clone()
        };
        assert!(combined.is_serialized_file());
        let bare = UnityFsNode { flags: 0, ..node };
        assert!(!bare.is_serialized_file());
    }

    #[test]
    fn entry_block_windows_cover_exactly_the_overlap() {
        // Disjoint and merely adjacent ranges contribute nothing.
        assert_eq!(super::entry_block_window(10, 20, 30, 40), None);
        assert_eq!(super::entry_block_window(10, 20, 0, 10), None);
        assert_eq!(super::entry_block_window(10, 20, 20, 30), None);
        // Partial overlaps clip to the shared window.
        assert_eq!(super::entry_block_window(10, 20, 0, 15), Some((10, 15)));
        assert_eq!(super::entry_block_window(10, 20, 15, 30), Some((15, 20)));
        // Containment runs either way.
        assert_eq!(super::entry_block_window(10, 20, 0, 30), Some((10, 20)));
        assert_eq!(super::entry_block_window(10, 20, 12, 18), Some((12, 18)));
        // An empty entry has no window anywhere.
        assert_eq!(super::entry_block_window(10, 10, 0, 30), None);
    }

    #[test]
    fn metadata_size_rule_accepts_exact_and_aligned_slack() {
        // An exact byte count always passes, whatever the alignment.
        assert!(super::metadata_size_accepts(20, 20, 1));
        assert!(super::metadata_size_accepts(20, 20, 4));
        // Up to three alignment bytes pass when both sides are 4-aligned.
        assert!(super::metadata_size_accepts(17, 20, 4));
        assert!(super::metadata_size_accepts(18, 20, 0));
        assert!(super::metadata_size_accepts(19, 20, 8));
        // Anything else mismatches: unaligned base, unaligned declared size,
        // four slack bytes, or an overrun.
        assert!(!super::metadata_size_accepts(17, 20, 1));
        assert!(!super::metadata_size_accepts(17, 21, 4));
        assert!(!super::metadata_size_accepts(16, 20, 4));
        assert!(!super::metadata_size_accepts(21, 20, 4));
    }

    #[test]
    fn bundle_headers_match_reference_generated_bundles() {
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

        let fixture = include_str!("bundle_header_fixture.txt");
        let mut versions: std::collections::HashMap<String, i32> = std::collections::HashMap::new();
        let mut hexes: std::collections::HashMap<String, Vec<u8>> =
            std::collections::HashMap::new();
        let mut reads: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for line in fixture.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            if line.starts_with("BUNDLEVERSION|") {
                let parts: Vec<&str> = line.split('|').collect();
                assert_eq!(parts.len(), 3, "bad BUNDLEVERSION {line}");
                versions.insert(parts[1].to_string(), parts[2].parse().unwrap());
            } else if line.starts_with("BUNDLE_HEX|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad BUNDLE_HEX {line}");
                hexes.insert(parts[1].to_string(), hex_to_bytes(parts[2]));
            } else if line.starts_with("BUNDLE_READ|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad BUNDLE_READ {line}");
                reads.insert(parts[1].to_string(), parts[2].to_string());
            }
        }

        assert_eq!(
            versions.get("BF_LargeFilesSupport"),
            Some(&BUNDLE_VERSION_LARGE_FILES_SUPPORT)
        );
        assert_eq!(versions.get("BF_520_x"), Some(&6));
        assert_eq!(versions.get("BF_2022_2"), Some(&8));

        for ver in ["5", "6", "7", "8"] {
            let bytes = hexes
                .get(ver)
                .unwrap_or_else(|| panic!("missing BUNDLE_HEX {ver}"));
            let dump = reads
                .get(ver)
                .unwrap_or_else(|| panic!("missing BUNDLE_READ {ver}"));
            // Dump form: ver|V=..|Size=..|Comp=..|Uncomp=..|Flags=..|Eng=..|Rev=..|Blocks=..|...
            let head = dump.strip_prefix(&format!("{ver}|")).unwrap_or(dump);
            let parts: Vec<&str> = head.split('|').collect();
            let want_version: i32 = field(&parts, "V").parse().unwrap();
            let want_size: u64 = field(&parts, "Size").parse().unwrap();
            let want_comp: u32 = field(&parts, "Comp").parse().unwrap();
            let want_uncomp: u32 = field(&parts, "Uncomp").parse().unwrap();
            let want_flags: u32 = field(&parts, "Flags").parse().unwrap();
            let want_eng = field(&parts, "Eng");
            let want_rev = field(&parts, "Rev");
            let want_blocks: usize = field(&parts, "Blocks").parse().unwrap();
            let want_nodes: usize = field(&parts, "Nodes").parse().unwrap();
            let want_total: usize = field(&parts, "Total").parse().unwrap();
            let want_head_len: usize = field(&parts, "HeadLen").parse().unwrap();
            let want_pad: usize = field(&parts, "Pad").parse().unwrap();

            assert_eq!(bytes.len(), want_total, "v{ver} total length");
            let bundle = UnityFsBundle::parse(bytes)
                .unwrap_or_else(|e| panic!("v{ver} Rust parse failed: {e:?}"));
            assert_eq!(bundle.header.version, want_version, "v{ver} version");
            assert_eq!(bundle.header.declared_size, want_size, "v{ver} size");
            assert_eq!(
                bundle.header.compressed_blocks_info_size, want_comp,
                "v{ver} comp"
            );
            assert_eq!(
                bundle.header.uncompressed_blocks_info_size, want_uncomp,
                "v{ver} uncomp"
            );
            assert_eq!(bundle.header.flags, want_flags, "v{ver} flags");
            assert_eq!(bundle.header.unity_version, want_eng, "v{ver} engine");
            assert_eq!(bundle.header.unity_revision, want_rev, "v{ver} revision");
            assert_eq!(bundle.header.header_size, want_head_len, "v{ver} head len");
            assert_eq!(bundle.storage_blocks.len(), want_blocks, "v{ver} blocks");
            assert_eq!(bundle.nodes.len(), want_nodes, "v{ver} nodes");
            assert_eq!(bundle.uncompressed_data_hash, [0; 16], "v{ver} hash");
            assert_eq!(
                bundle.metadata_offset,
                want_head_len + want_pad,
                "v{ver} metadata offset"
            );
            assert_eq!(bundle.data_offset, want_total, "v{ver} data offset");
            if want_version >= BUNDLE_VERSION_LARGE_FILES_SUPPORT {
                assert_eq!(
                    bundle.metadata_offset % 16,
                    0,
                    "v{ver} v7+ metadata alignment"
                );
                // Versions 7 and 8 share the large-files alignment rule.
                assert!(want_pad < 16, "v{ver} pad range");
            } else {
                assert_eq!(want_pad, 0, "v{ver} pre-7 has no pad");
                assert_eq!(
                    bundle.metadata_offset, bundle.header.header_size,
                    "v{ver} pre-7 metadata follows header"
                );
            }
        }
    }

    #[test]
    fn metadata_tables_match_reference_constructed_bundles() {
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

        // Probe Temp/opencode/unityfsmetaprobe (FileStreamBundleFile.Read over
        // C#-constructed full bundles, SDK 10.0.401; LZ4 via K4os, LZMA via
        // LzmaCompression.CompressLzmaStream): A None combined 1x1 start v6,
        // B None separate 1x0 start v6, C LZ4 combined, D LZMA combined,
        // E LZHAM unsupported, F at-end v6, G at-end v7, H truncated.
        // Nothing here was produced by the Rust writer.
        let fixture = include_str!("unityfs_meta_fixture.txt");
        let mut hexes: std::collections::HashMap<String, Vec<u8>> =
            std::collections::HashMap::new();
        let mut reads: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for line in fixture.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            if line.starts_with("META_HEX|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad META_HEX {line}");
                hexes.insert(parts[1].to_string(), hex_to_bytes(parts[2]));
            } else if line.starts_with("META_READ|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad META_READ {line}");
                reads.insert(parts[1].to_string(), parts[2].to_string());
            }
        }

        // Baseline refuses LZHAM metadata; Rust must fail loud, never succeed.
        let e_dump = reads.get("E").expect("E dump");
        assert!(
            e_dump.contains("UnsupportedBundleDecompression"),
            "E refusal {e_dump}"
        );
        let e_bytes = hexes.get("E").expect("E hex");
        assert!(UnityFsBundle::parse(e_bytes).is_err(), "E Rust fails loud");

        // Truncated metadata fails loud on both sides (different messages,
        // both loud, no silent success).
        let h_dump = reads.get("H").expect("H dump");
        assert!(h_dump.starts_with("THROW:"), "H refusal {h_dump}");
        let h_bytes = hexes.get("H").expect("H hex");
        assert!(UnityFsBundle::parse(h_bytes).is_err(), "H Rust fails loud");

        // All decodable lanes reproduce blocks, nodes, header, and offset.
        for case in ["A", "B", "C", "D", "F", "G"] {
            let bytes = hexes.get(case).unwrap_or_else(|| panic!("missing {case}"));
            let dump = reads.get(case).unwrap_or_else(|| panic!("missing {case}"));
            let parts: Vec<&str> = dump.split('|').collect();
            let want_version: i32 = field(&parts, "V").parse().unwrap();
            let want_size: u64 = field(&parts, "Size").parse().unwrap();
            let want_comp: u32 = field(&parts, "Comp").parse().unwrap();
            let want_uncomp: u32 = field(&parts, "Uncomp").parse().unwrap();
            let want_flags: u32 = field(&parts, "Flags").parse().unwrap();
            let want_blocks: usize = field(&parts, "Blocks").parse().unwrap();
            let want_nodes: usize = field(&parts, "Nodes").parse().unwrap();
            let want_total: usize = field(&parts, "Total").parse().unwrap();

            assert_eq!(bytes.len(), want_total, "{case} total length");
            let bundle = UnityFsBundle::parse(bytes)
                .unwrap_or_else(|e| panic!("{case} Rust parse failed: {e:?}"));
            assert_eq!(bundle.header.version, want_version, "{case} version");
            assert_eq!(bundle.header.declared_size, want_size, "{case} size");
            assert_eq!(
                bundle.header.compressed_blocks_info_size, want_comp,
                "{case} comp"
            );
            assert_eq!(
                bundle.header.uncompressed_blocks_info_size, want_uncomp,
                "{case} uncomp"
            );
            assert_eq!(bundle.header.flags, want_flags, "{case} flags");
            assert_eq!(bundle.storage_blocks.len(), want_blocks, "{case} blocks");
            assert_eq!(bundle.nodes.len(), want_nodes, "{case} nodes");
            assert_eq!(want_blocks, 1, "{case} one block");
            let block = &bundle.storage_blocks[0];
            assert_eq!(block.uncompressed_size, 10, "{case} block uncomp");
            assert_eq!(block.compressed_size, 10, "{case} block comp");
            assert_eq!(block.compression_type(), 0, "{case} block None");
            if case == "B" {
                assert_eq!(want_nodes, 0, "B separate has no directory");
            } else {
                assert_eq!(want_nodes, 1, "{case} one node");
                let node = &bundle.nodes[0];
                assert_eq!(node.offset, 0, "{case} node offset");
                assert_eq!(node.size, 10, "{case} node size");
                assert_eq!(node.path, "data/file.bin", "{case} node path");
            }
            // Placement: start cases follow the header; end cases sit at
            // Size minus the compressed metadata length.
            if case == "F" || case == "G" {
                assert_eq!(
                    bundle.metadata_offset as u64,
                    want_size - u64::from(want_comp),
                    "{case} metadata at end"
                );
            } else {
                assert_eq!(
                    bundle.metadata_offset, bundle.header.header_size,
                    "{case} metadata follows header"
                );
            }
        }
    }

    #[test]
    fn entry_bytes_match_reference_constructed_bundles() {
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

        // Probe Temp/opencode/unityfsentryprobe (FileStreamBundleFile.Read with
        // ResourceFiles/FailedFiles over C#-constructed bundles, SDK 10.0.401;
        // LZ4 via K4os, LZMA via LzmaCompression, Zstd via ZstdSharp with
        // Huffman/FSE for D): A None, B LZ4, C LZMA, D Zstd, E reassembly,
        // F non-combined empty, G LZHAM unsupported (Failed, loud).
        let fixture = include_str!("unityfs_entry_fixture.txt");
        let mut hexes: std::collections::HashMap<String, Vec<u8>> =
            std::collections::HashMap::new();
        let mut reads: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for line in fixture.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            if line.starts_with("ENTRY_HEX|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad ENTRY_HEX {line}");
                hexes.insert(parts[1].to_string(), hex_to_bytes(parts[2]));
            } else if line.starts_with("ENTRY_READ|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad ENTRY_READ {line}");
                reads.insert(parts[1].to_string(), parts[2].to_string());
            }
        }

        // Decodable lanes reproduce entry bytes on both sides.
        for case in ["A", "B", "C", "D", "E"] {
            let bytes = hexes.get(case).unwrap_or_else(|| panic!("missing {case}"));
            let dump = reads.get(case).unwrap_or_else(|| panic!("missing {case}"));
            let file_hex = dump
                .split('|')
                .find_map(|p| p.strip_prefix("FILE:data/file.bin="))
                .unwrap_or_else(|| panic!("{case} no FILE row in {dump}"));
            let expected = hex_to_bytes(file_hex);
            let bundle = UnityFsBundle::parse(bytes)
                .unwrap_or_else(|e| panic!("{case} Rust parse failed: {e:?}"));
            assert_eq!(bundle.nodes.len(), 1, "{case} one node");
            let entry = bundle
                .read_entry(0)
                .unwrap_or_else(|e| panic!("{case} Rust entry failed: {e:?}"));
            assert_eq!(&entry[..], &expected[..], "{case} entry bytes");
        }

        // Non-combined directories carry zero entries on both sides.
        {
            let bytes = hexes.get("F").expect("F hex");
            let dump = reads.get("F").expect("F dump");
            assert!(dump.contains("Nodes=0"), "F no nodes {dump}");
            assert!(dump.contains("Res=0"), "F no resources {dump}");
            let bundle = UnityFsBundle::parse(bytes).expect("F Rust parse");
            assert!(bundle.nodes.is_empty(), "F Rust no nodes");
        }

        // LZHAM entries stay loud failures on both sides, never silent.
        {
            let bytes = hexes.get("G").expect("G hex");
            let dump = reads.get("G").expect("G dump");
            assert!(dump.contains("Failed=1"), "G reference failed {dump}");
            let bundle = UnityFsBundle::parse(bytes).expect("G Rust parse");
            assert_eq!(bundle.nodes.len(), 1, "G one node");
            assert!(bundle.read_entry(0).is_err(), "G Rust fails loud");
        }
    }
}
