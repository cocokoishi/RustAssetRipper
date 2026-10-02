use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::io::binary::{BinaryError, BinaryReader, Endian};

use super::header::{HeaderError, SerializedFileHeader};

/// Maximum encoded Unity version string length accepted by default.
pub const DEFAULT_MAX_VERSION_STRING_BYTES: usize = 256;

/// Fields at the start of the SerializedFile metadata block.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SerializedFileMetadataPrefix {
    pub metadata_offset: usize,
    pub metadata_size: usize,
    pub body_endian: Endian,
    /// Preserved as serialized text; parsing Unity's version grammar is a separate operation.
    pub unity_version_text: Option<String>,
    pub target_platform: Option<u32>,
    pub has_type_tree: bool,
    pub consumed_bytes: usize,
}

impl SerializedFileMetadataPrefix {
    /// Parse the metadata prefix from a complete SerializedFile input.
    pub fn parse_file(
        file_bytes: &[u8],
        header: &SerializedFileHeader,
    ) -> Result<Self, MetadataPrefixError> {
        Self::parse_file_with_limit(file_bytes, header, DEFAULT_MAX_VERSION_STRING_BYTES)
    }

    /// Parse the prefix with a caller-selected limit for the null-terminated version string.
    pub fn parse_file_with_limit(
        file_bytes: &[u8],
        header: &SerializedFileHeader,
        maximum_version_string_bytes: usize,
    ) -> Result<Self, MetadataPrefixError> {
        header
            .validate_for_file_size(file_bytes.len())
            .map_err(MetadataPrefixError::from_header)?;

        let metadata_size = usize::try_from(header.metadata_size).map_err(|_| {
            MetadataPrefixError::new(0, MetadataPrefixErrorKind::LayoutOverflow("metadata size"))
        })?;
        let metadata_offset = if header.format_version.metadata_at_end() {
            file_bytes.len().checked_sub(metadata_size).ok_or_else(|| {
                MetadataPrefixError::new(
                    0,
                    MetadataPrefixErrorKind::InvalidMetadataRange {
                        start: 0,
                        end: metadata_size,
                        file_size: file_bytes.len(),
                    },
                )
            })?
        } else {
            header.header_size
        };
        let metadata_end = metadata_offset.checked_add(metadata_size).ok_or_else(|| {
            MetadataPrefixError::new(
                metadata_offset,
                MetadataPrefixErrorKind::LayoutOverflow("metadata end"),
            )
        })?;
        if metadata_end > file_bytes.len() {
            return Err(MetadataPrefixError::new(
                metadata_offset,
                MetadataPrefixErrorKind::InvalidMetadataRange {
                    start: metadata_offset,
                    end: metadata_end,
                    file_size: file_bytes.len(),
                },
            ));
        }

        let metadata = &file_bytes[metadata_offset..metadata_end];
        let initial_endian = header.data_endian.unwrap_or(Endian::Little);
        let mut reader = BinaryReader::with_base_offset(metadata, initial_endian, metadata_offset)
            .map_err(MetadataPrefixError::from_binary)?;

        let body_endian = if header.format_version.metadata_at_end() {
            let marker = reader.read_u8().map_err(|error| {
                MetadataPrefixError::from_binary(error.with_context("metadata endian marker"))
            })?;
            let endian = if marker == 0 {
                Endian::Little
            } else {
                Endian::Big
            };
            reader.set_endian(endian);
            endian
        } else {
            header.data_endian.ok_or_else(|| {
                MetadataPrefixError::new(
                    0,
                    MetadataPrefixErrorKind::MissingHeaderEndian {
                        generation: header.format_version.raw(),
                    },
                )
            })?
        };

        let unity_version_text = if header.format_version.has_signature() {
            let text = reader
                .read_null_terminated_utf8(maximum_version_string_bytes)
                .map_err(|error| {
                    MetadataPrefixError::from_binary(error.with_context("Unity version string"))
                })?;
            Some(text.to_owned())
        } else {
            None
        };

        let target_platform = if header.format_version.has_platform() {
            Some(reader.read_u32().map_err(|error| {
                MetadataPrefixError::from_binary(error.with_context("target platform"))
            })?)
        } else {
            None
        };

        let has_type_tree = if header.format_version.has_enable_type_tree() {
            reader.read_u8().map(|value| value != 0).map_err(|error| {
                MetadataPrefixError::from_binary(error.with_context("type-tree flag"))
            })?
        } else {
            true
        };

        Ok(Self {
            metadata_offset,
            metadata_size,
            body_endian,
            unity_version_text,
            target_platform,
            has_type_tree,
            consumed_bytes: reader.position(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetadataPrefixError {
    offset: usize,
    kind: MetadataPrefixErrorKind,
}

impl MetadataPrefixError {
    fn new(offset: usize, kind: MetadataPrefixErrorKind) -> Self {
        Self { offset, kind }
    }

    fn from_binary(error: BinaryError) -> Self {
        let offset = error.offset();
        Self::new(offset, MetadataPrefixErrorKind::Binary(error))
    }

    fn from_header(error: HeaderError) -> Self {
        let offset = error.offset();
        Self::new(offset, MetadataPrefixErrorKind::Header(error))
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn kind(&self) -> &MetadataPrefixErrorKind {
        &self.kind
    }
}

impl Display for MetadataPrefixError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match &self.kind {
            MetadataPrefixErrorKind::Binary(error) => Display::fmt(error, f),
            MetadataPrefixErrorKind::Header(error) => Display::fmt(error, f),
            _ => write!(f, "{} at byte offset {}", self.kind, self.offset),
        }
    }
}

impl Error for MetadataPrefixError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            MetadataPrefixErrorKind::Binary(error) => Some(error),
            MetadataPrefixErrorKind::Header(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MetadataPrefixErrorKind {
    Binary(BinaryError),
    Header(HeaderError),
    MissingHeaderEndian {
        generation: i32,
    },
    InvalidMetadataRange {
        start: usize,
        end: usize,
        file_size: usize,
    },
    LayoutOverflow(&'static str),
}

impl Display for MetadataPrefixErrorKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Binary(error) => Display::fmt(error, f),
            Self::Header(error) => Display::fmt(error, f),
            Self::MissingHeaderEndian { generation } => write!(
                f,
                "generation {generation} requires an endian marker in the SerializedFile header"
            ),
            Self::InvalidMetadataRange {
                start,
                end,
                file_size,
            } => write!(
                f,
                "metadata range {start}..{end} exceeds file size {file_size}"
            ),
            Self::LayoutOverflow(field) => write!(f, "metadata {field} arithmetic overflowed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SerializedFileMetadataPrefix;
    use crate::io::binary::Endian;
    use crate::serialized_file::format_version::FormatVersion;
    use crate::serialized_file::header::SerializedFileHeader;

    fn make_file(
        version: FormatVersion,
        metadata: &[u8],
        body_endian: Endian,
    ) -> (Vec<u8>, SerializedFileHeader) {
        let header_size = if version.has_large_file_support() {
            48
        } else if version.has_endianness() {
            20
        } else {
            16
        };
        let file_size = header_size + metadata.len();
        let mut bytes = vec![0; header_size];
        bytes.extend_from_slice(metadata);
        let header = SerializedFileHeader {
            metadata_size: metadata.len() as u64,
            file_size: file_size as u64,
            format_version: version,
            data_offset: if version.metadata_at_end() {
                0
            } else {
                file_size as u64
            },
            data_endian: if version.has_endianness() {
                Some(body_endian)
            } else {
                None
            },
            reserved_v22: version.has_large_file_support().then_some(0),
            header_size,
        };
        (bytes, header)
    }

    #[test]
    fn pre_generation_9_reads_unaligned_endian_from_metadata_at_file_end() {
        let mut metadata = vec![1];
        metadata.extend_from_slice(b"2022.3.1f1\0");
        metadata.extend_from_slice(&[0, 0, 0, 7]);
        let (file, header) = make_file(FormatVersion::Unknown8, &metadata, Endian::Little);

        let prefix = SerializedFileMetadataPrefix::parse_file(&file, &header).unwrap();
        assert_eq!(prefix.metadata_offset, 16);
        assert_eq!(prefix.body_endian, Endian::Big);
        assert_eq!(prefix.unity_version_text.as_deref(), Some("2022.3.1f1"));
        assert_eq!(prefix.target_platform, Some(7));
        assert!(prefix.has_type_tree);
        assert_eq!(prefix.consumed_bytes, metadata.len());
    }

    #[test]
    fn generation_9_uses_header_endian_and_unaligned_metadata_prefix() {
        let mut metadata = b"2022.3.1f1\0".to_vec();
        metadata.extend_from_slice(&[7, 0, 0, 0]);
        let (file, header) = make_file(FormatVersion::Unknown9, &metadata, Endian::Little);

        let prefix = SerializedFileMetadataPrefix::parse_file(&file, &header).unwrap();
        assert_eq!(prefix.metadata_offset, 20);
        assert_eq!(prefix.body_endian, Endian::Little);
        assert_eq!(prefix.target_platform, Some(7));
        assert!(prefix.has_type_tree);
    }

    #[test]
    fn type_tree_flag_is_implicit_before_generation_13_and_serialized_after() {
        let mut old_metadata = b"2019.4.0f1\0".to_vec();
        old_metadata.extend_from_slice(&[0, 0, 0, 5]);
        let (old_file, old_header) =
            make_file(FormatVersion::Unknown12, &old_metadata, Endian::Big);
        let old_prefix = SerializedFileMetadataPrefix::parse_file(&old_file, &old_header).unwrap();
        assert!(old_prefix.has_type_tree);

        let mut new_metadata = b"2019.4.0f1\0".to_vec();
        new_metadata.extend_from_slice(&[0, 0, 0, 5, 0]);
        let (new_file, new_header) =
            make_file(FormatVersion::HasTypeTreeHashes, &new_metadata, Endian::Big);
        let new_prefix = SerializedFileMetadataPrefix::parse_file(&new_file, &new_header).unwrap();
        assert!(!new_prefix.has_type_tree);
    }

    #[test]
    fn version_string_limit_is_reported_with_absolute_offset() {
        let mut metadata = b"long\0".to_vec();
        metadata.extend_from_slice(&[0, 0, 0, 5]);
        metadata.extend_from_slice(&[0; 4]);
        let (file, header) = make_file(FormatVersion::Unknown9, &metadata, Endian::Little);
        let error =
            SerializedFileMetadataPrefix::parse_file_with_limit(&file, &header, 3).unwrap_err();

        assert_eq!(error.offset(), 20);
        assert!(error.to_string().contains("Unity version string"));
    }
}
