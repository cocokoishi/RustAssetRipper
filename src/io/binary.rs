use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::str;

/// Byte order for an input structure. Callers select this from the format specification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Endian {
    Big,
    Little,
}

/// A low-level read failure with the absolute byte offset and any parser context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BinaryError {
    offset: usize,
    kind: BinaryErrorKind,
    contexts: Vec<String>,
}

impl BinaryError {
    fn new(offset: usize, kind: BinaryErrorKind) -> Self {
        Self {
            offset,
            kind,
            contexts: Vec::new(),
        }
    }

    /// Absolute offset in the root byte slice where the failed operation began.
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// The specific reason the read failed.
    pub fn kind(&self) -> &BinaryErrorKind {
        &self.kind
    }

    /// Add a field, object, or parsing-stage label while returning an error to a caller.
    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.contexts.push(context.into());
        self
    }
}

impl Display for BinaryError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte offset {}", self.kind, self.offset)?;
        for context in self.contexts.iter().rev() {
            write!(f, " while parsing {context}")?;
        }
        Ok(())
    }
}

impl Error for BinaryError {}

/// Causes reported by [`BinaryReader`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BinaryErrorKind {
    UnexpectedEof {
        requested: usize,
        remaining: usize,
    },
    OffsetOverflow {
        requested: usize,
    },
    InvalidAlignment {
        alignment: usize,
    },
    InvalidUtf8 {
        valid_up_to: usize,
    },
    UnterminatedString {
        maximum_length: usize,
        remaining: usize,
    },
}

impl Display for BinaryErrorKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEof {
                requested,
                remaining,
            } => write!(
                f,
                "truncated input (requested {requested} bytes, {remaining} remain)"
            ),
            Self::OffsetOverflow { requested } => {
                write!(
                    f,
                    "byte range overflows the address space (requested {requested} bytes)"
                )
            }
            Self::InvalidAlignment { alignment } => {
                write!(f, "alignment {alignment} is not a nonzero power of two")
            }
            Self::InvalidUtf8 { valid_up_to } => {
                write!(f, "invalid UTF-8 after {valid_up_to} valid bytes")
            }
            Self::UnterminatedString {
                maximum_length,
                remaining,
            } => write!(
                f,
                "null-terminated string not found within {maximum_length} bytes ({remaining} remain)"
            ),
        }
    }
}

/// A bounded view over an input slice.
///
/// Reads do not advance the cursor when they fail. Sub-readers retain their absolute base
/// offset so errors remain locatable in the original input. Alignment is relative to the
/// absolute stream offset, as it is for aligned Unity binary records.
#[derive(Debug)]
pub struct BinaryReader<'a> {
    data: &'a [u8],
    position: usize,
    base_offset: usize,
    endian: Endian,
}

impl<'a> BinaryReader<'a> {
    /// Create a reader over a complete input slice.
    pub fn new(data: &'a [u8], endian: Endian) -> Self {
        Self {
            data,
            position: 0,
            base_offset: 0,
            endian,
        }
    }

    /// Create a reader over a subrange while retaining the absolute root-input offset.
    pub fn with_base_offset(
        data: &'a [u8],
        endian: Endian,
        base_offset: usize,
    ) -> Result<Self, BinaryError> {
        if base_offset.checked_add(data.len()).is_none() {
            return Err(BinaryError::new(
                base_offset,
                BinaryErrorKind::OffsetOverflow {
                    requested: data.len(),
                },
            ));
        }
        Ok(Self {
            data,
            position: 0,
            base_offset,
            endian,
        })
    }

    /// Current position relative to this reader's slice.
    pub fn position(&self) -> usize {
        self.position
    }

    /// Current position relative to the root input slice.
    pub fn offset(&self) -> usize {
        self.base_offset + self.position
    }

    /// Length of this reader's bounded slice.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Number of bytes that can still be read from this slice.
    pub fn remaining(&self) -> usize {
        self.data.len() - self.position
    }

    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    pub fn endian(&self) -> Endian {
        self.endian
    }

    pub fn set_endian(&mut self, endian: Endian) {
        self.endian = endian;
    }

    /// Read exactly `length` bytes without allocating.
    pub fn read_exact(&mut self, length: usize) -> Result<&'a [u8], BinaryError> {
        let range = self.checked_range(length)?;
        self.position = range.end;
        Ok(&self.data[range])
    }

    /// Advance over exactly `length` bytes.
    pub fn skip(&mut self, length: usize) -> Result<(), BinaryError> {
        self.read_exact(length).map(|_| ())
    }

    /// Return a bounded child reader and advance this reader by the child's length.
    pub fn read_subreader(&mut self, length: usize) -> Result<BinaryReader<'a>, BinaryError> {
        let child_base = self.offset();
        let bytes = self.read_exact(length)?;
        Ok(BinaryReader {
            data: bytes,
            position: 0,
            base_offset: child_base,
            endian: self.endian,
        })
    }

    /// Align the absolute stream position to a nonzero power-of-two boundary.
    pub fn align(&mut self, alignment: usize) -> Result<(), BinaryError> {
        if alignment == 0 || !alignment.is_power_of_two() {
            return Err(BinaryError::new(
                self.offset(),
                BinaryErrorKind::InvalidAlignment { alignment },
            ));
        }

        let mask = alignment - 1;
        let padding = (alignment - (self.offset() & mask)) & mask;
        self.skip(padding)
    }

    /// Read a UTF-8 string from the next `length` bytes without allocating.
    ///
    /// Invalid UTF-8 leaves the cursor unchanged so the caller can report or recover from
    /// the exact field that failed.
    pub fn read_utf8(&mut self, length: usize) -> Result<&'a str, BinaryError> {
        let range = self.checked_range(length)?;
        let value = str::from_utf8(&self.data[range.clone()]).map_err(|error| {
            BinaryError::new(
                self.offset(),
                BinaryErrorKind::InvalidUtf8 {
                    valid_up_to: error.valid_up_to(),
                },
            )
        })?;
        self.position = range.end;
        Ok(value)
    }

    /// Read a null-terminated UTF-8 string whose contents are at most `maximum_length` bytes.
    /// The cursor is unchanged if the terminator is absent or the contents are invalid UTF-8.
    pub fn read_null_terminated_utf8(
        &mut self,
        maximum_length: usize,
    ) -> Result<&'a str, BinaryError> {
        let start = self.position;
        let search_length = self.remaining().min(maximum_length.saturating_add(1));
        let search_end = start + search_length;
        let bytes = &self.data[start..search_end];
        let terminator = bytes.iter().position(|byte| *byte == 0).ok_or_else(|| {
            BinaryError::new(
                self.offset(),
                BinaryErrorKind::UnterminatedString {
                    maximum_length,
                    remaining: self.remaining(),
                },
            )
        })?;
        if terminator > maximum_length {
            return Err(BinaryError::new(
                self.offset(),
                BinaryErrorKind::UnterminatedString {
                    maximum_length,
                    remaining: self.remaining(),
                },
            ));
        }

        let value = str::from_utf8(&bytes[..terminator]).map_err(|error| {
            BinaryError::new(
                self.offset(),
                BinaryErrorKind::InvalidUtf8 {
                    valid_up_to: error.valid_up_to(),
                },
            )
        })?;
        self.position = start + terminator + 1;
        Ok(value)
    }

    pub fn read_u8(&mut self) -> Result<u8, BinaryError> {
        Ok(self.read_exact(1)?[0])
    }

    pub fn read_i8(&mut self) -> Result<i8, BinaryError> {
        Ok(self.read_u8()? as i8)
    }

    pub fn read_u16(&mut self) -> Result<u16, BinaryError> {
        let bytes = self.read_exact(2)?;
        let array = [bytes[0], bytes[1]];
        Ok(match self.endian {
            Endian::Big => u16::from_be_bytes(array),
            Endian::Little => u16::from_le_bytes(array),
        })
    }

    pub fn read_i16(&mut self) -> Result<i16, BinaryError> {
        let bytes = self.read_exact(2)?;
        let array = [bytes[0], bytes[1]];
        Ok(match self.endian {
            Endian::Big => i16::from_be_bytes(array),
            Endian::Little => i16::from_le_bytes(array),
        })
    }

    pub fn read_u32(&mut self) -> Result<u32, BinaryError> {
        let bytes = self.read_exact(4)?;
        let array = [bytes[0], bytes[1], bytes[2], bytes[3]];
        Ok(match self.endian {
            Endian::Big => u32::from_be_bytes(array),
            Endian::Little => u32::from_le_bytes(array),
        })
    }

    pub fn read_i32(&mut self) -> Result<i32, BinaryError> {
        let bytes = self.read_exact(4)?;
        let array = [bytes[0], bytes[1], bytes[2], bytes[3]];
        Ok(match self.endian {
            Endian::Big => i32::from_be_bytes(array),
            Endian::Little => i32::from_le_bytes(array),
        })
    }

    pub fn read_u64(&mut self) -> Result<u64, BinaryError> {
        let bytes = self.read_exact(8)?;
        let array = [
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ];
        Ok(match self.endian {
            Endian::Big => u64::from_be_bytes(array),
            Endian::Little => u64::from_le_bytes(array),
        })
    }

    pub fn read_i64(&mut self) -> Result<i64, BinaryError> {
        let bytes = self.read_exact(8)?;
        let array = [
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ];
        Ok(match self.endian {
            Endian::Big => i64::from_be_bytes(array),
            Endian::Little => i64::from_le_bytes(array),
        })
    }

    pub fn read_u128(&mut self) -> Result<u128, BinaryError> {
        let bytes = self.read_exact(16)?;
        let array = [
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        ];
        Ok(match self.endian {
            Endian::Big => u128::from_be_bytes(array),
            Endian::Little => u128::from_le_bytes(array),
        })
    }

    pub fn read_f32(&mut self) -> Result<f32, BinaryError> {
        self.read_u32().map(f32::from_bits)
    }

    pub fn read_f64(&mut self) -> Result<f64, BinaryError> {
        self.read_u64().map(f64::from_bits)
    }

    fn checked_range(&self, length: usize) -> Result<std::ops::Range<usize>, BinaryError> {
        let end = self.position.checked_add(length).ok_or_else(|| {
            BinaryError::new(
                self.offset(),
                BinaryErrorKind::OffsetOverflow { requested: length },
            )
        })?;
        if end > self.data.len() {
            return Err(BinaryError::new(
                self.offset(),
                BinaryErrorKind::UnexpectedEof {
                    requested: length,
                    remaining: self.remaining(),
                },
            ));
        }
        Ok(self.position..end)
    }
}

#[cfg(test)]
mod tests {
    use super::{BinaryErrorKind, BinaryReader, Endian};

    #[test]
    fn integer_reads_follow_explicit_endianness() {
        let input = [0x01, 0x02, 0x03, 0x04];
        let mut big = BinaryReader::new(&input, Endian::Big);
        let mut little = BinaryReader::new(&input, Endian::Little);

        assert_eq!(big.read_u32().unwrap(), 0x0102_0304);
        assert_eq!(little.read_u32().unwrap(), 0x0403_0201);
        assert_eq!(big.position(), 4);
        assert_eq!(little.position(), 4);
    }

    #[test]
    fn signed_and_float_reads_preserve_bit_patterns() {
        let input = [0xff, 0xff, 0xff, 0xfe, 0x3f, 0x80, 0, 0];
        let mut reader = BinaryReader::new(&input, Endian::Big);

        assert_eq!(reader.read_i32().unwrap(), -2);
        assert_eq!(reader.read_f32().unwrap().to_bits(), 0x3f80_0000);
    }

    #[test]
    fn truncated_reads_report_offset_and_keep_cursor() {
        let input = [0xaa, 0xbb];
        let mut reader = BinaryReader::new(&input, Endian::Little);
        let error = reader.read_u32().unwrap_err();

        assert_eq!(error.offset(), 0);
        assert_eq!(
            error.kind(),
            &BinaryErrorKind::UnexpectedEof {
                requested: 4,
                remaining: 2,
            }
        );
        assert_eq!(reader.position(), 0);
    }

    #[test]
    fn absolute_alignment_works_inside_subreaders() {
        let input = [0, 1, 2, 3, 4, 5];
        let mut parent = BinaryReader::new(&input, Endian::Little);
        parent.skip(1).unwrap();
        let mut child = parent.read_subreader(4).unwrap();

        assert_eq!(child.offset(), 1);
        child.align(4).unwrap();
        assert_eq!(child.offset(), 4);
        assert_eq!(child.read_u8().unwrap(), 4);
        assert_eq!(parent.position(), 5);
    }

    #[test]
    fn invalid_alignment_is_rejected_without_advancing() {
        let input = [0, 1, 2, 3];
        let mut reader = BinaryReader::new(&input, Endian::Little);
        reader.skip(1).unwrap();
        let error = reader.align(3).unwrap_err();

        assert_eq!(error.offset(), 1);
        assert_eq!(
            error.kind(),
            &BinaryErrorKind::InvalidAlignment { alignment: 3 }
        );
        assert_eq!(reader.position(), 1);
    }

    #[test]
    fn invalid_utf8_does_not_advance_and_context_is_preserved() {
        let input = [0xff];
        let mut reader = BinaryReader::new(&input, Endian::Little);
        let error = reader
            .read_utf8(1)
            .unwrap_err()
            .with_context("metadata string");

        assert_eq!(error.offset(), 0);
        assert!(error.to_string().contains("metadata string"));
        assert_eq!(reader.position(), 0);
    }

    #[test]
    fn null_terminated_utf8_respects_length_and_keeps_following_bytes() {
        let input = b"abc\0tail";
        let mut reader = BinaryReader::new(input, Endian::Little);

        assert_eq!(reader.read_null_terminated_utf8(3).unwrap(), "abc");
        assert_eq!(reader.position(), 4);
        assert_eq!(reader.read_exact(4).unwrap(), b"tail");
    }

    #[test]
    fn missing_or_overlong_string_terminator_does_not_advance() {
        let input = b"abcd\0";
        let mut reader = BinaryReader::new(input, Endian::Little);
        let error = reader.read_null_terminated_utf8(3).unwrap_err();

        assert_eq!(error.offset(), 0);
        assert!(matches!(
            error.kind(),
            BinaryErrorKind::UnterminatedString {
                maximum_length: 3,
                remaining: 5
            }
        ));
        assert_eq!(reader.position(), 0);
    }

    #[test]
    fn offset_overflow_is_reported_without_advancing() {
        let input = [0];
        let mut reader = BinaryReader::new(&input, Endian::Little);
        reader.skip(1).unwrap();
        let error = reader.read_exact(usize::MAX).unwrap_err();

        assert_eq!(error.offset(), 1);
        assert_eq!(
            error.kind(),
            &BinaryErrorKind::OffsetOverflow {
                requested: usize::MAX,
            }
        );
        assert_eq!(reader.position(), 1);
    }
}
