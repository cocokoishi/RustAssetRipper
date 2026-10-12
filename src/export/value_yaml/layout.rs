use crate::processing::scene_naming::SceneUnityVersion;
use crate::serialized_file::metadata::{TypeTree, TypeTreeNode};

use super::field_rules::{
    baseline_type_scoped_field_alias, baseline_type_scoped_field_aliases,
    has_conditional_field_aliases, unsupported_contextual_field_alias_reason,
};
use super::model::{
    UnityYamlDictionaryRepresentation, UnityYamlEmptyArrayKind, UnityYamlGuidSchema,
    UnityYamlHash128Schema, UnityYamlObjectConversionError, UnityYamlObjectConversionHints,
    UnityYamlObjectConversionLimits, UnityYamlPropertyNameSchema, UnityYamlPropertyNameValueKind,
    UnityYamlSceneObjectIdentifierSchema,
};
use super::paths::{child_field_path, try_copy_string, wildcard_element_path, ConversionBudget};
use super::type_rules::{
    is_integer_type_tree_type, is_signed_file_id_type_tree_type, is_unsigned_byte_type_tree_type,
    is_unsigned_word_type_tree_type, is_valid_dictionary_key_type_tree_type,
};
pub(super) const TRANSFER_USING_FLOW_MAPPING_STYLE: u32 = 0x0020_0000;
pub(super) const TRANSFER_IGNORE_IN_META_FILES: u32 = 0x0008_0000;

impl UnityYamlObjectConversionHints {
    /// Build YAML representation hints from the same TypeTree flags and array shapes used to
    /// decode the SerializedValue tree.
    pub fn from_type_tree(
        type_tree: &TypeTree,
        unity_version: SceneUnityVersion,
        limits: UnityYamlObjectConversionLimits,
    ) -> Result<Self, UnityYamlObjectConversionError> {
        let layout = UnityYamlTypeTreeLayout::new(type_tree, limits)?;
        let mut hints = Self::default();
        let mut budget = ConversionBudget::new(limits);
        layout.collect_hints(0, "", 0, unity_version, &mut hints, &mut budget)?;
        Ok(hints)
    }
}

struct UnityYamlTypeTreeLayout<'a> {
    type_tree: &'a TypeTree,
    first_child: Vec<Option<usize>>,
    next_sibling: Vec<Option<usize>>,
}

impl<'a> UnityYamlTypeTreeLayout<'a> {
    fn new(
        type_tree: &'a TypeTree,
        limits: UnityYamlObjectConversionLimits,
    ) -> Result<Self, UnityYamlObjectConversionError> {
        let count = type_tree.nodes.len();
        if count == 0 {
            return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                node_index: 0,
                reason: "tree has no root node",
            });
        }
        if count > limits.max_type_tree_nodes {
            return Err(UnityYamlObjectConversionError::ValueLimitExceeded {
                requested: count,
                limit: limits.max_type_tree_nodes,
            });
        }
        if type_tree.nodes[0].level != 0 {
            return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                node_index: 0,
                reason: "root node level is not zero",
            });
        }

        let mut first_child = Vec::new();
        first_child.try_reserve_exact(count).map_err(|_| {
            UnityYamlObjectConversionError::AllocationFailed {
                resource: "TypeTree first-child index",
                requested: count,
            }
        })?;
        first_child.resize(count, None);

        let mut next_sibling = Vec::new();
        next_sibling.try_reserve_exact(count).map_err(|_| {
            UnityYamlObjectConversionError::AllocationFailed {
                resource: "TypeTree sibling index",
                requested: count,
            }
        })?;
        next_sibling.resize(count, None);

        let mut last_child = Vec::new();
        last_child.try_reserve_exact(count).map_err(|_| {
            UnityYamlObjectConversionError::AllocationFailed {
                resource: "TypeTree last-child index",
                requested: count,
            }
        })?;
        last_child.resize(count, None);

        let stack_capacity = count.min(limits.max_depth.saturating_add(1));
        let mut parents = Vec::new();
        parents.try_reserve_exact(stack_capacity).map_err(|_| {
            UnityYamlObjectConversionError::AllocationFailed {
                resource: "TypeTree parent stack",
                requested: stack_capacity,
            }
        })?;
        parents.push(0);

        for (node_index, node) in type_tree.nodes.iter().enumerate().skip(1) {
            let level = usize::from(node.level);
            if level == 0 {
                return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                    node_index,
                    reason: "tree contains more than one root",
                });
            }
            if level > limits.max_depth {
                return Err(UnityYamlObjectConversionError::DepthLimitExceeded {
                    requested: level,
                    limit: limits.max_depth,
                });
            }
            if level > parents.len() {
                return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                    node_index,
                    reason: "node level skips its parent",
                });
            }
            parents.truncate(level);
            let parent_index =
                *parents
                    .get(level - 1)
                    .ok_or(UnityYamlObjectConversionError::InvalidTypeTree {
                        node_index,
                        reason: "node has no parent",
                    })?;
            if let Some(previous) = last_child[parent_index] {
                next_sibling[previous] = Some(node_index);
            } else {
                first_child[parent_index] = Some(node_index);
            }
            last_child[parent_index] = Some(node_index);
            parents.push(node_index);
        }

        Ok(Self {
            type_tree,
            first_child,
            next_sibling,
        })
    }

    fn node(&self, node_index: usize) -> Result<&TypeTreeNode, UnityYamlObjectConversionError> {
        self.type_tree.nodes.get(node_index).ok_or(
            UnityYamlObjectConversionError::InvalidTypeTree {
                node_index,
                reason: "node index is outside the tree",
            },
        )
    }

    fn type_name(&self, node_index: usize) -> Result<&str, UnityYamlObjectConversionError> {
        self.node(node_index)?.type_name.as_deref().ok_or(
            UnityYamlObjectConversionError::InvalidTypeTree {
                node_index,
                reason: "type name is unresolved",
            },
        )
    }

    fn child_indices(
        &self,
        node_index: usize,
    ) -> Result<Vec<usize>, UnityYamlObjectConversionError> {
        let mut children = Vec::new();
        let mut child = *self.first_child.get(node_index).ok_or(
            UnityYamlObjectConversionError::InvalidTypeTree {
                node_index,
                reason: "node index is outside the tree",
            },
        )?;
        while let Some(child_index) = child {
            children.try_reserve(1).map_err(|_| {
                UnityYamlObjectConversionError::AllocationFailed {
                    resource: "TypeTree direct children",
                    requested: children.len().saturating_add(1),
                }
            })?;
            children.push(child_index);
            child = self.next_sibling[child_index];
        }
        Ok(children)
    }

    fn unique_child_index_by_name(
        &self,
        node_index: usize,
        field_name: &str,
    ) -> Result<Option<usize>, UnityYamlObjectConversionError> {
        let mut matching_child = None;
        for child_index in self.child_indices(node_index)? {
            if self.node(child_index)?.name.as_deref() == Some(field_name)
                && matching_child.replace(child_index).is_some()
            {
                return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                    node_index: child_index,
                    reason: "TypeTree node contains a duplicate child field name",
                });
            }
        }
        Ok(matching_child)
    }

    fn value_array_field_alias(
        &self,
        field_node_index: usize,
        field_name: &str,
    ) -> Result<Option<&'static str>, UnityYamlObjectConversionError> {
        let aliases = match field_name {
            "m_PositionValues" => ("m_PositionValues3", "m_PositionValues4"),
            "m_ScaleValues" => ("m_ScaleValues3", "m_ScaleValues4"),
            _ => return Ok(None),
        };
        let named_array_node = self.unique_child_index_by_name(field_node_index, "m_Array")?;
        let source_array_node = self.unique_child_index_by_name(field_node_index, "Array")?;
        let array_node = match (named_array_node, source_array_node) {
            (Some(_), Some(_)) => {
                return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                    node_index: field_node_index,
                    reason: "ValueArray vector field has multiple array nodes",
                });
            }
            (Some(index), None) | (None, Some(index)) => index,
            (None, None) => {
                return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                    node_index: field_node_index,
                    reason: "ValueArray vector field has no array child",
                });
            }
        };
        let named_data_node = self.unique_child_index_by_name(array_node, "m_Data")?;
        let source_data_node = self.unique_child_index_by_name(array_node, "data")?;
        let data_node = match (named_data_node, source_data_node) {
            (Some(_), Some(_)) => {
                return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                    node_index: array_node,
                    reason: "ValueArray array node has multiple data fields",
                });
            }
            (Some(index), None) | (None, Some(index)) => index,
            (None, None) => {
                return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                    node_index: array_node,
                    reason: "ValueArray array node has no data field",
                });
            }
        };
        match self.type_name(data_node)? {
            "Vector3Float" | "float3" => Ok(Some(aliases.0)),
            "Vector4Float" | "float4" => Ok(Some(aliases.1)),
            _ => Err(UnityYamlObjectConversionError::InvalidTypeTree {
                node_index: data_node,
                reason: "unsupported ValueArray element TypeTree type",
            }),
        }
    }

    fn array_element_node(
        &self,
        node_index: usize,
    ) -> Result<Option<usize>, UnityYamlObjectConversionError> {
        let node_type = self.type_name(node_index)?;
        if matches!(node_type, "Array" | "TypelessData") {
            if node_type == "TypelessData" {
                return Ok(None);
            }
            return self.array_shape_data_node(node_index);
        }

        let children = self.child_indices(node_index)?;
        if children.len() != 1 {
            return Ok(None);
        }
        let array_node = children[0];
        let Some(data_node) = self.array_shape_data_node(array_node)? else {
            return Ok(None);
        };
        let child = self.node(array_node)?;
        let is_vector = matches!(node_type, "vector" | "staticvector" | "set" | "map");
        let is_named_vector = if child.name.as_deref() == Some("Array") {
            let element_type = self.type_name(data_node)?;
            element_type == node_type
                || matches!(element_type, "Generic Mono" | "managedReferenceArrayItem")
        } else {
            false
        };
        Ok((is_vector || is_named_vector).then_some(data_node))
    }

    fn array_shape_data_node(
        &self,
        array_node: usize,
    ) -> Result<Option<usize>, UnityYamlObjectConversionError> {
        if self.type_name(array_node)? != "Array" {
            return Ok(None);
        }
        let children = self.child_indices(array_node)?;
        if children.len() != 2
            || self.node(children[0])?.name.as_deref() != Some("size")
            || self.node(children[1])?.name.as_deref() != Some("data")
            || self.first_child[children[0]].is_some()
        {
            return Ok(None);
        }
        Ok(Some(children[1]))
    }

    fn hash128_schema(
        &self,
        node_index: usize,
        unity_version: SceneUnityVersion,
    ) -> Result<UnityYamlHash128Schema, UnityYamlObjectConversionError> {
        let children = self.child_indices(node_index)?;
        if children.len() != 16 {
            return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                node_index,
                reason: "Hash128 does not contain exactly 16 byte fields",
            });
        }
        let mut byte_field_names = Vec::new();
        byte_field_names.try_reserve_exact(16).map_err(|_| {
            UnityYamlObjectConversionError::AllocationFailed {
                resource: "Hash128 TypeTree field names",
                requested: 16,
            }
        })?;
        for child_index in children {
            let field = self.node(child_index)?;
            if !is_unsigned_byte_type_tree_type(self.type_name(child_index)?)
                || self.first_child[child_index].is_some()
            {
                return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                    node_index: child_index,
                    reason: "Hash128 field is not an unsigned-byte leaf",
                });
            }
            let name =
                field
                    .name
                    .as_deref()
                    .ok_or(UnityYamlObjectConversionError::InvalidTypeTree {
                        node_index: child_index,
                        reason: "Hash128 byte field name is unresolved",
                    })?;
            if byte_field_names.iter().any(|existing| existing == name) {
                return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                    node_index: child_index,
                    reason: "Hash128 byte field names are duplicated",
                });
            }
            byte_field_names.push(try_copy_string(name, "Hash128 TypeTree field name")?);
        }
        Ok(UnityYamlHash128Schema {
            // Pass008_DivideAmbiguousPPtr repairs this stale TypeTree version at Unity 5.
            serialized_version: if unity_version.major < 5 { 1 } else { 2 },
            byte_field_names,
        })
    }

    fn guid_schema(
        &self,
        node_index: usize,
    ) -> Result<UnityYamlGuidSchema, UnityYamlObjectConversionError> {
        const WORD_NAMES: [[&str; 2]; 4] = [
            ["m_Data_0_", "data[0]"],
            ["m_Data_1_", "data[1]"],
            ["m_Data_2_", "data[2]"],
            ["m_Data_3_", "data[3]"],
        ];
        let children = self.child_indices(node_index)?;
        if children.len() != WORD_NAMES.len() {
            return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                node_index,
                reason: "GUID does not contain exactly four data words",
            });
        }

        let mut word_field_names = Vec::new();
        word_field_names
            .try_reserve_exact(WORD_NAMES.len())
            .map_err(|_| UnityYamlObjectConversionError::AllocationFailed {
                resource: "GUID TypeTree field names",
                requested: WORD_NAMES.len(),
            })?;
        for expected_names in &WORD_NAMES {
            let mut matching_child = None;
            let mut matching_name = None;
            for &child_index in &children {
                let child_name = self.node(child_index)?.name.as_deref();
                if child_name.is_some_and(|name| expected_names.contains(&name)) {
                    if matching_child.is_some() {
                        return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                            node_index: child_index,
                            reason: "GUID word field name is duplicated",
                        });
                    }
                    matching_child = Some(child_index);
                    matching_name = child_name;
                }
            }
            let child_index =
                matching_child.ok_or(UnityYamlObjectConversionError::InvalidTypeTree {
                    node_index,
                    reason: "GUID data word field is missing",
                })?;
            if !is_unsigned_word_type_tree_type(self.type_name(child_index)?)
                || self.first_child[child_index].is_some()
            {
                return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                    node_index: child_index,
                    reason: "GUID data word is not an unsigned 32-bit leaf",
                });
            }
            let matching_name =
                matching_name.ok_or(UnityYamlObjectConversionError::InvalidTypeTree {
                    node_index,
                    reason: "GUID data word field is missing",
                })?;
            word_field_names.push(try_copy_string(matching_name, "GUID TypeTree field name")?);
        }
        let word_field_names = word_field_names.try_into().map_err(|_| {
            UnityYamlObjectConversionError::InvalidTypeTree {
                node_index,
                reason: "GUID does not contain exactly four data words",
            }
        })?;
        Ok(UnityYamlGuidSchema { word_field_names })
    }

    fn property_name_schema(
        &self,
        node_index: usize,
        explicit_property_name: bool,
    ) -> Result<Option<UnityYamlPropertyNameSchema>, UnityYamlObjectConversionError> {
        let children = self.child_indices(node_index)?;
        if children.len() != 1 {
            if explicit_property_name {
                return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                    node_index,
                    reason: "PropertyName must contain exactly one value field",
                });
            }
            return Ok(None);
        }

        let child_index = children[0];
        let child = self.node(child_index)?;
        let field_name =
            child
                .name
                .as_deref()
                .ok_or(UnityYamlObjectConversionError::InvalidTypeTree {
                    node_index: child_index,
                    reason: "PropertyName value field name is unresolved",
                })?;
        let child_type = self.type_name(child_index)?;
        let value_kind =
            if matches!(child_type, "int" | "SInt32") && self.first_child[child_index].is_none() {
                UnityYamlPropertyNameValueKind::Int32
            } else if matches!(child_type, "string" | "Utf8String")
                && is_string_type_tree_node(self, child_index)?
            {
                UnityYamlPropertyNameValueKind::Utf8String
            } else {
                if explicit_property_name {
                    return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                        node_index: child_index,
                        reason: "PropertyName value must be an Int32 or serialized UTF-8 string",
                    });
                }
                return Ok(None);
            };

        Ok(Some(UnityYamlPropertyNameSchema {
            value_field_name: try_copy_string(field_name, "PropertyName value field name")?,
            value_kind,
        }))
    }

    fn scene_object_identifier_schema(
        &self,
        node_index: usize,
    ) -> Result<UnityYamlSceneObjectIdentifierSchema, UnityYamlObjectConversionError> {
        let children = self.child_indices(node_index)?;
        let mut target_object_field = None;
        let mut target_prefab_field = None;
        for child_index in children {
            let child = self.node(child_index)?;
            let field_name =
                child
                    .name
                    .as_deref()
                    .ok_or(UnityYamlObjectConversionError::InvalidTypeTree {
                        node_index: child_index,
                        reason: "SceneObjectIdentifier field name is unresolved",
                    })?;
            let normalized_name = field_name.strip_prefix("m_").unwrap_or(field_name);
            let target_field = if normalized_name.eq_ignore_ascii_case("TargetObject") {
                &mut target_object_field
            } else if normalized_name.eq_ignore_ascii_case("TargetPrefab") {
                &mut target_prefab_field
            } else {
                continue;
            };
            if target_field.replace(child_index).is_some() {
                return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                    node_index: child_index,
                    reason: "SceneObjectIdentifier target field is duplicated",
                });
            }
        }

        let target_object_index =
            target_object_field.ok_or(UnityYamlObjectConversionError::InvalidTypeTree {
                node_index,
                reason: "SceneObjectIdentifier TargetObject field is missing",
            })?;
        let target_prefab_index =
            target_prefab_field.ok_or(UnityYamlObjectConversionError::InvalidTypeTree {
                node_index,
                reason: "SceneObjectIdentifier TargetPrefab field is missing",
            })?;
        let target_object_node = self.node(target_object_index)?;
        let target_prefab_node = self.node(target_prefab_index)?;
        if !is_signed_file_id_type_tree_type(self.type_name(target_object_index)?)
            || self.first_child[target_object_index].is_some()
        {
            return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                node_index: target_object_index,
                reason: "SceneObjectIdentifier TargetObject is not a signed 64-bit field",
            });
        }
        if !is_signed_file_id_type_tree_type(self.type_name(target_prefab_index)?)
            || self.first_child[target_prefab_index].is_some()
        {
            return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                node_index: target_prefab_index,
                reason: "SceneObjectIdentifier TargetPrefab is not a signed 64-bit field",
            });
        }

        let target_object_field_name = target_object_node.name.as_deref().ok_or(
            UnityYamlObjectConversionError::InvalidTypeTree {
                node_index: target_object_index,
                reason: "SceneObjectIdentifier TargetObject field name is unresolved",
            },
        )?;
        let target_prefab_field_name = target_prefab_node.name.as_deref().ok_or(
            UnityYamlObjectConversionError::InvalidTypeTree {
                node_index: target_prefab_index,
                reason: "SceneObjectIdentifier TargetPrefab field name is unresolved",
            },
        )?;
        Ok(UnityYamlSceneObjectIdentifierSchema {
            target_object_field_name: try_copy_string(
                target_object_field_name,
                "SceneObjectIdentifier TargetObject field name",
            )?,
            target_prefab_field_name: try_copy_string(
                target_prefab_field_name,
                "SceneObjectIdentifier TargetPrefab field name",
            )?,
        })
    }

    /// Whether a TypeTree node is a `pair` element whose first field is a valid
    /// dictionary key, so vector-held elements render keyed (`- key: value`).
    fn is_keyed_pair_element(
        &self,
        node_index: usize,
    ) -> Result<bool, UnityYamlObjectConversionError> {
        if self.type_name(node_index)? != "pair" {
            return Ok(false);
        }
        let pair_fields = self.child_indices(node_index)?;
        if pair_fields.len() != 2
            || self.node(pair_fields[0])?.name.as_deref() != Some("first")
            || self.node(pair_fields[1])?.name.as_deref() != Some("second")
        {
            return Ok(false);
        }
        Ok(is_valid_dictionary_key_type_tree_type(
            self.type_name(pair_fields[0])?,
        ))
    }

    fn dictionary_representation(
        &self,
        node_index: usize,
        element_node: usize,
        unity_version: SceneUnityVersion,
    ) -> Result<UnityYamlDictionaryRepresentation, UnityYamlObjectConversionError> {
        if self.type_name(node_index)? != "map" || self.type_name(element_node)? != "pair" {
            return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                node_index,
                reason: "map data node is not a pair",
            });
        }
        let pair_fields = self.child_indices(element_node)?;
        if pair_fields.len() != 2
            || self.node(pair_fields[0])?.name.as_deref() != Some("first")
            || self.node(pair_fields[1])?.name.as_deref() != Some("second")
        {
            return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                node_index: element_node,
                reason: "map pair must contain ordered first and second fields",
            });
        }

        if is_valid_dictionary_key_type_tree_type(self.type_name(pair_fields[0])?) {
            Ok(UnityYamlDictionaryRepresentation::KeyedMapping)
        } else if unity_version.major > 5 || (unity_version.major == 5 && unity_version.minor >= 4)
        {
            Ok(UnityYamlDictionaryRepresentation::PairSequence)
        } else {
            Ok(UnityYamlDictionaryRepresentation::LegacyPairMapping)
        }
    }

    fn collect_hints(
        &self,
        node_index: usize,
        field_path: &str,
        depth: usize,
        unity_version: SceneUnityVersion,
        hints: &mut UnityYamlObjectConversionHints,
        budget: &mut ConversionBudget,
    ) -> Result<(), UnityYamlObjectConversionError> {
        budget.visit(depth)?;
        let node = self.node(node_index)?;
        let node_type = self.type_name(node_index)?;

        if node.version > 1 && self.first_child[node_index].is_some() {
            if let Some(previous) = hints.serialized_version_paths.get(field_path) {
                if *previous != node.version {
                    return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                        node_index,
                        reason: "ambiguous serialized versions share one field path",
                    });
                }
            } else {
                hints.serialized_version_paths.try_reserve(1).map_err(|_| {
                    UnityYamlObjectConversionError::AllocationFailed {
                        resource: "Unity YAML serialized-version hints",
                        requested: hints.serialized_version_paths.len().saturating_add(1),
                    }
                })?;
                hints.serialized_version_paths.insert(
                    try_copy_string(field_path, "Unity YAML serialized-version path")?,
                    node.version,
                );
            }
        }

        let field_aliases = baseline_type_scoped_field_aliases(node_type);
        if !field_aliases.is_empty() || has_conditional_field_aliases(node_type) {
            for child_index in self.child_indices(node_index)? {
                let child = self.node(child_index)?;
                let Some(field_name) = child.name.as_deref() else {
                    continue;
                };
                let child_type = self.type_name(child_index)?;
                let alias = if node_type == "ValueArray" {
                    self.value_array_field_alias(child_index, field_name)?
                } else {
                    baseline_type_scoped_field_alias(node_type, field_name, child_type)
                };
                let Some(alias) = alias else {
                    if let Some(reason) =
                        unsupported_contextual_field_alias_reason(node_type, field_name, child_type)
                    {
                        return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                            node_index: child_index,
                            reason,
                        });
                    }
                    continue;
                };
                let alias_path =
                    child_field_path(field_path, field_name, budget.limits.max_field_path_bytes)?;
                if let Some(previous) = hints.field_name_aliases.get(&alias_path) {
                    if previous != alias {
                        return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                            node_index: child_index,
                            reason: "ambiguous field aliases share one serialized field path",
                        });
                    }
                } else {
                    hints.field_name_aliases.try_reserve(1).map_err(|_| {
                        UnityYamlObjectConversionError::AllocationFailed {
                            resource: "Unity YAML field-name alias hints",
                            requested: hints.field_name_aliases.len().saturating_add(1),
                        }
                    })?;
                    hints.field_name_aliases.insert(
                        alias_path,
                        try_copy_string(alias, "Unity YAML field-name alias")?,
                    );
                }
            }
        }

        if !field_path.is_empty() && node.meta_flags & TRANSFER_IGNORE_IN_META_FILES != 0 {
            hints
                .importer_excluded_field_paths
                .try_reserve(1)
                .map_err(|_| UnityYamlObjectConversionError::AllocationFailed {
                    resource: "Unity YAML importer field paths",
                    requested: hints.importer_excluded_field_paths.len().saturating_add(1),
                })?;
            hints.importer_excluded_field_paths.insert(try_copy_string(
                field_path,
                "Unity YAML importer field path",
            )?);
        }

        if node_type == "ExposedReferenceTable" {
            for child_index in self.child_indices(node_index)? {
                let child = self.node(child_index)?;
                if child.name.as_deref() == Some("m_References")
                    && self.array_element_node(child_index)?.is_some()
                {
                    let sequence_path = child_field_path(
                        field_path,
                        "m_References",
                        budget.limits.max_field_path_bytes,
                    )?;
                    if !hints.force_block_sequence_paths.contains(&sequence_path) {
                        hints
                            .force_block_sequence_paths
                            .try_reserve(1)
                            .map_err(|_| UnityYamlObjectConversionError::AllocationFailed {
                                resource: "Unity YAML block-sequence override paths",
                                requested: hints.force_block_sequence_paths.len().saturating_add(1),
                            })?;
                        hints.force_block_sequence_paths.insert(sequence_path);
                    }
                }
            }
        }

        let array_element_node = self.array_element_node(node_index)?;
        if node_type == "map" && array_element_node.is_none() {
            return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                node_index,
                reason: "map node has no valid Array/size/data shape",
            });
        }
        if let Some(element_node) = array_element_node {
            if node_type == "map" && !hints.force_block_sequence_paths.contains(field_path) {
                let representation =
                    self.dictionary_representation(node_index, element_node, unity_version)?;
                if let Some(previous) = hints.dictionary_representations.get(field_path) {
                    if *previous != representation {
                        return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                            node_index,
                            reason: "ambiguous dictionaries share one serialized field path",
                        });
                    }
                } else {
                    hints
                        .dictionary_representations
                        .try_reserve(1)
                        .map_err(|_| UnityYamlObjectConversionError::AllocationFailed {
                            resource: "Unity YAML dictionary schema hints",
                            requested: hints.dictionary_representations.len().saturating_add(1),
                        })?;
                    hints.dictionary_representations.insert(
                        try_copy_string(field_path, "Unity YAML dictionary schema path")?,
                        representation,
                    );
                }
            } else {
                let element_type = self.type_name(element_node)?;
                let kind = if is_integer_type_tree_type(element_type) {
                    UnityYamlEmptyArrayKind::IntegerHex
                } else {
                    UnityYamlEmptyArrayKind::BlockSequence
                };
                if let Some(previous) = hints.empty_array_kinds.get(field_path) {
                    if *previous != kind {
                        return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                            node_index,
                            reason: "ambiguous arrays share one serialized field path",
                        });
                    }
                } else {
                    hints.empty_array_kinds.try_reserve(1).map_err(|_| {
                        UnityYamlObjectConversionError::AllocationFailed {
                            resource: "Unity YAML array schema hints",
                            requested: hints.empty_array_kinds.len().saturating_add(1),
                        }
                    })?;
                    hints.empty_array_kinds.insert(
                        try_copy_string(field_path, "Unity YAML array hint path")?,
                        kind,
                    );
                }
            }

            let element_path = wildcard_element_path(field_path, budget.limits)?;
            return self.collect_hints(
                element_node,
                &element_path,
                depth + 1,
                unity_version,
                hints,
                budget,
            );
        }

        if node_type == "Hash128" {
            let schema = self.hash128_schema(node_index, unity_version)?;
            if let Some(previous) = hints.hash128_schemas.get(field_path) {
                if *previous != schema {
                    return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                        node_index,
                        reason: "ambiguous Hash128 schemas share one serialized field path",
                    });
                }
            } else {
                hints.hash128_schemas.try_reserve(1).map_err(|_| {
                    UnityYamlObjectConversionError::AllocationFailed {
                        resource: "Unity YAML Hash128 schema hints",
                        requested: hints.hash128_schemas.len().saturating_add(1),
                    }
                })?;
                hints.hash128_schemas.insert(
                    try_copy_string(field_path, "Unity YAML Hash128 schema path")?,
                    schema,
                );
            }
            return Ok(());
        }

        if node_type == "GUID" {
            let schema = self.guid_schema(node_index)?;
            if let Some(previous) = hints.guid_schemas.get(field_path) {
                if *previous != schema {
                    return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                        node_index,
                        reason: "ambiguous GUID schemas share one serialized field path",
                    });
                }
            } else {
                hints.guid_schemas.try_reserve(1).map_err(|_| {
                    UnityYamlObjectConversionError::AllocationFailed {
                        resource: "Unity YAML GUID schema hints",
                        requested: hints.guid_schemas.len().saturating_add(1),
                    }
                })?;
                hints.guid_schemas.insert(
                    try_copy_string(field_path, "Unity YAML GUID schema path")?,
                    schema,
                );
            }
            return Ok(());
        }

        if node_type == "SceneObjectIdentifier" {
            let schema = self.scene_object_identifier_schema(node_index)?;
            if let Some(previous) = hints.scene_object_identifier_schemas.get(field_path) {
                if *previous != schema {
                    return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                        node_index,
                        reason: "ambiguous SceneObjectIdentifier schemas share one serialized field path",
                    });
                }
            } else {
                hints
                    .scene_object_identifier_schemas
                    .try_reserve(1)
                    .map_err(|_| UnityYamlObjectConversionError::AllocationFailed {
                        resource: "Unity YAML SceneObjectIdentifier schema hints",
                        requested: hints
                            .scene_object_identifier_schemas
                            .len()
                            .saturating_add(1),
                    })?;
                hints.scene_object_identifier_schemas.insert(
                    try_copy_string(field_path, "Unity YAML SceneObjectIdentifier schema path")?,
                    schema,
                );
            }
            return Ok(());
        }

        let explicit_property_name = node_type == "PropertyName";
        if explicit_property_name || node_type == "string" {
            if let Some(schema) = self.property_name_schema(node_index, explicit_property_name)? {
                if let Some(previous) = hints.property_name_schemas.get(field_path) {
                    if *previous != schema {
                        return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                            node_index,
                            reason:
                                "ambiguous PropertyName schemas share one serialized field path",
                        });
                    }
                } else {
                    hints.property_name_schemas.try_reserve(1).map_err(|_| {
                        UnityYamlObjectConversionError::AllocationFailed {
                            resource: "Unity YAML PropertyName schema hints",
                            requested: hints.property_name_schemas.len().saturating_add(1),
                        }
                    })?;
                    hints.property_name_schemas.insert(
                        try_copy_string(field_path, "Unity YAML PropertyName schema path")?,
                        schema,
                    );
                }
                return Ok(());
            }
        }

        if node_type == "TypelessData" || node_type == "string" || node_type.starts_with("PPtr<") {
            if node_type == "TypelessData" {
                // `TypelessData` members expand into a byte count plus a `_typelessdata`
                // payload in project YAML, even though the tree carries one `Bytes`
                // value (observed on Mesh `m_DataSize`, Texture2D `image data`, and
                // Sprite render-data vertex blobs). Recorded before the leaf early
                // return below.
                hints
                    .typeless_data_expansion_paths
                    .try_reserve(1)
                    .map_err(|_| UnityYamlObjectConversionError::AllocationFailed {
                        resource: "Unity YAML TypelessData hint paths",
                        requested: hints.typeless_data_expansion_paths.len().saturating_add(1),
                    })?;
                hints.typeless_data_expansion_paths.insert(try_copy_string(
                    field_path,
                    "Unity YAML TypelessData hint path",
                )?);
            }
            return Ok(());
        }

        if node.meta_flags & TRANSFER_USING_FLOW_MAPPING_STYLE != 0 {
            hints.flow_mapped_struct_paths.try_reserve(1).map_err(|_| {
                UnityYamlObjectConversionError::AllocationFailed {
                    resource: "Unity YAML flow mapping hints",
                    requested: hints.flow_mapped_struct_paths.len().saturating_add(1),
                }
            })?;
            hints.flow_mapped_struct_paths.insert(try_copy_string(
                field_path,
                "Unity YAML flow mapping hint path",
            )?);
        }

        // `pair` nodes with valid dictionary-key first fields render as keyed
        // single-pair mappings (`- key: value`) rather than `first`/`second`
        // structs, in any position (vector elements, direct fields). Pairs owned
        // by `map` nodes record here too but always convert through the dictionary
        // branch, so the hint never misfires there; pairs with other key types
        // keep struct rendering.
        if self.is_keyed_pair_element(node_index)? {
            hints.keyed_pair_element_paths.try_reserve(1).map_err(|_| {
                UnityYamlObjectConversionError::AllocationFailed {
                    resource: "Unity YAML keyed pair hint paths",
                    requested: hints.keyed_pair_element_paths.len().saturating_add(1),
                }
            })?;
            hints.keyed_pair_element_paths.insert(try_copy_string(
                field_path,
                "Unity YAML keyed pair hint path",
            )?);
        }

        let mut child = *self.first_child.get(node_index).ok_or(
            UnityYamlObjectConversionError::InvalidTypeTree {
                node_index,
                reason: "node index is outside the tree",
            },
        )?;
        while let Some(child_index) = child {
            let child_node = self.node(child_index)?;
            let field_name = child_node.name.as_deref().ok_or(
                UnityYamlObjectConversionError::InvalidTypeTree {
                    node_index: child_index,
                    reason: "field name is unresolved",
                },
            )?;
            let child_path =
                child_field_path(field_path, field_name, budget.limits.max_field_path_bytes)?;
            self.collect_hints(
                child_index,
                &child_path,
                depth + 1,
                unity_version,
                hints,
                budget,
            )?;
            child = self.next_sibling[child_index];
        }
        Ok(())
    }
}

fn is_string_type_tree_node(
    layout: &UnityYamlTypeTreeLayout<'_>,
    node_index: usize,
) -> Result<bool, UnityYamlObjectConversionError> {
    let children = layout.child_indices(node_index)?;
    if children.len() != 1 {
        return Ok(false);
    }
    let Some(data_node) = layout.array_shape_data_node(children[0])? else {
        return Ok(false);
    };
    Ok(matches!(layout.type_name(data_node)?, "char" | "UInt8")
        && layout.first_child[data_node].is_none())
}
