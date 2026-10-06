//! Bounded Zstandard frame decoding for Unity storage blocks.
//!
//! Mirrors `AssetRipper.IO.Files/BundleFiles/ZstdCompression.cs`: the four-byte frame
//! signature check and bounded decompression of exactly the declared sizes. One deliberate
//! hardening: inputs shorter than the signature read `false` instead of throwing like the
//! source's exact stream read.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

pub const ZSTD_FRAME_MAGIC: [u8; 4] = [0x28, 0xb5, 0x2f, 0xfd];

/// Decode one or more Zstandard frames into exactly the declared output size.
pub fn decompress_block(
    input: &[u8],
    expected_output_size: usize,
    maximum_output_size: usize,
    maximum_window_size: usize,
) -> Result<Vec<u8>, ZstdBlockError> {
    if expected_output_size > maximum_output_size {
        return Err(ZstdBlockError::new(
            ZstdBlockErrorKind::OutputLimitExceeded {
                requested: expected_output_size,
                limit: maximum_output_size,
            },
        ));
    }
    let maximum_window_size_u64 = u64::try_from(maximum_window_size).map_err(|_| {
        ZstdBlockError::new(ZstdBlockErrorKind::WindowLimitUnrepresentable {
            limit: maximum_window_size,
        })
    })?;

    let mut output = Vec::new();
    output
        .try_reserve_exact(expected_output_size)
        .map_err(|_| {
            ZstdBlockError::new(ZstdBlockErrorKind::AllocationFailed {
                requested: expected_output_size,
            })
        })?;
    output.resize(expected_output_size, 0);

    let mut decoder = ruzstd::decoding::FrameDecoder::new();
    decoder.set_max_window_size(maximum_window_size_u64);
    let bytes_written = decoder
        .decode_all(input, &mut output)
        .map_err(|error| ZstdBlockError::new(ZstdBlockErrorKind::Decoder(error.to_string())))?;

    if bytes_written != expected_output_size {
        return Err(ZstdBlockError::new(
            ZstdBlockErrorKind::OutputSizeMismatch {
                expected: expected_output_size,
                actual: bytes_written,
            },
        ));
    }
    Ok(output)
}

pub fn has_frame_magic(input: &[u8]) -> bool {
    input.starts_with(&ZSTD_FRAME_MAGIC)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ZstdBlockError {
    kind: ZstdBlockErrorKind,
}

impl ZstdBlockError {
    fn new(kind: ZstdBlockErrorKind) -> Self {
        Self { kind }
    }
}

impl Display for ZstdBlockError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.kind)
    }
}

impl Error for ZstdBlockError {}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ZstdBlockErrorKind {
    OutputLimitExceeded { requested: usize, limit: usize },
    WindowLimitUnrepresentable { limit: usize },
    AllocationFailed { requested: usize },
    OutputSizeMismatch { expected: usize, actual: usize },
    Decoder(String),
}

impl Display for ZstdBlockErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutputLimitExceeded { requested, limit } => write!(
                formatter,
                "decoded size {requested} exceeds configured limit {limit}"
            ),
            Self::WindowLimitUnrepresentable { limit } => {
                write!(formatter, "window limit {limit} cannot be represented")
            }
            Self::AllocationFailed { requested } => {
                write!(formatter, "could not allocate {requested} decoded bytes")
            }
            Self::OutputSizeMismatch { expected, actual } => write!(
                formatter,
                "decoded output has {actual} bytes, expected {expected}"
            ),
            Self::Decoder(detail) => write!(formatter, "Zstandard decoder: {detail}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{decompress_block, has_frame_magic, ZstdBlockErrorKind, ZSTD_FRAME_MAGIC};

    fn raw_block_frame(bytes: &[u8]) -> Vec<u8> {
        assert!(bytes.len() <= 255);
        let mut frame = ZSTD_FRAME_MAGIC.to_vec();
        frame.push(0x20); // Single segment, one-byte content size, no dict/checksum.
        frame.push(bytes.len() as u8);
        let block_header = ((bytes.len() as u32) << 3) | 1; // Last, raw block.
        frame.extend_from_slice(&block_header.to_le_bytes()[..3]);
        frame.extend_from_slice(bytes);
        frame
    }

    #[test]
    fn decodes_spec_constructed_raw_block_frame() {
        let input = raw_block_frame(b"independent zstd frame fixture");
        assert!(has_frame_magic(&input));
        assert_eq!(
            decompress_block(&input, 30, 30, 1024).unwrap(),
            b"independent zstd frame fixture"
        );
    }

    fn rle_block_frame(byte: u8, repeated_size: u8) -> Vec<u8> {
        let mut frame = ZSTD_FRAME_MAGIC.to_vec();
        frame.push(0x20); // Single segment, one-byte content size, no dict/checksum.
        frame.push(repeated_size);
        let block_header = (u32::from(repeated_size) << 3) | 3; // Last, RLE block.
        frame.extend_from_slice(&block_header.to_le_bytes()[..3]);
        frame.push(byte);
        frame
    }

    #[test]
    fn decodes_spec_constructed_rle_block_frame() {
        let input = rle_block_frame(b'R', 64);
        assert_eq!(
            decompress_block(&input, 64, 64, 1024).unwrap(),
            vec![b'R'; 64]
        );
    }

    #[test]
    fn handles_concatenated_frames_and_skippable_frames() {
        let mut input = raw_block_frame(b"first");
        input.extend_from_slice(&[0x50, 0x2a, 0x4d, 0x18]);
        input.extend_from_slice(&3u32.to_le_bytes());
        input.extend_from_slice(b"tag");
        input.extend_from_slice(&raw_block_frame(b"second"));

        assert_eq!(
            decompress_block(&input, 11, 11, 1024).unwrap(),
            b"firstsecond"
        );
    }

    #[test]
    fn enforces_output_and_window_limits() {
        let frame = raw_block_frame(b"output");
        let output_limit = decompress_block(&frame, 7, 6, 1024).unwrap_err();
        assert_eq!(
            output_limit.kind,
            ZstdBlockErrorKind::OutputLimitExceeded {
                requested: 7,
                limit: 6
            }
        );

        let windowed = [
            ZSTD_FRAME_MAGIC.as_slice(),
            &[0, 0x20],
            &[0x19, 0, 0],
            b"abc",
        ]
        .concat();
        let window_limit = decompress_block(&windowed, 3, 3, 1024).unwrap_err();
        let ZstdBlockErrorKind::Decoder(detail) = window_limit.kind else {
            panic!("expected the decoder to reject an oversized window")
        };
        assert!(detail.contains("window_size is too big"), "{detail}");
    }

    #[test]
    fn reports_short_decoded_output_and_malformed_frames() {
        let frame = raw_block_frame(b"abc");
        let mismatch = decompress_block(&frame, 4, 4, 1024).unwrap_err();
        assert_eq!(
            mismatch.kind,
            ZstdBlockErrorKind::OutputSizeMismatch {
                expected: 4,
                actual: 3
            }
        );

        let malformed = decompress_block(&ZSTD_FRAME_MAGIC, 3, 3, 1024).unwrap_err();
        assert!(matches!(malformed.kind, ZstdBlockErrorKind::Decoder(_)));
    }

    #[test]
    fn guards_short_signatures_and_unreservable_outputs() {
        // Inputs shorter than the signature read false instead of throwing
        // like the source's exact stream read.
        assert!(!has_frame_magic(&[]));
        assert!(!has_frame_magic(&ZSTD_FRAME_MAGIC[..3]));
        assert!(has_frame_magic(&ZSTD_FRAME_MAGIC));
        let mut prefixed = ZSTD_FRAME_MAGIC.to_vec();
        prefixed.extend_from_slice(b"trailing");
        assert!(has_frame_magic(&prefixed));
        // An output no allocator could reserve fails before decoding.
        let allocation = decompress_block(&[], usize::MAX, usize::MAX, 1024).unwrap_err();
        assert_eq!(
            allocation.kind,
            ZstdBlockErrorKind::AllocationFailed {
                requested: usize::MAX
            }
        );
        assert!(allocation.to_string().contains("could not allocate"));
    }
}
