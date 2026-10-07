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
    let mut reader = BrotliBitReader::new(input);
    if read_brotli_window_bits(&mut reader).is_none() {
        return false;
    }

    // AssetRipper's marker is an empty, non-final Brotli metadata meta-block.
    let Some(is_last) = reader.read_bits(1) else {
        return false;
    };
    let Some(nibble_code) = reader.read_bits(2) else {
        return false;
    };
    if is_last != 0 || decode_metadata_nibble_count(nibble_code) != Some(0) {
        return false;
    }
    let Some(reserved) = reader.read_bits(1) else {
        return false;
    };
    let Some(skip_bytes) = reader.read_bits(2) else {
        return false;
    };
    if reserved != 0 || skip_bytes == 0 {
        return false;
    }

    // RFC 7932 encodes MSKIPLEN - 1 in the selected number of bytes.
    let Some(length_minus_one) = reader.read_bits((skip_bytes * 8) as usize) else {
        return false;
    };
    let Some(length) = usize::try_from(length_minus_one)
        .ok()
        .and_then(|value| value.checked_add(1))
    else {
        return false;
    };
    if length != BROTLI_FILE_SIGNATURE.len() || !reader.align_to_byte() {
        return false;
    }
    reader.read_aligned_bytes(length) == Some(BROTLI_FILE_SIGNATURE)
}

fn decode_metadata_nibble_count(code: u32) -> Option<u32> {
    match code {
        3 => Some(0),
        0 => Some(4),
        1 => Some(5),
        2 => Some(6),
        _ => None,
    }
}

fn read_brotli_window_bits(reader: &mut BrotliBitReader<'_>) -> Option<u8> {
    let first_bit = reader.read_bits(1)?;
    if first_bit == 0 {
        return Some(16);
    }

    let code4 = 1 | (reader.read_bits(3)? << 1);
    match code4 {
        3 => Some(18),
        5 => Some(19),
        7 => Some(20),
        9 => Some(21),
        11 => Some(22),
        13 => Some(23),
        15 => Some(24),
        1 => {
            let code7 = code4 | (reader.read_bits(3)? << 4);
            match code7 {
                1 => Some(17),
                33 => Some(10),
                49 => Some(11),
                65 => Some(12),
                81 => Some(13),
                97 => Some(14),
                113 => Some(15),
                _ => None,
            }
        }
        _ => None,
    }
}

struct BrotliBitReader<'a> {
    input: &'a [u8],
    bit_position: usize,
}

impl<'a> BrotliBitReader<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self {
            input,
            bit_position: 0,
        }
    }

    fn read_bits(&mut self, count: usize) -> Option<u32> {
        if count > u32::BITS as usize {
            return None;
        }
        let end = self.bit_position.checked_add(count)?;
        if end > self.input.len().checked_mul(8)? {
            return None;
        }
        let mut value = 0u32;
        for index in 0..count {
            let byte = *self.input.get(self.bit_position / 8)?;
            let bit = (byte >> (self.bit_position % 8)) & 1;
            value |= u32::from(bit) << index;
            self.bit_position += 1;
        }
        Some(value)
    }

    fn align_to_byte(&mut self) -> bool {
        while self.bit_position & 7 != 0 {
            if self.read_bits(1) != Some(0) {
                return false;
            }
        }
        true
    }

    fn read_aligned_bytes(&mut self, length: usize) -> Option<&'a [u8]> {
        if self.bit_position & 7 != 0 {
            return None;
        }
        let start = self.bit_position / 8;
        let end = start.checked_add(length)?;
        let bytes = self.input.get(start..end)?;
        self.bit_position = end.checked_mul(8)?;
        Some(bytes)
    }
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
    fn rejects_the_source_detector_nonstandard_marker_length_prefix() {
        let mut input = vec![0xce, ((BROTLI_FILE_SIGNATURE.len() as u8) << 2) | 1, 0];
        input.extend_from_slice(BROTLI_FILE_SIGNATURE);
        input.extend_from_slice(&[0; 4]);
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

    #[test]
    fn dotnet_encoded_members_decode_to_the_same_bytes() {
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

        // .NET-encoded vectors (probe Temp/opencode/gzipprobe, GZipStream
        // Optimal): nothing here was produced by the Rust writer, so decode
        // parity is independently proven. The Rust encoder is intentionally
        // not byte-identical to .NET (different settings); both outputs are
        // valid GZip members and the Rust writer is covered by round-trip
        // tests elsewhere.
        let fixture = include_str!("gzip_dotnet_fixture.txt");
        let mut cases: std::collections::HashMap<String, (Vec<u8>, Vec<u8>)> =
            std::collections::HashMap::new();
        for line in fixture.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            if line.starts_with("GZIP|") {
                let parts: Vec<&str> = line.split('|').collect();
                let name = parts[1].to_string();
                if name == "empty" {
                    // Degenerate: .NET emits zero bytes (no 1F 8B member);
                    // both sides agree there is nothing to inflate.
                    continue;
                }
                if name == "concat" {
                    let comp = parts
                        .iter()
                        .find_map(|p| p.strip_prefix("comp="))
                        .unwrap_or_else(|| panic!("missing comp in {line}"));
                    let back = parts
                        .iter()
                        .find_map(|p| p.strip_prefix("back="))
                        .unwrap_or_else(|| panic!("missing back in {line}"));
                    cases.insert(name, (hex_to_bytes(comp), hex_to_bytes(back)));
                } else {
                    let plain = parts
                        .iter()
                        .find_map(|p| p.strip_prefix("plain="))
                        .unwrap_or_else(|| panic!("missing plain in {line}"));
                    let comp = parts
                        .iter()
                        .find_map(|p| p.strip_prefix("comp="))
                        .unwrap_or_else(|| panic!("missing comp in {line}"));
                    let back = parts
                        .iter()
                        .find_map(|p| p.strip_prefix("back="))
                        .unwrap_or_else(|| panic!("missing back in {line}"));
                    let plain_bytes = hex_to_bytes(plain);
                    let back_bytes = hex_to_bytes(back);
                    assert_eq!(
                        plain_bytes, back_bytes,
                        "{name} .NET round-trip is self-consistent"
                    );
                    cases.insert(name, (hex_to_bytes(comp), plain_bytes));
                }
            }
        }

        assert!(cases.len() >= 3, "expected gzip vectors");
        for (name, (compressed, expected)) in &cases {
            assert!(has_gzip_magic(compressed), "{name} has 1F 8B magic");
            assert_eq!(
                &decompress_gzip(compressed, expected.len() + 8).unwrap(),
                expected,
                "{name} Rust decodes .NET bytes"
            );
        }
        // Concatenated members decode as one logical stream on both sides.
        let (concat_comp, concat_plain) = cases.get("concat").expect("concat vector");
        assert_eq!(concat_plain, b"first second");
        assert_eq!(&decompress_gzip(concat_comp, 64).unwrap(), concat_plain);
    }

    #[test]
    fn dotnet_encoded_brotli_streams_decode_to_the_same_bytes() {
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

        // Probe Temp/opencode/compencodeprobe (.NET BrotliStream Optimal,
        // SDK 10.0.401): B1 compressible text, B2 0..255 binary, B3 empty.
        // These replace the hand-only Brotli evidence with independently
        // encoded streams (the reference decodes with the same .NET library,
        // so the oracle is the independent encoder, not a second decoder).
        let fixture = include_str!("reference_encoded_fixture.txt");
        let mut raws: std::collections::HashMap<String, Vec<u8>> = std::collections::HashMap::new();
        let mut encs: std::collections::HashMap<String, Vec<u8>> = std::collections::HashMap::new();
        for line in fixture.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            if line.starts_with("COMP_RAW|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad COMP_RAW {line}");
                raws.insert(parts[1].to_string(), hex_to_bytes(parts[2]));
            } else if line.starts_with("COMP_ENC|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad COMP_ENC {line}");
                encs.insert(parts[1].to_string(), hex_to_bytes(parts[2]));
            } else if line.starts_with("COMP_REF|") {
                assert!(line.ends_with("|OK"), "reference round-trip {line}");
            }
        }

        for case in ["B1", "B2", "B3"] {
            let raw = raws.get(case).unwrap_or_else(|| panic!("missing {case}"));
            let enc = encs.get(case).unwrap_or_else(|| panic!("missing {case}"));
            let decoded = decompress_brotli(enc, raw.len().max(1) * 2 + 16)
                .unwrap_or_else(|e| panic!("{case} Rust decode failed: {e:?}"));
            assert_eq!(&decoded, raw, "{case} bytes");
        }
    }
}
