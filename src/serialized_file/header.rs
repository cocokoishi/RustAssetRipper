use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::io::binary::{BinaryError, BinaryReader, Endian};

use super::format_version::FormatVersion;

const MIN_METADATA_SIZE: u64 = 13;

/// Header fields needed to safely locate SerializedFile metadata and object data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SerializedFileHeader {
    pub metadata_size: u64,
    pub file_size: u64,
    pub format_version: FormatVersion,
    pub data_offset: u64,
    /// Header-declared body byte order. Pre-generation-9 files store this flag in metadata.
    pub data_endian: Option<Endian>,
    /// The generation 22+ 64-bit header field whose purpose is not known by the reference.
    pub reserved_v22: Option<i64>,
    pub header_size: usize,
}

impl SerializedFileHeader {
    /// Parse a SerializedFile header. The header is always big endian; its versioned body
    /// byte order is returned separately in `data_endian`.
    pub fn parse(input: &[u8]) -> Result<Self, HeaderError> {
        let mut reader = BinaryReader::new(input, Endian::Big);

        let legacy_metadata_size = read_i32(&mut reader, "legacy metadata size")? as i64;
        let legacy_file_size = i64::from(read_u32(&mut reader, "legacy file size")?);
        let version_offset = reader.offset();
        let raw_version = read_i32(&mut reader, "format generation")?;
        let format_version = FormatVersion::try_from(raw_version).map_err(|_| {
            HeaderError::new(
                version_offset,
                HeaderErrorKind::UnsupportedFormatVersion(raw_version),
            )
        })?;
        let legacy_data_offset = i64::from(read_u32(&mut reader, "legacy data offset")?);

        let mut metadata_size = legacy_metadata_size;
        let mut file_size = legacy_file_size;
        let mut data_offset = legacy_data_offset;
        let mut data_endian = None;
        let mut reserved_v22 = None;

        if format_version.has_endianness() {
            let marker = read_u8(&mut reader, "endianness marker")?;
            data_endian = Some(if marker == 0 {
                Endian::Little
            } else {
                Endian::Big
            });
            reader
                .align(4)
                .map_err(|error| binary_error(error, "header alignment"))?;
        }

        if format_version.has_large_file_support() {
            metadata_size = i64::from(read_u32(&mut reader, "extended metadata size")?);

            let file_size_offset = reader.offset();
            file_size = read_i64(&mut reader, "extended file size")?;
            if file_size < 0 {
                return Err(HeaderError::new(
                    file_size_offset,
                    HeaderErrorKind::NegativeFileSize(file_size),
                ));
            }

            let data_offset_field = reader.offset();
            data_offset = read_i64(&mut reader, "extended data offset")?;
            if data_offset < 0 {
                return Err(HeaderError::new(
                    data_offset_field,
                    HeaderErrorKind::NegativeDataOffset(data_offset),
                ));
            }

            reserved_v22 = Some(read_i64(&mut reader, "reserved extended header field")?);
        }

        if metadata_size <= 0 {
            return Err(HeaderError::new(
                0,
                HeaderErrorKind::InvalidMetadataSize(metadata_size),
            ));
        }
        if file_size < 0 {
            return Err(HeaderError::new(
                4,
                HeaderErrorKind::NegativeFileSize(file_size),
            ));
        }
        if data_offset < 0 {
            return Err(HeaderError::new(
                12,
                HeaderErrorKind::NegativeDataOffset(data_offset),
            ));
        }

        Ok(Self {
            metadata_size: metadata_size as u64,
            file_size: file_size as u64,
            format_version,
            data_offset: data_offset as u64,
            data_endian,
            reserved_v22,
            header_size: reader.position(),
        })
    }

    /// Check declared file and metadata bounds against the complete serialized-file length.
    /// This is separate from `parse` so the header can be inspected from a header-only slice.
    pub fn validate_for_file_size(&self, actual_file_size: usize) -> Result<(), HeaderError> {
        let actual_file_size = actual_file_size as u64;
        if self.file_size != actual_file_size {
            return Err(HeaderError::new(
                4,
                HeaderErrorKind::DeclaredFileSizeMismatch {
                    declared: self.file_size,
                    actual: actual_file_size,
                },
            ));
        }

        let header_size = self.header_size as u64;
        let minimum_file_size = header_size
            .checked_add(MIN_METADATA_SIZE)
            .ok_or_else(|| HeaderError::new(0, HeaderErrorKind::LayoutOverflow))?;
        if self.file_size < minimum_file_size {
            return Err(HeaderError::new(
                4,
                HeaderErrorKind::FileTooSmall {
                    declared: self.file_size,
                    minimum: minimum_file_size,
                },
            ));
        }

        let (metadata_start, metadata_end, invalid_bounds) =
            if self.format_version.metadata_at_end() {
                let metadata_start = self
                    .file_size
                    .checked_sub(self.metadata_size)
                    .ok_or_else(|| HeaderError::new(0, HeaderErrorKind::LayoutOverflow))?;
                (
                    metadata_start,
                    self.file_size,
                    metadata_start < header_size || self.data_offset > metadata_start,
                )
            } else {
                let metadata_end = header_size
                    .checked_add(self.metadata_size)
                    .ok_or_else(|| HeaderError::new(0, HeaderErrorKind::LayoutOverflow))?;
                (
                    header_size,
                    metadata_end,
                    metadata_end > self.data_offset || self.data_offset > self.file_size,
                )
            };
        if invalid_bounds {
            return Err(HeaderError::new(
                12,
                HeaderErrorKind::MetadataOrDataOutOfBounds {
                    metadata_start,
                    metadata_end,
                    data_offset: self.data_offset,
                    file_size: self.file_size,
                },
            ));
        }
        Ok(())
    }
}

fn read_u8(reader: &mut BinaryReader<'_>, field: &'static str) -> Result<u8, HeaderError> {
    reader.read_u8().map_err(|error| binary_error(error, field))
}

fn read_u32(reader: &mut BinaryReader<'_>, field: &'static str) -> Result<u32, HeaderError> {
    reader
        .read_u32()
        .map_err(|error| binary_error(error, field))
}

fn read_i32(reader: &mut BinaryReader<'_>, field: &'static str) -> Result<i32, HeaderError> {
    reader
        .read_i32()
        .map_err(|error| binary_error(error, field))
}

fn read_i64(reader: &mut BinaryReader<'_>, field: &'static str) -> Result<i64, HeaderError> {
    reader
        .read_i64()
        .map_err(|error| binary_error(error, field))
}

fn binary_error(error: BinaryError, field: &'static str) -> HeaderError {
    let offset = error.offset();
    HeaderError::new(offset, HeaderErrorKind::Binary(error.with_context(field)))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeaderError {
    offset: usize,
    kind: HeaderErrorKind,
}

impl HeaderError {
    fn new(offset: usize, kind: HeaderErrorKind) -> Self {
        Self { offset, kind }
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn kind(&self) -> &HeaderErrorKind {
        &self.kind
    }
}

impl Display for HeaderError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if let HeaderErrorKind::Binary(error) = &self.kind {
            return Display::fmt(error, f);
        }
        write!(f, "{} at byte offset {}", self.kind, self.offset)
    }
}

impl Error for HeaderError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            HeaderErrorKind::Binary(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HeaderErrorKind {
    Binary(BinaryError),
    UnsupportedFormatVersion(i32),
    InvalidMetadataSize(i64),
    NegativeFileSize(i64),
    NegativeDataOffset(i64),
    DeclaredFileSizeMismatch {
        declared: u64,
        actual: u64,
    },
    FileTooSmall {
        declared: u64,
        minimum: u64,
    },
    MetadataOrDataOutOfBounds {
        metadata_start: u64,
        metadata_end: u64,
        data_offset: u64,
        file_size: u64,
    },
    LayoutOverflow,
}

impl Display for HeaderErrorKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Binary(error) => Display::fmt(error, f),
            Self::UnsupportedFormatVersion(version) => {
                write!(f, "unsupported SerializedFile format generation {version}")
            }
            Self::InvalidMetadataSize(size) => {
                write!(f, "invalid SerializedFile metadata size {size}")
            }
            Self::NegativeFileSize(size) => {
                write!(f, "negative SerializedFile size {size}")
            }
            Self::NegativeDataOffset(offset) => {
                write!(f, "negative SerializedFile data offset {offset}")
            }
            Self::DeclaredFileSizeMismatch { declared, actual } => write!(
                f,
                "SerializedFile header declares {declared} bytes, input has {actual}"
            ),
            Self::FileTooSmall { declared, minimum } => write!(
                f,
                "SerializedFile declares {declared} bytes, below minimum {minimum}"
            ),
            Self::MetadataOrDataOutOfBounds {
                metadata_start,
                metadata_end,
                data_offset,
                file_size,
            } => write!(
                f,
                "SerializedFile metadata range {metadata_start}..{metadata_end}, data offset {data_offset}, and file size {file_size} are inconsistent"
            ),
            Self::LayoutOverflow => write!(f, "SerializedFile layout arithmetic overflowed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{HeaderErrorKind, SerializedFileHeader};
    use crate::io::binary::Endian;
    use crate::serialized_file::format_version::FormatVersion;

    fn legacy_header(
        generation: u32,
        file_size: u32,
        data_offset: u32,
        endian_marker: Option<u8>,
    ) -> Vec<u8> {
        let mut bytes = vec![
            0,
            0,
            0,
            13, // metadata size
            (file_size >> 24) as u8,
            (file_size >> 16) as u8,
            (file_size >> 8) as u8,
            file_size as u8,
            0,
            0,
            0,
            generation as u8,
            (data_offset >> 24) as u8,
            (data_offset >> 16) as u8,
            (data_offset >> 8) as u8,
            data_offset as u8,
        ];
        if let Some(marker) = endian_marker {
            bytes.extend_from_slice(&[marker, 0, 0, 0]);
        }
        bytes
    }

    fn large_header(file_size: i64, data_offset: i64) -> Vec<u8> {
        let mut bytes = vec![
            0, 0, 0, 0, // legacy metadata size is replaced by the extended field
            0, 0, 0, 0, // legacy file size is replaced
            0, 0, 0, 22, // generation 22
            0, 0, 0, 0, // legacy data offset is replaced
            0, 0, 0, 0, // little-endian body marker and header alignment
            0, 0, 0, 13, // extended metadata size
        ];
        bytes.extend_from_slice(&file_size.to_be_bytes());
        bytes.extend_from_slice(&data_offset.to_be_bytes());
        bytes.extend_from_slice(&0i64.to_be_bytes()); // unknown/reserved field
        bytes
    }

    #[test]
    fn generation_8_uses_the_legacy_16_byte_header() {
        let mut input = legacy_header(8, 29, 0, None);
        input.extend_from_slice(&[0; 13]);
        let header = SerializedFileHeader::parse(&input).unwrap();

        assert_eq!(header.header_size, 16);
        assert_eq!(header.format_version, FormatVersion::Unknown8);
        assert_eq!(header.metadata_size, 13);
        assert_eq!(header.file_size, 29);
        assert_eq!(header.data_offset, 0);
        assert_eq!(header.data_endian, None);
        header.validate_for_file_size(input.len()).unwrap();
    }

    #[test]
    fn generation_9_adds_the_endian_marker_and_alignment() {
        let mut input = legacy_header(9, 33, 33, Some(1));
        input.extend_from_slice(&[0; 13]);
        let header = SerializedFileHeader::parse(&input).unwrap();

        assert_eq!(header.header_size, 20);
        assert_eq!(header.format_version, FormatVersion::Unknown9);
        assert_eq!(header.data_endian, Some(Endian::Big));
        header.validate_for_file_size(input.len()).unwrap();
    }

    #[test]
    fn generation_21_keeps_the_endian_header_without_extended_sizes() {
        let mut input = legacy_header(21, 33, 33, Some(0));
        input.extend_from_slice(&[0; 13]);
        let header = SerializedFileHeader::parse(&input).unwrap();

        assert_eq!(header.header_size, 20);
        assert_eq!(header.format_version, FormatVersion::StoresTypeDependencies);
        assert_eq!(header.data_endian, Some(Endian::Little));
        assert_eq!(header.reserved_v22, None);
        header.validate_for_file_size(input.len()).unwrap();
    }

    #[test]
    fn generation_22_reads_the_extended_size_and_offset_fields() {
        let mut input = large_header(61, 61);
        input.extend_from_slice(&[0; 13]);
        let header = SerializedFileHeader::parse(&input).unwrap();

        assert_eq!(header.header_size, 48);
        assert_eq!(header.format_version, FormatVersion::LargeFilesSupport);
        assert_eq!(header.metadata_size, 13);
        assert_eq!(header.file_size, 61);
        assert_eq!(header.data_offset, 61);
        assert_eq!(header.reserved_v22, Some(0));
        header.validate_for_file_size(input.len()).unwrap();
    }

    #[test]
    fn unsupported_generation_is_rejected_at_its_field_offset() {
        let input = legacy_header(4, 29, 29, None);
        let error = SerializedFileHeader::parse(&input).unwrap_err();

        assert_eq!(error.offset(), 8);
        assert_eq!(error.kind(), &HeaderErrorKind::UnsupportedFormatVersion(4));
    }

    #[test]
    fn negative_extended_file_size_is_rejected() {
        let input = large_header(-1, 61);
        let error = SerializedFileHeader::parse(&input).unwrap_err();

        assert_eq!(error.offset(), 24);
        assert_eq!(error.kind(), &HeaderErrorKind::NegativeFileSize(-1));
    }

    #[test]
    fn declared_size_and_metadata_bounds_are_checked() {
        let mut input = legacy_header(8, 29, 28, None);
        input.extend_from_slice(&[0; 13]);
        let header = SerializedFileHeader::parse(&input).unwrap();
        let size_error = header.validate_for_file_size(28).unwrap_err();
        assert_eq!(
            size_error.kind(),
            &HeaderErrorKind::DeclaredFileSizeMismatch {
                declared: 29,
                actual: 28,
            }
        );

        let bounds_error = header.validate_for_file_size(input.len()).unwrap_err();
        assert!(matches!(
            bounds_error.kind(),
            HeaderErrorKind::MetadataOrDataOutOfBounds { .. }
        ));
    }

    #[test]
    fn truncated_header_returns_a_contextual_parse_error() {
        let input = [0, 0, 0, 13, 0, 0, 0, 29, 0, 0, 0];
        let error = SerializedFileHeader::parse(&input).unwrap_err();

        assert_eq!(error.offset(), 8);
        assert!(error.to_string().contains("format generation"));
    }

    #[test]
    fn headers_match_reference_written_fixtures() {
        fn hex_to_bytes(hex: &str) -> Vec<u8> {
            assert!(hex.len().is_multiple_of(2), "odd hex length for {hex}");
            let mut out = Vec::with_capacity(hex.len() / 2);
            let bytes = hex.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                let hi = match bytes[i] {
                    b'0'..=b'9' => bytes[i] - b'0',
                    b'a'..=b'f' => bytes[i] - b'a' + 10,
                    b'A'..=b'F' => bytes[i] - b'A' + 10,
                    b => panic!("invalid hex byte {b}"),
                };
                let lo = match bytes[i + 1] {
                    b'0'..=b'9' => bytes[i + 1] - b'0',
                    b'a'..=b'f' => bytes[i + 1] - b'a' + 10,
                    b'A'..=b'F' => bytes[i + 1] - b'A' + 10,
                    b => panic!("invalid hex byte {b}"),
                };
                out.push((hi << 4) | lo);
                i += 2;
            }
            out
        }

        fn field<'a>(parts: &[&'a str], key: &str) -> &'a str {
            let prefix = format!("{key}=");
            for part in parts {
                if let Some(v) = part.strip_prefix(&prefix) {
                    return v;
                }
            }
            panic!("missing {key} in {parts:?}");
        }

        let fixture = include_str!("file_header_fixture.txt");
        let mut preds: Vec<(i32, bool, bool)> = Vec::new();
        let mut hexes: std::collections::HashMap<String, Vec<u8>> =
            std::collections::HashMap::new();
        let mut reads: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for line in fixture.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            if line.starts_with("HEADERPRED|") {
                let parts: Vec<&str> = line.split('|').collect();
                assert_eq!(parts.len(), 4, "bad HEADERPRED {line}");
                preds.push((
                    parts[1].parse().unwrap(),
                    parts[2] == "True",
                    parts[3] == "True",
                ));
            } else if line.starts_with("HEADER_HEX|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad HEADER_HEX {line}");
                hexes.insert(parts[1].to_string(), hex_to_bytes(parts[2]));
            } else if line.starts_with("HEADER_READ|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad HEADER_READ {line}");
                reads.insert(parts[1].to_string(), parts[2].to_string());
            }
        }

        assert!(preds.len() >= 21, "expected HEADERPRED rows");
        for (raw, want_endian, want_large) in preds {
            let generation = FormatVersion::try_from(raw).unwrap();
            assert_eq!(
                generation.has_endianness(),
                want_endian,
                "HasEndianess gen {raw}"
            );
            assert_eq!(
                generation.has_large_file_support(),
                want_large,
                "HasLargeFilesSupport gen {raw}"
            );
        }

        for ver in ["5", "8", "9", "21", "22"] {
            let bytes = hexes
                .get(ver)
                .unwrap_or_else(|| panic!("missing HEADER_HEX {ver}"));
            let dump = reads
                .get(ver)
                .unwrap_or_else(|| panic!("missing HEADER_READ {ver}"));
            let parts: Vec<&str> = dump.split('|').collect();
            let want_meta: u64 = field(&parts, "Meta").parse().unwrap();
            let want_file: u64 = field(&parts, "File").parse().unwrap();
            let want_v: i32 = field(&parts, "V").parse().unwrap();
            let want_data: u64 = field(&parts, "Data").parse().unwrap();
            let want_endian = field(&parts, "Endian") == "True";
            let want_len: usize = field(&parts, "Len").parse().unwrap();

            assert_eq!(bytes.len(), want_len, "v{ver} byte length");
            let header = SerializedFileHeader::parse(bytes)
                .unwrap_or_else(|e| panic!("v{ver} parse: {e:?}"));
            assert_eq!(header.format_version.raw(), want_v, "v{ver} version");
            assert_eq!(header.metadata_size, want_meta, "v{ver} meta");
            assert_eq!(header.file_size, want_file, "v{ver} file size");
            assert_eq!(header.data_offset, want_data, "v{ver} data offset");
            assert_eq!(header.header_size, want_len, "v{ver} header size");
            if want_v >= 9 {
                assert_eq!(
                    header.data_endian,
                    Some(if want_endian {
                        Endian::Big
                    } else {
                        Endian::Little
                    }),
                    "v{ver} endian marker"
                );
            } else {
                assert_eq!(header.data_endian, None, "v{ver} pre-9 has no marker");
            }
            if want_v >= 22 {
                assert!(header.reserved_v22.is_some(), "v{ver} reserved field");
            } else {
                assert_eq!(header.reserved_v22, None, "v{ver} no reserved field");
            }
        }
    }
}
