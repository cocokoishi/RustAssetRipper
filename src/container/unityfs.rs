use std::borrow::Cow;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::compression::lz4_block::{decompress_block, Lz4BlockError};
use crate::compression::lzma::{decompress_block as decompress_lzma_block, LzmaBlockError};
use crate::compression::zstd::{
    decompress_block as decompress_zstd_block, has_frame_magic, ZstdBlockError,
};
use crate::io::binary::{BinaryError, BinaryReader, Endian};

const UNITY_FS_MAGIC: &str = "UnityFS";
const BLOCK_INFO_AT_END: u32 = 0x80;
const BLOCK_INFO_NEEDS_PADDING: u32 = 0x200;
const BLOCKS_AND_DIRECTORY_COMBINED: u32 = 0x40;
const COMPRESSION_MASK: u32 = 0x3f;
const BLOCK_COMPRESSION_MASK: u16 = 0x3f;

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

        let blocks_at_end = flags & BLOCK_INFO_AT_END != 0;
        if version >= 7 {
            reader.align(16).map_err(|error| {
                UnityFsError::from_binary(error, "UnityFS v7+ header alignment")
            })?;
        }

        let metadata_offset = if blocks_at_end {
            let offset = declared_size
                .checked_sub(u64::from(compressed_blocks_info_size))
                .ok_or_else(|| {
                    UnityFsError::new(
                        declared_size_offset,
                        UnityFsErrorKind::InvalidMetadataRange {
                            metadata_offset: 0,
                            metadata_size: compressed_blocks_info_size,
                            input_size: input.len(),
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
            reader.position()
        };
        let metadata_end = metadata_offset
            .checked_add(compressed_blocks_info_size as usize)
            .ok_or_else(|| {
                UnityFsError::new(
                    metadata_offset,
                    UnityFsErrorKind::AddressSpaceOverflow("metadata range"),
                )
            })?;
        let compressed_metadata = input.get(metadata_offset..metadata_end).ok_or_else(|| {
            UnityFsError::new(
                metadata_offset,
                UnityFsErrorKind::InvalidMetadataRange {
                    metadata_offset,
                    metadata_size: compressed_blocks_info_size,
                    input_size: input.len(),
                },
            )
        })?;
        if metadata_offset < header_size || (!blocks_at_end && metadata_offset < reader.position())
        {
            return Err(UnityFsError::new(
                metadata_offset,
                UnityFsErrorKind::InvalidMetadataRange {
                    metadata_offset,
                    metadata_size: compressed_blocks_info_size,
                    input_size: input.len(),
                },
            ));
        }

        let metadata_limit = limits.max_metadata_uncompressed_bytes;
        let uncompressed_metadata_size =
            usize::try_from(uncompressed_blocks_info_size).map_err(|_| {
                UnityFsError::new(
                    metadata_offset,
                    UnityFsErrorKind::AddressSpaceOverflow("uncompressed metadata size"),
                )
            })?;
        if uncompressed_metadata_size > metadata_limit {
            return Err(UnityFsError::new(
                metadata_offset,
                UnityFsErrorKind::LimitExceeded {
                    resource: "uncompressed blocks metadata",
                    requested: uncompressed_metadata_size as u64,
                    limit: metadata_limit,
                },
            ));
        }
        let metadata_compression = (flags & COMPRESSION_MASK) as u8;
        let decoded_metadata = decode_payload(
            compressed_metadata,
            uncompressed_metadata_size,
            metadata_limit,
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
            flags & BLOCKS_AND_DIRECTORY_COMBINED != 0,
            &limits,
        )?;

        let mut data_offset = if blocks_at_end {
            header_size
        } else {
            metadata_end
        };
        if blocks_at_end && version >= 7 {
            data_offset = align_offset(data_offset, 16).ok_or_else(|| {
                UnityFsError::new(
                    data_offset,
                    UnityFsErrorKind::AddressSpaceOverflow("data alignment"),
                )
            })?;
        }
        if flags & BLOCK_INFO_NEEDS_PADDING != 0 {
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
            input.len()
        };
        if data_offset > data_end {
            return Err(UnityFsError::new(
                data_offset,
                UnityFsErrorKind::InvalidDataRange {
                    data_offset,
                    data_end,
                    input_size: input.len(),
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

        Ok(Self {
            input,
            limits,
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
            uncompressed_data_hash: metadata.uncompressed_data_hash,
            storage_blocks: metadata.storage_blocks,
            nodes: metadata.nodes,
            metadata_offset,
            data_offset,
            uncompressed_data_size,
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
            let overlap_start = entry_start.max(block_uncompressed_offset);
            let overlap_end = entry_end.min(block_end);
            if overlap_start < overlap_end {
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
    if bytes_read != declared_size {
        let aligned_bytes_read = bytes_read.checked_add(3).map(|value| value & !3);
        if !(reader_base_offset.is_multiple_of(4)
            && declared_size.is_multiple_of(4)
            && aligned_bytes_read == Some(declared_size))
        {
            return Err(metadata_problem(
                physical_offset,
                bytes_read,
                UnityFsErrorKind::MetadataSizeMismatch {
                    bytes_read,
                    declared_size,
                },
            ));
        }
    }
    Ok(ParsedMetadata {
        uncompressed_data_hash: hash,
        storage_blocks: blocks,
        nodes,
    })
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
    use super::{UnityFsBundle, UnityFsErrorKind, UnityFsLimits};

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
        if version >= 7 {
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
}
