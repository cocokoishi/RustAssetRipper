use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::io::binary::{BinaryError, BinaryReader, Endian};

use super::format_version::FormatVersion;
use super::header::{HeaderError, SerializedFileHeader};
use super::managed_types::managed_reference as managed_names;
use super::metadata::{
    unity_version_aligns_arrays, SerializedFileMetadata, TypeTree, TypeTreeNode,
};
use super::transfer_flags::TransferInstructionFlags;

const ALIGN_BYTES_FLAG: u32 = 0x4000;
const CHAR_PROPERTY_MASK_FLAG: u32 = 0x0200_0000;
const MAX_TYPE_TREE_DEPTH: usize = 256;

mod writer;
pub use writer::{
    ObjectDefaultValueLimits, ObjectEncodeError, ObjectEncodeErrorKind, ObjectEncodeLimits,
};

/// Per-object bounds for dynamic TypeTree decoding. Callers can raise these limits when
/// processing trusted large assets or lower them for untrusted input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectDecodeLimits {
    pub max_decoded_allocation_bytes: usize,
    pub max_array_elements: usize,
    pub max_string_bytes: usize,
    pub max_type_tree_depth: usize,
}

impl Default for ObjectDecodeLimits {
    fn default() -> Self {
        Self {
            max_decoded_allocation_bytes: 256 * 1024 * 1024,
            max_array_elements: 16 * 1024 * 1024,
            max_string_bytes: 64 * 1024 * 1024,
            max_type_tree_depth: MAX_TYPE_TREE_DEPTH,
        }
    }
}

struct DecodeBudget<'a> {
    limits: &'a ObjectDecodeLimits,
    allocated_bytes: usize,
}

impl DecodeBudget<'_> {
    fn charge(
        &mut self,
        requested: usize,
        offset: usize,
        resource: &str,
    ) -> Result<(), ObjectDecodeError> {
        let total = self.allocated_bytes.checked_add(requested).ok_or_else(|| {
            ObjectDecodeError::new(
                offset,
                ObjectDecodeErrorKind::AllocationSizeOverflow {
                    resource: resource.to_owned(),
                },
            )
        })?;
        if total > self.limits.max_decoded_allocation_bytes {
            return Err(ObjectDecodeError::new(
                offset,
                ObjectDecodeErrorKind::DecodeLimitExceeded {
                    resource: resource.to_owned(),
                    requested: total,
                    limit: self.limits.max_decoded_allocation_bytes,
                },
            ));
        }
        self.allocated_bytes = total;
        Ok(())
    }

    fn charge_items<T>(
        &mut self,
        count: usize,
        offset: usize,
        resource: &str,
    ) -> Result<(), ObjectDecodeError> {
        let bytes = count.checked_mul(std::mem::size_of::<T>()).ok_or_else(|| {
            ObjectDecodeError::new(
                offset,
                ObjectDecodeErrorKind::AllocationSizeOverflow {
                    resource: resource.to_owned(),
                },
            )
        })?;
        self.charge(bytes, offset, resource)
    }
}

struct TypeTreeLayout<'a> {
    nodes: &'a [TypeTreeNode],
    first_child: Vec<Option<usize>>,
    next_sibling: Vec<Option<usize>>,
}

impl<'a> TypeTreeLayout<'a> {
    fn new(
        type_tree: &'a TypeTree,
        object_index: usize,
        offset: usize,
    ) -> Result<Self, ObjectDecodeError> {
        let nodes = type_tree.nodes.as_slice();
        if nodes.is_empty() {
            return Err(ObjectDecodeError::new(
                offset,
                ObjectDecodeErrorKind::EmptyTypeTree { object_index },
            ));
        }
        if nodes[0].level != 0 {
            return Err(invalid_tree_levels(offset, object_index, 0, 0));
        }

        let mut first_child = reserve_option_indices(nodes.len(), offset)?;
        let mut next_sibling = reserve_option_indices(nodes.len(), offset)?;
        let mut last_child = reserve_option_indices(nodes.len(), offset)?;
        let mut stack = Vec::new();
        stack
            .try_reserve_exact(nodes.len())
            .map_err(|_| allocation_error(offset, "TypeTree traversal stack", nodes.len()))?;
        stack.push(0usize);

        for index in 1..nodes.len() {
            let level = nodes[index].level;
            while let Some(&parent_index) = stack.last() {
                if nodes[parent_index].level >= level {
                    stack.pop();
                } else {
                    break;
                }
            }
            let Some(&parent_index) = stack.last() else {
                return Err(invalid_tree_levels(offset, object_index, index - 1, index));
            };
            if nodes[parent_index].level.checked_add(1) != Some(level) {
                return Err(invalid_tree_levels(
                    offset,
                    object_index,
                    parent_index,
                    index,
                ));
            }
            if let Some(previous_child) = last_child[parent_index] {
                next_sibling[previous_child] = Some(index);
            } else {
                first_child[parent_index] = Some(index);
            }
            last_child[parent_index] = Some(index);
            stack.push(index);
        }

        Ok(Self {
            nodes,
            first_child,
            next_sibling,
        })
    }
}

/// A structured view of one object's serialized payload.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedObjectPayload {
    pub object_index: usize,
    pub path_id: i64,
    pub type_name: String,
    pub root: SerializedValue,
    pub consumed_bytes: usize,
    /// Bytes after the fields described by the TypeTree remain visible here.
    pub trailing_bytes: Vec<u8>,
    pub allocated_bytes: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SerializedFieldValue {
    pub name: String,
    pub value: SerializedValue,
}

/// Typed field data decoded directly from a SerializedFile object payload.
#[derive(Clone, Debug, PartialEq)]
pub enum SerializedValue {
    Bool(bool),
    I8(i8),
    U8(u8),
    I16(i16),
    U16(u16),
    Char(u16),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    F32(f32),
    F64(f64),
    /// Raw UTF-8 bytes are retained so invalid or non-canonical source strings are not lost.
    StringBytes(Vec<u8>),
    Bytes(Vec<u8>),
    Array(Vec<SerializedValue>),
    Struct(Vec<SerializedFieldValue>),
    /// A Unity object reference retaining its file-table index and signed PathID.
    PPtr {
        file_id: i32,
        path_id: i64,
    },
    /// Unknown leaf data with a nonnegative TypeTree byte size is preserved without decoding.
    Raw {
        type_name: String,
        bytes: Vec<u8>,
    },
}

impl SerializedValue {
    /// Returns the ordered fields when this value is a struct.
    pub fn struct_fields(&self) -> Option<&[SerializedFieldValue]> {
        match self {
            Self::Struct(fields) => Some(fields),
            _ => None,
        }
    }

    /// Finds a field only when its name is unique in this struct.
    ///
    /// Duplicate names are preserved by TypeTree decoding and reported instead of silently
    /// selecting one. Non-struct values are also reported distinctly from a missing field.
    pub fn unique_field(
        &self,
        name: &str,
    ) -> Result<Option<&SerializedValue>, SerializedValueLookupError> {
        let Self::Struct(fields) = self else {
            return Err(SerializedValueLookupError::NotStruct);
        };
        let mut matching = fields.iter().filter(|field| field.name == name);
        let Some(first) = matching.next() else {
            return Ok(None);
        };
        if matching.next().is_some() {
            let match_count = fields.iter().filter(|field| field.name == name).count();
            return Err(SerializedValueLookupError::DuplicateField {
                name: name.to_owned(),
                match_count,
            });
        }
        Ok(Some(&first.value))
    }

    /// Finds a field in this struct and nested `Base` structs without discarding duplicates.
    ///
    /// Unity TypeTrees may encode inherited object fields below a field named `Base`; following
    /// only that chain avoids searching unrelated nested resource records or array elements.
    pub fn unique_field_in_base_chain(
        &self,
        name: &str,
    ) -> Result<Option<&SerializedValue>, SerializedValueLookupError> {
        if !matches!(self, Self::Struct(_)) {
            return Err(SerializedValueLookupError::NotStruct);
        }
        let mut scope = self;
        let mut found = None;
        while let Self::Struct(_) = scope {
            if let Some(value) = scope.unique_field(name)? {
                if found.is_some() {
                    return Err(SerializedValueLookupError::DuplicateField {
                        name: name.to_owned(),
                        match_count: 2,
                    });
                }
                found = Some(value);
            }
            let Some(base) = scope.unique_field("Base")? else {
                break;
            };
            if !matches!(base, Self::Struct(_)) {
                break;
            }
            scope = base;
        }
        Ok(found)
    }

    /// Finds one field in this struct and nested `Base` structs for a controlled value update.
    /// Duplicate fields and duplicate matches across the base chain are rejected.
    pub fn unique_field_in_base_chain_mut(
        &mut self,
        name: &str,
    ) -> Result<Option<&mut SerializedValue>, SerializedValueMutationError> {
        if !matches!(self, Self::Struct(_)) {
            return Err(SerializedValueMutationError::NotStruct);
        }

        let mut current_path = Vec::<usize>::new();
        let mut found_path = None::<Vec<usize>>;
        let mut current: &SerializedValue = self;
        while let Self::Struct(fields) = current {
            if let Some(field_index) = unique_field_index(fields, name)? {
                if found_path.is_some() {
                    return Err(SerializedValueMutationError::DuplicateField {
                        name: name.to_owned(),
                        match_count: 2,
                    });
                }
                let path_length = current_path
                    .len()
                    .checked_add(1)
                    .ok_or(SerializedValueMutationError::FieldPathDepthOverflow)?;
                let mut path = Vec::new();
                path.try_reserve_exact(path_length).map_err(|_| {
                    SerializedValueMutationError::AllocationFailed {
                        requested: path_length,
                    }
                })?;
                path.extend_from_slice(&current_path);
                path.push(field_index);
                found_path = Some(path);
            }

            let Some(base_index) = unique_field_index(fields, "Base")? else {
                break;
            };
            let base = &fields[base_index].value;
            if !matches!(base, Self::Struct(_)) {
                break;
            }
            let requested_path_depth = current_path
                .len()
                .checked_add(1)
                .ok_or(SerializedValueMutationError::FieldPathDepthOverflow)?;
            current_path.try_reserve(1).map_err(|_| {
                SerializedValueMutationError::AllocationFailed {
                    requested: requested_path_depth,
                }
            })?;
            current_path.push(base_index);
            current = base;
        }

        let Some(path) = found_path else {
            return Ok(None);
        };
        let mut target = self;
        for field_index in path {
            let Self::Struct(fields) = target else {
                return Err(SerializedValueMutationError::InvalidFieldPath);
            };
            let Some(field) = fields.get_mut(field_index) else {
                return Err(SerializedValueMutationError::InvalidFieldPath);
            };
            target = &mut field.value;
        }
        Ok(Some(target))
    }

    /// Returns the ordered elements when this value is an array.
    pub fn array_elements(&self) -> Option<&[SerializedValue]> {
        match self {
            Self::Array(values) => Some(values),
            _ => None,
        }
    }

    /// Returns the array elements mutably while leaving their order and values intact.
    pub fn array_elements_mut(&mut self) -> Option<&mut [SerializedValue]> {
        match self {
            Self::Array(values) => Some(values.as_mut_slice()),
            _ => None,
        }
    }

    /// Appends one value to an array after checking both the caller's element bound and capacity.
    pub fn push_array_element_bounded(
        &mut self,
        value: SerializedValue,
        maximum_elements: usize,
    ) -> Result<(), SerializedValueMutationError> {
        let Self::Array(values) = self else {
            return Err(SerializedValueMutationError::NotArray);
        };
        let requested = values
            .len()
            .checked_add(1)
            .ok_or(SerializedValueMutationError::ArrayElementCountOverflow)?;
        if requested > maximum_elements {
            return Err(SerializedValueMutationError::ArrayElementLimitExceeded {
                requested,
                limit: maximum_elements,
            });
        }
        values
            .try_reserve(1)
            .map_err(|_| SerializedValueMutationError::AllocationFailed { requested })?;
        values.push(value);
        Ok(())
    }

    /// Returns the raw file-table index and PathID when this value is a PPtr.
    pub fn pptr(&self) -> Option<(i32, i64)> {
        match self {
            Self::PPtr { file_id, path_id } => Some((*file_id, *path_id)),
            _ => None,
        }
    }
}

fn unique_field_index(
    fields: &[SerializedFieldValue],
    name: &str,
) -> Result<Option<usize>, SerializedValueMutationError> {
    let mut matching = fields
        .iter()
        .enumerate()
        .filter(|(_, field)| field.name == name);
    let Some((index, _)) = matching.next() else {
        return Ok(None);
    };
    if matching.next().is_some() {
        let match_count = fields.iter().filter(|field| field.name == name).count();
        return Err(SerializedValueMutationError::DuplicateField {
            name: name.to_owned(),
            match_count,
        });
    }
    Ok(Some(index))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SerializedValueLookupError {
    NotStruct,
    DuplicateField { name: String, match_count: usize },
}

impl Display for SerializedValueLookupError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotStruct => f.write_str("field lookup requires a struct value"),
            Self::DuplicateField { name, match_count } => write!(
                f,
                "field '{name}' occurs {match_count} times in the decoded struct"
            ),
        }
    }
}

impl Error for SerializedValueLookupError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SerializedValueMutationError {
    NotStruct,
    NotArray,
    DuplicateField { name: String, match_count: usize },
    FieldPathDepthOverflow,
    InvalidFieldPath,
    ArrayElementCountOverflow,
    ArrayElementLimitExceeded { requested: usize, limit: usize },
    AllocationFailed { requested: usize },
}

impl Display for SerializedValueMutationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotStruct => f.write_str("field mutation requires a struct value"),
            Self::NotArray => f.write_str("array append requires an array value"),
            Self::DuplicateField { name, match_count } => write!(
                f,
                "field '{name}' occurs {match_count} times along the decoded base chain"
            ),
            Self::FieldPathDepthOverflow => f.write_str("field path depth overflows usize"),
            Self::InvalidFieldPath => f.write_str("decoded field path changed during mutation"),
            Self::ArrayElementCountOverflow => f.write_str("array element count overflows usize"),
            Self::ArrayElementLimitExceeded { requested, limit } => write!(
                f,
                "array append would create {requested} elements, exceeding the limit of {limit}"
            ),
            Self::AllocationFailed { requested } => write!(
                f,
                "could not reserve capacity for {requested} serialized value entries"
            ),
        }
    }
}

impl Error for SerializedValueMutationError {}

/// Reader-side TypeTree usability behind the absent-TypeTree recovery row
/// (`SER-TT-ABSENT-001`).
///
/// This states only what the reader knows without assemblies or generated
/// layouts: whether the resolved serialized type carries a TypeTree and whether
/// that tree has nodes. The factory's `MonoBehaviourReadFacts.has_type_tree`
/// consumes this fact; layout construction (`TypeTreeLayout::new`) and any
/// parse-acceptance decision stay downstream. A present-but-unparseable tree
/// reports `Present` here and surfaces its layout error when built, exactly
/// like the previous inline prelude checks.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TypeTreeUsability {
    /// No TypeTree: recovery must come from another structure source.
    Missing,
    /// A TypeTree with no nodes: undecodable like a missing one.
    Empty,
    /// A TypeTree with nodes: decode proceeds to layout construction.
    Present,
}

/// Classify the resolved serialized type's TypeTree for the decode prelude and
/// the absent-tree recovery facts.
pub fn classify_type_tree_usability(type_tree: Option<&TypeTree>) -> TypeTreeUsability {
    match type_tree {
        None => TypeTreeUsability::Missing,
        Some(tree) if tree.nodes.is_empty() => TypeTreeUsability::Empty,
        Some(_) => TypeTreeUsability::Present,
    }
}

/// `GameAssetFactory.ReadAsset` version gate over caller facts: assets from Unity
/// versions before 3.5 (stripped-version assets compare below it) cannot be read,
/// so the reader refuses them before touching the payload. Callers supply the parsed
/// major/minor pair; unknown or unparseable versions are NOT refused here, preserving
/// current behavior for files without version text.
pub const fn version_is_unsupported(major: u32, minor: u32) -> bool {
    major < 3 || (major == 3 && minor < 5)
}

/// Split `major.minor` off a Unity version string for the readability gate; anything
/// else proceeds like today.
fn version_major_minor(text: &str) -> Option<(u32, u32)> {
    let mut parts = text.split('.');
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

impl SerializedFileMetadata {
    /// Decode one object's payload using its resolved serialized type and TypeTree.
    ///
    /// A missing TypeTree is reported explicitly. This method does not claim coverage for the
    /// source project's no-TypeTree type recovery, MonoBehaviour assembly resolver, or managed
    /// reference registry paths; it reports the reader-side usability fact
    /// (`classify_type_tree_usability`) those paths consume.
    pub fn decode_object_payload(
        &self,
        file_bytes: &[u8],
        header: &SerializedFileHeader,
        object_index: usize,
    ) -> Result<ParsedObjectPayload, ObjectDecodeError> {
        self.decode_object_payload_with_limits(
            file_bytes,
            header,
            object_index,
            &ObjectDecodeLimits::default(),
        )
    }

    /// Decode one object using caller-selected bounds for dynamic allocations and recursion.
    pub fn decode_object_payload_with_limits(
        &self,
        file_bytes: &[u8],
        header: &SerializedFileHeader,
        object_index: usize,
        limits: &ObjectDecodeLimits,
    ) -> Result<ParsedObjectPayload, ObjectDecodeError> {
        let mut budget = DecodeBudget {
            limits,
            allocated_bytes: 0,
        };
        // The source refuses pre-3.5 assets before reading anything; refused payloads
        // flow into the existing raw-retention path downstream like any decode failure.
        if let Some((major, minor)) = self
            .prefix
            .unity_version_text
            .as_deref()
            .and_then(version_major_minor)
        {
            if version_is_unsupported(major, minor) {
                return Err(ObjectDecodeError::new(
                    0,
                    ObjectDecodeErrorKind::UnreadableUnsupportedVersion { object_index },
                ));
            }
        }
        header
            .validate_for_file_size(file_bytes.len())
            .map_err(ObjectDecodeError::from_header)?;
        let object = self.objects.get(object_index).ok_or_else(|| {
            ObjectDecodeError::new(
                0,
                ObjectDecodeErrorKind::InvalidObjectIndex {
                    object_index,
                    object_count: self.objects.len(),
                },
            )
        })?;
        let type_index = object.resolved_type_index.ok_or_else(|| {
            ObjectDecodeError::new(
                0,
                ObjectDecodeErrorKind::ObjectTypeUnresolved { object_index },
            )
        })?;
        let serialized_type = self.types.get(type_index).ok_or_else(|| {
            ObjectDecodeError::new(
                0,
                ObjectDecodeErrorKind::InvalidResolvedTypeIndex {
                    object_index,
                    type_index,
                    type_count: self.types.len(),
                },
            )
        })?;
        let type_tree = match classify_type_tree_usability(serialized_type.type_tree.as_ref()) {
            TypeTreeUsability::Missing => {
                return Err(ObjectDecodeError::new(
                    0,
                    ObjectDecodeErrorKind::MissingTypeTree { object_index },
                ));
            }
            TypeTreeUsability::Empty => {
                return Err(ObjectDecodeError::new(
                    0,
                    ObjectDecodeErrorKind::EmptyTypeTree { object_index },
                ));
            }
            TypeTreeUsability::Present => serialized_type.type_tree.as_ref().ok_or_else(|| {
                ObjectDecodeError::new(0, ObjectDecodeErrorKind::MissingTypeTree { object_index })
            })?,
        };
        let type_root = type_tree.nodes.first().ok_or_else(|| {
            ObjectDecodeError::new(0, ObjectDecodeErrorKind::EmptyTypeTree { object_index })
        })?;
        let type_layout = TypeTreeLayout::new(type_tree, object_index, 0)?;
        let data_range = object.data_range(header).ok_or_else(|| {
            ObjectDecodeError::new(
                0,
                ObjectDecodeErrorKind::ObjectDataRangeOverflow { object_index },
            )
        })?;
        let start = usize::try_from(data_range.start).map_err(|_| {
            ObjectDecodeError::new(
                0,
                ObjectDecodeErrorKind::ObjectDataRangeOverflow { object_index },
            )
        })?;
        let end = usize::try_from(data_range.end).map_err(|_| {
            ObjectDecodeError::new(
                start,
                ObjectDecodeErrorKind::ObjectDataRangeOverflow { object_index },
            )
        })?;
        let payload = file_bytes.get(start..end).ok_or_else(|| {
            ObjectDecodeError::new(
                start,
                ObjectDecodeErrorKind::ObjectDataOutOfBounds {
                    object_index,
                    start: data_range.start,
                    end: data_range.end,
                    file_size: file_bytes.len(),
                },
            )
        })?;
        let mut reader = BinaryReader::with_base_offset(payload, self.prefix.body_endian, start)
            .map_err(ObjectDecodeError::from_binary)?;
        let arrays_align = unity_version_aligns_arrays(self.prefix.unity_version_text.as_deref());

        let root = decode_root(
            &mut reader,
            &type_layout,
            arrays_align,
            object_index,
            type_root.type_name.as_deref().unwrap_or(""),
            &mut budget,
        )?;
        let consumed_bytes = reader.position();
        let trailing_length = reader.remaining();
        budget.charge(trailing_length, reader.offset(), "trailing object bytes")?;
        let trailing_bytes = reader
            .read_exact(reader.remaining())
            .map_err(|error| binary_error(error, "trailing object payload bytes"))?
            .to_vec();

        Ok(ParsedObjectPayload {
            object_index,
            path_id: object.path_id,
            type_name: type_root
                .type_name
                .clone()
                .unwrap_or_else(|| format!("type {}", serialized_type.raw_type_id)),
            root,
            consumed_bytes,
            trailing_bytes,
            allocated_bytes: budget.allocated_bytes,
        })
    }
}

fn decode_root(
    reader: &mut BinaryReader<'_>,
    type_tree: &TypeTreeLayout<'_>,
    arrays_align: bool,
    object_index: usize,
    root_name: &str,
    budget: &mut DecodeBudget<'_>,
) -> Result<SerializedValue, ObjectDecodeError> {
    for (node_index, node) in type_tree.nodes.iter().enumerate() {
        let children = direct_children(type_tree, node_index, object_index, reader.offset())?;
        let type_name = node.type_name.as_deref().unwrap_or("");
        let node_name = node.name.as_deref().unwrap_or("");
        // `TypeTreeNodeStruct.IsManagedReference`: a `managedReference` (or array-item)
        // node holding one `rid` (or legacy `id`) child.
        let managed_reference_node =
            is_managed_reference_element(type_tree, node_index, object_index, reader.offset())?;
        // `TypeTreeNodeStruct.IsManagedReferencesRegistry`.
        let registry_node = type_name == managed_names::REGISTRY_NAME
            && node_name == managed_names::REGISTRY_FIELD_NAME
            && children.len() > 1;
        let dynamic_managed_data = type_name == managed_names::REFERENCED_OBJECT_DATA_NAME
            && node_name == managed_names::DATA_FIELD_NAME;
        if managed_reference_node || registry_node || dynamic_managed_data {
            // `TypeTreeNodeStruct.IsIndexedManagedReference` selects the legacy `id`
            // field shape over the `rid`-addressed shape for the rejection report,
            // mirroring the source field-shape selection.
            let is_indexed = managed_reference_node
                && is_indexed_managed_reference_element(
                    type_tree,
                    node_index,
                    object_index,
                    reader.offset(),
                )?;
            return Err(ObjectDecodeError::new(
                reader.offset(),
                ObjectDecodeErrorKind::UnsupportedManagedReferences {
                    object_index,
                    type_name: type_name.to_owned(),
                    field_name: node_name.to_owned(),
                    is_indexed,
                },
            ));
        }
    }
    decode_node_value(
        reader,
        type_tree,
        0,
        arrays_align,
        object_index,
        root_name,
        0,
        false,
        budget,
    )
}

// Keep the reader, tree, field identity, version rule, and allocation budget explicit at this
// parser boundary; bundling these per-call values in mutable global state would hide provenance.
#[allow(clippy::too_many_arguments)]
fn decode_field(
    reader: &mut BinaryReader<'_>,
    type_tree: &TypeTreeLayout<'_>,
    node_index: usize,
    arrays_align: bool,
    object_index: usize,
    parent_path: &str,
    depth: usize,
    budget: &mut DecodeBudget<'_>,
) -> Result<SerializedFieldValue, ObjectDecodeError> {
    let node = type_tree.nodes.get(node_index).ok_or_else(|| {
        ObjectDecodeError::new(
            reader.offset(),
            ObjectDecodeErrorKind::InvalidTypeTreeNodeIndex { node_index },
        )
    })?;
    let field_name = node.name.as_deref().unwrap_or("");
    let path = if parent_path.is_empty() {
        field_name.to_owned()
    } else if field_name.is_empty() {
        parent_path.to_owned()
    } else {
        format!("{parent_path}.{field_name}")
    };
    let align_after = field_aligns(type_tree, node_index, object_index, reader.offset())?;
    let value = decode_node_value(
        reader,
        type_tree,
        node_index,
        arrays_align,
        object_index,
        &path,
        depth,
        align_after,
        budget,
    )?;
    if align_after {
        reader
            .align_relative(4)
            .map_err(|error| binary_error(error, &format!("{path} alignment")))?;
    }
    Ok(SerializedFieldValue {
        name: field_name.to_owned(),
        value,
    })
}

#[allow(clippy::too_many_arguments)]
fn decode_node_value(
    reader: &mut BinaryReader<'_>,
    type_tree: &TypeTreeLayout<'_>,
    node_index: usize,
    arrays_align: bool,
    object_index: usize,
    path: &str,
    depth: usize,
    align_nested_arrays: bool,
    budget: &mut DecodeBudget<'_>,
) -> Result<SerializedValue, ObjectDecodeError> {
    if depth > budget.limits.max_type_tree_depth {
        return Err(ObjectDecodeError::new(
            reader.offset(),
            ObjectDecodeErrorKind::TypeTreeDepthExceeded {
                object_index,
                path: path.to_owned(),
                depth,
            },
        ));
    }
    let node = type_tree.nodes.get(node_index).ok_or_else(|| {
        ObjectDecodeError::new(
            reader.offset(),
            ObjectDecodeErrorKind::InvalidTypeTreeNodeIndex { node_index },
        )
    })?;
    let type_name = node.type_name.as_deref().unwrap_or("");

    if matches!(type_name, "string" | "Utf8String")
        && is_string_node(type_tree, node_index, object_index, reader.offset())?
    {
        let length = read_i32(reader, &format!("{path} string byte length"))?;
        let length = nonnegative_length(length, reader, object_index, path)?;
        if length > budget.limits.max_string_bytes {
            return Err(decode_limit_error(
                reader.offset(),
                format!("{path} string bytes"),
                length,
                budget.limits.max_string_bytes,
            ));
        }
        if length > reader.remaining() {
            return Err(ObjectDecodeError::new(
                reader.offset(),
                ObjectDecodeErrorKind::LengthExceedsPayload {
                    object_index,
                    path: path.to_owned(),
                    length,
                    remaining: reader.remaining(),
                },
            ));
        }
        budget.charge(length, reader.offset(), "decoded string bytes")?;
        let bytes = reader
            .read_exact(length)
            .map_err(|error| binary_error(error, &format!("{path} string bytes")))?
            .to_vec();
        reader
            .align_relative(4)
            .map_err(|error| binary_error(error, &format!("{path} string alignment")))?;
        return Ok(SerializedValue::StringBytes(bytes));
    }

    if type_name == "TypelessData"
        && array_shape(type_tree, node_index, object_index, reader.offset())?.is_some()
    {
        let length = read_i32(reader, &format!("{path} typeless data length"))?;
        let length = nonnegative_length(length, reader, object_index, path)?;
        if length > budget.limits.max_array_elements {
            return Err(decode_limit_error(
                reader.offset(),
                format!("{path} typeless data bytes"),
                length,
                budget.limits.max_array_elements,
            ));
        }
        if length > reader.remaining() {
            return Err(ObjectDecodeError::new(
                reader.offset(),
                ObjectDecodeErrorKind::LengthExceedsPayload {
                    object_index,
                    path: path.to_owned(),
                    length,
                    remaining: reader.remaining(),
                },
            ));
        }
        budget.charge(length, reader.offset(), "decoded typeless data bytes")?;
        let bytes = reader
            .read_exact(length)
            .map_err(|error| binary_error(error, &format!("{path} typeless data")))?;
        let result = SerializedValue::Bytes(bytes.to_vec());
        if arrays_align || align_nested_arrays {
            reader
                .align_relative(4)
                .map_err(|error| binary_error(error, &format!("{path} array alignment")))?;
        }
        return Ok(result);
    }

    if let Some(array_node_index) =
        array_node_for_value(type_tree, node_index, object_index, reader.offset())?
    {
        return decode_array(
            reader,
            type_tree,
            array_node_index,
            arrays_align,
            object_index,
            path,
            depth + 1,
            align_nested_arrays,
            budget,
        );
    }

    if let Some((file_id_node, path_id_node)) =
        pptr_shape(type_tree, node_index, object_index, reader.offset())?
    {
        let pointer_offset = reader.offset();
        let file_id = decode_field(
            reader,
            type_tree,
            file_id_node,
            arrays_align,
            object_index,
            path,
            depth + 1,
            budget,
        )?;
        let path_id = decode_field(
            reader,
            type_tree,
            path_id_node,
            arrays_align,
            object_index,
            path,
            depth + 1,
            budget,
        )?;
        let file_id_value = match file_id.value {
            SerializedValue::I32(value) => value,
            _ => {
                return Err(invalid_pptr_component(
                    pointer_offset,
                    object_index,
                    path,
                    &file_id.name,
                    type_tree.nodes[file_id_node]
                        .type_name
                        .as_deref()
                        .unwrap_or(""),
                ));
            }
        };
        let path_id_value = match path_id.value {
            SerializedValue::I32(value) => i64::from(value),
            SerializedValue::I64(value) => value,
            _ => {
                return Err(invalid_pptr_component(
                    pointer_offset,
                    object_index,
                    path,
                    &path_id.name,
                    type_tree.nodes[path_id_node]
                        .type_name
                        .as_deref()
                        .unwrap_or(""),
                ));
            }
        };
        return Ok(SerializedValue::PPtr {
            file_id: file_id_value,
            path_id: path_id_value,
        });
    }

    if let Some(value) = decode_primitive(
        reader,
        type_name,
        node.meta_flags & CHAR_PROPERTY_MASK_FLAG != 0,
        path,
    )? {
        return Ok(value);
    }

    let children = direct_children(type_tree, node_index, object_index, reader.offset())?;
    if !children.is_empty() {
        let mut fields = reserve_fields(children.len(), reader.offset(), path, budget)?;
        for child_index in children {
            fields.push(decode_field(
                reader,
                type_tree,
                child_index,
                arrays_align,
                object_index,
                path,
                depth + 1,
                budget,
            )?);
        }
        return Ok(SerializedValue::Struct(fields));
    }

    let raw_size = node.byte_size;
    let size = usize::try_from(raw_size).map_err(|_| {
        ObjectDecodeError::new(
            reader.offset(),
            ObjectDecodeErrorKind::UnknownLeafWithoutSize {
                object_index,
                path: path.to_owned(),
                type_name: type_name.to_owned(),
                byte_size: raw_size,
            },
        )
    })?;
    if size > reader.remaining() {
        return Err(ObjectDecodeError::new(
            reader.offset(),
            ObjectDecodeErrorKind::LengthExceedsPayload {
                object_index,
                path: path.to_owned(),
                length: size,
                remaining: reader.remaining(),
            },
        ));
    }
    budget.charge(size, reader.offset(), "unknown raw leaf bytes")?;
    reader
        .read_exact(size)
        .map(|bytes| SerializedValue::Raw {
            type_name: type_name.to_owned(),
            bytes: bytes.to_vec(),
        })
        .map_err(|error| binary_error(error, &format!("{path} raw field")))
}

#[allow(clippy::too_many_arguments)]
fn decode_array(
    reader: &mut BinaryReader<'_>,
    type_tree: &TypeTreeLayout<'_>,
    array_node_index: usize,
    arrays_align: bool,
    object_index: usize,
    path: &str,
    depth: usize,
    align_nested_arrays: bool,
    budget: &mut DecodeBudget<'_>,
) -> Result<SerializedValue, ObjectDecodeError> {
    let (size_node, data_node) =
        array_shape(type_tree, array_node_index, object_index, reader.offset())?.ok_or_else(
            || {
                ObjectDecodeError::new(
                    reader.offset(),
                    ObjectDecodeErrorKind::InvalidArrayShape {
                        object_index,
                        path: path.to_owned(),
                        node_index: array_node_index,
                    },
                )
            },
        )?;
    let count = read_i32(reader, &format!("{path} array count"))?;
    let count = nonnegative_length(count, reader, object_index, path)?;
    if count > budget.limits.max_array_elements {
        return Err(decode_limit_error(
            reader.offset(),
            format!("{path} array elements"),
            count,
            budget.limits.max_array_elements,
        ));
    }
    let minimum_size =
        minimum_encoded_size(type_tree, data_node, object_index, reader.offset(), 0)?;
    if count > 0 && minimum_size == 0 {
        return Err(ObjectDecodeError::new(
            reader.offset(),
            ObjectDecodeErrorKind::ZeroWidthArrayElement {
                object_index,
                path: path.to_owned(),
                count,
            },
        ));
    }
    let minimum_bytes = count.checked_mul(minimum_size).ok_or_else(|| {
        ObjectDecodeError::new(
            reader.offset(),
            ObjectDecodeErrorKind::ArraySizeOverflow {
                object_index,
                path: path.to_owned(),
                count,
                minimum_element_size: minimum_size,
            },
        )
    })?;
    if minimum_bytes > reader.remaining() {
        return Err(ObjectDecodeError::new(
            reader.offset(),
            ObjectDecodeErrorKind::ArrayExceedsPayload {
                object_index,
                path: path.to_owned(),
                count,
                minimum_element_size: minimum_size,
                required: minimum_bytes,
                remaining: reader.remaining(),
            },
        ));
    }

    let mut values = reserve_values(count, reader.offset(), path, budget)?;
    for index in 0..count {
        let element_path = format!("{path}[{index}]");
        values.push(decode_node_value(
            reader,
            type_tree,
            data_node,
            arrays_align,
            object_index,
            &element_path,
            depth + 1,
            align_nested_arrays,
            budget,
        )?);
    }
    if align_nested_arrays
        || (arrays_align
            && array_base_is_primitive(type_tree, data_node, object_index, reader.offset())?)
    {
        reader
            .align_relative(4)
            .map_err(|error| binary_error(error, &format!("{path} array alignment")))?;
    }
    let _ = size_node;
    Ok(SerializedValue::Array(values))
}

fn decode_primitive(
    reader: &mut BinaryReader<'_>,
    type_name: &str,
    treat_integer_as_char: bool,
    path: &str,
) -> Result<Option<SerializedValue>, ObjectDecodeError> {
    let result = match type_name {
        "bool" => Some(SerializedValue::Bool(read_u8(reader, path)? != 0)),
        "char" | "UInt8" => Some(SerializedValue::U8(read_u8(reader, path)?)),
        "SInt8" => Some(SerializedValue::I8(read_i8(reader, path)?)),
        "short" | "SInt16" => Some(SerializedValue::I16(read_i16(reader, path)?)),
        "ushort" | "UInt16" | "unsigned short" => Some(if treat_integer_as_char {
            SerializedValue::Char(read_u16(reader, path)?)
        } else {
            SerializedValue::U16(read_u16(reader, path)?)
        }),
        "int" | "SInt32" | "Type*" => Some(SerializedValue::I32(read_i32(reader, path)?)),
        "uint" | "UInt32" | "unsigned int" => Some(SerializedValue::U32(read_u32(reader, path)?)),
        "SInt64" | "long long" => Some(SerializedValue::I64(read_i64(reader, path)?)),
        "UInt64" | "FileSize" | "unsigned long long" => {
            Some(SerializedValue::U64(read_u64(reader, path)?))
        }
        "float" => Some(SerializedValue::F32(read_f32(reader, path)?)),
        "double" => Some(SerializedValue::F64(read_f64(reader, path)?)),
        _ => None,
    };
    Ok(result)
}

fn array_node_for_value(
    type_tree: &TypeTreeLayout<'_>,
    node_index: usize,
    object_index: usize,
    offset: usize,
) -> Result<Option<usize>, ObjectDecodeError> {
    if array_shape(type_tree, node_index, object_index, offset)?.is_some() {
        return Ok(Some(node_index));
    }
    let node = &type_tree.nodes[node_index];
    let children = direct_children(type_tree, node_index, object_index, offset)?;
    if children.len() != 1 {
        return Ok(None);
    }
    let child_index = children[0];
    let child = &type_tree.nodes[child_index];
    let is_vector = matches!(
        node.type_name.as_deref(),
        Some("vector" | "staticvector" | "set" | "map")
    );
    let is_named_vector = if child.name.as_deref() == Some("Array") {
        if let Some((_, element_index)) = array_shape(type_tree, child_index, object_index, offset)?
        {
            let element_type = type_tree.nodes[element_index].type_name.as_deref();
            element_type == node.type_name.as_deref()
                || element_type == Some("Generic Mono")
                || is_managed_reference_element(type_tree, element_index, object_index, offset)?
        } else {
            false
        }
    } else {
        false
    };
    if (is_vector || is_named_vector)
        && array_shape(type_tree, child_index, object_index, offset)?.is_some()
    {
        Ok(Some(child_index))
    } else {
        Ok(None)
    }
}

fn array_shape(
    type_tree: &TypeTreeLayout<'_>,
    node_index: usize,
    object_index: usize,
    offset: usize,
) -> Result<Option<(usize, usize)>, ObjectDecodeError> {
    let node = type_tree.nodes.get(node_index).ok_or_else(|| {
        ObjectDecodeError::new(
            offset,
            ObjectDecodeErrorKind::InvalidTypeTreeNodeIndex { node_index },
        )
    })?;
    if !matches!(node.type_name.as_deref(), Some("Array" | "TypelessData")) {
        return Ok(None);
    }
    let children = direct_children(type_tree, node_index, object_index, offset)?;
    if children.len() != 2
        || type_tree.nodes[children[0]].name.as_deref() != Some("size")
        || type_tree.nodes[children[1]].name.as_deref() != Some("data")
        || !direct_children(type_tree, children[0], object_index, offset)?.is_empty()
    {
        return Ok(None);
    }
    Ok(Some((children[0], children[1])))
}

fn is_string_node(
    type_tree: &TypeTreeLayout<'_>,
    node_index: usize,
    object_index: usize,
    offset: usize,
) -> Result<bool, ObjectDecodeError> {
    let children = direct_children(type_tree, node_index, object_index, offset)?;
    if children.len() != 1 {
        return Ok(false);
    }
    let Some((_, data_node)) = array_shape(type_tree, children[0], object_index, offset)? else {
        return Ok(false);
    };
    let data = &type_tree.nodes[data_node];
    Ok(matches!(data.type_name.as_deref(), Some("char" | "UInt8"))
        && direct_children(type_tree, data_node, object_index, offset)?.is_empty())
}

/// Matches AssetRipper's TypeTree PPtr shape test: a `PPtr<...>` node with exactly two leaf
/// children named `m_FileID` and `m_PathID` in that order.
fn pptr_shape(
    type_tree: &TypeTreeLayout<'_>,
    node_index: usize,
    object_index: usize,
    offset: usize,
) -> Result<Option<(usize, usize)>, ObjectDecodeError> {
    let node = type_tree.nodes.get(node_index).ok_or_else(|| {
        ObjectDecodeError::new(
            offset,
            ObjectDecodeErrorKind::InvalidTypeTreeNodeIndex { node_index },
        )
    })?;
    let type_name = type_name(node);
    if !type_name.starts_with("PPtr<") || !type_name.ends_with('>') {
        return Ok(None);
    }
    let children = direct_children(type_tree, node_index, object_index, offset)?;
    if children.len() != 2
        || type_tree.nodes[children[0]].name.as_deref() != Some("m_FileID")
        || type_tree.nodes[children[1]].name.as_deref() != Some("m_PathID")
        || !direct_children(type_tree, children[0], object_index, offset)?.is_empty()
        || !direct_children(type_tree, children[1], object_index, offset)?.is_empty()
    {
        return Ok(None);
    }
    Ok(Some((children[0], children[1])))
}

fn invalid_pptr_component(
    offset: usize,
    object_index: usize,
    path: &str,
    field_name: &str,
    type_name: &str,
) -> ObjectDecodeError {
    ObjectDecodeError::new(
        offset,
        ObjectDecodeErrorKind::InvalidPPtrComponent {
            object_index,
            path: path.to_owned(),
            field_name: field_name.to_owned(),
            type_name: type_name.to_owned(),
        },
    )
}

fn field_aligns(
    type_tree: &TypeTreeLayout<'_>,
    node_index: usize,
    object_index: usize,
    offset: usize,
) -> Result<bool, ObjectDecodeError> {
    let mut current = node_index;
    let mut aligned = false;
    let mut visited = 0;
    loop {
        let node = type_tree.nodes.get(current).ok_or_else(|| {
            ObjectDecodeError::new(
                offset,
                ObjectDecodeErrorKind::InvalidTypeTreeNodeIndex {
                    node_index: current,
                },
            )
        })?;
        aligned |= node.meta_flags & ALIGN_BYTES_FLAG != 0;
        if visited > MAX_TYPE_TREE_DEPTH {
            return Err(ObjectDecodeError::new(
                offset,
                ObjectDecodeErrorKind::TypeTreeDepthExceeded {
                    object_index,
                    path: format!("TypeTree node {node_index}"),
                    depth: visited,
                },
            ));
        }
        visited += 1;
        if let Some((_, data_node)) = array_shape(type_tree, current, object_index, offset)? {
            current = data_node;
            continue;
        }
        let node = &type_tree.nodes[current];
        let children = direct_children(type_tree, current, object_index, offset)?;
        if children.len() == 1
            && (matches!(
                node.type_name.as_deref(),
                Some("vector" | "staticvector" | "set" | "map")
            ) || (type_tree.nodes[children[0]].name.as_deref() == Some("Array")
                && array_shape(type_tree, children[0], object_index, offset)?.is_some()))
        {
            current = children[0];
            continue;
        }
        return Ok(aligned);
    }
}

fn array_base_is_primitive(
    type_tree: &TypeTreeLayout<'_>,
    node_index: usize,
    object_index: usize,
    offset: usize,
) -> Result<bool, ObjectDecodeError> {
    let mut current = node_index;
    for _ in 0..=MAX_TYPE_TREE_DEPTH {
        if matches!(
            type_tree.nodes[current].type_name.as_deref(),
            Some("string" | "Utf8String")
        ) && is_string_node(type_tree, current, object_index, offset)?
        {
            return Ok(true);
        }
        if decode_primitive_name(type_name(&type_tree.nodes[current])).is_some() {
            return Ok(true);
        }
        if let Some((_, data_node)) = array_shape(type_tree, current, object_index, offset)? {
            current = data_node;
            continue;
        }
        let children = direct_children(type_tree, current, object_index, offset)?;
        if children.len() == 1 {
            current = children[0];
            continue;
        }
        return Ok(false);
    }
    Err(ObjectDecodeError::new(
        offset,
        ObjectDecodeErrorKind::TypeTreeDepthExceeded {
            object_index,
            path: format!("TypeTree node {node_index}"),
            depth: MAX_TYPE_TREE_DEPTH + 1,
        },
    ))
}

fn minimum_encoded_size(
    type_tree: &TypeTreeLayout<'_>,
    node_index: usize,
    object_index: usize,
    offset: usize,
    depth: usize,
) -> Result<usize, ObjectDecodeError> {
    if depth > MAX_TYPE_TREE_DEPTH {
        return Err(ObjectDecodeError::new(
            offset,
            ObjectDecodeErrorKind::TypeTreeDepthExceeded {
                object_index,
                path: format!("TypeTree node {node_index}"),
                depth,
            },
        ));
    }
    let node = type_tree.nodes.get(node_index).ok_or_else(|| {
        ObjectDecodeError::new(
            offset,
            ObjectDecodeErrorKind::InvalidTypeTreeNodeIndex { node_index },
        )
    })?;
    if matches!(node.type_name.as_deref(), Some("string" | "Utf8String"))
        && is_string_node(type_tree, node_index, object_index, offset)?
    {
        return Ok(4);
    }
    if array_shape(type_tree, node_index, object_index, offset)?.is_some()
        || array_node_for_value(type_tree, node_index, object_index, offset)?.is_some()
    {
        return Ok(4);
    }
    if let Some(size) = primitive_width(type_name(node)) {
        return Ok(size);
    }
    let children = direct_children(type_tree, node_index, object_index, offset)?;
    if !children.is_empty() {
        let mut total = 0usize;
        for child in children {
            total = total
                .checked_add(minimum_encoded_size(
                    type_tree,
                    child,
                    object_index,
                    offset,
                    depth + 1,
                )?)
                .ok_or_else(|| {
                    ObjectDecodeError::new(offset, ObjectDecodeErrorKind::LayoutOverflow)
                })?;
        }
        return Ok(total);
    }
    usize::try_from(node.byte_size).map_err(|_| {
        ObjectDecodeError::new(
            offset,
            ObjectDecodeErrorKind::UnknownLeafWithoutSize {
                object_index,
                path: format!("TypeTree node {node_index}"),
                type_name: type_name(node).to_owned(),
                byte_size: node.byte_size,
            },
        )
    })
}

/// `TypeTreeNodeStruct.IsManagedReference` for an array element node: the element type
/// is `managedReference` (or `managedRefArrayItem`) holding one `rid` (or legacy
/// `id`) child. This is the predicate `IsNamedVector` applies to SerializeReference
/// array elements.
fn is_managed_reference_element(
    type_tree: &TypeTreeLayout<'_>,
    node_index: usize,
    object_index: usize,
    offset: usize,
) -> Result<bool, ObjectDecodeError> {
    let node = &type_tree.nodes[node_index];
    if !matches!(
        node.type_name.as_deref(),
        Some(
            managed_names::MANAGED_REFERENCE_NAME
                | managed_names::MANAGED_REFERENCE_ARRAY_ITEM_NAME
        )
    ) {
        return Ok(false);
    }
    let children = direct_children(type_tree, node_index, object_index, offset)?;
    Ok(children.len() == 1
        && matches!(
            type_tree.nodes[children[0]].name.as_deref(),
            Some(managed_names::REFERENCE_ID_FIELD_NAME | managed_names::INDEX_FIELD_NAME)
        ))
}

/// `TypeTreeNodeStruct.IsIndexedManagedReference` for an array element node: a
/// managed-reference element (see `is_managed_reference_element`) whose identifier
/// child is the legacy `id` field. The source uses this to select the
/// `IndexedManagedReference` field shape over the `rid`-addressed
/// `ManagedReference` shape.
fn is_indexed_managed_reference_element(
    type_tree: &TypeTreeLayout<'_>,
    node_index: usize,
    object_index: usize,
    offset: usize,
) -> Result<bool, ObjectDecodeError> {
    if !is_managed_reference_element(type_tree, node_index, object_index, offset)? {
        return Ok(false);
    }
    let children = direct_children(type_tree, node_index, object_index, offset)?;
    Ok(children.len() == 1
        && type_tree.nodes[children[0]].name.as_deref() == Some(managed_names::INDEX_FIELD_NAME))
}

fn direct_children(
    type_tree: &TypeTreeLayout<'_>,
    node_index: usize,
    _object_index: usize,
    offset: usize,
) -> Result<Vec<usize>, ObjectDecodeError> {
    let mut children = Vec::new();
    let mut child = *type_tree.first_child.get(node_index).ok_or_else(|| {
        ObjectDecodeError::new(
            offset,
            ObjectDecodeErrorKind::InvalidTypeTreeNodeIndex { node_index },
        )
    })?;
    while let Some(child_index) = child {
        children
            .try_reserve(1)
            .map_err(|_| allocation_error(offset, "TypeTree children", children.len() + 1))?;
        children.push(child_index);
        child = type_tree.next_sibling[child_index];
    }
    Ok(children)
}

fn reserve_option_indices(
    count: usize,
    offset: usize,
) -> Result<Vec<Option<usize>>, ObjectDecodeError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| allocation_error(offset, "TypeTree child index", count))?;
    values.resize(count, None);
    Ok(values)
}

fn invalid_tree_levels(
    offset: usize,
    object_index: usize,
    node_index: usize,
    child_index: usize,
) -> ObjectDecodeError {
    ObjectDecodeError::new(
        offset,
        ObjectDecodeErrorKind::InvalidTypeTreeLevels {
            object_index,
            node_index,
            child_index,
        },
    )
}

fn reserve_values(
    count: usize,
    offset: usize,
    path: &str,
    budget: &mut DecodeBudget<'_>,
) -> Result<Vec<SerializedValue>, ObjectDecodeError> {
    budget.charge_items::<SerializedValue>(count, offset, path)?;
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| allocation_error(offset, path, count))?;
    Ok(values)
}

fn reserve_fields(
    count: usize,
    offset: usize,
    path: &str,
    budget: &mut DecodeBudget<'_>,
) -> Result<Vec<SerializedFieldValue>, ObjectDecodeError> {
    budget.charge_items::<SerializedFieldValue>(count, offset, path)?;
    let mut fields = Vec::new();
    fields
        .try_reserve_exact(count)
        .map_err(|_| allocation_error(offset, path, count))?;
    Ok(fields)
}

fn nonnegative_length(
    value: i32,
    reader: &BinaryReader<'_>,
    object_index: usize,
    path: &str,
) -> Result<usize, ObjectDecodeError> {
    usize::try_from(value).map_err(|_| {
        ObjectDecodeError::new(
            reader.offset().saturating_sub(4),
            ObjectDecodeErrorKind::NegativeArrayLength {
                object_index,
                path: path.to_owned(),
                length: value,
            },
        )
    })
}

fn primitive_width(type_name: &str) -> Option<usize> {
    match type_name {
        "bool" | "char" | "UInt8" | "SInt8" => Some(1),
        "short" | "SInt16" | "ushort" | "UInt16" | "unsigned short" => Some(2),
        "int" | "SInt32" | "Type*" | "uint" | "UInt32" | "unsigned int" | "float" => Some(4),
        "SInt64" | "long long" | "UInt64" | "FileSize" | "unsigned long long" | "double" => Some(8),
        _ => None,
    }
}

fn decode_primitive_name(type_name: &str) -> Option<()> {
    primitive_width(type_name).map(|_| ())
}

fn type_name(node: &super::metadata::TypeTreeNode) -> &str {
    node.type_name.as_deref().unwrap_or("")
}

fn allocation_error(offset: usize, path: &str, count: usize) -> ObjectDecodeError {
    ObjectDecodeError::new(
        offset,
        ObjectDecodeErrorKind::AllocationFailed {
            path: path.to_owned(),
            count,
        },
    )
}

fn decode_limit_error(
    offset: usize,
    resource: String,
    requested: usize,
    limit: usize,
) -> ObjectDecodeError {
    ObjectDecodeError::new(
        offset,
        ObjectDecodeErrorKind::DecodeLimitExceeded {
            resource,
            requested,
            limit,
        },
    )
}

macro_rules! read_primitive {
    ($name:ident, $ty:ty, $reader_method:ident) => {
        fn $name(reader: &mut BinaryReader<'_>, context: &str) -> Result<$ty, ObjectDecodeError> {
            reader
                .$reader_method()
                .map_err(|error| binary_error(error, context))
        }
    };
}

read_primitive!(read_u8, u8, read_u8);
read_primitive!(read_i8, i8, read_i8);
read_primitive!(read_u16, u16, read_u16);
read_primitive!(read_i16, i16, read_i16);
read_primitive!(read_u32, u32, read_u32);
read_primitive!(read_i32, i32, read_i32);
read_primitive!(read_u64, u64, read_u64);
read_primitive!(read_i64, i64, read_i64);
read_primitive!(read_f32, f32, read_f32);
read_primitive!(read_f64, f64, read_f64);

fn binary_error(error: BinaryError, context: &str) -> ObjectDecodeError {
    let offset = error.offset();
    ObjectDecodeError::new(
        offset,
        ObjectDecodeErrorKind::Binary(error.with_context(context.to_owned())),
    )
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectDecodeError {
    offset: usize,
    kind: ObjectDecodeErrorKind,
}

impl ObjectDecodeError {
    fn new(offset: usize, kind: ObjectDecodeErrorKind) -> Self {
        Self { offset, kind }
    }

    fn from_binary(error: BinaryError) -> Self {
        let offset = error.offset();
        Self::new(offset, ObjectDecodeErrorKind::Binary(error))
    }

    fn from_header(error: HeaderError) -> Self {
        let offset = error.offset();
        Self::new(offset, ObjectDecodeErrorKind::Header(error))
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn kind(&self) -> &ObjectDecodeErrorKind {
        &self.kind
    }
}

impl Display for ObjectDecodeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ObjectDecodeErrorKind::Binary(error) => Display::fmt(error, f),
            ObjectDecodeErrorKind::Header(error) => Display::fmt(error, f),
            kind => write!(f, "{kind} at byte offset {}", self.offset),
        }
    }
}

impl Error for ObjectDecodeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            ObjectDecodeErrorKind::Binary(error) => Some(error),
            ObjectDecodeErrorKind::Header(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObjectDecodeErrorKind {
    Binary(BinaryError),
    Header(HeaderError),
    InvalidObjectIndex {
        object_index: usize,
        object_count: usize,
    },
    ObjectTypeUnresolved {
        object_index: usize,
    },
    /// The file declares a Unity version before 3.5, whose assets the source
    /// does not read.
    UnreadableUnsupportedVersion {
        object_index: usize,
    },
    InvalidResolvedTypeIndex {
        object_index: usize,
        type_index: usize,
        type_count: usize,
    },
    MissingTypeTree {
        object_index: usize,
    },
    EmptyTypeTree {
        object_index: usize,
    },
    UnsupportedManagedReferences {
        object_index: usize,
        type_name: String,
        field_name: String,
        /// The legacy `id` flavor (`IsIndexedManagedReference`) versus `rid`-addressed.
        is_indexed: bool,
    },
    InvalidPPtrComponent {
        object_index: usize,
        path: String,
        field_name: String,
        type_name: String,
    },
    ObjectDataRangeOverflow {
        object_index: usize,
    },
    ObjectDataOutOfBounds {
        object_index: usize,
        start: u64,
        end: u64,
        file_size: usize,
    },
    InvalidTypeTreeNodeIndex {
        node_index: usize,
    },
    InvalidTypeTreeLevels {
        object_index: usize,
        node_index: usize,
        child_index: usize,
    },
    TypeTreeDepthExceeded {
        object_index: usize,
        path: String,
        depth: usize,
    },
    InvalidArrayShape {
        object_index: usize,
        path: String,
        node_index: usize,
    },
    NegativeArrayLength {
        object_index: usize,
        path: String,
        length: i32,
    },
    LengthExceedsPayload {
        object_index: usize,
        path: String,
        length: usize,
        remaining: usize,
    },
    ArraySizeOverflow {
        object_index: usize,
        path: String,
        count: usize,
        minimum_element_size: usize,
    },
    ArrayExceedsPayload {
        object_index: usize,
        path: String,
        count: usize,
        minimum_element_size: usize,
        required: usize,
        remaining: usize,
    },
    ZeroWidthArrayElement {
        object_index: usize,
        path: String,
        count: usize,
    },
    UnknownLeafWithoutSize {
        object_index: usize,
        path: String,
        type_name: String,
        byte_size: i32,
    },
    DecodeLimitExceeded {
        resource: String,
        requested: usize,
        limit: usize,
    },
    AllocationSizeOverflow {
        resource: String,
    },
    AllocationFailed {
        path: String,
        count: usize,
    },
    LayoutOverflow,
}

impl Display for ObjectDecodeErrorKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Binary(error) => Display::fmt(error, f),
            Self::Header(error) => Display::fmt(error, f),
            Self::InvalidObjectIndex {
                object_index,
                object_count,
            } => write!(
                f,
                "object index {object_index} is outside the {object_count}-entry object table"
            ),
            Self::ObjectTypeUnresolved { object_index } => {
                write!(f, "object {object_index} has no resolved serialized type")
            }
            Self::UnreadableUnsupportedVersion { object_index } => write!(
                f,
                "object {object_index} comes from a Unity version before 3.5 and cannot be read"
            ),
            Self::InvalidResolvedTypeIndex {
                object_index,
                type_index,
                type_count,
            } => write!(
                f,
                "object {object_index} resolves to type {type_index}, outside the {type_count}-entry type table"
            ),
            Self::MissingTypeTree { object_index } => {
                write!(f, "object {object_index} has no TypeTree")
            }
            Self::EmptyTypeTree { object_index } => {
                write!(f, "object {object_index} references an empty TypeTree")
            }
            Self::UnsupportedManagedReferences {
                object_index,
                type_name,
                field_name,
                is_indexed,
            } => write!(
                f,
                "object {object_index} contains {} managed-reference TypeTree node {type_name} {field_name}; registry decoding is not implemented",
                if *is_indexed {
                    "legacy indexed"
                } else {
                    "rid-addressed"
                }
            ),
            Self::InvalidPPtrComponent {
                object_index,
                path,
                field_name,
                type_name,
            } => write!(
                f,
                "object {object_index} field {path} has unsupported PPtr component {field_name} of type {type_name}"
            ),
            Self::ObjectDataRangeOverflow { object_index } => {
                write!(f, "object {object_index} data range overflows the address space")
            }
            Self::ObjectDataOutOfBounds {
                object_index,
                start,
                end,
                file_size,
            } => write!(
                f,
                "object {object_index} payload range {start}..{end} exceeds input size {file_size}"
            ),
            Self::InvalidTypeTreeNodeIndex { node_index } => {
                write!(f, "TypeTree node index {node_index} is out of bounds")
            }
            Self::InvalidTypeTreeLevels {
                object_index,
                node_index,
                child_index,
            } => write!(
                f,
                "object {object_index} TypeTree node {child_index} has an invalid level relative to node {node_index}"
            ),
            Self::TypeTreeDepthExceeded {
                object_index,
                path,
                depth,
            } => write!(
                f,
                "object {object_index} field {path} exceeds TypeTree decode depth at {depth}"
            ),
            Self::InvalidArrayShape {
                object_index,
                path,
                node_index,
            } => write!(
                f,
                "object {object_index} field {path} has an invalid array node at index {node_index}"
            ),
            Self::NegativeArrayLength {
                object_index,
                path,
                length,
            } => write!(
                f,
                "object {object_index} field {path} has negative array/string length {length}"
            ),
            Self::LengthExceedsPayload {
                object_index,
                path,
                length,
                remaining,
            } => write!(
                f,
                "object {object_index} field {path} requires {length} bytes, but only {remaining} remain"
            ),
            Self::ArraySizeOverflow {
                object_index,
                path,
                count,
                minimum_element_size,
            } => write!(
                f,
                "object {object_index} field {path} array size overflows for {count} elements of minimum size {minimum_element_size}"
            ),
            Self::ArrayExceedsPayload {
                object_index,
                path,
                count,
                minimum_element_size,
                required,
                remaining,
            } => write!(
                f,
                "object {object_index} field {path} array count {count} requires at least {required} bytes ({minimum_element_size} per element), but only {remaining} remain"
            ),
            Self::ZeroWidthArrayElement {
                object_index,
                path,
                count,
            } => write!(
                f,
                "object {object_index} field {path} has {count} array elements with no bounded serialized width"
            ),
            Self::UnknownLeafWithoutSize {
                object_index,
                path,
                type_name,
                byte_size,
            } => write!(
                f,
                "object {object_index} field {path} has unknown leaf type {type_name} with invalid byte size {byte_size}"
            ),
            Self::DecodeLimitExceeded {
                resource,
                requested,
                limit,
            } => write!(
                f,
                "object payload decode requires {requested} units for {resource}, above the configured limit {limit}"
            ),
            Self::AllocationSizeOverflow { resource } => {
                write!(f, "decoded allocation size overflows while reserving {resource}")
            }
            Self::AllocationFailed { path, count } => {
                write!(f, "could not reserve {count} values for field {path}")
            }
            Self::LayoutOverflow => write!(f, "object payload layout arithmetic overflowed"),
        }
    }
}

/// Raw `BuildTarget` value meaning "no target".
///
/// Mirrors `BuildTarget.NoTarget` (`0xFFFFFFFE`, from
/// `AssetRipper/Source/AssetRipper.IO.Files/BuildTarget.cs`). The full
/// platform-enum mapping stays open (see `SER-META-PLATFORM-001`); the flags
/// kernel only needs this one comparison, exactly like `SerializedFile.Flags`.
pub const BUILD_TARGET_NO_TARGET: u32 = 0xFFFFFFFE;

/// First engine default-resource file name in `SpecialFileNames`.
pub const DEFAULT_RESOURCE_NAME_1: &str = "unity default resources";
/// Second engine default-resource file name in `SpecialFileNames`.
pub const DEFAULT_RESOURCE_NAME_2: &str = "unity_default_resources";
/// Engine editor-resource file name in `SpecialFileNames`.
pub const EDITOR_RESOURCE_NAME: &str = "unity editor resources";
/// First builtin-extra file name in `SpecialFileNames`.
pub const BUILTIN_EXTRA_NAME_1: &str = "unity builtin extra";
/// Second builtin-extra file name in `SpecialFileNames`.
pub const BUILTIN_EXTRA_NAME_2: &str = "unity_builtin_extra";

/// `SpecialFileNames.IsEngineResource` over a caller-supplied file name.
///
/// The source compares with ordinal, case-sensitive equality against the raw
/// `FileBase.Name` (not `NameFixed`), so this does the same: callers must pass
/// the raw name unchanged. A `FixFileIdentifier` output such as
/// `"library/unity default resources"` therefore never matches here.
pub fn is_engine_resource_name(file_name: &str) -> bool {
    file_name == DEFAULT_RESOURCE_NAME_1
        || file_name == DEFAULT_RESOURCE_NAME_2
        || file_name == EDITOR_RESOURCE_NAME
}

/// `SpecialFileNames.IsBuiltinExtra` over a caller-supplied file name.
///
/// Same exact-match semantics as [`is_engine_resource_name`].
pub fn is_builtin_extra_name(file_name: &str) -> bool {
    file_name == BUILTIN_EXTRA_NAME_1 || file_name == BUILTIN_EXTRA_NAME_2
}

/// Transfer flags governing how a SerializedFile's objects are read.
///
/// Mirrors `SerializedFile.Flags`
/// (`AssetRipper/Source/AssetRipper.IO.Files/SerializedFiles/SerializedFile.cs`):
/// a `NoTarget` platform on a platform-carrying generation reads editor scenes
/// (`.unity` paths) as `SerializeEditorMinimalScene` and anything else with bare
/// flags, while every other file reads as `SerializeGameRelease`. Engine
/// resources (and, below generation 10, builtin-extra files) additionally set
/// `IsBuiltinResourcesFile`, and big-endian bodies set `SwapEndianess`.
///
/// Caller facts: `generation` is the file generation (`HasPlatform` is
/// `generation >= Unknown_8`, shared with the metadata prefix reader);
/// `platform` is the parsed target-platform value (`None` when the generation
/// carries no platform field — only `Some(BUILD_TARGET_NO_TARGET)` on a
/// platform-carrying generation takes the scene branch); `file_path` is the raw
/// `FileBase.FilePath` (the `.unity` check is an ordinal, case-sensitive suffix
/// on the full path, matching `EndsWith(".unity", Ordinal)`); `file_name` is
/// the raw `FileBase.Name` (not `NameFixed`); `body_endian` is the file body
/// endianness (the source's `EndianType` after `GetEndianType`).
///
/// The generated per-class release/editor read dispatch and the export
/// collections that carry `Flags` stay caller-side: this kernel only derives the
/// flag word. `SpecialFileNames.FixFileIdentifier` (`NameFixed`) normalization
/// is likewise out of scope here.
pub fn serialized_file_object_read_flags(
    generation: FormatVersion,
    platform: Option<u32>,
    file_path: &str,
    file_name: &str,
    body_endian: Endian,
) -> TransferInstructionFlags {
    let mut flags = if generation.has_platform() && platform == Some(BUILD_TARGET_NO_TARGET) {
        if file_path.ends_with(".unity") {
            TransferInstructionFlags::SERIALIZE_EDITOR_MINIMAL_SCENE
        } else {
            TransferInstructionFlags::NONE
        }
    } else {
        TransferInstructionFlags::SERIALIZE_GAME_RELEASE
    };
    let builtin_extra_below_generation_10 =
        generation.raw() < FormatVersion::Unknown10.raw() && is_builtin_extra_name(file_name);
    if is_engine_resource_name(file_name) || builtin_extra_below_generation_10 {
        flags = flags.with(TransferInstructionFlags::IS_BUILTIN_RESOURCES_FILE);
    }
    if body_endian == Endian::Big {
        flags = flags.with(TransferInstructionFlags::SWAP_ENDIANESS);
    }
    flags
}

#[cfg(test)]
mod tests {
    use super::{
        ObjectDecodeErrorKind, ObjectEncodeErrorKind, ObjectEncodeLimits, SerializedFieldValue,
        SerializedValue, SerializedValueLookupError,
    };
    use crate::io::binary::Endian;
    use crate::serialized_file::format_version::FormatVersion;
    use crate::serialized_file::header::SerializedFileHeader;
    use crate::serialized_file::metadata::{SerializedFileMetadata, TypeTree, TypeTreeNode};

    fn layout_node(level: u8, name: &str, type_name: &str) -> TypeTreeNode {
        TypeTreeNode {
            level,
            version: 1,
            type_flags: 0,
            type_string_offset: None,
            name_string_offset: None,
            type_name: Some(type_name.to_owned()),
            name: Some(name.to_owned()),
            byte_size: 0,
            index: 0,
            meta_flags: 0,
            reference_type_hash: None,
        }
    }

    fn element_layout(nodes: Vec<TypeTreeNode>) -> super::TypeTreeLayout<'static> {
        // The layout borrows the tree; leak the box so the test can hold it.
        let tree: &'static TypeTree = Box::leak(Box::new(TypeTree {
            nodes,
            string_buffer: Vec::new(),
        }));
        super::TypeTreeLayout::new(tree, 0, 0).unwrap()
    }

    #[test]
    fn managed_reference_elements_require_the_full_source_predicate() {
        // `managedReference` holding one `rid` child matches.
        let layout = element_layout(vec![
            layout_node(0, "item", "managedReference"),
            layout_node(1, "rid", "SInt64"),
        ]);
        assert!(super::is_managed_reference_element(&layout, 0, 0, 0).unwrap());
        // The legacy `id` child matches too.
        let layout = element_layout(vec![
            layout_node(0, "item", "managedRefArrayItem"),
            layout_node(1, "id", "SInt32"),
        ]);
        assert!(super::is_managed_reference_element(&layout, 0, 0, 0).unwrap());
        // A wrong child name does not match.
        let layout = element_layout(vec![
            layout_node(0, "item", "managedReference"),
            layout_node(1, "value", "SInt64"),
        ]);
        assert!(!super::is_managed_reference_element(&layout, 0, 0, 0).unwrap());
        // Two children do not match.
        let layout = element_layout(vec![
            layout_node(0, "item", "managedReference"),
            layout_node(1, "rid", "SInt64"),
            layout_node(1, "extra", "SInt32"),
        ]);
        assert!(!super::is_managed_reference_element(&layout, 0, 0, 0).unwrap());
        // No children do not match.
        let layout = element_layout(vec![layout_node(0, "item", "managedRefArrayItem")]);
        assert!(!super::is_managed_reference_element(&layout, 0, 0, 0).unwrap());
        // Other element types never match, even with a `rid` child.
        let layout = element_layout(vec![
            layout_node(0, "item", "Generic Mono"),
            layout_node(1, "rid", "SInt64"),
        ]);
        assert!(!super::is_managed_reference_element(&layout, 0, 0, 0).unwrap());
    }

    #[test]
    fn indexed_managed_references_require_the_legacy_identifier() {
        // The legacy `id` child selects the indexed shape.
        let layout = element_layout(vec![
            layout_node(0, "item", "managedReference"),
            layout_node(1, "id", "SInt32"),
        ]);
        assert!(super::is_indexed_managed_reference_element(&layout, 0, 0, 0).unwrap());
        // The modern `rid` child does not.
        let layout = element_layout(vec![
            layout_node(0, "item", "managedReference"),
            layout_node(1, "rid", "SInt64"),
        ]);
        assert!(!super::is_indexed_managed_reference_element(&layout, 0, 0, 0).unwrap());
        // The array-item spelling participates as well.
        let layout = element_layout(vec![
            layout_node(0, "item", "managedRefArrayItem"),
            layout_node(1, "id", "SInt32"),
        ]);
        assert!(super::is_indexed_managed_reference_element(&layout, 0, 0, 0).unwrap());
        // Non-reference elements never select the indexed shape.
        let layout = element_layout(vec![
            layout_node(0, "item", "Generic Mono"),
            layout_node(1, "id", "SInt32"),
        ]);
        assert!(!super::is_indexed_managed_reference_element(&layout, 0, 0, 0).unwrap());
        // Two children do not match, even when the first is `id`.
        let layout = element_layout(vec![
            layout_node(0, "item", "managedReference"),
            layout_node(1, "id", "SInt32"),
            layout_node(1, "extra", "SInt32"),
        ]);
        assert!(!super::is_indexed_managed_reference_element(&layout, 0, 0, 0).unwrap());
    }

    #[test]
    fn structured_values_support_unique_fields_arrays_and_pointer_identity() {
        let value = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "target".to_owned(),
            value: SerializedValue::PPtr {
                file_id: 2,
                path_id: 41,
            },
        }]);
        let target = value.unique_field("target").unwrap().unwrap();
        assert_eq!(target.pptr(), Some((2, 41)));
        assert_eq!(value.unique_field("missing").unwrap(), None);
        assert_eq!(value.array_elements(), None);
        assert_eq!(target.struct_fields(), None);

        let array = SerializedValue::Array(vec![SerializedValue::U8(7)]);
        assert_eq!(array.array_elements().unwrap(), &[SerializedValue::U8(7)]);
        assert_eq!(array.pptr(), None);
    }

    #[test]
    fn structured_field_lookup_rejects_duplicate_names_and_non_structs() {
        let duplicate = SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "value".to_owned(),
                value: SerializedValue::I32(1),
            },
            SerializedFieldValue {
                name: "value".to_owned(),
                value: SerializedValue::I32(2),
            },
        ]);
        assert_eq!(
            duplicate.unique_field("value"),
            Err(SerializedValueLookupError::DuplicateField {
                name: "value".to_owned(),
                match_count: 2,
            })
        );
        assert_eq!(
            SerializedValue::Bool(true).unique_field("value"),
            Err(SerializedValueLookupError::NotStruct)
        );
    }

    #[test]
    fn inherited_field_lookup_follows_only_base_structs_and_detects_duplicates() {
        let inherited = SerializedValue::Struct(vec![super::SerializedFieldValue {
            name: "Base".to_owned(),
            value: SerializedValue::Struct(vec![super::SerializedFieldValue {
                name: "m_Name".to_owned(),
                value: SerializedValue::StringBytes(b"child".to_vec()),
            }]),
        }]);
        assert_eq!(
            inherited.unique_field_in_base_chain("m_Name").unwrap(),
            Some(&SerializedValue::StringBytes(b"child".to_vec()))
        );

        let duplicate = SerializedValue::Struct(vec![
            super::SerializedFieldValue {
                name: "m_Name".to_owned(),
                value: SerializedValue::StringBytes(b"derived".to_vec()),
            },
            super::SerializedFieldValue {
                name: "Base".to_owned(),
                value: SerializedValue::Struct(vec![super::SerializedFieldValue {
                    name: "m_Name".to_owned(),
                    value: SerializedValue::StringBytes(b"base".to_vec()),
                }]),
            },
        ]);
        assert_eq!(
            duplicate.unique_field_in_base_chain("m_Name"),
            Err(SerializedValueLookupError::DuplicateField {
                name: "m_Name".to_owned(),
                match_count: 2,
            })
        );
    }

    #[test]
    fn inherited_component_array_mutation_preserves_source_slots_and_checks_bounds() {
        let mut root = SerializedValue::Struct(vec![super::SerializedFieldValue {
            name: "Base".to_owned(),
            value: SerializedValue::Struct(vec![super::SerializedFieldValue {
                name: "m_Component".to_owned(),
                value: SerializedValue::Array(vec![
                    SerializedValue::PPtr {
                        file_id: 0,
                        path_id: 0,
                    },
                    SerializedValue::PPtr {
                        file_id: 2,
                        path_id: 41,
                    },
                ]),
            }]),
        }]);

        let components = root
            .unique_field_in_base_chain_mut("m_Component")
            .unwrap()
            .unwrap();
        components
            .push_array_element_bounded(
                SerializedValue::PPtr {
                    file_id: 3,
                    path_id: 99,
                },
                3,
            )
            .unwrap();
        assert!(matches!(
            components.push_array_element_bounded(
                SerializedValue::PPtr {
                    file_id: 4,
                    path_id: 100,
                },
                3
            ),
            Err(
                super::SerializedValueMutationError::ArrayElementLimitExceeded {
                    requested: 4,
                    limit: 3,
                }
            )
        ));

        let components = root
            .unique_field_in_base_chain("m_Component")
            .unwrap()
            .unwrap();
        assert_eq!(
            components.array_elements().unwrap(),
            &[
                SerializedValue::PPtr {
                    file_id: 0,
                    path_id: 0,
                },
                SerializedValue::PPtr {
                    file_id: 2,
                    path_id: 41,
                },
                SerializedValue::PPtr {
                    file_id: 3,
                    path_id: 99,
                },
            ]
        );
    }

    #[test]
    fn inherited_field_mutation_rejects_duplicate_matches() {
        let mut root = SerializedValue::Struct(vec![
            super::SerializedFieldValue {
                name: "m_Component".to_owned(),
                value: SerializedValue::Array(Vec::new()),
            },
            super::SerializedFieldValue {
                name: "Base".to_owned(),
                value: SerializedValue::Struct(vec![super::SerializedFieldValue {
                    name: "m_Component".to_owned(),
                    value: SerializedValue::Array(Vec::new()),
                }]),
            },
        ]);

        assert!(matches!(
            root.unique_field_in_base_chain_mut("m_Component"),
            Err(super::SerializedValueMutationError::DuplicateField {
                name,
                match_count: 2,
            }) if name == "m_Component"
        ));
    }

    fn node(
        level: u8,
        name: &str,
        type_name: &str,
        byte_size: i32,
        meta_flags: u32,
    ) -> TypeTreeNode {
        TypeTreeNode {
            level,
            version: 1,
            type_flags: 0,
            type_string_offset: None,
            name_string_offset: None,
            type_name: Some(type_name.to_owned()),
            name: Some(name.to_owned()),
            byte_size,
            index: 0,
            meta_flags,
            reference_type_hash: None,
        }
    }

    fn make_file(payload: &[u8]) -> (Vec<u8>, SerializedFileHeader, SerializedFileMetadata) {
        let generation = FormatVersion::Unknown9;
        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2022.3.1f1\0");
        metadata.extend_from_slice(&0u32.to_le_bytes()); // target platform
        metadata.extend_from_slice(&[1]); // type tree enabled
        let metadata_offset = 20usize;
        let data_offset = metadata_offset + metadata.len();
        let object_bytes = payload.to_vec();
        let file_size = data_offset + object_bytes.len();
        let mut file = vec![0; 20];
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&object_bytes);
        let header = SerializedFileHeader {
            metadata_size: metadata.len() as u64,
            file_size: file_size as u64,
            format_version: generation,
            data_offset: data_offset as u64,
            data_endian: Some(Endian::Little),
            reserved_v22: None,
            header_size: 20,
        };
        let mut metadata = SerializedFileMetadata {
            prefix: super::super::metadata_prefix::SerializedFileMetadataPrefix {
                metadata_offset,
                metadata_size: metadata.len(),
                body_endian: Endian::Little,
                unity_version_text: Some("2022.3.1f1".to_owned()),
                target_platform: Some(0),
                has_type_tree: true,
                consumed_bytes: 16,
            },
            types: vec![super::super::metadata::SerializedType {
                raw_type_id: 1,
                type_id_for_hash: 1,
                is_stripped_type: false,
                script_type_index: -1,
                script_id: None,
                old_type_hash: None,
                type_tree: Some(TypeTree {
                    nodes: vec![
                        node(0, "Root", "Root", -1, 0),
                        node(1, "m_Number", "int", 4, 0),
                        node(1, "m_Flag", "bool", 1, 0),
                    ],
                    string_buffer: Vec::new(),
                }),
                type_dependencies: Vec::new(),
            }],
            long_file_id: None,
            objects: vec![super::super::metadata::ObjectInfo {
                path_id: 42,
                byte_start: 0,
                byte_size: payload.len() as u32,
                serialized_type_index: None,
                raw_type_id: 1,
                class_id: Some(1),
                script_type_index: None,
                is_destroyed: None,
                is_stripped: None,
                resolved_type_index: Some(0),
            }],
            script_types: Vec::new(),
            externals: Vec::new(),
            reference_types: Vec::new(),
            user_information: None,
            trailing_metadata: Vec::new(),
            consumed_bytes: 16,
        };
        metadata.prefix.metadata_size = metadata.prefix.consumed_bytes;
        (file, header, metadata)
    }

    #[test]
    fn decodes_primitive_fields_and_preserves_trailing_payload_bytes() {
        let payload = [0x78, 0x56, 0x34, 0x12, 1, 0xaa];
        let (file, header, metadata) = make_file(&payload);
        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();

        assert_eq!(object.path_id, 42);
        assert_eq!(object.type_name, "Root");
        assert_eq!(
            object.root,
            SerializedValue::Struct(vec![
                super::SerializedFieldValue {
                    name: "m_Number".to_owned(),
                    value: SerializedValue::I32(0x1234_5678),
                },
                super::SerializedFieldValue {
                    name: "m_Flag".to_owned(),
                    value: SerializedValue::Bool(true),
                },
            ])
        );
        assert_eq!(object.consumed_bytes, 5);
        assert_eq!(object.trailing_bytes, [0xaa]);
    }

    #[test]
    fn encodes_typed_object_fields_and_retains_trailing_bytes_in_both_endian_modes() {
        let little_endian_payload = [0x78, 0x56, 0x34, 0x12, 1, 0xaa];
        let (file, header, metadata) = make_file(&little_endian_payload);
        let mut object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        assert_eq!(
            metadata
                .encode_object_payload(&object, ObjectEncodeLimits::default())
                .unwrap(),
            little_endian_payload
        );

        *object
            .root
            .unique_field_in_base_chain_mut("m_Number")
            .unwrap()
            .unwrap() = SerializedValue::I32(0x0102_0304);
        let encoded = metadata
            .encode_object_payload(&object, ObjectEncodeLimits::default())
            .unwrap();
        assert_eq!(encoded, [0x04, 0x03, 0x02, 0x01, 1, 0xaa]);
        let data_range = metadata.objects[0].data_range(&header).unwrap();
        let data_start = usize::try_from(data_range.start).unwrap();
        let mut edited_file = file.clone();
        edited_file[data_start..data_start + encoded.len()].copy_from_slice(&encoded);
        let decoded = metadata
            .decode_object_payload(&edited_file, &header, 0)
            .unwrap();
        assert_eq!(
            decoded.root,
            SerializedValue::Struct(vec![
                super::SerializedFieldValue {
                    name: "m_Number".to_owned(),
                    value: SerializedValue::I32(0x0102_0304),
                },
                super::SerializedFieldValue {
                    name: "m_Flag".to_owned(),
                    value: SerializedValue::Bool(true),
                },
            ])
        );
        assert_eq!(decoded.trailing_bytes, [0xaa]);
        let error = metadata
            .encode_object_payload(
                &object,
                ObjectEncodeLimits {
                    max_encoded_payload_bytes: 4,
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            ObjectEncodeErrorKind::EncodedPayloadLimitExceeded { limit: 4, .. }
        ));

        let big_endian_payload = [0x12, 0x34, 0x56, 0x78, 1, 0xaa];
        let (file, header, mut metadata) = make_file(&big_endian_payload);
        metadata.prefix.body_endian = Endian::Big;
        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        assert_eq!(
            metadata
                .encode_object_payload(&object, ObjectEncodeLimits::default())
                .unwrap(),
            big_endian_payload
        );
    }

    #[test]
    fn tree_usability_distinguishes_absent_and_empty_trees() {
        use super::TypeTreeUsability;
        assert_eq!(
            super::classify_type_tree_usability(None),
            TypeTreeUsability::Missing
        );
        let empty = TypeTree {
            nodes: Vec::new(),
            string_buffer: Vec::new(),
        };
        assert_eq!(
            super::classify_type_tree_usability(Some(&empty)),
            TypeTreeUsability::Empty
        );
        let present = TypeTree {
            nodes: vec![node(0, "Root", "Root", -1, 0)],
            string_buffer: Vec::new(),
        };
        assert_eq!(
            super::classify_type_tree_usability(Some(&present)),
            TypeTreeUsability::Present
        );
        // The decode prelude reports each fact with its own error.
        let (file, header, mut metadata) = make_file(&[0]);
        metadata.types[0].type_tree = Some(empty);
        let error = metadata
            .decode_object_payload(&file, &header, 0)
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            ObjectDecodeErrorKind::EmptyTypeTree { object_index: 0 }
        ));
        metadata.types[0].type_tree = None;
        let error = metadata
            .decode_object_payload(&file, &header, 0)
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            ObjectDecodeErrorKind::MissingTypeTree { object_index: 0 }
        ));
    }

    #[test]
    fn pre_3_5_payloads_are_refused_before_decoding() {
        assert!(!super::version_is_unsupported(3, 5));
        assert!(!super::version_is_unsupported(4, 0));
        assert!(!super::version_is_unsupported(2022, 3));
        assert!(super::version_is_unsupported(3, 4));
        assert!(super::version_is_unsupported(2, 6));
        assert!(super::version_is_unsupported(0, 0));
        // A positively identified pre-3.5 file is refused with its own error.
        let full = [0x78, 0x56, 0x34, 0x12, 1];
        let (file, header, mut metadata) = make_file(&full);
        metadata.prefix.unity_version_text = Some("3.4.2f3".to_owned());
        let error = metadata
            .decode_object_payload(&file, &header, 0)
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            ObjectDecodeErrorKind::UnreadableUnsupportedVersion { object_index: 0 }
        ));
        // The 3.5 boundary itself, missing text, and garbage text all proceed.
        metadata.prefix.unity_version_text = Some("3.5.0b1".to_owned());
        assert!(metadata.decode_object_payload(&file, &header, 0).is_ok());
        metadata.prefix.unity_version_text = None;
        assert!(metadata.decode_object_payload(&file, &header, 0).is_ok());
        metadata.prefix.unity_version_text = Some("not-a-version".to_owned());
        assert!(metadata.decode_object_payload(&file, &header, 0).is_ok());
    }

    #[test]
    fn rejects_negative_array_counts_and_missing_type_trees() {
        let (file, header, mut metadata) = make_file(&[0xff, 0xff, 0xff, 0xff]);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Values", "Array", -1, 0),
                node(2, "size", "int", 4, 0),
                node(2, "data", "int", 4, 0),
            ],
            string_buffer: Vec::new(),
        });
        let error = metadata
            .decode_object_payload(&file, &header, 0)
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            ObjectDecodeErrorKind::NegativeArrayLength { length: -1, .. }
        ));

        metadata.types[0].type_tree = None;
        let error = metadata
            .decode_object_payload(&file, &header, 0)
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            ObjectDecodeErrorKind::MissingTypeTree { object_index: 0 }
        ));

        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Target", "managedReference", 8, 0),
                node(2, "rid", "SInt64", 8, 0),
            ],
            string_buffer: Vec::new(),
        });
        let error = metadata
            .decode_object_payload(&file, &header, 0)
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            ObjectDecodeErrorKind::UnsupportedManagedReferences {
                is_indexed: false,
                ..
            }
        ));

        // The legacy `id` flavor reports the indexed selection instead.
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Target", "managedReference", 4, 0),
                node(2, "id", "SInt32", 4, 0),
            ],
            string_buffer: Vec::new(),
        });
        let error = metadata
            .decode_object_payload(&file, &header, 0)
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            ObjectDecodeErrorKind::UnsupportedManagedReferences {
                is_indexed: true,
                ..
            }
        ));
        assert!(error.to_string().contains("legacy indexed"));
    }

    #[test]
    fn decodes_byte_arrays_with_versioned_alignment_before_the_next_field() {
        let payload = [1, 0, 0, 0, 0x7f, 0, 0, 0, 1];
        let (file, header, mut metadata) = make_file(&payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Bytes", "vector", -1, 0),
                node(2, "Array", "Array", -1, 0),
                node(3, "size", "int", 4, 0),
                node(3, "data", "UInt8", 1, 0),
                node(1, "m_Active", "bool", 1, 0),
            ],
            string_buffer: Vec::new(),
        });

        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        let SerializedValue::Struct(fields) = object.root else {
            panic!("expected a structured root value");
        };
        assert_eq!(fields[0].name, "m_Bytes");
        assert_eq!(
            fields[0].value,
            SerializedValue::Array(vec![SerializedValue::U8(0x7f)])
        );
        assert_eq!(fields[1].name, "m_Active");
        assert_eq!(fields[1].value, SerializedValue::Bool(true));
        assert_eq!(object.consumed_bytes, payload.len());
        assert!(object.trailing_bytes.is_empty());

        let mut shortened = metadata.decode_object_payload(&file, &header, 0).unwrap();
        *shortened
            .root
            .unique_field_in_base_chain_mut("m_Bytes")
            .unwrap()
            .unwrap() = SerializedValue::Array(Vec::new());
        let encoded = metadata
            .encode_object_payload(
                &shortened,
                ObjectEncodeLimits {
                    max_encoded_payload_bytes: 5,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(encoded, [0, 0, 0, 0, 1]);
        let data_start =
            usize::try_from(metadata.objects[0].data_range(&header).unwrap().start).unwrap();
        let mut edited_file = file.clone();
        edited_file[data_start..data_start + encoded.len()].copy_from_slice(&encoded);
        let mut edited_metadata = metadata.clone();
        edited_metadata.objects[0].byte_size = encoded.len() as u32;
        let decoded = edited_metadata
            .decode_object_payload(&edited_file, &header, 0)
            .unwrap();
        assert_eq!(
            decoded.root,
            SerializedValue::Struct(vec![
                super::SerializedFieldValue {
                    name: "m_Bytes".to_owned(),
                    value: SerializedValue::Array(Vec::new()),
                },
                super::SerializedFieldValue {
                    name: "m_Active".to_owned(),
                    value: SerializedValue::Bool(true),
                },
            ])
        );
    }

    #[test]
    fn decodes_aligned_utf8_byte_strings_before_later_fields() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&3i32.to_le_bytes());
        payload.extend_from_slice(b"abc");
        payload.push(0); // string alignment to a four-byte boundary
        payload.extend_from_slice(&0xaabb_ccddu32.to_le_bytes());
        let (file, header, mut metadata) = make_file(&payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Name", "string", -1, 0),
                node(2, "Array", "Array", -1, 0),
                node(3, "size", "int", 4, 0),
                node(3, "data", "char", 1, 0),
                node(1, "m_Flags", "uint", 4, 0),
            ],
            string_buffer: Vec::new(),
        });

        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        assert_eq!(
            metadata
                .encode_object_payload(&object, ObjectEncodeLimits::default())
                .unwrap(),
            payload
        );
        let SerializedValue::Struct(fields) = object.root else {
            panic!("expected a structured root value");
        };
        assert_eq!(
            fields[0].value,
            SerializedValue::StringBytes(b"abc".to_vec())
        );
        assert_eq!(fields[1].value, SerializedValue::U32(0xaabb_ccdd));
        assert_eq!(object.consumed_bytes, payload.len());
    }

    #[test]
    fn decodes_and_reencodes_utf8string_type_tree_nodes() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&5i32.to_le_bytes());
        payload.extend_from_slice(b"track");
        payload.extend_from_slice(&[0; 3]); // string alignment to a four-byte boundary
        let (file, header, mut metadata) = make_file(&payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Name", "Utf8String", -1, 0),
                node(2, "Array", "Array", -1, 0),
                node(3, "size", "int", 4, 0),
                node(3, "data", "char", 1, 0),
            ],
            string_buffer: Vec::new(),
        });

        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        assert_eq!(
            object.root,
            SerializedValue::Struct(vec![super::SerializedFieldValue {
                name: "m_Name".to_owned(),
                value: SerializedValue::StringBytes(b"track".to_vec()),
            }])
        );
        assert_eq!(
            metadata
                .encode_object_payload(&object, ObjectEncodeLimits::default())
                .unwrap(),
            payload
        );
    }

    #[test]
    fn honors_field_align_bytes_and_keeps_unknown_sized_leaves() {
        let payload = [7, 1, 0, 0, 0, 0, 0, 0, 0xaa, 0xbb];
        let (file, header, mut metadata) = make_file(&payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_First", "UInt8", 1, 0),
                node(1, "m_Number", "int", 4, 0x4000),
                node(1, "m_Unknown", "VendorThing", 2, 0),
            ],
            string_buffer: Vec::new(),
        });

        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        assert_eq!(
            metadata
                .encode_object_payload(&object, ObjectEncodeLimits::default())
                .unwrap(),
            payload
        );
        let SerializedValue::Struct(fields) = object.root else {
            panic!("expected a structured root value");
        };
        assert_eq!(fields[0].value, SerializedValue::U8(7));
        assert_eq!(fields[1].value, SerializedValue::I32(1));
        assert_eq!(
            fields[2].value,
            SerializedValue::Raw {
                type_name: "VendorThing".to_owned(),
                bytes: vec![0xaa, 0xbb],
            }
        );
        assert_eq!(object.consumed_bytes, payload.len());
    }

    #[test]
    fn decodes_typed_pptr_nodes_without_losing_file_or_path_identity() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&2i32.to_le_bytes());
        payload.extend_from_slice(&0x1234_5678_9abc_def0i64.to_le_bytes());
        let (file, header, mut metadata) = make_file(&payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Target", "PPtr<$GameObject>", 12, 0),
                node(2, "m_FileID", "SInt32", 4, 0),
                node(2, "m_PathID", "SInt64", 8, 0),
            ],
            string_buffer: Vec::new(),
        });

        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        assert_eq!(
            object.root,
            SerializedValue::Struct(vec![super::SerializedFieldValue {
                name: "m_Target".to_owned(),
                value: SerializedValue::PPtr {
                    file_id: 2,
                    path_id: 0x1234_5678_9abc_def0,
                },
            }])
        );
        assert_eq!(object.consumed_bytes, payload.len());
    }

    #[test]
    fn sign_extends_legacy_32_bit_pptr_path_ids() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&(-1i32).to_le_bytes());
        payload.extend_from_slice(&(-42i32).to_le_bytes());
        let (file, header, mut metadata) = make_file(&payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "PPtr<Object>", "PPtr<Object>", 8, 0),
                node(1, "m_FileID", "SInt32", 4, 0),
                node(1, "m_PathID", "SInt32", 4, 0),
            ],
            string_buffer: Vec::new(),
        });

        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        assert_eq!(
            object.root,
            SerializedValue::PPtr {
                file_id: -1,
                path_id: -42,
            }
        );
        assert_eq!(object.consumed_bytes, payload.len());
    }

    #[test]
    fn decodes_nested_vector_struct_fields() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&1.0f32.to_le_bytes());
        payload.extend_from_slice(&(-2.5f32).to_le_bytes());
        payload.extend_from_slice(&3.25f32.to_le_bytes());
        let (file, header, mut metadata) = make_file(&payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Position", "Vector3f", -1, 0),
                node(2, "x", "float", 4, 0),
                node(2, "y", "float", 4, 0),
                node(2, "z", "float", 4, 0),
            ],
            string_buffer: Vec::new(),
        });

        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        let SerializedValue::Struct(fields) = object.root else {
            panic!("expected a structured root value");
        };
        assert_eq!(
            fields[0].value,
            SerializedValue::Struct(vec![
                super::SerializedFieldValue {
                    name: "x".to_owned(),
                    value: SerializedValue::F32(1.0),
                },
                super::SerializedFieldValue {
                    name: "y".to_owned(),
                    value: SerializedValue::F32(-2.5),
                },
                super::SerializedFieldValue {
                    name: "z".to_owned(),
                    value: SerializedValue::F32(3.25),
                },
            ])
        );
    }

    #[test]
    fn honors_the_type_tree_integer_as_character_flag() {
        let payload = 0x0041u16.to_le_bytes();
        let (file, header, mut metadata) = make_file(&payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Character", "UInt16", 2, 0x0200_0000),
            ],
            string_buffer: Vec::new(),
        });

        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        let SerializedValue::Struct(fields) = object.root else {
            panic!("expected a structured root value");
        };
        assert_eq!(fields[0].value, SerializedValue::Char(0x0041));
    }

    #[test]
    fn decodes_object_payload_scalars_using_metadata_endianness() {
        let payload = [0x12, 0x34, 0x56, 0x78];
        let (file, mut header, mut metadata) = make_file(&payload);
        header.data_endian = Some(Endian::Big);
        metadata.prefix.body_endian = Endian::Big;
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Value", "int", 4, 0),
            ],
            string_buffer: Vec::new(),
        });

        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        let SerializedValue::Struct(fields) = object.root else {
            panic!("expected a structured root value");
        };
        assert_eq!(fields[0].value, SerializedValue::I32(0x1234_5678));
    }

    #[test]
    fn caller_decode_limits_reject_excessive_array_storage() {
        let payload = [2, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0];
        let (file, header, mut metadata) = make_file(&payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Values", "vector", -1, 0),
                node(2, "Array", "Array", -1, 0),
                node(3, "size", "int", 4, 0),
                node(3, "data", "int", 4, 0),
            ],
            string_buffer: Vec::new(),
        });
        let limits = super::ObjectDecodeLimits {
            max_array_elements: 1,
            ..super::ObjectDecodeLimits::default()
        };

        let error = metadata
            .decode_object_payload_with_limits(&file, &header, 0, &limits)
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            ObjectDecodeErrorKind::DecodeLimitExceeded {
                requested: 2,
                limit: 1,
                ..
            }
        ));
    }

    #[test]
    fn aligns_each_inner_complex_array_when_the_field_requests_alignment() {
        let payload = [
            2, 0, 0, 0, // outer array count
            1, 0, 0, 0, // first inner count
            0x11, 0, 0, 0, // first byte plus inner-array padding
            1, 0, 0, 0, // second inner count
            0x22, 0, 0, 0, // second byte plus inner-array padding
        ];
        let (file, header, mut metadata) = make_file(&payload);
        metadata.types[0].type_tree = Some(TypeTree {
            nodes: vec![
                node(0, "Root", "Root", -1, 0),
                node(1, "m_Nested", "vector", -1, 0x4000),
                node(2, "Array", "Array", -1, 0),
                node(3, "size", "int", 4, 0),
                node(3, "data", "vector", -1, 0),
                node(4, "Array", "Array", -1, 0),
                node(5, "size", "int", 4, 0),
                node(5, "data", "Custom", -1, 0),
                node(6, "m_Value", "UInt8", 1, 0),
            ],
            string_buffer: Vec::new(),
        });

        let object = metadata.decode_object_payload(&file, &header, 0).unwrap();
        let SerializedValue::Struct(fields) = object.root else {
            panic!("expected a structured root value");
        };
        assert_eq!(
            fields[0].value,
            SerializedValue::Array(vec![
                SerializedValue::Array(vec![SerializedValue::Struct(vec![
                    super::SerializedFieldValue {
                        name: "m_Value".to_owned(),
                        value: SerializedValue::U8(0x11),
                    },
                ])]),
                SerializedValue::Array(vec![SerializedValue::Struct(vec![
                    super::SerializedFieldValue {
                        name: "m_Value".to_owned(),
                        value: SerializedValue::U8(0x22),
                    },
                ])]),
            ])
        );
        assert_eq!(object.consumed_bytes, payload.len());
    }

    #[test]
    fn object_read_flags_select_release_for_ordinary_game_files() {
        use super::TransferInstructionFlags;
        use crate::serialized_file::format_version::FormatVersion;

        // An ordinary platformed file reads as a game release.
        let flags = super::serialized_file_object_read_flags(
            FormatVersion::LargeFilesSupport,
            Some(7),
            "/game/level0",
            "level0",
            crate::io::binary::Endian::Little,
        );
        assert_eq!(flags, TransferInstructionFlags::SERIALIZE_GAME_RELEASE);
        assert!(flags.is_release());
        // A generation without a platform field never takes the scene branch,
        // even with a NoTarget stand-in and a `.unity` path.
        let flags = super::serialized_file_object_read_flags(
            FormatVersion::Unknown7,
            None,
            "/game/Level.unity",
            "Level",
            crate::io::binary::Endian::Little,
        );
        assert_eq!(flags, TransferInstructionFlags::SERIALIZE_GAME_RELEASE);
    }

    #[test]
    fn object_read_flags_select_editor_scene_for_unity_paths_without_target() {
        use super::TransferInstructionFlags;
        use crate::io::binary::Endian;
        use crate::serialized_file::format_version::FormatVersion;

        let scene = super::serialized_file_object_read_flags(
            FormatVersion::LargeFilesSupport,
            Some(super::BUILD_TARGET_NO_TARGET),
            "Assets/Scenes/Main.unity",
            "Main",
            Endian::Little,
        );
        assert_eq!(
            scene,
            TransferInstructionFlags::SERIALIZE_EDITOR_MINIMAL_SCENE
        );
        assert!(scene.is_editor_scene());
        assert!(!scene.is_release());
        // A NoTarget platform without a `.unity` path carries bare flags.
        let bare = super::serialized_file_object_read_flags(
            FormatVersion::LargeFilesSupport,
            Some(super::BUILD_TARGET_NO_TARGET),
            "/game/sharedassets0.assets",
            "sharedassets0.assets",
            Endian::Little,
        );
        assert_eq!(bare, TransferInstructionFlags::NONE);
        // The suffix check is ordinal and case-sensitive, like the source.
        let upper = super::serialized_file_object_read_flags(
            FormatVersion::LargeFilesSupport,
            Some(super::BUILD_TARGET_NO_TARGET),
            "Assets/Scenes/Main.UNITY",
            "Main",
            Endian::Little,
        );
        assert_eq!(upper, TransferInstructionFlags::NONE);
        let backup = super::serialized_file_object_read_flags(
            FormatVersion::LargeFilesSupport,
            Some(super::BUILD_TARGET_NO_TARGET),
            "Assets/Scenes/Main.unity.backup",
            "Main",
            Endian::Little,
        );
        assert_eq!(backup, TransferInstructionFlags::NONE);
        // A real platform target always reads as a game release.
        let release = super::serialized_file_object_read_flags(
            FormatVersion::LargeFilesSupport,
            Some(7),
            "Assets/Scenes/Main.unity",
            "Main",
            Endian::Little,
        );
        assert_eq!(release, TransferInstructionFlags::SERIALIZE_GAME_RELEASE);
    }

    #[test]
    fn object_read_flags_mark_builtin_resource_files() {
        use super::TransferInstructionFlags;
        use crate::io::binary::Endian;
        use crate::serialized_file::format_version::FormatVersion;

        // Engine resources set the builtin bit on any generation.
        for name in [
            super::DEFAULT_RESOURCE_NAME_1,
            super::DEFAULT_RESOURCE_NAME_2,
            super::EDITOR_RESOURCE_NAME,
        ] {
            let flags = super::serialized_file_object_read_flags(
                FormatVersion::LargeFilesSupport,
                Some(7),
                "/game/resources.assets",
                name,
                Endian::Little,
            );
            assert_eq!(
                flags,
                TransferInstructionFlags::SERIALIZE_GAME_RELEASE
                    .with(TransferInstructionFlags::IS_BUILTIN_RESOURCES_FILE)
            );
            assert!(flags.is_builtin_resources());
        }
        // Builtin-extra names only count below generation 10.
        for name in [super::BUILTIN_EXTRA_NAME_1, super::BUILTIN_EXTRA_NAME_2] {
            let old = super::serialized_file_object_read_flags(
                FormatVersion::Unknown9,
                Some(super::BUILD_TARGET_NO_TARGET),
                "/game/extra.assets",
                name,
                Endian::Little,
            );
            assert!(old.is_builtin_resources());
            let cutoff = super::serialized_file_object_read_flags(
                FormatVersion::Unknown10,
                Some(7),
                "/game/extra.assets",
                name,
                Endian::Little,
            );
            assert!(!cutoff.is_builtin_resources());
            assert_eq!(cutoff, TransferInstructionFlags::SERIALIZE_GAME_RELEASE);
        }
        // Ordinary names never set the builtin bit.
        let plain = super::serialized_file_object_read_flags(
            FormatVersion::Unknown9,
            None,
            "/game/sharedassets0.assets",
            "sharedassets0.assets",
            Endian::Little,
        );
        assert!(!plain.is_builtin_resources());
    }

    #[test]
    fn object_read_flags_swap_endianness_for_big_endian_bodies() {
        use super::TransferInstructionFlags;
        use crate::io::binary::Endian;
        use crate::serialized_file::format_version::FormatVersion;

        let big = super::serialized_file_object_read_flags(
            FormatVersion::LargeFilesSupport,
            Some(7),
            "/game/data.assets",
            "data.assets",
            Endian::Big,
        );
        assert_eq!(
            big,
            TransferInstructionFlags::SERIALIZE_GAME_RELEASE
                .with(TransferInstructionFlags::SWAP_ENDIANESS)
        );
        // The endian bit composes with the scene and builtin bits.
        let scene_big = super::serialized_file_object_read_flags(
            FormatVersion::LargeFilesSupport,
            Some(super::BUILD_TARGET_NO_TARGET),
            "Assets/Scenes/Main.unity",
            super::DEFAULT_RESOURCE_NAME_2,
            Endian::Big,
        );
        assert!(scene_big.is_editor_scene());
        assert!(scene_big.is_builtin_resources());
        assert_eq!(
            scene_big.0,
            TransferInstructionFlags::SERIALIZE_EDITOR_MINIMAL_SCENE.0
                | TransferInstructionFlags::IS_BUILTIN_RESOURCES_FILE.0
                | TransferInstructionFlags::SWAP_ENDIANESS.0
        );
    }

    #[test]
    fn special_file_name_predicates_match_exact_names_only() {
        assert!(super::is_engine_resource_name("unity default resources"));
        assert!(super::is_engine_resource_name("unity_default_resources"));
        assert!(super::is_engine_resource_name("unity editor resources"));
        assert!(!super::is_engine_resource_name("unity builtin extra"));
        // Ordinal comparison: case and normalization matter.
        assert!(!super::is_engine_resource_name("Unity Default Resources"));
        assert!(!super::is_engine_resource_name(
            "library/unity default resources"
        ));
        assert!(!super::is_engine_resource_name(""));
        assert!(super::is_builtin_extra_name("unity builtin extra"));
        assert!(super::is_builtin_extra_name("unity_builtin_extra"));
        assert!(!super::is_builtin_extra_name("Unity Builtin Extra"));
        assert!(!super::is_builtin_extra_name("unity editor resources"));
    }
}
