//! Bounded decoders for AssetRipper's compressed-file wrappers.

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io::{self, Read, Write};

const BROTLI_FILE_SIGNATURE: &[u8] = b"UnityWeb Compressed Content (brotli)";

pub fn has_gzip_magic(input: &[u8]) -> bool {
    input.starts_with(&[0x1f, 0x8b])
}

/// Match the metadata-block marker used by AssetRipper's Brotli file scheme.
///
/// This is not equivalent to checking for an arbitrary Brotli stream: the source
/// scheme reads the first metadata-block length and compares its payload with a
/// fixed UnityWeb marker before invoking the Brotli decoder.
pub fn has_brotli_file_signature(input: &[u8]) -> bool {
    if input.len() < 4 {
        return false;
    }

    // The first byte contains the Brotli window header. The source detector
    // advances past it and reads the following metadata-block length fields.
    let mut position = 1usize;
    let mut current = input[position];
    position += 1;
    let size_bytes = usize::from(current & 0x03);
    if position
        .checked_add(size_bytes)
        .is_none_or(|end| end > input.len())
    {
        return false;
    }

    let mut length = 0usize;
    for index in 0..size_bytes {
        let next = input[position];
        position += 1;
        let bits = usize::from((current >> 2) | ((next & 0x03) << 6));
        let Some(shift) = index.checked_mul(8) else {
            return false;
        };
        let Some(part) = bits.checked_shl(shift as u32) else {
            return false;
        };
        let Some(combined) = length.checked_add(part) else {
            return false;
        };
        length = combined;
        current = next;
    }

    if length != BROTLI_FILE_SIGNATURE.len() {
        return false;
    }
    let Some(end) = position.checked_add(length) else {
        return false;
    };
    input.get(position..end) == Some(BROTLI_FILE_SIGNATURE)
}

pub fn decompress_gzip(
    input: &[u8],
    maximum_output_size: usize,
) -> Result<Vec<u8>, WrapperCodecError> {
    let decoder = flate2::read::MultiGzDecoder::new(input);
    decompress_limited(decoder, maximum_output_size, "GZip")
}

pub fn decompress_brotli(
    input: &[u8],
    maximum_output_size: usize,
) -> Result<Vec<u8>, WrapperCodecError> {
    let decoder = brotli_decompressor::Decompressor::new(input, 4096);
    decompress_limited(decoder, maximum_output_size, "Brotli")
}

fn decompress_limited<R: Read>(
    mut decoder: R,
    maximum_output_size: usize,
    codec: &'static str,
) -> Result<Vec<u8>, WrapperCodecError> {
    let mut output = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let remaining = maximum_output_size - output.len();
        let read_limit = remaining.saturating_add(1).min(buffer.len());
        let bytes_read = decoder.read(&mut buffer[..read_limit]).map_err(|error| {
            WrapperCodecError::new(WrapperCodecErrorKind::Decoder {
                codec,
                detail: error.to_string(),
            })
        })?;
        if bytes_read == 0 {
            return Ok(output);
        }
        if bytes_read > remaining {
            return Err(WrapperCodecError::new(
                WrapperCodecErrorKind::OutputLimitExceeded {
                    codec,
                    limit: maximum_output_size,
                },
            ));
        }

        let requested = output.len().checked_add(bytes_read).ok_or_else(|| {
            WrapperCodecError::new(WrapperCodecErrorKind::OutputLengthOverflow { codec })
        })?;
        output.try_reserve(bytes_read).map_err(|_| {
            WrapperCodecError::new(WrapperCodecErrorKind::AllocationFailed { codec, requested })
        })?;
        output.extend_from_slice(&buffer[..bytes_read]);
    }
}

/// Encode GZip data while bounding the compressed output written to memory.
pub fn compress_gzip(
    input: &[u8],
    maximum_compressed_size: usize,
) -> Result<Vec<u8>, WrapperCodecError> {
    let writer = BoundedVecWriter {
        bytes: Vec::new(),
        maximum: maximum_compressed_size,
        exceeded_limit: false,
        allocation_failed: None,
        length_overflow: false,
    };
    let mut encoder = flate2::write::GzEncoder::new(writer, flate2::Compression::default());
    if let Err(error) = encoder.write_all(input) {
        return Err(encode_error(encoder.get_ref(), error));
    }
    if let Err(error) = encoder.try_finish() {
        return Err(encode_error(encoder.get_ref(), error));
    }
    let writer = encoder.finish().map_err(|error| {
        WrapperCodecError::new(WrapperCodecErrorKind::Encoder {
            codec: "GZip",
            detail: error.to_string(),
        })
    })?;
    Ok(writer.bytes)
}

fn encode_error(writer: &BoundedVecWriter, error: io::Error) -> WrapperCodecError {
    let kind = if writer.exceeded_limit {
        WrapperCodecErrorKind::OutputLimitExceeded {
            codec: "GZip encoder",
            limit: writer.maximum,
        }
    } else if let Some(requested) = writer.allocation_failed {
        WrapperCodecErrorKind::AllocationFailed {
            codec: "GZip encoder",
            requested,
        }
    } else if writer.length_overflow {
        WrapperCodecErrorKind::OutputLengthOverflow {
            codec: "GZip encoder",
        }
    } else {
        WrapperCodecErrorKind::Encoder {
            codec: "GZip",
            detail: error.to_string(),
        }
    };
    WrapperCodecError::new(kind)
}

struct BoundedVecWriter {
    bytes: Vec<u8>,
    maximum: usize,
    exceeded_limit: bool,
    allocation_failed: Option<usize>,
    length_overflow: bool,
}

impl Write for BoundedVecWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let requested = self.bytes.len().checked_add(bytes.len()).ok_or_else(|| {
            self.length_overflow = true;
            io::Error::new(io::ErrorKind::InvalidData, "GZip output length overflow")
        })?;
        if requested > self.maximum {
            self.exceeded_limit = true;
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "GZip output exceeds configured limit",
            ));
        }
        self.bytes.try_reserve(bytes.len()).map_err(|_| {
            self.allocation_failed = Some(requested);
            io::Error::new(io::ErrorKind::OutOfMemory, "could not allocate GZip output")
        })?;
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WrapperCodecError {
    kind: WrapperCodecErrorKind,
}

impl WrapperCodecError {
    fn new(kind: WrapperCodecErrorKind) -> Self {
        Self { kind }
    }

    pub fn kind(&self) -> &WrapperCodecErrorKind {
        &self.kind
    }
}

impl Display for WrapperCodecError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.kind)
    }
}

impl Error for WrapperCodecError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WrapperCodecErrorKind {
    OutputLimitExceeded {
        codec: &'static str,
        limit: usize,
    },
    OutputLengthOverflow {
        codec: &'static str,
    },
    AllocationFailed {
        codec: &'static str,
        requested: usize,
    },
    Decoder {
        codec: &'static str,
        detail: String,
    },
    Encoder {
        codec: &'static str,
        detail: String,
    },
}

impl Display for WrapperCodecErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutputLimitExceeded { codec, limit } => {
                write!(formatter, "{codec} output exceeds configured limit {limit}")
            }
            Self::OutputLengthOverflow { codec } => {
                write!(formatter, "{codec} decoded output length overflows")
            }
            Self::AllocationFailed { codec, requested } => {
                write!(
                    formatter,
                    "could not allocate {requested} decoded {codec} bytes"
                )
            }
            Self::Decoder { codec, detail } => write!(formatter, "{codec} decoder: {detail}"),
            Self::Encoder { codec, detail } => write!(formatter, "{codec} encoder: {detail}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        compress_gzip, decompress_brotli, decompress_gzip, has_brotli_file_signature,
        has_gzip_magic, WrapperCodecErrorKind, BROTLI_FILE_SIGNATURE,
    };
    fn stored_gzip_member(payload: &[u8]) -> Vec<u8> {
        assert!(payload.len() <= u16::MAX as usize);
        let length = payload.len() as u16;
        let mut bytes = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 255];
        bytes.push(1); // Final DEFLATE block, stored payload.
        bytes.extend_from_slice(&length.to_le_bytes());
        bytes.extend_from_slice(&(!length).to_le_bytes());
        bytes.extend_from_slice(payload);
        bytes.extend_from_slice(&crc32(payload).to_le_bytes());
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes
    }

    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = u32::MAX;
        for byte in bytes {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                let mask = 0u32.wrapping_sub(crc & 1);
                crc = (crc >> 1) ^ (0xedb8_8320 & mask);
            }
        }
        !crc
    }

    #[test]
    fn detects_gzip_magic_and_decodes_concatenated_members() {
        let first = stored_gzip_member(b"first ");
        let second = stored_gzip_member(b"second");
        let mut input = first;
        input.extend_from_slice(&second);

        assert!(has_gzip_magic(&input));
        assert_eq!(decompress_gzip(&input, 64).unwrap(), b"first second");
    }

    #[test]
    fn writes_gzip_members_with_a_compressed_output_limit() {
        let payload = b"source GZipFile supports writing as well as reading";
        let encoded = compress_gzip(payload, 1024).unwrap();
        assert!(has_gzip_magic(&encoded));
        assert_eq!(decompress_gzip(&encoded, payload.len()).unwrap(), payload);

        let limit = compress_gzip(payload, 4).unwrap_err();
        assert_eq!(
            limit.kind(),
            &WrapperCodecErrorKind::OutputLimitExceeded {
                codec: "GZip encoder",
                limit: 4,
            }
        );
    }

    #[test]
    fn validates_gzip_checksum_and_output_limit() {
        let mut malformed = stored_gzip_member(b"crc");
        let checksum_index = malformed.len() - 8;
        malformed[checksum_index] ^= 1;
        assert!(decompress_gzip(&malformed, 64).is_err());

        let oversized = decompress_gzip(&stored_gzip_member(b"too long"), 4).unwrap_err();
        assert_eq!(
            oversized.kind(),
            &WrapperCodecErrorKind::OutputLimitExceeded {
                codec: "GZip",
                limit: 4,
            }
        );
    }

    #[test]
    fn detects_the_unity_brotli_metadata_marker() {
        let mut input = vec![0xce, ((BROTLI_FILE_SIGNATURE.len() as u8) << 2) | 1, 0];
        input.extend_from_slice(BROTLI_FILE_SIGNATURE);
        input.extend_from_slice(&[0; 4]);
        assert!(has_brotli_file_signature(&input));

        input[3] ^= 1;
        assert!(!has_brotli_file_signature(&input));
        assert!(!has_brotli_file_signature(&input[..3]));
    }

    #[test]
    fn decodes_brotli_test_vector_with_output_limit() {
        // This published brotli-decompressor vector encodes ten X bytes followed by ten Y bytes.
        let encoded = [
            0x1b, 0x13, 0x00, 0x00, 0xa4, 0xb0, 0xb2, 0xea, 0x81, 0x47, 0x02, 0x8a,
        ];
        let expected = [b'X'; 10].into_iter().chain([b'Y'; 10]).collect::<Vec<_>>();
        assert_eq!(decompress_brotli(&encoded, 20).unwrap(), expected);

        let limited = decompress_brotli(&encoded, 19).unwrap_err();
        assert_eq!(
            limited.kind(),
            &WrapperCodecErrorKind::OutputLimitExceeded {
                codec: "Brotli",
                limit: 19,
            }
        );

        let malformed = decompress_brotli(&[0xff, 0xff], 32).unwrap_err();
        assert!(matches!(
            malformed.kind(),
            WrapperCodecErrorKind::Decoder {
                codec: "Brotli",
                ..
            }
        ));
    }

    #[test]
    fn rejects_empty_gzip_without_panicking() {
        assert!(!has_gzip_magic(&[]));
        assert!(decompress_gzip(&[], 16).is_err());
    }
}
