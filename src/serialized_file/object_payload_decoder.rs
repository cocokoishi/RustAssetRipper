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
        let options = super::managed_registry::ManagedRegistryReadOptions {
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
        let registry = super::managed_registry::read_managed_registry_with_resolver(
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

/// `TypeTreeNodeStruct.IsManagedReferencesRegistry` over caller facts: the
/// `ManagedReferencesRegistry` type holding the `references` field with more than one
/// subnode (the identifier plus at least one referenced object).
fn is_managed_references_registry(type_name: &str, node_name: &str, child_count: usize) -> bool {
    type_name == managed_names::REGISTRY_NAME
        && node_name == managed_names::REGISTRY_FIELD_NAME
        && child_count > 1
}

/// `TypeTreeNodeStruct.IsReferencedObjectData` over caller facts: the
/// `ReferencedObjectData` dynamic payload holder, which carries no subnodes because the
/// object content is read dynamically, not from the tree. A same-named node WITH
/// children is an ordinary struct here, not dynamic data.
fn is_referenced_object_data(type_name: &str, node_name: &str, child_count: usize) -> bool {
    type_name == managed_names::REFERENCED_OBJECT_DATA_NAME
        && node_name == managed_names::DATA_FIELD_NAME
        && child_count == 0
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

    pub(crate) fn source_type_tree_lookup(
        object_index: usize,
        class_id: i32,
        version: String,
        message: String,
    ) -> Self {
        Self::new(
            0,
            ObjectDecodeErrorKind::SourceTypeTreeLookup {
                object_index,
                class_id,
                version,
                message,
            },
        )
    }

    pub(crate) fn factory_object_outcome(
        object_index: usize,
        class_id: i32,
        outcome: String,
        diagnostic: Option<String>,
    ) -> Self {
        Self::new(
            0,
            ObjectDecodeErrorKind::FactoryObjectOutcome {
                object_index,
                class_id,
                outcome,
                diagnostic,
            },
        )
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
    /// The source factory retained this object as unknown or unreadable raw data.
    FactoryObjectOutcome {
        object_index: usize,
        class_id: i32,
        outcome: String,
        diagnostic: Option<String>,
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
    SourceTypeTreeLookup {
        object_index: usize,
        class_id: i32,
        version: String,
        message: String,
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
    InvalidManagedReferenceId {
        object_index: usize,
        path: String,
        indexed: bool,
        value: String,
    },
    ManagedRegistryDecode {
        object_index: usize,
        path: String,
        message: String,
    },
    UnsupportedReferencedObjectData {
        object_index: usize,
        path: String,
    },
    ManagedRegistryDepthExceeded {
        object_index: usize,
        path: String,
        requested: usize,
        limit: usize,
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
            Self::FactoryObjectOutcome {
                object_index,
                class_id,
                outcome,
                diagnostic,
            } => {
                write!(f, "object {object_index} class {class_id} produced {outcome}")?;
                if let Some(diagnostic) = diagnostic {
                    write!(f, ": {diagnostic}")?;
                }
                Ok(())
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
            Self::SourceTypeTreeLookup {
                object_index,
                class_id,
                version,
                message,
            } => write!(
                f,
                "object {object_index} class {class_id} has no usable source TPK TypeTree at {version}: {message}"
            ),
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
                "object {object_index} contains {} managed-reference TypeTree node {type_name} {field_name} without an ID child",
                if *is_indexed {
                    "legacy indexed"
                } else {
                    "rid-addressed"
                }
            ),
            Self::InvalidManagedReferenceId {
                object_index,
                path,
                indexed,
                value,
            } => write!(
                f,
                "object {object_index} field {path} has {value} for a {} managed-reference identifier",
                if *indexed { "legacy indexed" } else { "rid-addressed" }
            ),
            Self::ManagedRegistryDecode {
                object_index,
                path,
                message,
            } => write!(
                f,
                "object {object_index} managed-reference registry at {path} could not be decoded: {message}"
            ),
            Self::UnsupportedReferencedObjectData { object_index, path } => write!(
                f,
                "object {object_index} field {path} contains an unresolved ReferencedObjectData placeholder"
            ),
            Self::ManagedRegistryDepthExceeded {
                object_index,
                path,
                requested,
                limit,
            } => write!(
                f,
                "object {object_index} field {path} nests managed-reference registry depth {requested}, exceeding limit {limit}"
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
