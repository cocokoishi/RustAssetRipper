use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::io::binary::{BinaryError, BinaryReader};

use super::format_version::FormatVersion;
use super::header::{HeaderError, SerializedFileHeader};
use super::metadata_prefix::{MetadataPrefixError, SerializedFileMetadataPrefix};

/// The metadata tables following the SerializedFile metadata prefix.
///
/// Object records retain offsets and sizes into the data section. Object payloads are not
/// copied here; they remain addressable through the original input and their checked ranges.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SerializedFileMetadata {
    pub prefix: SerializedFileMetadataPrefix,
    pub types: Vec<SerializedType>,
    /// Present for generations 7 through 13. The reference reader passes this field to
    /// `ObjectInfo.Read`, but that method selects the PathID width from the generation.
    pub long_file_id: Option<u32>,
    pub objects: Vec<ObjectInfo>,
    pub script_types: Vec<LocalSerializedObjectIdentifier>,
    pub externals: Vec<FileIdentifier>,
    pub reference_types: Vec<SerializedTypeReference>,
    pub user_information: Option<String>,
    /// Bytes after the last known metadata field, retained for forward compatibility.
    pub trailing_metadata: Vec<u8>,
    /// Number of bytes consumed from the start of the metadata region, including the prefix.
    pub consumed_bytes: usize,
}

impl SerializedFileMetadata {
    pub fn parse_file(
        file_bytes: &[u8],
        header: &SerializedFileHeader,
    ) -> Result<Self, SerializedFileMetadataError> {
        Self::parse_file_inner(file_bytes, header, false).map(|(metadata, _)| metadata)
    }

    pub(super) fn parse_file_with_rewrite_layouts(
        file_bytes: &[u8],
        header: &SerializedFileHeader,
    ) -> Result<(Self, SerializedFileMetadataRewriteLayouts), SerializedFileMetadataError> {
        let (metadata, layouts) = Self::parse_file_inner(file_bytes, header, true)?;
        let layouts = layouts.ok_or_else(|| {
            SerializedFileMetadataError::new(
                metadata.prefix.metadata_offset,
                SerializedFileMetadataErrorKind::LayoutOverflow("missing tracked rewrite layouts"),
            )
        })?;
        Ok((metadata, layouts))
    }

    fn parse_file_inner(
        file_bytes: &[u8],
        header: &SerializedFileHeader,
        collect_rewrite_layouts: bool,
    ) -> Result<(Self, Option<SerializedFileMetadataRewriteLayouts>), SerializedFileMetadataError>
    {
        header
            .validate_for_file_size(file_bytes.len())
            .map_err(SerializedFileMetadataError::from_header)?;

        let metadata_size = usize::try_from(header.metadata_size).map_err(|_| {
            SerializedFileMetadataError::new(
                0,
                SerializedFileMetadataErrorKind::LayoutOverflow("metadata size"),
            )
        })?;
        let metadata_offset = if header.format_version.metadata_at_end() {
            file_bytes.len().checked_sub(metadata_size).ok_or_else(|| {
                SerializedFileMetadataError::new(
                    0,
                    SerializedFileMetadataErrorKind::InvalidMetadataRange {
                        start: 0,
                        end: metadata_size,
                        file_size: file_bytes.len(),
                    },
                )
            })?
        } else {
            header.header_size
        };
        let metadata_end = metadata_offset.checked_add(metadata_size).ok_or_else(|| {
            SerializedFileMetadataError::new(
                metadata_offset,
                SerializedFileMetadataErrorKind::LayoutOverflow("metadata end"),
            )
        })?;
        let metadata_region = file_bytes
            .get(metadata_offset..metadata_end)
            .ok_or_else(|| {
                SerializedFileMetadataError::new(
                    metadata_offset,
                    SerializedFileMetadataErrorKind::InvalidMetadataRange {
                        start: metadata_offset,
                        end: metadata_end,
                        file_size: file_bytes.len(),
                    },
                )
            })?;
        Self::parse_metadata_region_inner(
            metadata_region,
            metadata_offset,
            file_bytes.len(),
            header,
            collect_rewrite_layouts,
        )
    }

    pub(crate) fn parse_metadata_region(
        metadata_region: &[u8],
        metadata_offset: usize,
        actual_file_size: usize,
        header: &SerializedFileHeader,
    ) -> Result<Self, SerializedFileMetadataError> {
        Self::parse_metadata_region_inner(
            metadata_region,
            metadata_offset,
            actual_file_size,
            header,
            false,
        )
        .map(|(metadata, _)| metadata)
    }

    fn parse_metadata_region_inner(
        metadata_region: &[u8],
        metadata_offset: usize,
        actual_file_size: usize,
        header: &SerializedFileHeader,
        collect_rewrite_layouts: bool,
    ) -> Result<(Self, Option<SerializedFileMetadataRewriteLayouts>), SerializedFileMetadataError>
    {
        let prefix = SerializedFileMetadataPrefix::parse_metadata_region_with_limit(
            metadata_region,
            metadata_offset,
            actual_file_size,
            header,
            super::metadata_prefix::DEFAULT_MAX_VERSION_STRING_BYTES,
        )
        .map_err(SerializedFileMetadataError::from_prefix)?;
        let tables_offset = prefix
            .metadata_offset
            .checked_add(prefix.consumed_bytes)
            .ok_or_else(|| {
                SerializedFileMetadataError::new(
                    prefix.metadata_offset,
                    SerializedFileMetadataErrorKind::LayoutOverflow("table offset"),
                )
            })?;
        let tables = metadata_region
            .get(prefix.consumed_bytes..)
            .ok_or_else(|| {
                SerializedFileMetadataError::new(
                    tables_offset,
                    SerializedFileMetadataErrorKind::InvalidMetadataRange {
                        start: tables_offset,
                        end: prefix.metadata_offset.saturating_add(prefix.metadata_size),
                        file_size: actual_file_size,
                    },
                )
            })?;
        let mut reader = BinaryReader::with_base_offset(tables, prefix.body_endian, tables_offset)
            .map_err(SerializedFileMetadataError::from_binary)?;
        let generation = header.format_version;

        let types = read_array(&mut reader, "serialized types", 4, |reader, index| {
            read_serialized_type(
                reader,
                generation,
                prefix.has_type_tree,
                &prefix,
                index,
                false,
            )
            .map(SerializedType::from)
        })?;

        let long_file_id = if generation.has_long_file_id() {
            Some(read_u32(&mut reader, "long FileID flag")?)
        } else {
            None
        };

        let (objects, object_record_layout) = if collect_rewrite_layouts {
            let parsed = read_array_with_sidecar(
                &mut reader,
                "object records",
                4,
                "object record layouts",
                prefix.metadata_offset,
                |reader, index| {
                    read_object_info(
                        reader,
                        generation,
                        &types,
                        header,
                        prefix.metadata_offset,
                        index,
                    )
                },
            )?;
            (
                parsed.items,
                Some(ObjectRecordTableLayout {
                    count_field_offset: parsed.count_field_offset,
                    records_start_offset: parsed.records_start_offset,
                    records_end_offset: parsed.records_end_offset,
                    records: parsed.sidecar,
                }),
            )
        } else {
            let objects = read_array(&mut reader, "object records", 4, |reader, index| {
                read_object_info(
                    reader,
                    generation,
                    &types,
                    header,
                    prefix.metadata_offset,
                    index,
                )
                .map(|(object, _)| object)
            })?;
            (objects, None)
        };

        let script_types = if generation.has_script_types() {
            read_array(
                &mut reader,
                "script type identifiers",
                4,
                |reader, index| read_local_serialized_object_identifier(reader, generation, index),
            )?
        } else {
            Vec::new()
        };

        let (externals, external_file_table_layout) = if collect_rewrite_layouts {
            let array = read_array_with_boundaries(
                &mut reader,
                "external files",
                1,
                prefix.metadata_offset,
                |reader, index| read_file_identifier(reader, generation, index),
            )?;
            (array.items, Some(array.layout))
        } else {
            (
                read_array(&mut reader, "external files", 1, |reader, index| {
                    read_file_identifier(reader, generation, index)
                })?,
                None,
            )
        };

        let reference_types = if generation.has_ref_types() {
            read_array(&mut reader, "reference types", 4, |reader, index| {
                read_serialized_type(
                    reader,
                    generation,
                    prefix.has_type_tree,
                    &prefix,
                    index,
                    true,
                )
                .map(SerializedTypeReference::from)
            })?
        } else {
            Vec::new()
        };

        let user_information = if generation.has_user_information() {
            Some(read_string(&mut reader, "user information")?)
        } else {
            None
        };

        let trailing_metadata = reader
            .read_exact(reader.remaining())
            .map_err(|error| binary_error(error, "trailing metadata"))?
            .to_vec();
        let consumed_bytes = prefix
            .consumed_bytes
            .checked_add(reader.position())
            .ok_or_else(|| {
                SerializedFileMetadataError::new(
                    tables_offset,
                    SerializedFileMetadataErrorKind::LayoutOverflow("consumed byte count"),
                )
            })?;
        let rewrite_layouts = match (object_record_layout, external_file_table_layout) {
            (Some(object_records), Some(external_files)) => {
                Some(SerializedFileMetadataRewriteLayouts {
                    object_records,
                    external_files,
                })
            }
            (None, None) => None,
            _ => {
                return Err(SerializedFileMetadataError::new(
                    tables_offset,
                    SerializedFileMetadataErrorKind::LayoutOverflow(
                        "incomplete SerializedFile rewrite table layouts",
                    ),
                ));
            }
        };

        Ok((
            Self {
                prefix,
                types,
                long_file_id,
                objects,
                script_types,
                externals,
                reference_types,
                user_information,
                trailing_metadata,
                consumed_bytes,
            },
            rewrite_layouts,
        ))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ObjectRecordLayout {
    pub path_id: i64,
    /// Byte offset relative to the beginning of this SerializedFile's metadata region.
    pub data_offset_field_offset: usize,
    /// Byte offset relative to the beginning of this SerializedFile's metadata region.
    pub data_size_field_offset: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ObjectRecordTableLayout {
    /// Byte offset of the signed record count relative to the metadata region.
    pub count_field_offset: usize,
    /// Byte offset of the first object record relative to the metadata region.
    pub records_start_offset: usize,
    /// Byte offset immediately after the final object record relative to the metadata region.
    pub records_end_offset: usize,
    pub records: Vec<ObjectRecordLayout>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ExternalFileTableLayout {
    pub count_field_offset: usize,
    pub records_start_offset: usize,
    pub records_end_offset: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SerializedFileMetadataRewriteLayouts {
    pub object_records: ObjectRecordTableLayout,
    pub external_files: ExternalFileTableLayout,
}

/// One serialized class/type definition in the metadata table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SerializedType {
    pub raw_type_id: i32,
    /// Generation-7 through generation-16 readers treat negative IDs as the MonoBehaviour
    /// class marker for some version-dependent hash decisions. Preserve both representations.
    pub type_id_for_hash: i32,
    pub is_stripped_type: bool,
    pub script_type_index: i16,
    pub script_id: Option<[u32; 4]>,
    pub old_type_hash: Option<[u32; 4]>,
    pub type_tree: Option<TypeTree>,
    pub type_dependencies: Vec<i32>,
}

impl From<SerializedTypeBase> for SerializedType {
    fn from(value: SerializedTypeBase) -> Self {
        Self {
            raw_type_id: value.raw_type_id,
            type_id_for_hash: value.type_id_for_hash,
            is_stripped_type: value.is_stripped_type,
            script_type_index: value.script_type_index,
            script_id: value.script_id,
            old_type_hash: value.old_type_hash,
            type_tree: value.type_tree,
            type_dependencies: value.type_dependencies,
        }
    }
}

/// A type used by a managed reference field (`[SerializeReference]`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SerializedTypeReference {
    pub raw_type_id: i32,
    pub type_id_for_hash: i32,
    pub is_stripped_type: bool,
    pub script_type_index: i16,
    pub script_id: Option<[u32; 4]>,
    pub old_type_hash: Option<[u32; 4]>,
    pub type_tree: Option<TypeTree>,
    pub class_name: Option<String>,
    pub namespace: Option<String>,
    pub assembly_name: Option<String>,
}

impl From<SerializedTypeBase> for SerializedTypeReference {
    fn from(value: SerializedTypeBase) -> Self {
        Self {
            raw_type_id: value.raw_type_id,
            type_id_for_hash: value.type_id_for_hash,
            is_stripped_type: value.is_stripped_type,
            script_type_index: value.script_type_index,
            script_id: value.script_id,
            old_type_hash: value.old_type_hash,
            type_tree: value.type_tree,
            class_name: value.class_name,
            namespace: value.namespace,
            assembly_name: value.assembly_name,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SerializedTypeBase {
    raw_type_id: i32,
    type_id_for_hash: i32,
    is_stripped_type: bool,
    script_type_index: i16,
    script_id: Option<[u32; 4]>,
    old_type_hash: Option<[u32; 4]>,
    type_tree: Option<TypeTree>,
    type_dependencies: Vec<i32>,
    class_name: Option<String>,
    namespace: Option<String>,
    assembly_name: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeTree {
    pub nodes: Vec<TypeTreeNode>,
    pub string_buffer: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypeTreeNode {
    pub level: u8,
    pub version: i32,
    pub type_flags: i32,
    pub type_string_offset: Option<u32>,
    pub name_string_offset: Option<u32>,
    pub type_name: Option<String>,
    pub name: Option<String>,
    pub byte_size: i32,
    pub index: i32,
    pub meta_flags: u32,
    pub reference_type_hash: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectInfo {
    pub path_id: i64,
    /// Offset relative to `SerializedFileHeader::data_offset`.
    pub byte_start: u64,
    pub byte_size: u32,
    /// Generation 17+ serializes a type-table index; earlier files serialize a raw type ID.
    pub serialized_type_index: Option<i32>,
    pub raw_type_id: i32,
    pub class_id: Option<i16>,
    pub script_type_index: Option<i16>,
    pub is_destroyed: Option<u16>,
    pub is_stripped: Option<bool>,
    pub resolved_type_index: Option<usize>,
}

impl ObjectInfo {
    /// Absolute file range for the object's serialized payload.
    pub fn data_range(&self, header: &SerializedFileHeader) -> Option<std::ops::Range<u64>> {
        let start = header.data_offset.checked_add(self.byte_start)?;
        let end = start.checked_add(u64::from(self.byte_size))?;
        Some(start..end)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalSerializedObjectIdentifier {
    pub local_serialized_file_index: i32,
    pub local_identifier_in_file: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileIdentifier {
    pub asset_path: Option<String>,
    pub guid: Option<[u32; 4]>,
    pub file_type: Option<i32>,
    pub path_name_origin: String,
}

impl FileIdentifier {
    /// Return the dependency lookup name used by the source `FileIdentifier.PathName`.
    /// The serialized origin remains available unchanged in `path_name_origin`.
    pub fn fixed_path_name(&self) -> String {
        crate::io::special_names::fix_file_identifier(&self.path_name_origin)
    }

    /// Return the source `FileIdentifier.GetFilePath` identity used by game-bundle collection
    /// matching. Platform external dependency requests use normalized `PathName` instead.
    /// `AssetType.Meta` uses the Unity GUID words; other file types use the normalized path.
    /// A Meta identifier without its generation-5+ GUID is malformed and returns `None`.
    pub fn get_file_path(&self) -> Option<String> {
        const ASSET_TYPE_META: i32 = 3;

        if self.file_type == Some(ASSET_TYPE_META) {
            self.guid.map(|[data0, data1, data2, data3]| {
                // `FileIdentifier.GetFilePath` returns `UnityGuid.ToString`, which emits each
                // 32-bit word's nibbles least significant first.
                let mut value = String::with_capacity(32);
                for word in [data0, data1, data2, data3] {
                    for shift in 0..8 {
                        let digit = ((word >> (shift * 4)) & 0x0f) as usize;
                        value.push(b"0123456789abcdef"[digit] as char);
                    }
                }
                value
            })
        } else {
            Some(self.fixed_path_name())
        }
    }
}

fn read_serialized_type(
    reader: &mut BinaryReader<'_>,
    generation: FormatVersion,
    has_type_tree: bool,
    prefix: &SerializedFileMetadataPrefix,
    index: usize,
    is_reference_type: bool,
) -> Result<SerializedTypeBase, SerializedFileMetadataError> {
    let context = if is_reference_type {
        format!("reference type {index}")
    } else {
        format!("serialized type {index}")
    };
    let raw_type_id = read_i32(reader, &format!("{context} type ID"))?;
    let type_id_for_hash = if generation.raw() < 16 && raw_type_id < 0 {
        -1
    } else {
        raw_type_id
    };
    let is_stripped_type = if generation.raw() >= 16 {
        read_bool(reader, &format!("{context} stripped flag"))?
    } else {
        false
    };
    let script_type_index = if generation.raw() >= 17 {
        read_i16(reader, &format!("{context} script type index"))?
    } else {
        -1
    };

    let (script_id, old_type_hash) = if generation.raw() >= 13 {
        let script_id = if should_read_type_script_id(
            type_id_for_hash,
            script_type_index,
            is_reference_type,
            prefix.unity_version_text.as_deref(),
        ) {
            Some(read_hash128(reader, &format!("{context} script ID"))?)
        } else {
            None
        };
        let old_type_hash = Some(read_hash128(reader, &format!("{context} type hash"))?);
        (script_id, old_type_hash)
    } else {
        (None, None)
    };

    let type_tree = if has_type_tree {
        Some(read_type_tree(reader, generation, &context)?)
    } else {
        None
    };
    let mut type_dependencies = Vec::new();
    let mut class_name = None;
    let mut namespace = None;
    let mut assembly_name = None;
    if has_type_tree && generation.raw() >= 21 {
        if is_reference_type {
            class_name = Some(read_string(reader, &format!("{context} class name"))?);
            namespace = Some(read_string(reader, &format!("{context} namespace"))?);
            assembly_name = Some(read_string(reader, &format!("{context} assembly name"))?);
        } else {
            type_dependencies = read_array(
                reader,
                &format!("{context} type dependencies"),
                4,
                |reader, dependency_index| {
                    read_i32(
                        reader,
                        &format!("{context} type dependency {dependency_index}"),
                    )
                },
            )?;
        }
    }

    Ok(SerializedTypeBase {
        raw_type_id,
        type_id_for_hash,
        is_stripped_type,
        script_type_index,
        script_id,
        old_type_hash,
        type_tree,
        type_dependencies,
        class_name,
        namespace,
        assembly_name,
    })
}

/// Whether `SerializedTypeBase.Read` reads the `ScriptID` hash for one type entry.
///
/// Ports the `readScriptID` gate (`AssetRipper/Source/AssetRipper.IO.Files/SerializedFiles/Parser/SerializedTypeBase.cs`
/// lines 67-76): MonoBehaviour markers (`-1`, after the pre-`RefactoredClassId`
/// negative-ID clamp that `read_serialized_type` preserves as `type_id_for_hash`) and
/// `MonoScript` (`114`) always carry the hash; otherwise the script type index decides,
/// except pre-`2018.3.0a1` Unity versions ignore script types for plain serialized types
/// (reference types override `IgnoreScriptTypeForHash` to false and always honor a
/// non-negative index).
///
/// Deliberately read-side only: the sibling `Write` path gates on the raw type ID
/// instead of the clamped marker, so the two expressions must not be unified.
pub fn should_read_type_script_id(
    type_id_for_hash: i32,
    script_type_index: i16,
    is_reference_type: bool,
    unity_version: Option<&str>,
) -> bool {
    type_id_for_hash == -1
        || type_id_for_hash == 114
        || (script_type_index >= 0
            && (is_reference_type || !ignore_script_type_for_hash(unity_version)))
}

fn ignore_script_type_for_hash(unity_version: Option<&str>) -> bool {
    let Some(version) = unity_version.and_then(ParsedUnityVersion::parse) else {
        // AssetRipper falls back to UnityVersion 0.0.0 if parsing fails. That value does not
        // satisfy its "older than 2018.3.0a1" test, so the ScriptID hash is read.
        return false;
    };
    version
        < ParsedUnityVersion {
            major: 2018,
            minor: 3,
            patch: 0,
            release_rank: 0,
            release_number: 1,
        }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct ParsedUnityVersion {
    major: u32,
    minor: u32,
    patch: u32,
    release_rank: u8,
    release_number: u32,
}

pub(crate) fn unity_version_aligns_arrays(unity_version: Option<&str>) -> bool {
    ParsedUnityVersion::parse(unity_version.unwrap_or_default())
        .is_some_and(|version| version.major >= 2017)
}

impl ParsedUnityVersion {
    fn parse(value: &str) -> Option<Self> {
        let mut cursor = 0usize;
        let major = parse_version_number(value, &mut cursor)?;
        if value.as_bytes().get(cursor) != Some(&b'.') {
            return None;
        }
        cursor += 1;
        let minor = parse_version_number(value, &mut cursor)?;
        if value.as_bytes().get(cursor) != Some(&b'.') {
            return None;
        }
        cursor += 1;
        let patch = parse_version_number(value, &mut cursor)?;
        let suffix = value.as_bytes().get(cursor..)?;
        let (release_rank, release_number) = if suffix.is_empty() {
            (2, 0) // Treat a bare numeric version as a final release.
        } else {
            let release_rank = match suffix[0] {
                b'a' => 0,
                b'b' => 1,
                b'f' => 2,
                b'p' => 3,
                b'c' => 4,
                b'x' => 5,
                _ => return None,
            };
            let release_number = parse_ascii_decimal(&suffix[1..])?;
            (release_rank, release_number)
        };
        Some(Self {
            major,
            minor,
            patch,
            release_rank,
            release_number,
        })
    }
}

fn parse_version_number(value: &str, cursor: &mut usize) -> Option<u32> {
    let bytes = value.as_bytes();
    let start = *cursor;
    while bytes.get(*cursor).is_some_and(u8::is_ascii_digit) {
        *cursor += 1;
    }
    if start == *cursor {
        return None;
    }
    parse_ascii_decimal(&bytes[start..*cursor])
}

fn parse_ascii_decimal(bytes: &[u8]) -> Option<u32> {
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    bytes.iter().try_fold(0u32, |value, byte| {
        value.checked_mul(10)?.checked_add(u32::from(byte - b'0'))
    })
}

/// Whether a generation uses the string-buffer TypeTree node form.
///
/// Ports `TypeTreeNode.IsFormat5` (`AssetRipper/Source/AssetRipper.IO.Files/SerializedFiles/Parser/TypeTrees/TypeTreeNode.cs`
/// lines 12, 42-43, 72-73): generation 10 or 12+. Generation 11 is the distinct
/// later legacy branch that stays on the null-terminated form.
pub(crate) fn is_format5(generation: FormatVersion) -> bool {
    generation.raw() == 10 || generation.raw() >= 12
}

/// Whether a generation carries the per-node reference-type hash.
///
/// Ports `TypeTreeNode.HasRefTypeHash` (`TypeTreeNode.cs` lines 17, 53-56, 82-85):
/// generation 19+ (`FormatVersion.TypeTreeNodeWithTypeFlags`).
pub(crate) fn has_ref_type_hash(generation: FormatVersion) -> bool {
    generation.raw() >= 19
}

fn read_type_tree(
    reader: &mut BinaryReader<'_>,
    generation: FormatVersion,
    context: &str,
) -> Result<TypeTree, SerializedFileMetadataError> {
    if is_format5(generation) {
        let nodes_count = read_nonnegative_count(reader, &format!("{context} TypeTree nodes"), 24)?;
        let string_buffer_size =
            read_nonnegative_count(reader, &format!("{context} TypeTree string buffer size"), 1)?;
        let node_size = if has_ref_type_hash(generation) {
            32
        } else {
            24
        };
        let encoded_nodes_size = nodes_count.checked_mul(node_size).ok_or_else(|| {
            SerializedFileMetadataError::new(
                reader.offset(),
                SerializedFileMetadataErrorKind::LayoutOverflow("TypeTree nodes"),
            )
        })?;
        let required_size = encoded_nodes_size
            .checked_add(string_buffer_size)
            .ok_or_else(|| {
                SerializedFileMetadataError::new(
                    reader.offset(),
                    SerializedFileMetadataErrorKind::LayoutOverflow("TypeTree storage"),
                )
            })?;
        if required_size > reader.remaining() {
            return Err(SerializedFileMetadataError::new(
                reader.offset(),
                SerializedFileMetadataErrorKind::CountExceedsMetadata {
                    field: format!("{context} TypeTree nodes and string buffer"),
                    count: required_size,
                    remaining: reader.remaining(),
                },
            ));
        }

        let mut nodes = reserve_vec(nodes_count, reader.offset(), "TypeTree nodes")?;
        for node_index in 0..nodes_count {
            nodes.push(read_modern_type_tree_node(
                reader, generation, context, node_index,
            )?);
        }

        let string_buffer = reader
            .read_exact(string_buffer_size)
            .map_err(|error| binary_error(error, &format!("{context} TypeTree string buffer")))?
            .to_vec();
        validate_string_buffer(&string_buffer, reader.offset() - string_buffer_size)?;
        let string_buffer_offset = reader.offset() - string_buffer_size;
        for (node_index, node) in nodes.iter_mut().enumerate() {
            let type_offset = node.type_string_offset.ok_or_else(|| {
                SerializedFileMetadataError::new(
                    string_buffer_offset,
                    SerializedFileMetadataErrorKind::MissingTypeTreeStringOffset,
                )
            })?;
            let name_offset = node.name_string_offset.ok_or_else(|| {
                SerializedFileMetadataError::new(
                    string_buffer_offset,
                    SerializedFileMetadataErrorKind::MissingTypeTreeStringOffset,
                )
            })?;
            node.type_name = Some(resolve_type_tree_string(
                type_offset,
                &string_buffer,
                string_buffer_offset,
                &format!("{context} node {node_index} type name"),
            )?);
            node.name = Some(resolve_type_tree_string(
                name_offset,
                &string_buffer,
                string_buffer_offset,
                &format!("{context} node {node_index} name"),
            )?);
        }
        Ok(TypeTree {
            nodes,
            string_buffer,
        })
    } else {
        let maximum_nodes = reader.remaining() / 26;
        let mut nodes = Vec::new();
        let root = read_legacy_type_tree_node(reader, generation, context, 0, 0)?;
        let root_children = root.1;
        nodes.push(root.0);
        let mut stack = Vec::new();
        if root_children > 0 {
            stack.push((root_children, 1usize));
        }
        while let Some((remaining_children, depth)) = stack.last_mut() {
            if *remaining_children == 0 {
                stack.pop();
                continue;
            }
            *remaining_children -= 1;
            if nodes.len() >= maximum_nodes {
                return Err(SerializedFileMetadataError::new(
                    reader.offset(),
                    SerializedFileMetadataErrorKind::InvalidTypeTreeNodeCount,
                ));
            }
            let level = u8::try_from(*depth).map_err(|_| {
                SerializedFileMetadataError::new(
                    reader.offset(),
                    SerializedFileMetadataErrorKind::TypeTreeDepthOverflow(*depth),
                )
            })?;
            let (node, child_count) =
                read_legacy_type_tree_node(reader, generation, context, nodes.len(), level)?;
            nodes.try_reserve(1).map_err(|_| {
                allocation_error(reader.offset(), "TypeTree nodes", nodes.len() + 1)
            })?;
            nodes.push(node);
            if child_count > 0 {
                if child_count > maximum_nodes.saturating_sub(nodes.len()) {
                    return Err(SerializedFileMetadataError::new(
                        reader.offset(),
                        SerializedFileMetadataErrorKind::InvalidTreeChildCount(child_count),
                    ));
                }
                stack.push((child_count, usize::from(level) + 1));
            }
        }
        Ok(TypeTree {
            nodes,
            string_buffer: Vec::new(),
        })
    }
}

fn read_modern_type_tree_node(
    reader: &mut BinaryReader<'_>,
    generation: FormatVersion,
    context: &str,
    node_index: usize,
) -> Result<TypeTreeNode, SerializedFileMetadataError> {
    // TypeTreeNode.Read writes fields in this exact order: version, level, flags, offsets,
    // size, index, transfer flags, and (generation 19+) reference hash.
    let version = i32::from(read_u16(
        reader,
        &format!("{context} TypeTree node {node_index} version"),
    )?);
    let level = read_u8(
        reader,
        &format!("{context} TypeTree node {node_index} level"),
    )?;
    let type_flags = i32::from(read_u8(
        reader,
        &format!("{context} TypeTree node {node_index} type flags"),
    )?);
    let type_string_offset = read_u32(
        reader,
        &format!("{context} TypeTree node {node_index} type string offset"),
    )?;
    let name_string_offset = read_u32(
        reader,
        &format!("{context} TypeTree node {node_index} name string offset"),
    )?;
    let byte_size = read_i32(
        reader,
        &format!("{context} TypeTree node {node_index} byte size"),
    )?;
    let index = read_i32(
        reader,
        &format!("{context} TypeTree node {node_index} index"),
    )?;
    let meta_flags = read_u32(
        reader,
        &format!("{context} TypeTree node {node_index} meta flags"),
    )?;
    let reference_type_hash = if has_ref_type_hash(generation) {
        Some(read_u64(
            reader,
            &format!("{context} TypeTree node {node_index} reference type hash"),
        )?)
    } else {
        None
    };
    Ok(TypeTreeNode {
        level,
        version,
        type_flags,
        type_string_offset: Some(type_string_offset),
        name_string_offset: Some(name_string_offset),
        type_name: None,
        name: None,
        byte_size,
        index,
        meta_flags,
        reference_type_hash,
    })
}

fn read_legacy_type_tree_node(
    reader: &mut BinaryReader<'_>,
    _generation: FormatVersion,
    context: &str,
    node_index: usize,
    level: u8,
) -> Result<(TypeTreeNode, usize), SerializedFileMetadataError> {
    let type_name = read_string(
        reader,
        &format!("{context} TypeTree node {node_index} type"),
    )?;
    let name = read_string(
        reader,
        &format!("{context} TypeTree node {node_index} name"),
    )?;
    let byte_size = read_i32(
        reader,
        &format!("{context} TypeTree node {node_index} byte size"),
    )?;
    let index = read_i32(
        reader,
        &format!("{context} TypeTree node {node_index} index"),
    )?;
    let type_flags = read_i32(
        reader,
        &format!("{context} TypeTree node {node_index} type flags"),
    )?;
    let version = read_i32(
        reader,
        &format!("{context} TypeTree node {node_index} version"),
    )?;
    let meta_flags = read_u32(
        reader,
        &format!("{context} TypeTree node {node_index} meta flags"),
    )?;
    let child_count = read_nonnegative_count(
        reader,
        &format!("{context} TypeTree node {node_index} children"),
        26,
    )?;
    Ok((
        TypeTreeNode {
            level,
            version,
            type_flags,
            type_string_offset: None,
            name_string_offset: None,
            type_name: Some(type_name),
            name: Some(name),
            byte_size,
            index,
            meta_flags,
            reference_type_hash: None,
        },
        child_count,
    ))
}

fn validate_string_buffer(
    string_buffer: &[u8],
    base_offset: usize,
) -> Result<(), SerializedFileMetadataError> {
    let mut start = 0usize;
    while start < string_buffer.len() {
        let tail = &string_buffer[start..];
        let length = tail.iter().position(|byte| *byte == 0).ok_or_else(|| {
            SerializedFileMetadataError::new(
                base_offset + start,
                SerializedFileMetadataErrorKind::UnterminatedTypeTreeString,
            )
        })?;
        std::str::from_utf8(&tail[..length]).map_err(|error| {
            SerializedFileMetadataError::new(
                base_offset + start + error.valid_up_to(),
                SerializedFileMetadataErrorKind::InvalidTypeTreeString,
            )
        })?;
        start += length + 1;
    }
    Ok(())
}

fn resolve_type_tree_string(
    raw_offset: u32,
    string_buffer: &[u8],
    base_offset: usize,
    context: &str,
) -> Result<String, SerializedFileMetadataError> {
    if raw_offset & 0x8000_0000 != 0 {
        let offset = raw_offset & 0x7fff_ffff;
        return super::common_strings::get(offset)
            .map(str::to_owned)
            .ok_or_else(|| {
                SerializedFileMetadataError::new(
                    base_offset,
                    SerializedFileMetadataErrorKind::UnknownCommonStringOffset {
                        offset,
                        context: context.to_owned(),
                    },
                )
            });
    }
    let offset = raw_offset as usize;
    if offset >= string_buffer.len() || (offset > 0 && string_buffer.get(offset - 1) != Some(&0)) {
        return Err(SerializedFileMetadataError::new(
            base_offset,
            SerializedFileMetadataErrorKind::InvalidLocalStringOffset {
                offset: raw_offset,
                buffer_size: string_buffer.len(),
                context: context.to_owned(),
            },
        ));
    }
    let tail = &string_buffer[offset..];
    let length = tail.iter().position(|byte| *byte == 0).ok_or_else(|| {
        SerializedFileMetadataError::new(
            base_offset + offset,
            SerializedFileMetadataErrorKind::UnterminatedTypeTreeString,
        )
    })?;
    std::str::from_utf8(&tail[..length])
        .map(str::to_owned)
        .map_err(|error| {
            SerializedFileMetadataError::new(
                base_offset + offset + error.valid_up_to(),
                SerializedFileMetadataErrorKind::InvalidTypeTreeString,
            )
        })
}

fn read_object_info(
    reader: &mut BinaryReader<'_>,
    generation: FormatVersion,
    types: &[SerializedType],
    header: &SerializedFileHeader,
    metadata_start: usize,
    index: usize,
) -> Result<(ObjectInfo, ObjectRecordLayout), SerializedFileMetadataError> {
    let context = format!("object record {index}");
    let path_id = if generation.raw() >= 14 {
        reader
            .align(4)
            .map_err(|error| binary_error(error, &format!("{context} PathID alignment")))?;
        read_i64(reader, &format!("{context} PathID"))?
    } else {
        i64::from(read_i32(reader, &format!("{context} PathID"))?)
    };
    let data_offset_field_offset =
        reader.offset().checked_sub(metadata_start).ok_or_else(|| {
            SerializedFileMetadataError::new(
                reader.offset(),
                SerializedFileMetadataErrorKind::LayoutOverflow("object offset field location"),
            )
        })?;
    let byte_start = if generation.has_large_file_support() {
        let value = read_i64(reader, &format!("{context} data offset"))?;
        u64::try_from(value).map_err(|_| {
            SerializedFileMetadataError::new(
                reader.offset() - 8,
                SerializedFileMetadataErrorKind::NegativeObjectOffset(value),
            )
        })?
    } else {
        u64::from(read_u32(reader, &format!("{context} data offset"))?)
    };
    let data_size_field_offset = reader.offset().checked_sub(metadata_start).ok_or_else(|| {
        SerializedFileMetadataError::new(
            reader.offset(),
            SerializedFileMetadataErrorKind::LayoutOverflow("object size field location"),
        )
    })?;
    let raw_byte_size = read_i32(reader, &format!("{context} data size"))?;
    let byte_size = u32::try_from(raw_byte_size).map_err(|_| {
        SerializedFileMetadataError::new(
            reader.offset() - 4,
            SerializedFileMetadataErrorKind::NegativeObjectSize(raw_byte_size),
        )
    })?;
    let range_start = header.data_offset.checked_add(byte_start).ok_or_else(|| {
        SerializedFileMetadataError::new(
            reader.offset(),
            SerializedFileMetadataErrorKind::LayoutOverflow("object data start"),
        )
    })?;
    let range_end = range_start
        .checked_add(u64::from(byte_size))
        .ok_or_else(|| {
            SerializedFileMetadataError::new(
                reader.offset(),
                SerializedFileMetadataErrorKind::LayoutOverflow("object data end"),
            )
        })?;
    if range_end > header.file_size {
        return Err(SerializedFileMetadataError::new(
            reader.offset(),
            SerializedFileMetadataErrorKind::ObjectDataOutOfBounds {
                object_index: index,
                start: range_start,
                end: range_end,
                file_size: header.file_size,
            },
        ));
    }

    let (serialized_type_index, raw_type_id) = if generation.raw() >= 17 {
        (
            Some(read_i32(
                reader,
                &format!("{context} serialized type index"),
            )?),
            0,
        )
    } else {
        (None, read_i32(reader, &format!("{context} type ID"))?)
    };
    let class_id = if generation.raw() < 16 {
        Some(read_i16(reader, &format!("{context} class ID"))?)
    } else {
        None
    };
    let script_type_index = if (11..17).contains(&generation.raw()) {
        Some(read_i16(reader, &format!("{context} script type index"))?)
    } else {
        None
    };
    let is_destroyed = if generation.raw() < 11 {
        Some(read_u16(reader, &format!("{context} destroyed flag"))?)
    } else {
        None
    };
    let is_stripped = if (15..17).contains(&generation.raw()) {
        Some(read_bool(reader, &format!("{context} stripped flag"))?)
    } else {
        None
    };

    let resolved_type_index = if let Some(serialized_type_index) = serialized_type_index {
        if serialized_type_index >= 0 {
            let resolved = usize::try_from(serialized_type_index).map_err(|_| {
                SerializedFileMetadataError::new(
                    reader.offset(),
                    SerializedFileMetadataErrorKind::InvalidTypeIndex {
                        object_index: index,
                        type_index: serialized_type_index,
                        type_count: types.len(),
                    },
                )
            })?;
            if resolved >= types.len() {
                return Err(SerializedFileMetadataError::new(
                    reader.offset(),
                    SerializedFileMetadataErrorKind::InvalidTypeIndex {
                        object_index: index,
                        type_index: serialized_type_index,
                        type_count: types.len(),
                    },
                ));
            }
            Some(resolved)
        } else {
            find_type_index(types, 0, None, reader.offset(), index)?
        }
    } else {
        find_type_index(types, raw_type_id, is_stripped, reader.offset(), index)?
    };

    Ok((
        ObjectInfo {
            path_id,
            byte_start,
            byte_size,
            serialized_type_index,
            raw_type_id: resolved_type_index
                .map(|type_index| types[type_index].raw_type_id)
                .unwrap_or(raw_type_id),
            class_id,
            script_type_index: script_type_index.or_else(|| {
                resolved_type_index.map(|type_index| types[type_index].script_type_index)
            }),
            is_destroyed,
            is_stripped: is_stripped.or_else(|| {
                resolved_type_index.map(|type_index| types[type_index].is_stripped_type)
            }),
            resolved_type_index,
        },
        ObjectRecordLayout {
            path_id,
            data_offset_field_offset,
            data_size_field_offset,
        },
    ))
}

fn find_type_index(
    types: &[SerializedType],
    raw_type_id: i32,
    stripped: Option<bool>,
    offset: usize,
    object_index: usize,
) -> Result<Option<usize>, SerializedFileMetadataError> {
    if types.is_empty() {
        return Ok(None);
    }
    let mut found = None;
    for (index, serialized_type) in types.iter().enumerate() {
        if serialized_type.raw_type_id == raw_type_id
            && stripped.is_none_or(|value| serialized_type.is_stripped_type == value)
        {
            if found.is_some() {
                return Err(SerializedFileMetadataError::new(
                    offset,
                    SerializedFileMetadataErrorKind::AmbiguousObjectType {
                        object_index,
                        raw_type_id,
                        stripped,
                    },
                ));
            }
            found = Some(index);
        }
    }
    if found.is_none() {
        return Err(SerializedFileMetadataError::new(
            offset,
            SerializedFileMetadataErrorKind::ObjectTypeNotFound {
                object_index,
                raw_type_id,
                stripped,
            },
        ));
    }
    Ok(found)
}

fn read_local_serialized_object_identifier(
    reader: &mut BinaryReader<'_>,
    generation: FormatVersion,
    index: usize,
) -> Result<LocalSerializedObjectIdentifier, SerializedFileMetadataError> {
    let context = format!("script type identifier {index}");
    let local_serialized_file_index = read_i32(reader, &format!("{context} file index"))?;
    let local_identifier_in_file = if generation.raw() >= 14 {
        reader
            .align(4)
            .map_err(|error| binary_error(error, &format!("{context} alignment")))?;
        read_i64(reader, &format!("{context} local identifier"))?
    } else {
        i64::from(read_i32(reader, &format!("{context} local identifier"))?)
    };
    Ok(LocalSerializedObjectIdentifier {
        local_serialized_file_index,
        local_identifier_in_file,
    })
}

fn read_file_identifier(
    reader: &mut BinaryReader<'_>,
    generation: FormatVersion,
    index: usize,
) -> Result<FileIdentifier, SerializedFileMetadataError> {
    let context = format!("external file {index}");
    let asset_path = if generation.raw() >= 6 {
        Some(read_string(reader, &format!("{context} asset path"))?)
    } else {
        None
    };
    let (guid, file_type) = if generation.raw() >= 5 {
        (
            Some(read_hash128(reader, &format!("{context} GUID"))?),
            Some(read_i32(reader, &format!("{context} asset type"))?),
        )
    } else {
        (None, None)
    };
    let path_name_origin = read_string(reader, &format!("{context} path name"))?;
    Ok(FileIdentifier {
        asset_path,
        guid,
        file_type,
        path_name_origin,
    })
}

fn read_hash128(
    reader: &mut BinaryReader<'_>,
    context: &str,
) -> Result<[u32; 4], SerializedFileMetadataError> {
    Ok([
        read_u32(reader, &format!("{context} word 0"))?,
        read_u32(reader, &format!("{context} word 1"))?,
        read_u32(reader, &format!("{context} word 2"))?,
        read_u32(reader, &format!("{context} word 3"))?,
    ])
}

fn read_string(
    reader: &mut BinaryReader<'_>,
    context: &str,
) -> Result<String, SerializedFileMetadataError> {
    let maximum_length = reader.remaining();
    reader
        .read_null_terminated_utf8(maximum_length)
        .map(str::to_owned)
        .map_err(|error| binary_error(error, context))
}

fn read_bool(
    reader: &mut BinaryReader<'_>,
    context: &str,
) -> Result<bool, SerializedFileMetadataError> {
    read_u8(reader, context).map(|value| value != 0)
}

fn read_nonnegative_count(
    reader: &mut BinaryReader<'_>,
    field: &str,
    minimum_item_size: usize,
) -> Result<usize, SerializedFileMetadataError> {
    let offset = reader.offset();
    let raw_count = read_i32(reader, &format!("{field} count"))?;
    let count = usize::try_from(raw_count).map_err(|_| {
        SerializedFileMetadataError::new(
            offset,
            SerializedFileMetadataErrorKind::NegativeCount {
                field: field.to_owned(),
                count: raw_count,
            },
        )
    })?;
    if let Some(maximum_count) = reader.remaining().checked_div(minimum_item_size) {
        if count > maximum_count {
            return Err(SerializedFileMetadataError::new(
                offset,
                SerializedFileMetadataErrorKind::CountExceedsMetadata {
                    field: field.to_owned(),
                    count,
                    remaining: reader.remaining(),
                },
            ));
        }
    }
    Ok(count)
}

fn read_array<T>(
    reader: &mut BinaryReader<'_>,
    field: &str,
    minimum_item_size: usize,
    mut read_item: impl FnMut(&mut BinaryReader<'_>, usize) -> Result<T, SerializedFileMetadataError>,
) -> Result<Vec<T>, SerializedFileMetadataError> {
    let count = read_nonnegative_count(reader, field, minimum_item_size)?;
    let mut items = reserve_vec(count, reader.offset(), field)?;
    for index in 0..count {
        items.push(read_item(reader, index)?);
    }
    Ok(items)
}

struct ArrayWithBoundaries<T> {
    items: Vec<T>,
    layout: ExternalFileTableLayout,
}

fn read_array_with_boundaries<T>(
    reader: &mut BinaryReader<'_>,
    field: &str,
    minimum_item_size: usize,
    metadata_start: usize,
    mut read_item: impl FnMut(&mut BinaryReader<'_>, usize) -> Result<T, SerializedFileMetadataError>,
) -> Result<ArrayWithBoundaries<T>, SerializedFileMetadataError> {
    let count_field_offset = reader.offset().checked_sub(metadata_start).ok_or_else(|| {
        SerializedFileMetadataError::new(
            reader.offset(),
            SerializedFileMetadataErrorKind::LayoutOverflow("array count field location"),
        )
    })?;
    let count = read_nonnegative_count(reader, field, minimum_item_size)?;
    let records_start_offset = reader.offset().checked_sub(metadata_start).ok_or_else(|| {
        SerializedFileMetadataError::new(
            reader.offset(),
            SerializedFileMetadataErrorKind::LayoutOverflow("array record table start"),
        )
    })?;
    let mut items = reserve_vec(count, reader.offset(), field)?;
    for index in 0..count {
        items.push(read_item(reader, index)?);
    }
    let records_end_offset = reader.offset().checked_sub(metadata_start).ok_or_else(|| {
        SerializedFileMetadataError::new(
            reader.offset(),
            SerializedFileMetadataErrorKind::LayoutOverflow("array record table end"),
        )
    })?;
    Ok(ArrayWithBoundaries {
        items,
        layout: ExternalFileTableLayout {
            count_field_offset,
            records_start_offset,
            records_end_offset,
        },
    })
}

struct ArrayWithSidecar<T, U> {
    items: Vec<T>,
    sidecar: Vec<U>,
    count_field_offset: usize,
    records_start_offset: usize,
    records_end_offset: usize,
}

fn read_array_with_sidecar<T, U>(
    reader: &mut BinaryReader<'_>,
    field: &str,
    minimum_item_size: usize,
    sidecar_field: &str,
    metadata_start: usize,
    mut read_item: impl FnMut(
        &mut BinaryReader<'_>,
        usize,
    ) -> Result<(T, U), SerializedFileMetadataError>,
) -> Result<ArrayWithSidecar<T, U>, SerializedFileMetadataError> {
    let count_field_offset = reader.offset().checked_sub(metadata_start).ok_or_else(|| {
        SerializedFileMetadataError::new(
            reader.offset(),
            SerializedFileMetadataErrorKind::LayoutOverflow("object count field location"),
        )
    })?;
    let count = read_nonnegative_count(reader, field, minimum_item_size)?;
    let records_start_offset = reader.offset().checked_sub(metadata_start).ok_or_else(|| {
        SerializedFileMetadataError::new(
            reader.offset(),
            SerializedFileMetadataErrorKind::LayoutOverflow("object record table start"),
        )
    })?;
    let mut items = reserve_vec(count, reader.offset(), field)?;
    let mut sidecar = reserve_vec(count, reader.offset(), sidecar_field)?;
    for index in 0..count {
        let (item, associated) = read_item(reader, index)?;
        items.push(item);
        sidecar.push(associated);
    }
    let records_end_offset = reader.offset().checked_sub(metadata_start).ok_or_else(|| {
        SerializedFileMetadataError::new(
            reader.offset(),
            SerializedFileMetadataErrorKind::LayoutOverflow("object record table end"),
        )
    })?;
    Ok(ArrayWithSidecar {
        items,
        sidecar,
        count_field_offset,
        records_start_offset,
        records_end_offset,
    })
}

fn reserve_vec<T>(
    count: usize,
    offset: usize,
    field: &str,
) -> Result<Vec<T>, SerializedFileMetadataError> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| allocation_error(offset, field, count))?;
    Ok(result)
}

fn allocation_error(offset: usize, field: &str, count: usize) -> SerializedFileMetadataError {
    SerializedFileMetadataError::new(
        offset,
        SerializedFileMetadataErrorKind::AllocationFailed {
            field: field.to_owned(),
            count,
        },
    )
}

macro_rules! read_primitive {
    ($name:ident, $ty:ty, $reader_method:ident) => {
        fn $name(
            reader: &mut BinaryReader<'_>,
            context: &str,
        ) -> Result<$ty, SerializedFileMetadataError> {
            reader
                .$reader_method()
                .map_err(|error| binary_error(error, context))
        }
    };
}

read_primitive!(read_u8, u8, read_u8);
read_primitive!(read_u16, u16, read_u16);
read_primitive!(read_i16, i16, read_i16);
read_primitive!(read_u32, u32, read_u32);
read_primitive!(read_i32, i32, read_i32);
read_primitive!(read_u64, u64, read_u64);
read_primitive!(read_i64, i64, read_i64);

fn binary_error(error: BinaryError, context: &str) -> SerializedFileMetadataError {
    let offset = error.offset();
    SerializedFileMetadataError::new(
        offset,
        SerializedFileMetadataErrorKind::Binary(error.with_context(context.to_owned())),
    )
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SerializedFileMetadataError {
    offset: usize,
    kind: SerializedFileMetadataErrorKind,
}

impl SerializedFileMetadataError {
    fn new(offset: usize, kind: SerializedFileMetadataErrorKind) -> Self {
        Self { offset, kind }
    }

    fn from_binary(error: BinaryError) -> Self {
        let offset = error.offset();
        Self::new(offset, SerializedFileMetadataErrorKind::Binary(error))
    }

    fn from_header(error: HeaderError) -> Self {
        let offset = error.offset();
        Self::new(offset, SerializedFileMetadataErrorKind::Header(error))
    }

    fn from_prefix(error: MetadataPrefixError) -> Self {
        let offset = error.offset();
        Self::new(offset, SerializedFileMetadataErrorKind::Prefix(error))
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn kind(&self) -> &SerializedFileMetadataErrorKind {
        &self.kind
    }
}

impl Display for SerializedFileMetadataError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match &self.kind {
            SerializedFileMetadataErrorKind::Binary(error) => Display::fmt(error, f),
            SerializedFileMetadataErrorKind::Header(error) => Display::fmt(error, f),
            SerializedFileMetadataErrorKind::Prefix(error) => Display::fmt(error, f),
            kind => write!(f, "{kind} at byte offset {}", self.offset),
        }
    }
}

impl Error for SerializedFileMetadataError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            SerializedFileMetadataErrorKind::Binary(error) => Some(error),
            SerializedFileMetadataErrorKind::Header(error) => Some(error),
            SerializedFileMetadataErrorKind::Prefix(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SerializedFileMetadataErrorKind {
    Binary(BinaryError),
    Header(HeaderError),
    Prefix(MetadataPrefixError),
    NegativeCount {
        field: String,
        count: i32,
    },
    CountExceedsMetadata {
        field: String,
        count: usize,
        remaining: usize,
    },
    AllocationFailed {
        field: String,
        count: usize,
    },
    InvalidMetadataRange {
        start: usize,
        end: usize,
        file_size: usize,
    },
    LayoutOverflow(&'static str),
    NegativeObjectOffset(i64),
    NegativeObjectSize(i32),
    ObjectDataOutOfBounds {
        object_index: usize,
        start: u64,
        end: u64,
        file_size: u64,
    },
    InvalidTypeIndex {
        object_index: usize,
        type_index: i32,
        type_count: usize,
    },
    ObjectTypeNotFound {
        object_index: usize,
        raw_type_id: i32,
        stripped: Option<bool>,
    },
    AmbiguousObjectType {
        object_index: usize,
        raw_type_id: i32,
        stripped: Option<bool>,
    },
    InvalidTreeChildCount(usize),
    InvalidTypeTreeNodeCount,
    TypeTreeDepthOverflow(usize),
    MissingTypeTreeStringOffset,
    UnterminatedTypeTreeString,
    InvalidTypeTreeString,
    UnknownCommonStringOffset {
        offset: u32,
        context: String,
    },
    InvalidLocalStringOffset {
        offset: u32,
        buffer_size: usize,
        context: String,
    },
}

impl Display for SerializedFileMetadataErrorKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Binary(error) => Display::fmt(error, f),
            Self::Header(error) => Display::fmt(error, f),
            Self::Prefix(error) => Display::fmt(error, f),
            Self::NegativeCount { field, count } => {
                write!(f, "{field} has negative count {count}")
            }
            Self::CountExceedsMetadata {
                field,
                count,
                remaining,
            } => write!(f, "{field} count {count} cannot fit in {remaining} metadata bytes"),
            Self::AllocationFailed { field, count } => {
                write!(f, "could not reserve {count} entries for {field}")
            }
            Self::InvalidMetadataRange {
                start,
                end,
                file_size,
            } => write!(f, "metadata table range {start}..{end} exceeds file size {file_size}"),
            Self::LayoutOverflow(field) => write!(f, "metadata {field} arithmetic overflowed"),
            Self::NegativeObjectOffset(offset) => write!(f, "object data offset is negative: {offset}"),
            Self::NegativeObjectSize(size) => write!(f, "object data size is negative: {size}"),
            Self::ObjectDataOutOfBounds {
                object_index,
                start,
                end,
                file_size,
            } => write!(
                f,
                "object {object_index} data range {start}..{end} exceeds file size {file_size}"
            ),
            Self::InvalidTypeIndex {
                object_index,
                type_index,
                type_count,
            } => write!(
                f,
                "object {object_index} references serialized type {type_index}, but the table has {type_count} entries"
            ),
            Self::ObjectTypeNotFound {
                object_index,
                raw_type_id,
                stripped,
            } => write!(
                f,
                "object {object_index} has no matching serialized type for ID {raw_type_id} and stripped={stripped:?}"
            ),
            Self::AmbiguousObjectType {
                object_index,
                raw_type_id,
                stripped,
            } => write!(
                f,
                "object {object_index} matches multiple serialized types for ID {raw_type_id} and stripped={stripped:?}"
            ),
            Self::InvalidTreeChildCount(count) => {
                write!(f, "TypeTree child count {count} exceeds remaining metadata")
            }
            Self::InvalidTypeTreeNodeCount => {
                write!(f, "TypeTree contains more nodes than can fit in remaining metadata")
            }
            Self::TypeTreeDepthOverflow(depth) => {
                write!(f, "TypeTree depth {depth} exceeds its serialized byte field")
            }
            Self::MissingTypeTreeStringOffset => {
                write!(f, "modern TypeTree node is missing a serialized string offset")
            }
            Self::UnterminatedTypeTreeString => {
                write!(f, "TypeTree string buffer contains an unterminated string")
            }
            Self::InvalidTypeTreeString => {
                write!(f, "TypeTree string buffer contains invalid UTF-8")
            }
            Self::UnknownCommonStringOffset { offset, context } => write!(
                f,
                "{context} uses unrecognized Unity common TypeTree string offset {offset}"
            ),
            Self::InvalidLocalStringOffset {
                offset,
                buffer_size,
                context,
            } => write!(
                f,
                "{context} uses local TypeTree string offset {offset}, outside its {buffer_size}-byte string buffer"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    include!("metadata_tests_01.rs");
}
