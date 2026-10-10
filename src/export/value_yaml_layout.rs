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
    format_dotnet_f32, format_dotnet_f64, format_hash128, format_unity_guid_words,
    project_pointer_to_unity_yaml, UnityYamlDocument, UnityYamlMappingStyle, UnityYamlNode,
    UnityYamlNodeBuildError, UnityYamlNodeKind, UnityYamlScalar, UnityYamlSequenceStyle,
    UnityYamlWriteError,
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
    /// `serializedVersion` values emitted for versioned nested TypeTree structs.
    pub serialized_version_paths: HashMap<String, i32>,
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
    /// Preserve names from a recovered TypeTree when the source factory materialized the
    /// corresponding generated engine class, whose editor walker uses its generated members.
    pub preserve_type_tree_field_names: bool,
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
    Hash128Overflow {
        field_path: String,
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
            Self::Hash128Overflow { field_path } => write!(
                f,
                "Hash128 at {field_path:?} overflows the baseline's checked 32-bit arithmetic"
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
    let hash128_root_schema = hash128_schema_for_path("", hints, limits)?;
    let mut body = if let Some(schema) = scene_object_identifier_schema {
        convert_scene_object_identifier_value(fields, schema, "", 0, &mut budget)?
    } else if let Some(schema) = hash128_root_schema {
        convert_hash128_value(fields, schema, "", 0, limits, &mut budget)?
    } else {
        let mut body = UnityYamlNode::mapping(if is_flow_mapped_path("", hints, limits)? {
            UnityYamlMappingStyle::Flow
        } else {
            UnityYamlMappingStyle::Block
        });
        if !stripped && serialized_version > 1 {
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
        if field_mode == UnityYamlObjectFieldMode::ProjectAsset
            && !stripped
            && has_mono_behaviour_yaml_header(&body)
        {
            flatten_mapping_field(&mut body, "m_Structure")?;
            insert_material_editor_fields(
                &mut body,
                &[
                    "m_ObjectHideFlags",
                    "m_CorrespondingSourceObject",
                    "m_PrefabInstance",
                    "m_PrefabAsset",
                ],
            )?;
            insert_mapping_field_before(
                &mut body,
                "m_EditorHideFlags",
                "m_Script",
                UnityYamlNode::scalar(UnityYamlScalar::Signed(0)),
            )?;
            insert_material_field_after(
                &mut body,
                "m_EditorClassIdentifier",
                "m_Name",
                UnityYamlNode::string(String::new()),
            )?;
        } else {
            move_mapping_field_to_end(&mut body, "m_Structure")?;
        }
    }
    if source.class_id == 21 && field_mode == UnityYamlObjectFieldMode::ProjectAsset && !stripped {
        match (unity_version.major, unity_version.minor) {
            (2017, 4) => insert_material_editor_fields(
                &mut body,
                &[
                    "m_ObjectHideFlags",
                    "m_PrefabParentObject",
                    "m_PrefabInternal",
                ],
            )?,
            (2018, 2) => insert_material_editor_fields(
                &mut body,
                &[
                    "m_ObjectHideFlags",
                    "m_CorrespondingSourceObject",
                    "m_PrefabInternal",
                ],
            )?,
            (2018, 3) | (2019, 4) => insert_material_editor_fields(
                &mut body,
                &[
                    "m_ObjectHideFlags",
                    "m_CorrespondingSourceObject",
                    "m_PrefabInstance",
                    "m_PrefabAsset",
                ],
            )?,
            (2022, 3) => insert_late_material_editor_fields(&mut body, false)?,
            (6000, 4) => insert_late_material_editor_fields(&mut body, true)?,
            _ => {}
        }
    }
    UnityYamlDocument::object(source.class_id, file_id, class_name, body, stripped)
        .map_err(UnityYamlObjectConversionError::NodeBuild)
}

fn insert_material_editor_fields(
    body: &mut UnityYamlNode,
    editor_fields: &[&str],
) -> Result<(), UnityYamlObjectConversionError> {
    let UnityYamlNodeKind::Mapping { style, .. } = &body.kind else {
        return Err(UnityYamlObjectConversionError::NodeBuild(
            UnityYamlNodeBuildError::NotAMapping,
        ));
    };
    let style = *style;
    let previous = std::mem::replace(
        &mut body.kind,
        UnityYamlNodeKind::Mapping {
            style,
            entries: Vec::new(),
        },
    );
    let UnityYamlNodeKind::Mapping { style, mut entries } = previous else {
        return Err(UnityYamlObjectConversionError::NodeBuild(
            UnityYamlNodeBuildError::NotAMapping,
        ));
    };
    let requested = entries.len().checked_add(editor_fields.len()).ok_or(
        UnityYamlObjectConversionError::AllocationFailed {
            resource: "Material Editor YAML fields",
            requested: usize::MAX,
        },
    )?;
    let mut ordered = Vec::new();
    ordered.try_reserve_exact(requested).map_err(|_| {
        UnityYamlObjectConversionError::AllocationFailed {
            resource: "Material Editor YAML fields",
            requested,
        }
    })?;

    if let Some(entry) = take_yaml_mapping_field(&mut entries, "serializedVersion") {
        ordered.push(entry);
    }
    for &field in editor_fields {
        ordered.push(take_yaml_mapping_field(&mut entries, field).unwrap_or((
            UnityYamlNode::plain_string(field),
            match field {
                "m_ObjectHideFlags" => UnityYamlNode::scalar(UnityYamlScalar::Unsigned(0)),
                _ => {
                    let mut pointer = UnityYamlNode::mapping(UnityYamlMappingStyle::Flow);
                    pointer
                        .insert_field("fileID", UnityYamlNode::scalar(UnityYamlScalar::Signed(0)))
                        .map_err(UnityYamlObjectConversionError::NodeBuild)?;
                    pointer
                }
            },
        )));
    }
    ordered.append(&mut entries);
    body.kind = UnityYamlNodeKind::Mapping {
        style,
        entries: ordered,
    };
    Ok(())
}

fn insert_late_material_editor_fields(
    body: &mut UnityYamlNode,
    include_allow_locking: bool,
) -> Result<(), UnityYamlObjectConversionError> {
    insert_material_editor_fields(
        body,
        &[
            "m_ObjectHideFlags",
            "m_CorrespondingSourceObject",
            "m_PrefabInstance",
            "m_PrefabAsset",
        ],
    )?;
    insert_material_field_after(body, "m_Parent", "m_Shader", material_null_pointer()?)?;
    insert_material_field_after(
        body,
        "m_ModifiedSerializedProperties",
        "m_Parent",
        UnityYamlNode::scalar(UnityYamlScalar::Unsigned(0)),
    )?;
    insert_material_field_after(
        body,
        "m_LockedProperties",
        "disabledShaderPasses",
        UnityYamlNode::plain_string(""),
    )?;
    if include_allow_locking {
        insert_material_field_after(
            body,
            "m_AllowLocking",
            "m_BuildTextureStacks",
            UnityYamlNode::scalar(UnityYamlScalar::Boolean(false)),
        )?;
    }
    Ok(())
}

fn material_null_pointer() -> Result<UnityYamlNode, UnityYamlObjectConversionError> {
    let mut pointer = UnityYamlNode::mapping(UnityYamlMappingStyle::Flow);
    pointer
        .insert_field("fileID", UnityYamlNode::scalar(UnityYamlScalar::Signed(0)))
        .map_err(UnityYamlObjectConversionError::NodeBuild)?;
    Ok(pointer)
}

fn insert_material_field_after(
    body: &mut UnityYamlNode,
    field_name: &str,
    anchor_name: &str,
    default_value: UnityYamlNode,
) -> Result<(), UnityYamlObjectConversionError> {
    let UnityYamlNodeKind::Mapping { entries, .. } = &mut body.kind else {
        return Err(UnityYamlObjectConversionError::NodeBuild(
            UnityYamlNodeBuildError::NotAMapping,
        ));
    };
    let field = take_yaml_mapping_field(entries, field_name)
        .unwrap_or((UnityYamlNode::plain_string(field_name), default_value));
    let Some(anchor_index) = entries.iter().position(|(key, _)| {
        matches!(
            &key.kind,
            UnityYamlNodeKind::Scalar(
                UnityYamlScalar::PlainString(value) | UnityYamlScalar::String(value)
            ) if value == anchor_name
        )
    }) else {
        return Err(UnityYamlObjectConversionError::InvalidTypeTree {
            node_index: 0,
            reason: "missing Unity YAML editor-field insertion anchor",
        });
    };
    entries
        .try_reserve(1)
        .map_err(|_| UnityYamlObjectConversionError::AllocationFailed {
            resource: "Material Editor YAML fields",
            requested: entries.len().saturating_add(1),
        })?;
    entries.insert(anchor_index + 1, field);
    Ok(())
}

fn insert_mapping_field_before(
    body: &mut UnityYamlNode,
    field_name: &str,
    anchor_name: &str,
    default_value: UnityYamlNode,
) -> Result<(), UnityYamlObjectConversionError> {
    let UnityYamlNodeKind::Mapping { entries, .. } = &mut body.kind else {
        return Err(UnityYamlObjectConversionError::NodeBuild(
            UnityYamlNodeBuildError::NotAMapping,
        ));
    };
    let field = take_yaml_mapping_field(entries, field_name)
        .unwrap_or((UnityYamlNode::plain_string(field_name), default_value));
    let Some(anchor_index) = entries.iter().position(|(key, _)| {
        matches!(
            &key.kind,
            UnityYamlNodeKind::Scalar(
                UnityYamlScalar::PlainString(value) | UnityYamlScalar::String(value)
            ) if value == anchor_name
        )
    }) else {
        return Err(UnityYamlObjectConversionError::InvalidTypeTree {
            node_index: 0,
            reason: "missing Unity YAML editor-field insertion anchor",
        });
    };
    entries
        .try_reserve(1)
        .map_err(|_| UnityYamlObjectConversionError::AllocationFailed {
            resource: "Unity YAML editor fields",
            requested: entries.len().saturating_add(1),
        })?;
    entries.insert(anchor_index, field);
    Ok(())
}

fn has_mono_behaviour_yaml_header(body: &UnityYamlNode) -> bool {
    let UnityYamlNodeKind::Mapping { entries, .. } = &body.kind else {
        return false;
    };
    ["m_GameObject", "m_Script"].into_iter().all(|field_name| {
        entries.iter().any(|(key, value)| {
            matches!(
                &key.kind,
                UnityYamlNodeKind::Scalar(
                    UnityYamlScalar::PlainString(name) | UnityYamlScalar::String(name)
                ) if name == field_name
            ) && matches!(&value.kind, UnityYamlNodeKind::Mapping { .. })
        })
    })
}
