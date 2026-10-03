//! Bounded conversion from decoded SerializedValue trees to Unity YAML object documents.

use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::str;

use crate::processing::scene_naming::SceneUnityVersion;
use crate::serialized_file::object::{SerializedFieldValue, SerializedValue};

use super::pointer_map::ProjectPointerResolution;
use super::scene_collection::ExportAssetIdentity;
use super::unity_yaml::{
    project_pointer_to_unity_yaml, UnityYamlDocument, UnityYamlMappingStyle, UnityYamlNode,
    UnityYamlNodeBuildError, UnityYamlScalar, UnityYamlSequenceStyle, UnityYamlWriteError,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnityYamlEmptyArrayKind {
    /// Baseline integer/bool/char arrays are flattened to a reverse-endian hexadecimal scalar.
    IntegerHex,
    BlockSequence,
    BlockCurveSequence,
    FlowSequence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnityYamlObjectConversionLimits {
    pub max_values: usize,
    pub max_depth: usize,
    pub max_field_path_bytes: usize,
    pub max_generated_scalar_bytes: usize,
}

impl Default for UnityYamlObjectConversionLimits {
    fn default() -> Self {
        Self {
            max_values: 20_000_000,
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
    empty_array_hints: &HashMap<String, UnityYamlEmptyArrayKind>,
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

    let mut body = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
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
        empty_array_hints,
        unity_version,
        missing_reference_guid,
        limits,
        &mut budget,
        &mut resolve_pointer,
    )?;
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
    empty_array_hints: &HashMap<String, UnityYamlEmptyArrayKind>,
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
        if field.name == "Base" {
            if let SerializedValue::Struct(base_fields) = &field.value {
                budget.visit(depth)?;
                append_struct_fields(
                    destination,
                    base_fields,
                    &field_path,
                    depth + 1,
                    source,
                    empty_array_hints,
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
            empty_array_hints,
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
    empty_array_hints: &HashMap<String, UnityYamlEmptyArrayKind>,
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
            empty_array_hints,
            unity_version,
            missing_reference_guid,
            limits,
            budget,
            resolve_pointer,
        )?,
        SerializedValue::Struct(fields) => {
            let mut mapping = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
            append_struct_fields(
                &mut mapping,
                fields,
                field_path,
                depth + 1,
                source,
                empty_array_hints,
                unity_version,
                missing_reference_guid,
                limits,
                budget,
                resolve_pointer,
            )?;
            mapping
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

#[allow(clippy::too_many_arguments)]
fn convert_array<F>(
    items: &[SerializedValue],
    field_path: &str,
    depth: usize,
    source: ExportAssetIdentity,
    empty_array_hints: &HashMap<String, UnityYamlEmptyArrayKind>,
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
        let kind = empty_array_hints.get(field_path).copied().ok_or_else(|| {
            UnityYamlObjectConversionError::MissingEmptyArrayHint {
                field_path: field_path.to_owned(),
            }
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

    if let Some(kind) = empty_array_hints.get(field_path) {
        if *kind == UnityYamlEmptyArrayKind::IntegerHex {
            return convert_integer_array(items, field_path, depth, limits, budget);
        }
    } else if integer_array_width(&items[0]).is_some() {
        return convert_integer_array(items, field_path, depth, limits, budget);
    }

    let style = match empty_array_hints.get(field_path) {
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
                empty_array_hints,
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
        serialized_object_value_to_unity_yaml_document(
            source(),
            "GameObject",
            6,
            1_000_001,
            false,
            root,
            hints,
            version(),
            None,
            UnityYamlObjectConversionLimits::default(),
            resolver,
        )
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
                    class_id: 21,
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
