use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::io::binary::Endian;

use super::super::metadata::SerializedFileMetadata;
use super::{
    array_base_is_primitive, array_node_for_value, array_shape, direct_children, field_aligns,
    is_string_node, minimum_encoded_size, pptr_shape, unity_version_aligns_arrays,
    ParsedObjectPayload, SerializedFieldValue, SerializedValue, TypeTreeLayout,
    CHAR_PROPERTY_MASK_FLAG,
};

/// Bounds for encoding one TypeTree-backed SerializedFile object payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectEncodeLimits {
    pub max_encoded_payload_bytes: usize,
    pub max_array_elements: usize,
    pub max_string_bytes: usize,
    pub max_type_tree_depth: usize,
}

impl Default for ObjectEncodeLimits {
    fn default() -> Self {
        Self {
            max_encoded_payload_bytes: 256 * 1024 * 1024,
            max_array_elements: 16 * 1024 * 1024,
            max_string_bytes: 64 * 1024 * 1024,
            max_type_tree_depth: 256,
        }
    }
}

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

/// Error produced when a typed value cannot be encoded using its SerializedFile TypeTree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectEncodeError {
    object_index: usize,
    path: String,
    kind: ObjectEncodeErrorKind,
}

impl ObjectEncodeError {
    fn new(object_index: usize, path: impl Into<String>, kind: ObjectEncodeErrorKind) -> Self {
        Self {
            object_index,
            path: path.into(),
            kind,
        }
    }

    pub fn object_index(&self) -> usize {
        self.object_index
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn kind(&self) -> &ObjectEncodeErrorKind {
        &self.kind
    }
}

impl Display for ObjectEncodeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "could not encode SerializedFile object {} at '{}': {}",
            self.object_index, self.path, self.kind
        )
    }
}

impl Error for ObjectEncodeError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObjectEncodeErrorKind {
    InvalidObjectIndex {
        object_count: usize,
    },
    PayloadIdentityMismatch {
        payload_path_id: i64,
        table_path_id: i64,
    },
    ObjectTypeUnresolved,
    InvalidResolvedTypeIndex {
        type_index: usize,
        type_count: usize,
    },
    MissingTypeTree,
    EmptyTypeTree,
    InvalidTypeTree {
        message: String,
    },
    UnsupportedManagedReferences {
        type_name: String,
        field_name: String,
    },
    InvalidManagedReferenceEncoding {
        reason: String,
    },
    ValueTypeMismatch {
        expected: String,
        actual: &'static str,
    },
    StructFieldCountMismatch {
        expected: usize,
        actual: usize,
    },
    StructFieldNameMismatch {
        field_index: usize,
        expected: String,
        actual: String,
    },
    ArrayElementLimitExceeded {
        requested: usize,
        limit: usize,
    },
    DefaultValueNodeLimitExceeded {
        requested: usize,
        limit: usize,
    },
    DefaultValueAllocationLimitExceeded {
        requested: usize,
        limit: usize,
    },
    DefaultTypeTreeNodeLimitExceeded {
        requested: usize,
        limit: usize,
    },
    ZeroWidthArrayElement {
        count: usize,
    },
    StringByteLimitExceeded {
        requested: usize,
        limit: usize,
    },
    EncodedPayloadLimitExceeded {
        requested: usize,
        limit: usize,
    },
    LengthOverflow {
        resource: &'static str,
    },
    RawLeafSizeMismatch {
        expected: usize,
        actual: usize,
    },
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
}

impl Display for ObjectEncodeErrorKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidObjectIndex { object_count } => {
                write!(f, "object index is outside the {object_count}-entry table")
            }
            Self::PayloadIdentityMismatch {
                payload_path_id,
                table_path_id,
            } => write!(
                f,
                "payload PathID {payload_path_id} does not match table PathID {table_path_id}"
            ),
            Self::ObjectTypeUnresolved => f.write_str("object type is unresolved"),
            Self::InvalidResolvedTypeIndex {
                type_index,
                type_count,
            } => write!(
                f,
                "resolved type index {type_index} is outside the {type_count}-entry type table"
            ),
            Self::MissingTypeTree => f.write_str("resolved object type has no TypeTree"),
            Self::EmptyTypeTree => f.write_str("resolved object TypeTree has no root node"),
            Self::InvalidTypeTree { message } => write!(f, "invalid TypeTree: {message}"),
            Self::UnsupportedManagedReferences {
                type_name,
                field_name,
            } => write!(
                f,
                "managed-reference shape '{type_name}' at '{field_name}' is not supported by this encoder"
            ),
            Self::InvalidManagedReferenceEncoding { reason } => {
                write!(f, "managed-reference value cannot be encoded: {reason}")
            }
            Self::ValueTypeMismatch { expected, actual } => {
                write!(f, "TypeTree expects {expected}, but value is {actual}")
            }
            Self::StructFieldCountMismatch { expected, actual } => write!(
                f,
                "TypeTree struct has {expected} fields but value has {actual}"
            ),
            Self::StructFieldNameMismatch {
                field_index,
                expected,
                actual,
            } => write!(
                f,
                "struct field {field_index} is named '{actual}', expected '{expected}'"
            ),
            Self::ArrayElementLimitExceeded { requested, limit } => write!(
                f,
                "array has {requested} elements, exceeding the limit of {limit}"
            ),
            Self::DefaultValueNodeLimitExceeded { requested, limit } => write!(
                f,
                "default value has {requested} nodes, exceeding the limit of {limit}"
            ),
            Self::DefaultValueAllocationLimitExceeded { requested, limit } => write!(
                f,
                "default value allocation would reach {requested} bytes, exceeding the limit of {limit}"
            ),
            Self::DefaultTypeTreeNodeLimitExceeded { requested, limit } => write!(
                f,
                "TypeTree has {requested} nodes, exceeding the default-value limit of {limit}"
            ),
            Self::ZeroWidthArrayElement { count } => write!(
                f,
                "array has {count} elements whose TypeTree encoding has zero width"
            ),
            Self::StringByteLimitExceeded { requested, limit } => write!(
                f,
                "string has {requested} bytes, exceeding the limit of {limit}"
            ),
            Self::EncodedPayloadLimitExceeded { requested, limit } => write!(
                f,
                "encoded payload would contain {requested} bytes, exceeding the limit of {limit}"
            ),
            Self::LengthOverflow { resource } => write!(f, "{resource} length overflows its field"),
            Self::RawLeafSizeMismatch { expected, actual } => write!(
                f,
                "raw TypeTree leaf requires {expected} bytes but value has {actual}"
            ),
            Self::AllocationFailed { resource, requested } => {
                write!(f, "could not reserve {requested} bytes for {resource}")
            }
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

    /// Encode one decoded object's typed root and retained trailing bytes.
    ///
    /// This writes only the object payload. It does not rebuild the enclosing SerializedFile's
    /// object sizes, offsets, metadata tables, or header.
    pub fn encode_object_payload(
        &self,
        payload: &ParsedObjectPayload,
        limits: ObjectEncodeLimits,
    ) -> Result<Vec<u8>, ObjectEncodeError> {
        let object = self.objects.get(payload.object_index).ok_or_else(|| {
            ObjectEncodeError::new(
                payload.object_index,
                "<object>",
                ObjectEncodeErrorKind::InvalidObjectIndex {
                    object_count: self.objects.len(),
                },
            )
        })?;
        if object.path_id != payload.path_id {
            return Err(ObjectEncodeError::new(
                payload.object_index,
                "<object>",
                ObjectEncodeErrorKind::PayloadIdentityMismatch {
                    payload_path_id: payload.path_id,
                    table_path_id: object.path_id,
                },
            ));
        }
        let type_index = object.resolved_type_index.ok_or_else(|| {
            ObjectEncodeError::new(
                payload.object_index,
                "<object>",
                ObjectEncodeErrorKind::ObjectTypeUnresolved,
            )
        })?;
        self.encode_payload_for_type_with_capacity_hint(
            payload.object_index,
            type_index,
            &payload.root,
            payload.consumed_bytes,
            &payload.trailing_bytes,
            limits,
        )
    }

    /// Encode a new typed payload using an existing serialized type-table entry.
    ///
    /// This is useful when creating an object record whose class already has a serialized type
    /// in the file. The caller supplies the object index to use in diagnostics; this method does
    /// not check that the index already exists in the object table.
    pub fn encode_object_payload_for_type(
        &self,
        object_index: usize,
        type_index: usize,
        root: &SerializedValue,
        trailing_bytes: &[u8],
        limits: ObjectEncodeLimits,
    ) -> Result<Vec<u8>, ObjectEncodeError> {
        self.encode_payload_for_type_with_capacity_hint(
            object_index,
            type_index,
            root,
            0,
            trailing_bytes,
            limits,
        )
    }

    fn encode_payload_for_type_with_capacity_hint(
        &self,
        object_index: usize,
        type_index: usize,
        value: &SerializedValue,
        consumed_bytes_hint: usize,
        trailing_bytes: &[u8],
        limits: ObjectEncodeLimits,
    ) -> Result<Vec<u8>, ObjectEncodeError> {
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
        let root = type_tree.nodes.first().ok_or_else(|| {
            ObjectEncodeError::new(
                object_index,
                "<object>",
                ObjectEncodeErrorKind::EmptyTypeTree,
            )
        })?;
        let layout = TypeTreeLayout::new(type_tree, object_index, 0)
            .map_err(|error| invalid_tree(object_index, "<object>", error))?;
        if trailing_bytes.len() > limits.max_encoded_payload_bytes {
            return Err(ObjectEncodeError::new(
                object_index,
                "<object>",
                ObjectEncodeErrorKind::EncodedPayloadLimitExceeded {
                    requested: trailing_bytes.len(),
                    limit: limits.max_encoded_payload_bytes,
                },
            ));
        }
        // The decoded size is only a capacity hint: callers may shrink an array before encoding.
        // Cap the hint so a caller-controlled `consumed_bytes` value cannot trigger a huge eager
        // allocation, then let checked writes grow the buffer as needed.
        let capacity_hint = consumed_bytes_hint.saturating_add(trailing_bytes.len());
        let requested_capacity = capacity_hint
            .min(limits.max_encoded_payload_bytes)
            .min(1024 * 1024);
        let mut output = Vec::new();
        output.try_reserve_exact(requested_capacity).map_err(|_| {
            ObjectEncodeError::new(
                object_index,
                "<object>",
                ObjectEncodeErrorKind::AllocationFailed {
                    resource: "object payload buffer",
                    requested: requested_capacity,
                },
            )
        })?;
        let mut encoder = PayloadEncoder {
            type_tree: &layout,
            endian: self.prefix.body_endian,
            arrays_align: unity_version_aligns_arrays(self.prefix.unity_version_text.as_deref()),
            reference_types: &self.reference_types,
            allow_managed_registry: true,
            object_index,
            limits,
            output,
        };
        encoder.encode_root(value, root.type_name.as_deref().unwrap_or(""))?;
        encoder.write_bytes("<trailing>", trailing_bytes)?;
        Ok(encoder.output)
    }
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

struct PayloadEncoder<'layout, 'tree, 'references> {
    type_tree: &'layout TypeTreeLayout<'tree>,
    endian: Endian,
    arrays_align: bool,
    reference_types: &'references [super::super::metadata::SerializedTypeReference],
    allow_managed_registry: bool,
    object_index: usize,
    limits: ObjectEncodeLimits,
    output: Vec<u8>,
}

impl PayloadEncoder<'_, '_, '_> {
    fn encode_root(
        &mut self,
        value: &SerializedValue,
        root_name: &str,
    ) -> Result<(), ObjectEncodeError> {
        self.encode_node_value(0, value, root_name, 0, false)
    }

    fn encode_managed_registry(
        &mut self,
        registry: &super::super::managed_registry::ManagedRegistryRead,
        path: &str,
        depth: usize,
    ) -> Result<(), ObjectEncodeError> {
        use super::super::managed_registry::{
            frame_registry_entry, COUNTED_FORMAT, TERMINATED_FORMAT, TERMINUS_ASSEMBLY_NAME,
            TERMINUS_CLASS_NAME, TERMINUS_NAMESPACE,
        };

        if registry.entries.len() > self.limits.max_array_elements {
            return Err(ObjectEncodeError::new(
                self.object_index,
                path,
                ObjectEncodeErrorKind::ArrayElementLimitExceeded {
                    requested: registry.entries.len(),
                    limit: self.limits.max_array_elements,
                },
            ));
        }
        if !matches!(registry.version, TERMINATED_FORMAT | COUNTED_FORMAT) {
            return Err(ObjectEncodeError::new(
                self.object_index,
                path,
                ObjectEncodeErrorKind::InvalidManagedReferenceEncoding {
                    reason: format!("unsupported registry format {}", registry.version),
                },
            ));
        }
        self.write_i32(path, registry.version)?;
        if registry.version == COUNTED_FORMAT {
            let count = i32::try_from(registry.entries.len()).map_err(|_| {
                ObjectEncodeError::new(
                    self.object_index,
                    path,
                    ObjectEncodeErrorKind::LengthOverflow {
                        resource: "managed-reference registry entry count",
                    },
                )
            })?;
            self.write_i32(path, count)?;
        }

        for (entry_index, entry) in registry.entries.iter().enumerate() {
            let entry_path = format!("{path}.RefIds[{entry_index}]");
            if registry.version == TERMINATED_FORMAT {
                if entry.reference_id != entry_index as i64 {
                    return Err(ObjectEncodeError::new(
                        self.object_index,
                        &entry_path,
                        ObjectEncodeErrorKind::InvalidManagedReferenceEncoding {
                            reason: format!(
                                "terminated registry entry has ID {}, expected its index {entry_index}",
                                entry.reference_id
                            ),
                        },
                    ));
                }
            } else {
                self.write_bytes(
                    &entry_path,
                    &encode_u64_bytes(entry.reference_id as u64, self.endian),
                )?;
            }
            self.write_registry_string(&entry_path, "class", &entry.class_name)?;
            self.write_registry_string(&entry_path, "ns", &entry.namespace_name)?;
            self.write_registry_string(&entry_path, "asm", &entry.assembly_name)?;

            let frame = frame_registry_entry(
                entry.reference_id,
                entry.class_name.is_empty(),
                entry.assembly_name.is_empty(),
            );
            match (&entry.data, frame.has_content) {
                (
                    super::super::managed_registry::ManagedRegistryEntryData::Content(value),
                    true,
                ) => self.encode_managed_registry_content(
                    entry_index,
                    entry,
                    value,
                    &entry_path,
                    depth + 1,
                )?,
                (super::super::managed_registry::ManagedRegistryEntryData::Empty, false) => {}
                (super::super::managed_registry::ManagedRegistryEntryData::Content(_), false) => {
                    return Err(ObjectEncodeError::new(
                        self.object_index,
                        &entry_path,
                        ObjectEncodeErrorKind::InvalidManagedReferenceEncoding {
                            reason: "a null, unknown, or empty-type entry carries content"
                                .to_owned(),
                        },
                    ));
                }
                (super::super::managed_registry::ManagedRegistryEntryData::Empty, true) => {
                    return Err(ObjectEncodeError::new(
                        self.object_index,
                        &entry_path,
                        ObjectEncodeErrorKind::InvalidManagedReferenceEncoding {
                            reason: "a content-bearing entry has no decoded payload".to_owned(),
                        },
                    ));
                }
            }
        }

        if registry.version == TERMINATED_FORMAT {
            self.write_registry_string(path, "terminus class", TERMINUS_CLASS_NAME)?;
            self.write_registry_string(path, "terminus namespace", TERMINUS_NAMESPACE)?;
            self.write_registry_string(path, "terminus assembly", TERMINUS_ASSEMBLY_NAME)?;
        }
        self.align_four(path)
    }

    fn write_registry_string(
        &mut self,
        path: &str,
        field_name: &str,
        value: &str,
    ) -> Result<(), ObjectEncodeError> {
        let string_path = format!("{path}.type.{field_name}");
        if value.len() > self.limits.max_string_bytes {
            return Err(ObjectEncodeError::new(
                self.object_index,
                string_path,
                ObjectEncodeErrorKind::StringByteLimitExceeded {
                    requested: value.len(),
                    limit: self.limits.max_string_bytes,
                },
            ));
        }
        let length = i32::try_from(value.len()).map_err(|_| {
            ObjectEncodeError::new(
                self.object_index,
                &string_path,
                ObjectEncodeErrorKind::LengthOverflow {
                    resource: "managed-reference type-name length",
                },
            )
        })?;
        self.write_i32(&string_path, length)?;
        self.write_bytes(&string_path, value.as_bytes())?;
        self.align_four(&string_path)
    }

    fn encode_managed_registry_content(
        &mut self,
        entry_index: usize,
        entry: &super::super::managed_registry::ManagedRegistryEntry,
        value: &SerializedValue,
        path: &str,
        depth: usize,
    ) -> Result<(), ObjectEncodeError> {
        use crate::io::special_names::remove_assembly_file_extension;
        use crate::processing::script_identity::{
            find_type_reference_index, ScriptIdentifier, TYPE_REFERENCE_NOT_FOUND,
            TYPE_TREE_NODE_BUILD_FAILED,
        };

        let script = ScriptIdentifier {
            assembly: remove_assembly_file_extension(&entry.assembly_name),
            namespace: entry.namespace_name.clone(),
            name: Some(entry.class_name.clone()),
        };
        let type_index =
            find_type_reference_index(self.reference_types, &script).ok_or_else(|| {
                ObjectEncodeError::new(
                    self.object_index,
                    path,
                    ObjectEncodeErrorKind::InvalidManagedReferenceEncoding {
                        reason: format!(
                            "could not resolve {}: {TYPE_REFERENCE_NOT_FOUND}",
                            script.unique_name()
                        ),
                    },
                )
            })?;
        let source_tree = self.reference_types[type_index]
            .type_tree
            .as_ref()
            .ok_or_else(|| {
                ObjectEncodeError::new(
                    self.object_index,
                    path,
                    ObjectEncodeErrorKind::InvalidManagedReferenceEncoding {
                        reason: format!(
                            "could not build {}: {TYPE_TREE_NODE_BUILD_FAILED}",
                            script.unique_name()
                        ),
                    },
                )
            })?;
        let built = crate::processing::script_identity::build_type_nodes(&source_tree.nodes)
            .ok_or_else(|| {
                ObjectEncodeError::new(
                    self.object_index,
                    path,
                    ObjectEncodeErrorKind::InvalidManagedReferenceEncoding {
                        reason: format!(
                            "could not build {}: {TYPE_TREE_NODE_BUILD_FAILED}",
                            script.unique_name()
                        ),
                    },
                )
            })?;
        let payload_tree = super::super::managed_registry::flatten_built_tree(&built);
        let payload_tree = super::super::metadata::TypeTree {
            nodes: payload_tree,
            string_buffer: Vec::new(),
        };
        let root_name = payload_tree
            .nodes
            .first()
            .and_then(|node| node.type_name.as_deref())
            .ok_or_else(|| {
                ObjectEncodeError::new(
                    self.object_index,
                    path,
                    ObjectEncodeErrorKind::InvalidManagedReferenceEncoding {
                        reason: format!(
                            "{} has an empty referenced TypeTree",
                            script.unique_name()
                        ),
                    },
                )
            })?;
        let layout = TypeTreeLayout::new(&payload_tree, self.object_index, self.output.len())
            .map_err(|error| invalid_tree(self.object_index, path, error))?;
        let remaining_payload_bytes = self
            .limits
            .max_encoded_payload_bytes
            .saturating_sub(self.output.len());
        let dynamic_limits = ObjectEncodeLimits {
            max_encoded_payload_bytes: remaining_payload_bytes,
            ..self.limits
        };
        let mut encoder = PayloadEncoder {
            type_tree: &layout,
            endian: self.endian,
            arrays_align: self.arrays_align,
            reference_types: self.reference_types,
            allow_managed_registry: true,
            object_index: self.object_index,
            limits: dynamic_limits,
            output: Vec::new(),
        };
        encoder
            .encode_node_value(0, value, root_name, depth, false)
            .map_err(|error| {
                ObjectEncodeError::new(
                    self.object_index,
                    path,
                    ObjectEncodeErrorKind::InvalidManagedReferenceEncoding {
                        reason: format!("registry entry {entry_index}: {error}"),
                    },
                )
            })?;
        self.write_bytes(path, &encoder.output)
    }

    fn encode_field(
        &mut self,
        node_index: usize,
        value: &SerializedValue,
        path: &str,
        depth: usize,
    ) -> Result<(), ObjectEncodeError> {
        let aligned = field_aligns(
            self.type_tree,
            node_index,
            self.object_index,
            self.output.len(),
        )
        .map_err(|error| invalid_tree(self.object_index, path, error))?;
        self.encode_node_value(node_index, value, path, depth, aligned)?;
        if aligned {
            self.align_four(path)?;
        }
        Ok(())
    }

    fn encode_node_value(
        &mut self,
        node_index: usize,
        value: &SerializedValue,
        path: &str,
        depth: usize,
        align_nested_arrays: bool,
    ) -> Result<(), ObjectEncodeError> {
        if depth > self.limits.max_type_tree_depth {
            return Err(ObjectEncodeError::new(
                self.object_index,
                path,
                ObjectEncodeErrorKind::InvalidTypeTree {
                    message: format!(
                        "nesting depth {depth} exceeds the limit of {}",
                        self.limits.max_type_tree_depth
                    ),
                },
            ));
        }
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
        let field_name = node.name.as_deref().unwrap_or("");
        let children = self.children(node_index, path)?;

        if super::is_managed_reference_element(
            self.type_tree,
            node_index,
            self.object_index,
            self.output.len(),
        )
        .map_err(|error| invalid_tree(self.object_index, path, error))?
        {
            let indexed = super::is_indexed_managed_reference_element(
                self.type_tree,
                node_index,
                self.object_index,
                self.output.len(),
            )
            .map_err(|error| invalid_tree(self.object_index, path, error))?;
            let SerializedValue::ManagedReferenceId {
                reference_id,
                indexed: value_indexed,
            } = value
            else {
                return Err(self.value_mismatch(path, "managed-reference ID", value));
            };
            if indexed != *value_indexed {
                return Err(ObjectEncodeError::new(
                    self.object_index,
                    path,
                    ObjectEncodeErrorKind::InvalidManagedReferenceEncoding {
                        reason: format!(
                            "TypeTree uses the {} identifier but value uses the {} identifier",
                            if indexed {
                                "legacy index"
                            } else {
                                "stable rid"
                            },
                            if *value_indexed {
                                "legacy index"
                            } else {
                                "stable rid"
                            }
                        ),
                    },
                ));
            }
            let child_index = *children.first().ok_or_else(|| {
                ObjectEncodeError::new(
                    self.object_index,
                    path,
                    ObjectEncodeErrorKind::InvalidTypeTree {
                        message: "managed-reference identifier child is missing".to_owned(),
                    },
                )
            })?;
            let id_name = self.type_tree.nodes[child_index]
                .name
                .as_deref()
                .unwrap_or(if indexed { "id" } else { "rid" });
            let id_path = child_path(path, id_name);
            let id_value = if indexed {
                SerializedValue::I32(i32::try_from(*reference_id).map_err(|_| {
                    ObjectEncodeError::new(
                        self.object_index,
                        path,
                        ObjectEncodeErrorKind::InvalidManagedReferenceEncoding {
                            reason: format!(
                                "indexed managed-reference ID {reference_id} is outside i32"
                            ),
                        },
                    )
                })?)
            } else {
                SerializedValue::I64(*reference_id)
            };
            self.encode_field(child_index, &id_value, &id_path, depth + 1)?;
            return Ok(());
        }

        if super::is_managed_references_registry(type_name, field_name, children.len()) {
            if !self.allow_managed_registry {
                return Err(ObjectEncodeError::new(
                    self.object_index,
                    path,
                    ObjectEncodeErrorKind::UnsupportedManagedReferences {
                        type_name: type_name.to_owned(),
                        field_name: field_name.to_owned(),
                    },
                ));
            }
            let SerializedValue::ManagedReferenceRegistry(registry) = value else {
                return Err(self.value_mismatch(path, "managed-reference registry", value));
            };
            self.encode_managed_registry(registry, path, depth + 1)?;
            return Ok(());
        }

        if super::is_referenced_object_data(type_name, field_name, children.len()) {
            return Err(ObjectEncodeError::new(
                self.object_index,
                path,
                ObjectEncodeErrorKind::UnsupportedManagedReferences {
                    type_name: type_name.to_owned(),
                    field_name: field_name.to_owned(),
                },
            ));
        }

        if matches!(type_name, "string" | "Utf8String")
            && is_string_node(
                self.type_tree,
                node_index,
                self.object_index,
                self.output.len(),
            )
            .map_err(|error| invalid_tree(self.object_index, path, error))?
        {
            let SerializedValue::StringBytes(bytes) = value else {
                return Err(self.value_mismatch(path, "string", value));
            };
            if bytes.len() > self.limits.max_string_bytes {
                return Err(ObjectEncodeError::new(
                    self.object_index,
                    path,
                    ObjectEncodeErrorKind::StringByteLimitExceeded {
                        requested: bytes.len(),
                        limit: self.limits.max_string_bytes,
                    },
                ));
            }
            let length = i32::try_from(bytes.len()).map_err(|_| {
                ObjectEncodeError::new(
                    self.object_index,
                    path,
                    ObjectEncodeErrorKind::LengthOverflow {
                        resource: "Unity string byte length",
                    },
                )
            })?;
            self.write_i32(path, length)?;
            self.write_bytes(path, bytes)?;
            return self.align_four(path);
        }

        if type_name == "TypelessData"
            && array_shape(
                self.type_tree,
                node_index,
                self.object_index,
                self.output.len(),
            )
            .map_err(|error| invalid_tree(self.object_index, path, error))?
            .is_some()
        {
            let SerializedValue::Bytes(bytes) = value else {
                return Err(self.value_mismatch(path, "TypelessData bytes", value));
            };
            if bytes.len() > self.limits.max_array_elements {
                return Err(ObjectEncodeError::new(
                    self.object_index,
                    path,
                    ObjectEncodeErrorKind::ArrayElementLimitExceeded {
                        requested: bytes.len(),
                        limit: self.limits.max_array_elements,
                    },
                ));
            }
            let length = i32::try_from(bytes.len()).map_err(|_| {
                ObjectEncodeError::new(
                    self.object_index,
                    path,
                    ObjectEncodeErrorKind::LengthOverflow {
                        resource: "TypelessData byte length",
                    },
                )
            })?;
            self.write_i32(path, length)?;
            self.write_bytes(path, bytes)?;
            if self.arrays_align || align_nested_arrays {
                self.align_four(path)?;
            }
            return Ok(());
        }

        if let Some(array_node_index) = array_node_for_value(
            self.type_tree,
            node_index,
            self.object_index,
            self.output.len(),
        )
        .map_err(|error| invalid_tree(self.object_index, path, error))?
        {
            return self.encode_array(
                array_node_index,
                value,
                path,
                depth + 1,
                align_nested_arrays,
            );
        }

        if let Some((file_id_node, path_id_node)) = pptr_shape(
            self.type_tree,
            node_index,
            self.object_index,
            self.output.len(),
        )
        .map_err(|error| invalid_tree(self.object_index, path, error))?
        {
            let SerializedValue::PPtr { file_id, path_id } = value else {
                return Err(self.value_mismatch(path, "PPtr", value));
            };
            let file_name = self.type_tree.nodes[file_id_node]
                .name
                .as_deref()
                .unwrap_or("m_FileID");
            let file_path = child_path(path, file_name);
            self.encode_field(
                file_id_node,
                &SerializedValue::I32(*file_id),
                &file_path,
                depth + 1,
            )?;

            let path_node = &self.type_tree.nodes[path_id_node];
            let path_value = match primitive_width(path_node.type_name.as_deref().unwrap_or("")) {
                Some(4) => SerializedValue::I32(i32::try_from(*path_id).map_err(|_| {
                    ObjectEncodeError::new(
                        self.object_index,
                        path,
                        ObjectEncodeErrorKind::LengthOverflow {
                            resource: "32-bit PPtr PathID",
                        },
                    )
                })?),
                Some(8) => SerializedValue::I64(*path_id),
                _ => {
                    return Err(ObjectEncodeError::new(
                        self.object_index,
                        path,
                        ObjectEncodeErrorKind::InvalidTypeTree {
                            message: "PPtr PathID node is not a 32-bit or 64-bit integer"
                                .to_owned(),
                        },
                    ));
                }
            };
            let path_name = path_node.name.as_deref().unwrap_or("m_PathID");
            let path_path = child_path(path, path_name);
            self.encode_field(path_id_node, &path_value, &path_path, depth + 1)?;
            return Ok(());
        }

        if self.encode_primitive(node_index, value, path)? {
            return Ok(());
        }

        if !children.is_empty() {
            let SerializedValue::Struct(fields) = value else {
                return Err(self.value_mismatch(path, "struct", value));
            };
            if fields.len() != children.len() {
                return Err(ObjectEncodeError::new(
                    self.object_index,
                    path,
                    ObjectEncodeErrorKind::StructFieldCountMismatch {
                        expected: children.len(),
                        actual: fields.len(),
                    },
                ));
            }
            for (field_index, (child_index, field)) in children.into_iter().zip(fields).enumerate()
            {
                let expected_name = self.type_tree.nodes[child_index]
                    .name
                    .as_deref()
                    .unwrap_or("");
                if field.name != expected_name {
                    return Err(ObjectEncodeError::new(
                        self.object_index,
                        path,
                        ObjectEncodeErrorKind::StructFieldNameMismatch {
                            field_index,
                            expected: expected_name.to_owned(),
                            actual: field.name.clone(),
                        },
                    ));
                }
                let field_path = child_path(path, expected_name);
                self.encode_field(child_index, &field.value, &field_path, depth + 1)?;
            }
            return Ok(());
        }

        let SerializedValue::Raw {
            type_name: raw_type_name,
            bytes,
        } = value
        else {
            return Err(self.value_mismatch(path, type_name, value));
        };
        if raw_type_name != type_name {
            return Err(self.value_mismatch(path, type_name, value));
        }
        let expected_size = usize::try_from(node.byte_size).map_err(|_| {
            ObjectEncodeError::new(
                self.object_index,
                path,
                ObjectEncodeErrorKind::InvalidTypeTree {
                    message: format!(
                        "unknown leaf '{type_name}' has invalid byte size {}",
                        node.byte_size
                    ),
                },
            )
        })?;
        if bytes.len() != expected_size {
            return Err(ObjectEncodeError::new(
                self.object_index,
                path,
                ObjectEncodeErrorKind::RawLeafSizeMismatch {
                    expected: expected_size,
                    actual: bytes.len(),
                },
            ));
        }
        self.write_bytes(path, bytes)
    }

    fn encode_array(
        &mut self,
        array_node_index: usize,
        value: &SerializedValue,
        path: &str,
        depth: usize,
        align_nested_arrays: bool,
    ) -> Result<(), ObjectEncodeError> {
        let (values, data_node) = match value {
            SerializedValue::Array(values) => {
                (values, self.array_data_node(array_node_index, path)?)
            }
            other => return Err(self.value_mismatch(path, "array", other)),
        };
        if values.len() > self.limits.max_array_elements {
            return Err(ObjectEncodeError::new(
                self.object_index,
                path,
                ObjectEncodeErrorKind::ArrayElementLimitExceeded {
                    requested: values.len(),
                    limit: self.limits.max_array_elements,
                },
            ));
        }
        let count = i32::try_from(values.len()).map_err(|_| {
            ObjectEncodeError::new(
                self.object_index,
                path,
                ObjectEncodeErrorKind::LengthOverflow {
                    resource: "TypeTree array element count",
                },
            )
        })?;
        let minimum_size = minimum_encoded_size(
            self.type_tree,
            data_node,
            self.object_index,
            self.output.len(),
            0,
        )
        .map_err(|error| invalid_tree(self.object_index, path, error))?;
        if count > 0 && minimum_size == 0 {
            return Err(ObjectEncodeError::new(
                self.object_index,
                path,
                ObjectEncodeErrorKind::ZeroWidthArrayElement {
                    count: values.len(),
                },
            ));
        }
        self.write_i32(path, count)?;
        for (index, value) in values.iter().enumerate() {
            let item_path = format!("{path}[{index}]");
            self.encode_node_value(data_node, value, &item_path, depth + 1, align_nested_arrays)?;
        }
        if align_nested_arrays
            || (self.arrays_align
                && array_base_is_primitive(
                    self.type_tree,
                    data_node,
                    self.object_index,
                    self.output.len(),
                )
                .map_err(|error| invalid_tree(self.object_index, path, error))?)
        {
            self.align_four(path)?;
        }
        Ok(())
    }

    fn encode_primitive(
        &mut self,
        node_index: usize,
        value: &SerializedValue,
        path: &str,
    ) -> Result<bool, ObjectEncodeError> {
        let node = &self.type_tree.nodes[node_index];
        let type_name = node.type_name.as_deref().unwrap_or("");
        let mut bytes = [0_u8; 8];
        let length = match type_name {
            "bool" => match value {
                SerializedValue::Bool(value) => {
                    bytes[0] = u8::from(*value);
                    1
                }
                _ => return Err(self.value_mismatch(path, type_name, value)),
            },
            "char" | "UInt8" => match value {
                SerializedValue::U8(value) => {
                    bytes[0] = *value;
                    1
                }
                _ => return Err(self.value_mismatch(path, type_name, value)),
            },
            "SInt8" => match value {
                SerializedValue::I8(value) => {
                    bytes[0] = *value as u8;
                    1
                }
                _ => return Err(self.value_mismatch(path, type_name, value)),
            },
            "short" | "SInt16" => match value {
                SerializedValue::I16(value) => {
                    bytes[..2].copy_from_slice(&encode_u16_bytes(*value as u16, self.endian));
                    2
                }
                _ => return Err(self.value_mismatch(path, type_name, value)),
            },
            "ushort" | "UInt16" | "unsigned short" => match value {
                SerializedValue::U16(value) => {
                    bytes[..2].copy_from_slice(&encode_u16_bytes(*value, self.endian));
                    2
                }
                SerializedValue::Char(value) => {
                    bytes[..2].copy_from_slice(&encode_u16_bytes(*value, self.endian));
                    2
                }
                _ => return Err(self.value_mismatch(path, type_name, value)),
            },
            "int" | "SInt32" | "Type*" => match value {
                SerializedValue::I32(value) => {
                    bytes[..4].copy_from_slice(&encode_u32_bytes(*value as u32, self.endian));
                    4
                }
                _ => return Err(self.value_mismatch(path, type_name, value)),
            },
            "uint" | "UInt32" | "unsigned int" => match value {
                SerializedValue::U32(value) => {
                    bytes[..4].copy_from_slice(&encode_u32_bytes(*value, self.endian));
                    4
                }
                _ => return Err(self.value_mismatch(path, type_name, value)),
            },
            "SInt64" | "long long" => match value {
                SerializedValue::I64(value) => {
                    bytes.copy_from_slice(&encode_u64_bytes(*value as u64, self.endian));
                    8
                }
                _ => return Err(self.value_mismatch(path, type_name, value)),
            },
            "UInt64" | "FileSize" | "unsigned long long" => match value {
                SerializedValue::U64(value) => {
                    bytes.copy_from_slice(&encode_u64_bytes(*value, self.endian));
                    8
                }
                _ => return Err(self.value_mismatch(path, type_name, value)),
            },
            "float" => match value {
                SerializedValue::F32(value) => {
                    bytes[..4].copy_from_slice(&encode_u32_bytes(value.to_bits(), self.endian));
                    4
                }
                _ => return Err(self.value_mismatch(path, type_name, value)),
            },
            "double" => match value {
                SerializedValue::F64(value) => {
                    bytes.copy_from_slice(&encode_u64_bytes(value.to_bits(), self.endian));
                    8
                }
                _ => return Err(self.value_mismatch(path, type_name, value)),
            },
            _ => return Ok(false),
        };
        self.write_bytes(path, &bytes[..length])?;
        Ok(true)
    }

    fn array_data_node(
        &self,
        array_node_index: usize,
        path: &str,
    ) -> Result<usize, ObjectEncodeError> {
        array_shape(
            self.type_tree,
            array_node_index,
            self.object_index,
            self.output.len(),
        )
        .map_err(|error| invalid_tree(self.object_index, path, error))?
        .map(|(_, data_node)| data_node)
        .ok_or_else(|| {
            ObjectEncodeError::new(
                self.object_index,
                path,
                ObjectEncodeErrorKind::InvalidTypeTree {
                    message: format!(
                        "array node {array_node_index} has an invalid Array/size/data shape"
                    ),
                },
            )
        })
    }

    fn children(&self, node_index: usize, path: &str) -> Result<Vec<usize>, ObjectEncodeError> {
        direct_children(
            self.type_tree,
            node_index,
            self.object_index,
            self.output.len(),
        )
        .map_err(|error| invalid_tree(self.object_index, path, error))
    }

    fn value_mismatch(
        &self,
        path: &str,
        expected: &str,
        actual: &SerializedValue,
    ) -> ObjectEncodeError {
        ObjectEncodeError::new(
            self.object_index,
            path,
            ObjectEncodeErrorKind::ValueTypeMismatch {
                expected: expected.to_owned(),
                actual: value_kind(actual),
            },
        )
    }

    fn write_i32(&mut self, path: &str, value: i32) -> Result<(), ObjectEncodeError> {
        self.write_bytes(path, &encode_u32_bytes(value as u32, self.endian))
    }

    fn write_bytes(&mut self, path: &str, bytes: &[u8]) -> Result<(), ObjectEncodeError> {
        let requested = self.output.len().checked_add(bytes.len()).ok_or_else(|| {
            ObjectEncodeError::new(
                self.object_index,
                path,
                ObjectEncodeErrorKind::LengthOverflow {
                    resource: "encoded object payload",
                },
            )
        })?;
        if requested > self.limits.max_encoded_payload_bytes {
            return Err(ObjectEncodeError::new(
                self.object_index,
                path,
                ObjectEncodeErrorKind::EncodedPayloadLimitExceeded {
                    requested,
                    limit: self.limits.max_encoded_payload_bytes,
                },
            ));
        }
        if requested > self.output.capacity() {
            let geometric_capacity = self.output.capacity().saturating_mul(2).max(64);
            let target_capacity = geometric_capacity
                .max(requested)
                .min(self.limits.max_encoded_payload_bytes);
            let additional = target_capacity.saturating_sub(self.output.len());
            self.output.try_reserve_exact(additional).map_err(|_| {
                ObjectEncodeError::new(
                    self.object_index,
                    path,
                    ObjectEncodeErrorKind::AllocationFailed {
                        resource: "object payload bytes",
                        requested: target_capacity,
                    },
                )
            })?;
        }
        self.output.extend_from_slice(bytes);
        Ok(())
    }

    fn align_four(&mut self, path: &str) -> Result<(), ObjectEncodeError> {
        let padding = (4 - (self.output.len() & 3)) & 3;
        const ZEROES: [u8; 3] = [0; 3];
        self.write_bytes(path, &ZEROES[..padding])
    }
}

fn child_path(parent: &str, child: &str) -> String {
    if parent.is_empty() {
        child.to_owned()
    } else if child.is_empty() {
        parent.to_owned()
    } else {
        format!("{parent}.{child}")
    }
}

fn invalid_tree(
    object_index: usize,
    path: &str,
    error: super::ObjectDecodeError,
) -> ObjectEncodeError {
    ObjectEncodeError::new(
        object_index,
        path,
        ObjectEncodeErrorKind::InvalidTypeTree {
            message: error.to_string(),
        },
    )
}

fn value_kind(value: &SerializedValue) -> &'static str {
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
        SerializedValue::ManagedReferenceId { .. } => "managed reference ID",
        SerializedValue::ManagedReferenceRegistry(_) => "managed reference registry",
        SerializedValue::Raw { .. } => "raw",
    }
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

fn encode_u16_bytes(value: u16, endian: Endian) -> [u8; 2] {
    match endian {
        Endian::Little => value.to_le_bytes(),
        Endian::Big => value.to_be_bytes(),
    }
}

fn encode_u32_bytes(value: u32, endian: Endian) -> [u8; 4] {
    match endian {
        Endian::Little => value.to_le_bytes(),
        Endian::Big => value.to_be_bytes(),
    }
}

fn encode_u64_bytes(value: u64, endian: Endian) -> [u8; 8] {
    match endian {
        Endian::Little => value.to_le_bytes(),
        Endian::Big => value.to_be_bytes(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::binary::Endian;
    use crate::serialized_file::metadata::{
        SerializedFileMetadata, SerializedType, TypeTree, TypeTreeNode,
    };
    use crate::serialized_file::metadata_prefix::SerializedFileMetadataPrefix;
    use crate::serialized_file::object::SerializedFieldValue;

    fn node(level: u8, name: &str, type_name: &str, byte_size: i32) -> TypeTreeNode {
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
            meta_flags: 0,
            reference_type_hash: None,
        }
    }

    fn metadata_with_tree(tree: TypeTree) -> SerializedFileMetadata {
        SerializedFileMetadata {
            prefix: SerializedFileMetadataPrefix {
                metadata_offset: 0,
                metadata_size: 0,
                body_endian: Endian::Little,
                unity_version_text: Some("2022.3.1f1".to_owned()),
                target_platform: Some(0),
                has_type_tree: true,
                consumed_bytes: 0,
            },
            types: vec![SerializedType {
                raw_type_id: 4,
                type_id_for_hash: 4,
                is_stripped_type: false,
                script_type_index: -1,
                script_id: None,
                old_type_hash: None,
                type_tree: Some(tree),
                type_dependencies: Vec::new(),
            }],
            long_file_id: None,
            objects: Vec::new(),
            script_types: Vec::new(),
            externals: Vec::new(),
            reference_types: Vec::new(),
            user_information: None,
            trailing_metadata: Vec::new(),
            consumed_bytes: 0,
        }
    }

    fn encoder<'layout, 'tree>(
        layout: &'layout TypeTreeLayout<'tree>,
        arrays_align: bool,
    ) -> PayloadEncoder<'layout, 'tree, 'static> {
        PayloadEncoder {
            type_tree: layout,
            endian: Endian::Little,
            arrays_align,
            reference_types: &[],
            allow_managed_registry: false,
            object_index: 7,
            limits: ObjectEncodeLimits::default(),
            output: Vec::new(),
        }
    }

    #[test]
    fn encodes_pptr_file_and_path_ids_using_the_declared_integer_width() {
        let tree = TypeTree {
            nodes: vec![
                node(0, "", "Root", -1),
                node(1, "m_Target", "PPtr<GameObject>", 8),
                node(2, "m_FileID", "SInt32", 4),
                node(2, "m_PathID", "SInt32", 4),
            ],
            string_buffer: Vec::new(),
        };
        let layout = TypeTreeLayout::new(&tree, 7, 0).unwrap();
        let value = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "m_Target".to_owned(),
            value: SerializedValue::PPtr {
                file_id: 2,
                path_id: -3,
            },
        }]);
        let mut encoder = encoder(&layout, false);

        encoder.encode_root(&value, "Root").unwrap();

        assert_eq!(encoder.output, [2, 0, 0, 0, 0xfd, 0xff, 0xff, 0xff]);
    }

    #[test]
    fn encodes_64_bit_pptr_path_ids() {
        let tree = TypeTree {
            nodes: vec![
                node(0, "", "Root", -1),
                node(1, "m_Target", "PPtr<GameObject>", 12),
                node(2, "m_FileID", "SInt32", 4),
                node(2, "m_PathID", "SInt64", 8),
            ],
            string_buffer: Vec::new(),
        };
        let layout = TypeTreeLayout::new(&tree, 7, 0).unwrap();
        let value = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "m_Target".to_owned(),
            value: SerializedValue::PPtr {
                file_id: 2,
                path_id: -3,
            },
        }]);
        let mut encoder = encoder(&layout, false);

        encoder.encode_root(&value, "Root").unwrap();

        assert_eq!(
            encoder.output,
            [2, 0, 0, 0, 0xfd, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff]
        );
    }

    #[test]
    fn encodes_version_aligned_primitive_arrays_with_zero_padding() {
        let tree = TypeTree {
            nodes: vec![
                node(0, "", "Root", -1),
                node(1, "m_Values", "vector", -1),
                node(2, "Array", "Array", -1),
                node(3, "size", "int", 4),
                node(3, "data", "UInt8", 1),
            ],
            string_buffer: Vec::new(),
        };
        let layout = TypeTreeLayout::new(&tree, 7, 0).unwrap();
        let value = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "m_Values".to_owned(),
            value: SerializedValue::Array(vec![SerializedValue::U8(7)]),
        }]);
        let mut encoder = encoder(&layout, true);

        encoder.encode_root(&value, "Root").unwrap();

        assert_eq!(encoder.output, [1, 0, 0, 0, 7, 0, 0, 0]);
    }

    #[test]
    fn rejects_nonempty_zero_width_arrays() {
        let tree = TypeTree {
            nodes: vec![
                node(0, "", "Root", -1),
                node(1, "m_Values", "vector", -1),
                node(2, "Array", "Array", -1),
                node(3, "size", "int", 4),
                node(3, "data", "ZeroWidth", 0),
            ],
            string_buffer: Vec::new(),
        };
        let layout = TypeTreeLayout::new(&tree, 7, 0).unwrap();
        let value = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "m_Values".to_owned(),
            value: SerializedValue::Array(vec![SerializedValue::Raw {
                type_name: "ZeroWidth".to_owned(),
                bytes: Vec::new(),
            }]),
        }]);
        let mut encoder = encoder(&layout, false);

        let error = encoder.encode_root(&value, "Root").unwrap_err();

        assert!(matches!(
            error.kind(),
            ObjectEncodeErrorKind::ZeroWidthArrayElement { count: 1 }
        ));
    }

    #[test]
    fn writes_unknown_fixed_size_leaves_and_aligns_the_following_field() {
        let mut aligned = node(1, "m_Value", "UInt8", 1);
        aligned.meta_flags = 0x4000;
        let tree = TypeTree {
            nodes: vec![
                node(0, "", "Root", -1),
                node(1, "m_Raw", "Opaque", 2),
                aligned,
            ],
            string_buffer: Vec::new(),
        };
        let layout = TypeTreeLayout::new(&tree, 7, 0).unwrap();
        let value = SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "m_Raw".to_owned(),
                value: SerializedValue::Raw {
                    type_name: "Opaque".to_owned(),
                    bytes: vec![0xaa, 0xbb],
                },
            },
            SerializedFieldValue {
                name: "m_Value".to_owned(),
                value: SerializedValue::U8(7),
            },
        ]);
        let mut encoder = encoder(&layout, false);

        encoder.encode_root(&value, "Root").unwrap();

        assert_eq!(encoder.output, [0xaa, 0xbb, 7, 0]);
    }

    #[test]
    fn constructs_bounded_type_tree_defaults_and_encodes_them() {
        let tree = TypeTree {
            nodes: vec![
                node(0, "", "Transform", -1),
                node(1, "m_GameObject", "PPtr<GameObject>", 12),
                node(2, "m_FileID", "SInt32", 4),
                node(2, "m_PathID", "SInt64", 8),
                node(1, "m_LocalRotation", "Quaternion", -1),
                node(2, "x", "float", 4),
                node(2, "y", "float", 4),
                node(2, "z", "float", 4),
                node(2, "w", "float", 4),
                node(1, "m_Name", "string", -1),
                node(2, "Array", "Array", -1),
                node(3, "size", "int", 4),
                node(3, "data", "char", 1),
                node(1, "m_Bytes", "TypelessData", -1),
                node(2, "size", "int", 4),
                node(2, "data", "UInt8", 1),
                node(1, "m_Children", "vector", -1),
                node(2, "Array", "Array", -1),
                node(3, "size", "int", 4),
                node(3, "data", "PPtr<Transform>", 12),
                node(4, "m_FileID", "SInt32", 4),
                node(4, "m_PathID", "SInt64", 8),
                node(1, "m_Opaque", "OpaqueRecord", 3),
            ],
            string_buffer: Vec::new(),
        };
        let metadata = metadata_with_tree(tree);

        let value = metadata
            .default_object_value_for_type(0, 0, ObjectDefaultValueLimits::default())
            .unwrap();

        assert_eq!(
            value.unique_field("m_GameObject").unwrap(),
            Some(&SerializedValue::PPtr {
                file_id: 0,
                path_id: 0,
            })
        );
        assert_eq!(
            value.unique_field("m_Name").unwrap(),
            Some(&SerializedValue::StringBytes(Vec::new()))
        );
        assert_eq!(
            value.unique_field("m_Bytes").unwrap(),
            Some(&SerializedValue::Bytes(Vec::new()))
        );
        assert_eq!(
            value.unique_field("m_Children").unwrap(),
            Some(&SerializedValue::Array(Vec::new()))
        );
        assert_eq!(
            value.unique_field("m_Opaque").unwrap(),
            Some(&SerializedValue::Raw {
                type_name: "OpaqueRecord".to_owned(),
                bytes: vec![0; 3],
            })
        );
        let payload = metadata
            .encode_object_payload_for_type(0, 0, &value, &[], ObjectEncodeLimits::default())
            .unwrap();
        assert_eq!(payload.len(), 12 + 16 + 4 + 4 + 4 + 3);
    }

    #[test]
    fn constructs_array_element_template_from_its_type_tree_subtree() {
        let metadata = metadata_with_tree(TypeTree {
            nodes: vec![
                node(0, "", "GameObject", -1),
                node(1, "m_Component", "vector", -1),
                node(2, "Array", "Array", -1),
                node(3, "size", "int", 4),
                node(3, "data", "ComponentPair", -1),
                node(4, "component", "PPtr<Component>", 12),
                node(5, "m_FileID", "SInt32", 4),
                node(5, "m_PathID", "SInt64", 8),
            ],
            string_buffer: Vec::new(),
        });

        let template = metadata
            .default_value_for_type_tree_node(0, 0, 4, ObjectDefaultValueLimits::default())
            .unwrap();

        assert_eq!(
            template.unique_field("component").unwrap(),
            Some(&SerializedValue::PPtr {
                file_id: 0,
                path_id: 0,
            })
        );
    }

    #[test]
    fn default_value_builder_enforces_node_and_allocation_limits() {
        let metadata = metadata_with_tree(TypeTree {
            nodes: vec![node(0, "", "OpaqueRecord", 16)],
            string_buffer: Vec::new(),
        });

        let type_tree_error = metadata
            .default_object_value_for_type(
                0,
                0,
                ObjectDefaultValueLimits {
                    max_type_tree_nodes: 0,
                    ..ObjectDefaultValueLimits::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            type_tree_error.kind(),
            ObjectEncodeErrorKind::DefaultTypeTreeNodeLimitExceeded {
                requested: 1,
                limit: 0
            }
        ));

        let node_error = metadata
            .default_object_value_for_type(
                0,
                0,
                ObjectDefaultValueLimits {
                    max_value_nodes: 0,
                    ..ObjectDefaultValueLimits::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            node_error.kind(),
            ObjectEncodeErrorKind::DefaultValueNodeLimitExceeded {
                requested: 1,
                limit: 0
            }
        ));

        let allocation_error = metadata
            .default_object_value_for_type(
                0,
                0,
                ObjectDefaultValueLimits {
                    max_allocation_bytes: 1,
                    ..ObjectDefaultValueLimits::default()
                },
            )
            .unwrap_err();
        assert!(matches!(
            allocation_error.kind(),
            ObjectEncodeErrorKind::DefaultValueAllocationLimitExceeded {
                requested,
                limit: 1
            } if *requested > 1
        ));
    }

    #[test]
    fn default_value_builder_rejects_malformed_pptr_and_managed_reference_shapes() {
        let malformed_pptr = metadata_with_tree(TypeTree {
            nodes: vec![
                node(0, "", "Root", -1),
                node(1, "m_Target", "PPtr<GameObject>", 4),
            ],
            string_buffer: Vec::new(),
        });
        let error = malformed_pptr
            .default_object_value_for_type(0, 0, ObjectDefaultValueLimits::default())
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            ObjectEncodeErrorKind::InvalidTypeTree { message }
                if message.contains("PPtr node has an invalid component shape")
        ));

        let malformed_pptr_integer = metadata_with_tree(TypeTree {
            nodes: vec![
                node(0, "", "Root", -1),
                node(1, "m_Target", "PPtr<GameObject>", 8),
                node(2, "m_FileID", "float", 4),
                node(2, "m_PathID", "SInt32", 4),
            ],
            string_buffer: Vec::new(),
        });
        let error = malformed_pptr_integer
            .default_object_value_for_type(0, 0, ObjectDefaultValueLimits::default())
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            ObjectEncodeErrorKind::InvalidTypeTree { message }
                if message.contains("PPtr file ID node is not 32-bit")
        ));

        let managed_reference = metadata_with_tree(TypeTree {
            nodes: vec![
                node(0, "", "Root", -1),
                node(1, "m_Reference", "managedReference", 8),
                node(2, "rid", "SInt64", 8),
            ],
            string_buffer: Vec::new(),
        });
        let error = managed_reference
            .default_object_value_for_type(0, 0, ObjectDefaultValueLimits::default())
            .unwrap_err();
        assert!(matches!(
            error.kind(),
            ObjectEncodeErrorKind::UnsupportedManagedReferences {
                type_name,
                field_name,
            } if type_name == "managedReference" && field_name == "m_Reference"
        ));
    }
}
