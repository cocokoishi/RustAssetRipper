//! Bounded conversion from decoded SerializedValue trees to Unity YAML object documents.

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::Write as _;
use std::fmt::{self, Display, Formatter};
use std::str;

use crate::processing::scene_naming::SceneUnityVersion;
use crate::serialized_file::metadata::{TypeTree, TypeTreeNode};
use crate::serialized_file::object::{SerializedFieldValue, SerializedValue};

use super::pointer_map::ProjectPointerResolution;
use super::scene_collection::ExportAssetIdentity;
use super::unity_yaml::{
    format_unity_guid_words, project_pointer_to_unity_yaml, UnityYamlDocument,
    UnityYamlMappingStyle, UnityYamlNode, UnityYamlNodeBuildError, UnityYamlNodeKind,
    UnityYamlScalar, UnityYamlSequenceStyle, UnityYamlWriteError,
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

const STRIPPED_ASSET_ALLOWED_FIELDS: &[&str] = &[
    "m_CorrespondingSourceObject",
    "m_PrefabAsset",
    "m_PrefabInstance",
    "m_PrefabInternal",
    "m_PrefabParentObject",
];

const STRIPPED_MONO_BEHAVIOUR_ALLOWED_FIELDS: &[&str] = &[
    "m_CorrespondingSourceObject",
    "m_EditorClassIdentifier",
    "m_EditorHideFlags",
    "m_Enabled",
    "m_GameObject",
    "m_Name",
    "m_PrefabAsset",
    "m_PrefabInstance",
    "m_PrefabInternal",
    "m_PrefabParentObject",
    "m_Script",
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnityYamlDictionaryRepresentation {
    /// Primitive, string, and GUID keys become YAML mapping keys.
    KeyedMapping,
    /// Older Unity versions serialize complex-key dictionaries as repeated `data` mappings.
    LegacyPairMapping,
    /// Unity 5.4+ uses a sequence of `first`/`second` pair mappings for complex keys.
    PairSequence,
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
    /// Paths emitted as lists even when their TypeTree shape is a dictionary.
    pub force_block_sequence_paths: HashSet<String>,
    /// TypeTree fields marked `IgnoreInMetaFiles`, using [*] for array elements.
    pub importer_excluded_field_paths: HashSet<String>,
    /// Dictionary representation derived from TypeTree `map` and `pair` nodes.
    pub dictionary_representations: HashMap<String, UnityYamlDictionaryRepresentation>,
    pub hash128_schemas: HashMap<String, UnityYamlHash128Schema>,
    pub guid_schemas: HashMap<String, UnityYamlGuidSchema>,
    pub property_name_schemas: HashMap<String, UnityYamlPropertyNameSchema>,
    pub scene_object_identifier_schemas: HashMap<String, UnityYamlSceneObjectIdentifierSchema>,
    pub field_name_aliases: HashMap<String, String>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnityYamlPropertyNameValueKind {
    Int32,
    Utf8String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnityYamlPropertyNameSchema {
    pub value_field_name: String,
    pub value_kind: UnityYamlPropertyNameValueKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnityYamlSceneObjectIdentifierSchema {
    pub target_object_field_name: String,
    pub target_prefab_field_name: String,
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
    InvalidDictionaryPair {
        field_path: String,
        element_index: usize,
    },
    InvalidDictionaryKey {
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
    PropertyNameFieldCount {
        field_path: String,
        expected: usize,
        actual: usize,
    },
    InvalidPropertyNameValue {
        field_path: String,
        field_name: String,
    },
    InvalidSceneObjectIdentifierValue {
        field_path: String,
        field_name: String,
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
            Self::InvalidDictionaryPair {
                field_path,
                element_index,
            } => write!(
                f,
                "Unity dictionary at {field_path:?} has a malformed first/second pair at index {element_index}"
            ),
            Self::InvalidDictionaryKey {
                field_path,
                element_index,
            } => write!(
                f,
                "Unity dictionary at {field_path:?} has a key that cannot be represented as a scalar at index {element_index}"
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
            Self::PropertyNameFieldCount {
                field_path,
                expected,
                actual,
            } => write!(
                f,
                "PropertyName at {field_path:?} has {actual} fields; expected {expected}"
            ),
            Self::InvalidPropertyNameValue {
                field_path,
                field_name,
            } => write!(
                f,
                "PropertyName value field {field_name:?} at {field_path:?} is missing, duplicated, or has an unsupported value"
            ),
            Self::InvalidSceneObjectIdentifierValue {
                field_path,
                field_name,
            } => write!(
                f,
                "SceneObjectIdentifier field {field_name:?} at {field_path:?} is missing, duplicated, or not a signed file ID"
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
        let array_node = self
            .unique_child_index_by_name(field_node_index, "m_Array")?
            .ok_or(UnityYamlObjectConversionError::InvalidTypeTree {
                node_index: field_node_index,
                reason: "ValueArray vector field has no m_Array child",
            })?;
        let data_node = self
            .unique_child_index_by_name(array_node, "m_Data")?
            .ok_or(UnityYamlObjectConversionError::InvalidTypeTree {
                node_index: array_node,
                reason: "ValueArray m_Array node has no m_Data child",
            })?;
        match self.type_name(data_node)? {
            "Vector3Float" => Ok(Some(aliases.0)),
            "Vector4Float" => Ok(Some(aliases.1)),
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

    let scene_object_identifier_schema = if field_mode == UnityYamlObjectFieldMode::ProjectAsset {
        scene_object_identifier_schema_for_path("", hints, limits)?
    } else {
        None
    };
    let mut body = if let Some(schema) = scene_object_identifier_schema {
        convert_scene_object_identifier_value(fields, schema, "", 0, &mut budget)?
    } else if let Some(schema) = hash128_schema_for_path("", hints, limits)? {
        convert_hash128_value(fields, schema, "", 0, limits, &mut budget)?
    } else {
        let mut body = UnityYamlNode::mapping(if is_flow_mapped_path("", hints, limits)? {
            UnityYamlMappingStyle::Flow
        } else {
            UnityYamlMappingStyle::Block
        });
        if !stripped {
            body.insert_field(
                "serializedVersion",
                UnityYamlNode::scalar(UnityYamlScalar::Signed(i64::from(serialized_version))),
            )
            .map_err(UnityYamlObjectConversionError::NodeBuild)?;
        }
        append_struct_fields(
            &mut body,
            fields,
            "",
            1,
            source,
            hints,
            field_mode,
            stripped_field_allowlist(source.class_id, stripped),
            unity_version,
            missing_reference_guid,
            limits,
            &mut budget,
            &mut resolve_pointer,
        )?;
        body
    };
    if stripped {
        retain_stripped_yaml_fields(&mut body, source.class_id)?;
    }
    if source.class_id == 114 {
        move_mapping_field_to_end(&mut body, "m_Structure")?;
    }
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
    stripped_fields: Option<&'static [&'static str]>,
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
                    stripped_fields,
                    unity_version,
                    missing_reference_guid,
                    limits,
                    budget,
                    resolve_pointer,
                )?;
                continue;
            }
        }
        if stripped_fields.is_some_and(|allowed| !allowed.contains(&field.name.as_str())) {
            continue;
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
        let contextual_field_name =
            field_name_alias_for_path(&field_path, hints, limits)?.unwrap_or(field.name.as_str());
        let field_name = baseline_yaml_field_name(contextual_field_name)?;
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
            let scene_object_identifier_schema =
                if field_mode == UnityYamlObjectFieldMode::ProjectAsset {
                    scene_object_identifier_schema_for_path(field_path, hints, limits)?
                } else {
                    None
                };
            if let Some(schema) = scene_object_identifier_schema {
                convert_scene_object_identifier_value(fields, schema, field_path, depth, budget)?
            } else if let Some(schema) = property_name_schema_for_path(field_path, hints, limits)? {
                convert_property_name_value(fields, schema, field_path, depth, budget)?
            } else if let Some(schema) = hash128_schema_for_path(field_path, hints, limits)? {
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
                    None,
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

fn dictionary_representation_for_path(
    field_path: &str,
    hints: &UnityYamlObjectConversionHints,
    limits: UnityYamlObjectConversionLimits,
) -> Result<Option<UnityYamlDictionaryRepresentation>, UnityYamlObjectConversionError> {
    if let Some(representation) = hints.dictionary_representations.get(field_path) {
        return Ok(Some(*representation));
    }
    if !field_path.contains('[') {
        return Ok(None);
    }
    let canonical_path = canonical_array_path(field_path, limits.max_field_path_bytes)?;
    Ok(hints
        .dictionary_representations
        .get(&canonical_path)
        .copied())
}

fn force_block_sequence_for_path(
    field_path: &str,
    hints: &UnityYamlObjectConversionHints,
    limits: UnityYamlObjectConversionLimits,
) -> Result<bool, UnityYamlObjectConversionError> {
    if hints.force_block_sequence_paths.contains(field_path) {
        return Ok(true);
    }
    if !field_path.contains('[') {
        return Ok(false);
    }
    let canonical_path = canonical_array_path(field_path, limits.max_field_path_bytes)?;
    Ok(hints.force_block_sequence_paths.contains(&canonical_path))
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

fn property_name_schema_for_path<'a>(
    field_path: &str,
    hints: &'a UnityYamlObjectConversionHints,
    limits: UnityYamlObjectConversionLimits,
) -> Result<Option<&'a UnityYamlPropertyNameSchema>, UnityYamlObjectConversionError> {
    if let Some(schema) = hints.property_name_schemas.get(field_path) {
        return Ok(Some(schema));
    }
    if !field_path.contains('[') {
        return Ok(None);
    }
    let canonical_path = canonical_array_path(field_path, limits.max_field_path_bytes)?;
    Ok(hints.property_name_schemas.get(&canonical_path))
}

fn field_name_alias_for_path<'a>(
    field_path: &str,
    hints: &'a UnityYamlObjectConversionHints,
    limits: UnityYamlObjectConversionLimits,
) -> Result<Option<&'a str>, UnityYamlObjectConversionError> {
    if let Some(alias) = hints.field_name_aliases.get(field_path) {
        return Ok(Some(alias));
    }
    if !field_path.contains('[') {
        return Ok(None);
    }
    let canonical_path = canonical_array_path(field_path, limits.max_field_path_bytes)?;
    Ok(hints
        .field_name_aliases
        .get(&canonical_path)
        .map(String::as_str))
}

fn scene_object_identifier_schema_for_path<'a>(
    field_path: &str,
    hints: &'a UnityYamlObjectConversionHints,
    limits: UnityYamlObjectConversionLimits,
) -> Result<Option<&'a UnityYamlSceneObjectIdentifierSchema>, UnityYamlObjectConversionError> {
    if let Some(schema) = hints.scene_object_identifier_schemas.get(field_path) {
        return Ok(Some(schema));
    }
    if !field_path.contains('[') {
        return Ok(None);
    }
    let canonical_path = canonical_array_path(field_path, limits.max_field_path_bytes)?;
    Ok(hints.scene_object_identifier_schemas.get(&canonical_path))
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

fn convert_property_name_value(
    fields: &[SerializedFieldValue],
    schema: &UnityYamlPropertyNameSchema,
    field_path: &str,
    depth: usize,
    budget: &mut ConversionBudget,
) -> Result<UnityYamlNode, UnityYamlObjectConversionError> {
    if fields.len() != 1 {
        return Err(UnityYamlObjectConversionError::PropertyNameFieldCount {
            field_path: field_path.to_owned(),
            expected: 1,
            actual: fields.len(),
        });
    }

    budget.visit(depth + 1)?;
    let mut matching_value = None;
    for field in fields
        .iter()
        .filter(|field| field.name == schema.value_field_name)
    {
        if matching_value.is_some() {
            return Err(UnityYamlObjectConversionError::InvalidPropertyNameValue {
                field_path: field_path.to_owned(),
                field_name: schema.value_field_name.clone(),
            });
        }
        matching_value = Some(&field.value);
    }

    match (schema.value_kind, matching_value) {
        (UnityYamlPropertyNameValueKind::Int32, Some(SerializedValue::I32(value))) => {
            Ok(UnityYamlNode::string(value.to_string()))
        }
        (UnityYamlPropertyNameValueKind::Utf8String, Some(SerializedValue::StringBytes(bytes))) => {
            let value = str::from_utf8(bytes).map_err(|_| {
                UnityYamlObjectConversionError::InvalidUtf8String {
                    field_path: field_path.to_owned(),
                }
            })?;
            Ok(UnityYamlNode::string(try_copy_string(
                value,
                "Unity PropertyName string",
            )?))
        }
        _ => Err(UnityYamlObjectConversionError::InvalidPropertyNameValue {
            field_path: field_path.to_owned(),
            field_name: schema.value_field_name.clone(),
        }),
    }
}

fn convert_scene_object_identifier_value(
    fields: &[SerializedFieldValue],
    schema: &UnityYamlSceneObjectIdentifierSchema,
    field_path: &str,
    depth: usize,
    budget: &mut ConversionBudget,
) -> Result<UnityYamlNode, UnityYamlObjectConversionError> {
    let target_object = scene_object_identifier_file_id(
        fields,
        &schema.target_object_field_name,
        field_path,
        depth,
        budget,
    )?;
    let target_prefab = scene_object_identifier_file_id(
        fields,
        &schema.target_prefab_field_name,
        field_path,
        depth,
        budget,
    )?;
    let mut mapping = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    mapping
        .insert_field(
            "targetObject",
            UnityYamlNode::scalar(UnityYamlScalar::Signed(target_object)),
        )
        .map_err(UnityYamlObjectConversionError::NodeBuild)?;
    mapping
        .insert_field(
            "targetPrefab",
            UnityYamlNode::scalar(UnityYamlScalar::Signed(target_prefab)),
        )
        .map_err(UnityYamlObjectConversionError::NodeBuild)?;
    Ok(mapping)
}

fn scene_object_identifier_file_id(
    fields: &[SerializedFieldValue],
    field_name: &str,
    field_path: &str,
    depth: usize,
    budget: &mut ConversionBudget,
) -> Result<i64, UnityYamlObjectConversionError> {
    let mut matching_value = None;
    for field in fields.iter().filter(|field| field.name == field_name) {
        if matching_value.is_some() {
            return Err(
                UnityYamlObjectConversionError::InvalidSceneObjectIdentifierValue {
                    field_path: field_path.to_owned(),
                    field_name: field_name.to_owned(),
                },
            );
        }
        matching_value = Some(&field.value);
    }
    budget.visit(depth + 1)?;
    match matching_value {
        Some(SerializedValue::I64(value)) => Ok(*value),
        _ => Err(
            UnityYamlObjectConversionError::InvalidSceneObjectIdentifierValue {
                field_path: field_path.to_owned(),
                field_name: field_name.to_owned(),
            },
        ),
    }
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
    if !force_block_sequence_for_path(field_path, hints, limits)? {
        if let Some(representation) = dictionary_representation_for_path(field_path, hints, limits)?
        {
            return convert_dictionary(
                items,
                field_path,
                depth,
                representation,
                source,
                hints,
                field_mode,
                unity_version,
                missing_reference_guid,
                limits,
                budget,
                resolve_pointer,
            );
        }
    }

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

#[allow(clippy::too_many_arguments)]
fn convert_dictionary<F>(
    items: &[SerializedValue],
    field_path: &str,
    depth: usize,
    representation: UnityYamlDictionaryRepresentation,
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
    let mut output = match representation {
        UnityYamlDictionaryRepresentation::KeyedMapping
        | UnityYamlDictionaryRepresentation::LegacyPairMapping => {
            UnityYamlNode::mapping(UnityYamlMappingStyle::Block)
        }
        UnityYamlDictionaryRepresentation::PairSequence => {
            UnityYamlNode::sequence(UnityYamlSequenceStyle::BlockCurve)
        }
    };

    for (element_index, item) in items.iter().enumerate() {
        let element_path =
            indexed_field_path(field_path, element_index, limits.max_field_path_bytes)?;
        let SerializedValue::Struct(pair_fields) = item else {
            return Err(UnityYamlObjectConversionError::InvalidDictionaryPair {
                field_path: field_path.to_owned(),
                element_index,
            });
        };
        let mut first = None;
        let mut second = None;
        for pair_field in pair_fields {
            match pair_field.name.as_str() {
                "first" if first.is_none() => first = Some(&pair_field.value),
                "second" if second.is_none() => second = Some(&pair_field.value),
                "first" | "second" => {
                    return Err(UnityYamlObjectConversionError::InvalidDictionaryPair {
                        field_path: field_path.to_owned(),
                        element_index,
                    });
                }
                _ => {}
            }
        }
        if pair_fields.len() != 2 || first.is_none() || second.is_none() {
            return Err(UnityYamlObjectConversionError::InvalidDictionaryPair {
                field_path: field_path.to_owned(),
                element_index,
            });
        }
        budget.visit(depth + 1)?;
        let Some(first) = first else {
            return Err(UnityYamlObjectConversionError::InvalidDictionaryPair {
                field_path: field_path.to_owned(),
                element_index,
            });
        };
        let Some(second) = second else {
            return Err(UnityYamlObjectConversionError::InvalidDictionaryPair {
                field_path: field_path.to_owned(),
                element_index,
            });
        };

        let first_path = child_field_path(&element_path, "first", limits.max_field_path_bytes)?;
        let second_path = child_field_path(&element_path, "second", limits.max_field_path_bytes)?;
        let first_node = if representation == UnityYamlDictionaryRepresentation::KeyedMapping {
            if let Some(key) = primitive_dictionary_key(first, field_path, element_index)? {
                budget.visit(depth + 2)?;
                key
            } else {
                convert_value(
                    first,
                    &first_path,
                    depth + 2,
                    source,
                    hints,
                    field_mode,
                    unity_version,
                    missing_reference_guid,
                    limits,
                    budget,
                    resolve_pointer,
                )?
            }
        } else {
            convert_value(
                first,
                &first_path,
                depth + 2,
                source,
                hints,
                field_mode,
                unity_version,
                missing_reference_guid,
                limits,
                budget,
                resolve_pointer,
            )?
        };
        let second_node = convert_value(
            second,
            &second_path,
            depth + 2,
            source,
            hints,
            field_mode,
            unity_version,
            missing_reference_guid,
            limits,
            budget,
            resolve_pointer,
        )?;

        match representation {
            UnityYamlDictionaryRepresentation::KeyedMapping => {
                if !matches!(&first_node.kind, UnityYamlNodeKind::Scalar(_)) {
                    return Err(UnityYamlObjectConversionError::InvalidDictionaryKey {
                        field_path: field_path.to_owned(),
                        element_index,
                    });
                }
                output
                    .insert(first_node, second_node)
                    .map_err(UnityYamlObjectConversionError::NodeBuild)?;
            }
            UnityYamlDictionaryRepresentation::LegacyPairMapping
            | UnityYamlDictionaryRepresentation::PairSequence => {
                let mut pair = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
                pair.insert_field("first", first_node)
                    .map_err(UnityYamlObjectConversionError::NodeBuild)?;
                pair.insert_field("second", second_node)
                    .map_err(UnityYamlObjectConversionError::NodeBuild)?;
                if representation == UnityYamlDictionaryRepresentation::LegacyPairMapping {
                    output
                        .insert_field("data", pair)
                        .map_err(UnityYamlObjectConversionError::NodeBuild)?;
                } else {
                    output
                        .push(pair)
                        .map_err(UnityYamlObjectConversionError::NodeBuild)?;
                }
            }
        }
    }

    Ok(output)
}

fn primitive_dictionary_key(
    value: &SerializedValue,
    field_path: &str,
    element_index: usize,
) -> Result<Option<UnityYamlNode>, UnityYamlObjectConversionError> {
    let key = match value {
        SerializedValue::Bool(value) => Some(try_copy_string(
            if *value { "True" } else { "False" },
            "Unity dictionary boolean key",
        )?),
        SerializedValue::Char(value) => {
            let Some(character) = char::from_u32(u32::from(*value)) else {
                return Err(UnityYamlObjectConversionError::InvalidDictionaryKey {
                    field_path: field_path.to_owned(),
                    element_index,
                });
            };
            let mut key = String::new();
            key.try_reserve_exact(character.len_utf8()).map_err(|_| {
                UnityYamlObjectConversionError::AllocationFailed {
                    resource: "Unity dictionary character key",
                    requested: character.len_utf8(),
                }
            })?;
            key.push(character);
            Some(key)
        }
        SerializedValue::I8(value) => Some(format_dictionary_numeric_key(
            *value,
            field_path,
            element_index,
        )?),
        SerializedValue::U8(value) => Some(format_dictionary_numeric_key(
            *value,
            field_path,
            element_index,
        )?),
        SerializedValue::I16(value) => Some(format_dictionary_numeric_key(
            *value,
            field_path,
            element_index,
        )?),
        SerializedValue::U16(value) => Some(format_dictionary_numeric_key(
            *value,
            field_path,
            element_index,
        )?),
        SerializedValue::I32(value) => Some(format_dictionary_numeric_key(
            *value,
            field_path,
            element_index,
        )?),
        SerializedValue::U32(value) => Some(format_dictionary_numeric_key(
            *value,
            field_path,
            element_index,
        )?),
        SerializedValue::I64(value) => Some(format_dictionary_numeric_key(
            *value,
            field_path,
            element_index,
        )?),
        SerializedValue::U64(value) => Some(format_dictionary_numeric_key(
            *value,
            field_path,
            element_index,
        )?),
        SerializedValue::F32(value) => Some(format_dictionary_float32_key(
            *value,
            field_path,
            element_index,
        )?),
        SerializedValue::F64(value) => Some(format_dictionary_float64_key(
            *value,
            field_path,
            element_index,
        )?),
        _ => None,
    };
    Ok(key.map(UnityYamlNode::string))
}

fn format_dictionary_numeric_key<T: Display>(
    value: T,
    field_path: &str,
    element_index: usize,
) -> Result<String, UnityYamlObjectConversionError> {
    let mut key = String::new();
    key.try_reserve_exact(64)
        .map_err(|_| UnityYamlObjectConversionError::AllocationFailed {
            resource: "Unity dictionary numeric key",
            requested: 64,
        })?;
    write!(&mut key, "{value}").map_err(|_| {
        UnityYamlObjectConversionError::InvalidDictionaryKey {
            field_path: field_path.to_owned(),
            element_index,
        }
    })?;
    Ok(key)
}

fn format_dictionary_float32_key(
    value: f32,
    field_path: &str,
    element_index: usize,
) -> Result<String, UnityYamlObjectConversionError> {
    if value.is_nan() {
        try_copy_string("NaN", "Unity dictionary floating-point key")
    } else if value == f32::INFINITY {
        try_copy_string("Infinity", "Unity dictionary floating-point key")
    } else if value == f32::NEG_INFINITY {
        try_copy_string("-Infinity", "Unity dictionary floating-point key")
    } else {
        format_dictionary_numeric_key(value, field_path, element_index)
    }
}

fn format_dictionary_float64_key(
    value: f64,
    field_path: &str,
    element_index: usize,
) -> Result<String, UnityYamlObjectConversionError> {
    if value.is_nan() {
        try_copy_string("NaN", "Unity dictionary floating-point key")
    } else if value == f64::INFINITY {
        try_copy_string("Infinity", "Unity dictionary floating-point key")
    } else if value == f64::NEG_INFINITY {
        try_copy_string("-Infinity", "Unity dictionary floating-point key")
    } else {
        format_dictionary_numeric_key(value, field_path, element_index)
    }
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

fn baseline_type_scoped_field_aliases(type_name: &str) -> &'static [(&'static str, &'static str)] {
    const ANIMATION_CLIP_ALIASES: &[(&str, &str)] =
        &[("m_AnimationClipSettings", "m_MuscleClipInfo")];
    const GENERIC_BINDING_ALIASES: &[(&str, &str)] = &[("m_TypeID", "m_ClassID")];
    const ANIMATOR_CONTROLLER_ALIASES: &[(&str, &str)] = &[
        ("m_AnimatorEvents", "m_AnimatorParameters"),
        ("m_Layers", "m_AnimatorLayers"),
    ];
    const ANIMATOR_CONTROLLER_LAYER_ALIASES: &[(&str, &str)] = &[("m_HumanMask", "m_Mask")];
    const BLEND_TREE_ALIASES: &[(&str, &str)] = &[
        ("m_BlendEvent", "m_BlendParameter"),
        ("m_BlendEventY", "m_BlendParameterY"),
    ];
    const LIGHTMAP_SETTINGS_ALIASES: &[(&str, &str)] =
        &[("m_LightmapSnapshot", "m_LightingDataAsset")];
    const TEXTURE_3D_ALIASES: &[(&str, &str)] = &[
        ("m_DataSize", "m_CompleteImageSize"),
        ("m_ImageCount", "m_Depth"),
    ];
    const MESH_RENDERER_ALIASES: &[(&str, &str)] = &[("m_StitchSeams", "m_StitchLightmapSeams")];
    const PREFAB_ALIASES: &[(&str, &str)] = &[
        ("m_IsPrefabParent", "m_IsPrefabAsset"),
        ("m_ParentPrefab", "m_SourcePrefab"),
    ];
    const LAYER_CONSTANT_ALIASES: &[(&str, &str)] = &[(
        "m_StateMachineMotionSetIndex",
        "m_StateMachineSynchronizedLayerIndex",
    )];
    const CONTROLLER_CONSTANT_ALIASES: &[(&str, &str)] = &[("m_HumanLayerArray", "m_LayerArray")];
    const STATE_MACHINE_CONSTANT_ALIASES: &[(&str, &str)] =
        &[("m_MotionSetCount", "m_SynchronizedLayerCount")];
    const BUILD_SETTINGS_ALIASES: &[(&str, &str)] = &[("m_Levels", "m_Scenes")];
    const HEIGHTMAP_ALIASES: &[(&str, &str)] = &[
        (
            "m_EnableSurfaceMaskTextureCompression",
            "m_EnableHolesTextureCompression",
        ),
        ("m_SurfaceMask", "m_Holes"),
        ("m_SurfaceMaskLOD", "m_HolesLOD"),
    ];
    const GRAPHICS_SETTINGS_ALIASES: &[(&str, &str)] = &[
        ("m_BuildTargetShaderSettings", "m_TierSettings"),
        ("m_AlbedoSwatchInfos", "m_AlbedoSwitchInfos"),
    ];
    const TEXTURE_IMPORTER_ALIASES: &[(&str, &str)] =
        &[("m_BuildTargetSettings", "m_PlatformSettings")];
    const BUILD_TARGET_SHADER_SETTINGS_ALIASES: &[(&str, &str)] =
        &[("m_ShaderSettings", "m_Settings")];
    const BINDING_ALIASES: &[(&str, &str)] = &[("m_Set", "m_EncodedData")];
    const CONSTANT_BUFFER_PARAMETER_ALIASES: &[(&str, &str)] =
        &[("m_Index", "m_OffsetInConstantBuffer")];
    const VERTEX_DATA_ALIASES: &[(&str, &str)] = &[("m_DataSize", "m_Data")];
    const COMPUTE_SHADER_VARIANT_ALIASES: &[(&str, &str)] = &[("m_Kernels", "m_KernelParents")];
    const SHADER_ALIASES: &[(&str, &str)] = &[("m_SubProgramBlob", "m_CompressedBlob")];
    const GAME_OBJECT_ALIASES: &[(&str, &str)] = &[("m_Component", "m_Components")];

    match type_name {
        "AnimationClip" => ANIMATION_CLIP_ALIASES,
        "GenericBinding" => GENERIC_BINDING_ALIASES,
        "AnimatorController" => ANIMATOR_CONTROLLER_ALIASES,
        "AnimatorControllerLayer" => ANIMATOR_CONTROLLER_LAYER_ALIASES,
        "BlendTree" => BLEND_TREE_ALIASES,
        "LightmapSettings" => LIGHTMAP_SETTINGS_ALIASES,
        "Texture3D" => TEXTURE_3D_ALIASES,
        "MeshRenderer" => MESH_RENDERER_ALIASES,
        "Prefab" => PREFAB_ALIASES,
        "HumanLayerConstant" | "LayerConstant" => LAYER_CONSTANT_ALIASES,
        "ControllerConstant" => CONTROLLER_CONSTANT_ALIASES,
        "StateMachineConstant" => STATE_MACHINE_CONSTANT_ALIASES,
        "BuildSettings" => BUILD_SETTINGS_ALIASES,
        "Heightmap" => HEIGHTMAP_ALIASES,
        "GraphicsSettings" => GRAPHICS_SETTINGS_ALIASES,
        "TextureImporter" => TEXTURE_IMPORTER_ALIASES,
        "BuildTargetShaderSettings" => BUILD_TARGET_SHADER_SETTINGS_ALIASES,
        "Binding" => BINDING_ALIASES,
        "MatrixParameter" | "StructParameter" | "VectorParameter" => {
            CONSTANT_BUFFER_PARAMETER_ALIASES
        }
        "VertexData" => VERTEX_DATA_ALIASES,
        "ComputeShaderPlatformVariant" => COMPUTE_SHADER_VARIANT_ALIASES,
        "Shader" => SHADER_ALIASES,
        "GameObject" => GAME_OBJECT_ALIASES,
        _ => &[],
    }
}

fn has_conditional_field_aliases(type_name: &str) -> bool {
    matches!(
        type_name,
        "ParticleSystem" | "ShapeModule" | "ClipMuscleConstant" | "Xform" | "ValueArray" | "Mesh"
    )
}

fn baseline_type_scoped_field_alias(
    parent_type: &str,
    field_name: &str,
    field_type: &str,
) -> Option<&'static str> {
    match (parent_type, field_name) {
        ("ClipMuscleConstant", "m_AverageSpeed") => match field_type {
            "Vector4Float" => Some("m_AverageSpeed4"),
            "Vector3Float" => Some("m_AverageSpeed3"),
            _ => None,
        },
        ("Xform", "m_T") => match field_type {
            "Vector4Float" => Some("m_T4"),
            "Vector3Float" => Some("m_T3"),
            _ => None,
        },
        ("Xform", "m_S") => match field_type {
            "Vector4Float" => Some("m_S4"),
            "Vector3Float" => Some("m_S3"),
            _ => None,
        },
        ("ParticleSystem", "m_StartDelay") => Some(if field_type == "float" {
            "m_StartDelay_Single"
        } else {
            "m_StartDelay_MinMaxCurve"
        }),
        ("ShapeModule", "m_Arc") if field_type != "float" => Some("m_ArcParameter"),
        ("ShapeModule", "m_Radius") if field_type != "float" => Some("m_RadiusParameter"),
        ("Mesh", "m_Shapes") if field_type == "vector" => Some("m_ShapesList"),
        _ => baseline_type_scoped_field_aliases(parent_type)
            .iter()
            .find(|(source_name, _)| *source_name == field_name)
            .map(|(_, alias)| *alias),
    }
}

fn unsupported_contextual_field_alias_reason(
    parent_type: &str,
    field_name: &str,
    field_type: &str,
) -> Option<&'static str> {
    let recognized_type = match (parent_type, field_name) {
        ("ClipMuscleConstant", "m_AverageSpeed") => {
            matches!(field_type, "Vector4Float" | "Vector3Float")
        }
        ("Xform", "m_T" | "m_S") => matches!(field_type, "Vector4Float" | "Vector3Float"),
        _ => true,
    };
    (!recognized_type).then_some("unsupported child TypeTree type for baseline field alias")
}

fn baseline_yaml_field_name(field_name: &str) -> Result<String, UnityYamlObjectConversionError> {
    let replacement = match field_name {
        "m_Image_data" => Some("m_ImageData"),
        "m_TextureFormat" => Some("m_Format"),
        "m_TextureDimension" => Some("m_Dimension"),
        "m_ObjectHideFlags" => Some("m_HideFlags"),
        "m_Val" => Some("m_Value"),
        _ => None,
    };
    if let Some(replacement) = replacement {
        return try_copy_string(replacement, "Unity baseline YAML field name");
    }

    for (source_prefix, output_prefix) in [
        ("m_Dst", "m_Destination"),
        ("m_Dest", "m_Destination"),
        ("m_Src", "m_Source"),
    ] {
        let Some(suffix) = field_name.strip_prefix(source_prefix) else {
            continue;
        };
        if !baseline_prefix_has_uppercase_initial(suffix) {
            continue;
        }
        let requested = output_prefix.len().checked_add(suffix.len()).ok_or(
            UnityYamlObjectConversionError::AllocationFailed {
                resource: "Unity baseline YAML field name",
                requested: usize::MAX,
            },
        )?;
        let mut output_name = String::new();
        output_name.try_reserve_exact(requested).map_err(|_| {
            UnityYamlObjectConversionError::AllocationFailed {
                resource: "Unity baseline YAML field name",
                requested,
            }
        })?;
        output_name.push_str(output_prefix);
        output_name.push_str(suffix);
        return Ok(output_name);
    }

    try_copy_string(field_name, "Unity baseline YAML field name")
}

fn baseline_prefix_has_uppercase_initial(suffix: &str) -> bool {
    suffix
        .encode_utf16()
        .next()
        .and_then(|unit| char::from_u32(u32::from(unit)))
        .is_some_and(char::is_uppercase)
}

fn stripped_field_allowlist(class_id: i32, stripped: bool) -> Option<&'static [&'static str]> {
    stripped.then_some(stripped_fields_for_class_id(class_id))
}

fn stripped_fields_for_class_id(class_id: i32) -> &'static [&'static str] {
    if class_id == 114 {
        STRIPPED_MONO_BEHAVIOUR_ALLOWED_FIELDS
    } else {
        STRIPPED_ASSET_ALLOWED_FIELDS
    }
}

fn retain_stripped_yaml_fields(
    body: &mut UnityYamlNode,
    class_id: i32,
) -> Result<(), UnityYamlObjectConversionError> {
    let allowed_fields = stripped_fields_for_class_id(class_id);
    let UnityYamlNodeKind::Mapping { entries, .. } = &mut body.kind else {
        return Err(UnityYamlObjectConversionError::NodeBuild(
            UnityYamlNodeBuildError::NotAMapping,
        ));
    };
    entries.retain(|(key, _)| match &key.kind {
        UnityYamlNodeKind::Scalar(
            UnityYamlScalar::String(name) | UnityYamlScalar::PlainString(name),
        ) => allowed_fields.contains(&name.as_str()),
        _ => false,
    });
    Ok(())
}

fn move_mapping_field_to_end(
    mapping: &mut UnityYamlNode,
    field_name: &str,
) -> Result<(), UnityYamlObjectConversionError> {
    let UnityYamlNodeKind::Mapping { entries, .. } = &mut mapping.kind else {
        return Err(UnityYamlObjectConversionError::NodeBuild(
            UnityYamlNodeBuildError::NotAMapping,
        ));
    };
    let field_index = entries.iter().position(|(key, _)| {
        matches!(
            &key.kind,
            UnityYamlNodeKind::Scalar(
                UnityYamlScalar::String(name) | UnityYamlScalar::PlainString(name)
            ) if name == field_name
        )
    });
    if let Some(field_index) = field_index {
        let field = entries.remove(field_index);
        entries.push(field);
    }
    Ok(())
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

fn is_signed_file_id_type_tree_type(type_name: &str) -> bool {
    matches!(type_name, "SInt64" | "long long")
}

fn is_valid_dictionary_key_type_tree_type(type_name: &str) -> bool {
    type_name == "GUID"
        || matches!(type_name, "string" | "Utf8String" | "float" | "double")
        || is_integer_type_tree_type(type_name)
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
        source_with_class_id(1)
    }

    fn source_with_class_id(class_id: i32) -> ExportAssetIdentity {
        ExportAssetIdentity {
            collection: ExportAssetCollectionIdentity::SerializedFile { file_index: 2 },
            object_index: 7,
            path_id: 90,
            class_id,
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

    fn property_name_type_tree() -> TypeTree {
        TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                // Raw serialized TypeTrees encode PropertyName as a string wrapper with one
                // integer or Utf8String child; the baseline generator names it PropertyName.
                type_tree_node(1, "string", "m_IntegerId", 0),
                type_tree_node(2, "SInt32", "Id_Int32", 0),
                type_tree_node(1, "string", "m_StringId", 0),
                type_tree_node(2, "Utf8String", "Id_Utf8String", 0),
                type_tree_node(3, "Array", "Array", 0),
                type_tree_node(4, "SInt32", "size", 0),
                type_tree_node(4, "char", "data", 0),
            ],
            string_buffer: Vec::new(),
        }
    }

    fn scene_object_identifier_type_tree(array_element: bool) -> TypeTree {
        let mut nodes = if array_element {
            vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "vector", "m_Identifiers", 0),
                type_tree_node(2, "Array", "Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "SceneObjectIdentifier", "data", 0),
            ]
        } else {
            vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "SceneObjectIdentifier", "m_Identifier", 0),
            ]
        };
        let field_level = if array_element { 4 } else { 2 };
        nodes.push(type_tree_node(field_level, "SInt64", "TargetObject", 0));
        nodes.push(type_tree_node(field_level, "SInt64", "TargetPrefab", 0));
        TypeTree {
            nodes,
            string_buffer: Vec::new(),
        }
    }

    fn scene_object_identifier_value(target_object: i64, target_prefab: i64) -> SerializedValue {
        SerializedValue::Struct(vec![field(
            "m_Identifier",
            SerializedValue::Struct(vec![
                field("TargetObject", SerializedValue::I64(target_object)),
                field("TargetPrefab", SerializedValue::I64(target_prefab)),
            ]),
        )])
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

    fn object_field<'a>(document: &'a UnityYamlDocument, field_name: &str) -> &'a UnityYamlNode {
        let UnityYamlNodeKind::Mapping {
            entries: root_entries,
            ..
        } = &document.root.kind
        else {
            unreachable!("object root is a mapping")
        };
        let (_, body) = root_entries.first().expect("object body exists");
        let UnityYamlNodeKind::Mapping { entries, .. } = &body.kind else {
            unreachable!("object body is a mapping")
        };
        entries
            .iter()
            .find_map(|(key, value)| match &key.kind {
                UnityYamlNodeKind::Scalar(
                    UnityYamlScalar::PlainString(name) | UnityYamlScalar::String(name),
                ) if name == field_name => Some(value),
                _ => None,
            })
            .expect("object field exists")
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
        convert_object_with_options(source(), root, hints, field_mode, false, resolver)
    }

    fn convert_object_with_options<F>(
        source: ExportAssetIdentity,
        root: &SerializedValue,
        hints: &UnityYamlObjectConversionHints,
        field_mode: UnityYamlObjectFieldMode,
        stripped: bool,
        resolver: F,
    ) -> Result<UnityYamlDocument, UnityYamlObjectConversionError>
    where
        F: FnMut(UnityYamlPPtrRequest<'_>) -> ProjectPointerResolution,
    {
        let class_name = match source.class_id {
            74 => "AnimationClip",
            114 => "MonoBehaviour",
            _ => "GameObject",
        };
        serialized_object_value_to_unity_yaml_document(
            source,
            class_name,
            6,
            1_000_001,
            stripped,
            root,
            hints,
            field_mode,
            version(),
            None,
            UnityYamlObjectConversionLimits::default(),
            resolver,
        )
    }

    fn null_pptr() -> SerializedValue {
        SerializedValue::PPtr {
            file_id: 0,
            path_id: 0,
        }
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
        assert!(full_text.contains("m_HideFlags: 1"));
        assert!(!full_text.contains("m_ObjectHideFlags"));
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
    fn baseline_global_and_prefix_field_names_are_emitted_as_yaml_aliases() {
        let root = SerializedValue::Struct(vec![
            field("m_Image_data", SerializedValue::U8(7)),
            field("m_TextureFormat", SerializedValue::I32(10)),
            field("m_TextureDimension", SerializedValue::I32(2)),
            field("m_ObjectHideFlags", SerializedValue::U32(1)),
            field("m_Val", SerializedValue::I32(-4)),
            field("m_DstPosition", SerializedValue::I32(11)),
            field("m_DestRotation", SerializedValue::I32(12)),
            field("m_SrcScale", SerializedValue::I32(13)),
            field("m_DstÉtage", SerializedValue::I32(17)),
            field("m_Dst𝐀", SerializedValue::I32(19)),
            field("m_Dstposition", SerializedValue::I32(14)),
            field("m_Dst", SerializedValue::I32(15)),
            field("m_Dest", SerializedValue::I32(16)),
            field("m_Destination", SerializedValue::I32(18)),
        ]);
        let document =
            convert_with_object_hints(&root, &UnityYamlObjectConversionHints::default(), |_| {
                unreachable!("the generated fixture has no PPtrs")
            })
            .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();

        assert!(output.contains(concat!(
            "m_ImageData: 7\n  m_Format: 10\n  m_Dimension: 2\n",
            "  m_HideFlags: 1\n  m_Value: -4\n",
            "  m_DestinationPosition: 11\n  m_DestinationRotation: 12\n",
            "  m_SourceScale: 13\n  m_DestinationÉtage: 17\n  m_Dst𝐀: 19\n",
            "  m_Dstposition: 14\n  m_Dst: 15\n  m_Dest: 16\n",
            "  m_Destination: 18"
        )));
        for serialized_name in [
            "m_Image_data",
            "m_TextureFormat",
            "m_TextureDimension",
            "m_ObjectHideFlags",
            "m_Val",
            "m_DstPosition",
            "m_DestRotation",
            "m_SrcScale",
            "m_DstÉtage",
        ] {
            assert!(
                !output.contains(&format!("  {serialized_name}:")),
                "found {serialized_name}"
            );
        }
    }

    #[test]
    fn animation_clip_settings_field_uses_type_tree_scoped_alias() {
        let animation_clip_type_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "AnimationClip", "Base", 0),
                type_tree_node(1, "AnimationClipSettings", "m_AnimationClipSettings", 0),
                type_tree_node(2, "bool", "m_LoopTime", 0),
            ],
            string_buffer: Vec::new(),
        };
        let animation_clip_hints = UnityYamlObjectConversionHints::from_type_tree(
            &animation_clip_type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert_eq!(
            animation_clip_hints
                .field_name_aliases
                .get("m_AnimationClipSettings")
                .map(String::as_str),
            Some("m_MuscleClipInfo")
        );
        let animation_clip_value = SerializedValue::Struct(vec![field(
            "m_AnimationClipSettings",
            SerializedValue::Struct(vec![field("m_LoopTime", SerializedValue::Bool(true))]),
        )]);
        let animation_clip_document = convert_object_with_options(
            source_with_class_id(74),
            &animation_clip_value,
            &animation_clip_hints,
            UnityYamlObjectFieldMode::ProjectAsset,
            false,
            |_| unreachable!("AnimationClip settings contain no PPtrs"),
        )
        .unwrap();
        let animation_clip_output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[animation_clip_document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(animation_clip_output.contains("m_MuscleClipInfo:"));
        assert!(!animation_clip_output.contains("m_AnimationClipSettings:"));

        let array_type_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "vector", "m_Clips", 0),
                type_tree_node(2, "Array", "Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "AnimationClip", "data", 0),
                type_tree_node(4, "AnimationClipSettings", "m_AnimationClipSettings", 0),
                type_tree_node(5, "bool", "m_LoopTime", 0),
            ],
            string_buffer: Vec::new(),
        };
        let array_hints = UnityYamlObjectConversionHints::from_type_tree(
            &array_type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert_eq!(
            array_hints
                .field_name_aliases
                .get("m_Clips[*].m_AnimationClipSettings")
                .map(String::as_str),
            Some("m_MuscleClipInfo")
        );
        let array_value = SerializedValue::Struct(vec![field(
            "m_Clips",
            SerializedValue::Array(vec![SerializedValue::Struct(vec![field(
                "m_AnimationClipSettings",
                SerializedValue::Struct(vec![field("m_LoopTime", SerializedValue::Bool(false))]),
            )])]),
        )]);
        let array_document = convert_with_object_hints(&array_value, &array_hints, |_| {
            unreachable!("AnimationClip settings contain no PPtrs")
        })
        .unwrap();
        let array_output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[array_document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(array_output.contains("m_MuscleClipInfo:"));

        let other_type_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "AnimationClipSettings", "m_AnimationClipSettings", 0),
                type_tree_node(2, "bool", "m_LoopTime", 0),
            ],
            string_buffer: Vec::new(),
        };
        let other_hints = UnityYamlObjectConversionHints::from_type_tree(
            &other_type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert!(other_hints.field_name_aliases.is_empty());
        let other_document = convert_with_object_hints(&animation_clip_value, &other_hints, |_| {
            unreachable!("the generated fixture has no PPtrs")
        })
        .unwrap();
        let other_output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[other_document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(other_output.contains("m_AnimationClipSettings:"));
        assert!(!other_output.contains("m_MuscleClipInfo:"));
    }

    #[test]
    fn generic_binding_type_id_uses_type_tree_scoped_alias() {
        let type_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "vector", "m_Bindings", 0),
                type_tree_node(2, "Array", "Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "GenericBinding", "data", 0),
                type_tree_node(4, "SInt32", "m_TypeID", 0),
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
            hints
                .field_name_aliases
                .get("m_Bindings[*].m_TypeID")
                .map(String::as_str),
            Some("m_ClassID")
        );

        let value = SerializedValue::Struct(vec![field(
            "m_Bindings",
            SerializedValue::Array(vec![SerializedValue::Struct(vec![field(
                "m_TypeID",
                SerializedValue::I32(17),
            )])]),
        )]);
        let document = convert_with_object_hints(&value, &hints, |_| {
            unreachable!("GenericBinding contains no PPtrs")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(output.contains("m_ClassID: 17"));
        assert!(!output.contains("m_TypeID:"));

        let unrelated_type_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "SInt32", "m_TypeID", 0),
            ],
            string_buffer: Vec::new(),
        };
        let unrelated_hints = UnityYamlObjectConversionHints::from_type_tree(
            &unrelated_type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert!(unrelated_hints.field_name_aliases.is_empty());
        let unrelated_value =
            SerializedValue::Struct(vec![field("m_TypeID", SerializedValue::I32(17))]);
        let unrelated_document =
            convert_with_object_hints(&unrelated_value, &unrelated_hints, |_| {
                unreachable!("the generated fixture has no PPtrs")
            })
            .unwrap();
        let unrelated_output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[unrelated_document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(unrelated_output.contains("m_TypeID: 17"));
        assert!(!unrelated_output.contains("m_ClassID: 17"));
    }

    #[test]
    fn animator_controller_layer_and_blend_tree_aliases_are_type_scoped() {
        let controller_type_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "AnimatorController", "Base", 0),
                type_tree_node(1, "vector", "m_AnimatorEvents", 0),
                type_tree_node(2, "Array", "Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "AnimatorEvent", "data", 0),
                type_tree_node(1, "vector", "m_Layers", 0),
                type_tree_node(2, "Array", "Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "AnimatorControllerLayer", "data", 0),
                type_tree_node(4, "PPtr<AvatarMask>", "m_HumanMask", 0),
            ],
            string_buffer: Vec::new(),
        };
        let controller_hints = UnityYamlObjectConversionHints::from_type_tree(
            &controller_type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert_eq!(
            controller_hints
                .field_name_aliases
                .get("m_AnimatorEvents")
                .map(String::as_str),
            Some("m_AnimatorParameters")
        );
        assert_eq!(
            controller_hints
                .field_name_aliases
                .get("m_Layers")
                .map(String::as_str),
            Some("m_AnimatorLayers")
        );
        assert_eq!(
            controller_hints
                .field_name_aliases
                .get("m_Layers[*].m_HumanMask")
                .map(String::as_str),
            Some("m_Mask")
        );
        let controller_value = SerializedValue::Struct(vec![
            field(
                "m_AnimatorEvents",
                SerializedValue::Array(vec![SerializedValue::Struct(Vec::new())]),
            ),
            field(
                "m_Layers",
                SerializedValue::Array(vec![SerializedValue::Struct(vec![field(
                    "m_HumanMask",
                    null_pptr(),
                )])]),
            ),
        ]);
        let controller_document =
            convert_with_object_hints(&controller_value, &controller_hints, |_| {
                unreachable!("the generated null-mask fixture has no non-null PPtrs")
            })
            .unwrap();
        let controller_output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[controller_document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(controller_output.contains("m_AnimatorParameters:"));
        assert!(controller_output.contains("m_AnimatorLayers:"));
        assert!(controller_output.contains("m_Mask: {fileID: 0}"));
        assert!(!controller_output.contains("m_AnimatorEvents:"));
        assert!(!controller_output.contains("m_Layers:"));
        assert!(!controller_output.contains("m_HumanMask:"));

        let blend_tree_type_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "BlendTree", "Base", 0),
                type_tree_node(1, "string", "m_BlendEvent", 0),
                type_tree_node(2, "Array", "Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "char", "data", 0),
                type_tree_node(1, "string", "m_BlendEventY", 0),
                type_tree_node(2, "Array", "Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "char", "data", 0),
            ],
            string_buffer: Vec::new(),
        };
        let blend_tree_hints = UnityYamlObjectConversionHints::from_type_tree(
            &blend_tree_type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        let blend_tree_value = SerializedValue::Struct(vec![
            field(
                "m_BlendEvent",
                SerializedValue::StringBytes(b"Horizontal".to_vec()),
            ),
            field(
                "m_BlendEventY",
                SerializedValue::StringBytes(b"Vertical".to_vec()),
            ),
        ]);
        let blend_tree_document =
            convert_with_object_hints(&blend_tree_value, &blend_tree_hints, |_| {
                unreachable!("BlendTree parameters contain no PPtrs")
            })
            .unwrap();
        let blend_tree_output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[blend_tree_document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(blend_tree_output.contains("m_BlendParameter: Horizontal"));
        assert!(blend_tree_output.contains("m_BlendParameterY: Vertical"));
        assert!(!blend_tree_output.contains("m_BlendEvent:"));
        assert!(!blend_tree_output.contains("m_BlendEventY:"));
    }

    #[test]
    fn lightmap_texture3d_mesh_renderer_and_prefab_aliases_are_type_scoped() {
        let type_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "SInt32", "m_DataSize", 0),
                type_tree_node(1, "LightmapSettings", "m_Lighting", 0),
                type_tree_node(2, "PPtr<LightingDataAsset>", "m_LightmapSnapshot", 0),
                type_tree_node(1, "Texture3D", "m_Texture", 0),
                type_tree_node(2, "SInt32", "m_DataSize", 0),
                type_tree_node(2, "SInt32", "m_ImageCount", 0),
                type_tree_node(1, "MeshRenderer", "m_Renderer", 0),
                type_tree_node(2, "bool", "m_StitchSeams", 0),
                type_tree_node(1, "Prefab", "m_Prefab", 0),
                type_tree_node(2, "bool", "m_IsPrefabParent", 0),
                type_tree_node(2, "PPtr<Prefab>", "m_ParentPrefab", 0),
            ],
            string_buffer: Vec::new(),
        };
        let hints = UnityYamlObjectConversionHints::from_type_tree(
            &type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        for (path, alias) in [
            ("m_Lighting.m_LightmapSnapshot", "m_LightingDataAsset"),
            ("m_Texture.m_DataSize", "m_CompleteImageSize"),
            ("m_Texture.m_ImageCount", "m_Depth"),
            ("m_Renderer.m_StitchSeams", "m_StitchLightmapSeams"),
            ("m_Prefab.m_IsPrefabParent", "m_IsPrefabAsset"),
            ("m_Prefab.m_ParentPrefab", "m_SourcePrefab"),
        ] {
            assert_eq!(
                hints.field_name_aliases.get(path).map(String::as_str),
                Some(alias)
            );
        }

        let value = SerializedValue::Struct(vec![
            field("m_DataSize", SerializedValue::I32(777)),
            field(
                "m_Lighting",
                SerializedValue::Struct(vec![field("m_LightmapSnapshot", null_pptr())]),
            ),
            field(
                "m_Texture",
                SerializedValue::Struct(vec![
                    field("m_DataSize", SerializedValue::I32(2048)),
                    field("m_ImageCount", SerializedValue::I32(8)),
                ]),
            ),
            field(
                "m_Renderer",
                SerializedValue::Struct(vec![field("m_StitchSeams", SerializedValue::Bool(true))]),
            ),
            field(
                "m_Prefab",
                SerializedValue::Struct(vec![
                    field("m_IsPrefabParent", SerializedValue::Bool(true)),
                    field("m_ParentPrefab", null_pptr()),
                ]),
            ),
        ]);
        let document = convert_with_object_hints(&value, &hints, |_| {
            unreachable!("the generated pointer fields are null")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        for expected in [
            "m_DataSize: 777",
            "m_LightingDataAsset: {fileID: 0}",
            "m_CompleteImageSize: 2048",
            "m_Depth: 8",
            "m_StitchLightmapSeams: 1",
            "m_IsPrefabAsset: 1",
            "m_SourcePrefab: {fileID: 0}",
        ] {
            assert!(output.contains(expected), "missing {expected}");
        }
        for serialized_name in [
            "m_LightmapSnapshot",
            "m_ImageCount",
            "m_StitchSeams",
            "m_IsPrefabParent",
            "m_ParentPrefab",
        ] {
            assert!(
                !output.contains(&format!("{serialized_name}:")),
                "found old key {serialized_name}"
            );
        }
        assert!(!output.contains("m_DataSize: 2048"));
    }

    #[test]
    fn animation_constant_field_aliases_follow_type_paths() {
        let type_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "SInt32", "m_MotionSetCount", 0),
                type_tree_node(1, "HumanLayerConstant", "m_HumanLayer", 0),
                type_tree_node(2, "SInt32", "m_StateMachineMotionSetIndex", 0),
                type_tree_node(1, "LayerConstant", "m_Layer", 0),
                type_tree_node(2, "SInt32", "m_StateMachineMotionSetIndex", 0),
                type_tree_node(1, "ControllerConstant", "m_Controller", 0),
                type_tree_node(2, "vector", "m_HumanLayerArray", 0),
                type_tree_node(3, "Array", "Array", 0),
                type_tree_node(4, "SInt32", "size", 0),
                type_tree_node(4, "SInt32", "data", 0),
                type_tree_node(1, "StateMachineConstant", "m_StateMachine", 0),
                type_tree_node(2, "SInt32", "m_MotionSetCount", 0),
            ],
            string_buffer: Vec::new(),
        };
        let hints = UnityYamlObjectConversionHints::from_type_tree(
            &type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        for (path, alias) in [
            (
                "m_HumanLayer.m_StateMachineMotionSetIndex",
                "m_StateMachineSynchronizedLayerIndex",
            ),
            (
                "m_Layer.m_StateMachineMotionSetIndex",
                "m_StateMachineSynchronizedLayerIndex",
            ),
            ("m_Controller.m_HumanLayerArray", "m_LayerArray"),
            (
                "m_StateMachine.m_MotionSetCount",
                "m_SynchronizedLayerCount",
            ),
        ] {
            assert_eq!(
                hints.field_name_aliases.get(path).map(String::as_str),
                Some(alias)
            );
        }

        let value = SerializedValue::Struct(vec![
            field("m_MotionSetCount", SerializedValue::I32(99)),
            field(
                "m_HumanLayer",
                SerializedValue::Struct(vec![field(
                    "m_StateMachineMotionSetIndex",
                    SerializedValue::I32(3),
                )]),
            ),
            field(
                "m_Layer",
                SerializedValue::Struct(vec![field(
                    "m_StateMachineMotionSetIndex",
                    SerializedValue::I32(4),
                )]),
            ),
            field(
                "m_Controller",
                SerializedValue::Struct(vec![field(
                    "m_HumanLayerArray",
                    SerializedValue::Array(vec![SerializedValue::I32(5)]),
                )]),
            ),
            field(
                "m_StateMachine",
                SerializedValue::Struct(vec![field("m_MotionSetCount", SerializedValue::I32(6))]),
            ),
        ]);
        let document = convert_with_object_hints(&value, &hints, |_| {
            unreachable!("animation constant fixture has no PPtrs")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        for expected in [
            "m_MotionSetCount: 99",
            "m_StateMachineSynchronizedLayerIndex: 3",
            "m_StateMachineSynchronizedLayerIndex: 4",
            "m_LayerArray:",
            "m_SynchronizedLayerCount: 6",
        ] {
            assert!(output.contains(expected), "missing {expected}");
        }
        assert!(!output.contains("m_HumanLayerArray:"));
        assert!(!output.contains("m_MotionSetCount: 6"));
        assert!(!output.contains("m_StateMachineMotionSetIndex:"));
    }

    #[test]
    fn build_settings_and_heightmap_aliases_follow_type_paths() {
        let type_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "SInt32", "m_Levels", 0),
                type_tree_node(1, "BuildSettings", "m_BuildSettings", 0),
                type_tree_node(2, "vector", "m_Levels", 0),
                type_tree_node(3, "Array", "Array", 0),
                type_tree_node(4, "SInt32", "size", 0),
                type_tree_node(4, "string", "data", 0),
                type_tree_node(1, "Heightmap", "m_Heightmap", 0),
                type_tree_node(2, "bool", "m_EnableSurfaceMaskTextureCompression", 0),
                type_tree_node(2, "PPtr<Texture2D>", "m_SurfaceMask", 0),
                type_tree_node(2, "SInt32", "m_SurfaceMaskLOD", 0),
            ],
            string_buffer: Vec::new(),
        };
        let hints = UnityYamlObjectConversionHints::from_type_tree(
            &type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        for (path, alias) in [
            ("m_BuildSettings.m_Levels", "m_Scenes"),
            (
                "m_Heightmap.m_EnableSurfaceMaskTextureCompression",
                "m_EnableHolesTextureCompression",
            ),
            ("m_Heightmap.m_SurfaceMask", "m_Holes"),
            ("m_Heightmap.m_SurfaceMaskLOD", "m_HolesLOD"),
        ] {
            assert_eq!(
                hints.field_name_aliases.get(path).map(String::as_str),
                Some(alias)
            );
        }

        let value = SerializedValue::Struct(vec![
            field("m_Levels", SerializedValue::I32(123)),
            field(
                "m_BuildSettings",
                SerializedValue::Struct(vec![field(
                    "m_Levels",
                    SerializedValue::Array(vec![SerializedValue::StringBytes(
                        b"Assets/Scenes/Main.unity".to_vec(),
                    )]),
                )]),
            ),
            field(
                "m_Heightmap",
                SerializedValue::Struct(vec![
                    field(
                        "m_EnableSurfaceMaskTextureCompression",
                        SerializedValue::Bool(true),
                    ),
                    field("m_SurfaceMask", null_pptr()),
                    field("m_SurfaceMaskLOD", SerializedValue::I32(2)),
                ]),
            ),
        ]);
        let document = convert_with_object_hints(&value, &hints, |_| {
            unreachable!("the generated texture pointer is null")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        for expected in [
            "m_Levels: 123",
            "m_Scenes:",
            "Assets/Scenes/Main.unity",
            "m_EnableHolesTextureCompression: 1",
            "m_Holes: {fileID: 0}",
            "m_HolesLOD: 2",
        ] {
            assert!(output.contains(expected), "missing {expected}");
        }
        assert!(!output.contains("m_EnableSurfaceMaskTextureCompression:"));
        assert!(!output.contains("m_SurfaceMask:"));
        assert!(!output.contains("m_SurfaceMaskLOD:"));
        assert!(!output.contains("m_Levels:\n"));
    }

    #[test]
    fn graphics_and_importer_setting_aliases_are_type_scoped() {
        let type_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "SInt32", "m_BuildTargetSettings", 0),
                type_tree_node(1, "SInt32", "m_ShaderSettings", 0),
                type_tree_node(1, "GraphicsSettings", "m_Graphics", 0),
                type_tree_node(2, "SInt32", "m_BuildTargetShaderSettings", 0),
                type_tree_node(2, "SInt32", "m_AlbedoSwatchInfos", 0),
                type_tree_node(1, "TextureImporter", "m_Importer", 0),
                type_tree_node(2, "SInt32", "m_BuildTargetSettings", 0),
                type_tree_node(1, "BuildTargetShaderSettings", "m_LegacySettings", 0),
                type_tree_node(2, "SInt32", "m_ShaderSettings", 0),
            ],
            string_buffer: Vec::new(),
        };
        let hints = UnityYamlObjectConversionHints::from_type_tree(
            &type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        for (path, alias) in [
            ("m_Graphics.m_BuildTargetShaderSettings", "m_TierSettings"),
            ("m_Graphics.m_AlbedoSwatchInfos", "m_AlbedoSwitchInfos"),
            ("m_Importer.m_BuildTargetSettings", "m_PlatformSettings"),
            ("m_LegacySettings.m_ShaderSettings", "m_Settings"),
        ] {
            assert_eq!(
                hints.field_name_aliases.get(path).map(String::as_str),
                Some(alias)
            );
        }

        let value = SerializedValue::Struct(vec![
            field("m_BuildTargetSettings", SerializedValue::I32(77)),
            field("m_ShaderSettings", SerializedValue::I32(88)),
            field(
                "m_Graphics",
                SerializedValue::Struct(vec![
                    field("m_BuildTargetShaderSettings", SerializedValue::I32(1)),
                    field("m_AlbedoSwatchInfos", SerializedValue::I32(2)),
                ]),
            ),
            field(
                "m_Importer",
                SerializedValue::Struct(vec![field(
                    "m_BuildTargetSettings",
                    SerializedValue::I32(3),
                )]),
            ),
            field(
                "m_LegacySettings",
                SerializedValue::Struct(vec![field("m_ShaderSettings", SerializedValue::I32(4))]),
            ),
        ]);
        let document = convert_with_object_hints(&value, &hints, |_| {
            unreachable!("the generated settings fixture has no PPtrs")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        for expected in [
            "m_BuildTargetSettings: 77",
            "m_ShaderSettings: 88",
            "m_TierSettings: 1",
            "m_AlbedoSwitchInfos: 2",
            "m_PlatformSettings: 3",
            "m_Settings: 4",
        ] {
            assert!(output.contains(expected), "missing {expected}");
        }
        assert!(!output.contains("m_BuildTargetShaderSettings:"));
        assert!(!output.contains("m_AlbedoSwatchInfos:"));
        assert!(!output.contains("m_PlatformSettings: 77"));
    }

    #[test]
    fn buffer_vertex_compute_yaml_aliases_are_type_scoped() {
        let type_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "SInt32", "m_Index", 0),
                type_tree_node(1, "Binding", "m_Binding", 0),
                type_tree_node(2, "SInt32", "m_Set", 0),
                type_tree_node(1, "MatrixParameter", "m_Matrix", 0),
                type_tree_node(2, "SInt32", "m_Index", 0),
                type_tree_node(1, "StructParameter", "m_Struct", 0),
                type_tree_node(2, "SInt32", "m_Index", 0),
                type_tree_node(1, "VectorParameter", "m_Vector", 0),
                type_tree_node(2, "SInt32", "m_Index", 0),
                type_tree_node(1, "VertexData", "m_Vertex", 0),
                type_tree_node(2, "SInt32", "m_DataSize", 0),
                type_tree_node(1, "ComputeShaderPlatformVariant", "m_Compute", 0),
                type_tree_node(2, "SInt32", "m_Kernels", 0),
            ],
            string_buffer: Vec::new(),
        };
        let hints = UnityYamlObjectConversionHints::from_type_tree(
            &type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        for (path, alias) in [
            ("m_Binding.m_Set", "m_EncodedData"),
            ("m_Matrix.m_Index", "m_OffsetInConstantBuffer"),
            ("m_Struct.m_Index", "m_OffsetInConstantBuffer"),
            ("m_Vector.m_Index", "m_OffsetInConstantBuffer"),
            ("m_Vertex.m_DataSize", "m_Data"),
            ("m_Compute.m_Kernels", "m_KernelParents"),
        ] {
            assert_eq!(
                hints.field_name_aliases.get(path).map(String::as_str),
                Some(alias)
            );
        }

        let value = SerializedValue::Struct(vec![
            field("m_Index", SerializedValue::I32(99)),
            field(
                "m_Binding",
                SerializedValue::Struct(vec![field("m_Set", SerializedValue::I32(1))]),
            ),
            field(
                "m_Matrix",
                SerializedValue::Struct(vec![field("m_Index", SerializedValue::I32(2))]),
            ),
            field(
                "m_Struct",
                SerializedValue::Struct(vec![field("m_Index", SerializedValue::I32(3))]),
            ),
            field(
                "m_Vector",
                SerializedValue::Struct(vec![field("m_Index", SerializedValue::I32(4))]),
            ),
            field(
                "m_Vertex",
                SerializedValue::Struct(vec![field("m_DataSize", SerializedValue::I32(5))]),
            ),
            field(
                "m_Compute",
                SerializedValue::Struct(vec![field("m_Kernels", SerializedValue::I32(6))]),
            ),
        ]);
        let document = convert_with_object_hints(&value, &hints, |_| {
            unreachable!("the generated shader settings fixture has no PPtrs")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        for expected in [
            "m_Index: 99",
            "m_EncodedData: 1",
            "m_OffsetInConstantBuffer: 2",
            "m_OffsetInConstantBuffer: 3",
            "m_OffsetInConstantBuffer: 4",
            "m_Data: 5",
            "m_KernelParents: 6",
        ] {
            assert!(output.contains(expected), "missing {expected}");
        }
        assert!(!output.contains("m_Set:"));
        assert!(!output.contains("m_DataSize:"));
        assert!(!output.contains("m_Kernels:"));
        assert!(!output.contains("m_Index: 2"));
    }

    #[test]
    fn particle_and_shape_module_aliases_follow_child_type() {
        let type_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "SInt32", "m_StartDelay", 0),
                type_tree_node(1, "SInt32", "m_Arc", 0),
                type_tree_node(1, "ParticleSystem", "m_ParticleSingle", 0),
                type_tree_node(2, "float", "m_StartDelay", 0),
                type_tree_node(1, "ParticleSystem", "m_ParticleCurve", 0),
                type_tree_node(2, "MinMaxCurve", "m_StartDelay", 0),
                type_tree_node(1, "ShapeModule", "m_ShapeArcCurve", 0),
                type_tree_node(2, "MinMaxCurve", "m_Arc", 0),
                type_tree_node(2, "float", "m_Radius", 0),
                type_tree_node(1, "ShapeModule", "m_ShapeRadiusCurve", 0),
                type_tree_node(2, "float", "m_Arc", 0),
                type_tree_node(2, "MinMaxCurve", "m_Radius", 0),
            ],
            string_buffer: Vec::new(),
        };
        let hints = UnityYamlObjectConversionHints::from_type_tree(
            &type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        for (path, alias) in [
            ("m_ParticleSingle.m_StartDelay", "m_StartDelay_Single"),
            ("m_ParticleCurve.m_StartDelay", "m_StartDelay_MinMaxCurve"),
            ("m_ShapeArcCurve.m_Arc", "m_ArcParameter"),
            ("m_ShapeRadiusCurve.m_Radius", "m_RadiusParameter"),
        ] {
            assert_eq!(
                hints.field_name_aliases.get(path).map(String::as_str),
                Some(alias)
            );
        }
        assert!(!hints
            .field_name_aliases
            .contains_key("m_ShapeArcCurve.m_Radius"));
        assert!(!hints
            .field_name_aliases
            .contains_key("m_ShapeRadiusCurve.m_Arc"));
        assert!(!hints.field_name_aliases.contains_key("m_StartDelay"));
        assert!(!hints.field_name_aliases.contains_key("m_Arc"));

        let value = SerializedValue::Struct(vec![
            field("m_StartDelay", SerializedValue::I32(9)),
            field("m_Arc", SerializedValue::I32(8)),
            field(
                "m_ParticleSingle",
                SerializedValue::Struct(vec![field("m_StartDelay", SerializedValue::F32(0.5))]),
            ),
            field(
                "m_ParticleCurve",
                SerializedValue::Struct(vec![field(
                    "m_StartDelay",
                    SerializedValue::Struct(Vec::new()),
                )]),
            ),
            field(
                "m_ShapeArcCurve",
                SerializedValue::Struct(vec![
                    field("m_Arc", SerializedValue::Struct(Vec::new())),
                    field("m_Radius", SerializedValue::F32(2.0)),
                ]),
            ),
            field(
                "m_ShapeRadiusCurve",
                SerializedValue::Struct(vec![
                    field("m_Arc", SerializedValue::F32(3.0)),
                    field("m_Radius", SerializedValue::Struct(Vec::new())),
                ]),
            ),
        ]);
        let document = convert_with_object_hints(&value, &hints, |_| {
            unreachable!("ParticleSystem and ShapeModule fixtures contain no PPtrs")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        for expected in [
            "m_StartDelay: 9",
            "m_Arc: 8",
            "m_StartDelay_Single: 0.5",
            "m_StartDelay_MinMaxCurve: {}",
            "m_ArcParameter: {}",
            "m_Radius: 2",
            "m_RadiusParameter: {}",
            "m_Arc: 3",
        ] {
            assert!(output.contains(expected), "missing {expected}");
        }
    }

    #[test]
    fn clip_muscle_and_xform_aliases_follow_vector_child_type() {
        let type_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "SInt32", "m_T", 0),
                type_tree_node(1, "ClipMuscleConstant", "m_Clip", 0),
                type_tree_node(2, "Vector3Float", "m_AverageSpeed", 0),
                type_tree_node(3, "float", "x", 0),
                type_tree_node(3, "float", "y", 0),
                type_tree_node(3, "float", "z", 0),
                type_tree_node(1, "Xform", "m_Xform4_3", 0),
                type_tree_node(2, "Vector4Float", "m_T", 0),
                type_tree_node(3, "float", "x", 0),
                type_tree_node(3, "float", "y", 0),
                type_tree_node(3, "float", "z", 0),
                type_tree_node(3, "float", "w", 0),
                type_tree_node(2, "Vector3Float", "m_S", 0),
                type_tree_node(3, "float", "x", 0),
                type_tree_node(3, "float", "y", 0),
                type_tree_node(3, "float", "z", 0),
                type_tree_node(1, "Xform", "m_Xform3_4", 0),
                type_tree_node(2, "Vector3Float", "m_T", 0),
                type_tree_node(3, "float", "x", 0),
                type_tree_node(3, "float", "y", 0),
                type_tree_node(3, "float", "z", 0),
                type_tree_node(2, "Vector4Float", "m_S", 0),
                type_tree_node(3, "float", "x", 0),
                type_tree_node(3, "float", "y", 0),
                type_tree_node(3, "float", "z", 0),
                type_tree_node(3, "float", "w", 0),
            ],
            string_buffer: Vec::new(),
        };
        let hints = UnityYamlObjectConversionHints::from_type_tree(
            &type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        for (path, alias) in [
            ("m_Clip.m_AverageSpeed", "m_AverageSpeed3"),
            ("m_Xform4_3.m_T", "m_T4"),
            ("m_Xform4_3.m_S", "m_S3"),
            ("m_Xform3_4.m_T", "m_T3"),
            ("m_Xform3_4.m_S", "m_S4"),
        ] {
            assert_eq!(
                hints.field_name_aliases.get(path).map(String::as_str),
                Some(alias)
            );
        }
        assert!(!hints.field_name_aliases.contains_key("m_T"));

        let vector3 = || {
            SerializedValue::Struct(vec![
                field("x", SerializedValue::F32(1.0)),
                field("y", SerializedValue::F32(2.0)),
                field("z", SerializedValue::F32(3.0)),
            ])
        };
        let vector4 = || {
            SerializedValue::Struct(vec![
                field("x", SerializedValue::F32(1.0)),
                field("y", SerializedValue::F32(2.0)),
                field("z", SerializedValue::F32(3.0)),
                field("w", SerializedValue::F32(4.0)),
            ])
        };
        let value = SerializedValue::Struct(vec![
            field("m_T", SerializedValue::I32(77)),
            field(
                "m_Clip",
                SerializedValue::Struct(vec![field("m_AverageSpeed", vector3())]),
            ),
            field(
                "m_Xform4_3",
                SerializedValue::Struct(vec![field("m_T", vector4()), field("m_S", vector3())]),
            ),
            field(
                "m_Xform3_4",
                SerializedValue::Struct(vec![field("m_T", vector3()), field("m_S", vector4())]),
            ),
        ]);
        let document = convert_with_object_hints(&value, &hints, |_| {
            unreachable!("the generated vector fixture has no PPtrs")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        for expected in [
            "m_T: 77",
            "m_AverageSpeed3:",
            "m_T4:",
            "m_S3:",
            "m_T3:",
            "m_S4:",
        ] {
            assert!(output.contains(expected), "missing {expected}");
        }
        for serialized_name in ["m_AverageSpeed:", "m_T:", "m_S:"] {
            assert!(
                !output.contains(&format!("    {serialized_name}")),
                "found old nested key {serialized_name}"
            );
        }

        let unsupported = TypeTree {
            nodes: vec![
                type_tree_node(0, "Xform", "Base", 0),
                type_tree_node(1, "SInt32", "m_T", 0),
            ],
            string_buffer: Vec::new(),
        };
        assert!(matches!(
            UnityYamlObjectConversionHints::from_type_tree(
                &unsupported,
                version(),
                UnityYamlObjectConversionLimits::default()
            ),
            Err(UnityYamlObjectConversionError::InvalidTypeTree {
                reason: "unsupported child TypeTree type for baseline field alias",
                ..
            })
        ));
    }

    #[test]
    fn value_array_aliases_follow_nested_vector_element_type() {
        fn append_vector_array_field(
            nodes: &mut Vec<TypeTreeNode>,
            field_name: &str,
            element_type: &str,
        ) {
            nodes.push(type_tree_node(2, "vector", field_name, 0));
            nodes.push(type_tree_node(3, "Array", "m_Array", 0));
            nodes.push(type_tree_node(4, "SInt32", "size", 0));
            nodes.push(type_tree_node(4, element_type, "m_Data", 0));
            for component in ["x", "y", "z"] {
                nodes.push(type_tree_node(5, "float", component, 0));
            }
            if element_type == "Vector4Float" {
                nodes.push(type_tree_node(5, "float", "w", 0));
            }
        }

        let mut nodes = vec![
            type_tree_node(0, "GameObject", "Base", 0),
            type_tree_node(1, "SInt32", "m_PositionValues", 0),
            type_tree_node(1, "ValueArray", "m_ThreeFour", 0),
        ];
        append_vector_array_field(&mut nodes, "m_PositionValues", "Vector3Float");
        append_vector_array_field(&mut nodes, "m_ScaleValues", "Vector4Float");
        nodes.push(type_tree_node(1, "ValueArray", "m_FourThree", 0));
        append_vector_array_field(&mut nodes, "m_PositionValues", "Vector4Float");
        append_vector_array_field(&mut nodes, "m_ScaleValues", "Vector3Float");
        let type_tree = TypeTree {
            nodes,
            string_buffer: Vec::new(),
        };
        let hints = UnityYamlObjectConversionHints::from_type_tree(
            &type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        for (path, alias) in [
            ("m_ThreeFour.m_PositionValues", "m_PositionValues3"),
            ("m_ThreeFour.m_ScaleValues", "m_ScaleValues4"),
            ("m_FourThree.m_PositionValues", "m_PositionValues4"),
            ("m_FourThree.m_ScaleValues", "m_ScaleValues3"),
        ] {
            assert_eq!(
                hints.field_name_aliases.get(path).map(String::as_str),
                Some(alias)
            );
        }
        assert!(!hints.field_name_aliases.contains_key("m_PositionValues"));

        let vector3 = || {
            SerializedValue::Struct(vec![
                field("x", SerializedValue::F32(1.0)),
                field("y", SerializedValue::F32(2.0)),
                field("z", SerializedValue::F32(3.0)),
            ])
        };
        let vector4 = || {
            SerializedValue::Struct(vec![
                field("x", SerializedValue::F32(1.0)),
                field("y", SerializedValue::F32(2.0)),
                field("z", SerializedValue::F32(3.0)),
                field("w", SerializedValue::F32(4.0)),
            ])
        };
        let value = SerializedValue::Struct(vec![
            field("m_PositionValues", SerializedValue::I32(77)),
            field(
                "m_ThreeFour",
                SerializedValue::Struct(vec![
                    field("m_PositionValues", SerializedValue::Array(vec![vector3()])),
                    field("m_ScaleValues", SerializedValue::Array(vec![vector4()])),
                ]),
            ),
            field(
                "m_FourThree",
                SerializedValue::Struct(vec![
                    field("m_PositionValues", SerializedValue::Array(vec![vector4()])),
                    field("m_ScaleValues", SerializedValue::Array(vec![vector3()])),
                ]),
            ),
        ]);
        let document = convert_with_object_hints(&value, &hints, |_| {
            unreachable!("the generated ValueArray fixture has no PPtrs")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        for expected in [
            "m_PositionValues: 77",
            "m_PositionValues3:",
            "m_PositionValues4:",
            "m_ScaleValues3:",
            "m_ScaleValues4:",
            "x: 1",
            "w: 4",
        ] {
            assert!(output.contains(expected), "missing {expected}");
        }

        let unsupported = TypeTree {
            nodes: vec![
                type_tree_node(0, "ValueArray", "Base", 0),
                type_tree_node(1, "vector", "m_PositionValues", 0),
                type_tree_node(2, "Array", "m_Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "SInt32", "m_Data", 0),
            ],
            string_buffer: Vec::new(),
        };
        assert!(matches!(
            UnityYamlObjectConversionHints::from_type_tree(
                &unsupported,
                version(),
                UnityYamlObjectConversionLimits::default()
            ),
            Err(UnityYamlObjectConversionError::InvalidTypeTree {
                reason: "unsupported ValueArray element TypeTree type",
                ..
            })
        ));
    }

    #[test]
    fn mesh_shape_and_shader_blob_aliases_follow_baseline_rules() {
        let type_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "vector", "m_Shapes", 0),
                type_tree_node(1, "Mesh", "m_Mesh", 0),
                type_tree_node(2, "vector", "m_Shapes", 0),
                type_tree_node(1, "Mesh", "m_NonVectorMesh", 0),
                type_tree_node(2, "SInt32", "m_Shapes", 0),
                type_tree_node(1, "Shader", "m_Shader", 0),
                type_tree_node(2, "SInt32", "m_SubProgramBlob", 0),
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
            hints
                .field_name_aliases
                .get("m_Mesh.m_Shapes")
                .map(String::as_str),
            Some("m_ShapesList")
        );
        assert!(!hints
            .field_name_aliases
            .contains_key("m_NonVectorMesh.m_Shapes"));
        assert!(!hints.field_name_aliases.contains_key("m_Shapes"));
        assert_eq!(
            hints
                .field_name_aliases
                .get("m_Shader.m_SubProgramBlob")
                .map(String::as_str),
            Some("m_CompressedBlob")
        );

        let value = SerializedValue::Struct(vec![
            field("m_Shapes", SerializedValue::I32(77)),
            field(
                "m_Mesh",
                SerializedValue::Struct(vec![field(
                    "m_Shapes",
                    SerializedValue::Array(vec![SerializedValue::I32(1)]),
                )]),
            ),
            field(
                "m_NonVectorMesh",
                SerializedValue::Struct(vec![field("m_Shapes", SerializedValue::I32(8))]),
            ),
            field(
                "m_Shader",
                SerializedValue::Struct(vec![field("m_SubProgramBlob", SerializedValue::I32(9))]),
            ),
        ]);
        let document = convert_with_object_hints(&value, &hints, |_| {
            unreachable!("the generated Mesh/Shader fixture has no PPtrs")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        for expected in [
            "m_Shapes: 77",
            "m_ShapesList:",
            "m_Shapes: 8",
            "m_CompressedBlob: 9",
        ] {
            assert!(output.contains(expected), "missing {expected}");
        }
        assert!(!output.contains("m_SubProgramBlob:"));
    }

    #[test]
    fn game_object_component_array_uses_plural_baseline_field_name() {
        let type_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "vector", "m_Component", 0),
                type_tree_node(1, "SomeStruct", "m_Unrelated", 0),
                type_tree_node(2, "SInt32", "m_Component", 0),
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
            hints
                .field_name_aliases
                .get("m_Component")
                .map(String::as_str),
            Some("m_Components")
        );
        assert!(!hints
            .field_name_aliases
            .contains_key("m_Unrelated.m_Component"));

        let value = SerializedValue::Struct(vec![
            field(
                "m_Component",
                SerializedValue::Array(vec![SerializedValue::I32(123)]),
            ),
            field(
                "m_Unrelated",
                SerializedValue::Struct(vec![field("m_Component", SerializedValue::I32(456))]),
            ),
        ]);
        let document = convert_with_object_hints(&value, &hints, |_| {
            unreachable!("the generated GameObject fixture has no PPtrs")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(output.contains("m_Components:"));
        assert!(output.contains("m_Component: 456"));
        assert!(!output.contains("m_Component:\n"));
    }

    #[test]
    fn stripped_asset_yaml_keeps_only_baseline_prefab_fields() {
        let root = SerializedValue::Struct(vec![field(
            "Base",
            SerializedValue::Struct(vec![
                field("m_CorrespondingSourceObject", null_pptr()),
                field("m_PrefabAsset", null_pptr()),
                field("m_PrefabInstance", null_pptr()),
                field("m_PrefabInternal", null_pptr()),
                field("m_PrefabParentObject", null_pptr()),
                field("m_ObjectHideFlags", SerializedValue::U32(0)),
                field(
                    "m_Name",
                    SerializedValue::StringBytes(b"not retained".to_vec()),
                ),
            ]),
        )]);
        let document = convert_object_with_options(
            source(),
            &root,
            &UnityYamlObjectConversionHints::default(),
            UnityYamlObjectFieldMode::ProjectAsset,
            true,
            |_| ProjectPointerResolution::Null,
        )
        .unwrap();
        assert!(document.root.stripped);
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(output.contains("--- !u!1 &1000001 stripped"));
        assert!(output.contains("m_CorrespondingSourceObject: {fileID: 0}"));
        assert!(output.contains("m_PrefabAsset: {fileID: 0}"));
        assert!(output.contains("m_PrefabInstance: {fileID: 0}"));
        assert!(output.contains("m_PrefabInternal: {fileID: 0}"));
        assert!(output.contains("m_PrefabParentObject: {fileID: 0}"));
        assert!(!output.contains("serializedVersion"));
        assert!(!output.contains("m_ObjectHideFlags"));
        assert!(!output.contains("m_Name"));
    }

    #[test]
    fn stripped_monobehaviour_yaml_keeps_baseline_identity_fields() {
        let root = SerializedValue::Struct(vec![
            field("m_CorrespondingSourceObject", null_pptr()),
            field(
                "m_EditorClassIdentifier",
                SerializedValue::StringBytes(Vec::new()),
            ),
            field("m_EditorHideFlags", SerializedValue::I32(0)),
            field("m_Enabled", SerializedValue::Bool(true)),
            field("m_GameObject", null_pptr()),
            field("m_Name", SerializedValue::StringBytes(Vec::new())),
            field("m_PrefabAsset", null_pptr()),
            field("m_PrefabInstance", null_pptr()),
            field("m_PrefabInternal", null_pptr()),
            field("m_PrefabParentObject", null_pptr()),
            field("m_Script", null_pptr()),
            field("m_ObjectHideFlags", SerializedValue::U32(0)),
            field("m_CustomData", SerializedValue::I32(123)),
        ]);
        let document = convert_object_with_options(
            source_with_class_id(114),
            &root,
            &UnityYamlObjectConversionHints::default(),
            UnityYamlObjectFieldMode::ProjectAsset,
            true,
            |_| ProjectPointerResolution::Null,
        )
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(output.contains("--- !u!114 &1000001 stripped"));
        for field_name in STRIPPED_MONO_BEHAVIOUR_ALLOWED_FIELDS {
            assert!(output.contains(field_name), "missing {field_name}");
        }
        assert!(!output.contains("serializedVersion"));
        assert!(!output.contains("m_ObjectHideFlags"));
        assert!(!output.contains("m_CustomData"));
    }

    #[test]
    fn monobehaviour_structure_field_is_written_last() {
        let value = SerializedValue::Struct(vec![
            field("m_Before", SerializedValue::I32(1)),
            field(
                "m_Structure",
                SerializedValue::Struct(vec![field("m_Value", SerializedValue::I32(2))]),
            ),
            field("m_After", SerializedValue::I32(3)),
        ]);
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[convert_object_with_options(
                source_with_class_id(114),
                &value,
                &UnityYamlObjectConversionHints::default(),
                UnityYamlObjectFieldMode::ProjectAsset,
                false,
                |_| unreachable!("the generated fixture has no PPtrs"),
            )
            .unwrap()],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        let before = output.find("  m_Before: 1").unwrap();
        let after = output.find("  m_After: 3").unwrap();
        let structure = output.find("  m_Structure:").unwrap();
        assert!(before < after && after < structure);

        let non_mono_output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[convert_object_with_options(
                source(),
                &value,
                &UnityYamlObjectConversionHints::default(),
                UnityYamlObjectFieldMode::ProjectAsset,
                false,
                |_| unreachable!("the generated fixture has no PPtrs"),
            )
            .unwrap()],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        let non_mono_structure = non_mono_output.find("  m_Structure:").unwrap();
        let non_mono_after = non_mono_output.find("  m_After: 3").unwrap();
        assert!(non_mono_structure < non_mono_after);
    }

    #[test]
    fn dictionary_yaml_uses_type_tree_keys_and_unity_version_shape() {
        let string_map_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "map", "m_StringMap", 0),
                type_tree_node(2, "Array", "Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "pair", "data", 0),
                type_tree_node(4, "string", "first", 0),
                type_tree_node(4, "SInt32", "second", 0),
            ],
            string_buffer: Vec::new(),
        };
        let string_hints = UnityYamlObjectConversionHints::from_type_tree(
            &string_map_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert_eq!(
            string_hints.dictionary_representations.get("m_StringMap"),
            Some(&UnityYamlDictionaryRepresentation::KeyedMapping)
        );
        let string_map = SerializedValue::Struct(vec![field(
            "m_StringMap",
            SerializedValue::Array(vec![
                SerializedValue::Struct(vec![
                    field("first", SerializedValue::StringBytes(b"One".to_vec())),
                    field("second", SerializedValue::I32(1)),
                ]),
                SerializedValue::Struct(vec![
                    field("first", SerializedValue::StringBytes(b"Two".to_vec())),
                    field("second", SerializedValue::I32(2)),
                ]),
            ]),
        )]);
        let string_output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[convert_with_object_hints(&string_map, &string_hints, |_| {
                unreachable!("the generated dictionary has no PPtrs")
            })
            .unwrap()],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(string_output.contains("    One: 1"));
        assert!(string_output.contains("    Two: 2"));

        let empty_string_map = SerializedValue::Struct(vec![field(
            "m_StringMap",
            SerializedValue::Array(Vec::new()),
        )]);
        let empty_string_output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[
                convert_with_object_hints(&empty_string_map, &string_hints, |_| {
                    unreachable!("the generated dictionary has no PPtrs")
                })
                .unwrap(),
            ],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(empty_string_output.contains("m_StringMap: {}"));

        let bool_map_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "map", "m_BoolMap", 0),
                type_tree_node(2, "Array", "Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "pair", "data", 0),
                type_tree_node(4, "bool", "first", 0),
                type_tree_node(4, "SInt32", "second", 0),
            ],
            string_buffer: Vec::new(),
        };
        let bool_hints = UnityYamlObjectConversionHints::from_type_tree(
            &bool_map_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        let bool_map = SerializedValue::Struct(vec![field(
            "m_BoolMap",
            SerializedValue::Array(vec![
                SerializedValue::Struct(vec![
                    field("first", SerializedValue::Bool(true)),
                    field("second", SerializedValue::I32(1)),
                ]),
                SerializedValue::Struct(vec![
                    field("first", SerializedValue::Bool(false)),
                    field("second", SerializedValue::I32(0)),
                ]),
            ]),
        )]);
        let bool_output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[convert_with_object_hints(&bool_map, &bool_hints, |_| {
                unreachable!("the generated dictionary has no PPtrs")
            })
            .unwrap()],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(bool_output.contains("True: 1"));
        assert!(bool_output.contains("False: 0"));

        let mut integer_map_tree = bool_map_tree.clone();
        integer_map_tree.nodes[5].type_name = Some("SInt32".to_owned());
        let integer_hints = UnityYamlObjectConversionHints::from_type_tree(
            &integer_map_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        let integer_map = SerializedValue::Struct(vec![field(
            "m_BoolMap",
            SerializedValue::Array(vec![
                SerializedValue::Struct(vec![
                    field("first", SerializedValue::I32(-2)),
                    field("second", SerializedValue::I32(-20)),
                ]),
                SerializedValue::Struct(vec![
                    field("first", SerializedValue::I32(3)),
                    field("second", SerializedValue::I32(30)),
                ]),
            ]),
        )]);
        let integer_output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[
                convert_with_object_hints(&integer_map, &integer_hints, |_| {
                    unreachable!("the generated dictionary has no PPtrs")
                })
                .unwrap(),
            ],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(integer_output.contains("-2: -20"));
        assert!(integer_output.contains("3: 30"));

        let char_map_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "map", "m_CharMap", 0),
                type_tree_node(2, "Array", "Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "pair", "data", 0),
                type_tree_node(4, "char", "first", 0),
                type_tree_node(4, "SInt32", "second", 0),
            ],
            string_buffer: Vec::new(),
        };
        let char_hints = UnityYamlObjectConversionHints::from_type_tree(
            &char_map_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        let char_map = SerializedValue::Struct(vec![field(
            "m_CharMap",
            SerializedValue::Array(vec![SerializedValue::Struct(vec![
                field("first", SerializedValue::Char(u16::from(b'A'))),
                field("second", SerializedValue::I32(1)),
            ])]),
        )]);
        let char_output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[convert_with_object_hints(&char_map, &char_hints, |_| {
                unreachable!("the generated dictionary has no PPtrs")
            })
            .unwrap()],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(char_output.contains("A: 1"));

        let guid_map_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "map", "m_GuidMap", 0),
                type_tree_node(2, "Array", "Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "pair", "data", 0),
                type_tree_node(4, "GUID", "first", 0),
                type_tree_node(5, "UInt32", "m_Data_0_", 0),
                type_tree_node(5, "UInt32", "m_Data_1_", 0),
                type_tree_node(5, "UInt32", "m_Data_2_", 0),
                type_tree_node(5, "UInt32", "m_Data_3_", 0),
                type_tree_node(4, "bool", "second", 0),
            ],
            string_buffer: Vec::new(),
        };
        let guid_hints = UnityYamlObjectConversionHints::from_type_tree(
            &guid_map_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        let guid_map = SerializedValue::Struct(vec![field(
            "m_GuidMap",
            SerializedValue::Array(vec![SerializedValue::Struct(vec![
                field("first", guid_value_struct()),
                field("second", SerializedValue::Bool(true)),
            ])]),
        )]);
        let guid_output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[convert_with_object_hints(&guid_map, &guid_hints, |_| {
                unreachable!("the generated dictionary has no PPtrs")
            })
            .unwrap()],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(guid_output.contains("01020304111213142122232431323334: 1"));

        let vector_map_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "map", "m_VectorMap", 0),
                type_tree_node(2, "Array", "Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "pair", "data", 0),
                type_tree_node(4, "Vector3f", "first", 0),
                type_tree_node(5, "float", "x", 0),
                type_tree_node(5, "float", "y", 0),
                type_tree_node(5, "float", "z", 0),
                type_tree_node(4, "SInt32", "second", 0),
            ],
            string_buffer: Vec::new(),
        };
        let vector_map_value = SerializedValue::Struct(vec![field(
            "m_VectorMap",
            SerializedValue::Array(vec![
                SerializedValue::Struct(vec![
                    field(
                        "first",
                        SerializedValue::Struct(vec![
                            field("x", SerializedValue::F32(1.0)),
                            field("y", SerializedValue::F32(2.0)),
                            field("z", SerializedValue::F32(3.0)),
                        ]),
                    ),
                    field("second", SerializedValue::I32(4)),
                ]),
                SerializedValue::Struct(vec![
                    field(
                        "first",
                        SerializedValue::Struct(vec![
                            field("x", SerializedValue::F32(5.0)),
                            field("y", SerializedValue::F32(6.0)),
                            field("z", SerializedValue::F32(7.0)),
                        ]),
                    ),
                    field("second", SerializedValue::I32(8)),
                ]),
            ]),
        )]);
        let modern_hints = UnityYamlObjectConversionHints::from_type_tree(
            &vector_map_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert_eq!(
            modern_hints.dictionary_representations.get("m_VectorMap"),
            Some(&UnityYamlDictionaryRepresentation::PairSequence)
        );
        let modern_output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[
                convert_with_object_hints(&vector_map_value, &modern_hints, |_| {
                    unreachable!("the generated dictionary has no PPtrs")
                })
                .unwrap(),
            ],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert_eq!(modern_output.matches("  - first:").count(), 2);

        let legacy_version = SceneUnityVersion::parse("5.3.8f1").unwrap();
        let legacy_hints = UnityYamlObjectConversionHints::from_type_tree(
            &vector_map_tree,
            legacy_version,
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert_eq!(
            legacy_hints.dictionary_representations.get("m_VectorMap"),
            Some(&UnityYamlDictionaryRepresentation::LegacyPairMapping)
        );
        let legacy_output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[
                convert_with_object_hints(&vector_map_value, &legacy_hints, |_| {
                    unreachable!("the generated dictionary has no PPtrs")
                })
                .unwrap(),
            ],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert_eq!(legacy_output.matches("    data:").count(), 2);
    }

    #[test]
    fn exposed_reference_table_entries_use_block_sequences_instead_of_keyed_maps() {
        let exposed_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "ExposedReferenceTable", "Base", 0),
                type_tree_node(1, "map", "m_References", 0),
                type_tree_node(2, "Array", "Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "pair", "data", 0),
                type_tree_node(4, "string", "first", 0),
                type_tree_node(5, "Array", "Array", 0),
                type_tree_node(6, "SInt32", "size", 0),
                type_tree_node(6, "char", "data", 0),
                type_tree_node(4, "SInt32", "second", 0),
            ],
            string_buffer: Vec::new(),
        };
        let exposed_hints = UnityYamlObjectConversionHints::from_type_tree(
            &exposed_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert!(exposed_hints
            .force_block_sequence_paths
            .contains("m_References"));
        assert!(!exposed_hints
            .dictionary_representations
            .contains_key("m_References"));
        assert_eq!(
            exposed_hints.empty_array_kinds.get("m_References"),
            Some(&UnityYamlEmptyArrayKind::BlockSequence)
        );

        let entries = SerializedValue::Array(vec![
            SerializedValue::Struct(vec![
                field("first", SerializedValue::StringBytes(b"Player".to_vec())),
                field("second", SerializedValue::I32(9)),
            ]),
            SerializedValue::Struct(vec![
                field(
                    "first",
                    SerializedValue::StringBytes(b"Object.Name".to_vec()),
                ),
                field("second", SerializedValue::I32(14)),
            ]),
        ]);
        let exposed_value = SerializedValue::Struct(vec![field("m_References", entries.clone())]);
        let exposed_document = convert_with_object_hints(&exposed_value, &exposed_hints, |_| {
            unreachable!("the synthetic table has no PPtrs")
        })
        .unwrap();
        let exposed_yaml = object_field(&exposed_document, "m_References");
        let UnityYamlNodeKind::Sequence { style, items } = &exposed_yaml.kind else {
            panic!("editor exposed references are a YAML sequence")
        };
        assert_eq!(*style, UnityYamlSequenceStyle::Block);
        assert_eq!(items.len(), 2);
        assert!(matches!(items[0].kind, UnityYamlNodeKind::Mapping { .. }));

        let mut ordinary_tree = exposed_tree.clone();
        ordinary_tree.nodes[0].type_name = Some("GameObject".to_owned());
        let ordinary_hints = UnityYamlObjectConversionHints::from_type_tree(
            &ordinary_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert!(!ordinary_hints
            .force_block_sequence_paths
            .contains("m_References"));
        assert_eq!(
            ordinary_hints
                .dictionary_representations
                .get("m_References"),
            Some(&UnityYamlDictionaryRepresentation::KeyedMapping)
        );
        let ordinary_value = SerializedValue::Struct(vec![field("m_References", entries)]);
        let ordinary_document = convert_with_object_hints(&ordinary_value, &ordinary_hints, |_| {
            unreachable!("the synthetic dictionary has no PPtrs")
        })
        .unwrap();
        let ordinary_yaml = object_field(&ordinary_document, "m_References");
        let UnityYamlNodeKind::Mapping { entries, .. } = &ordinary_yaml.kind else {
            panic!("ordinary string-key dictionaries remain YAML mappings")
        };
        assert_eq!(entries.len(), 2);

        let empty = SerializedValue::Struct(vec![field(
            "m_References",
            SerializedValue::Array(Vec::new()),
        )]);
        let empty_document = convert_with_object_hints(&empty, &exposed_hints, |_| {
            unreachable!("the empty table has no PPtrs")
        })
        .unwrap();
        assert_eq!(
            object_field(&empty_document, "m_References").kind,
            UnityYamlNodeKind::Sequence {
                style: UnityYamlSequenceStyle::Block,
                items: Vec::new(),
            }
        );
    }

    #[test]
    fn malformed_dictionary_pair_shapes_fail_with_context() {
        let type_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "map", "m_Map", 0),
                type_tree_node(2, "Array", "Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "pair", "data", 0),
                type_tree_node(4, "string", "first", 0),
                type_tree_node(4, "SInt32", "second", 0),
            ],
            string_buffer: Vec::new(),
        };
        let hints = UnityYamlObjectConversionHints::from_type_tree(
            &type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        let malformed = SerializedValue::Struct(vec![field(
            "m_Map",
            SerializedValue::Array(vec![SerializedValue::Struct(vec![field(
                "first",
                SerializedValue::StringBytes(b"key".to_vec()),
            )])]),
        )]);
        assert!(matches!(
            convert_with_object_hints(&malformed, &hints, |_| {
                unreachable!("the malformed dictionary has no PPtrs")
            }),
            Err(UnityYamlObjectConversionError::InvalidDictionaryPair {
                field_path,
                element_index: 0,
            }) if field_path == "m_Map"
        ));

        let malformed_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "map", "m_Map", 0),
                type_tree_node(2, "Array", "Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "pair", "data", 0),
                type_tree_node(4, "string", "first", 0),
            ],
            string_buffer: Vec::new(),
        };
        assert!(matches!(
            UnityYamlObjectConversionHints::from_type_tree(
                &malformed_tree,
                version(),
                UnityYamlObjectConversionLimits::default(),
            ),
            Err(UnityYamlObjectConversionError::InvalidTypeTree {
                reason: "map pair must contain ordered first and second fields",
                ..
            })
        ));
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
        assert!(text.contains("serializedVersion: 6\n  m_HideFlags: 0\n  m_Source: {fileID: 0}\n"));
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
    fn typed_data_lists_match_the_reference_runtime() {
        // Expected hex strings come from the isolated `AssetRipper.Yaml` probe in
        // `Temp/opencode/typedprobe` (recorded in docs/validation-log.md), which called
        // `YamlScalarNode.CreateHex` for every supported element width.
        let cases: Vec<(SerializedValue, &str)> = vec![
            (
                SerializedValue::Array(vec![
                    SerializedValue::U8(0x00),
                    SerializedValue::U8(0x12),
                    SerializedValue::U8(0xFF),
                ]),
                "0012ff",
            ),
            (
                SerializedValue::Array(vec![
                    SerializedValue::I8(-1),
                    SerializedValue::I8(0x7F),
                    SerializedValue::I8(-128),
                ]),
                "ff7f80",
            ),
            (
                SerializedValue::Array(vec![
                    SerializedValue::I16(0x1234),
                    SerializedValue::I16(-2),
                ]),
                "3412feff",
            ),
            (
                SerializedValue::Array(vec![
                    SerializedValue::U16(0x1234),
                    SerializedValue::U16(0xFFFF),
                ]),
                "3412ffff",
            ),
            (
                SerializedValue::Array(vec![
                    SerializedValue::Char(0x41),
                    SerializedValue::Char(0x56DE),
                ]),
                "4100de56",
            ),
            (
                SerializedValue::Array(vec![
                    SerializedValue::I32(0x0102_0304),
                    SerializedValue::I32(-1),
                ]),
                "04030201ffffffff",
            ),
            (
                SerializedValue::Array(vec![
                    SerializedValue::U32(0x0102_0304),
                    SerializedValue::U32(0xFFFF_FFFF),
                ]),
                "04030201ffffffff",
            ),
            (
                SerializedValue::Array(vec![
                    SerializedValue::I64(0x0102_0304_0506_0708),
                    SerializedValue::I64(-1),
                ]),
                "0807060504030201ffffffffffffffff",
            ),
            (
                SerializedValue::Array(vec![
                    SerializedValue::U64(0x0102_0304_0506_0708),
                    SerializedValue::U64(0xFFFF_FFFF_FFFF_FFFF),
                ]),
                "0807060504030201ffffffffffffffff",
            ),
            (
                SerializedValue::Array(vec![
                    SerializedValue::Bool(true),
                    SerializedValue::Bool(false),
                    SerializedValue::Bool(true),
                ]),
                "010001",
            ),
        ];

        for (value, expected) in cases {
            let structure = SerializedValue::Struct(vec![field("m_Data", value)]);
            let document = convert(&structure, &HashMap::new(), |_| {
                unreachable!("typed data lists contain no PPtrs")
            })
            .unwrap();
            let output = crate::export::unity_yaml::write_unity_yaml_documents(
                &[document],
                crate::export::unity_yaml::UnityYamlWriteLimits::default(),
            )
            .unwrap();
            assert!(
                output.contains(&format!("m_Data: {expected}")),
                "expected {expected} in {output}"
            );
        }
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
    fn property_name_types_emit_integer_and_utf8_ids_as_yaml_strings() {
        let type_tree = property_name_type_tree();
        let hints = UnityYamlObjectConversionHints::from_type_tree(
            &type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert_eq!(
            hints.property_name_schemas.get("m_IntegerId"),
            Some(&UnityYamlPropertyNameSchema {
                value_field_name: "Id_Int32".to_owned(),
                value_kind: UnityYamlPropertyNameValueKind::Int32,
            })
        );
        assert_eq!(
            hints.property_name_schemas.get("m_StringId"),
            Some(&UnityYamlPropertyNameSchema {
                value_field_name: "Id_Utf8String".to_owned(),
                value_kind: UnityYamlPropertyNameValueKind::Utf8String,
            })
        );

        let value = SerializedValue::Struct(vec![
            field(
                "m_IntegerId",
                SerializedValue::Struct(vec![field("Id_Int32", SerializedValue::I32(-42))]),
            ),
            field(
                "m_StringId",
                SerializedValue::Struct(vec![field(
                    "Id_Utf8String",
                    SerializedValue::StringBytes(b"Track/Object.Name".to_vec()),
                )]),
            ),
        ]);
        let document = convert_with_object_hints(&value, &hints, |_| {
            unreachable!("PropertyName contains no PPtrs")
        })
        .unwrap();
        assert_eq!(
            &object_field(&document, "m_IntegerId").kind,
            &UnityYamlNodeKind::Scalar(UnityYamlScalar::String("-42".to_owned()))
        );
        assert_eq!(
            &object_field(&document, "m_StringId").kind,
            &UnityYamlNodeKind::Scalar(UnityYamlScalar::String("Track/Object.Name".to_owned()))
        );

        let invalid_value = SerializedValue::Struct(vec![field(
            "m_IntegerId",
            SerializedValue::Struct(vec![field("Id_Int32", SerializedValue::U32(42))]),
        )]);
        assert!(matches!(
            convert_with_object_hints(&invalid_value, &hints, |_| {
                unreachable!("PropertyName contains no PPtrs")
            }),
            Err(UnityYamlObjectConversionError::InvalidPropertyNameValue {
                field_path,
                field_name,
            }) if field_path == "m_IntegerId" && field_name == "Id_Int32"
        ));
    }

    #[test]
    fn property_name_array_hints_match_wildcard_elements() {
        let type_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "vector", "m_Ids", 0),
                type_tree_node(2, "Array", "Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "string", "data", 0),
                type_tree_node(4, "SInt32", "Id_Int32", 0),
            ],
            string_buffer: Vec::new(),
        };
        let hints = UnityYamlObjectConversionHints::from_type_tree(
            &type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert!(hints.property_name_schemas.contains_key("m_Ids[*]"));
        let value = SerializedValue::Struct(vec![field(
            "m_Ids",
            SerializedValue::Array(vec![SerializedValue::Struct(vec![field(
                "Id_Int32",
                SerializedValue::I32(73),
            )])]),
        )]);
        let document = convert_with_object_hints(&value, &hints, |_| {
            unreachable!("PropertyName array contains no PPtrs")
        })
        .unwrap();
        let array = object_field(&document, "m_Ids");
        let UnityYamlNodeKind::Sequence { items, .. } = &array.kind else {
            panic!("PropertyName arrays export as YAML sequences")
        };
        assert_eq!(
            items[0].kind,
            UnityYamlNodeKind::Scalar(UnityYamlScalar::String("73".to_owned()))
        );
    }

    #[test]
    fn scene_object_identifier_uses_project_yaml_target_field_names() {
        let type_tree = scene_object_identifier_type_tree(false);
        let hints = UnityYamlObjectConversionHints::from_type_tree(
            &type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert_eq!(
            hints.scene_object_identifier_schemas.get("m_Identifier"),
            Some(&UnityYamlSceneObjectIdentifierSchema {
                target_object_field_name: "TargetObject".to_owned(),
                target_prefab_field_name: "TargetPrefab".to_owned(),
            })
        );

        let document =
            convert_with_object_hints(&scene_object_identifier_value(123, -456), &hints, |_| {
                unreachable!("SceneObjectIdentifier contains no PPtrs")
            })
            .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(
            output.contains("m_Identifier:\n    targetObject: 123\n    targetPrefab: -456"),
            "{output:?}"
        );
        assert!(!output.contains("TargetObject"));
        assert!(!output.contains("TargetPrefab"));
    }

    #[test]
    fn scene_object_identifier_array_hints_match_wildcard_elements() {
        let type_tree = scene_object_identifier_type_tree(true);
        let hints = UnityYamlObjectConversionHints::from_type_tree(
            &type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert!(hints
            .scene_object_identifier_schemas
            .contains_key("m_Identifiers[*]"));

        let value = SerializedValue::Struct(vec![field(
            "m_Identifiers",
            SerializedValue::Array(vec![SerializedValue::Struct(vec![
                field("TargetObject", SerializedValue::I64(73)),
                field("TargetPrefab", SerializedValue::I64(84)),
            ])]),
        )]);
        let document = convert_with_object_hints(&value, &hints, |_| {
            unreachable!("SceneObjectIdentifier contains no PPtrs")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(output.contains("m_Identifiers:\n  - targetObject: 73\n    targetPrefab: 84"));
    }

    #[test]
    fn malformed_scene_object_identifier_schema_and_value_are_rejected() {
        let malformed = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "SceneObjectIdentifier", "m_Identifier", 0),
                type_tree_node(2, "SInt32", "TargetObject", 0),
                type_tree_node(2, "SInt64", "TargetPrefab", 0),
            ],
            string_buffer: Vec::new(),
        };
        assert!(matches!(
            UnityYamlObjectConversionHints::from_type_tree(
                &malformed,
                version(),
                UnityYamlObjectConversionLimits::default()
            ),
            Err(UnityYamlObjectConversionError::InvalidTypeTree {
                reason: "SceneObjectIdentifier TargetObject is not a signed 64-bit field",
                ..
            })
        ));

        let type_tree = scene_object_identifier_type_tree(false);
        let hints = UnityYamlObjectConversionHints::from_type_tree(
            &type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        let invalid_value = SerializedValue::Struct(vec![field(
            "m_Identifier",
            SerializedValue::Struct(vec![
                field("TargetObject", SerializedValue::U64(123)),
                field("TargetPrefab", SerializedValue::I64(456)),
            ]),
        )]);
        assert!(matches!(
            convert_with_object_hints(&invalid_value, &hints, |_| {
                unreachable!("SceneObjectIdentifier contains no PPtrs")
            }),
            Err(UnityYamlObjectConversionError::InvalidSceneObjectIdentifierValue {
                field_path,
                field_name,
            }) if field_path == "m_Identifier" && field_name == "TargetObject"
        ));
    }

    #[test]
    fn malformed_property_name_type_tree_is_rejected() {
        let malformed = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "PropertyName", "m_Id", 0),
                type_tree_node(2, "float", "value", 0),
            ],
            string_buffer: Vec::new(),
        };
        assert!(matches!(
            UnityYamlObjectConversionHints::from_type_tree(
                &malformed,
                version(),
                UnityYamlObjectConversionLimits::default()
            ),
            Err(UnityYamlObjectConversionError::InvalidTypeTree {
                reason: "PropertyName value must be an Int32 or serialized UTF-8 string",
                ..
            })
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
