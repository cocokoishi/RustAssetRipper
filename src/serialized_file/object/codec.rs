use super::super::header::SerializedFileHeader;
use super::super::metadata::{
    unity_version_aligns_arrays, SerializedFileMetadata, SerializedTypeReference, TypeTree,
};
use super::decode_budget::{
    DecodeBudget, ManagedPayloadDecodeContext, ObjectDecodeLimits, ObjectDecodeTarget,
};
use super::decode_error::{ObjectDecodeError, ObjectDecodeErrorKind};
use super::recovery::{
    classify_type_tree_usability, version_is_unsupported, version_major_minor, TypeTreeUsability,
};
use super::type_tree::{
    allocation_error, array_base_is_primitive, array_node_for_value, array_shape, direct_children,
    field_aligns, invalid_pptr_component, is_indexed_managed_reference_element,
    is_managed_reference_element, is_managed_references_registry, is_referenced_object_data,
    is_string_node, minimum_encoded_size, pptr_shape, TypeTreeLayout, CHAR_PROPERTY_MASK_FLAG,
};
use super::value::{ParsedObjectPayload, SerializedFieldValue, SerializedValue};
use crate::io::binary::{BinaryError, BinaryReader, Endian};
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
        // The source refuses pre-3.5 assets before reading anything; refused payloads
        // flow into the existing raw-retention path downstream like any decode failure.
        self.ensure_object_version_supported(object_index)?;
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
        let mut budget = DecodeBudget {
            limits,
            allocated_bytes: 0,
            managed_type_references: &self.reference_types,
            managed_reference_type_resolver: None,
            stream_offset: 0,
            managed_registry_depth: 0,
        };
        self.decode_object_payload_from_tree(
            file_bytes,
            header,
            ObjectDecodeTarget {
                index: object_index,
                object,
                fallback_type_id: serialized_type.raw_type_id,
            },
            type_tree,
            &mut budget,
        )
    }

    /// Decode one object with a source-supplied TypeTree. The asset factory uses this
    /// when a generated class has a usable versioned TPK layout but the file has no
    /// usable TypeTree.
    pub(crate) fn decode_object_payload_using_type_tree_with_limits(
        &self,
        file_bytes: &[u8],
        header: &SerializedFileHeader,
        object_index: usize,
        type_tree: &TypeTree,
        limits: &ObjectDecodeLimits,
    ) -> Result<ParsedObjectPayload, ObjectDecodeError> {
        self.ensure_object_version_supported(object_index)?;
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
        let mut budget = DecodeBudget {
            limits,
            allocated_bytes: 0,
            managed_type_references: &self.reference_types,
            managed_reference_type_resolver: None,
            stream_offset: 0,
            managed_registry_depth: 0,
        };
        self.decode_object_payload_from_tree(
            file_bytes,
            header,
            ObjectDecodeTarget {
                index: object_index,
                object,
                fallback_type_id: object.raw_type_id,
            },
            type_tree,
            &mut budget,
        )
    }

    /// Decode one independently-read object payload while retaining its original file offset.
    /// The caller must provide the exact object range from the SerializedFile stream.
    pub(crate) fn decode_object_payload_from_data_with_limits(
        &self,
        payload: &[u8],
        header: &SerializedFileHeader,
        object_index: usize,
        actual_file_size: usize,
        limits: &ObjectDecodeLimits,
    ) -> Result<ParsedObjectPayload, ObjectDecodeError> {
        self.ensure_object_version_supported(object_index)?;
        header
            .validate_for_file_size(actual_file_size)
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
        let data_range = object.data_range(header).ok_or_else(|| {
            ObjectDecodeError::new(
                0,
                ObjectDecodeErrorKind::ObjectDataRangeOverflow { object_index },
            )
        })?;
        let expected_len = data_range
            .end
            .checked_sub(data_range.start)
            .ok_or_else(|| {
                ObjectDecodeError::new(
                    0,
                    ObjectDecodeErrorKind::ObjectDataRangeOverflow { object_index },
                )
            })?;
        if data_range.end > actual_file_size as u64 || expected_len != payload.len() as u64 {
            return Err(ObjectDecodeError::new(
                usize::try_from(data_range.start).unwrap_or(usize::MAX),
                ObjectDecodeErrorKind::ObjectDataOutOfBounds {
                    object_index,
                    start: data_range.start,
                    end: data_range.end,
                    file_size: actual_file_size,
                },
            ));
        }
        let start = usize::try_from(data_range.start).map_err(|_| {
            ObjectDecodeError::new(
                0,
                ObjectDecodeErrorKind::ObjectDataRangeOverflow { object_index },
            )
        })?;
        let mut budget = DecodeBudget {
            limits,
            allocated_bytes: 0,
            managed_type_references: &self.reference_types,
            managed_reference_type_resolver: None,
            stream_offset: start,
            managed_registry_depth: 0,
        };
        self.decode_object_payload_from_payload(
            payload,
            start,
            ObjectDecodeTarget {
                index: object_index,
                object,
                fallback_type_id: serialized_type.raw_type_id,
            },
            type_tree,
            &mut budget,
        )
    }

    fn ensure_object_version_supported(
        &self,
        object_index: usize,
    ) -> Result<(), ObjectDecodeError> {
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
        Ok(())
    }

    fn decode_object_payload_from_tree(
        &self,
        file_bytes: &[u8],
        header: &SerializedFileHeader,
        target: ObjectDecodeTarget<'_>,
        type_tree: &TypeTree,
        budget: &mut DecodeBudget<'_>,
    ) -> Result<ParsedObjectPayload, ObjectDecodeError> {
        let object_index = target.index;
        let object = target.object;
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
        self.decode_object_payload_from_payload(payload, start, target, type_tree, budget)
    }

    fn decode_object_payload_from_payload(
        &self,
        payload: &[u8],
        start: usize,
        target: ObjectDecodeTarget<'_>,
        type_tree: &TypeTree,
        budget: &mut DecodeBudget<'_>,
    ) -> Result<ParsedObjectPayload, ObjectDecodeError> {
        let ObjectDecodeTarget {
            index: object_index,
            object,
            fallback_type_id,
        } = target;
        let type_root = type_tree.nodes.first().ok_or_else(|| {
            ObjectDecodeError::new(0, ObjectDecodeErrorKind::EmptyTypeTree { object_index })
        })?;
        let type_layout = TypeTreeLayout::new(type_tree, object_index, 0)?;
        let mut reader = BinaryReader::with_base_offset(payload, self.prefix.body_endian, start)
            .map_err(ObjectDecodeError::from_binary)?;
        let arrays_align = unity_version_aligns_arrays(self.prefix.unity_version_text.as_deref());

        let root = decode_root(
            &mut reader,
            &type_layout,
            arrays_align,
            object_index,
            type_root.type_name.as_deref().unwrap_or(""),
            budget,
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
                .unwrap_or_else(|| format!("type {fallback_type_id}")),
            root,
            consumed_bytes,
            trailing_bytes,
            allocated_bytes: budget.allocated_bytes,
        })
    }
}

/// Decode one freestanding payload against a caller-built node list (managed
/// reference registry entries, whose trees come from the type resolver rather
/// than a file). The node list must be level-nested exactly like a file
/// TypeTree. Managed-reference ID nodes use the same ID representation as
/// ordinary file-object fields. Nested registries inherit the owning object
/// stream offset and registry-depth bound.
pub(crate) fn decode_typetree_payload(
    type_tree: &TypeTree,
    payload: &[u8],
    endian: Endian,
    arrays_align: bool,
    managed_type_references: &[SerializedTypeReference],
    context: ManagedPayloadDecodeContext<'_>,
    limits: &ObjectDecodeLimits,
) -> Result<(SerializedValue, usize, usize), ObjectDecodeError> {
    decode_typetree_payload_with_resolver(
        type_tree,
        payload,
        endian,
        arrays_align,
        managed_type_references,
        context,
        limits,
    )
}

pub(crate) fn decode_typetree_payload_with_resolver(
    type_tree: &TypeTree,
    payload: &[u8],
    endian: Endian,
    arrays_align: bool,
    managed_type_references: &[SerializedTypeReference],
    context: ManagedPayloadDecodeContext<'_>,
    limits: &ObjectDecodeLimits,
) -> Result<(SerializedValue, usize, usize), ObjectDecodeError> {
    let mut budget = DecodeBudget {
        limits,
        allocated_bytes: 0,
        managed_type_references,
        managed_reference_type_resolver: context.managed_reference_type_resolver,
        stream_offset: context.stream_offset,
        managed_registry_depth: context.managed_registry_depth,
    };
    let type_root = type_tree.nodes.first().ok_or_else(|| {
        ObjectDecodeError::new(
            0,
            ObjectDecodeErrorKind::EmptyTypeTree {
                object_index: context.entry_index,
            },
        )
    })?;
    let type_layout = TypeTreeLayout::new(type_tree, context.entry_index, 0)?;
    let mut reader = BinaryReader::with_base_offset(payload, endian, context.stream_offset)
        .map_err(ObjectDecodeError::from_binary)?;
    let root = decode_root(
        &mut reader,
        &type_layout,
        arrays_align,
        context.entry_index,
        type_root.type_name.as_deref().unwrap_or(""),
        &mut budget,
    )?;
    Ok((root, reader.position(), budget.allocated_bytes))
}

fn decode_root(
    reader: &mut BinaryReader<'_>,
    type_tree: &TypeTreeLayout<'_>,
    arrays_align: bool,
    object_index: usize,
    root_name: &str,
    budget: &mut DecodeBudget<'_>,
) -> Result<SerializedValue, ObjectDecodeError> {
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
pub(super) fn decode_field(
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
fn serialized_value_kind(value: &SerializedValue) -> &'static str {
    match value {
        SerializedValue::Bool(_) => "bool",
        SerializedValue::I8(_) => "i8",
        SerializedValue::U8(_) => "u8",
        SerializedValue::I16(_) => "i16",
        SerializedValue::U16(_) => "u16",
        SerializedValue::Char(_) => "char",
        SerializedValue::I32(_) => "i32",
        SerializedValue::U32(_) => "u32",
        SerializedValue::I64(_) => "i64",
        SerializedValue::U64(_) => "u64",
        SerializedValue::F32(_) => "f32",
        SerializedValue::F64(_) => "f64",
        SerializedValue::StringBytes(_) => "string",
        SerializedValue::Bytes(_) => "bytes",
        SerializedValue::Array(_) => "array",
        SerializedValue::Struct(_) => "struct",
        SerializedValue::PPtr { .. } => "PPtr",
        SerializedValue::ManagedReferenceId { .. } => "managed-reference ID",
        SerializedValue::ManagedReferenceRegistry(_) => "managed-reference registry",
        SerializedValue::Raw { .. } => "raw",
    }
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
    let node_name = node.name.as_deref().unwrap_or("");

    let children = direct_children(type_tree, node_index, object_index, reader.offset())?;
    if is_managed_reference_element(type_tree, node_index, object_index, reader.offset())? {
        let is_indexed = is_indexed_managed_reference_element(
            type_tree,
            node_index,
            object_index,
            reader.offset(),
        )?;
        let child_index = *children.first().ok_or_else(|| {
            ObjectDecodeError::new(
                reader.offset(),
                ObjectDecodeErrorKind::UnsupportedManagedReferences {
                    object_index,
                    type_name: type_name.to_owned(),
                    field_name: node_name.to_owned(),
                    is_indexed,
                },
            )
        })?;
        let identifier = decode_field(
            reader,
            type_tree,
            child_index,
            arrays_align,
            object_index,
            path,
            depth + 1,
            budget,
        )?;
        let reference_id = match identifier.value {
            SerializedValue::I32(value) if is_indexed => i64::from(value),
            SerializedValue::I64(value) if !is_indexed => value,
            value => {
                return Err(ObjectDecodeError::new(
                    reader.offset(),
                    ObjectDecodeErrorKind::InvalidManagedReferenceId {
                        object_index,
                        path: path.to_owned(),
                        indexed: is_indexed,
                        value: serialized_value_kind(&value).to_owned(),
                    },
                ));
            }
        };
        return Ok(SerializedValue::ManagedReferenceId {
            reference_id,
            indexed: is_indexed,
        });
    }

    if is_managed_references_registry(type_name, node_name, children.len()) {
        let registry_depth = budget
            .managed_registry_depth
            .checked_add(1)
            .ok_or_else(|| {
                ObjectDecodeError::new(
                    reader.offset(),
                    ObjectDecodeErrorKind::ManagedRegistryDepthExceeded {
                        object_index,
                        path: path.to_owned(),
                        requested: usize::MAX,
                        limit: budget.limits.max_managed_registry_depth,
                    },
                )
            })?;
        if registry_depth > budget.limits.max_managed_registry_depth {
            return Err(ObjectDecodeError::new(
                reader.offset(),
                ObjectDecodeErrorKind::ManagedRegistryDepthExceeded {
                    object_index,
                    path: path.to_owned(),
                    requested: registry_depth,
                    limit: budget.limits.max_managed_registry_depth,
                },
            ));
        }
        let registry_limits = ObjectDecodeLimits {
            max_decoded_allocation_bytes: budget.remaining_allocation_bytes(),
            ..*budget.limits
        };
        let options = super::super::managed_registry::ManagedRegistryReadOptions {
            endian: reader.endian(),
            arrays_align,
            stream_offset: budget
                .stream_offset
                .checked_add(reader.position())
                .ok_or_else(|| {
                    ObjectDecodeError::new(
                        reader.offset(),
                        ObjectDecodeErrorKind::ObjectDataRangeOverflow { object_index },
                    )
                })?,
            registry_depth,
            object_decode_limits: registry_limits,
            max_entries: budget.limits.max_array_elements,
            max_total_decoded_allocation_bytes: budget.remaining_allocation_bytes(),
        };
        let registry = super::super::managed_registry::read_managed_registry_with_resolver(
            reader.remaining_slice(),
            options,
            budget.managed_type_references,
            budget.managed_reference_type_resolver,
        )
        .map_err(|error| {
            ObjectDecodeError::new(
                reader.offset(),
                ObjectDecodeErrorKind::ManagedRegistryDecode {
                    object_index,
                    path: path.to_owned(),
                    message: error.to_string(),
                },
            )
        })?;
        reader
            .skip(registry.consumed_bytes)
            .map_err(ObjectDecodeError::from_binary)?;
        budget.charge(
            registry.allocated_bytes,
            reader.offset(),
            "managed reference registry",
        )?;
        return Ok(SerializedValue::ManagedReferenceRegistry(Box::new(
            registry,
        )));
    }

    if is_referenced_object_data(type_name, node_name, children.len()) {
        return Err(ObjectDecodeError::new(
            reader.offset(),
            ObjectDecodeErrorKind::UnsupportedReferencedObjectData {
                object_index,
                path: path.to_owned(),
            },
        ));
    }

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
    let data_node_value = &type_tree.nodes[data_node];
    let compact_byte_array = count > budget.limits.max_array_elements
        && matches!(data_node_value.type_name.as_deref(), Some("char" | "UInt8"))
        && direct_children(type_tree, data_node, object_index, reader.offset())?.is_empty();
    if !compact_byte_array && count > budget.limits.max_array_elements {
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

    // Large Unity byte vectors such as Mesh.m_IndexBuffer are stored contiguously in
    // the payload. Representing each byte as a SerializedValue enum multiplies their
    // decoded allocation by the enum size and can make a valid object appear to exceed
    // the allocation budget by dozens of times. Keep the exact bytes instead; this is
    // also the representation the Unity YAML writer uses for byte data.
    if compact_byte_array {
        budget.charge(count, reader.offset(), path)?;
        let bytes = reader
            .read_exact(count)
            .map_err(|error| binary_error(error, &format!("{path} byte array")))?
            .to_vec();
        if align_nested_arrays
            || (arrays_align
                && array_base_is_primitive(type_tree, data_node, object_index, reader.offset())?)
        {
            reader
                .align_relative(4)
                .map_err(|error| binary_error(error, &format!("{path} array alignment")))?;
        }
        return Ok(SerializedValue::Bytes(bytes));
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
