use super::super::{
    array_node_for_value, array_shape, direct_children, is_string_node, minimum_encoded_size,
    pptr_shape, SerializedFieldValue, SerializedValue, TypeTreeLayout, CHAR_PROPERTY_MASK_FLAG,
};
use super::error::{ObjectEncodeError, ObjectEncodeErrorKind};
use super::{child_path, invalid_tree};
use crate::serialized_file::metadata::{SerializedFileMetadata, TypeTree};
/// Bounds for constructing a zero/default value tree from a SerializedFile TypeTree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectDefaultValueLimits {
    /// Maximum TypeTree node count accepted before building a traversal layout.
    pub max_type_tree_nodes: usize,
    /// Maximum number of TypeTree-backed value nodes to materialize.
    pub max_value_nodes: usize,
    /// Maximum combined memory budget for value nodes, field names, and raw leaf bytes.
    pub max_allocation_bytes: usize,
    pub max_type_tree_depth: usize,
}

impl Default for ObjectDefaultValueLimits {
    fn default() -> Self {
        Self {
            max_type_tree_nodes: 1_000_000,
            max_value_nodes: 1_000_000,
            max_allocation_bytes: 256 * 1024 * 1024,
            max_type_tree_depth: 256,
        }
    }
}

impl SerializedFileMetadata {
    /// Construct a bounded zero/default value tree for an existing serialized type entry.
    ///
    /// Primitive fields are zeroed, PPtrs are null, strings and arrays are empty, and unknown
    /// fixed-size leaves are zero-filled. This is a schema-based constructor; callers still need
    /// to set fields whose Unity defaults are nonzero (for example, Transform rotation and scale).
    pub fn default_object_value_for_type(
        &self,
        object_index: usize,
        type_index: usize,
        limits: ObjectDefaultValueLimits,
    ) -> Result<SerializedValue, ObjectEncodeError> {
        self.default_value_for_type_tree_node(object_index, type_index, 0, limits)
    }

    /// Construct a bounded zero/default value for one node in an existing SerializedFile TypeTree.
    ///
    /// This supports schema-derived array element templates without materializing a parent object's
    /// variable-length arrays. The node index addresses `SerializedType.type_tree.nodes` directly.
    pub fn default_value_for_type_tree_node(
        &self,
        object_index: usize,
        type_index: usize,
        node_index: usize,
        limits: ObjectDefaultValueLimits,
    ) -> Result<SerializedValue, ObjectEncodeError> {
        let serialized_type = self.types.get(type_index).ok_or_else(|| {
            ObjectEncodeError::new(
                object_index,
                "<object>",
                ObjectEncodeErrorKind::InvalidResolvedTypeIndex {
                    type_index,
                    type_count: self.types.len(),
                },
            )
        })?;
        let type_tree = serialized_type.type_tree.as_ref().ok_or_else(|| {
            ObjectEncodeError::new(
                object_index,
                "<object>",
                ObjectEncodeErrorKind::MissingTypeTree,
            )
        })?;
        default_value_for_type_tree_node(type_tree, object_index, node_index, limits)
    }
}

/// Construct a bounded zero/default value for one node in a standalone TypeTree.
///
/// Unlike [`SerializedFileMetadata::default_value_for_type_tree_node`], this does not require the
/// TypeTree to be present in a SerializedFile type table. It is used for source-generated Unity
/// assets whose schema comes from AssetRipper's reference TPK database.
pub fn default_value_for_type_tree_node(
    type_tree: &TypeTree,
    object_index: usize,
    node_index: usize,
    limits: ObjectDefaultValueLimits,
) -> Result<SerializedValue, ObjectEncodeError> {
    if type_tree.nodes.is_empty() {
        return Err(ObjectEncodeError::new(
            object_index,
            "<object>",
            ObjectEncodeErrorKind::EmptyTypeTree,
        ));
    }
    if type_tree.nodes.len() > limits.max_type_tree_nodes {
        return Err(ObjectEncodeError::new(
            object_index,
            "<object>",
            ObjectEncodeErrorKind::DefaultTypeTreeNodeLimitExceeded {
                requested: type_tree.nodes.len(),
                limit: limits.max_type_tree_nodes,
            },
        ));
    }
    let layout = TypeTreeLayout::new(type_tree, object_index, 0)
        .map_err(|error| invalid_tree(object_index, "<object>", error))?;
    let mut builder = DefaultValueBuilder {
        type_tree: &layout,
        object_index,
        limits,
        value_nodes: 0,
        allocation_bytes: 0,
    };
    builder.validate_managed_reference_support_from(node_index)?;
    builder.build_node(node_index, "<type-tree-node>", 0)
}

struct DefaultValueBuilder<'layout, 'tree> {
    type_tree: &'layout TypeTreeLayout<'tree>,
    object_index: usize,
    limits: ObjectDefaultValueLimits,
    value_nodes: usize,
    allocation_bytes: usize,
}

impl DefaultValueBuilder<'_, '_> {
    fn validate_managed_reference_support_from(
        &self,
        root_node_index: usize,
    ) -> Result<(), ObjectEncodeError> {
        let root_level = self
            .type_tree
            .nodes
            .get(root_node_index)
            .ok_or_else(|| {
                self.invalid_tree(
                    "<type-tree-node>",
                    format!("node index {root_node_index} is out of range"),
                )
            })?
            .level;
        for (node_index, node) in self
            .type_tree
            .nodes
            .iter()
            .enumerate()
            .skip(root_node_index)
        {
            if node_index != root_node_index && node.level <= root_level {
                break;
            }
            let path = format!("TypeTree node {node_index}");
            let children = self.children(node_index, &path)?;
            let type_name = node.type_name.as_deref().unwrap_or("");
            let field_name = node.name.as_deref().unwrap_or("");
            let managed_reference_node =
                matches!(type_name, "managedReference" | "managedRefArrayItem")
                    && children.len() == 1
                    && matches!(
                        self.type_tree.nodes[children[0]].name.as_deref(),
                        Some("rid" | "id")
                    );
            let registry_node = type_name == "ManagedReferencesRegistry"
                && field_name == "references"
                && children.len() > 1;
            let dynamic_managed_data = type_name == "ReferencedObjectData" && field_name == "data";
            if managed_reference_node || registry_node || dynamic_managed_data {
                return Err(ObjectEncodeError::new(
                    self.object_index,
                    field_name,
                    ObjectEncodeErrorKind::UnsupportedManagedReferences {
                        type_name: type_name.to_owned(),
                        field_name: field_name.to_owned(),
                    },
                ));
            }
        }
        Ok(())
    }

    fn build_node(
        &mut self,
        node_index: usize,
        path: &str,
        depth: usize,
    ) -> Result<SerializedValue, ObjectEncodeError> {
        if depth > self.limits.max_type_tree_depth {
            return Err(self.invalid_tree(
                path,
                format!(
                    "nesting depth {depth} exceeds the limit of {}",
                    self.limits.max_type_tree_depth
                ),
            ));
        }
        self.charge_node(path)?;
        self.charge_allocation(path, std::mem::size_of::<SerializedValue>())?;

        let node = self.type_tree.nodes.get(node_index).ok_or_else(|| {
            ObjectEncodeError::new(
                self.object_index,
                path,
                ObjectEncodeErrorKind::InvalidTypeTree {
                    message: format!("node index {node_index} is out of range"),
                },
            )
        })?;
        let type_name = node.type_name.as_deref().unwrap_or("");
        let children = self.children(node_index, path)?;

        if matches!(type_name, "string" | "Utf8String") {
            if !is_string_node(self.type_tree, node_index, self.object_index, 0)
                .map_err(|error| invalid_tree(self.object_index, path, error))?
            {
                return Err(self.invalid_tree(path, "string node has an invalid TypeTree shape"));
            }
            return Ok(SerializedValue::StringBytes(Vec::new()));
        }

        if type_name == "TypelessData" {
            if array_shape(self.type_tree, node_index, self.object_index, 0)
                .map_err(|error| invalid_tree(self.object_index, path, error))?
                .is_none()
            {
                return Err(self.invalid_tree(
                    path,
                    "TypelessData node has an invalid TypeTree array shape",
                ));
            }
            return Ok(SerializedValue::Bytes(Vec::new()));
        }

        if let Some(array_node_index) =
            array_node_for_value(self.type_tree, node_index, self.object_index, 0)
                .map_err(|error| invalid_tree(self.object_index, path, error))?
        {
            minimum_encoded_size(
                self.type_tree,
                array_node_index,
                self.object_index,
                0,
                depth + 1,
            )
            .map_err(|error| invalid_tree(self.object_index, path, error))?;
            return Ok(SerializedValue::Array(Vec::new()));
        }
        if matches!(type_name, "vector" | "staticvector" | "set" | "map") {
            return Err(self.invalid_tree(path, "container node has an invalid array shape"));
        }

        if type_name.starts_with("PPtr<") {
            let Some((file_id_node, path_id_node)) =
                pptr_shape(self.type_tree, node_index, self.object_index, 0)
                    .map_err(|error| invalid_tree(self.object_index, path, error))?
            else {
                return Err(self.invalid_tree(path, "PPtr node has an invalid component shape"));
            };
            let file_id_type = self.type_tree.nodes[file_id_node]
                .type_name
                .as_deref()
                .unwrap_or("");
            if !matches!(file_id_type, "int" | "SInt32" | "Type*") {
                return Err(self.invalid_tree(path, "PPtr file ID node is not 32-bit"));
            }
            let path_id_type = self.type_tree.nodes[path_id_node]
                .type_name
                .as_deref()
                .unwrap_or("");
            if !matches!(
                path_id_type,
                "int" | "SInt32" | "Type*" | "SInt64" | "long long"
            ) {
                return Err(self.invalid_tree(path, "PPtr PathID node is not 32-bit or 64-bit"));
            }
            return Ok(SerializedValue::PPtr {
                file_id: 0,
                path_id: 0,
            });
        }

        if let Some(value) = default_primitive(type_name, node.meta_flags) {
            return Ok(value);
        }

        if !children.is_empty() {
            let field_bytes = children
                .len()
                .checked_mul(std::mem::size_of::<SerializedFieldValue>())
                .ok_or_else(|| {
                    ObjectEncodeError::new(
                        self.object_index,
                        path,
                        ObjectEncodeErrorKind::LengthOverflow {
                            resource: "default struct field storage",
                        },
                    )
                })?;
            self.charge_allocation(path, field_bytes)?;
            let mut fields = Vec::new();
            fields.try_reserve_exact(children.len()).map_err(|_| {
                ObjectEncodeError::new(
                    self.object_index,
                    path,
                    ObjectEncodeErrorKind::AllocationFailed {
                        resource: "default struct fields",
                        requested: children.len(),
                    },
                )
            })?;
            for child_index in children {
                let name = self.type_tree.nodes[child_index]
                    .name
                    .as_deref()
                    .unwrap_or("");
                self.charge_allocation(path, name.len())?;
                let mut owned_name = String::new();
                owned_name.try_reserve_exact(name.len()).map_err(|_| {
                    ObjectEncodeError::new(
                        self.object_index,
                        path,
                        ObjectEncodeErrorKind::AllocationFailed {
                            resource: "default field name",
                            requested: name.len(),
                        },
                    )
                })?;
                owned_name.push_str(name);
                let child_path = child_path(path, name);
                let value = self.build_node(child_index, &child_path, depth + 1)?;
                fields.push(SerializedFieldValue {
                    name: owned_name,
                    value,
                });
            }
            return Ok(SerializedValue::Struct(fields));
        }

        let size = usize::try_from(node.byte_size).map_err(|_| {
            self.invalid_tree(
                path,
                format!(
                    "unknown leaf '{type_name}' has invalid byte size {}",
                    node.byte_size
                ),
            )
        })?;
        self.charge_allocation(path, size)?;
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(size).map_err(|_| {
            ObjectEncodeError::new(
                self.object_index,
                path,
                ObjectEncodeErrorKind::AllocationFailed {
                    resource: "default raw leaf bytes",
                    requested: size,
                },
            )
        })?;
        bytes.resize(size, 0);
        self.charge_allocation(path, type_name.len())?;
        let mut raw_type_name = String::new();
        raw_type_name
            .try_reserve_exact(type_name.len())
            .map_err(|_| {
                ObjectEncodeError::new(
                    self.object_index,
                    path,
                    ObjectEncodeErrorKind::AllocationFailed {
                        resource: "default raw leaf type name",
                        requested: type_name.len(),
                    },
                )
            })?;
        raw_type_name.push_str(type_name);
        Ok(SerializedValue::Raw {
            type_name: raw_type_name,
            bytes,
        })
    }

    fn children(&self, node_index: usize, path: &str) -> Result<Vec<usize>, ObjectEncodeError> {
        direct_children(self.type_tree, node_index, self.object_index, 0)
            .map_err(|error| invalid_tree(self.object_index, path, error))
    }

    fn charge_node(&mut self, path: &str) -> Result<(), ObjectEncodeError> {
        let requested = self.value_nodes.checked_add(1).ok_or_else(|| {
            ObjectEncodeError::new(
                self.object_index,
                path,
                ObjectEncodeErrorKind::LengthOverflow {
                    resource: "default value node count",
                },
            )
        })?;
        if requested > self.limits.max_value_nodes {
            return Err(ObjectEncodeError::new(
                self.object_index,
                path,
                ObjectEncodeErrorKind::DefaultValueNodeLimitExceeded {
                    requested,
                    limit: self.limits.max_value_nodes,
                },
            ));
        }
        self.value_nodes = requested;
        Ok(())
    }

    fn charge_allocation(&mut self, path: &str, amount: usize) -> Result<(), ObjectEncodeError> {
        let requested = self.allocation_bytes.checked_add(amount).ok_or_else(|| {
            ObjectEncodeError::new(
                self.object_index,
                path,
                ObjectEncodeErrorKind::LengthOverflow {
                    resource: "default value allocation budget",
                },
            )
        })?;
        if requested > self.limits.max_allocation_bytes {
            return Err(ObjectEncodeError::new(
                self.object_index,
                path,
                ObjectEncodeErrorKind::DefaultValueAllocationLimitExceeded {
                    requested,
                    limit: self.limits.max_allocation_bytes,
                },
            ));
        }
        self.allocation_bytes = requested;
        Ok(())
    }

    fn invalid_tree(&self, path: &str, message: impl Into<String>) -> ObjectEncodeError {
        ObjectEncodeError::new(
            self.object_index,
            path,
            ObjectEncodeErrorKind::InvalidTypeTree {
                message: message.into(),
            },
        )
    }
}

fn default_primitive(type_name: &str, meta_flags: u32) -> Option<SerializedValue> {
    Some(match type_name {
        "bool" => SerializedValue::Bool(false),
        "char" | "UInt8" => SerializedValue::U8(0),
        "SInt8" => SerializedValue::I8(0),
        "short" | "SInt16" => SerializedValue::I16(0),
        "ushort" | "UInt16" | "unsigned short" => {
            if meta_flags & CHAR_PROPERTY_MASK_FLAG != 0 {
                SerializedValue::Char(0)
            } else {
                SerializedValue::U16(0)
            }
        }
        "int" | "SInt32" | "Type*" => SerializedValue::I32(0),
        "uint" | "UInt32" | "unsigned int" => SerializedValue::U32(0),
        "SInt64" | "long long" => SerializedValue::I64(0),
        "UInt64" | "FileSize" | "unsigned long long" => SerializedValue::U64(0),
        "float" => SerializedValue::F32(0.0),
        "double" => SerializedValue::F64(0.0),
        _ => return None,
    })
}
