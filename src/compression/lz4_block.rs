use std::error::Error;
use std::fmt::{self, Display, Formatter};

/// Decode a raw LZ4 block to exactly the size declared by its container.
pub fn decompress_block(
    input: &[u8],
    expected_size: usize,
    maximum_output_size: usize,
) -> Result<Vec<u8>, Lz4BlockError> {
    if expected_size > maximum_output_size {
        return Err(Lz4BlockError::new(
            0,
            Lz4BlockErrorKind::OutputLimitExceeded {
                expected: expected_size,
                limit: maximum_output_size,
            },
        ));
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(expected_size)
        .map_err(|_| Lz4BlockError::new(0, Lz4BlockErrorKind::AllocationFailed(expected_size)))?;

    let mut input_position = 0usize;
    while input_position < input.len() {
        let token_offset = input_position;
        let token = input[input_position];
        input_position += 1;

        let literal_nibble = usize::from(token >> 4);
        let literal_length = read_extended_length(input, &mut input_position, literal_nibble)?;
        let literal_end = input_position
            .checked_add(literal_length)
            .ok_or_else(|| Lz4BlockError::new(token_offset, Lz4BlockErrorKind::LengthOverflow))?;
        let literals = input.get(input_position..literal_end).ok_or_else(|| {
            Lz4BlockError::new(
                input_position,
                Lz4BlockErrorKind::TruncatedInput {
                    requested: literal_length,
                    remaining: input.len().saturating_sub(input_position),
                },
            )
        })?;
        ensure_output_fits(output.len(), literal_length, expected_size, token_offset)?;
        output.extend_from_slice(literals);
        input_position = literal_end;

        if input_position == input.len() {
            break;
        }

        let offset_end = input_position
            .checked_add(2)
            .ok_or_else(|| Lz4BlockError::new(input_position, Lz4BlockErrorKind::LengthOverflow))?;
        let offset_bytes = input.get(input_position..offset_end).ok_or_else(|| {
            Lz4BlockError::new(
                input_position,
                Lz4BlockErrorKind::TruncatedInput {
                    requested: 2,
                    remaining: input.len().saturating_sub(input_position),
                },
            )
        })?;
        let match_offset = usize::from(u16::from_le_bytes([offset_bytes[0], offset_bytes[1]]));
        input_position = offset_end;
        if match_offset == 0 || match_offset > output.len() {
            return Err(Lz4BlockError::new(
                input_position - 2,
                Lz4BlockErrorKind::InvalidMatchOffset {
                    match_offset,
                    produced: output.len(),
                },
            ));
        }

        let match_nibble = usize::from(token & 0x0f);
        let extended_match = read_extended_length(input, &mut input_position, match_nibble)?;
        let match_length = extended_match
            .checked_add(4)
            .ok_or_else(|| Lz4BlockError::new(token_offset, Lz4BlockErrorKind::LengthOverflow))?;
        ensure_output_fits(output.len(), match_length, expected_size, token_offset)?;

        for _ in 0..match_length {
            let source_index = output.len() - match_offset;
            let byte = output[source_index];
            output.push(byte);
        }
    }

    if output.len() != expected_size {
        return Err(Lz4BlockError::new(
            input_position,
            Lz4BlockErrorKind::OutputSizeMismatch {
                expected: expected_size,
                actual: output.len(),
            },
        ));
    }
    Ok(output)
}

fn read_extended_length(
    input: &[u8],
    input_position: &mut usize,
    nibble: usize,
) -> Result<usize, Lz4BlockError> {
    let mut length = nibble;
    if nibble == 15 {
        loop {
            let byte = input.get(*input_position).copied().ok_or_else(|| {
                Lz4BlockError::new(
                    *input_position,
                    Lz4BlockErrorKind::TruncatedInput {
                        requested: 1,
                        remaining: 0,
                    },
                )
            })?;
            *input_position += 1;
            length = length.checked_add(usize::from(byte)).ok_or_else(|| {
                Lz4BlockError::new(*input_position - 1, Lz4BlockErrorKind::LengthOverflow)
            })?;
            if byte != u8::MAX {
                break;
            }
        }
    }
    Ok(length)
}

fn ensure_output_fits(
    current: usize,
    additional: usize,
    expected: usize,
    input_offset: usize,
) -> Result<(), Lz4BlockError> {
    let requested = current
        .checked_add(additional)
        .ok_or_else(|| Lz4BlockError::new(input_offset, Lz4BlockErrorKind::LengthOverflow))?;
    if requested > expected {
        return Err(Lz4BlockError::new(
            input_offset,
            Lz4BlockErrorKind::OutputExceedsDeclaredSize {
                requested,
                expected,
            },
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Lz4BlockError {
    input_offset: usize,
    kind: Lz4BlockErrorKind,
}

impl Lz4BlockError {
    fn new(input_offset: usize, kind: Lz4BlockErrorKind) -> Self {
        Self { input_offset, kind }
    }

    pub fn input_offset(&self) -> usize {
        self.input_offset
    }

    pub fn kind(&self) -> &Lz4BlockErrorKind {
        &self.kind
    }
}

impl Display for Lz4BlockError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{} at compressed byte {}", self.kind, self.input_offset)
    }
}

impl Error for Lz4BlockError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Lz4BlockErrorKind {
    TruncatedInput {
        requested: usize,
        remaining: usize,
    },
    LengthOverflow,
    InvalidMatchOffset {
        match_offset: usize,
        produced: usize,
    },
    OutputLimitExceeded {
        expected: usize,
        limit: usize,
    },
    OutputExceedsDeclaredSize {
        requested: usize,
        expected: usize,
    },
    OutputSizeMismatch {
        expected: usize,
        actual: usize,
    },
    AllocationFailed(usize),
}

impl Display for Lz4BlockErrorKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::TruncatedInput {
                requested,
                remaining,
            } => write!(
                f,
                "truncated input (requested {requested} bytes, {remaining} remain)"
            ),
            Self::LengthOverflow => write!(f, "decoded length overflows the address space"),
            Self::InvalidMatchOffset {
                match_offset,
                produced,
            } => write!(
                f,
                "invalid match offset {match_offset} with only {produced} output bytes"
            ),
            Self::OutputLimitExceeded { expected, limit } => {
                write!(f, "declared output {expected} exceeds limit {limit}")
            }
            Self::OutputExceedsDeclaredSize {
                requested,
                expected,
            } => write!(
                f,
                "sequence would produce {requested} bytes, exceeding declared size {expected}"
            ),
            Self::OutputSizeMismatch { expected, actual } => {
                write!(f, "decoded {actual} bytes, expected {expected}")
            }
            Self::AllocationFailed(size) => {
                write!(f, "could not reserve {size} output bytes")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{decompress_block, Lz4BlockErrorKind};

    #[test]
    fn decodes_overlapping_matches_and_exact_literal_sequences() {
        let input = [
            0x83, b'a', b'b', b'c', b'd', b'e', b'f', b'g', b'h', 3, 0, 0x50, b'1', b'2', b'3',
            b'4', b'5',
        ];
        assert_eq!(
            decompress_block(&input, 20, 32).unwrap(),
            b"abcdefghfghfghf12345"
        );

        let literal_only = [0x30, b'a', b'b', b'c'];
        assert_eq!(decompress_block(&literal_only, 3, 3).unwrap(), b"abc");
    }

    #[test]
    fn malformed_offsets_lengths_and_expansion_limits_are_reported() {
        let error = decompress_block(&[0x00, 0, 0], 4, 4).unwrap_err();
        assert!(matches!(
            error.kind(),
            Lz4BlockErrorKind::InvalidMatchOffset {
                match_offset: 0,
                ..
            }
        ));

        let error = decompress_block(&[0xf0], 0, 32).unwrap_err();
        assert!(matches!(
            error.kind(),
            Lz4BlockErrorKind::TruncatedInput { .. }
        ));

        let error = decompress_block(&[0x00], 8, 7).unwrap_err();
        assert!(matches!(
            error.kind(),
            Lz4BlockErrorKind::OutputLimitExceeded {
                expected: 8,
                limit: 7
            }
        ));

        let error = decompress_block(&[0], 1, 1).unwrap_err();
        assert!(matches!(
            error.kind(),
            Lz4BlockErrorKind::OutputSizeMismatch {
                expected: 1,
                actual: 0
            }
        ));

        let error = decompress_block(&[0x20, b'a', b'b'], 1, 8).unwrap_err();
        assert!(matches!(
            error.kind(),
            Lz4BlockErrorKind::OutputExceedsDeclaredSize {
                requested: 2,
                expected: 1
            }
        ));
    }
}
