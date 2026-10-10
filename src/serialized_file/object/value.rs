use std::error::Error;
use std::fmt::{self, Display, Formatter};
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedObjectPayload {
    pub object_index: usize,
    pub path_id: i64,
    pub type_name: String,
    pub root: SerializedValue,
    pub consumed_bytes: usize,
    /// Bytes after the fields described by the TypeTree remain visible here.
    pub trailing_bytes: Vec<u8>,
    pub allocated_bytes: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SerializedFieldValue {
    pub name: String,
    pub value: SerializedValue,
}

/// Typed field data decoded directly from a SerializedFile object payload.
#[derive(Clone, Debug, PartialEq)]
pub enum SerializedValue {
    Bool(bool),
    I8(i8),
    U8(u8),
    I16(i16),
    U16(u16),
    Char(u16),
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
    F32(f32),
    F64(f64),
    /// Raw UTF-8 bytes are retained so invalid or non-canonical source strings are not lost.
    StringBytes(Vec<u8>),
    Bytes(Vec<u8>),
    Array(Vec<SerializedValue>),
    Struct(Vec<SerializedFieldValue>),
    /// A Unity object reference retaining its file-table index and signed PathID.
    PPtr {
        file_id: i32,
        path_id: i64,
    },
    /// A SerializeReference identifier. Its registry lives on the owning serialized object;
    /// keeping the identifier as an edge avoids recursive expansion and preserves cycles.
    ManagedReferenceId {
        reference_id: i64,
        indexed: bool,
    },
    /// The dynamically typed SerializeReference registry attached to one serialized object.
    ManagedReferenceRegistry(Box<super::super::managed_registry::ManagedRegistryRead>),
    /// Unknown leaf data with a nonnegative TypeTree byte size is preserved without decoding.
    Raw {
        type_name: String,
        bytes: Vec<u8>,
    },
}

impl SerializedValue {
    /// Returns the ordered fields when this value is a struct.
    pub fn struct_fields(&self) -> Option<&[SerializedFieldValue]> {
        match self {
            Self::Struct(fields) => Some(fields),
            _ => None,
        }
    }

    /// Finds a field only when its name is unique in this struct.
    ///
    /// Duplicate names are preserved by TypeTree decoding and reported instead of silently
    /// selecting one. Non-struct values are also reported distinctly from a missing field.
    pub fn unique_field(
        &self,
        name: &str,
    ) -> Result<Option<&SerializedValue>, SerializedValueLookupError> {
        let Self::Struct(fields) = self else {
            return Err(SerializedValueLookupError::NotStruct);
        };
        let mut matching = fields.iter().filter(|field| field.name == name);
        let Some(first) = matching.next() else {
            return Ok(None);
        };
        if matching.next().is_some() {
            let match_count = fields.iter().filter(|field| field.name == name).count();
            return Err(SerializedValueLookupError::DuplicateField {
                name: name.to_owned(),
                match_count,
            });
        }
        Ok(Some(&first.value))
    }

    /// Finds a field in this struct and nested `Base` structs without discarding duplicates.
    ///
    /// Unity TypeTrees may encode inherited object fields below a field named `Base`; following
    /// only that chain avoids searching unrelated nested resource records or array elements.
    pub fn unique_field_in_base_chain(
        &self,
        name: &str,
    ) -> Result<Option<&SerializedValue>, SerializedValueLookupError> {
        if !matches!(self, Self::Struct(_)) {
            return Err(SerializedValueLookupError::NotStruct);
        }
        let mut scope = self;
        let mut found = None;
        while let Self::Struct(_) = scope {
            if let Some(value) = scope.unique_field(name)? {
                if found.is_some() {
                    return Err(SerializedValueLookupError::DuplicateField {
                        name: name.to_owned(),
                        match_count: 2,
                    });
                }
                found = Some(value);
            }
            let Some(base) = scope.unique_field("Base")? else {
                break;
            };
            if !matches!(base, Self::Struct(_)) {
                break;
            }
            scope = base;
        }
        Ok(found)
    }

    /// Finds one field in this struct and nested `Base` structs for a controlled value update.
    /// Duplicate fields and duplicate matches across the base chain are rejected.
    pub fn unique_field_in_base_chain_mut(
        &mut self,
        name: &str,
    ) -> Result<Option<&mut SerializedValue>, SerializedValueMutationError> {
        if !matches!(self, Self::Struct(_)) {
            return Err(SerializedValueMutationError::NotStruct);
        }

        let mut current_path = Vec::<usize>::new();
        let mut found_path = None::<Vec<usize>>;
        let mut current: &SerializedValue = self;
        while let Self::Struct(fields) = current {
            if let Some(field_index) = unique_field_index(fields, name)? {
                if found_path.is_some() {
                    return Err(SerializedValueMutationError::DuplicateField {
                        name: name.to_owned(),
                        match_count: 2,
                    });
                }
                let path_length = current_path
                    .len()
                    .checked_add(1)
                    .ok_or(SerializedValueMutationError::FieldPathDepthOverflow)?;
                let mut path = Vec::new();
                path.try_reserve_exact(path_length).map_err(|_| {
                    SerializedValueMutationError::AllocationFailed {
                        requested: path_length,
                    }
                })?;
                path.extend_from_slice(&current_path);
                path.push(field_index);
                found_path = Some(path);
            }

            let Some(base_index) = unique_field_index(fields, "Base")? else {
                break;
            };
            let base = &fields[base_index].value;
            if !matches!(base, Self::Struct(_)) {
                break;
            }
            let requested_path_depth = current_path
                .len()
                .checked_add(1)
                .ok_or(SerializedValueMutationError::FieldPathDepthOverflow)?;
            current_path.try_reserve(1).map_err(|_| {
                SerializedValueMutationError::AllocationFailed {
                    requested: requested_path_depth,
                }
            })?;
            current_path.push(base_index);
            current = base;
        }

        let Some(path) = found_path else {
            return Ok(None);
        };
        let mut target = self;
        for field_index in path {
            let Self::Struct(fields) = target else {
                return Err(SerializedValueMutationError::InvalidFieldPath);
            };
            let Some(field) = fields.get_mut(field_index) else {
                return Err(SerializedValueMutationError::InvalidFieldPath);
            };
            target = &mut field.value;
        }
        Ok(Some(target))
    }

    /// Returns the ordered elements when this value is an array.
    pub fn array_elements(&self) -> Option<&[SerializedValue]> {
        match self {
            Self::Array(values) => Some(values),
            _ => None,
        }
    }

    /// Returns the array elements mutably while leaving their order and values intact.
    pub fn array_elements_mut(&mut self) -> Option<&mut [SerializedValue]> {
        match self {
            Self::Array(values) => Some(values.as_mut_slice()),
            _ => None,
        }
    }

    /// Appends one value to an array after checking both the caller's element bound and capacity.
    pub fn push_array_element_bounded(
        &mut self,
        value: SerializedValue,
        maximum_elements: usize,
    ) -> Result<(), SerializedValueMutationError> {
        let Self::Array(values) = self else {
            return Err(SerializedValueMutationError::NotArray);
        };
        let requested = values
            .len()
            .checked_add(1)
            .ok_or(SerializedValueMutationError::ArrayElementCountOverflow)?;
        if requested > maximum_elements {
            return Err(SerializedValueMutationError::ArrayElementLimitExceeded {
                requested,
                limit: maximum_elements,
            });
        }
        values
            .try_reserve(1)
            .map_err(|_| SerializedValueMutationError::AllocationFailed { requested })?;
        values.push(value);
        Ok(())
    }

    /// Returns the raw file-table index and PathID when this value is a PPtr.
    pub fn pptr(&self) -> Option<(i32, i64)> {
        match self {
            Self::PPtr { file_id, path_id } => Some((*file_id, *path_id)),
            _ => None,
        }
    }
}

fn unique_field_index(
    fields: &[SerializedFieldValue],
    name: &str,
) -> Result<Option<usize>, SerializedValueMutationError> {
    let mut matching = fields
        .iter()
        .enumerate()
        .filter(|(_, field)| field.name == name);
    let Some((index, _)) = matching.next() else {
        return Ok(None);
    };
    if matching.next().is_some() {
        let match_count = fields.iter().filter(|field| field.name == name).count();
        return Err(SerializedValueMutationError::DuplicateField {
            name: name.to_owned(),
            match_count,
        });
    }
    Ok(Some(index))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SerializedValueLookupError {
    NotStruct,
    DuplicateField { name: String, match_count: usize },
}

impl Display for SerializedValueLookupError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotStruct => f.write_str("field lookup requires a struct value"),
            Self::DuplicateField { name, match_count } => write!(
                f,
                "field '{name}' occurs {match_count} times in the decoded struct"
            ),
        }
    }
}

impl Error for SerializedValueLookupError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SerializedValueMutationError {
    NotStruct,
    NotArray,
    DuplicateField { name: String, match_count: usize },
    FieldPathDepthOverflow,
    InvalidFieldPath,
    ArrayElementCountOverflow,
    ArrayElementLimitExceeded { requested: usize, limit: usize },
    AllocationFailed { requested: usize },
}

impl Display for SerializedValueMutationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotStruct => f.write_str("field mutation requires a struct value"),
            Self::NotArray => f.write_str("array append requires an array value"),
            Self::DuplicateField { name, match_count } => write!(
                f,
                "field '{name}' occurs {match_count} times along the decoded base chain"
            ),
            Self::FieldPathDepthOverflow => f.write_str("field path depth overflows usize"),
            Self::InvalidFieldPath => f.write_str("decoded field path changed during mutation"),
            Self::ArrayElementCountOverflow => f.write_str("array element count overflows usize"),
            Self::ArrayElementLimitExceeded { requested, limit } => write!(
                f,
                "array append would create {requested} elements, exceeding the limit of {limit}"
            ),
            Self::AllocationFailed { requested } => write!(
                f,
                "could not reserve capacity for {requested} serialized value entries"
            ),
        }
    }
}

impl Error for SerializedValueMutationError {}
