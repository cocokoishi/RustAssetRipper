//! Bounded decoder for Unity's LZMA block payloads.
//!
//! One deliberate difference is documented, not changed: the source read loop stops
//! at the requested count and leaves a short stream for downstream size checks to
//! reject, while this decoder fails closed on a short stream and truncates a long
//! stream to the requested count (pinned by the output-size contract test below).
//!
//! Independent-encoder coverage comes from SharpCompress 0.50.4 vectors (the
//! baseline's own LZMA library): `decodes_sharpcompress_encoded_unity_payloads`
//! and `decodes_sharpcompress_encoded_size_prefixed_payloads` decode payloads no
//! `lzma-rs` code has touched.

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io::{self, Cursor, Write};

/// Decode a Unity LZMA payload: five LZMA properties bytes followed by a raw stream.
///
/// The expected decoded size is supplied by Unity's bundle metadata. Both the output
/// allocation and the decoder's history buffer are bounded by `maximum_output_size`.
pub fn decompress_block(
    input: &[u8],
    expected_output_size: usize,
    maximum_output_size: usize,
) -> Result<Vec<u8>, LzmaBlockError> {
    if input.len() < 5 {
        return Err(LzmaBlockError::new(
            input.len(),
            LzmaBlockErrorKind::HeaderTooShort {
                actual: input.len(),
            },
        ));
    }
    decompress_raw_block(
        &input[..5],
        &input[5..],
        5,
        expected_output_size,
        maximum_output_size,
    )
}

/// Decode Unity's size-prefixed LZMA stream: five property bytes, an eight-byte
/// little-endian decoded length, and the raw LZMA payload.
pub fn decompress_size_block(
    input: &[u8],
    expected_output_size: usize,
    maximum_output_size: usize,
) -> Result<Vec<u8>, LzmaBlockError> {
    const SIZE_HEADER_LENGTH: usize = 13;
    if input.len() < SIZE_HEADER_LENGTH {
        return Err(LzmaBlockError::new(
            input.len(),
            LzmaBlockErrorKind::SizeHeaderTooShort {
                actual: input.len(),
            },
        ));
    }
    let declared_size = u64::from_le_bytes(input[5..SIZE_HEADER_LENGTH].try_into().unwrap());
    if declared_size != expected_output_size as u64 {
        return Err(LzmaBlockError::new(
            5,
            LzmaBlockErrorKind::DeclaredSizeMismatch {
                declared: declared_size,
                expected: expected_output_size,
            },
        ));
    }
    decompress_raw_block(
        &input[..5],
        &input[SIZE_HEADER_LENGTH..],
        SIZE_HEADER_LENGTH,
        expected_output_size,
        maximum_output_size,
    )
}

fn decompress_raw_block(
    properties: &[u8],
    compressed_payload: &[u8],
    payload_offset: usize,
    expected_output_size: usize,
    maximum_output_size: usize,
) -> Result<Vec<u8>, LzmaBlockError> {
    if expected_output_size > maximum_output_size {
        return Err(LzmaBlockError::new(
            0,
            LzmaBlockErrorKind::OutputLimitExceeded {
                requested: expected_output_size,
                limit: maximum_output_size,
            },
        ));
    }
    let property_byte = properties[0];
    if property_byte >= 9 * 5 * 5 {
        return Err(LzmaBlockError::new(
            0,
            LzmaBlockErrorKind::InvalidProperties {
                value: property_byte,
            },
        ));
    }
    let lc = u32::from(property_byte % 9);
    let property_tail = u32::from(property_byte / 9);
    let lp = property_tail % 5;
    let pb = property_tail / 5;

    let declared_dictionary_size =
        u32::from_le_bytes([properties[1], properties[2], properties[3], properties[4]]).max(4096);
    // A valid match cannot refer farther back than the amount of output we allow.
    // Clamping also prevents malicious dictionary declarations from overflowing
    // the decoder's circular-buffer index arithmetic on 32-bit targets.
    let index_limit = usize::MAX / 2;
    let effective_dictionary_size = (declared_dictionary_size as usize)
        .min(maximum_output_size.max(1))
        .min(index_limit.max(1));
    let effective_dictionary_size = u32::try_from(effective_dictionary_size).map_err(|_| {
        LzmaBlockError::new(
            1,
            LzmaBlockErrorKind::DictionarySizeUnrepresentable {
                size: effective_dictionary_size,
            },
        )
    })?;

    let mut output = Vec::new();
    output
        .try_reserve_exact(expected_output_size)
        .map_err(|_| {
            LzmaBlockError::new(
                0,
                LzmaBlockErrorKind::AllocationFailed {
                    requested: expected_output_size,
                },
            )
        })?;

    let params = lzma_rs::decompress::raw::LzmaParams::new(
        lzma_rs::decompress::raw::LzmaProperties { lc, lp, pb },
        effective_dictionary_size,
        Some(expected_output_size as u64),
    );
    let mut decoder = lzma_rs::decompress::raw::LzmaDecoder::new(
        params,
        Some(maximum_output_size.min(index_limit)),
    )
    .map_err(|error| LzmaBlockError::new(0, LzmaBlockErrorKind::Decoder(error.to_string())))?;

    let mut writer = BoundedOutput {
        bytes: output,
        maximum: expected_output_size,
        exceeded: false,
    };
    let mut reader = Cursor::new(compressed_payload);
    if let Err(error) = decoder.decompress(&mut reader, &mut writer) {
        if writer.exceeded {
            return Err(LzmaBlockError::new(
                5 + reader.position() as usize,
                LzmaBlockErrorKind::OutputExceeded {
                    expected: expected_output_size,
                },
            ));
        }
        return Err(LzmaBlockError::new(
            payload_offset + reader.position() as usize,
            LzmaBlockErrorKind::Decoder(error.to_string()),
        ));
    }

    if writer.bytes.len() != expected_output_size {
        return Err(LzmaBlockError::new(
            payload_offset + reader.position() as usize,
            LzmaBlockErrorKind::OutputSizeMismatch {
                expected: expected_output_size,
                actual: writer.bytes.len(),
            },
        ));
    }
    Ok(writer.bytes)
}

struct BoundedOutput {
    bytes: Vec<u8>,
    maximum: usize,
    exceeded: bool,
}

impl Write for BoundedOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let end = self.bytes.len().checked_add(bytes.len()).ok_or_else(|| {
            self.exceeded = true;
            io::Error::new(io::ErrorKind::InvalidData, "decoded output length overflow")
        })?;
        if end > self.maximum {
            self.exceeded = true;
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "decoded output exceeds expected size",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LzmaBlockError {
    input_offset: usize,
    kind: LzmaBlockErrorKind,
}

impl LzmaBlockError {
    fn new(input_offset: usize, kind: LzmaBlockErrorKind) -> Self {
        Self { input_offset, kind }
    }

    pub fn input_offset(&self) -> usize {
        self.input_offset
    }
}

impl Display for LzmaBlockError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} at payload byte {}",
            self.kind, self.input_offset
        )
    }
}

impl Error for LzmaBlockError {}

#[derive(Clone, Debug, Eq, PartialEq)]
enum LzmaBlockErrorKind {
    HeaderTooShort { actual: usize },
    SizeHeaderTooShort { actual: usize },
    DeclaredSizeMismatch { declared: u64, expected: usize },
    InvalidProperties { value: u8 },
    DictionarySizeUnrepresentable { size: usize },
    OutputLimitExceeded { requested: usize, limit: usize },
    AllocationFailed { requested: usize },
    OutputExceeded { expected: usize },
    OutputSizeMismatch { expected: usize, actual: usize },
    Decoder(String),
}

impl Display for LzmaBlockErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::HeaderTooShort { actual } => {
                write!(
                    formatter,
                    "LZMA properties header has {actual} bytes, expected 5"
                )
            }
            Self::SizeHeaderTooShort { actual } => write!(
                formatter,
                "size-prefixed LZMA header has {actual} bytes, expected at least 13"
            ),
            Self::DeclaredSizeMismatch { declared, expected } => write!(
                formatter,
                "size-prefixed LZMA stream declares {declared} decoded bytes, expected {expected}"
            ),
            Self::InvalidProperties { value } => {
                write!(formatter, "invalid LZMA properties byte {value}")
            }
            Self::DictionarySizeUnrepresentable { size } => {
                write!(
                    formatter,
                    "LZMA dictionary size {size} cannot be represented"
                )
            }
            Self::OutputLimitExceeded { requested, limit } => write!(
                formatter,
                "decoded size {requested} exceeds configured limit {limit}"
            ),
            Self::AllocationFailed { requested } => {
                write!(formatter, "could not allocate {requested} decoded bytes")
            }
            Self::OutputExceeded { expected } => {
                write!(formatter, "decoded output exceeds declared size {expected}")
            }
            Self::OutputSizeMismatch { expected, actual } => write!(
                formatter,
                "decoded output has {actual} bytes, expected {expected}"
            ),
            Self::Decoder(detail) => write!(formatter, "LZMA decoder: {detail}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{decompress_block, decompress_size_block, LzmaBlockErrorKind};

    #[test]
    fn decodes_a_raw_stream_with_unity_properties() {
        let original = b"Unity LZMA blocks preserve the complete decoded payload.";
        let mut framed = Vec::new();
        lzma_rs::lzma_compress(&mut Cursor::new(original), &mut framed).unwrap();
        assert!(framed.len() >= 13);

        // The .lzma-alone frame has a 5-byte properties prefix, an 8-byte
        // uncompressed-size field, and the same raw stream used by Unity.
        let mut unity_payload = framed[..5].to_vec();
        unity_payload.extend_from_slice(&framed[13..]);

        assert_eq!(
            decompress_block(&unity_payload, original.len(), original.len()).unwrap(),
            original
        );
    }

    #[test]
    fn decodes_a_size_prefixed_lzma_stream() {
        let original = b"Unity Web chunks carry their decoded size in the LZMA frame.";
        let mut framed = Vec::new();
        lzma_rs::lzma_compress(&mut Cursor::new(original), &mut framed).unwrap();
        framed[5..13].copy_from_slice(&(original.len() as u64).to_le_bytes());

        assert_eq!(
            decompress_size_block(&framed, original.len(), original.len()).unwrap(),
            original
        );
    }

    #[test]
    fn rejects_truncated_or_mismatched_size_prefixes() {
        let short = decompress_size_block(&[0; 12], 0, 0).unwrap_err();
        assert_eq!(
            short.kind,
            LzmaBlockErrorKind::SizeHeaderTooShort { actual: 12 }
        );

        let mut frame = Vec::new();
        lzma_rs::lzma_compress(&mut Cursor::new(b"abc"), &mut frame).unwrap();
        frame[5..13].copy_from_slice(&3u64.to_le_bytes());
        let mismatch = decompress_size_block(&frame, 4, 4).unwrap_err();
        assert_eq!(
            mismatch.kind,
            LzmaBlockErrorKind::DeclaredSizeMismatch {
                declared: 3,
                expected: 4,
            }
        );
    }

    #[test]
    fn rejects_short_and_invalid_property_headers() {
        let short = decompress_block(&[0, 1, 2, 3], 0, 0).unwrap_err();
        assert_eq!(short.input_offset(), 4);
        assert_eq!(short.kind, LzmaBlockErrorKind::HeaderTooShort { actual: 4 });

        let invalid = decompress_block(&[225, 0, 0, 0, 0], 0, 0).unwrap_err();
        assert_eq!(invalid.input_offset(), 0);
        assert_eq!(
            invalid.kind,
            LzmaBlockErrorKind::InvalidProperties { value: 225 }
        );
    }

    #[test]
    fn rejects_output_larger_than_configured_limit() {
        let error = decompress_block(&[0, 0, 0, 0, 0], 16, 15).unwrap_err();
        assert_eq!(
            error.kind,
            LzmaBlockErrorKind::OutputLimitExceeded {
                requested: 16,
                limit: 15
            }
        );
    }

    /// Output-size contract against `LzmaCompression.DecompressLzmaStream`: a short
    /// stream fails closed (the source read loop would leave its pre-sized buffer
    /// short and fail downstream in `ReadMetadata`), while a long stream truncates to
    /// the requested count exactly like the source loop. The outcomes below were
    /// verified against the decoder dependency; `OutputSizeMismatch` and
    /// `OutputExceeded` remain as backstops for decoders that return without
    /// honoring the requested count.
    #[test]
    fn output_size_contract_matches_the_source_read_loop() {
        let original = b"0123456789abcdef0123456789abcdef";
        let mut framed = Vec::new();
        lzma_rs::lzma_compress(&mut Cursor::new(original), &mut framed).unwrap();
        let mut payload = framed[..5].to_vec();
        payload.extend_from_slice(&framed[13..]);
        // Fewer bytes than requested: a hard error, never a short buffer.
        let short = decompress_block(&payload, 64, 64).unwrap_err();
        assert!(matches!(short.kind, LzmaBlockErrorKind::Decoder(_)));
        // More bytes than requested: the first `expected` bytes, like the source loop.
        assert_eq!(decompress_block(&payload, 8, 64).unwrap(), b"01234567");
    }

    #[test]
    fn rejects_malformed_raw_stream_with_context_offset() {
        let error = decompress_block(&[93, 0, 0, 16, 0, 1], 32, 64).unwrap_err();
        assert!(matches!(error.kind, LzmaBlockErrorKind::Decoder(_)));
        assert!(error.input_offset >= 5);
    }

    /// Independent-encoder vectors for the size-prefixed lane: each payload was
    /// encoded with SharpCompress 0.50.4 (the baseline's own LZMA library) in an
    /// offline dotnet probe, framed as five property bytes plus the little-endian
    /// decoded length plus the raw stream (the `DecompressLzmaSizeStream` layout),
    /// and round-tripped through the SharpCompress decoder as validity proof.
    /// Nothing here was produced by `lzma-rs`. Both use properties
    /// `5D 00 00 10 00` (lc=3, lp=0, pb=2, 1 MiB dictionary).
    #[test]
    fn decodes_sharpcompress_encoded_size_prefixed_payloads() {
        let scene_text = parse_hex(
            "5D000010007800000000000000002A9B89277861869378C2126627CA9A4A37B67458E0262ACF8BB3465C1FCB99398063CF8000",
        );
        assert_eq!(scene_text.len(), 51);
        let expected_scene_text =
            b"UnityWeb scene chunk payload. UnityWeb scene chunk payload. UnityWeb scene chunk payload. UnityWeb scene chunk payload. ";
        assert_eq!(expected_scene_text.len(), 120);
        assert_eq!(
            decompress_size_block(
                &scene_text,
                expected_scene_text.len(),
                expected_scene_text.len()
            )
            .unwrap()
            .as_slice(),
            expected_scene_text
        );

        let sequence = parse_hex(
            "5D00001000000100000000000000000052500A84F99BB28021A969D627E03E065A5F048D53D404BA39570509C15524DE9DB871593160A19FF96F4973F2C8EA8CBA1A8B29692180FE338366AF466DEC9E898A0B83F03C0E898E3FED5FE79E90D91CFF32F4B2E03951B2D21415B4C571BADB06E3799A9FBB38C1B000AC930BAA0619031208155B9BC848F0322EFE2DA087C8F0A4E0D251EB8D675692B24D84C5F18631DF6A625BC2792DD9F73C73BA747407D83CA9562224A166F85A845F3067D2F64B492E7F20EBDBF8100E947877C73F6BEFB4CD95E26FF6446E06CF0B821ACBDB7AF0578D98FF90C03EE6C1124175EE032896EB09370E1E",
        );
        assert_eq!(sequence.len(), 248);
        let expected_sequence: Vec<u8> = (0u8..=255).collect();
        assert_eq!(
            decompress_size_block(&sequence, expected_sequence.len(), expected_sequence.len())
                .unwrap(),
            expected_sequence
        );
    }

    fn parse_hex(hex: &str) -> Vec<u8> {
        assert!(hex.len().is_multiple_of(2), "test hex must pair nibbles");
        (0..hex.len())
            .step_by(2)
            .map(|index| {
                u8::from_str_radix(&hex[index..index + 2], 16).expect("test hex must be valid")
            })
            .collect()
    }

    /// Independent-encoder vectors for the UnityFS metadata lane: each payload was
    /// encoded with SharpCompress 0.50.4 (the baseline's own LZMA library, mirroring
    /// `CompressLzmaStream`: five property bytes plus the raw stream) in an offline
    /// dotnet probe, which also round-tripped every vector through the SharpCompress
    /// decoder. Nothing here was produced by `lzma-rs`, breaking the self-encoding
    /// loop the other tests use. All three use properties `5D 00 00 10 00`
    /// (lc=3, lp=0, pb=2, 1 MiB dictionary).
    #[test]
    fn decodes_sharpcompress_encoded_unity_payloads() {
        let repeated =
            parse_hex("5D0000100000241949986F1011485D43E4F891745C8A47E9D66EE801B113E65249BE5E8000");
        assert_eq!(repeated.len(), 37);
        let expected_repeated =
            b"Hello UnityFS metadata! Hello UnityFS metadata! Hello UnityFS metadata! ";
        assert_eq!(expected_repeated.len(), 72);
        assert_eq!(
            decompress_block(&repeated, expected_repeated.len(), expected_repeated.len())
                .unwrap()
                .as_slice(),
            expected_repeated
        );

        let sequence = parse_hex(
            "5D0000100000000052500A84F99BB28021A969D627E03E065A5F048D53D404BA39570509C15524DE9DB871593160A19FF96F4973F2C8EA8CBA1A8B29692180FE338366AF466DEC9E898A0B83F03C0E898E3FED5FE79E90D91CFF32F4B2E03951B2D21415B4C571BADB06E3799A9FBB38C1B000AC930BAA0619031208155B9BC848F0322EFE2DA087C8F0A4E0D251EB8D675692B24D84C5F18631DF6A625BC2792DD9F73C73BA747407D83CA9562224A166F85A845F3067D2F64B492E7F20EBDBF8100E947877C73F6BEFB4CD95E26FF6446E06CF0B821ACBDB7AF0578D98FF90C03EE6C1124175EE032896EB09370E1E",
        );
        assert_eq!(sequence.len(), 240);
        let expected_sequence: Vec<u8> = (0u8..=255).collect();
        assert_eq!(
            decompress_block(&sequence, expected_sequence.len(), expected_sequence.len()).unwrap(),
            expected_sequence
        );

        let metadata = parse_hex(
            "5D00001000002D6D3C1A00177EEB2644FF6A1F536A5A91464786307E5BE1A06A1EAF3A8CA000",
        );
        assert_eq!(metadata.len(), 38);
        let mut expected_metadata = vec![0x5A; 16];
        expected_metadata.extend_from_slice(&[0, 0, 0, 1]);
        expected_metadata.extend_from_slice(&[0, 0, 0, 19]);
        expected_metadata.extend_from_slice(&[0, 0, 0, 19]);
        expected_metadata.extend_from_slice(&[0, 0]);
        expected_metadata.extend_from_slice(&[0; 8]);
        expected_metadata.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 19]);
        expected_metadata.extend_from_slice(&[0, 0, 0, 4]);
        expected_metadata.extend_from_slice(b"data/file.bin");
        expected_metadata.push(0);
        assert_eq!(expected_metadata.len(), 64);
        assert_eq!(
            decompress_block(&metadata, expected_metadata.len(), expected_metadata.len()).unwrap(),
            expected_metadata
        );
    }

    #[test]
    fn reference_runtime_outputs_match_the_rust_decoder() {
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

        // Probe Temp/opencode/lzmaprobe (LzmaCompression over SharpCompress,
        // SDK 10.0.401): existing SharpCompress vectors decoded through the
        // reference entry points, plus reference-generated GEN vectors, plus
        // the LZMA-metadata write refusal and the -1 size silent-empty proof.
        // Nothing here was produced by lzma-rs.
        let fixture = include_str!("lzma_reference_fixture.txt");
        let mut refs: std::collections::HashMap<String, Vec<u8>> = std::collections::HashMap::new();
        let mut gen_unity: Option<(Vec<u8>, Vec<u8>)> = None;
        let mut gen_size: Option<(Vec<u8>, Vec<u8>)> = None;
        let mut write_outcome: Option<String> = None;
        let mut neg_outcome: Option<String> = None;
        for line in fixture.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            if line.starts_with("LZMA_REF|") {
                // LZMA_REF|name|out=HEX|outlen=N|want=N
                let parts: Vec<&str> = line.split('|').collect();
                assert_eq!(parts.len(), 5, "bad LZMA_REF {line}");
                let out_hex = parts[2].strip_prefix("out=").unwrap();
                let outlen: usize = parts[3].strip_prefix("outlen=").unwrap().parse().unwrap();
                let want: usize = parts[4].strip_prefix("want=").unwrap().parse().unwrap();
                assert_eq!(outlen, want, "probe self-consistent {line}");
                refs.insert(parts[1].to_string(), hex_to_bytes(out_hex));
            } else if line.starts_with("LZMA_GEN|") {
                // LZMA_GEN|name|plain=HEX|comp=HEX|back=HEX
                let parts: Vec<&str> = line.split('|').collect();
                assert_eq!(parts.len(), 5, "bad LZMA_GEN {line}");
                let plain = hex_to_bytes(parts[2].strip_prefix("plain=").unwrap());
                let comp = hex_to_bytes(parts[3].strip_prefix("comp=").unwrap());
                let back = hex_to_bytes(parts[4].strip_prefix("back=").unwrap());
                assert_eq!(plain, back, "probe round-trip {}", parts[1]);
                if parts[1] == "unity-gen" {
                    gen_unity = Some((plain, comp));
                } else {
                    gen_size = Some((plain, comp));
                }
            } else if line.starts_with("LZMA_WRITE|") {
                write_outcome = Some(line.strip_prefix("LZMA_WRITE|").unwrap().to_string());
            } else if line.starts_with("LZMA_NEG|") {
                neg_outcome = Some(line.strip_prefix("LZMA_NEG|").unwrap().to_string());
            }
        }

        // Baseline cannot write LZMA metadata; no Rust write unit is owed.
        let write_outcome = write_outcome.expect("LZMA_WRITE row");
        assert!(
            write_outcome.contains("NotImplementedException"),
            "write refusal {write_outcome}"
        );

        // Baseline silently yields empty on a -1 size prefix; Rust fails
        // closed instead (documented hardening, never silent success).
        let neg_outcome = neg_outcome.expect("LZMA_NEG row");
        assert!(
            neg_outcome.contains("outlen=0"),
            "negative yields empty {neg_outcome}"
        );
        let neg_comp = hex_to_bytes("5D00001000FFFFFFFFFFFFFFFF002A9B89277861869378C2126627CA9A4A37B67458E0262ACF8BB3465C1FCB99398063CF8000");
        assert!(decompress_size_block(&neg_comp, 120, 120).is_err());

        // Reference-decoded outputs match the Rust decoder byte-for-byte.
        // Unity lane (DecompressLzmaStream layout).
        for (name, comp_hex, want_len) in [
            (
                "unity-repeated",
                "5D0000100000241949986F1011485D43E4F891745C8A47E9D66EE801B113E65249BE5E8000",
                72usize,
            ),
            (
                "unity-sequence",
                "5D0000100000000052500A84F99BB28021A969D627E03E065A5F048D53D404BA39570509C15524DE9DB871593160A19FF96F4973F2C8EA8CBA1A8B29692180FE338366AF466DEC9E898A0B83F03C0E898E3FED5FE79E90D91CFF32F4B2E03951B2D21415B4C571BADB06E3799A9FBB38C1B000AC930BAA0619031208155B9BC848F0322EFE2DA087C8F0A4E0D251EB8D675692B24D84C5F18631DF6A625BC2792DD9F73C73BA747407D83CA9562224A166F85A845F3067D2F64B492E7F20EBDBF8100E947877C73F6BEFB4CD95E26FF6446E06CF0B821ACBDB7AF0578D98FF90C03EE6C1124175EE032896EB09370E1E",
                256usize,
            ),
        ] {
            let ref_out = refs.get(name).unwrap_or_else(|| panic!("missing {name}"));
            let comp = hex_to_bytes(comp_hex);
            let rust_out = decompress_block(&comp, want_len, want_len).unwrap();
            assert_eq!(&rust_out, ref_out, "{name} matches reference");
        }
        // Size lane (DecompressLzmaSizeStream layout).
        for (name, comp_hex, want_len) in [
            (
                "size-scene",
                "5D000010007800000000000000002A9B89277861869378C2126627CA9A4A37B67458E0262ACF8BB3465C1FCB99398063CF8000",
                120usize,
            ),
            (
                "size-sequence",
                "5D00001000000100000000000000000052500A84F99BB28021A969D627E03E065A5F048D53D404BA39570509C15524DE9DB871593160A19FF96F4973F2C8EA8CBA1A8B29692180FE338366AF466DEC9E898A0B83F03C0E898E3FED5FE79E90D91CFF32F4B2E03951B2D21415B4C571BADB06E3799A9FBB38C1B000AC930BAA0619031208155B9BC848F0322EFE2DA087C8F0A4E0D251EB8D675692B24D84C5F18631DF6A625BC2792DD9F73C73BA747407D83CA9562224A166F85A845F3067D2F64B492E7F20EBDBF8100E947877C73F6BEFB4CD95E26FF6446E06CF0B821ACBDB7AF0578D98FF90C03EE6C1124175EE032896EB09370E1E",
                256usize,
            ),
        ] {
            let ref_out = refs.get(name).unwrap_or_else(|| panic!("missing {name}"));
            let comp = hex_to_bytes(comp_hex);
            let rust_out = decompress_size_block(&comp, want_len, want_len).unwrap();
            assert_eq!(&rust_out, ref_out, "{name} matches reference");
        }
        // Reference-generated vectors decode through Rust as well.
        let (plain, comp) = gen_unity.expect("unity-gen");
        assert_eq!(
            &decompress_block(&comp, plain.len(), plain.len()).unwrap(),
            &plain
        );
        let (plain, comp) = gen_size.expect("size-gen");
        assert_eq!(
            &decompress_size_block(&comp, plain.len(), plain.len()).unwrap(),
            &plain
        );
    }
}
