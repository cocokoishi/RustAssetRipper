//! Bounded conversion from decoded SerializedValue trees to Unity YAML object documents.

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::str;

use crate::processing::scene_naming::SceneUnityVersion;
use crate::serialized_file::metadata::{TypeTree, TypeTreeNode};
use crate::serialized_file::object::{SerializedFieldValue, SerializedValue};

use super::pointer_map::ProjectPointerResolution;
use super::scene_collection::ExportAssetIdentity;
use super::unity_yaml::{
    format_unity_guid_words, project_pointer_to_unity_yaml, UnityYamlDocument,
    UnityYamlMappingStyle, UnityYamlNode, UnityYamlNodeBuildError, UnityYamlScalar,
    UnityYamlSequenceStyle, UnityYamlWriteError,
};

const TRANSFER_USING_FLOW_MAPPING_STYLE: u32 = 0x0020_0000;
const TRANSFER_IGNORE_IN_META_FILES: u32 = 0x0008_0000;

const IMPORTER_FIELDS_TO_SKIP: &[&str] = &[
    "m_ObjectHideFlags",
    "m_ExtensionPtr",
    "m_PrefabParentObject",
    "m_CorrespondingSourceObject",
    "m_PrefabInternal",
    "m_PrefabAsset",
    "m_PrefabInstance",
];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum UnityYamlObjectFieldMode {
    #[default]
    ProjectAsset,
    AssetImporter,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnityYamlEmptyArrayKind {
    /// Baseline integer/bool/char arrays are flattened to a reverse-endian hexadecimal scalar.
    IntegerHex,
    BlockSequence,
    BlockCurveSequence,
    FlowSequence,
}

/// TypeTree-derived serialization choices needed by the YAML walker.
///
/// Empty arrays have no element values from which to infer their representation. Flow-mapped
/// structs also need the TypeTree transfer flag because SerializedValue does not retain schema
/// metadata.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UnityYamlObjectConversionHints {
    pub empty_array_kinds: HashMap<String, UnityYamlEmptyArrayKind>,
    /// Paths may contain [*] where a TypeTree struct appears as an array element.
    pub flow_mapped_struct_paths: HashSet<String>,
    /// TypeTree fields marked `IgnoreInMetaFiles`, using [*] for array elements.
    pub importer_excluded_field_paths: HashSet<String>,
    pub hash128_schemas: HashMap<String, UnityYamlHash128Schema>,
    pub guid_schemas: HashMap<String, UnityYamlGuidSchema>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnityYamlHash128Schema {
    pub serialized_version: i32,
    /// TypeTree byte fields in their serialized order.
    pub byte_field_names: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnityYamlGuidSchema {
    /// Four TypeTree words in UnityGuid constructor order.
    pub word_field_names: [String; 4],
}

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
        let mut budget = ConversionBudget { visited: 0, limits };
        layout.collect_hints(0, "", 0, unity_version, &mut hints, &mut budget)?;
        Ok(hints)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnityYamlObjectConversionLimits {
    pub max_values: usize,
    pub max_type_tree_nodes: usize,
    pub max_depth: usize,
    pub max_field_path_bytes: usize,
    pub max_generated_scalar_bytes: usize,
}

impl Default for UnityYamlObjectConversionLimits {
    fn default() -> Self {
        Self {
            max_values: 20_000_000,
            max_type_tree_nodes: 1_000_000,
            max_depth: 256,
            max_field_path_bytes: 1024 * 1024,
            max_generated_scalar_bytes: 512 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnityYamlPPtrRequest<'a> {
    pub source: ExportAssetIdentity,
    pub field_path: &'a str,
    pub file_id: i32,
    pub path_id: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UnityYamlObjectConversionError {
    RootIsNotStruct,
    ValueLimitExceeded {
        requested: usize,
        limit: usize,
    },
    DepthLimitExceeded {
        requested: usize,
        limit: usize,
    },
    FieldPathLimitExceeded {
        requested: usize,
        limit: usize,
    },
    MissingEmptyArrayHint {
        field_path: String,
    },
    InvalidIntegerArray {
        field_path: String,
        element_index: usize,
    },
    UnsupportedRawValue {
        field_path: String,
        type_name: String,
    },
    InvalidUtf8String {
        field_path: String,
    },
    InvalidTypeTree {
        node_index: usize,
        reason: &'static str,
    },
    Hash128FieldCount {
        field_path: String,
        expected: usize,
        actual: usize,
    },
    InvalidHash128Byte {
        field_path: String,
        field_name: String,
        field_index: usize,
    },
    GuidFieldCount {
        field_path: String,
        expected: usize,
        actual: usize,
    },
    InvalidGuidWord {
        field_path: String,
        field_name: String,
        word_index: usize,
    },
    HexScalarLimitExceeded {
        requested: usize,
        limit: usize,
        field_path: String,
    },
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    PointerConversion {
        field_path: String,
        source: Box<UnityYamlWriteError>,
    },
    NodeBuild(UnityYamlNodeBuildError),
}

impl Display for UnityYamlObjectConversionError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::RootIsNotStruct => {
                f.write_str("Unity object YAML conversion requires a struct root")
            }
            Self::ValueLimitExceeded { requested, limit } => write!(
                f,
                "Unity object YAML conversion visited {requested} values, exceeding limit {limit}"
            ),
            Self::DepthLimitExceeded { requested, limit } => write!(
                f,
                "Unity object YAML value depth {requested} exceeds limit {limit}"
            ),
            Self::FieldPathLimitExceeded { requested, limit } => write!(
                f,
                "Unity object YAML field path has {requested} bytes, exceeding limit {limit}"
            ),
            Self::MissingEmptyArrayHint { field_path } => write!(
                f,
                "empty Unity array at {field_path:?} requires TypeTree element-type metadata"
            ),
            Self::InvalidIntegerArray {
                field_path,
                element_index,
            } => write!(
                f,
                "integer-hex Unity array at {field_path:?} has a non-integer or inconsistent element at index {element_index}"
            ),
            Self::UnsupportedRawValue {
                field_path,
                type_name,
            } => write!(
                f,
                "cannot structurally emit raw TypeTree value {type_name:?} at {field_path:?}"
            ),
            Self::InvalidUtf8String { field_path } => {
                write!(f, "Unity string at {field_path:?} is not valid UTF-8")
            }
            Self::InvalidTypeTree { node_index, reason } => {
                write!(f, "invalid Unity TypeTree node {node_index}: {reason}")
            }
            Self::Hash128FieldCount {
                field_path,
                expected,
                actual,
            } => write!(
                f,
                "Hash128 at {field_path:?} has {actual} fields; expected {expected}"
            ),
            Self::InvalidHash128Byte {
                field_path,
                field_name,
                field_index,
            } => write!(
                f,
                "Hash128 byte {field_index} ({field_name:?}) at {field_path:?} is missing, duplicated, or not an unsigned byte"
            ),
            Self::GuidFieldCount {
                field_path,
                expected,
                actual,
            } => write!(
                f,
                "GUID at {field_path:?} has {actual} fields; expected {expected}"
            ),
            Self::InvalidGuidWord {
                field_path,
                field_name,
                word_index,
            } => write!(
                f,
                "GUID word {word_index} ({field_name:?}) at {field_path:?} is missing, duplicated, or not an unsigned 32-bit value"
            ),
            Self::HexScalarLimitExceeded {
                requested,
                limit,
                field_path,
            } => write!(
                f,
                "Unity hex scalar at {field_path:?} needs {requested} bytes, exceeding limit {limit}"
            ),
            Self::AllocationFailed { resource, requested } => {
                write!(f, "could not allocate {requested} bytes for {resource}")
            }
            Self::PointerConversion { field_path, source } => {
                write!(f, "cannot convert PPtr at {field_path:?}: {source}")
            }
            Self::NodeBuild(error) => Display::fmt(error, f),
        }
    }
}

impl Error for UnityYamlObjectConversionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::PointerConversion { source, .. } => Some(source.as_ref()),
            Self::NodeBuild(error) => Some(error),
            _ => None,
        }
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
        const WORD_NAMES: [&str; 4] = ["m_Data_0_", "m_Data_1_", "m_Data_2_", "m_Data_3_"];
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
        for expected_name in &WORD_NAMES {
            let mut matching_child = None;
            for &child_index in &children {
                if self.node(child_index)?.name.as_deref() == Some(*expected_name) {
                    if matching_child.is_some() {
                        return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                            node_index: child_index,
                            reason: "GUID word field name is duplicated",
                        });
                    }
                    matching_child = Some(child_index);
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
            word_field_names.push(try_copy_string(expected_name, "GUID TypeTree field name")?);
        }
        let word_field_names = word_field_names.try_into().map_err(|_| {
            UnityYamlObjectConversionError::InvalidTypeTree {
                node_index,
                reason: "GUID does not contain exactly four data words",
            }
        })?;
        Ok(UnityYamlGuidSchema { word_field_names })
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

        if let Some(element_node) = self.array_element_node(node_index)? {
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

        if node_type == "TypelessData" || node_type == "string" || node_type.starts_with("PPtr<") {
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

/// Convert one already-decoded object value into a Unity YAML object document.
///
/// `SerializedValue` retains ordered fields and PPtrs but not the TypeTree schema. Callers supply
/// element-kind hints for empty arrays, where the serialized value alone cannot distinguish a
/// numeric hex list from an object sequence. Non-null PPtrs are resolved by the caller from the
/// multi-file graph and project export map. The generic conversion deliberately rejects raw
/// unknown leaves instead of replacing them with hex dumps.
#[allow(clippy::too_many_arguments)]
pub fn serialized_object_value_to_unity_yaml_document<F>(
    source: ExportAssetIdentity,
    class_name: &str,
    serialized_version: i32,
    file_id: i64,
    stripped: bool,
    root: &SerializedValue,
    hints: &UnityYamlObjectConversionHints,
    field_mode: UnityYamlObjectFieldMode,
    unity_version: SceneUnityVersion,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlObjectConversionLimits,
    mut resolve_pointer: F,
) -> Result<UnityYamlDocument, UnityYamlObjectConversionError>
where
    F: FnMut(UnityYamlPPtrRequest<'_>) -> ProjectPointerResolution,
{
    let SerializedValue::Struct(fields) = root else {
        return Err(UnityYamlObjectConversionError::RootIsNotStruct);
    };
    let mut budget = ConversionBudget { visited: 0, limits };
    budget.visit(0)?;

    let body = if let Some(schema) = hash128_schema_for_path("", hints, limits)? {
        convert_hash128_value(fields, schema, "", 0, limits, &mut budget)?
    } else {
        let mut body = UnityYamlNode::mapping(if is_flow_mapped_path("", hints, limits)? {
            UnityYamlMappingStyle::Flow
        } else {
            UnityYamlMappingStyle::Block
        });
        body.insert_field(
            "serializedVersion",
            UnityYamlNode::scalar(UnityYamlScalar::Signed(i64::from(serialized_version))),
        )
        .map_err(UnityYamlObjectConversionError::NodeBuild)?;
        append_struct_fields(
            &mut body,
            fields,
            "",
            1,
            source,
            hints,
            field_mode,
            unity_version,
            missing_reference_guid,
            limits,
            &mut budget,
            &mut resolve_pointer,
        )?;
        body
    };
    UnityYamlDocument::object(source.class_id, file_id, class_name, body, stripped)
        .map_err(UnityYamlObjectConversionError::NodeBuild)
}

#[allow(clippy::too_many_arguments)]
fn append_struct_fields<F>(
    destination: &mut UnityYamlNode,
    fields: &[SerializedFieldValue],
    parent_path: &str,
    depth: usize,
    source: ExportAssetIdentity,
    hints: &UnityYamlObjectConversionHints,
    field_mode: UnityYamlObjectFieldMode,
    unity_version: SceneUnityVersion,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlObjectConversionLimits,
    budget: &mut ConversionBudget,
    resolve_pointer: &mut F,
) -> Result<(), UnityYamlObjectConversionError>
where
    F: FnMut(UnityYamlPPtrRequest<'_>) -> ProjectPointerResolution,
{
    for field in fields {
        let field_path = child_field_path(parent_path, &field.name, limits.max_field_path_bytes)?;
        if field_mode == UnityYamlObjectFieldMode::AssetImporter
            && importer_field_is_excluded(&field.name, &field_path, hints, limits)?
        {
            continue;
        }
        if field.name == "Base" {
            if let SerializedValue::Struct(base_fields) = &field.value {
                budget.visit(depth)?;
                append_struct_fields(
                    destination,
                    base_fields,
                    &field_path,
                    depth + 1,
                    source,
                    hints,
                    field_mode,
                    unity_version,
                    missing_reference_guid,
                    limits,
                    budget,
                    resolve_pointer,
                )?;
                continue;
            }
        }
        let value = convert_value(
            &field.value,
            &field_path,
            depth,
            source,
            hints,
            field_mode,
            unity_version,
            missing_reference_guid,
            limits,
            budget,
            resolve_pointer,
        )?;
        let field_name = try_copy_string(&field.name, "Unity YAML field name")?;
        destination
            .insert_field(field_name, value)
            .map_err(UnityYamlObjectConversionError::NodeBuild)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn convert_value<F>(
    value: &SerializedValue,
    field_path: &str,
    depth: usize,
    source: ExportAssetIdentity,
    hints: &UnityYamlObjectConversionHints,
    field_mode: UnityYamlObjectFieldMode,
    unity_version: SceneUnityVersion,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlObjectConversionLimits,
    budget: &mut ConversionBudget,
    resolve_pointer: &mut F,
) -> Result<UnityYamlNode, UnityYamlObjectConversionError>
where
    F: FnMut(UnityYamlPPtrRequest<'_>) -> ProjectPointerResolution,
{
    budget.visit(depth)?;
    let node = match value {
        SerializedValue::Bool(value) => UnityYamlNode::scalar(UnityYamlScalar::Boolean(*value)),
        SerializedValue::I8(value) => {
            UnityYamlNode::scalar(UnityYamlScalar::Signed(i64::from(*value)))
        }
        SerializedValue::U8(value) => {
            UnityYamlNode::scalar(UnityYamlScalar::Unsigned(u64::from(*value)))
        }
        SerializedValue::I16(value) => {
            UnityYamlNode::scalar(UnityYamlScalar::Signed(i64::from(*value)))
        }
        SerializedValue::U16(value) | SerializedValue::Char(value) => {
            UnityYamlNode::scalar(UnityYamlScalar::Unsigned(u64::from(*value)))
        }
        SerializedValue::I32(value) => {
            UnityYamlNode::scalar(UnityYamlScalar::Signed(i64::from(*value)))
        }
        SerializedValue::U32(value) => {
            UnityYamlNode::scalar(UnityYamlScalar::Unsigned(u64::from(*value)))
        }
        SerializedValue::I64(value) => UnityYamlNode::scalar(UnityYamlScalar::Signed(*value)),
        SerializedValue::U64(value) => UnityYamlNode::scalar(UnityYamlScalar::Unsigned(*value)),
        SerializedValue::F32(value) => UnityYamlNode::scalar(UnityYamlScalar::Float32(*value)),
        SerializedValue::F64(value) => UnityYamlNode::scalar(UnityYamlScalar::Float64(*value)),
        SerializedValue::StringBytes(bytes) => {
            let value = str::from_utf8(bytes).map_err(|_| {
                UnityYamlObjectConversionError::InvalidUtf8String {
                    field_path: field_path.to_owned(),
                }
            })?;
            UnityYamlNode::string(try_copy_string(value, "Unity YAML string")?)
        }
        SerializedValue::Bytes(bytes) => {
            let hex_length = bytes.len().checked_mul(2).ok_or(
                UnityYamlObjectConversionError::HexScalarLimitExceeded {
                    requested: usize::MAX,
                    limit: limits.max_generated_scalar_bytes,
                    field_path: field_path.to_owned(),
                },
            )?;
            let hex = encode_byte_hex(bytes, hex_length, field_path, limits)?;
            UnityYamlNode::plain_string(hex)
        }
        SerializedValue::Array(items) => convert_array(
            items,
            field_path,
            depth,
            source,
            hints,
            field_mode,
            unity_version,
            missing_reference_guid,
            limits,
            budget,
            resolve_pointer,
        )?,
        SerializedValue::Struct(fields) => {
            if let Some(schema) = hash128_schema_for_path(field_path, hints, limits)? {
                convert_hash128_value(fields, schema, field_path, depth, limits, budget)?
            } else if let Some(schema) = guid_schema_for_path(field_path, hints, limits)? {
                convert_guid_value(fields, schema, field_path, depth, budget)?
            } else {
                let mut mapping =
                    UnityYamlNode::mapping(if is_flow_mapped_path(field_path, hints, limits)? {
                        UnityYamlMappingStyle::Flow
                    } else {
                        UnityYamlMappingStyle::Block
                    });
                append_struct_fields(
                    &mut mapping,
                    fields,
                    field_path,
                    depth + 1,
                    source,
                    hints,
                    field_mode,
                    unity_version,
                    missing_reference_guid,
                    limits,
                    budget,
                    resolve_pointer,
                )?;
                mapping
            }
        }
        SerializedValue::PPtr { file_id, path_id } => {
            let resolution = if *path_id == 0 {
                ProjectPointerResolution::Null
            } else {
                resolve_pointer(UnityYamlPPtrRequest {
                    source,
                    field_path,
                    file_id: *file_id,
                    path_id: *path_id,
                })
            };
            project_pointer_to_unity_yaml(&resolution, unity_version, missing_reference_guid)
                .map_err(|source| UnityYamlObjectConversionError::PointerConversion {
                    field_path: field_path.to_owned(),
                    source: Box::new(source),
                })?
        }
        SerializedValue::Raw { type_name, .. } => {
            return Err(UnityYamlObjectConversionError::UnsupportedRawValue {
                field_path: field_path.to_owned(),
                type_name: type_name.clone(),
            });
        }
    };
    Ok(node)
}

fn hash128_schema_for_path<'a>(
    field_path: &str,
    hints: &'a UnityYamlObjectConversionHints,
    limits: UnityYamlObjectConversionLimits,
) -> Result<Option<&'a UnityYamlHash128Schema>, UnityYamlObjectConversionError> {
    if let Some(schema) = hints.hash128_schemas.get(field_path) {
        return Ok(Some(schema));
    }
    if !field_path.contains('[') {
        return Ok(None);
    }
    let canonical_path = canonical_array_path(field_path, limits.max_field_path_bytes)?;
    Ok(hints.hash128_schemas.get(&canonical_path))
}

fn guid_schema_for_path<'a>(
    field_path: &str,
    hints: &'a UnityYamlObjectConversionHints,
    limits: UnityYamlObjectConversionLimits,
) -> Result<Option<&'a UnityYamlGuidSchema>, UnityYamlObjectConversionError> {
    if let Some(schema) = hints.guid_schemas.get(field_path) {
        return Ok(Some(schema));
    }
    if !field_path.contains('[') {
        return Ok(None);
    }
    let canonical_path = canonical_array_path(field_path, limits.max_field_path_bytes)?;
    Ok(hints.guid_schemas.get(&canonical_path))
}

fn convert_hash128_value(
    fields: &[SerializedFieldValue],
    schema: &UnityYamlHash128Schema,
    field_path: &str,
    depth: usize,
    limits: UnityYamlObjectConversionLimits,
    budget: &mut ConversionBudget,
) -> Result<UnityYamlNode, UnityYamlObjectConversionError> {
    const HASH128_BYTE_COUNT: usize = 16;
    if schema.byte_field_names.len() != HASH128_BYTE_COUNT || fields.len() != HASH128_BYTE_COUNT {
        return Err(UnityYamlObjectConversionError::Hash128FieldCount {
            field_path: field_path.to_owned(),
            expected: HASH128_BYTE_COUNT,
            actual: fields.len(),
        });
    }

    let mut bytes = [0u8; HASH128_BYTE_COUNT];
    for (field_index, field_name) in schema.byte_field_names.iter().enumerate() {
        budget.visit(depth + 1)?;
        let mut matching_value = None;
        for field in fields.iter().filter(|field| field.name == *field_name) {
            if matching_value.is_some() {
                return Err(UnityYamlObjectConversionError::InvalidHash128Byte {
                    field_path: field_path.to_owned(),
                    field_name: field_name.clone(),
                    field_index,
                });
            }
            matching_value = Some(&field.value);
        }
        let Some(SerializedValue::U8(value)) = matching_value else {
            return Err(UnityYamlObjectConversionError::InvalidHash128Byte {
                field_path: field_path.to_owned(),
                field_name: field_name.clone(),
                field_index,
            });
        };
        bytes[field_index] = *value;
    }

    let mut mapping = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    mapping
        .insert_field(
            "serializedVersion",
            UnityYamlNode::scalar(UnityYamlScalar::Signed(i64::from(
                schema.serialized_version,
            ))),
        )
        .map_err(UnityYamlObjectConversionError::NodeBuild)?;
    if schema.serialized_version > 1 {
        const HASH_STRING_LENGTH: usize = HASH128_BYTE_COUNT * 2;
        if HASH_STRING_LENGTH > limits.max_generated_scalar_bytes {
            return Err(UnityYamlObjectConversionError::HexScalarLimitExceeded {
                requested: HASH_STRING_LENGTH,
                limit: limits.max_generated_scalar_bytes,
                field_path: field_path.to_owned(),
            });
        }
        let mut hash = String::new();
        hash.try_reserve_exact(HASH_STRING_LENGTH).map_err(|_| {
            UnityYamlObjectConversionError::AllocationFailed {
                resource: "Hash128 YAML string",
                requested: HASH_STRING_LENGTH,
            }
        })?;
        for chunk in bytes.chunks_exact(4) {
            let word = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            for shift in (0..8).rev() {
                hash.push(hex_digit(((word >> (shift * 4)) & 0x0f) as u8));
            }
        }
        mapping
            .insert_field("Hash", UnityYamlNode::string(hash))
            .map_err(UnityYamlObjectConversionError::NodeBuild)?;
    } else {
        for (field_index, value) in bytes.into_iter().enumerate() {
            mapping
                .insert_field(
                    format!("bytes[{field_index}]"),
                    UnityYamlNode::scalar(UnityYamlScalar::Unsigned(u64::from(value))),
                )
                .map_err(UnityYamlObjectConversionError::NodeBuild)?;
        }
    }
    Ok(mapping)
}

fn convert_guid_value(
    fields: &[SerializedFieldValue],
    schema: &UnityYamlGuidSchema,
    field_path: &str,
    depth: usize,
    budget: &mut ConversionBudget,
) -> Result<UnityYamlNode, UnityYamlObjectConversionError> {
    if fields.len() != schema.word_field_names.len() {
        return Err(UnityYamlObjectConversionError::GuidFieldCount {
            field_path: field_path.to_owned(),
            expected: schema.word_field_names.len(),
            actual: fields.len(),
        });
    }

    let mut words = [0u32; 4];
    for (word_index, field_name) in schema.word_field_names.iter().enumerate() {
        budget.visit(depth + 1)?;
        let mut matching_value = None;
        for field in fields.iter().filter(|field| field.name == *field_name) {
            if matching_value.is_some() {
                return Err(UnityYamlObjectConversionError::InvalidGuidWord {
                    field_path: field_path.to_owned(),
                    field_name: field_name.clone(),
                    word_index,
                });
            }
            matching_value = Some(&field.value);
        }
        let Some(SerializedValue::U32(value)) = matching_value else {
            return Err(UnityYamlObjectConversionError::InvalidGuidWord {
                field_path: field_path.to_owned(),
                field_name: field_name.clone(),
                word_index,
            });
        };
        words[word_index] = *value;
    }

    let mut serialized_guid = [0u8; 16];
    for (word_index, word) in words.into_iter().enumerate() {
        let start = word_index * 4;
        serialized_guid[start..start + 4].copy_from_slice(&word.to_le_bytes());
    }
    Ok(UnityYamlNode::string(format_unity_guid_words(
        serialized_guid,
    )))
}

#[allow(clippy::too_many_arguments)]
fn convert_array<F>(
    items: &[SerializedValue],
    field_path: &str,
    depth: usize,
    source: ExportAssetIdentity,
    hints: &UnityYamlObjectConversionHints,
    field_mode: UnityYamlObjectFieldMode,
    unity_version: SceneUnityVersion,
    missing_reference_guid: Option<[u8; 16]>,
    limits: UnityYamlObjectConversionLimits,
    budget: &mut ConversionBudget,
    resolve_pointer: &mut F,
) -> Result<UnityYamlNode, UnityYamlObjectConversionError>
where
    F: FnMut(UnityYamlPPtrRequest<'_>) -> ProjectPointerResolution,
{
    if items.is_empty() {
        let kind = hints
            .empty_array_kinds
            .get(field_path)
            .copied()
            .ok_or_else(|| UnityYamlObjectConversionError::MissingEmptyArrayHint {
                field_path: field_path.to_owned(),
            })?;
        return match kind {
            UnityYamlEmptyArrayKind::IntegerHex => Ok(UnityYamlNode::plain_string(String::new())),
            UnityYamlEmptyArrayKind::BlockSequence => {
                Ok(UnityYamlNode::sequence(UnityYamlSequenceStyle::Block))
            }
            UnityYamlEmptyArrayKind::BlockCurveSequence => {
                Ok(UnityYamlNode::sequence(UnityYamlSequenceStyle::BlockCurve))
            }
            UnityYamlEmptyArrayKind::FlowSequence => {
                Ok(UnityYamlNode::sequence(UnityYamlSequenceStyle::Flow))
            }
        };
    }

    if let Some(kind) = hints.empty_array_kinds.get(field_path) {
        if *kind == UnityYamlEmptyArrayKind::IntegerHex {
            return convert_integer_array(items, field_path, depth, limits, budget);
        }
    } else if integer_array_width(&items[0]).is_some() {
        return convert_integer_array(items, field_path, depth, limits, budget);
    }

    let style = match hints.empty_array_kinds.get(field_path) {
        Some(UnityYamlEmptyArrayKind::BlockCurveSequence) => UnityYamlSequenceStyle::BlockCurve,
        Some(UnityYamlEmptyArrayKind::FlowSequence) => UnityYamlSequenceStyle::Flow,
        _ => UnityYamlSequenceStyle::Block,
    };
    let mut sequence = UnityYamlNode::sequence(style);
    for (index, item) in items.iter().enumerate() {
        let element_path = indexed_field_path(field_path, index, limits.max_field_path_bytes)?;
        sequence
            .push(convert_value(
                item,
                &element_path,
                depth + 1,
                source,
                hints,
                field_mode,
                unity_version,
                missing_reference_guid,
                limits,
                budget,
                resolve_pointer,
            )?)
            .map_err(UnityYamlObjectConversionError::NodeBuild)?;
    }
    Ok(sequence)
}

fn importer_field_is_excluded(
    field_name: &str,
    field_path: &str,
    hints: &UnityYamlObjectConversionHints,
    limits: UnityYamlObjectConversionLimits,
) -> Result<bool, UnityYamlObjectConversionError> {
    if IMPORTER_FIELDS_TO_SKIP.contains(&field_name)
        || hints.importer_excluded_field_paths.contains(field_path)
    {
        return Ok(true);
    }
    if field_path.contains('[') {
        let canonical_path = canonical_array_path(field_path, limits.max_field_path_bytes)?;
        return Ok(hints
            .importer_excluded_field_paths
            .contains(&canonical_path));
    }
    Ok(false)
}

fn convert_integer_array(
    items: &[SerializedValue],
    field_path: &str,
    depth: usize,
    limits: UnityYamlObjectConversionLimits,
    budget: &mut ConversionBudget,
) -> Result<UnityYamlNode, UnityYamlObjectConversionError> {
    let width = integer_array_width(&items[0]).ok_or_else(|| {
        UnityYamlObjectConversionError::InvalidIntegerArray {
            field_path: field_path.to_owned(),
            element_index: 0,
        }
    })?;
    let hex_length = items
        .len()
        .checked_mul(width)
        .and_then(|bytes| bytes.checked_mul(2))
        .ok_or_else(|| UnityYamlObjectConversionError::HexScalarLimitExceeded {
            requested: usize::MAX,
            limit: limits.max_generated_scalar_bytes,
            field_path: field_path.to_owned(),
        })?;
    if hex_length > limits.max_generated_scalar_bytes {
        return Err(UnityYamlObjectConversionError::HexScalarLimitExceeded {
            requested: hex_length,
            limit: limits.max_generated_scalar_bytes,
            field_path: field_path.to_owned(),
        });
    }
    for (index, value) in items.iter().enumerate() {
        if integer_array_width(value) != Some(width) {
            return Err(UnityYamlObjectConversionError::InvalidIntegerArray {
                field_path: field_path.to_owned(),
                element_index: index,
            });
        }
        let element_path = indexed_field_path(field_path, index, limits.max_field_path_bytes)?;
        budget.visit(depth + 1)?;
        budget.check_field_path(&element_path)?;
    }

    let mut hex = String::new();
    hex.try_reserve_exact(hex_length).map_err(|_| {
        UnityYamlObjectConversionError::AllocationFailed {
            resource: "Unity integer-list hex scalar",
            requested: hex_length,
        }
    })?;
    for value in items {
        let raw = integer_array_raw(value).ok_or_else(|| {
            UnityYamlObjectConversionError::InvalidIntegerArray {
                field_path: field_path.to_owned(),
                element_index: 0,
            }
        })?;
        for byte_index in 0..width {
            let byte = ((raw >> (byte_index * 8)) & 0xff) as u8;
            hex.push(hex_digit(byte >> 4));
            hex.push(hex_digit(byte & 0x0f));
        }
    }
    Ok(UnityYamlNode::plain_string(hex))
}

fn integer_array_width(value: &SerializedValue) -> Option<usize> {
    match value {
        SerializedValue::Bool(_) | SerializedValue::I8(_) | SerializedValue::U8(_) => Some(1),
        SerializedValue::I16(_) | SerializedValue::U16(_) | SerializedValue::Char(_) => Some(2),
        SerializedValue::I32(_) | SerializedValue::U32(_) => Some(4),
        SerializedValue::I64(_) | SerializedValue::U64(_) => Some(8),
        _ => None,
    }
}

fn integer_array_raw(value: &SerializedValue) -> Option<u64> {
    match value {
        SerializedValue::Bool(value) => Some(u64::from(*value)),
        SerializedValue::I8(value) => Some(u64::from(*value as u8)),
        SerializedValue::U8(value) => Some(u64::from(*value)),
        SerializedValue::I16(value) => Some(u64::from(*value as u16)),
        SerializedValue::U16(value) | SerializedValue::Char(value) => Some(u64::from(*value)),
        SerializedValue::I32(value) => Some(u64::from(*value as u32)),
        SerializedValue::U32(value) => Some(u64::from(*value)),
        SerializedValue::I64(value) => Some(*value as u64),
        SerializedValue::U64(value) => Some(*value),
        _ => None,
    }
}

fn encode_byte_hex(
    bytes: &[u8],
    requested: usize,
    field_path: &str,
    limits: UnityYamlObjectConversionLimits,
) -> Result<String, UnityYamlObjectConversionError> {
    if requested > limits.max_generated_scalar_bytes {
        return Err(UnityYamlObjectConversionError::HexScalarLimitExceeded {
            requested,
            limit: limits.max_generated_scalar_bytes,
            field_path: field_path.to_owned(),
        });
    }
    let mut hex = String::new();
    hex.try_reserve_exact(requested).map_err(|_| {
        UnityYamlObjectConversionError::AllocationFailed {
            resource: "Unity byte-array hex scalar",
            requested,
        }
    })?;
    for &byte in bytes {
        hex.push(hex_digit(byte >> 4));
        hex.push(hex_digit(byte & 0x0f));
    }
    Ok(hex)
}

const fn hex_digit(nibble: u8) -> char {
    if nibble < 10 {
        (b'0' + nibble) as char
    } else {
        (b'a' + (nibble - 10)) as char
    }
}

fn is_integer_type_tree_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "bool"
            | "char"
            | "SInt8"
            | "UInt8"
            | "SInt16"
            | "UInt16"
            | "SInt32"
            | "UInt32"
            | "SInt64"
            | "UInt64"
            | "FileSize"
            | "signed char"
            | "unsigned char"
            | "short"
            | "unsigned short"
            | "int"
            | "unsigned int"
            | "long long"
            | "unsigned long long"
            | "Type*"
    )
}

fn is_unsigned_byte_type_tree_type(type_name: &str) -> bool {
    matches!(type_name, "UInt8" | "byte" | "unsigned char")
}

fn is_unsigned_word_type_tree_type(type_name: &str) -> bool {
    matches!(type_name, "UInt32" | "uint" | "unsigned int")
}

fn wildcard_element_path(
    parent: &str,
    limits: UnityYamlObjectConversionLimits,
) -> Result<String, UnityYamlObjectConversionError> {
    let requested = parent.len().checked_add(3).ok_or(
        UnityYamlObjectConversionError::FieldPathLimitExceeded {
            requested: usize::MAX,
            limit: limits.max_field_path_bytes,
        },
    )?;
    if requested > limits.max_field_path_bytes {
        return Err(UnityYamlObjectConversionError::FieldPathLimitExceeded {
            requested,
            limit: limits.max_field_path_bytes,
        });
    }
    let mut path = String::new();
    path.try_reserve_exact(requested).map_err(|_| {
        UnityYamlObjectConversionError::AllocationFailed {
            resource: "Unity YAML array schema path",
            requested,
        }
    })?;
    path.push_str(parent);
    path.push_str("[*]");
    Ok(path)
}

fn is_flow_mapped_path(
    path: &str,
    hints: &UnityYamlObjectConversionHints,
    limits: UnityYamlObjectConversionLimits,
) -> Result<bool, UnityYamlObjectConversionError> {
    if hints.flow_mapped_struct_paths.contains(path) {
        return Ok(true);
    }
    if !path.contains('[') {
        return Ok(false);
    }
    let canonical_path = canonical_array_path(path, limits.max_field_path_bytes)?;
    Ok(hints.flow_mapped_struct_paths.contains(&canonical_path))
}

fn canonical_array_path(
    path: &str,
    max_bytes: usize,
) -> Result<String, UnityYamlObjectConversionError> {
    let mut canonical = String::new();
    canonical.try_reserve_exact(path.len()).map_err(|_| {
        UnityYamlObjectConversionError::AllocationFailed {
            resource: "Unity YAML canonical field path",
            requested: path.len(),
        }
    })?;
    let bytes = path.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'[' {
            let mut digit_end = index + 1;
            while digit_end < bytes.len() && bytes[digit_end].is_ascii_digit() {
                digit_end += 1;
            }
            if digit_end > index + 1 && bytes.get(digit_end) == Some(&b']') {
                canonical.push_str("[*]");
                index = digit_end + 1;
                continue;
            }
        }
        let Some(character) = path[index..].chars().next() else {
            return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                node_index: 0,
                reason: "field path is not valid UTF-8",
            });
        };
        canonical.push(character);
        index += character.len_utf8();
    }
    if canonical.len() > max_bytes {
        return Err(UnityYamlObjectConversionError::FieldPathLimitExceeded {
            requested: canonical.len(),
            limit: max_bytes,
        });
    }
    Ok(canonical)
}

fn child_field_path(
    parent: &str,
    name: &str,
    max_bytes: usize,
) -> Result<String, UnityYamlObjectConversionError> {
    let separator_bytes = usize::from(!parent.is_empty());
    let requested = parent
        .len()
        .checked_add(separator_bytes)
        .and_then(|length| length.checked_add(name.len()))
        .ok_or(UnityYamlObjectConversionError::FieldPathLimitExceeded {
            requested: usize::MAX,
            limit: max_bytes,
        })?;
    if requested > max_bytes {
        return Err(UnityYamlObjectConversionError::FieldPathLimitExceeded {
            requested,
            limit: max_bytes,
        });
    }
    let mut path = String::new();
    path.try_reserve_exact(requested).map_err(|_| {
        UnityYamlObjectConversionError::AllocationFailed {
            resource: "Unity YAML field path",
            requested,
        }
    })?;
    path.push_str(parent);
    if !parent.is_empty() {
        path.push('.');
    }
    path.push_str(name);
    Ok(path)
}

fn indexed_field_path(
    parent: &str,
    index: usize,
    max_bytes: usize,
) -> Result<String, UnityYamlObjectConversionError> {
    let index = index.to_string();
    let requested = parent
        .len()
        .checked_add(index.len())
        .and_then(|length| length.checked_add(2))
        .ok_or(UnityYamlObjectConversionError::FieldPathLimitExceeded {
            requested: usize::MAX,
            limit: max_bytes,
        })?;
    if requested > max_bytes {
        return Err(UnityYamlObjectConversionError::FieldPathLimitExceeded {
            requested,
            limit: max_bytes,
        });
    }
    let mut path = String::new();
    path.try_reserve_exact(requested).map_err(|_| {
        UnityYamlObjectConversionError::AllocationFailed {
            resource: "Unity YAML array field path",
            requested,
        }
    })?;
    path.push_str(parent);
    path.push('[');
    path.push_str(&index);
    path.push(']');
    Ok(path)
}

fn try_copy_string(
    value: &str,
    resource: &'static str,
) -> Result<String, UnityYamlObjectConversionError> {
    let mut copy = String::new();
    copy.try_reserve_exact(value.len()).map_err(|_| {
        UnityYamlObjectConversionError::AllocationFailed {
            resource,
            requested: value.len(),
        }
    })?;
    copy.push_str(value);
    Ok(copy)
}

struct ConversionBudget {
    visited: usize,
    limits: UnityYamlObjectConversionLimits,
}

impl ConversionBudget {
    fn visit(&mut self, depth: usize) -> Result<(), UnityYamlObjectConversionError> {
        if depth > self.limits.max_depth {
            return Err(UnityYamlObjectConversionError::DepthLimitExceeded {
                requested: depth,
                limit: self.limits.max_depth,
            });
        }
        self.visited = self.visited.checked_add(1).ok_or(
            UnityYamlObjectConversionError::ValueLimitExceeded {
                requested: usize::MAX,
                limit: self.limits.max_values,
            },
        )?;
        if self.visited > self.limits.max_values {
            return Err(UnityYamlObjectConversionError::ValueLimitExceeded {
                requested: self.visited,
                limit: self.limits.max_values,
            });
        }
        Ok(())
    }

    fn check_field_path(&self, path: &str) -> Result<(), UnityYamlObjectConversionError> {
        if path.len() > self.limits.max_field_path_bytes {
            return Err(UnityYamlObjectConversionError::FieldPathLimitExceeded {
                requested: path.len(),
                limit: self.limits.max_field_path_bytes,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::pointer_map::{
        ExportPointerGuid, UnityMetaPointer, UnityProjectAssetType, UnresolvedProjectPointer,
    };
    use crate::export::scene_collection::ExportAssetCollectionIdentity;
    use crate::serialized_file::object::SerializedFieldValue;

    fn source() -> ExportAssetIdentity {
        ExportAssetIdentity {
            collection: ExportAssetCollectionIdentity::SerializedFile { file_index: 2 },
            object_index: 7,
            path_id: 90,
            class_id: 1,
        }
    }

    fn field(name: &str, value: SerializedValue) -> SerializedFieldValue {
        SerializedFieldValue {
            name: name.to_owned(),
            value,
        }
    }

    fn type_tree_node(level: u8, type_name: &str, name: &str, meta_flags: u32) -> TypeTreeNode {
        TypeTreeNode {
            level,
            version: 1,
            type_flags: 0,
            type_string_offset: None,
            name_string_offset: None,
            type_name: Some(type_name.to_owned()),
            name: Some(name.to_owned()),
            byte_size: -1,
            index: 0,
            meta_flags,
            reference_type_hash: None,
        }
    }

    fn hash128_type_tree(serialized_version: i32) -> TypeTree {
        let mut nodes = vec![
            type_tree_node(0, "GameObject", "Base", 0),
            type_tree_node(1, "Hash128", "m_Hash", 0),
        ];
        nodes[1].version = serialized_version;
        for index in 0..16 {
            let name = if index < 10 {
                format!("m_Bytes__{index}")
            } else {
                format!("m_Bytes_{index}")
            };
            nodes.push(type_tree_node(2, "UInt8", &name, 0));
        }
        TypeTree {
            nodes,
            string_buffer: Vec::new(),
        }
    }

    fn hash128_value() -> SerializedValue {
        let fields = (0..16)
            .map(|index| {
                let name = if index < 10 {
                    format!("m_Bytes__{index}")
                } else {
                    format!("m_Bytes_{index}")
                };
                field(&name, SerializedValue::U8(index as u8))
            })
            .collect();
        SerializedValue::Struct(vec![field("m_Hash", SerializedValue::Struct(fields))])
    }

    fn guid_type_tree(array_element: bool) -> TypeTree {
        let mut nodes = if array_element {
            vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "vector", "m_Guids", 0),
                type_tree_node(2, "Array", "Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "GUID", "data", 0),
            ]
        } else {
            vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "GUID", "m_Guid", 0),
            ]
        };
        let field_level = if array_element { 4 } else { 2 };
        for index in 0..4 {
            nodes.push(type_tree_node(
                field_level,
                "UInt32",
                &format!("m_Data_{index}_"),
                0,
            ));
        }
        TypeTree {
            nodes,
            string_buffer: Vec::new(),
        }
    }

    fn guid_value_struct() -> SerializedValue {
        let words = [0x0102_0304, 0x1112_1314, 0x2122_2324, 0x3132_3334];
        SerializedValue::Struct(
            words
                .into_iter()
                .enumerate()
                .map(|(index, word)| field(&format!("m_Data_{index}_"), SerializedValue::U32(word)))
                .collect(),
        )
    }

    fn guid_value(array_element: bool) -> SerializedValue {
        if array_element {
            SerializedValue::Struct(vec![field(
                "m_Guids",
                SerializedValue::Array(vec![guid_value_struct()]),
            )])
        } else {
            SerializedValue::Struct(vec![field("m_Guid", guid_value_struct())])
        }
    }

    fn version() -> SceneUnityVersion {
        SceneUnityVersion::parse("2022.3.1f1").unwrap()
    }

    fn convert<F>(
        root: &SerializedValue,
        hints: &HashMap<String, UnityYamlEmptyArrayKind>,
        resolver: F,
    ) -> Result<UnityYamlDocument, UnityYamlObjectConversionError>
    where
        F: FnMut(UnityYamlPPtrRequest<'_>) -> ProjectPointerResolution,
    {
        let hints = UnityYamlObjectConversionHints {
            empty_array_kinds: hints.clone(),
            ..UnityYamlObjectConversionHints::default()
        };
        convert_with_object_hints(root, &hints, resolver)
    }

    fn convert_with_object_hints<F>(
        root: &SerializedValue,
        hints: &UnityYamlObjectConversionHints,
        resolver: F,
    ) -> Result<UnityYamlDocument, UnityYamlObjectConversionError>
    where
        F: FnMut(UnityYamlPPtrRequest<'_>) -> ProjectPointerResolution,
    {
        convert_with_object_field_mode(
            root,
            hints,
            UnityYamlObjectFieldMode::ProjectAsset,
            resolver,
        )
    }

    fn convert_with_object_field_mode<F>(
        root: &SerializedValue,
        hints: &UnityYamlObjectConversionHints,
        field_mode: UnityYamlObjectFieldMode,
        resolver: F,
    ) -> Result<UnityYamlDocument, UnityYamlObjectConversionError>
    where
        F: FnMut(UnityYamlPPtrRequest<'_>) -> ProjectPointerResolution,
    {
        serialized_object_value_to_unity_yaml_document(
            source(),
            "GameObject",
            6,
            1_000_001,
            false,
            root,
            hints,
            field_mode,
            version(),
            None,
            UnityYamlObjectConversionLimits::default(),
            resolver,
        )
    }

    #[test]
    fn importer_yaml_omits_baseline_and_type_tree_ignored_fields() {
        let type_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "UInt32", "m_ObjectHideFlags", 0),
                type_tree_node(1, "SInt32", "m_EditorOnly", TRANSFER_IGNORE_IN_META_FILES),
                type_tree_node(1, "SInt32", "m_Keep", 0),
                type_tree_node(1, "vector", "m_Items", 0),
                type_tree_node(2, "Array", "Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "Item", "data", 0),
                type_tree_node(
                    4,
                    "SInt32",
                    "m_ItemEditorOnly",
                    TRANSFER_IGNORE_IN_META_FILES,
                ),
                type_tree_node(4, "SInt32", "m_ItemKeep", 0),
            ],
            string_buffer: Vec::new(),
        };
        let hints = UnityYamlObjectConversionHints::from_type_tree(
            &type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert!(hints.importer_excluded_field_paths.contains("m_EditorOnly"));
        assert!(hints
            .importer_excluded_field_paths
            .contains("m_Items[*].m_ItemEditorOnly"));

        let value = SerializedValue::Struct(vec![
            field("m_ObjectHideFlags", SerializedValue::U32(1)),
            field("m_EditorOnly", SerializedValue::I32(2)),
            field("m_Keep", SerializedValue::I32(3)),
            field(
                "m_Items",
                SerializedValue::Array(vec![SerializedValue::Struct(vec![
                    field("m_ItemEditorOnly", SerializedValue::I32(4)),
                    field("m_ItemKeep", SerializedValue::I32(5)),
                ])]),
            ),
        ]);

        let full_document = convert_with_object_hints(&value, &hints, |_| {
            unreachable!("the generated fixture has no PPtrs")
        })
        .unwrap();
        let full_text = crate::export::unity_yaml::write_unity_yaml_documents(
            &[full_document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(full_text.contains("m_ObjectHideFlags: 1"));
        assert!(full_text.contains("m_EditorOnly: 2"));
        assert!(full_text.contains("m_ItemEditorOnly: 4"));

        let importer_document = convert_with_object_field_mode(
            &value,
            &hints,
            UnityYamlObjectFieldMode::AssetImporter,
            |_| unreachable!("the generated fixture has no PPtrs"),
        )
        .unwrap();
        let importer_text = crate::export::unity_yaml::write_unity_yaml_documents(
            &[importer_document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(!importer_text.contains("m_ObjectHideFlags"));
        assert!(!importer_text.contains("m_EditorOnly"));
        assert!(!importer_text.contains("m_ItemEditorOnly"));
        assert!(importer_text.contains("m_Keep: 3"));
        assert!(importer_text.contains("m_ItemKeep: 5"));
    }

    #[test]
    fn decoded_gameobject_fields_flatten_base_and_resolve_component_pptrs() {
        let root = SerializedValue::Struct(vec![
            field(
                "Base",
                SerializedValue::Struct(vec![
                    field("m_ObjectHideFlags", SerializedValue::U32(0)),
                    field(
                        "m_Source",
                        SerializedValue::PPtr {
                            file_id: 0,
                            path_id: 0,
                        },
                    ),
                ]),
            ),
            field("m_Layer", SerializedValue::I32(5)),
            field("m_Name", SerializedValue::StringBytes(b"Avatar".to_vec())),
            field(
                "m_Component",
                SerializedValue::Array(vec![SerializedValue::Struct(vec![field(
                    "component",
                    SerializedValue::PPtr {
                        file_id: 2,
                        path_id: 77,
                    },
                )])]),
            ),
        ]);
        let hints = HashMap::new();
        let mut observed = Vec::new();
        let document = convert(&root, &hints, |request| {
            observed.push((
                request.source.object_index,
                request.field_path.to_owned(),
                request.file_id,
                request.path_id,
            ));
            ProjectPointerResolution::Mapped(UnityMetaPointer {
                file_id: 2_345,
                guid: ExportPointerGuid::None,
                asset_type: UnityProjectAssetType::Serialized,
            })
        })
        .unwrap();
        let text = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();

        assert_eq!(
            observed,
            [(7, "m_Component[0].component".to_owned(), 2, 77)]
        );
        assert!(text
            .contains("serializedVersion: 6\n  m_ObjectHideFlags: 0\n  m_Source: {fileID: 0}\n"));
        assert!(text.contains("  m_Component:\n  - component: {fileID: 2345}\n"));
        assert!(text.contains("  m_Name: Avatar\n"));
    }

    #[test]
    fn integer_arrays_use_baseline_reverse_hex_and_empty_arrays_require_schema_hints() {
        let integer_array = SerializedValue::Struct(vec![field(
            "m_Data",
            SerializedValue::Array(vec![
                SerializedValue::I32(0x0102_0304),
                SerializedValue::I32(2),
            ]),
        )]);
        let hints = HashMap::new();
        let document = convert(&integer_array, &hints, |_| {
            unreachable!("integer arrays contain no PPtrs")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(output.contains("m_Data: 0403020102000000"));

        let empty_array = SerializedValue::Struct(vec![field(
            "m_Children",
            SerializedValue::Array(Vec::new()),
        )]);
        assert!(matches!(
            convert(&empty_array, &HashMap::new(), |_| {
                unreachable!("empty arrays contain no PPtrs")
            }),
            Err(UnityYamlObjectConversionError::MissingEmptyArrayHint { field_path })
                if field_path == "m_Children"
        ));
        let hints = HashMap::from([(
            "m_Children".to_owned(),
            UnityYamlEmptyArrayKind::BlockSequence,
        )]);
        let document = convert(&empty_array, &hints, |_| {
            unreachable!("empty arrays contain no PPtrs")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(output.contains("m_Children: []"));
    }

    #[test]
    fn type_tree_hints_drive_flow_structs_and_empty_array_shapes() {
        let type_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "Color", "m_Color", TRANSFER_USING_FLOW_MAPPING_STYLE),
                type_tree_node(2, "float", "r", 0),
                type_tree_node(1, "vector", "m_Colors", 0),
                type_tree_node(2, "Array", "Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "Color", "data", TRANSFER_USING_FLOW_MAPPING_STYLE),
                type_tree_node(4, "float", "r", 0),
                type_tree_node(1, "vector", "m_Bits", 0),
                type_tree_node(2, "Array", "Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "UInt8", "data", 0),
            ],
            string_buffer: Vec::new(),
        };
        let hints = UnityYamlObjectConversionHints::from_type_tree(
            &type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert_eq!(
            hints.empty_array_kinds.get("m_Colors"),
            Some(&UnityYamlEmptyArrayKind::BlockSequence)
        );
        assert_eq!(
            hints.empty_array_kinds.get("m_Bits"),
            Some(&UnityYamlEmptyArrayKind::IntegerHex)
        );
        assert!(hints.flow_mapped_struct_paths.contains("m_Color"));
        assert!(hints.flow_mapped_struct_paths.contains("m_Colors[*]"));

        let populated = SerializedValue::Struct(vec![
            field(
                "m_Color",
                SerializedValue::Struct(vec![field("r", SerializedValue::F32(0.25))]),
            ),
            field(
                "m_Colors",
                SerializedValue::Array(vec![SerializedValue::Struct(vec![field(
                    "r",
                    SerializedValue::F32(0.5),
                )])]),
            ),
        ]);
        let document = convert_with_object_hints(&populated, &hints, |_| {
            unreachable!("the synthetic color values contain no PPtrs")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(output.contains("m_Color: {r: 0.25}"));
        assert!(output.contains("m_Colors:\n  - {r: 0.5}"));

        let empty =
            SerializedValue::Struct(vec![field("m_Colors", SerializedValue::Array(Vec::new()))]);
        let document = convert_with_object_hints(&empty, &hints, |_| {
            unreachable!("the synthetic empty array contains no PPtrs")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(output.contains("m_Colors: []"));
    }

    #[test]
    fn type_tree_hints_apply_root_flow_mapping_and_reject_invalid_levels() {
        let type_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", TRANSFER_USING_FLOW_MAPPING_STYLE),
                type_tree_node(1, "string", "m_Name", 0),
            ],
            string_buffer: Vec::new(),
        };
        let hints = UnityYamlObjectConversionHints::from_type_tree(
            &type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert!(hints.flow_mapped_struct_paths.contains(""));
        let value = SerializedValue::Struct(vec![field(
            "m_Name",
            SerializedValue::StringBytes(b"Avatar".to_vec()),
        )]);
        let document = convert_with_object_hints(&value, &hints, |_| {
            unreachable!("the synthetic name contains no PPtrs")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(output.contains("GameObject: {serializedVersion: 6, m_Name: Avatar}"));

        let invalid = TypeTree {
            nodes: vec![
                type_tree_node(0, "Root", "Base", 0),
                type_tree_node(2, "int", "m_Value", 0),
            ],
            string_buffer: Vec::new(),
        };
        assert!(matches!(
            UnityYamlObjectConversionHints::from_type_tree(
                &invalid,
                version(),
                UnityYamlObjectConversionLimits::default()
            ),
            Err(UnityYamlObjectConversionError::InvalidTypeTree {
                node_index: 1,
                reason: "node level skips its parent"
            })
        ));
        assert!(matches!(
            UnityYamlObjectConversionHints::from_type_tree(
                &type_tree,
                version(),
                UnityYamlObjectConversionLimits {
                    max_type_tree_nodes: 1,
                    ..UnityYamlObjectConversionLimits::default()
                }
            ),
            Err(UnityYamlObjectConversionError::ValueLimitExceeded {
                requested: 2,
                limit: 1
            })
        ));
    }

    #[test]
    fn hash128_uses_baseline_versioned_yaml_forms_and_requires_all_bytes() {
        let value = hash128_value();
        let modern_tree = hash128_type_tree(1);
        let modern_hints = UnityYamlObjectConversionHints::from_type_tree(
            &modern_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert_eq!(
            modern_hints
                .hash128_schemas
                .get("m_Hash")
                .map(|schema| schema.serialized_version),
            Some(2)
        );
        let unity_5_hints = UnityYamlObjectConversionHints::from_type_tree(
            &modern_tree,
            SceneUnityVersion::parse("5.0.0f1").unwrap(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert_eq!(
            unity_5_hints
                .hash128_schemas
                .get("m_Hash")
                .map(|schema| schema.serialized_version),
            Some(2)
        );
        let document = convert_with_object_hints(&value, &modern_hints, |_| {
            unreachable!("Hash128 contains no PPtrs")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(output.contains("serializedVersion: 2"));
        assert!(output.contains("Hash: 03020100070605040b0a09080f0e0d0c"));
        assert!(!output.contains("m_Bytes__0"));

        let old_tree = hash128_type_tree(1);
        let old_hints = UnityYamlObjectConversionHints::from_type_tree(
            &old_tree,
            SceneUnityVersion::parse("4.7.0f1").unwrap(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert_eq!(
            old_hints
                .hash128_schemas
                .get("m_Hash")
                .map(|schema| schema.serialized_version),
            Some(1)
        );
        let document = convert_with_object_hints(&value, &old_hints, |_| {
            unreachable!("Hash128 contains no PPtrs")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(output.contains("serializedVersion: 1"));
        assert!(output.contains("bytes[0]: 0"));
        assert!(output.contains("bytes[15]: 15"));
        assert!(!output.contains("Hash: 03020100070605040b0a09080f0e0d0c"));

        let invalid = SerializedValue::Struct(vec![field(
            "m_Hash",
            SerializedValue::Struct(vec![field("m_Bytes__0", SerializedValue::I8(0))]),
        )]);
        assert!(matches!(
            convert_with_object_hints(&invalid, &modern_hints, |_| {
                unreachable!("Hash128 contains no PPtrs")
            }),
            Err(UnityYamlObjectConversionError::Hash128FieldCount {
                expected: 16,
                actual: 1,
                ..
            })
        ));
        let invalid_bytes = (0..16)
            .map(|index| {
                let name = if index < 10 {
                    format!("m_Bytes__{index}")
                } else {
                    format!("m_Bytes_{index}")
                };
                let value = if index == 7 {
                    SerializedValue::I8(index as i8)
                } else {
                    SerializedValue::U8(index as u8)
                };
                field(&name, value)
            })
            .collect();
        let invalid = SerializedValue::Struct(vec![field(
            "m_Hash",
            SerializedValue::Struct(invalid_bytes),
        )]);
        assert!(matches!(
            convert_with_object_hints(&invalid, &modern_hints, |_| {
                unreachable!("Hash128 contains no PPtrs")
            }),
            Err(UnityYamlObjectConversionError::InvalidHash128Byte { field_index: 7, .. })
        ));
    }

    #[test]
    fn guid_structs_emit_unity_guid_strings_and_validate_the_four_words() {
        let type_tree = guid_type_tree(false);
        let hints = UnityYamlObjectConversionHints::from_type_tree(
            &type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert_eq!(
            hints
                .guid_schemas
                .get("m_Guid")
                .map(|schema| schema.word_field_names.as_slice()),
            Some(
                [
                    "m_Data_0_".to_owned(),
                    "m_Data_1_".to_owned(),
                    "m_Data_2_".to_owned(),
                    "m_Data_3_".to_owned(),
                ]
                .as_slice()
            )
        );
        let document = convert_with_object_hints(&guid_value(false), &hints, |_| {
            unreachable!("GUID contains no PPtrs")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(output.contains("m_Guid: 01020304111213142122232431323334"));

        let missing_word = SerializedValue::Struct(vec![field(
            "m_Guid",
            SerializedValue::Struct(vec![
                field("m_Data_0_", SerializedValue::U32(1)),
                field("m_Data_1_", SerializedValue::U32(2)),
                field("m_Data_2_", SerializedValue::U32(3)),
            ]),
        )]);
        assert!(matches!(
            convert_with_object_hints(&missing_word, &hints, |_| {
                unreachable!("GUID contains no PPtrs")
            }),
            Err(UnityYamlObjectConversionError::GuidFieldCount {
                expected: 4,
                actual: 3,
                ..
            })
        ));

        let wrong_word = SerializedValue::Struct(vec![field(
            "m_Guid",
            SerializedValue::Struct(vec![
                field("m_Data_0_", SerializedValue::U32(1)),
                field("m_Data_1_", SerializedValue::U32(2)),
                field("m_Data_2_", SerializedValue::U64(3)),
                field("m_Data_3_", SerializedValue::U32(4)),
            ]),
        )]);
        assert!(matches!(
            convert_with_object_hints(&wrong_word, &hints, |_| {
                unreachable!("GUID contains no PPtrs")
            }),
            Err(UnityYamlObjectConversionError::InvalidGuidWord { word_index: 2, .. })
        ));
    }

    #[test]
    fn guid_type_tree_hints_resolve_each_array_element_by_schema_path() {
        let type_tree = guid_type_tree(true);
        let hints = UnityYamlObjectConversionHints::from_type_tree(
            &type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert!(hints.guid_schemas.contains_key("m_Guids[*]"));
        let document = convert_with_object_hints(&guid_value(true), &hints, |_| {
            unreachable!("GUID array contains no PPtrs")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(output.contains("m_Guids:\n  - 01020304111213142122232431323334"));
    }

    #[test]
    fn unresolved_pptrs_and_raw_or_invalid_utf8_values_are_explicit_errors() {
        let pptr = SerializedValue::Struct(vec![field(
            "m_Target",
            SerializedValue::PPtr {
                file_id: 2,
                path_id: 88,
            },
        )]);
        assert!(matches!(
            convert(&pptr, &HashMap::new(), |request| {
                ProjectPointerResolution::UnresolvedInput(UnresolvedProjectPointer {
                    source_file_index: 2,
                    source_object_index: request.source.object_index,
                    source_path_id: request.source.path_id,
                    field_path: request.field_path.to_owned(),
                    serialized_file_id: request.file_id,
                    serialized_path_id: request.path_id,
                    expected_class_id: Some(21),
                    resolution: crate::serialized_file::reference_graph::MultiFilePPtrResolution::MissingDependency {
                        external_index: 1,
                        path_id: request.path_id,
                    },
                })
            }),
            Err(UnityYamlObjectConversionError::PointerConversion { field_path, .. })
                if field_path == "m_Target"
        ));

        for (value, expected_path) in [
            (
                SerializedValue::Raw {
                    type_name: "UnknownLeaf".to_owned(),
                    bytes: vec![1, 2],
                },
                "m_Raw",
            ),
            (SerializedValue::StringBytes(vec![0xff]), "m_Text"),
        ] {
            let name = if expected_path == "m_Raw" {
                "m_Raw"
            } else {
                "m_Text"
            };
            let root = SerializedValue::Struct(vec![field(name, value)]);
            let error = convert(&root, &HashMap::new(), |_| {
                unreachable!("the value has no PPtrs")
            })
            .unwrap_err();
            match error {
                UnityYamlObjectConversionError::UnsupportedRawValue { field_path, .. }
                | UnityYamlObjectConversionError::InvalidUtf8String { field_path } => {
                    assert_eq!(field_path, expected_path)
                }
                other => panic!("unexpected error: {other}"),
            }
        }
    }
}
