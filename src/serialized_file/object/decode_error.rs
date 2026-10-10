use std::error::Error;
use std::fmt::{self, Display, Formatter};

use super::super::header::HeaderError;
use crate::io::binary::BinaryError;
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectDecodeError {
    offset: usize,
    kind: ObjectDecodeErrorKind,
}

impl ObjectDecodeError {
    pub(crate) fn new(offset: usize, kind: ObjectDecodeErrorKind) -> Self {
        Self { offset, kind }
    }

    pub(super) fn from_binary(error: BinaryError) -> Self {
        let offset = error.offset();
        Self::new(offset, ObjectDecodeErrorKind::Binary(error))
    }

    pub(super) fn from_header(error: HeaderError) -> Self {
        let offset = error.offset();
        Self::new(offset, ObjectDecodeErrorKind::Header(error))
    }

    pub(crate) fn source_type_tree_lookup(
        object_index: usize,
        class_id: i32,
        version: String,
        message: String,
    ) -> Self {
        Self::new(
            0,
            ObjectDecodeErrorKind::SourceTypeTreeLookup {
                object_index,
                class_id,
                version,
                message,
            },
        )
    }

    pub(crate) fn factory_object_outcome(
        object_index: usize,
        class_id: i32,
        outcome: String,
        diagnostic: Option<String>,
    ) -> Self {
        Self::new(
            0,
            ObjectDecodeErrorKind::FactoryObjectOutcome {
                object_index,
                class_id,
                outcome,
                diagnostic,
            },
        )
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn kind(&self) -> &ObjectDecodeErrorKind {
        &self.kind
    }
}

impl Display for ObjectDecodeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ObjectDecodeErrorKind::Binary(error) => Display::fmt(error, f),
            ObjectDecodeErrorKind::Header(error) => Display::fmt(error, f),
            kind => write!(f, "{kind} at byte offset {}", self.offset),
        }
    }
}

impl Error for ObjectDecodeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            ObjectDecodeErrorKind::Binary(error) => Some(error),
            ObjectDecodeErrorKind::Header(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObjectDecodeErrorKind {
    Binary(BinaryError),
    Header(HeaderError),
    InvalidObjectIndex {
        object_index: usize,
        object_count: usize,
    },
    ObjectTypeUnresolved {
        object_index: usize,
    },
    /// The source factory retained this object as unknown or unreadable raw data.
    FactoryObjectOutcome {
        object_index: usize,
        class_id: i32,
        outcome: String,
        diagnostic: Option<String>,
    },
    /// The file declares a Unity version before 3.5, whose assets the source
    /// does not read.
    UnreadableUnsupportedVersion {
        object_index: usize,
    },
    InvalidResolvedTypeIndex {
        object_index: usize,
        type_index: usize,
        type_count: usize,
    },
    MissingTypeTree {
        object_index: usize,
    },
    SourceTypeTreeLookup {
        object_index: usize,
        class_id: i32,
        version: String,
        message: String,
    },
    EmptyTypeTree {
        object_index: usize,
    },
    UnsupportedManagedReferences {
        object_index: usize,
        type_name: String,
        field_name: String,
        /// The legacy `id` flavor (`IsIndexedManagedReference`) versus `rid`-addressed.
        is_indexed: bool,
    },
    InvalidManagedReferenceId {
        object_index: usize,
        path: String,
        indexed: bool,
        value: String,
    },
    ManagedRegistryDecode {
        object_index: usize,
        path: String,
        message: String,
    },
    UnsupportedReferencedObjectData {
        object_index: usize,
        path: String,
    },
    ManagedRegistryDepthExceeded {
        object_index: usize,
        path: String,
        requested: usize,
        limit: usize,
    },
    InvalidPPtrComponent {
        object_index: usize,
        path: String,
        field_name: String,
        type_name: String,
    },
    ObjectDataRangeOverflow {
        object_index: usize,
    },
    ObjectDataOutOfBounds {
        object_index: usize,
        start: u64,
        end: u64,
        file_size: usize,
    },
    ObjectDataReadFailed {
        object_index: usize,
        offset: u64,
        message: String,
    },
    InvalidTypeTreeNodeIndex {
        node_index: usize,
    },
    InvalidTypeTreeLevels {
        object_index: usize,
        node_index: usize,
        child_index: usize,
    },
    TypeTreeDepthExceeded {
        object_index: usize,
        path: String,
        depth: usize,
    },
    InvalidArrayShape {
        object_index: usize,
        path: String,
        node_index: usize,
    },
    NegativeArrayLength {
        object_index: usize,
        path: String,
        length: i32,
    },
    LengthExceedsPayload {
        object_index: usize,
        path: String,
        length: usize,
        remaining: usize,
    },
    ArraySizeOverflow {
        object_index: usize,
        path: String,
        count: usize,
        minimum_element_size: usize,
    },
    ArrayExceedsPayload {
        object_index: usize,
        path: String,
        count: usize,
        minimum_element_size: usize,
        required: usize,
        remaining: usize,
    },
    ZeroWidthArrayElement {
        object_index: usize,
        path: String,
        count: usize,
    },
    UnknownLeafWithoutSize {
        object_index: usize,
        path: String,
        type_name: String,
        byte_size: i32,
    },
    DecodeLimitExceeded {
        resource: String,
        requested: usize,
        limit: usize,
    },
    AllocationSizeOverflow {
        resource: String,
    },
    AllocationFailed {
        path: String,
        count: usize,
    },
    LayoutOverflow,
}

impl Display for ObjectDecodeErrorKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Binary(error) => Display::fmt(error, f),
            Self::Header(error) => Display::fmt(error, f),
            Self::InvalidObjectIndex {
                object_index,
                object_count,
            } => write!(
                f,
                "object index {object_index} is outside the {object_count}-entry object table"
            ),
            Self::ObjectTypeUnresolved { object_index } => {
                write!(f, "object {object_index} has no resolved serialized type")
            }
            Self::FactoryObjectOutcome {
                object_index,
                class_id,
                outcome,
                diagnostic,
            } => {
                write!(f, "object {object_index} class {class_id} produced {outcome}")?;
                if let Some(diagnostic) = diagnostic {
                    write!(f, ": {diagnostic}")?;
                }
                Ok(())
            }
            Self::UnreadableUnsupportedVersion { object_index } => write!(
                f,
                "object {object_index} comes from a Unity version before 3.5 and cannot be read"
            ),
            Self::InvalidResolvedTypeIndex {
                object_index,
                type_index,
                type_count,
            } => write!(
                f,
                "object {object_index} resolves to type {type_index}, outside the {type_count}-entry type table"
            ),
            Self::MissingTypeTree { object_index } => {
                write!(f, "object {object_index} has no TypeTree")
            }
            Self::SourceTypeTreeLookup {
                object_index,
                class_id,
                version,
                message,
            } => write!(
                f,
                "object {object_index} class {class_id} has no usable source TPK TypeTree at {version}: {message}"
            ),
            Self::EmptyTypeTree { object_index } => {
                write!(f, "object {object_index} references an empty TypeTree")
            }
            Self::UnsupportedManagedReferences {
                object_index,
                type_name,
                field_name,
                is_indexed,
            } => write!(
                f,
                "object {object_index} contains {} managed-reference TypeTree node {type_name} {field_name} without an ID child",
                if *is_indexed {
                    "legacy indexed"
                } else {
                    "rid-addressed"
                }
            ),
            Self::InvalidManagedReferenceId {
                object_index,
                path,
                indexed,
                value,
            } => write!(
                f,
                "object {object_index} field {path} has {value} for a {} managed-reference identifier",
                if *indexed { "legacy indexed" } else { "rid-addressed" }
            ),
            Self::ManagedRegistryDecode {
                object_index,
                path,
                message,
            } => write!(
                f,
                "object {object_index} managed-reference registry at {path} could not be decoded: {message}"
            ),
            Self::UnsupportedReferencedObjectData { object_index, path } => write!(
                f,
                "object {object_index} field {path} contains an unresolved ReferencedObjectData placeholder"
            ),
            Self::ManagedRegistryDepthExceeded {
                object_index,
                path,
                requested,
                limit,
            } => write!(
                f,
                "object {object_index} field {path} nests managed-reference registry depth {requested}, exceeding limit {limit}"
            ),
            Self::InvalidPPtrComponent {
                object_index,
                path,
                field_name,
                type_name,
            } => write!(
                f,
                "object {object_index} field {path} has unsupported PPtr component {field_name} of type {type_name}"
            ),
            Self::ObjectDataRangeOverflow { object_index } => {
                write!(f, "object {object_index} data range overflows the address space")
            }
            Self::ObjectDataOutOfBounds {
                object_index,
                start,
                end,
                file_size,
            } => write!(
                f,
                "object {object_index} payload range {start}..{end} exceeds input size {file_size}"
            ),
            Self::ObjectDataReadFailed {
                object_index,
                offset,
                message,
            } => write!(
                f,
                "object {object_index} payload read failed at byte offset {offset}: {message}"
            ),
            Self::InvalidTypeTreeNodeIndex { node_index } => {
                write!(f, "TypeTree node index {node_index} is out of bounds")
            }
            Self::InvalidTypeTreeLevels {
                object_index,
                node_index,
                child_index,
            } => write!(
                f,
                "object {object_index} TypeTree node {child_index} has an invalid level relative to node {node_index}"
            ),
            Self::TypeTreeDepthExceeded {
                object_index,
                path,
                depth,
            } => write!(
                f,
                "object {object_index} field {path} exceeds TypeTree decode depth at {depth}"
            ),
            Self::InvalidArrayShape {
                object_index,
                path,
                node_index,
            } => write!(
                f,
                "object {object_index} field {path} has an invalid array node at index {node_index}"
            ),
            Self::NegativeArrayLength {
                object_index,
                path,
                length,
            } => write!(
                f,
                "object {object_index} field {path} has negative array/string length {length}"
            ),
            Self::LengthExceedsPayload {
                object_index,
                path,
                length,
                remaining,
            } => write!(
                f,
                "object {object_index} field {path} requires {length} bytes, but only {remaining} remain"
            ),
            Self::ArraySizeOverflow {
                object_index,
                path,
                count,
                minimum_element_size,
            } => write!(
                f,
                "object {object_index} field {path} array size overflows for {count} elements of minimum size {minimum_element_size}"
            ),
            Self::ArrayExceedsPayload {
                object_index,
                path,
                count,
                minimum_element_size,
                required,
                remaining,
            } => write!(
                f,
                "object {object_index} field {path} array count {count} requires at least {required} bytes ({minimum_element_size} per element), but only {remaining} remain"
            ),
            Self::ZeroWidthArrayElement {
                object_index,
                path,
                count,
            } => write!(
                f,
                "object {object_index} field {path} has {count} array elements with no bounded serialized width"
            ),
            Self::UnknownLeafWithoutSize {
                object_index,
                path,
                type_name,
                byte_size,
            } => write!(
                f,
                "object {object_index} field {path} has unknown leaf type {type_name} with invalid byte size {byte_size}"
            ),
            Self::DecodeLimitExceeded {
                resource,
                requested,
                limit,
            } => write!(
                f,
                "object payload decode requires {requested} units for {resource}, above the configured limit {limit}"
            ),
            Self::AllocationSizeOverflow { resource } => {
                write!(f, "decoded allocation size overflows while reserving {resource}")
            }
            Self::AllocationFailed { path, count } => {
                write!(f, "could not reserve {count} values for field {path}")
            }
            Self::LayoutOverflow => write!(f, "object payload layout arithmetic overflowed"),
        }
    }
}
