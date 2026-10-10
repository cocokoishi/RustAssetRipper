use super::super::managed_types::managed_reference as managed_names;
use super::super::metadata::{TypeTree, TypeTreeNode};
use super::decode_error::{ObjectDecodeError, ObjectDecodeErrorKind};
pub(super) const ALIGN_BYTES_FLAG: u32 = 0x4000;
pub(super) const CHAR_PROPERTY_MASK_FLAG: u32 = 0x0200_0000;
pub(super) const MAX_TYPE_TREE_DEPTH: usize = 256;

pub(super) struct TypeTreeLayout<'a> {
    pub(super) nodes: &'a [TypeTreeNode],
    pub(super) first_child: Vec<Option<usize>>,
    pub(super) next_sibling: Vec<Option<usize>>,
}

impl<'a> TypeTreeLayout<'a> {
    pub(super) fn new(
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

pub(super) fn array_node_for_value(
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

pub(super) fn array_shape(
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

pub(super) fn is_string_node(
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
pub(super) fn pptr_shape(
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

pub(super) fn invalid_pptr_component(
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

pub(super) fn field_aligns(
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

pub(super) fn array_base_is_primitive(
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

pub(super) fn minimum_encoded_size(
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
pub(super) fn is_managed_reference_element(
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
pub(super) fn is_indexed_managed_reference_element(
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
pub(super) fn is_managed_references_registry(
    type_name: &str,
    node_name: &str,
    child_count: usize,
) -> bool {
    type_name == managed_names::REGISTRY_NAME
        && node_name == managed_names::REGISTRY_FIELD_NAME
        && child_count > 1
}

/// `TypeTreeNodeStruct.IsReferencedObjectData` over caller facts: the
/// `ReferencedObjectData` dynamic payload holder, which carries no subnodes because the
/// object content is read dynamically, not from the tree. A same-named node WITH
/// children is an ordinary struct here, not dynamic data.
pub(super) fn is_referenced_object_data(
    type_name: &str,
    node_name: &str,
    child_count: usize,
) -> bool {
    type_name == managed_names::REFERENCED_OBJECT_DATA_NAME
        && node_name == managed_names::DATA_FIELD_NAME
        && child_count == 0
}

pub(super) fn direct_children(
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

pub(super) fn reserve_option_indices(
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

pub(super) fn invalid_tree_levels(
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

pub(super) fn primitive_width(type_name: &str) -> Option<usize> {
    match type_name {
        "bool" | "char" | "UInt8" | "SInt8" => Some(1),
        "short" | "SInt16" | "ushort" | "UInt16" | "unsigned short" => Some(2),
        "int" | "SInt32" | "Type*" | "uint" | "UInt32" | "unsigned int" | "float" => Some(4),
        "SInt64" | "long long" | "UInt64" | "FileSize" | "unsigned long long" | "double" => Some(8),
        _ => None,
    }
}

pub(super) fn decode_primitive_name(type_name: &str) -> Option<()> {
    primitive_width(type_name).map(|_| ())
}

pub(super) fn type_name(node: &TypeTreeNode) -> &str {
    node.type_name.as_deref().unwrap_or("")
}

pub(super) fn allocation_error(offset: usize, path: &str, count: usize) -> ObjectDecodeError {
    ObjectDecodeError::new(
        offset,
        ObjectDecodeErrorKind::AllocationFailed {
            path: path.to_owned(),
            count,
        },
    )
}
