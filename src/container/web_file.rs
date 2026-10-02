//! Unity WebData containers used for web player resources.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::io::binary::{BinaryError, BinaryReader, Endian};

const WEB_FILE_SIGNATURE: &str = "UnityWebData1.0";

/// Bounds for the WebData directory and its entries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WebFileLimits {
    pub max_header_bytes: usize,
    pub max_entries: usize,
    pub max_entry_name_bytes: usize,
    pub max_entry_bytes: usize,
    pub max_output_bytes: usize,
}

impl Default for WebFileLimits {
    fn default() -> Self {
        Self {
            max_header_bytes: 16 * 1024 * 1024,
            max_entries: 1_000_000,
            max_entry_name_bytes: 1024 * 1024,
            max_entry_bytes: 512 * 1024 * 1024,
            max_output_bytes: 1024 * 1024 * 1024,
        }
    }
}

/// One length-prefixed directory record and its checked byte range.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebFileEntry {
    pub name: String,
    pub offset: u32,
    pub size: u32,
    range: std::ops::Range<usize>,
}

/// A borrowed view of a Unity WebData file.
#[derive(Debug)]
pub struct WebFile<'a> {
    input: &'a [u8],
    pub entries: Vec<WebFileEntry>,
    pub header_size: usize,
}

impl<'a> WebFile<'a> {
    pub fn is_web_file(input: &[u8]) -> bool {
        let signature = WEB_FILE_SIGNATURE.as_bytes();
        input.len() > signature.len()
            && input.get(..signature.len()) == Some(signature)
            && input.get(signature.len()) == Some(&0)
    }

    pub fn parse(input: &'a [u8]) -> Result<Self, WebFileError> {
        Self::parse_with_limits(input, WebFileLimits::default())
    }

    pub fn parse_with_limits(input: &'a [u8], limits: WebFileLimits) -> Result<Self, WebFileError> {
        let mut reader = BinaryReader::new(input, Endian::Little);
        let signature = reader
            .read_null_terminated_utf8(WEB_FILE_SIGNATURE.len() + 1)
            .map_err(|error| WebFileError::binary(error, "WebData signature"))?;
        if signature != WEB_FILE_SIGNATURE {
            return Err(WebFileError::new(WebFileErrorKind::InvalidSignature));
        }

        let header_length = reader
            .read_i32()
            .map_err(|error| WebFileError::binary(error, "WebData header length"))?;
        if header_length < 0 {
            return Err(WebFileError::new(WebFileErrorKind::NegativeHeaderLength {
                value: header_length,
            }));
        }
        let header_size = header_length as usize;
        if header_size < reader.position() {
            return Err(WebFileError::new(
                WebFileErrorKind::HeaderLengthBeforeDirectory {
                    declared: header_size,
                    minimum: reader.position(),
                },
            ));
        }
        if header_size > limits.max_header_bytes {
            return Err(WebFileError::new(WebFileErrorKind::HeaderLimitExceeded {
                declared: header_size,
                limit: limits.max_header_bytes,
            }));
        }
        if header_size > input.len() {
            return Err(WebFileError::new(WebFileErrorKind::HeaderOutOfBounds {
                declared: header_size,
                input_size: input.len(),
            }));
        }

        let mut entries = Vec::new();
        while reader.position() < header_size {
            if entries.len() >= limits.max_entries {
                return Err(WebFileError::new(WebFileErrorKind::EntryLimitExceeded {
                    limit: limits.max_entries,
                }));
            }

            let record_offset = reader.offset();
            let offset = reader
                .read_i32()
                .map_err(|error| WebFileError::binary(error, "WebData entry offset"))?;
            let size = reader
                .read_i32()
                .map_err(|error| WebFileError::binary(error, "WebData entry size"))?;
            if offset < 0 {
                return Err(WebFileError::new(WebFileErrorKind::NegativeEntryOffset {
                    entry: entries.len(),
                    value: offset,
                }));
            }
            if size < 0 {
                return Err(WebFileError::new(WebFileErrorKind::NegativeEntrySize {
                    entry: entries.len(),
                    value: size,
                }));
            }
            if size as usize > limits.max_entry_bytes {
                return Err(WebFileError::new(
                    WebFileErrorKind::EntrySizeLimitExceeded {
                        entry: entries.len(),
                        size: size as usize,
                        limit: limits.max_entry_bytes,
                    },
                ));
            }

            let name_length = read_7bit_length(&mut reader)?;
            if name_length > limits.max_entry_name_bytes {
                return Err(WebFileError::new(WebFileErrorKind::NameLimitExceeded {
                    entry: entries.len(),
                    declared: name_length,
                    limit: limits.max_entry_name_bytes,
                }));
            }
            let name = reader
                .read_utf8(name_length)
                .map_err(|error| WebFileError::binary(error, "WebData entry name"))?;
            if reader.position() > header_size {
                return Err(WebFileError::new(
                    WebFileErrorKind::DirectoryEntryCrossesHeader {
                        entry: entries.len(),
                        record_offset,
                        header_size,
                        position: reader.position(),
                    },
                ));
            }

            let start = offset as usize;
            let end = start.checked_add(size as usize).ok_or_else(|| {
                WebFileError::new(WebFileErrorKind::EntryRangeOverflow {
                    entry: entries.len(),
                    offset: start,
                    size: size as usize,
                })
            })?;
            let range = start..end;
            if end > input.len() {
                return Err(WebFileError::new(WebFileErrorKind::EntryOutOfBounds {
                    entry: entries.len(),
                    offset: start,
                    size: size as usize,
                    input_size: input.len(),
                }));
            }

            let mut owned_name = String::new();
            owned_name.try_reserve_exact(name.len()).map_err(|_| {
                WebFileError::new(WebFileErrorKind::AllocationFailed {
                    requested: name.len(),
                })
            })?;
            owned_name.push_str(name);
            entries.try_reserve(1).map_err(|_| {
                WebFileError::new(WebFileErrorKind::AllocationFailed {
                    requested: entries.len().saturating_add(1),
                })
            })?;
            entries.push(WebFileEntry {
                name: owned_name,
                offset: offset as u32,
                size: size as u32,
                range,
            });
        }

        if reader.position() != header_size {
            return Err(WebFileError::new(
                WebFileErrorKind::DirectoryLengthMismatch {
                    declared: header_size,
                    parsed: reader.position(),
                },
            ));
        }

        Ok(Self {
            input,
            entries,
            header_size,
        })
    }

    pub fn entry_data(&self, index: usize) -> Option<&'a [u8]> {
        let entry = self.entries.get(index)?;
        self.input.get(entry.range.clone())
    }

    /// Serialize entries using the WebData 1.0 directory layout.
    pub fn write(
        &self,
        align_entries: bool,
        maximum_output_size: usize,
    ) -> Result<Vec<u8>, WebFileError> {
        let mut output = Vec::new();
        push_bytes(
            &mut output,
            WEB_FILE_SIGNATURE.as_bytes(),
            maximum_output_size,
        )?;
        push_bytes(&mut output, &[0], maximum_output_size)?;
        let header_length_position = output.len();
        push_bytes(&mut output, &[0; 4], maximum_output_size)?;

        let mut offset_positions = Vec::new();
        offset_positions
            .try_reserve_exact(self.entries.len())
            .map_err(|_| {
                WebFileError::new(WebFileErrorKind::AllocationFailed {
                    requested: self.entries.len(),
                })
            })?;
        for (index, entry) in self.entries.iter().enumerate() {
            let data = self.entry_data(index).ok_or_else(|| {
                WebFileError::new(WebFileErrorKind::EntryOutOfBounds {
                    entry: index,
                    offset: entry.offset as usize,
                    size: entry.size as usize,
                    input_size: self.input.len(),
                })
            })?;
            if data.len() > i32::MAX as usize {
                return Err(WebFileError::new(WebFileErrorKind::WriteIntegerOverflow {
                    field: "entry size",
                    value: data.len(),
                }));
            }

            offset_positions.push(output.len());
            push_bytes(&mut output, &[0; 4], maximum_output_size)?;
            push_bytes(
                &mut output,
                &(data.len() as i32).to_le_bytes(),
                maximum_output_size,
            )?;
            let name = entry.name.as_bytes();
            let name_length = u32::try_from(name.len()).map_err(|_| {
                WebFileError::new(WebFileErrorKind::WriteIntegerOverflow {
                    field: "entry name length",
                    value: name.len(),
                })
            })?;
            push_7bit_length(&mut output, name_length, maximum_output_size)?;
            push_bytes(&mut output, name, maximum_output_size)?;
        }

        if output.len() > i32::MAX as usize {
            return Err(WebFileError::new(WebFileErrorKind::WriteIntegerOverflow {
                field: "header length",
                value: output.len(),
            }));
        }
        let header_length = output.len() as i32;
        output[header_length_position..header_length_position + 4]
            .copy_from_slice(&header_length.to_le_bytes());

        for (index, entry) in self.entries.iter().enumerate() {
            if align_entries {
                let padding = (4 - (output.len() & 3)) & 3;
                push_bytes(&mut output, &[0; 3][..padding], maximum_output_size)?;
            }
            if output.len() > i32::MAX as usize {
                return Err(WebFileError::new(WebFileErrorKind::WriteIntegerOverflow {
                    field: "entry offset",
                    value: output.len(),
                }));
            }
            let offset = output.len() as i32;
            let position = offset_positions[index];
            output[position..position + 4].copy_from_slice(&offset.to_le_bytes());
            let data = self.entry_data(index).ok_or_else(|| {
                WebFileError::new(WebFileErrorKind::EntryOutOfBounds {
                    entry: index,
                    offset: entry.offset as usize,
                    size: entry.size as usize,
                    input_size: self.input.len(),
                })
            })?;
            push_bytes(&mut output, data, maximum_output_size)?;
        }
        Ok(output)
    }
}

fn read_7bit_length(reader: &mut BinaryReader<'_>) -> Result<usize, WebFileError> {
    let mut value = 0u32;
    for index in 0..5 {
        let byte = reader
            .read_u8()
            .map_err(|error| WebFileError::binary(error, "WebData string length"))?;
        if index == 4 && byte & 0xf0 != 0 {
            return Err(WebFileError::new(
                WebFileErrorKind::InvalidStringLengthEncoding,
            ));
        }
        value |= u32::from(byte & 0x7f) << (index * 7);
        if byte & 0x80 == 0 {
            return Ok(value as usize);
        }
    }
    Err(WebFileError::new(
        WebFileErrorKind::InvalidStringLengthEncoding,
    ))
}

fn push_7bit_length(
    output: &mut Vec<u8>,
    mut value: u32,
    maximum_output_size: usize,
) -> Result<(), WebFileError> {
    while value >= 0x80 {
        push_bytes(output, &[value as u8 | 0x80], maximum_output_size)?;
        value >>= 7;
    }
    push_bytes(output, &[value as u8], maximum_output_size)
}

fn push_bytes(
    output: &mut Vec<u8>,
    bytes: &[u8],
    maximum_output_size: usize,
) -> Result<(), WebFileError> {
    let requested = output
        .len()
        .checked_add(bytes.len())
        .ok_or_else(|| WebFileError::new(WebFileErrorKind::OutputLengthOverflow))?;
    if requested > maximum_output_size {
        return Err(WebFileError::new(WebFileErrorKind::OutputLimitExceeded {
            requested,
            limit: maximum_output_size,
        }));
    }
    output
        .try_reserve(bytes.len())
        .map_err(|_| WebFileError::new(WebFileErrorKind::AllocationFailed { requested }))?;
    output.extend_from_slice(bytes);
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WebFileError {
    kind: WebFileErrorKind,
}

impl WebFileError {
    fn new(kind: WebFileErrorKind) -> Self {
        Self { kind }
    }

    fn binary(error: BinaryError, field: &'static str) -> Self {
        Self::new(WebFileErrorKind::Binary(error.with_context(field)))
    }

    pub fn kind(&self) -> &WebFileErrorKind {
        &self.kind
    }
}

impl Display for WebFileError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.kind, formatter)
    }
}

impl Error for WebFileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            WebFileErrorKind::Binary(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WebFileErrorKind {
    Binary(BinaryError),
    InvalidSignature,
    NegativeHeaderLength {
        value: i32,
    },
    HeaderLengthBeforeDirectory {
        declared: usize,
        minimum: usize,
    },
    HeaderLimitExceeded {
        declared: usize,
        limit: usize,
    },
    HeaderOutOfBounds {
        declared: usize,
        input_size: usize,
    },
    EntryLimitExceeded {
        limit: usize,
    },
    EntrySizeLimitExceeded {
        entry: usize,
        size: usize,
        limit: usize,
    },
    NegativeEntryOffset {
        entry: usize,
        value: i32,
    },
    NegativeEntrySize {
        entry: usize,
        value: i32,
    },
    NameLimitExceeded {
        entry: usize,
        declared: usize,
        limit: usize,
    },
    InvalidStringLengthEncoding,
    DirectoryEntryCrossesHeader {
        entry: usize,
        record_offset: usize,
        header_size: usize,
        position: usize,
    },
    DirectoryLengthMismatch {
        declared: usize,
        parsed: usize,
    },
    EntryRangeOverflow {
        entry: usize,
        offset: usize,
        size: usize,
    },
    EntryOutOfBounds {
        entry: usize,
        offset: usize,
        size: usize,
        input_size: usize,
    },
    AllocationFailed {
        requested: usize,
    },
    WriteIntegerOverflow {
        field: &'static str,
        value: usize,
    },
    OutputLengthOverflow,
    OutputLimitExceeded {
        requested: usize,
        limit: usize,
    },
}

impl Display for WebFileErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Binary(error) => Display::fmt(error, formatter),
            Self::InvalidSignature => write!(formatter, "invalid Unity WebData signature"),
            Self::NegativeHeaderLength { value } => {
                write!(formatter, "negative WebData header length {value}")
            }
            Self::HeaderLengthBeforeDirectory { declared, minimum } => write!(
                formatter,
                "WebData header length {declared} ends before the directory starts at {minimum}"
            ),
            Self::HeaderLimitExceeded { declared, limit } => write!(
                formatter,
                "WebData header length {declared} exceeds configured limit {limit}"
            ),
            Self::HeaderOutOfBounds {
                declared,
                input_size,
            } => write!(
                formatter,
                "WebData header ends at {declared}, beyond input size {input_size}"
            ),
            Self::EntryLimitExceeded { limit } => {
                write!(formatter, "WebData entry count exceeds configured limit {limit}")
            }
            Self::EntrySizeLimitExceeded { entry, size, limit } => write!(
                formatter,
                "WebData entry {entry} size {size} exceeds configured limit {limit}"
            ),
            Self::NegativeEntryOffset { entry, value } => {
                write!(formatter, "WebData entry {entry} has negative offset {value}")
            }
            Self::NegativeEntrySize { entry, value } => {
                write!(formatter, "WebData entry {entry} has negative size {value}")
            }
            Self::NameLimitExceeded {
                entry,
                declared,
                limit,
            } => write!(
                formatter,
                "WebData entry {entry} name length {declared} exceeds limit {limit}"
            ),
            Self::InvalidStringLengthEncoding => {
                write!(formatter, "invalid WebData 7-bit string length")
            }
            Self::DirectoryEntryCrossesHeader {
                entry,
                record_offset,
                header_size,
                position,
            } => write!(
                formatter,
                "WebData entry {entry} beginning at {record_offset} ends at {position}, beyond header end {header_size}"
            ),
            Self::DirectoryLengthMismatch { declared, parsed } => write!(
                formatter,
                "WebData directory ended at {parsed}, but header length is {declared}"
            ),
            Self::EntryRangeOverflow {
                entry,
                offset,
                size,
            } => write!(
                formatter,
                "WebData entry {entry} range {offset}+{size} overflows"
            ),
            Self::EntryOutOfBounds {
                entry,
                offset,
                size,
                input_size,
            } => write!(
                formatter,
                "WebData entry {entry} range {offset}..{} exceeds input size {input_size}",
                offset.saturating_add(*size)
            ),
            Self::AllocationFailed { requested } => {
                write!(formatter, "could not allocate WebData storage for {requested} items")
            }
            Self::WriteIntegerOverflow { field, value } => {
                write!(formatter, "WebData {field} value {value} exceeds its 32-bit field")
            }
            Self::OutputLengthOverflow => write!(formatter, "WebData output length overflows"),
            Self::OutputLimitExceeded { requested, limit } => write!(
                formatter,
                "WebData output size {requested} exceeds configured limit {limit}"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{WebFile, WebFileErrorKind, WebFileLimits};

    fn encoded_7bit_length(mut length: usize) -> Vec<u8> {
        let mut bytes = Vec::new();
        while length >= 0x80 {
            bytes.push((length as u8) | 0x80);
            length >>= 7;
        }
        bytes.push(length as u8);
        bytes
    }

    fn web_file_with_one_entry(name: &str, data: &[u8]) -> Vec<u8> {
        let mut bytes = b"UnityWebData1.0\0".to_vec();
        bytes.extend_from_slice(&[0; 4]);
        let name_length = encoded_7bit_length(name.len());
        let header_size = bytes.len() + 4 + 4 + name_length.len() + name.len();
        let entry_offset = header_size;
        bytes.extend_from_slice(&(entry_offset as i32).to_le_bytes());
        bytes.extend_from_slice(&(data.len() as i32).to_le_bytes());
        bytes.extend_from_slice(&name_length);
        bytes.extend_from_slice(name.as_bytes());
        bytes[16..20].copy_from_slice(&(header_size as i32).to_le_bytes());
        bytes.extend_from_slice(data);
        bytes
    }

    #[test]
    fn parses_source_layout_and_borrows_entry_ranges() {
        let input = web_file_with_one_entry("assets/main.data", b"serialized bytes");
        assert!(WebFile::is_web_file(&input));
        let parsed = WebFile::parse(&input).unwrap();

        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.entries[0].name, "assets/main.data");
        assert_eq!(parsed.entries[0].offset as usize, parsed.header_size);
        assert_eq!(parsed.entry_data(0), Some(&b"serialized bytes"[..]));
    }

    #[test]
    fn writer_round_trips_entries_with_and_without_alignment() {
        let input = web_file_with_one_entry("a", b"one");
        let parsed = WebFile::parse(&input).unwrap();
        for align_entries in [false, true] {
            let written = parsed.write(align_entries, 1024).unwrap();
            let reread = WebFile::parse(&written).unwrap();
            assert_eq!(reread.entries.len(), parsed.entries.len());
            assert_eq!(reread.entries[0].name, parsed.entries[0].name);
            assert_eq!(reread.entry_data(0), parsed.entry_data(0));
            if align_entries {
                assert_eq!(reread.entries[0].offset & 3, 0);
            }
        }
    }

    #[test]
    fn reads_and_writes_multibyte_seven_bit_name_lengths() {
        let name = "a".repeat(130);
        let input = web_file_with_one_entry(&name, b"data");
        assert_eq!(&input[28..30], &[0x82, 0x01]);

        let parsed = WebFile::parse(&input).unwrap();
        assert_eq!(parsed.entries[0].name, name);
        let written = parsed.write(false, 1024).unwrap();
        let name_length_position = 28;
        assert_eq!(
            &written[name_length_position..name_length_position + 2],
            &[0x82, 0x01]
        );
        assert_eq!(WebFile::parse(&written).unwrap().entries[0].name, name);
    }

    #[test]
    fn rejects_invalid_directory_lengths_and_entry_ranges() {
        let mut invalid_header = web_file_with_one_entry("x", b"data");
        invalid_header[16..20].copy_from_slice(&29i32.to_le_bytes());
        assert!(matches!(
            WebFile::parse(&invalid_header).unwrap_err().kind(),
            WebFileErrorKind::DirectoryLengthMismatch { .. }
                | WebFileErrorKind::HeaderLengthBeforeDirectory { .. }
                | WebFileErrorKind::DirectoryEntryCrossesHeader { .. }
        ));

        let mut invalid_entry = web_file_with_one_entry("x", b"data");
        invalid_entry[20..24].copy_from_slice(&i32::MAX.to_le_bytes());
        assert!(matches!(
            WebFile::parse(&invalid_entry).unwrap_err().kind(),
            WebFileErrorKind::EntryOutOfBounds { .. }
        ));
    }

    #[test]
    fn enforces_entry_and_output_limits() {
        let input = web_file_with_one_entry("x", b"payload");
        let error = WebFile::parse_with_limits(
            &input,
            WebFileLimits {
                max_entry_bytes: 4,
                ..WebFileLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            error.kind(),
            WebFileErrorKind::EntrySizeLimitExceeded { limit: 4, .. }
        ));

        let parsed = WebFile::parse(&input).unwrap();
        let error = parsed.write(false, 8).unwrap_err();
        assert!(matches!(
            error.kind(),
            WebFileErrorKind::OutputLimitExceeded { .. }
        ));
    }
}
