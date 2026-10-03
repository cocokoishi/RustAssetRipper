use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::io::binary::Endian;

use super::super::metadata::SerializedFileMetadata;
use super::{
    array_base_is_primitive, array_node_for_value, array_shape, direct_children, field_aligns,
    is_string_node, minimum_encoded_size, pptr_shape, unity_version_aligns_arrays,
    ParsedObjectPayload, SerializedValue, TypeTreeLayout,
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
            object_index,
            limits,
            output,
        };
        encoder.encode_root(value, root.type_name.as_deref().unwrap_or(""))?;
        encoder.write_bytes("<trailing>", trailing_bytes)?;
        Ok(encoder.output)
    }
}

struct PayloadEncoder<'layout, 'tree> {
    type_tree: &'layout TypeTreeLayout<'tree>,
    endian: Endian,
    arrays_align: bool,
    object_index: usize,
    limits: ObjectEncodeLimits,
    output: Vec<u8>,
}

impl PayloadEncoder<'_, '_> {
    fn encode_root(
        &mut self,
        value: &SerializedValue,
        root_name: &str,
    ) -> Result<(), ObjectEncodeError> {
        for (node_index, node) in self.type_tree.nodes.iter().enumerate() {
            let children = self.children(node_index, "<root>")?;
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
        self.encode_node_value(0, value, root_name, 0, false)
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

        if type_name == "string"
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

        let children = self.children(node_index, path)?;
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
    use crate::serialized_file::metadata::{TypeTree, TypeTreeNode};
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

    fn encoder<'layout, 'tree>(
        layout: &'layout TypeTreeLayout<'tree>,
        arrays_align: bool,
    ) -> PayloadEncoder<'layout, 'tree> {
        PayloadEncoder {
            type_tree: layout,
            endian: Endian::Little,
            arrays_align,
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
}
