//! Unity `.meta` YAML envelopes written from export-owned importer values.

use std::env;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::time::{SystemTime, UNIX_EPOCH};

use super::unity_yaml::{
    format_unity_guid_words, write_unity_yaml_mapping, UnityYamlMappingStyle, UnityYamlNode,
    UnityYamlNodeBuildError, UnityYamlNodeKind, UnityYamlScalar, UnityYamlWriteError,
    UnityYamlWriteLimits,
};

const UNITY_META_FILE_FORMAT_VERSION: i64 = 2;
const MIN_UNIX_TIMESTAMP_SECONDS: i64 = -62_135_596_800;
const MAX_UNIX_TIMESTAMP_SECONDS: i64 = 253_402_300_799;

/// Options corresponding to the optional fields emitted by the baseline `Meta` value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnityMetaFileOptions {
    pub has_license_data: bool,
    pub is_folder_asset: bool,
    /// Explicit Unix seconds for deterministic output; otherwise `SOURCE_DATE_EPOCH` is used
    /// when it contains a valid integer, falling back to the current UTC time as the baseline does.
    pub time_created_unix_seconds: Option<i64>,
}

impl Default for UnityMetaFileOptions {
    fn default() -> Self {
        Self {
            has_license_data: true,
            is_folder_asset: false,
            time_created_unix_seconds: None,
        }
    }
}

/// The file-level mapping written beside one exported project asset.
#[derive(Debug)]
pub struct UnityMetaFile {
    guid: [u8; 16],
    importer_class: String,
    importer: UnityYamlNode,
    options: UnityMetaFileOptions,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UnityMetaFileError {
    ZeroGuid,
    InvalidImporterClassName,
    ImporterMustBePlainMapping,
    InvalidTimeCreated { value: i64 },
    CurrentTimeUnavailable,
    AllocationFailed,
    NodeBuild(UnityYamlNodeBuildError),
    YamlWrite(UnityYamlWriteError),
}

impl Display for UnityMetaFileError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroGuid => formatter.write_str("Unity .meta files cannot use a zero GUID"),
            Self::InvalidImporterClassName => formatter
                .write_str("Unity .meta importer class names must be non-empty ASCII identifiers"),
            Self::ImporterMustBePlainMapping => {
                formatter.write_str("Unity .meta importer data must be an undecorated YAML mapping")
            }
            Self::InvalidTimeCreated { value } => write!(
                formatter,
                "Unity .meta timeCreated Unix timestamp {value} is outside DateTimeOffset range"
            ),
            Self::CurrentTimeUnavailable => {
                formatter.write_str("could not read the current system time for Unity .meta")
            }
            Self::AllocationFailed => formatter.write_str("could not allocate Unity .meta data"),
            Self::NodeBuild(source) => Display::fmt(source, formatter),
            Self::YamlWrite(source) => Display::fmt(source, formatter),
        }
    }
}

impl Error for UnityMetaFileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::NodeBuild(source) => Some(source),
            Self::YamlWrite(source) => Some(source),
            _ => None,
        }
    }
}

impl UnityMetaFile {
    pub fn new(
        guid: [u8; 16],
        importer_class: &str,
        importer: UnityYamlNode,
        options: UnityMetaFileOptions,
    ) -> Result<Self, UnityMetaFileError> {
        if guid == [0; 16] {
            return Err(UnityMetaFileError::ZeroGuid);
        }
        if !is_unity_importer_class_name(importer_class) {
            return Err(UnityMetaFileError::InvalidImporterClassName);
        }
        if !matches!(importer.kind, UnityYamlNodeKind::Mapping { .. })
            || importer.tag.is_some()
            || importer.anchor.is_some()
            || importer.stripped
        {
            return Err(UnityMetaFileError::ImporterMustBePlainMapping);
        }
        let mut owned_importer_class = String::new();
        owned_importer_class
            .try_reserve_exact(importer_class.len())
            .map_err(|_| UnityMetaFileError::AllocationFailed)?;
        owned_importer_class.push_str(importer_class);
        Ok(Self {
            guid,
            importer_class: owned_importer_class,
            importer,
            options,
        })
    }

    /// Serialize a `.meta` document without YAML stream headers or Unity object tags.
    pub fn to_yaml(self, limits: UnityYamlWriteLimits) -> Result<String, UnityMetaFileError> {
        let mut root = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        insert_meta_field(
            &mut root,
            "fileFormatVersion",
            UnityYamlNode::scalar(UnityYamlScalar::Signed(UNITY_META_FILE_FORMAT_VERSION)),
        )?;
        insert_meta_field(
            &mut root,
            "guid",
            UnityYamlNode::string(format_unity_guid_words(self.guid)),
        )?;
        if self.options.is_folder_asset {
            insert_meta_field(
                &mut root,
                "folderAsset",
                UnityYamlNode::scalar(UnityYamlScalar::Boolean(true)),
            )?;
        }
        if self.options.has_license_data {
            let time_created = match self.options.time_created_unix_seconds {
                Some(value) => validate_time_created(value)?,
                None => source_time_created_unix_seconds()?,
            };
            insert_meta_field(
                &mut root,
                "timeCreated",
                UnityYamlNode::scalar(UnityYamlScalar::Signed(time_created)),
            )?;
            insert_meta_field(
                &mut root,
                "licenseType",
                UnityYamlNode::plain_string("Free"),
            )?;
        }
        insert_meta_field(&mut root, &self.importer_class, self.importer)?;
        write_unity_yaml_mapping(&root, limits).map_err(UnityMetaFileError::YamlWrite)
    }
}

fn insert_meta_field(
    mapping: &mut UnityYamlNode,
    name: &str,
    value: UnityYamlNode,
) -> Result<(), UnityMetaFileError> {
    mapping
        .insert_field(name, value)
        .map_err(UnityMetaFileError::NodeBuild)
}

fn is_unity_importer_class_name(value: &str) -> bool {
    let mut characters = value.bytes();
    let Some(first) = characters.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == b'_')
        && characters.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn source_time_created_unix_seconds() -> Result<i64, UnityMetaFileError> {
    let source_date_epoch = env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|value| value.trim().parse::<i64>().ok());
    let value = match source_date_epoch {
        Some(value) => value,
        None => current_unix_seconds()?,
    };
    validate_time_created(value)
}

fn current_unix_seconds() -> Result<i64, UnityMetaFileError> {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(duration) => i64::try_from(duration.as_secs())
            .map_err(|_| UnityMetaFileError::CurrentTimeUnavailable),
        Err(error) => i64::try_from(error.duration().as_secs())
            .map(|seconds| -seconds)
            .map_err(|_| UnityMetaFileError::CurrentTimeUnavailable),
    }
}

fn validate_time_created(value: i64) -> Result<i64, UnityMetaFileError> {
    if !(MIN_UNIX_TIMESTAMP_SECONDS..=MAX_UNIX_TIMESTAMP_SECONDS).contains(&value) {
        return Err(UnityMetaFileError::InvalidTimeCreated { value });
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::{UnityMetaFile, UnityMetaFileError, UnityMetaFileOptions};
    use crate::export::unity_yaml::{
        UnityYamlMappingStyle, UnityYamlNode, UnityYamlScalar, UnityYamlWriteError,
        UnityYamlWriteLimits,
    };

    fn importer() -> UnityYamlNode {
        let mut importer = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        importer
            .insert_field(
                "serializedVersion",
                UnityYamlNode::scalar(UnityYamlScalar::Signed(6)),
            )
            .unwrap();
        importer
            .insert_field(
                "m_AlphaIsTransparency",
                UnityYamlNode::scalar(UnityYamlScalar::Boolean(true)),
            )
            .unwrap();
        importer
    }

    #[test]
    fn meta_yaml_matches_source_envelope_order_and_importer_key_formatting() {
        let meta = UnityMetaFile::new(
            [1, 0, 0, 0, 0x12, 0x34, 0x56, 0x78, 0, 0, 0, 0, 0, 0, 0, 0],
            "TextureImporter",
            importer(),
            UnityMetaFileOptions {
                time_created_unix_seconds: Some(1_700_000_000),
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(
            meta.to_yaml(UnityYamlWriteLimits::default()).unwrap(),
            "fileFormatVersion: 2\nguid: 00000001785634120000000000000000\ntimeCreated: 1700000000\nlicenseType: Free\nTextureImporter:\n  serializedVersion: 6\n  m_alphaIsTransparency: 1\n"
        );
    }

    #[test]
    fn meta_yaml_emits_optional_folder_and_license_fields_as_source_scalars() {
        let meta = UnityMetaFile::new(
            [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            "DefaultImporter",
            UnityYamlNode::mapping(UnityYamlMappingStyle::Block),
            UnityMetaFileOptions {
                has_license_data: false,
                is_folder_asset: true,
                time_created_unix_seconds: None,
            },
        )
        .unwrap();

        assert_eq!(
            meta.to_yaml(UnityYamlWriteLimits::default()).unwrap(),
            "fileFormatVersion: 2\nguid: 00000001000000000000000000000000\nfolderAsset: 1\nDefaultImporter: {}\n"
        );
    }

    #[test]
    fn meta_yaml_rejects_invalid_identity_importer_and_bounds() {
        assert_eq!(
            UnityMetaFile::new([0; 16], "DefaultImporter", importer(), Default::default(),)
                .unwrap_err(),
            UnityMetaFileError::ZeroGuid
        );
        assert_eq!(
            UnityMetaFile::new(
                [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                "Importer:Injected",
                importer(),
                Default::default(),
            )
            .unwrap_err(),
            UnityMetaFileError::InvalidImporterClassName
        );
        let invalid_time = UnityMetaFile::new(
            [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            "DefaultImporter",
            importer(),
            UnityMetaFileOptions {
                time_created_unix_seconds: Some(i64::MAX),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            invalid_time.to_yaml(UnityYamlWriteLimits::default()),
            Err(UnityMetaFileError::InvalidTimeCreated { value: i64::MAX })
        );

        let bounded = UnityMetaFile::new(
            [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            "DefaultImporter",
            importer(),
            UnityMetaFileOptions {
                has_license_data: false,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(matches!(
            bounded.to_yaml(UnityYamlWriteLimits {
                max_output_bytes: 8,
                ..Default::default()
            }),
            Err(UnityMetaFileError::YamlWrite(
                UnityYamlWriteError::OutputLimitExceeded { .. }
            ))
        ));
    }
}
