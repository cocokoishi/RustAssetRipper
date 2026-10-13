use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use super::super::scene_collection::ExportAssetIdentity;
use super::super::unity_yaml::{UnityYamlNodeBuildError, UnityYamlWriteError};
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
    /// Paths (`[*]` for array elements) whose `TypelessData`-typed values expand
    /// into Unity's project-YAML shape (`<name>: <byte count>` plus a
    /// `_typelessdata` payload), because the TPK tree represents both members as
    /// one `Bytes` value. Observed on Mesh `m_DataSize`, Texture2D `image data`,
    /// and Sprite render-data vertex blobs.
    pub typeless_data_expansion_paths: HashSet<String>,
    /// Paths (`[*]` for array elements) whose `pair`-typed values render as keyed
    /// single-pair mappings (`- key: value`) instead of `first`/`second` structs.
    /// The frozen walker emits this shape for pairs with valid dictionary-key
    /// first fields in any position (observed on ComputeShader variant maps and
    /// Sprite render-data keys); pairs with other key types keep struct rendering.
    /// Pairs owned by `map` nodes record here too but always convert through the
    /// dictionary branch.
    pub keyed_pair_element_paths: HashSet<String>,
    pub hash128_schemas: HashMap<String, UnityYamlHash128Schema>,
    pub guid_schemas: HashMap<String, UnityYamlGuidSchema>,
    pub property_name_schemas: HashMap<String, UnityYamlPropertyNameSchema>,
    pub scene_object_identifier_schemas: HashMap<String, UnityYamlSceneObjectIdentifierSchema>,
    pub field_name_aliases: HashMap<String, String>,
    /// Preserve names from a recovered TypeTree when the source factory materialized the
    /// corresponding generated engine class, whose editor walker uses its generated members.
    pub preserve_type_tree_field_names: bool,
    /// Declared class name of a generated native object when its TypeTree reuses
    /// an abstract base layout. TypeTreeObject fallbacks deliberately leave this
    /// unset because their source ClassName comes from the editor schema.
    pub native_class_name: Option<String>,
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
    InvalidPairElement {
        field_path: String,
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
            Self::InvalidPairElement { field_path } => write!(
                f,
                "Unity pair array element at {field_path:?} does not have ordered first and second fields"
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
