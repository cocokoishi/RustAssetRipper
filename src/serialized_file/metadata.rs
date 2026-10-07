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

        let prefix = SerializedFileMetadataPrefix::parse_file(file_bytes, header)
            .map_err(SerializedFileMetadataError::from_prefix)?;
        let metadata_end = prefix
            .metadata_offset
            .checked_add(prefix.metadata_size)
            .ok_or_else(|| {
                SerializedFileMetadataError::new(
                    prefix.metadata_offset,
                    SerializedFileMetadataErrorKind::LayoutOverflow("metadata end"),
                )
            })?;
        let tables_offset = prefix
            .metadata_offset
            .checked_add(prefix.consumed_bytes)
            .ok_or_else(|| {
                SerializedFileMetadataError::new(
                    prefix.metadata_offset,
                    SerializedFileMetadataErrorKind::LayoutOverflow("table offset"),
                )
            })?;
        let tables = file_bytes.get(tables_offset..metadata_end).ok_or_else(|| {
            SerializedFileMetadataError::new(
                tables_offset,
                SerializedFileMetadataErrorKind::InvalidMetadataRange {
                    start: tables_offset,
                    end: metadata_end,
                    file_size: file_bytes.len(),
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
    use super::{SerializedFileMetadata, SerializedFileMetadataErrorKind};
    use crate::io::binary::Endian;
    use crate::serialized_file::format_version::FormatVersion;
    use crate::serialized_file::header::SerializedFileHeader;

    fn write_u16(bytes: &mut Vec<u8>, value: u16, endian: Endian) {
        match endian {
            Endian::Little => bytes.extend_from_slice(&value.to_le_bytes()),
            Endian::Big => bytes.extend_from_slice(&value.to_be_bytes()),
        }
    }

    fn write_i16(bytes: &mut Vec<u8>, value: i16, endian: Endian) {
        write_u16(bytes, value as u16, endian);
    }

    fn write_u32(bytes: &mut Vec<u8>, value: u32, endian: Endian) {
        match endian {
            Endian::Little => bytes.extend_from_slice(&value.to_le_bytes()),
            Endian::Big => bytes.extend_from_slice(&value.to_be_bytes()),
        }
    }

    fn write_i32(bytes: &mut Vec<u8>, value: i32, endian: Endian) {
        write_u32(bytes, value as u32, endian);
    }

    fn write_i64(bytes: &mut Vec<u8>, value: i64, endian: Endian) {
        match endian {
            Endian::Little => bytes.extend_from_slice(&value.to_le_bytes()),
            Endian::Big => bytes.extend_from_slice(&value.to_be_bytes()),
        }
    }

    fn write_string(bytes: &mut Vec<u8>, value: &str) {
        bytes.extend_from_slice(value.as_bytes());
        bytes.push(0);
    }

    fn make_file(
        generation: FormatVersion,
        body_endian: Endian,
        metadata_body: &[u8],
        payload: &[u8],
    ) -> (Vec<u8>, SerializedFileHeader) {
        make_file_with_version(
            generation,
            body_endian,
            metadata_body,
            payload,
            "2022.3.1f1",
        )
    }

    fn make_file_with_version(
        generation: FormatVersion,
        body_endian: Endian,
        metadata_body: &[u8],
        payload: &[u8],
        unity_version: &str,
    ) -> (Vec<u8>, SerializedFileHeader) {
        let header_size = if generation.has_large_file_support() {
            48
        } else if generation.has_endianness() {
            20
        } else {
            16
        };
        let mut metadata = Vec::new();
        if generation.metadata_at_end() {
            metadata.push(u8::from(body_endian == Endian::Big));
        }
        if generation.has_signature() {
            write_string(&mut metadata, unity_version);
        }
        if generation.has_platform() {
            write_u32(&mut metadata, 7, body_endian);
        }
        if generation.has_enable_type_tree() {
            metadata.push(1);
        }
        metadata.extend_from_slice(metadata_body);
        let metadata_size = metadata.len();
        let data_offset = if generation.metadata_at_end() {
            0
        } else {
            (header_size + metadata_size) as u64
        };
        let file_size = header_size + metadata_size + payload.len();
        let mut file = vec![0; header_size];
        file.extend_from_slice(&metadata);
        file.extend_from_slice(payload);
        let header = SerializedFileHeader {
            metadata_size: metadata_size as u64,
            file_size: file_size as u64,
            format_version: generation,
            data_offset,
            data_endian: generation.has_endianness().then_some(body_endian),
            reserved_v22: generation.has_large_file_support().then_some(0),
            header_size,
        };
        (file, header)
    }

    fn push_empty_modern_tree(bytes: &mut Vec<u8>, generation: FormatVersion, endian: Endian) {
        write_i32(bytes, 0, endian); // node count
        write_i32(bytes, 0, endian); // string buffer size
        assert!(generation.raw() == 10 || generation.raw() >= 12);
    }

    fn write_modern_node(
        bytes: &mut Vec<u8>,
        generation: FormatVersion,
        endian: Endian,
        version: u16,
        level: u8,
        type_offset: u32,
        name_offset: u32,
    ) {
        write_u16(bytes, version, endian);
        bytes.push(level);
        bytes.push(0);
        write_u32(bytes, type_offset, endian);
        write_u32(bytes, name_offset, endian);
        write_i32(bytes, -1, endian);
        write_i32(bytes, 0, endian);
        write_u32(bytes, 0, endian);
        if generation.raw() >= 19 {
            let value = 0x1122_3344_5566_7788u64;
            match endian {
                Endian::Little => bytes.extend_from_slice(&value.to_le_bytes()),
                Endian::Big => bytes.extend_from_slice(&value.to_be_bytes()),
            }
        }
    }

    fn align_table_position(generation: FormatVersion, tables: &mut Vec<u8>, header_size: usize) {
        if generation.raw() < 14 {
            return;
        }
        let metadata_prefix_size = usize::from(generation.metadata_at_end())
            + if generation.has_signature() { 11 } else { 0 }
            + if generation.has_platform() { 4 } else { 0 }
            + usize::from(generation.has_enable_type_tree());
        let absolute_offset = header_size + metadata_prefix_size + tables.len();
        let padding = (4 - absolute_offset % 4) % 4;
        tables.resize(tables.len() + padding, 0);
    }

    #[test]
    fn parses_generation_8_tables_types_objects_and_external_file_ids() {
        let generation = FormatVersion::Unknown8;
        let endian = Endian::Big;
        let mut tables = Vec::new();
        write_i32(&mut tables, 1, endian); // type count
        write_i32(&mut tables, 28, endian); // type ID
        write_string(&mut tables, "Base");
        write_string(&mut tables, "m_Name");
        write_i32(&mut tables, 4, endian);
        write_i32(&mut tables, 0, endian);
        write_i32(&mut tables, 0, endian);
        write_i32(&mut tables, 1, endian); // TypeTree node version
        write_u32(&mut tables, 0, endian); // TypeTree transfer flags
        write_i32(&mut tables, 0, endian); // TypeTree child count
        write_i32(&mut tables, 1, endian); // long FileID flag
        write_i32(&mut tables, 1, endian); // object count
        write_i32(&mut tables, 9, endian); // PathID remains 32-bit at generation 8
        write_u32(&mut tables, 0, endian); // payload offset relative to header data offset
        write_i32(&mut tables, 0, endian); // payload length
        write_i32(&mut tables, 28, endian);
        write_u16(&mut tables, 28, endian);
        write_u16(&mut tables, 0, endian); // destroyed
        write_i32(&mut tables, 1, endian); // external count
        write_string(&mut tables, ""); // generation 8 asset path
        for word in [1, 2, 3, 4] {
            write_u32(&mut tables, word, endian);
        }
        write_i32(&mut tables, 2, endian); // asset type
        write_string(&mut tables, "shared.assets");
        write_string(&mut tables, ""); // user information

        let (file, header) = make_file(generation, endian, &tables, &[]);
        let metadata = SerializedFileMetadata::parse_file(&file, &header).unwrap();
        assert_eq!(metadata.types.len(), 1);
        assert_eq!(metadata.types[0].raw_type_id, 28);
        assert_eq!(metadata.long_file_id, Some(1));
        assert_eq!(metadata.objects.len(), 1);
        assert_eq!(metadata.objects[0].path_id, 9);
        assert_eq!(metadata.objects[0].resolved_type_index, Some(0));
        assert_eq!(metadata.externals[0].asset_path.as_deref(), Some(""));
        assert_eq!(metadata.externals[0].guid, Some([1, 2, 3, 4]));
        assert_eq!(metadata.externals[0].path_name_origin, "shared.assets");
        assert!(metadata.trailing_metadata.is_empty());
    }

    #[test]
    fn parses_generation_12_script_table_and_little_endian_tables() {
        let generation = FormatVersion::Unknown12;
        let endian = Endian::Little;
        let mut tables = Vec::new();
        write_i32(&mut tables, 1, endian); // types
        write_i32(&mut tables, 28, endian);
        push_empty_modern_tree(&mut tables, generation, endian);
        write_u32(&mut tables, 0, endian); // generation 7-13 long FileID field
        write_i32(&mut tables, 1, endian); // objects
        write_i32(&mut tables, 45, endian);
        write_u32(&mut tables, 0, endian);
        write_i32(&mut tables, 0, endian);
        write_i32(&mut tables, 28, endian);
        write_u16(&mut tables, 28, endian);
        write_i16(&mut tables, 0, endian); // script type index
        write_i32(&mut tables, 1, endian); // script identifiers
        write_i32(&mut tables, 2, endian);
        write_i32(&mut tables, 1234, endian);
        write_i32(&mut tables, 0, endian); // externals
        write_string(&mut tables, "metadata tail");

        let (file, header) = make_file(generation, endian, &tables, &[]);
        let metadata = SerializedFileMetadata::parse_file(&file, &header).unwrap();
        assert_eq!(metadata.objects[0].path_id, 45);
        assert_eq!(metadata.script_types[0].local_serialized_file_index, 2);
        assert_eq!(metadata.script_types[0].local_identifier_in_file, 1234);
        assert_eq!(metadata.user_information.as_deref(), Some("metadata tail"));
    }

    #[test]
    fn parses_generation_13_hashes_and_generation_14_aligned_path_ids() {
        for generation in [FormatVersion::HasTypeTreeHashes, FormatVersion::Unknown14] {
            let endian = Endian::Little;
            let mut tables = Vec::new();
            write_i32(&mut tables, 1, endian); // types
            write_i32(&mut tables, 28, endian);
            if generation.raw() >= 16 {
                tables.push(0);
            }
            if generation.raw() >= 13 {
                for word in [10, 11, 12, 13] {
                    write_u32(&mut tables, word, endian);
                }
            }
            push_empty_modern_tree(&mut tables, generation, endian);
            if generation.has_long_file_id() {
                write_u32(&mut tables, 0, endian);
            }
            write_i32(&mut tables, 1, endian); // objects
            if generation.raw() >= 14 {
                let header_size = if generation.has_large_file_support() {
                    48
                } else {
                    20
                };
                align_table_position(generation, &mut tables, header_size);
                write_i64(&mut tables, 0x1_0000_0001, endian);
            } else {
                write_i32(&mut tables, 1, endian);
            }
            write_u32(&mut tables, 0, endian);
            write_i32(&mut tables, 0, endian);
            write_i32(&mut tables, 28, endian);
            if generation.raw() < 16 {
                write_u16(&mut tables, 28, endian);
            } else {
                tables.push(0); // stripped type flag for the type record
            }
            if (11..17).contains(&generation.raw()) {
                write_i16(&mut tables, -1, endian);
            }
            if (15..17).contains(&generation.raw()) {
                tables.push(0);
            }
            if generation.has_script_types() {
                if generation.raw() == 14 {
                    write_i32(&mut tables, 1, endian);
                    write_i32(&mut tables, 3, endian);
                    align_table_position(generation, &mut tables, 20);
                    write_i64(&mut tables, 0x1_0000_0002, endian);
                } else {
                    write_i32(&mut tables, 0, endian);
                }
            }
            if generation.raw() >= 14 {
                write_i32(&mut tables, 0, endian); // script count
            }
            write_i32(&mut tables, 0, endian); // external count
            write_string(&mut tables, ""); // user information

            let (file, header) = make_file(generation, endian, &tables, &[]);
            let result = SerializedFileMetadata::parse_file(&file, &header);
            assert!(
                result.is_ok(),
                "generation {}: {result:?}",
                generation.raw()
            );
        }
    }

    #[test]
    fn parses_generation_19_modern_type_tree_nodes_and_reference_hashes() {
        let generation = FormatVersion::TypeTreeNodeWithTypeFlags;
        let endian = Endian::Big;
        let mut tables = Vec::new();
        write_i32(&mut tables, 1, endian); // type count
        write_i32(&mut tables, 28, endian);
        tables.push(0); // generation 16+ stripped type flag
        write_i16(&mut tables, -1, endian); // generation 17+ type script index
        for word in [1, 2, 3, 4] {
            write_u32(&mut tables, word, endian);
        }
        write_i32(&mut tables, 1, endian); // nodes
        write_i32(&mut tables, 5, endian); // string buffer bytes
        write_modern_node(&mut tables, generation, endian, 2, 0, 0x8000_0000 | 222, 0);
        tables.extend_from_slice(b"name\0");
        write_i32(&mut tables, 1, endian); // object count
        align_table_position(generation, &mut tables, 20);
        write_i64(&mut tables, 1, endian); // PathID
                                           // Generation 19 object data fields
        write_u32(&mut tables, 0, endian);
        write_i32(&mut tables, 0, endian);
        write_i32(&mut tables, 0, endian); // type index
        write_i32(&mut tables, 0, endian); // scripts
        write_i32(&mut tables, 0, endian); // externals
        write_string(&mut tables, "");
        let (file, header) = make_file(generation, endian, &tables, &[]);
        let metadata = SerializedFileMetadata::parse_file(&file, &header).unwrap();
        let node = &metadata.types[0].type_tree.as_ref().unwrap().nodes[0];
        assert_eq!(node.version, 2);
        assert_eq!(node.type_string_offset, Some(0x8000_0000 | 222));
        assert_eq!(node.name_string_offset, Some(0));
        assert_eq!(node.type_name.as_deref(), Some("int"));
        assert_eq!(node.name.as_deref(), Some("name"));
        assert_eq!(node.reference_type_hash, Some(0x1122_3344_5566_7788));
        assert_eq!(metadata.objects[0].path_id, 1);
    }

    #[test]
    fn parses_the_generation_10_and_generation_11_type_tree_layouts() {
        let mut modern_tables = Vec::new();
        write_i32(&mut modern_tables, 1, Endian::Little); // type count
        write_i32(&mut modern_tables, 28, Endian::Little);
        write_i32(&mut modern_tables, 1, Endian::Little); // node count
        write_i32(&mut modern_tables, 6, Endian::Little); // string buffer size
        write_modern_node(
            &mut modern_tables,
            FormatVersion::Unknown10,
            Endian::Little,
            2,
            0,
            0,
            4,
        );
        modern_tables.extend_from_slice(b"int\0x\0");
        write_i32(&mut modern_tables, 0, Endian::Little); // long FileID field
        write_i32(&mut modern_tables, 0, Endian::Little); // object count
        write_i32(&mut modern_tables, 0, Endian::Little); // external count
        write_string(&mut modern_tables, ""); // user information
        let (modern_file, modern_header) = make_file(
            FormatVersion::Unknown10,
            Endian::Little,
            &modern_tables,
            &[],
        );
        let modern = SerializedFileMetadata::parse_file(&modern_file, &modern_header).unwrap();
        let modern_node = &modern.types[0].type_tree.as_ref().unwrap().nodes[0];
        assert_eq!(modern_node.type_name.as_deref(), Some("int"));
        assert_eq!(modern_node.name.as_deref(), Some("x"));

        let mut legacy_tables = Vec::new();
        write_i32(&mut legacy_tables, 1, Endian::Big); // type count
        write_i32(&mut legacy_tables, 28, Endian::Big);
        write_string(&mut legacy_tables, "int");
        write_string(&mut legacy_tables, "value");
        write_i32(&mut legacy_tables, 4, Endian::Big);
        write_i32(&mut legacy_tables, 0, Endian::Big);
        write_i32(&mut legacy_tables, 0, Endian::Big);
        write_i32(&mut legacy_tables, 1, Endian::Big);
        write_u32(&mut legacy_tables, 0, Endian::Big);
        write_i32(&mut legacy_tables, 0, Endian::Big); // leaf node
        write_u32(&mut legacy_tables, 0, Endian::Big); // long FileID field
        write_i32(&mut legacy_tables, 0, Endian::Big); // object count
        write_i32(&mut legacy_tables, 0, Endian::Big); // script types
        write_i32(&mut legacy_tables, 0, Endian::Big); // external count
        write_string(&mut legacy_tables, ""); // user information
        let (legacy_file, legacy_header) = make_file(
            FormatVersion::HasScriptTypeIndex,
            Endian::Big,
            &legacy_tables,
            &[],
        );
        let legacy = SerializedFileMetadata::parse_file(&legacy_file, &legacy_header).unwrap();
        let legacy_node = &legacy.types[0].type_tree.as_ref().unwrap().nodes[0];
        assert_eq!(legacy_node.type_name.as_deref(), Some("int"));
        assert_eq!(legacy_node.name.as_deref(), Some("value"));
    }

    #[test]
    fn serialized_type_script_id_hash_presence_tracks_unity_version_fallback() {
        for (version, expect_script_id) in [
            ("2017.4.40f1", false),
            ("2018.3.0a1", true),
            ("unparseable", true),
        ] {
            let mut tables = Vec::new();
            write_i32(&mut tables, 1, Endian::Little); // type count
            write_i32(&mut tables, 28, Endian::Little);
            tables.push(0); // stripped flag
            write_i16(&mut tables, 1, Endian::Little); // script type index
            if expect_script_id {
                for word in [1, 2, 3, 4] {
                    write_u32(&mut tables, word, Endian::Little);
                }
            }
            for word in [5, 6, 7, 8] {
                write_u32(&mut tables, word, Endian::Little); // old type hash
            }
            push_empty_modern_tree(&mut tables, FormatVersion::RefactorTypeData, Endian::Little);
            write_i32(&mut tables, 1, Endian::Little); // objects
            align_table_position(FormatVersion::RefactorTypeData, &mut tables, 20);
            write_i64(
                &mut tables,
                100 + i64::from(expect_script_id),
                Endian::Little,
            );
            write_u32(&mut tables, 0, Endian::Little);
            write_i32(&mut tables, 0, Endian::Little);
            write_i32(&mut tables, 0, Endian::Little); // serialized type index
            write_i32(&mut tables, 0, Endian::Little); // scripts
            write_i32(&mut tables, 0, Endian::Little); // externals
            write_string(&mut tables, ""); // user information

            let (file, header) = make_file_with_version(
                FormatVersion::RefactorTypeData,
                Endian::Little,
                &tables,
                &[],
                version,
            );
            let metadata = SerializedFileMetadata::parse_file(&file, &header).unwrap();
            assert_eq!(
                metadata.types[0].script_id.is_some(),
                expect_script_id,
                "Unity version {version}"
            );
            assert_eq!(metadata.objects[0].resolved_type_index, Some(0));
        }
    }

    #[test]
    fn type_script_id_presence_matches_the_source_read_gate() {
        use super::should_read_type_script_id;
        // MonoBehaviour markers (`-1`, the clamped pre-16 negative-ID form) and
        // `MonoScript` (`114`) always carry the hash, whatever the script index,
        // flavor, or Unity version.
        for type_id in [-1, 114] {
            for (script, reference, version) in [
                (-1, false, None),
                (0, true, Some("5.4.0f1")),
                (-1, false, Some("2022.1.0f1")),
            ] {
                assert!(
                    should_read_type_script_id(type_id, script, reference, version),
                    "type {type_id}, script {script}, reference {reference}, version {version:?}"
                );
            }
        }
        // Otherwise a non-negative script index decides, except old Unity versions
        // ignore script types for plain types while reference types honor them.
        assert!(should_read_type_script_id(28, 0, false, Some("2020.3.0f1")));
        assert!(!should_read_type_script_id(
            28,
            0,
            false,
            Some("2017.4.0f1")
        ));
        assert!(should_read_type_script_id(28, 0, true, Some("2017.4.0f1")));
        // An unparseable version falls back to reading, like the source 0.0.0 default.
        assert!(should_read_type_script_id(
            28,
            0,
            false,
            Some("not-a-version")
        ));
        // Negative script indexes never carry the hash for ordinary classes.
        assert!(!should_read_type_script_id(
            28,
            -1,
            false,
            Some("2020.3.0f1")
        ));
        assert!(!should_read_type_script_id(28, -1, true, None));
    }

    #[test]
    fn generation_22_reads_64_bit_path_ids_offsets_and_checked_payload_ranges() {
        let generation = FormatVersion::LargeFilesSupport;
        let mut tables = Vec::new();
        write_i32(&mut tables, 1, Endian::Little); // types
        write_i32(&mut tables, 28, Endian::Little);
        tables.push(0); // stripped type
        write_i16(&mut tables, -1, Endian::Little); // script type index
        for word in [1, 2, 3, 4] {
            write_u32(&mut tables, word, Endian::Little); // old type hash
        }
        push_empty_modern_tree(&mut tables, generation, Endian::Little);
        write_i32(&mut tables, 0, Endian::Little); // type dependencies
        write_i32(&mut tables, 1, Endian::Little); // object count
        align_table_position(generation, &mut tables, 48);
        write_i64(&mut tables, 0x1234_5678_9abc_def0, Endian::Little);
        write_i64(&mut tables, 0, Endian::Little); // large payload offset
        write_i32(&mut tables, 1, Endian::Little); // payload size
        write_i32(&mut tables, 0, Endian::Little); // serialized type index
        write_i32(&mut tables, 0, Endian::Little); // script types
        write_i32(&mut tables, 0, Endian::Little); // externals
        write_i32(&mut tables, 0, Endian::Little); // reference types
        write_string(&mut tables, ""); // user information
        let (file, header) = make_file(generation, Endian::Little, &tables, &[0xa5]);

        let metadata = SerializedFileMetadata::parse_file(&file, &header).unwrap();
        assert_eq!(metadata.objects[0].path_id, 0x1234_5678_9abc_def0);
        assert_eq!(metadata.objects[0].byte_start, 0);
        assert_eq!(metadata.objects[0].byte_size, 1);
        assert_eq!(
            metadata.objects[0].data_range(&header),
            Some(header.data_offset..header.file_size)
        );
        assert_eq!(file[header.data_offset as usize], 0xa5);
    }

    #[test]
    fn parses_generation_20_reference_types_and_generation_21_dependencies() {
        let generation = FormatVersion::StoresTypeDependencies;
        let endian = Endian::Little;
        let mut tables = Vec::new();
        write_i32(&mut tables, 1, endian); // normal types
        write_i32(&mut tables, 114, endian);
        tables.push(0); // generation 16+ stripped type flag
        write_i16(&mut tables, -1, endian); // generation 17+ type script index
        for word in [1, 2, 3, 4, 5, 6, 7, 8] {
            write_u32(&mut tables, word, endian);
        }
        push_empty_modern_tree(&mut tables, generation, endian);
        write_i32(&mut tables, 2, endian);
        write_i32(&mut tables, 8, endian);
        write_i32(&mut tables, -1, endian);
        write_i32(&mut tables, 0, endian); // object count
        write_i32(&mut tables, 0, endian); // script type count
        write_i32(&mut tables, 0, endian); // external count
        write_i32(&mut tables, 1, endian); // one reference type
        write_i32(&mut tables, 114, endian);
        tables.push(0); // generation 16+ stripped type flag
        write_i16(&mut tables, -1, endian); // generation 17+ type script index
        for word in [11, 12, 13, 14, 15, 16, 17, 18] {
            write_u32(&mut tables, word, endian);
        }
        push_empty_modern_tree(&mut tables, generation, endian);
        write_string(&mut tables, "Example");
        write_string(&mut tables, "Example.Namespace");
        write_string(&mut tables, "Example.Assembly");
        write_string(&mut tables, ""); // user information

        let (file, header) = make_file(generation, endian, &tables, &[]);
        let metadata = SerializedFileMetadata::parse_file(&file, &header).unwrap();
        assert_eq!(metadata.types[0].type_dependencies, [8, -1]);
        assert_eq!(metadata.reference_types.len(), 1);
        assert_eq!(
            metadata.reference_types[0].class_name.as_deref(),
            Some("Example")
        );
        assert_eq!(
            metadata.reference_types[0].namespace.as_deref(),
            Some("Example.Namespace")
        );
        assert_eq!(
            metadata.reference_types[0].assembly_name.as_deref(),
            Some("Example.Assembly")
        );
    }

    #[test]
    fn malformed_count_and_object_range_return_contextual_errors() {
        let generation = FormatVersion::Unknown9;
        let endian = Endian::Little;
        let mut negative_count_tables = Vec::new();
        write_i32(&mut negative_count_tables, -1, endian);
        let (file, header) = make_file(generation, endian, &negative_count_tables, &[]);
        let error = SerializedFileMetadata::parse_file(&file, &header).unwrap_err();
        assert!(matches!(
            error.kind(),
            SerializedFileMetadataErrorKind::NegativeCount { .. }
        ));

        let mut out_of_bounds_tables = Vec::new();
        write_i32(&mut out_of_bounds_tables, 0, endian); // types
        write_i32(&mut out_of_bounds_tables, 1, endian); // objects
        write_i32(&mut out_of_bounds_tables, 1, endian); // PathID
        write_u32(&mut out_of_bounds_tables, 999, endian); // data offset outside file
        write_i32(&mut out_of_bounds_tables, 1, endian);
        write_i32(&mut out_of_bounds_tables, 0, endian); // type ID
        write_u16(&mut out_of_bounds_tables, 0, endian); // class ID
        write_i32(&mut out_of_bounds_tables, 0, endian); // externals
        write_string(&mut out_of_bounds_tables, ""); // user info
        let (file, header) = make_file(generation, endian, &out_of_bounds_tables, &[]);
        let error = SerializedFileMetadata::parse_file(&file, &header).unwrap_err();
        assert!(matches!(
            error.kind(),
            SerializedFileMetadataErrorKind::ObjectDataOutOfBounds { .. }
        ));
    }

    #[test]
    fn type_table_index_out_of_bounds_is_not_silently_ignored() {
        let generation = FormatVersion::RefactorTypeData;
        let endian = Endian::Little;
        let mut tables = Vec::new();
        write_i32(&mut tables, 0, endian); // types
        write_i32(&mut tables, 1, endian); // objects
        write_i64(&mut tables, 77, endian); // aligned PathID; initial metadata absolute offset is aligned here
        write_u32(&mut tables, 0, endian);
        write_i32(&mut tables, 0, endian);
        write_i32(&mut tables, 4, endian); // invalid type table index
        write_i32(&mut tables, 0, endian); // scripts
        write_i32(&mut tables, 0, endian); // externals
        write_string(&mut tables, "");
        let (file, header) = make_file(generation, endian, &tables, &[]);
        let error = SerializedFileMetadata::parse_file(&file, &header).unwrap_err();
        assert!(matches!(
            error.kind(),
            SerializedFileMetadataErrorKind::InvalidTypeIndex { type_index: 4, .. }
        ));
    }

    #[test]
    fn modern_object_records_resolve_types_and_reject_bad_references() {
        fn modern_tables(type_index: i32) -> Vec<u8> {
            let generation = FormatVersion::TypeTreeNodeWithTypeFlags;
            let endian = Endian::Little;
            let mut tables = Vec::new();
            write_i32(&mut tables, 1, endian); // type count
            write_i32(&mut tables, 28, endian);
            tables.push(1); // stripped type flag
            write_i16(&mut tables, -1, endian); // type script index
            for word in [1, 2, 3, 4] {
                write_u32(&mut tables, word, endian);
            }
            push_empty_modern_tree(&mut tables, generation, endian);
            write_i32(&mut tables, 1, endian); // object count
            align_table_position(generation, &mut tables, 20);
            write_i64(&mut tables, 5, endian); // PathID
            write_u32(&mut tables, 0, endian);
            write_i32(&mut tables, 0, endian);
            write_i32(&mut tables, type_index, endian);
            write_i32(&mut tables, 0, endian); // scripts
            write_i32(&mut tables, 0, endian); // externals
            write_string(&mut tables, "");
            tables
        }

        // A resolved modern object backfills the raw ID and the script/stripped
        // slots from its type while the absent pre-17 fields stay empty, matching
        // the source defaults.
        let generation = FormatVersion::TypeTreeNodeWithTypeFlags;
        let endian = Endian::Little;
        let (file, header) = make_file(generation, endian, &modern_tables(0), &[]);
        let metadata = SerializedFileMetadata::parse_file(&file, &header).unwrap();
        let object = &metadata.objects[0];
        assert_eq!(object.path_id, 5);
        assert_eq!(object.serialized_type_index, Some(0));
        assert_eq!(object.raw_type_id, 28);
        assert_eq!(object.class_id, None);
        assert_eq!(object.script_type_index, Some(-1));
        assert_eq!(object.is_destroyed, None);
        assert_eq!(object.is_stripped, Some(true));
        assert_eq!(object.resolved_type_index, Some(0));

        // A negative serialized type index scans for TypeID 0 instead of
        // indexing, so a table without it is an explicit error.
        let (file, header) = make_file(generation, endian, &modern_tables(-1), &[]);
        let error = SerializedFileMetadata::parse_file(&file, &header).unwrap_err();
        assert!(matches!(
            error.kind(),
            SerializedFileMetadataErrorKind::ObjectTypeNotFound { raw_type_id: 0, .. }
        ));

        // Duplicate TypeIDs on the scan path stay ambiguous instead of taking
        // the first match.
        let generation = FormatVersion::RefactorTypeData;
        let mut tables = Vec::new();
        write_i32(&mut tables, 2, endian); // type count
        for base in [1, 5] {
            write_i32(&mut tables, 0, endian);
            tables.push(0);
            write_i16(&mut tables, -1, endian);
            for word in base..base + 4 {
                write_u32(&mut tables, word, endian);
            }
            push_empty_modern_tree(&mut tables, generation, endian);
        }
        write_i32(&mut tables, 1, endian); // object count
        align_table_position(generation, &mut tables, 20);
        write_i64(&mut tables, 3, endian); // PathID
        write_u32(&mut tables, 0, endian);
        write_i32(&mut tables, 0, endian);
        write_i32(&mut tables, -1, endian); // negative index scans for TypeID 0
        let (file, header) = make_file(generation, endian, &tables, &[]);
        let error = SerializedFileMetadata::parse_file(&file, &header).unwrap_err();
        assert!(matches!(
            error.kind(),
            SerializedFileMetadataErrorKind::AmbiguousObjectType { raw_type_id: 0, .. }
        ));

        // A negative large-file payload offset is rejected, not wrapped.
        let generation = FormatVersion::LargeFilesSupport;
        let mut tables = Vec::new();
        write_i32(&mut tables, 0, endian); // types
        write_i32(&mut tables, 1, endian); // objects
        align_table_position(generation, &mut tables, 48);
        write_i64(&mut tables, 9, endian); // PathID
        write_i64(&mut tables, -1, endian); // negative payload offset
        let (file, header) = make_file(generation, endian, &tables, &[]);
        let error = SerializedFileMetadata::parse_file(&file, &header).unwrap_err();
        assert!(matches!(
            error.kind(),
            SerializedFileMetadataErrorKind::NegativeObjectOffset(-1)
        ));

        // A negative payload size is rejected on the legacy integer path too.
        let generation = FormatVersion::Unknown9;
        let mut tables = Vec::new();
        write_i32(&mut tables, 0, endian); // types
        write_u32(&mut tables, 0, endian); // generation 7-13 long FileID field
        write_i32(&mut tables, 1, endian); // objects
        write_i32(&mut tables, 9, endian); // PathID
        write_u32(&mut tables, 0, endian);
        write_i32(&mut tables, -5, endian); // negative payload size
        let (file, header) = make_file(generation, endian, &tables, &[]);
        let error = SerializedFileMetadata::parse_file(&file, &header).unwrap_err();
        assert!(matches!(
            error.kind(),
            SerializedFileMetadataErrorKind::NegativeObjectSize(-5)
        ));
    }

    #[test]
    fn typetree_reader_matches_reference_runtime() {
        use super::{has_ref_type_hash, is_format5, read_type_tree};
        use crate::io::binary::BinaryReader;
        use crate::serialized_file::common_strings;

        fn hex_to_bytes(hex: &str) -> Vec<u8> {
            assert!(hex.len().is_multiple_of(2), "odd hex length for {hex}");
            let mut out = Vec::with_capacity(hex.len() / 2);
            let bytes = hex.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                let hi = hex_val(bytes[i]);
                let lo = hex_val(bytes[i + 1]);
                out.push((hi << 4) | lo);
                i += 2;
            }
            out
        }

        fn hex_val(b: u8) -> u8 {
            match b {
                b'0'..=b'9' => b - b'0',
                b'a'..=b'f' => b - b'a' + 10,
                b'A'..=b'F' => b - b'A' + 10,
                _ => panic!("invalid hex byte {b}"),
            }
        }

        fn parse_kv<'a>(chunk: &'a str, key: &str) -> &'a str {
            let prefix = format!("{key}=");
            for part in chunk.split(' ') {
                if let Some(v) = part.strip_prefix(&prefix) {
                    return v;
                }
            }
            panic!("missing {key} in {chunk}");
        }

        let fixture = include_str!("typetree_reader_fixture.txt");
        let mut preds: Vec<(i32, bool, bool)> = Vec::new();
        let mut commons: Vec<(u32, String)> = Vec::new();
        let mut hexes: std::collections::HashMap<String, Vec<u8>> =
            std::collections::HashMap::new();
        let mut reads: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for line in fixture.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            if line.starts_with("PRED|") {
                let parts: Vec<&str> = line.split('|').collect();
                assert_eq!(parts.len(), 4, "bad PRED {line}");
                preds.push((
                    parts[1].parse::<i32>().unwrap(),
                    parts[2] == "True",
                    parts[3] == "True",
                ));
            } else if line.starts_with("COMMON|") {
                let parts: Vec<&str> = line.splitn(4, '|').collect();
                assert_eq!(parts.len(), 4, "bad COMMON {line}");
                commons.push((parts[1].parse::<u32>().unwrap(), parts[3].to_string()));
            } else if line.contains("_HEX|") {
                let (name, hex) = line.split_once('|').unwrap();
                let name = name.strip_suffix("_HEX").unwrap().to_string();
                hexes.insert(name, hex_to_bytes(hex));
            } else if line.contains("_READ|") {
                let (name, dump) = line.split_once('|').unwrap();
                let name = name.strip_suffix("_READ").unwrap().to_string();
                reads.insert(name, dump.to_string());
            }
        }

        // Generation predicates cover every supported generation from the probe.
        assert!(preds.len() >= 21, "expected PRED rows, got {}", preds.len());
        for (raw, want_format5, want_hash) in preds {
            let generation = FormatVersion::try_from(raw).unwrap();
            assert_eq!(is_format5(generation), want_format5, "IsFormat5 gen {raw}");
            assert_eq!(
                has_ref_type_hash(generation),
                want_hash,
                "HasRefTypeHash gen {raw}"
            );
        }
        // The legacy/modern split is exactly generation 10 or 12+.
        for raw in [1, 2, 3, 5, 6, 7, 8, 9, 11] {
            assert!(
                !is_format5(FormatVersion::try_from(raw).unwrap()),
                "gen {raw}"
            );
        }
        for raw in [10, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22] {
            assert!(
                is_format5(FormatVersion::try_from(raw).unwrap()),
                "gen {raw}"
            );
        }

        // Full common-string table: every reference entry resolves identically.
        assert_eq!(commons.len(), 113, "expected 113 COMMON rows");
        for (offset, plain) in &commons {
            assert_eq!(
                common_strings::get(*offset).map(str::to_string),
                Some(plain.clone()),
                "common offset {offset}"
            );
        }
        for gap in [1u32, 2, 223, 9999, 5000] {
            assert_eq!(common_strings::get(gap), None, "gap offset {gap}");
        }

        // Nested and string-buffer payloads are C#-constructed in the probe.
        let cases: &[(&str, FormatVersion, Endian, bool)] = &[
            ("LEGACY_BE8", FormatVersion::Unknown8, Endian::Big, false),
            (
                "LEGACY_LE11",
                FormatVersion::HasScriptTypeIndex,
                Endian::Little,
                false,
            ),
            (
                "MODERN_LE10",
                FormatVersion::Unknown10,
                Endian::Little,
                false,
            ),
            (
                "MODERN_BE19",
                FormatVersion::TypeTreeNodeWithTypeFlags,
                Endian::Big,
                true,
            ),
            (
                "MODERN_LE22",
                FormatVersion::LargeFilesSupport,
                Endian::Little,
                true,
            ),
        ];
        for (name, generation, endian, want_hash) in cases {
            let bytes = hexes
                .get(*name)
                .unwrap_or_else(|| panic!("missing {name}_HEX"));
            let dump = reads
                .get(*name)
                .unwrap_or_else(|| panic!("missing {name}_READ"));
            assert!(!dump.starts_with("THROW"), "{name} unexpectedly threw");
            let mut reader = BinaryReader::new(bytes, *endian);
            let tree = read_type_tree(&mut reader, *generation, "fixture")
                .unwrap_or_else(|e| panic!("{name} Rust read failed: {e:?}"));
            assert_eq!(reader.remaining(), 0, "{name} trailing bytes");

            let (head, nodes_part) = dump.split_once(';').unwrap();
            let want_nodes: usize = parse_kv(head, "NODES").parse().unwrap();
            let want_buf_len: usize = parse_kv(head, "BUF").parse().unwrap();
            let want_buf_hex = parse_kv(head, "HEXBUF");
            assert_eq!(tree.nodes.len(), want_nodes, "{name} node count");
            assert_eq!(tree.string_buffer.len(), want_buf_len, "{name} buf len");
            assert_eq!(
                hex_to_bytes(want_buf_hex),
                tree.string_buffer,
                "{name} string buffer"
            );
            // Split `[i ...]` blocks without external crates.
            let mut blocks: Vec<String> = Vec::new();
            let mut cur = String::new();
            let mut open = false;
            for ch in nodes_part.chars() {
                if ch == '[' {
                    open = true;
                    cur.clear();
                } else if ch == ']' {
                    open = false;
                    blocks.push(std::mem::take(&mut cur));
                } else if open {
                    cur.push(ch);
                }
            }
            assert_eq!(blocks.len(), want_nodes, "{name} dump blocks");
            for (i, block) in blocks.iter().enumerate() {
                let node = &tree.nodes[i];
                let want_level: u8 = parse_kv(block, "L").parse().unwrap();
                let want_version: i32 = parse_kv(block, "V").parse().unwrap();
                let want_flags: i32 = parse_kv(block, "F").parse().unwrap();
                let want_type = parse_kv(block, "T");
                let want_name = parse_kv(block, "N");
                let want_size: i32 = parse_kv(block, "S").parse().unwrap();
                let want_index: i32 = parse_kv(block, "I").parse().unwrap();
                let want_meta: u32 = parse_kv(block, "M").parse().unwrap();
                let want_hash_hex = parse_kv(block, "H");
                let want_to_hex = parse_kv(block, "TO");
                let want_no_hex = parse_kv(block, "NO");
                assert_eq!(node.level, want_level, "{name} node {i} level");
                assert_eq!(node.version, want_version, "{name} node {i} version");
                assert_eq!(node.type_flags, want_flags, "{name} node {i} flags");
                assert_eq!(
                    node.type_name.as_deref(),
                    Some(want_type),
                    "{name} node {i} type"
                );
                assert_eq!(
                    node.name.as_deref(),
                    Some(want_name),
                    "{name} node {i} name"
                );
                assert_eq!(node.byte_size, want_size, "{name} node {i} size");
                assert_eq!(node.index, want_index, "{name} node {i} index");
                assert_eq!(node.meta_flags, want_meta, "{name} node {i} meta");
                if name.starts_with("LEGACY") {
                    assert_eq!(node.type_string_offset, None, "{name} node {i} TO");
                    assert_eq!(node.name_string_offset, None, "{name} node {i} NO");
                    assert_eq!(node.reference_type_hash, None, "{name} node {i} hash");
                } else {
                    let want_to = u32::from_str_radix(want_to_hex, 16).unwrap();
                    let want_no = u32::from_str_radix(want_no_hex, 16).unwrap();
                    assert_eq!(node.type_string_offset, Some(want_to), "{name} node {i} TO");
                    assert_eq!(node.name_string_offset, Some(want_no), "{name} node {i} NO");
                    if *want_hash {
                        let want_h = u64::from_str_radix(want_hash_hex, 16).unwrap();
                        assert_eq!(
                            node.reference_type_hash,
                            Some(want_h),
                            "{name} node {i} hash"
                        );
                    } else {
                        assert_eq!(node.reference_type_hash, None, "{name} node {i} hash");
                    }
                }
            }
        }

        // Negative node counts fail loudly on both sides.
        {
            let bytes = hexes.get("MODERN_NEG").expect("missing MODERN_NEG_HEX");
            let dump = reads.get("MODERN_NEG").expect("missing MODERN_NEG_READ");
            assert!(dump.starts_with("THROW:"), "reference should throw");
            let mut reader = BinaryReader::new(bytes, Endian::Little);
            let err = read_type_tree(&mut reader, FormatVersion::Unknown10, "fixture")
                .expect_err("negative count must fail");
            assert!(format!("{err:?}").len() > 8, "error carries context");
        }
    }
}
