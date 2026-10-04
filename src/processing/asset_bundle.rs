//! AssetBundle naming and Container pointer resolution.

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::str;

use crate::serialized_file::metadata::SerializedFileMetadata;
use crate::serialized_file::object::{SerializedValue, SerializedValueLookupError};
use crate::serialized_file::reference_graph::ObjectReferenceGraph;
use crate::serialized_file::references::PPtrResolution;

use super::scene_collections::AssetBundleContainerReference;

/// Resolve `GetAssetBundleName`: a non-empty serialized name wins, otherwise use the owning
/// source Bundle's name. The returned value is the unmodified name, before the processor's
/// `.bundle` suffix removal.
pub fn get_asset_bundle_name(
    root: &SerializedValue,
    source_bundle_name: &str,
) -> Result<String, AssetBundleNameDecodeError> {
    let mut selected = None;
    let aliases = [
        "m_AssetBundleName_R",
        "AssetBundleName_R",
        "m_AssetBundleName",
        "AssetBundleName",
    ];
    for alias in aliases {
        let value = root.unique_field_in_base_chain(alias).map_err(|source| {
            AssetBundleNameDecodeError::FieldLookup {
                field_name: alias,
                source,
            }
        })?;
        if let Some(value) = value {
            if let Some((first_alias, _)) = selected {
                return Err(AssetBundleNameDecodeError::DuplicateAliases {
                    first: first_alias,
                    second: alias,
                });
            }
            selected = Some((alias, value));
        }
    }

    let Some((field_name, SerializedValue::StringBytes(bytes))) = selected else {
        if let Some((field_name, value)) = selected {
            return Err(AssetBundleNameDecodeError::WrongType {
                field_name,
                actual: value_type_name(value),
            });
        }
        return copy_name(source_bundle_name, "source Bundle name");
    };
    let serialized_name =
        str::from_utf8(bytes).map_err(|error| AssetBundleNameDecodeError::InvalidUtf8 {
            field_name,
            valid_up_to: error.valid_up_to(),
        })?;
    if serialized_name.is_empty() {
        copy_name(source_bundle_name, "source Bundle name")
    } else {
        copy_name(serialized_name, "AssetBundleName_R")
    }
}

/// Apply the baseline's AssetBundle path rule to an already selected bundle name.
pub fn asset_bundle_name_for_asset(name: &str) -> Result<String, AssetBundleNameDecodeError> {
    let value = name
        .strip_suffix_case_insensitive(".bundle")
        .unwrap_or(name);
    copy_name(value, "asset bundle name")
}

trait StripSuffixCaseInsensitive {
    fn strip_suffix_case_insensitive(&self, suffix: &str) -> Option<&str>;
}

impl StripSuffixCaseInsensitive for str {
    fn strip_suffix_case_insensitive(&self, suffix: &str) -> Option<&str> {
        let start = self.len().checked_sub(suffix.len())?;
        self.get(start..)
            .filter(|tail| tail.eq_ignore_ascii_case(suffix))
            .map(|_| &self[..start])
    }
}

/// Classification of an AssetBundle Container PPtr using the owning file's PathID graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssetBundleContainerTarget {
    Null,
    LocalObject {
        object_index: usize,
    },
    SharedFile {
        file_id: i32,
        external_index: usize,
        path_id: i64,
    },
    MissingLocalObject {
        path_id: i64,
    },
    AmbiguousLocalObject {
        path_id: i64,
        match_count: usize,
    },
    InvalidFileId {
        file_id: i32,
        external_count: usize,
    },
}

/// Resolve one decoded Container entry against its owning SerializedFile.
pub fn resolve_asset_bundle_container_reference(
    reference: &AssetBundleContainerReference,
    metadata: &SerializedFileMetadata,
    reference_graph: &ObjectReferenceGraph,
) -> AssetBundleContainerTarget {
    let resolution =
        reference_graph.resolve_pointer(metadata, reference.file_id, reference.path_id);
    asset_bundle_container_target(resolution)
}

fn asset_bundle_container_target(resolution: PPtrResolution<'_>) -> AssetBundleContainerTarget {
    match resolution {
        PPtrResolution::Null => AssetBundleContainerTarget::Null,
        PPtrResolution::LocalObject { object_index } => {
            AssetBundleContainerTarget::LocalObject { object_index }
        }
        PPtrResolution::ExternalDependency {
            file_id,
            external_index,
            path_id,
            ..
        } => AssetBundleContainerTarget::SharedFile {
            file_id,
            external_index,
            path_id,
        },
        PPtrResolution::MissingLocalObject { path_id } => {
            AssetBundleContainerTarget::MissingLocalObject { path_id }
        }
        PPtrResolution::AmbiguousLocalObject {
            path_id,
            match_count,
        } => AssetBundleContainerTarget::AmbiguousLocalObject {
            path_id,
            match_count,
        },
        PPtrResolution::InvalidFileId {
            file_id,
            external_count,
        } => AssetBundleContainerTarget::InvalidFileId {
            file_id,
            external_count,
        },
    }
}

fn copy_name(value: &str, resource: &'static str) -> Result<String, AssetBundleNameDecodeError> {
    let mut name = String::new();
    name.try_reserve_exact(value.len()).map_err(|_| {
        AssetBundleNameDecodeError::AllocationFailed {
            resource,
            requested: value.len(),
        }
    })?;
    name.push_str(value);
    Ok(name)
}

fn value_type_name(value: &SerializedValue) -> &'static str {
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
        SerializedValue::Raw { .. } => "raw",
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssetBundleNameDecodeError {
    FieldLookup {
        field_name: &'static str,
        source: SerializedValueLookupError,
    },
    DuplicateAliases {
        first: &'static str,
        second: &'static str,
    },
    WrongType {
        field_name: &'static str,
        actual: &'static str,
    },
    InvalidUtf8 {
        field_name: &'static str,
        valid_up_to: usize,
    },
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
}

impl Display for AssetBundleNameDecodeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::FieldLookup { field_name, source } => {
                write!(
                    formatter,
                    "AssetBundle {field_name} lookup failed: {source}"
                )
            }
            Self::DuplicateAliases { first, second } => write!(
                formatter,
                "AssetBundle serialized name has both {first} and {second} fields"
            ),
            Self::WrongType { field_name, actual } => write!(
                formatter,
                "AssetBundle {field_name} has type {actual}; expected string"
            ),
            Self::InvalidUtf8 {
                field_name,
                valid_up_to,
            } => write!(
                formatter,
                "AssetBundle {field_name} is not UTF-8 (valid through byte {valid_up_to})"
            ),
            Self::AllocationFailed {
                resource,
                requested,
            } => write!(
                formatter,
                "could not allocate {requested} bytes for {resource}"
            ),
        }
    }
}

impl Error for AssetBundleNameDecodeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::FieldLookup { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        asset_bundle_container_target, asset_bundle_name_for_asset, get_asset_bundle_name,
        AssetBundleContainerTarget, AssetBundleNameDecodeError,
    };
    use crate::serialized_file::object::{SerializedFieldValue, SerializedValue};
    use crate::serialized_file::references::PPtrResolution;

    fn field(name: &str, value: SerializedValue) -> SerializedFieldValue {
        SerializedFieldValue {
            name: name.to_owned(),
            value,
        }
    }

    fn root(fields: Vec<SerializedFieldValue>) -> SerializedValue {
        SerializedValue::Struct(fields)
    }

    #[test]
    fn asset_bundle_name_prefers_nonempty_serialized_value_then_uses_bundle_name() {
        assert_eq!(
            get_asset_bundle_name(
                &root(vec![field(
                    "m_AssetBundleName_R",
                    SerializedValue::StringBytes(b"custom.bundle".to_vec()),
                )]),
                "source.bundle",
            )
            .unwrap(),
            "custom.bundle"
        );
        assert_eq!(
            get_asset_bundle_name(
                &root(vec![field(
                    "AssetBundleName_R",
                    SerializedValue::StringBytes(Vec::new()),
                )]),
                "source.bundle",
            )
            .unwrap(),
            "source.bundle"
        );
        assert_eq!(
            get_asset_bundle_name(&root(Vec::new()), "source.bundle").unwrap(),
            "source.bundle"
        );
    }

    #[test]
    fn asset_bundle_name_rejects_conflicting_aliases_and_malformed_values() {
        let conflict = root(vec![
            field(
                "m_AssetBundleName_R",
                SerializedValue::StringBytes(b"one".to_vec()),
            ),
            field(
                "AssetBundleName_R",
                SerializedValue::StringBytes(b"two".to_vec()),
            ),
        ]);
        assert!(matches!(
            get_asset_bundle_name(&conflict, "fallback"),
            Err(AssetBundleNameDecodeError::DuplicateAliases { .. })
        ));
        assert!(matches!(
            get_asset_bundle_name(
                &root(vec![field("m_AssetBundleName_R", SerializedValue::I32(3))]),
                "fallback",
            ),
            Err(AssetBundleNameDecodeError::WrongType { .. })
        ));
        assert!(matches!(
            get_asset_bundle_name(
                &root(vec![field(
                    "m_AssetBundleName_R",
                    SerializedValue::StringBytes(vec![0xff]),
                )]),
                "fallback",
            ),
            Err(AssetBundleNameDecodeError::InvalidUtf8 { .. })
        ));
    }

    #[test]
    fn asset_bundle_asset_name_removes_only_a_trailing_bundle_extension() {
        assert_eq!(
            asset_bundle_name_for_asset("avatars.bundle").unwrap(),
            "avatars"
        );
        assert_eq!(
            asset_bundle_name_for_asset("Avatars.BUNDLE").unwrap(),
            "Avatars"
        );
        assert_eq!(
            asset_bundle_name_for_asset("bundle.avatars").unwrap(),
            "bundle.avatars"
        );
    }

    #[test]
    fn container_pointer_resolution_keeps_null_local_shared_and_invalid_distinct() {
        assert_eq!(
            asset_bundle_container_target(PPtrResolution::Null),
            AssetBundleContainerTarget::Null
        );
        assert_eq!(
            asset_bundle_container_target(PPtrResolution::LocalObject { object_index: 4 }),
            AssetBundleContainerTarget::LocalObject { object_index: 4 }
        );
        assert_eq!(
            asset_bundle_container_target(PPtrResolution::ExternalDependency {
                file_id: 2,
                external_index: 1,
                path_id: 9,
                identifier: &crate::serialized_file::metadata::FileIdentifier {
                    asset_path: None,
                    guid: None,
                    file_type: None,
                    path_name_origin: String::new(),
                },
            }),
            AssetBundleContainerTarget::SharedFile {
                file_id: 2,
                external_index: 1,
                path_id: 9,
            }
        );
        assert_eq!(
            asset_bundle_container_target(PPtrResolution::InvalidFileId {
                file_id: -1,
                external_count: 0,
            }),
            AssetBundleContainerTarget::InvalidFileId {
                file_id: -1,
                external_count: 0,
            }
        );
    }
}
