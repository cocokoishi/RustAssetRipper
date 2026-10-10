use std::error::Error;
use std::fmt::{self, Display, Formatter};
/// Error produced when a typed value cannot be encoded using its SerializedFile TypeTree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectEncodeError {
    object_index: usize,
    path: String,
    kind: ObjectEncodeErrorKind,
}

impl ObjectEncodeError {
    pub(super) fn new(
        object_index: usize,
        path: impl Into<String>,
        kind: ObjectEncodeErrorKind,
    ) -> Self {
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
