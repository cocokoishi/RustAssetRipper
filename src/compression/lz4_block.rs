//! Raw LZ4 block decoding and fast-level encoding for Unity bundle blocks.
//!
//! Decoding mirrors the K4os raw block decoder behavior behind UnityFS
//! compression types 2 (`Lz4`) and 3 (`Lz4HC`), following the
//! [LZ4 Block Format Description](https://github.com/lz4/lz4/blob/dev/doc/lz4_Block_format.md).
//! Encoding fills the matching write-path role (`LZ4Codec.Encode(..., L00_FAST)`
//! in `FileStreamBundleFile.WriteFileStreamMetadata`): a fast-level match finder
//! over a 12-bit hash table emitting spec-valid blocks — minimum match length 4,
//! 16-bit offsets, nibble-15 length extensions, and a literal-only five-byte tail.
//! Byte-identity with K4os output is not promised (match selection affects only
//! density); every emitted block round-trips through [`decompress_block`].
//!
//! Open: the LZ4 frame container, preset dictionaries, and the HC optimal parser
//! stay unported; UnityFS consumes raw blocks only.

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

/// Multiplier of the 32-bit LZ4 sequence hash.
const LZ4_HASH_MULTIPLIER: u32 = 2654435761;
/// Hash table bits of the fast-level match finder.
const LZ4_HASH_BITS: u32 = 12;
/// Minimum match length the block format can encode.
const LZ4_MIN_MATCH: usize = 4;
/// Remaining input bytes required to keep scanning for matches; the loop exits
/// earlier so the tail is always emitted as literals.
const LZ4_SEARCH_LIMIT: usize = 12;
/// Trailing bytes always emitted as literals, per the block format.
const LZ4_LAST_LITERALS: usize = 5;
/// Largest encodable match distance (offsets are 16-bit; zero is invalid).
const LZ4_MAX_DISTANCE: usize = 0xFFFF;

/// Compress a byte string into a raw LZ4 block at the baseline's fast level.
///
/// Fills the `LZ4Codec.Encode(uncompressedBytes, compressedBytes, LZ4Level.L00_FAST)`
/// role on the UnityFS write path (`FileStreamBundleFile.WriteFileStreamMetadata`):
/// a single-pass match finder over a 12-bit hash table emitting spec-valid blocks
/// with minimum match length 4, 16-bit match offsets, nibble-15 length extensions,
/// and a literal-only five-byte tail. An empty input encodes to the canonical
/// single endmark token. `maximum_output_size` bounds the reservation up front
/// against the worst case (`len + len / 255 + 16`); match selection affects only
/// density, so byte-identity with K4os output is not promised, but every emitted
/// block decodes back through [`decompress_block`].
pub fn compress_block(input: &[u8], maximum_output_size: usize) -> Result<Vec<u8>, Lz4BlockError> {
    let worst_case = input
        .len()
        .checked_add(input.len() / 255 + 16)
        .ok_or_else(|| Lz4BlockError::new(0, Lz4BlockErrorKind::LengthOverflow))?;
    if worst_case > maximum_output_size {
        return Err(Lz4BlockError::new(
            0,
            Lz4BlockErrorKind::OutputLimitExceeded {
                expected: worst_case,
                limit: maximum_output_size,
            },
        ));
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(worst_case)
        .map_err(|_| Lz4BlockError::new(0, Lz4BlockErrorKind::AllocationFailed(worst_case)))?;

    if input.is_empty() {
        output.push(0x00);
        return Ok(output);
    }

    let mut table = vec![usize::MAX; 1 << LZ4_HASH_BITS];
    let mut anchor = 0usize;
    let mut position = 0usize;
    while input.len() - position >= LZ4_SEARCH_LIMIT {
        let sequence = u32::from_le_bytes([
            input[position],
            input[position + 1],
            input[position + 2],
            input[position + 3],
        ]);
        let hash = (sequence.wrapping_mul(LZ4_HASH_MULTIPLIER) >> (32 - LZ4_HASH_BITS)) as usize
            & ((1 << LZ4_HASH_BITS) - 1);
        let reference = table[hash];
        table[hash] = position;
        let distance = position.wrapping_sub(reference);
        if reference != usize::MAX
            && distance <= LZ4_MAX_DISTANCE
            && input[reference..reference + LZ4_MIN_MATCH]
                == input[position..position + LZ4_MIN_MATCH]
        {
            let literal_length = position - anchor;
            let mut match_length = LZ4_MIN_MATCH;
            let extension_cap = input.len() - LZ4_LAST_LITERALS;
            while position + match_length < extension_cap
                && input[reference + match_length] == input[position + match_length]
            {
                match_length += 1;
            }
            let token =
                ((literal_length.min(15) << 4) | (match_length - LZ4_MIN_MATCH).min(15)) as u8;
            output.push(token);
            push_extension_bytes(&mut output, literal_length);
            output.extend_from_slice(&input[anchor..position]);
            output.extend_from_slice(&(distance as u16).to_le_bytes());
            push_extension_bytes(&mut output, match_length - LZ4_MIN_MATCH);
            anchor = position + match_length;
            position = anchor;
        } else {
            position += 1;
        }
    }

    let literal_length = input.len() - anchor;
    output.push((literal_length.min(15) << 4) as u8);
    push_extension_bytes(&mut output, literal_length);
    output.extend_from_slice(&input[anchor..]);
    Ok(output)
}

/// Appends the nibble-15 extension bytes for one token length.
fn push_extension_bytes(output: &mut Vec<u8>, mut length: usize) {
    if length < 15 {
        return;
    }
    length -= 15;
    while length >= 255 {
        output.push(255);
        length -= 255;
    }
    output.push(length as u8);
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
    use super::{compress_block, decompress_block, Lz4BlockErrorKind};

    fn parse_hex(text: &str) -> Vec<u8> {
        text.as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }

    /// Byte-for-byte differential against the K4os reference codec behind the
    /// baseline (`K4os.Compression.LZ4` 1.3.8, `LZ4Level.L00_FAST`; probe
    /// `target/test-tmp/lz4probe`): every compressed payload below is K4os
    /// `Encode` output, and K4os `Decode` round-trips each to its raw form.
    ///
    /// The small shapes additionally confirm encoder-shape agreement: K4os emits
    /// byte-identical blocks to `compress_block` for the short, distinct-20,
    /// repeat, and run-13 inputs (see `encoder_emits_exact_bytes_for_canonical_shapes`).
    #[test]
    fn decodes_k4os_reference_vectors() {
        let phrase = "Hello, Unity bundle world! ";
        let bytes256: Vec<u8> = (0..=255u16).map(|value| value as u8).collect();
        let mut bytes256_compressed = vec![0xf0, 0xf1];
        bytes256_compressed.extend_from_slice(&bytes256);
        let vectors: &[(Vec<u8>, Vec<u8>)] = &[
            // K4os encodes empty input to empty output.
            (Vec::new(), Vec::new()),
            (
                parse_hex("800102030405060708"),
                vec![1, 2, 3, 4, 5, 6, 7, 8],
            ),
            (
                parse_hex("f0050714212e3b4855626f7c8996a3b0bdcad7e4f1fe"),
                (0..20).map(|index| index * 13 + 7).collect(),
            ),
            (
                parse_hex("8330313233343536370800503730313233"),
                b"01234567012345670123".to_vec(),
            ),
            (parse_hex("13610100506161616161"), vec![b'a'; 13]),
            (
                parse_hex(
                    "ff0c48656c6c6f2c20556e6974792062756e646c6520776f726c6421201b00a550726c642120",
                ),
                phrase.repeat(8).into_bytes(),
            ),
            (bytes256_compressed, bytes256),
            // A 255-chained match extension with overlapping RLE output.
            (parse_hex("1fab0100ffffffea50ababababab"), vec![0xab; 1024]),
            (
                parse_hex("8361626364656667680300503132333435"),
                b"abcdefghfghfghf12345".to_vec(),
            ),
        ];
        for (index, (compressed, expected)) in vectors.iter().enumerate() {
            assert_eq!(
                &decompress_block(compressed, expected.len(), expected.len()).unwrap(),
                expected,
                "k4os vector {index}"
            );
        }
    }

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

    #[test]
    fn encoder_emits_exact_bytes_for_canonical_shapes() {
        // Empty input is the canonical single endmark token.
        assert_eq!(compress_block(&[], 64).unwrap(), vec![0x00]);
        // Fewer than 12 bytes never enter the match finder: one literal token.
        assert_eq!(
            compress_block(&[1, 2, 3, 4, 5, 6, 7, 8], 64).unwrap(),
            vec![0x80, 1, 2, 3, 4, 5, 6, 7, 8]
        );
        // Twenty distinct bytes stay one literal run with a nibble-15 extension.
        let distinct: Vec<u8> = (0..20).map(|index| index * 13 + 7).collect();
        let mut expected = vec![0xf0, 0x05];
        expected.extend_from_slice(&distinct);
        assert_eq!(compress_block(&distinct, 64).unwrap(), expected);
        // A repeat is a literal run, a short match, and a literal-only tail:
        // the match stops at `len - 5` even though the run continues.
        let mut repeated = b"01234567".to_vec();
        repeated.extend_from_slice(b"012345670123");
        assert_eq!(
            compress_block(&repeated, 64).unwrap(),
            [
                0x83, b'0', b'1', b'2', b'3', b'4', b'5', b'6', b'7', 0x08, 0x00, 0x50, b'7', b'0',
                b'1', b'2', b'3',
            ]
        );
        // Thirteen identical bytes match at distance one with overlap.
        assert_eq!(
            compress_block(&[b'a'; 13], 64).unwrap(),
            [0x13, b'a', 0x01, 0x00, 0x50, b'a', b'a', b'a', b'a', b'a']
        );
    }

    #[test]
    fn encoder_round_trips_through_the_decoder() {
        let mut inputs: Vec<Vec<u8>> = vec![
            vec![],
            vec![0x00],
            b"Hello, Unity bundle world! ".repeat(8),
            (0..256).map(|index| index as u8).collect(),
            vec![0xab; 1024],
        ];
        // Deterministic pseudo-random bytes with scattered repeats.
        let mut random = vec![0u8; 4096];
        let mut state = 0x1234_5678u32;
        for byte in random.iter_mut() {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            *byte = (state >> 16) as u8;
        }
        let seed = random[8..16].to_vec();
        random[1000..1008].copy_from_slice(&seed);
        let early = random[2000..2020].to_vec();
        random[3000..3020].copy_from_slice(&early);
        inputs.push(random);
        // Repeats beyond the 16-bit offset range stay literals but stay valid.
        let mut far = vec![0x55u8; 70_000];
        let head = far[..64].to_vec();
        far.extend_from_slice(&head);
        inputs.push(far);
        for input in &inputs {
            let compressed = compress_block(input, input.len() + input.len() / 255 + 16).unwrap();
            assert_eq!(
                decompress_block(&compressed, input.len(), input.len()).unwrap(),
                *input
            );
        }
    }

    #[test]
    fn encoder_enforces_the_output_limit() {
        let error = compress_block(&[1, 2, 3], 0).unwrap_err();
        assert!(matches!(
            error.kind(),
            Lz4BlockErrorKind::OutputLimitExceeded { .. }
        ));
        let input = b"compressible compressible compressible input bytes";
        let worst_case = input.len() + input.len() / 255 + 16;
        compress_block(input, worst_case).unwrap();
    }
}
