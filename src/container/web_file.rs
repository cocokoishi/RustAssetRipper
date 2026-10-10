//! Unity WebData containers used for web player resources.
//!
//! Ports the `WebFile`/`WebFileEntry` directory parsing and writing plus the
//! `WebFileScheme` sniff shape (`is_web_file` needs the signature and its zero
//! terminator, mirroring the strict `Length - Position > Signature.Length`
//! room check). Entry display names follow `WebFileEntry.ToString`
//! (`FixFileIdentifier`). Entry payload mounting as `ResourceFile` objects stays with the
//! scheme dispatcher, and inputs not starting at the container base are the
//! caller's to slice.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::io::binary::{BinaryError, BinaryReader, Endian};
use crate::io::special_names::fix_file_identifier;

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

impl Display for WebFileEntry {
    /// `WebFileEntry.ToString`: the entry name normalized like every container file
    /// identifier. Mounting applies the same rule through the scheme dispatcher;
    /// this display keeps entry listings consistent without touching entry bytes.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&fix_file_identifier(&self.name))
    }
}

/// A borrowed view of a Unity WebData file.
#[derive(Debug)]
pub struct WebFile<'a> {
    input: &'a [u8],
    pub entries: Vec<WebFileEntry>,
    pub header_size: usize,
}

/// Parsed WebData directory records without a borrowed whole-file buffer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct WebFileDirectory {
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
        let directory = WebFileDirectory::parse_header_with_limits(input, input.len(), limits)?;
        Ok(Self {
            input,
            entries: directory.entries,
            header_size: directory.header_size,
        })
    }
}

impl WebFileDirectory {
    pub(crate) fn header_size_from_prefix(
        prefix: &[u8],
        input_size: usize,
        limits: WebFileLimits,
    ) -> Result<usize, WebFileError> {
        let mut reader = BinaryReader::new(prefix, Endian::Little);
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
        if header_size > input_size {
            return Err(WebFileError::new(WebFileErrorKind::HeaderOutOfBounds {
                declared: header_size,
                input_size,
            }));
        }
        Ok(header_size)
    }

    pub(crate) fn parse_header_with_limits(
        header_bytes: &[u8],
        input_size: usize,
        limits: WebFileLimits,
    ) -> Result<WebFileDirectory, WebFileError> {
        let header_size = Self::header_size_from_prefix(header_bytes, input_size, limits)?;
        if header_size > header_bytes.len() {
            return Err(WebFileError::new(WebFileErrorKind::HeaderBytesIncomplete {
                declared: header_size,
                available: header_bytes.len(),
            }));
        }

        let available_size = header_bytes.len().min(input_size);
        let mut reader = BinaryReader::new(&header_bytes[..available_size], Endian::Little);
        reader
            .read_null_terminated_utf8(WEB_FILE_SIGNATURE.len() + 1)
            .map_err(|error| WebFileError::binary(error, "WebData signature"))?;
        reader
            .read_i32()
            .map_err(|error| WebFileError::binary(error, "WebData header length"))?;

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

            if header_bytes.len() == header_size {
                if let Some(position) = reader.position().checked_add(4) {
                    if position > header_size && position <= input_size {
                        return Err(WebFileError::new(
                            WebFileErrorKind::DirectoryEntryCrossesHeader {
                                entry: entries.len(),
                                record_offset,
                                header_size,
                                position,
                            },
                        ));
                    }
                }
            }

            // Entry names use a four-byte little-endian length prefix, matching
            // `EndianWriter.Write(string)` behind `WebFileEntry.Write` (proven
            // against reference-written files; see the differential test).
            let name_length = reader
                .read_u32()
                .map_err(|error| WebFileError::binary(error, "WebData entry name length"))?
                as usize;
            if name_length > limits.max_entry_name_bytes {
                return Err(WebFileError::new(WebFileErrorKind::NameLimitExceeded {
                    entry: entries.len(),
                    declared: name_length,
                    limit: limits.max_entry_name_bytes,
                }));
            }
            if let Some(position) = reader.position().checked_add(name_length) {
                if position > header_size && position <= input_size {
                    return Err(WebFileError::new(
                        WebFileErrorKind::DirectoryEntryCrossesHeader {
                            entry: entries.len(),
                            record_offset,
                            header_size,
                            position,
                        },
                    ));
                }
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

            let range = entry_byte_range(entries.len(), offset as u32, size as u32, input_size)?;

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

        Ok(WebFileDirectory {
            entries,
            header_size,
        })
    }
}

impl<'a> WebFile<'a> {
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
            offset_positions.push(output.len());
            push_bytes(&mut output, &[0; 4], maximum_output_size)?;
            let size = write_i32_length(data.len(), "entry size")?;
            push_bytes(&mut output, &size.to_le_bytes(), maximum_output_size)?;
            let name = entry.name.as_bytes();
            let name_length = u32::try_from(name.len()).map_err(|_| {
                WebFileError::new(WebFileErrorKind::WriteIntegerOverflow {
                    field: "entry name length",
                    value: name.len(),
                })
            })?;
            push_bytes(&mut output, &name_length.to_le_bytes(), maximum_output_size)?;
            push_bytes(&mut output, name, maximum_output_size)?;
        }

        let header_length = write_i32_length(output.len(), "header length")?;
        output[header_length_position..header_length_position + 4]
            .copy_from_slice(&header_length.to_le_bytes());

        for (index, entry) in self.entries.iter().enumerate() {
            if align_entries {
                let padding = entry_padding(output.len());
                push_bytes(&mut output, &[0; 3][..padding], maximum_output_size)?;
            }
            let offset = write_i32_length(output.len(), "entry offset")?;
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

/// Four-byte data-alignment step used when writing entry payloads.
///
/// Mirrors the `EndianWriter.AlignStream` call in `WebFile.Write`: padding bytes
/// bring the output length up to a multiple of four.
fn entry_padding(output_len: usize) -> usize {
    (4 - (output_len & 3)) & 3
}

/// Checked entry payload range over caller facts: the stored offset and size widen
/// without wrapping (`EntryRangeOverflow`), then the range must land inside the input
/// (`EntryOutOfBounds`). This mirrors the `entry.Offset + basePosition` addressing and
/// the exact-sized payload read of `WebFile.Read`; inputs not starting at the container
/// base are the caller's to slice, as documented above. Callers pass the offset and
/// size after the negativity checks, so the `u32` widening cannot wrap. On 64-bit
/// targets `u32` pairs cannot overflow `usize`, so the overflow arm only fires on
/// 32-bit targets; it is preserved from the inline check it replaces.
fn entry_byte_range(
    entry: usize,
    offset: u32,
    size: u32,
    input_size: usize,
) -> Result<std::ops::Range<usize>, WebFileError> {
    let start = offset as usize;
    let end = start.checked_add(size as usize).ok_or_else(|| {
        WebFileError::new(WebFileErrorKind::EntryRangeOverflow {
            entry,
            offset: start,
            size: size as usize,
        })
    })?;
    if end > input_size {
        return Err(WebFileError::new(WebFileErrorKind::EntryOutOfBounds {
            entry,
            offset: start,
            size: size as usize,
            input_size,
        }));
    }
    Ok(start..end)
}

/// Checked `usize`-to-`i32` narrowing for written directory integers over caller facts:
/// the baseline's unchecked `(int)` casts would wrap past 2 GiB, so values that do not
/// fit fail with the naming field instead. One decision point serves the entry size,
/// header length, and entry offset alike; the overflow arms are unreachable through
/// `write` on realistic inputs, so the kernel keeps them provable.
fn write_i32_length(value: usize, field: &'static str) -> Result<i32, WebFileError> {
    if value > i32::MAX as usize {
        return Err(WebFileError::new(WebFileErrorKind::WriteIntegerOverflow {
            field,
            value,
        }));
    }
    Ok(value as i32)
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
    HeaderBytesIncomplete {
        declared: usize,
        available: usize,
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
            Self::HeaderBytesIncomplete {
                declared,
                available,
            } => write!(
                formatter,
                "WebData directory needs {declared} header bytes, but only {available} were read"
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
                write!(formatter, "invalid WebData string length")
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
    use super::{WebFile, WebFileEntry, WebFileErrorKind, WebFileLimits};

    fn encoded_name_length(length: usize) -> Vec<u8> {
        (length as u32).to_le_bytes().to_vec()
    }

    fn web_file_with_one_entry(name: &str, data: &[u8]) -> Vec<u8> {
        let mut bytes = b"UnityWebData1.0\0".to_vec();
        bytes.extend_from_slice(&[0; 4]);
        let name_length = encoded_name_length(name.len());
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
    fn reads_and_writes_long_names_with_four_byte_lengths() {
        let name = "a".repeat(130);
        let input = web_file_with_one_entry(&name, b"data");
        assert_eq!(&input[28..32], &[0x82, 0x00, 0x00, 0x00]);

        let parsed = WebFile::parse(&input).unwrap();
        assert_eq!(parsed.entries[0].name, name);
        let written = parsed.write(false, 1024).unwrap();
        let name_length_position = 28;
        assert_eq!(
            &written[name_length_position..name_length_position + 4],
            &[0x82, 0x00, 0x00, 0x00]
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

    #[test]
    fn sniff_needs_the_signature_and_its_terminator() {
        // The source requires room past the signature (`Length - Position >
        // Signature.Length`), so a bare 15-byte signature never matches.
        assert!(!WebFile::is_web_file(&[]));
        assert!(!WebFile::is_web_file(b"UnityWebData1.0"));
        assert!(!WebFile::is_web_file(b"UnityWebData1.0X"));
        assert!(!WebFile::is_web_file(b"0123456789abcdef"));
        assert!(WebFile::is_web_file(b"UnityWebData1.0\0"));
        assert!(WebFile::is_web_file(b"UnityWebData1.0\0extra"));
    }

    #[test]
    fn entry_padding_matches_four_byte_alignment() {
        assert_eq!(super::entry_padding(0), 0);
        assert_eq!(super::entry_padding(1), 3);
        assert_eq!(super::entry_padding(2), 2);
        assert_eq!(super::entry_padding(3), 1);
        assert_eq!(super::entry_padding(4), 0);
        assert_eq!(super::entry_padding(5), 3);
        assert_eq!(super::entry_padding(8), 0);
    }

    #[test]
    fn entry_ranges_accept_inside_reject_outside() {
        assert_eq!(super::entry_byte_range(0, 20, 8, 64), Ok(20..28));
        assert_eq!(super::entry_byte_range(0, 20, 0, 20), Ok(20..20));
        let error = super::entry_byte_range(2, 60, 8, 64).unwrap_err();
        assert!(matches!(
            error.kind(),
            WebFileErrorKind::EntryOutOfBounds {
                entry: 2,
                offset: 60,
                size: 8,
                input_size: 64,
            }
        ));
        // The parse path applies the same decision: a past-end entry is rejected.
        let mut overrun = web_file_with_one_entry("x", b"data");
        overrun[20..24].copy_from_slice(&i32::MAX.to_le_bytes());
        assert!(matches!(
            WebFile::parse(&overrun).unwrap_err().kind(),
            WebFileErrorKind::EntryOutOfBounds { .. }
        ));
    }

    #[test]
    fn written_integers_narrow_to_i32_or_fail() {
        assert_eq!(super::write_i32_length(0, "entry size"), Ok(0));
        assert_eq!(
            super::write_i32_length(i32::MAX as usize, "header length"),
            Ok(i32::MAX)
        );
        let error = super::write_i32_length(i32::MAX as usize + 1, "entry offset").unwrap_err();
        assert!(matches!(
            error.kind(),
            WebFileErrorKind::WriteIntegerOverflow { field, value }
                if *field == "entry offset" && *value == i32::MAX as usize + 1
        ));
    }

    fn web_file_with_entries(records: &[(&str, &[u8])]) -> Vec<u8> {
        let mut directory_len = 0usize;
        for (name, _) in records {
            directory_len += 4 + 4 + encoded_name_length(name.len()).len() + name.len();
        }
        let header_size = 20 + directory_len;
        let mut bytes = b"UnityWebData1.0\0".to_vec();
        bytes.extend_from_slice(&(header_size as i32).to_le_bytes());
        let mut offset = header_size;
        for (name, data) in records {
            bytes.extend_from_slice(&(offset as i32).to_le_bytes());
            bytes.extend_from_slice(&(data.len() as i32).to_le_bytes());
            bytes.extend_from_slice(&encoded_name_length(name.len()));
            bytes.extend_from_slice(name.as_bytes());
            offset += data.len();
        }
        for (_, data) in records {
            bytes.extend_from_slice(data);
        }
        bytes
    }

    #[test]
    fn parses_a_header_with_no_entries() {
        let mut bytes = b"UnityWebData1.0\0".to_vec();
        bytes.extend_from_slice(&20i32.to_le_bytes());
        let parsed = WebFile::parse(&bytes).unwrap();
        assert!(parsed.entries.is_empty());
        assert_eq!(parsed.header_size, 20);
        let written = parsed.write(true, 1024).unwrap();
        assert_eq!(WebFile::parse(&written).unwrap().entries.len(), 0);
    }

    #[test]
    fn parses_and_round_trips_multiple_entries_with_empty_names_and_data() {
        let input = web_file_with_entries(&[("a", b"one"), ("", b""), ("b/c", b"12")]);
        let parsed = WebFile::parse(&input).unwrap();
        assert_eq!(parsed.entries.len(), 3);
        assert_eq!(parsed.entries[1].name, "");
        assert_eq!(parsed.entry_data(1), Some(&b""[..]));
        assert_eq!(parsed.entry_data(2), Some(&b"12"[..]));
        for align_entries in [false, true] {
            let written = parsed.write(align_entries, 4096).unwrap();
            let reread = WebFile::parse(&written).unwrap();
            assert_eq!(reread.entries.len(), 3);
            for index in 0..3 {
                assert_eq!(reread.entries[index].name, parsed.entries[index].name);
                assert_eq!(reread.entry_data(index), parsed.entry_data(index));
            }
            if align_entries {
                for entry in &reread.entries {
                    assert_eq!(entry.offset & 3, 0);
                }
            }
        }
    }

    #[test]
    fn entry_display_normalizes_names_like_container_identifiers() {
        fn entry(name: &str) -> WebFileEntry {
            WebFileEntry {
                name: name.to_owned(),
                offset: 0,
                size: 0,
                range: 0..0,
            }
        }
        assert_eq!(entry("Resource.resource").to_string(), "resource.resource");
        assert_eq!(entry("library/Shared.Assets").to_string(), "shared.assets");
        assert_eq!(entry("archive:/Bundles/MainData").to_string(), "maindata");
    }

    #[test]
    fn directory_and_entries_match_reference_written_files() {
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

        // Probe Temp/opencode/legacybundleprobe (WebFile.Write plus Read over
        // reference-written files with align on/off, SDK 10.0.401; D3 is a
        // C#-constructed bad offset): D1/D2 carry a 200-char name exercising
        // the four-byte length; D3 fails loud on both sides.
        // D1/D2 are reference-generated, not Rust-generated.
        let fixture = include_str!("legacy_bundle_fixture.txt");
        let mut hexes: std::collections::HashMap<String, Vec<u8>> =
            std::collections::HashMap::new();
        let mut reads: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for line in fixture.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            if line.starts_with("LEGACY_HEX|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad LEGACY_HEX {line}");
                hexes.insert(parts[1].to_string(), hex_to_bytes(parts[2]));
            } else if line.starts_with("LEGACY_READ|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad LEGACY_READ {line}");
                reads.insert(parts[1].to_string(), parts[2].to_string());
            }
        }

        let long_name = format!("{}.bin", "n".repeat(200));
        for case in ["D1", "D2"] {
            let bytes = hexes.get(case).unwrap_or_else(|| panic!("missing {case}"));
            let dump = reads.get(case).unwrap_or_else(|| panic!("missing {case}"));
            assert!(
                dump.contains("FILE:a.bin=68656C6C6F20776562"),
                "{case} {dump}"
            );
            assert!(
                dump.contains("7365636F6E6420656E747279207061796C6F6164"),
                "{case} {dump}"
            );
            assert!(WebFile::is_web_file(bytes), "{case} sniff");
            let file =
                WebFile::parse(bytes).unwrap_or_else(|e| panic!("{case} Rust parse failed: {e:?}"));
            assert_eq!(file.entries.len(), 2, "{case} entries");
            assert_eq!(file.entries[0].name, "a.bin");
            assert_eq!(file.entries[1].name, long_name, "{case} long name");
            assert_eq!(file.entry_data(0).unwrap(), b"hello web");
            assert_eq!(file.entry_data(1).unwrap(), b"second entry payload");
        }

        // Bad offsets fail loud on both sides (source behavior now executed).
        {
            let bytes = hexes.get("D3").expect("D3 hex");
            let dump = reads.get("D3").expect("D3 dump");
            assert!(dump.starts_with("THROW:"), "D3 refusal {dump}");
            assert!(WebFile::parse(bytes).is_err(), "D3 Rust fails loud");
        }
    }
}
