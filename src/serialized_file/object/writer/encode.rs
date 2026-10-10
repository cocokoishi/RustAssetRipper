use super::error::{ObjectEncodeError, ObjectEncodeErrorKind};
use super::{child_path, invalid_tree};
use crate::io::binary::Endian;
use crate::serialized_file::metadata::SerializedFileMetadata;
use crate::serialized_file::object::unity_version_aligns_arrays;
use crate::serialized_file::object::{
    array_base_is_primitive, array_node_for_value, array_shape, direct_children, field_aligns,
    is_string_node, minimum_encoded_size, pptr_shape, ParsedObjectPayload, SerializedValue,
    TypeTreeLayout,
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

pub(super) struct PayloadEncoder<'layout, 'tree, 'references> {
    pub(super) type_tree: &'layout TypeTreeLayout<'tree>,
    pub(super) endian: Endian,
    pub(super) arrays_align: bool,
    pub(super) reference_types:
        &'references [crate::serialized_file::metadata::SerializedTypeReference],
    pub(super) allow_managed_registry: bool,
    pub(super) object_index: usize,
    pub(super) limits: ObjectEncodeLimits,
    pub(super) output: Vec<u8>,
}

impl PayloadEncoder<'_, '_, '_> {
    pub(super) fn encode_root(
        &mut self,
        value: &SerializedValue,
        root_name: &str,
    ) -> Result<(), ObjectEncodeError> {
        self.encode_node_value(0, value, root_name, 0, false)
    }

    fn encode_managed_registry(
        &mut self,
        registry: &crate::serialized_file::managed_registry::ManagedRegistryRead,
        path: &str,
        depth: usize,
    ) -> Result<(), ObjectEncodeError> {
        use crate::serialized_file::managed_registry::{
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
                    crate::serialized_file::managed_registry::ManagedRegistryEntryData::Content(
                        value,
                    ),
                    true,
                ) => self.encode_managed_registry_content(
                    entry_index,
                    entry,
                    value,
                    &entry_path,
                    depth + 1,
                )?,
                (
                    crate::serialized_file::managed_registry::ManagedRegistryEntryData::Empty,
                    false,
                ) => {}
                (
                    crate::serialized_file::managed_registry::ManagedRegistryEntryData::Content(_),
                    false,
                ) => {
                    return Err(ObjectEncodeError::new(
                        self.object_index,
                        &entry_path,
                        ObjectEncodeErrorKind::InvalidManagedReferenceEncoding {
                            reason: "a null, unknown, or empty-type entry carries content"
                                .to_owned(),
                        },
                    ));
                }
                (
                    crate::serialized_file::managed_registry::ManagedRegistryEntryData::Empty,
                    true,
                ) => {
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
        entry: &crate::serialized_file::managed_registry::ManagedRegistryEntry,
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
        let payload_tree = crate::serialized_file::managed_registry::flatten_built_tree(&built);
        let payload_tree = crate::serialized_file::metadata::TypeTree {
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

        if crate::serialized_file::object::is_managed_reference_element(
            self.type_tree,
            node_index,
            self.object_index,
            self.output.len(),
        )
        .map_err(|error| invalid_tree(self.object_index, path, error))?
        {
            let indexed = crate::serialized_file::object::is_indexed_managed_reference_element(
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

        if crate::serialized_file::object::is_managed_references_registry(
            type_name,
            field_name,
            children.len(),
        ) {
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

        if crate::serialized_file::object::is_referenced_object_data(
            type_name,
            field_name,
            children.len(),
        ) {
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
