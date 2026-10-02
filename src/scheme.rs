//! Source-compatible detection and recursive decoding of top-level file schemes.

use std::borrow::Cow;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::compression::wrappers::{
    decompress_brotli, decompress_gzip, has_brotli_file_signature, has_gzip_magic,
    WrapperCodecError,
};
use crate::container::raw_web::{RawWebBundle, RawWebError, RawWebKind, RawWebLimits};
use crate::container::unityfs::{UnityFsBundle, UnityFsError, UnityFsLimits};
use crate::container::web_file::{WebFile, WebFileError, WebFileLimits};
use crate::serialized_file::file::{SerializedFile, SerializedFileError};
use crate::serialized_file::format_version::FormatVersion;

/// The file types recognized by the local AssetRipper `SchemeReader`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileKind {
    UnityFs,
    UnityRaw,
    UnityWebBundle,
    UnityArchive,
    UnityWebData,
    SerializedFile,
    Resource,
}

/// Compression layers applied outside the final recognized file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WrapperKind {
    Gzip,
    Brotli,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WrapperLayer {
    pub kind: WrapperKind,
    pub input_size: usize,
    pub output_size: usize,
}

/// Limits for wrapper recursion and the source parsers called by the dispatcher.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SchemeReaderLimits {
    pub max_wrapper_depth: usize,
    pub max_decompressed_bytes_per_layer: usize,
    pub max_total_decompressed_bytes: usize,
    pub unity_fs: UnityFsLimits,
    pub raw_web: RawWebLimits,
    pub web_file: WebFileLimits,
}

impl Default for SchemeReaderLimits {
    fn default() -> Self {
        Self {
            max_wrapper_depth: 16,
            max_decompressed_bytes_per_layer: 512 * 1024 * 1024,
            max_total_decompressed_bytes: 2 * 1024 * 1024 * 1024,
            unity_fs: UnityFsLimits::default(),
            raw_web: RawWebLimits::default(),
            web_file: WebFileLimits::default(),
        }
    }
}

/// Input bytes after any recognized GZip/Brotli wrappers have been removed.
///
/// The original input is borrowed when no wrapper exists. For wrapped input only the latest
/// decoded layer is retained, so nested wrappers do not keep every intermediate expansion alive.
#[derive(Debug)]
pub struct DispatchedFile<'a> {
    contents: Cow<'a, [u8]>,
    kind: FileKind,
    wrappers: Vec<WrapperLayer>,
    limits: SchemeReaderLimits,
}

impl<'a> DispatchedFile<'a> {
    pub fn kind(&self) -> FileKind {
        self.kind
    }

    pub fn wrappers(&self) -> &[WrapperLayer] {
        &self.wrappers
    }

    pub fn bytes(&self) -> &[u8] {
        self.contents.as_ref()
    }

    /// Match the source `SchemeReader.IsReadableFile` behavior, which does not count unknown
    /// resource files as scheme-readable.
    pub fn is_readable(&self) -> bool {
        self.kind != FileKind::Resource || !self.wrappers.is_empty()
    }

    /// Dispatch one resource entry in a Unity WebData file through the same scheme registry.
    pub fn parse_web_data_entry(&self, index: usize) -> Result<DispatchedFile<'_>, SchemeError> {
        if self.kind != FileKind::UnityWebData {
            return Err(SchemeError::new(SchemeErrorKind::NotWebDataContainer));
        }
        let ParsedFile::WebData(web_file) = self.parse()? else {
            return Err(SchemeError::new(SchemeErrorKind::NotWebDataContainer));
        };
        let bytes = web_file.entry_data(index).ok_or_else(|| {
            SchemeError::new(SchemeErrorKind::WebDataEntryIndexOutOfBounds {
                index,
                entry_count: web_file.entries.len(),
            })
        })?;
        SchemeReader::read_with_limits(bytes, self.limits)
    }

    /// Parse the detected leaf with its format parser.
    pub fn parse(&self) -> Result<ParsedFile<'_>, SchemeError> {
        let bytes = self.contents.as_ref();
        match self.kind {
            FileKind::UnityFs => UnityFsBundle::parse_with_limits(bytes, self.limits.unity_fs)
                .map(ParsedFile::UnityFs)
                .map_err(|error| SchemeError::new(SchemeErrorKind::UnityFs(error))),
            FileKind::UnityRaw | FileKind::UnityWebBundle => {
                let bundle = RawWebBundle::parse_with_limits(bytes, self.limits.raw_web)
                    .map_err(|error| SchemeError::new(SchemeErrorKind::RawWeb(error)))?;
                let expected_kind = if self.kind == FileKind::UnityRaw {
                    RawWebKind::UnityRaw
                } else {
                    RawWebKind::UnityWeb
                };
                if bundle.header.kind != expected_kind {
                    return Err(SchemeError::new(SchemeErrorKind::DetectedKindChanged {
                        detected: self.kind,
                        parsed: bundle.header.kind,
                    }));
                }
                Ok(ParsedFile::RawWeb(bundle))
            }
            FileKind::UnityArchive => {
                Err(SchemeError::new(SchemeErrorKind::ArchiveReadUnsupported))
            }
            FileKind::UnityWebData => WebFile::parse_with_limits(bytes, self.limits.web_file)
                .map(ParsedFile::WebData)
                .map_err(|error| SchemeError::new(SchemeErrorKind::WebFile(error))),
            FileKind::SerializedFile => SerializedFile::parse(bytes)
                .map(ParsedFile::SerializedFile)
                .map_err(|error| SchemeError::new(SchemeErrorKind::SerializedFile(error))),
            FileKind::Resource => Ok(ParsedFile::Resource(bytes)),
        }
    }
}

/// Parsed file contents. Views borrow from the owning [`DispatchedFile`].
#[derive(Debug)]
pub enum ParsedFile<'a> {
    UnityFs(UnityFsBundle<'a>),
    RawWeb(RawWebBundle<'a>),
    WebData(WebFile<'a>),
    SerializedFile(SerializedFile<'a>),
    Resource(&'a [u8]),
}

pub struct SchemeReader;

impl SchemeReader {
    /// Detect the source scheme and decode any outer GZip/Brotli wrappers recursively.
    pub fn read(input: &[u8]) -> Result<DispatchedFile<'_>, SchemeError> {
        Self::read_with_limits(input, SchemeReaderLimits::default())
    }

    /// Check only scheme signatures, matching `SchemeReader.IsReadableFile`; this does not decode
    /// or fully parse the input. UnityArchive is considered readable here because the baseline
    /// registers its detector even though its `Read` implementation is unsupported.
    pub fn is_readable(input: &[u8]) -> bool {
        detect_primary_file_kind(input).is_some()
            || has_brotli_file_signature(input)
            || has_gzip_magic(input)
            || is_serialized_file(input)
    }

    pub fn read_with_limits(
        input: &[u8],
        limits: SchemeReaderLimits,
    ) -> Result<DispatchedFile<'_>, SchemeError> {
        let mut contents = Cow::Borrowed(input);
        let mut wrappers = Vec::new();
        let mut total_decompressed = 0usize;

        loop {
            let current = contents.as_ref();
            if let Some(kind) = detect_primary_file_kind(current) {
                return Ok(DispatchedFile {
                    contents,
                    kind,
                    wrappers,
                    limits,
                });
            }
            let Some((kind, output)) = decode_wrapper(current, limits, wrappers.len())? else {
                let kind = detect_file_kind(current);
                return Ok(DispatchedFile {
                    contents,
                    kind,
                    wrappers,
                    limits,
                });
            };

            let requested_total =
                total_decompressed
                    .checked_add(output.len())
                    .ok_or_else(|| {
                        SchemeError::new(SchemeErrorKind::TotalDecompressedLengthOverflow)
                    })?;
            if requested_total > limits.max_total_decompressed_bytes {
                return Err(SchemeError::new(
                    SchemeErrorKind::TotalDecompressionLimitExceeded {
                        requested: requested_total,
                        limit: limits.max_total_decompressed_bytes,
                    },
                ));
            }
            wrappers.try_reserve(1).map_err(|_| {
                SchemeError::new(SchemeErrorKind::AllocationFailed {
                    requested: wrappers.len().saturating_add(1),
                })
            })?;
            wrappers.push(WrapperLayer {
                kind,
                input_size: current.len(),
                output_size: output.len(),
            });
            total_decompressed = requested_total;
            contents = Cow::Owned(output);
        }
    }
}

fn decode_wrapper(
    input: &[u8],
    limits: SchemeReaderLimits,
    depth: usize,
) -> Result<Option<(WrapperKind, Vec<u8>)>, SchemeError> {
    let kind = if has_gzip_magic(input) {
        Some(WrapperKind::Gzip)
    } else if has_brotli_file_signature(input) {
        Some(WrapperKind::Brotli)
    } else {
        None
    };
    let Some(kind) = kind else {
        return Ok(None);
    };
    if depth >= limits.max_wrapper_depth {
        return Err(SchemeError::new(SchemeErrorKind::WrapperDepthExceeded {
            limit: limits.max_wrapper_depth,
        }));
    }

    let output = match kind {
        WrapperKind::Gzip => decompress_gzip(input, limits.max_decompressed_bytes_per_layer),
        WrapperKind::Brotli => decompress_brotli(input, limits.max_decompressed_bytes_per_layer),
    }
    .map_err(|error| {
        SchemeError::new(SchemeErrorKind::WrapperDecode {
            kind,
            source: error,
        })
    })?;
    Ok(Some((kind, output)))
}

/// Match the effective enumeration order of the source `Stack<IScheme>`.
///
/// The source collection initializer pushes SerializedFile first and FileStreamBundle last;
/// `Stack<T>` enumerates from its top, so the actual precedence starts with UnityFS.
fn detect_file_kind(input: &[u8]) -> FileKind {
    if let Some(kind) = detect_primary_file_kind(input) {
        return kind;
    }
    if is_serialized_file(input) {
        FileKind::SerializedFile
    } else {
        FileKind::Resource
    }
}

fn detect_primary_file_kind(input: &[u8]) -> Option<FileKind> {
    if has_bundle_signature(input, b"UnityFS") {
        Some(FileKind::UnityFs)
    } else if has_bundle_signature(input, b"UnityRaw") {
        Some(FileKind::UnityRaw)
    } else if has_bundle_signature(input, b"UnityWeb") {
        Some(FileKind::UnityWebBundle)
    } else if has_bundle_signature(input, b"UnityArchive") {
        Some(FileKind::UnityArchive)
    } else if WebFile::is_web_file(input) {
        Some(FileKind::UnityWebData)
    } else {
        None
    }
}

fn has_bundle_signature(input: &[u8], magic: &[u8]) -> bool {
    const SOURCE_MINIMUM_STREAM_LENGTH: usize = 0x20;
    input.len() >= SOURCE_MINIMUM_STREAM_LENGTH
        && input.get(..magic.len()) == Some(magic)
        && input.get(magic.len()) == Some(&0)
}

/// Source-compatible subset of `SerializedFileHeader.IsSerializedFileHeader`.
fn is_serialized_file(input: &[u8]) -> bool {
    if input.len() < 16 {
        return false;
    }
    let legacy_metadata_size = i32::from_be_bytes(input[0..4].try_into().expect("fixed slice"));
    let legacy_file_size = u32::from_be_bytes(input[4..8].try_into().expect("fixed slice"));
    let raw_version = i32::from_be_bytes(input[8..12].try_into().expect("fixed slice"));
    let Ok(version) = FormatVersion::try_from(raw_version) else {
        return false;
    };

    let (metadata_size, file_size) = if version.has_large_file_support() {
        let Some(metadata) = input.get(20..24) else {
            return false;
        };
        let Some(size) = input.get(24..32) else {
            return false;
        };
        (
            i32::from_be_bytes(metadata.try_into().expect("fixed slice")),
            u64::from_be_bytes(size.try_into().expect("fixed slice")),
        )
    } else {
        (legacy_metadata_size, u64::from(legacy_file_size))
    };

    metadata_size >= 13 && file_size >= 16 + 13 && u64::try_from(input.len()) == Ok(file_size)
}

#[derive(Debug)]
pub struct SchemeError {
    kind: SchemeErrorKind,
}

impl SchemeError {
    fn new(kind: SchemeErrorKind) -> Self {
        Self { kind }
    }

    pub fn kind(&self) -> &SchemeErrorKind {
        &self.kind
    }
}

impl Display for SchemeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.kind, formatter)
    }
}

impl Error for SchemeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            SchemeErrorKind::WrapperDecode { source, .. } => Some(source),
            SchemeErrorKind::UnityFs(error) => Some(error),
            SchemeErrorKind::RawWeb(error) => Some(error),
            SchemeErrorKind::WebFile(error) => Some(error),
            SchemeErrorKind::SerializedFile(error) => Some(error),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum SchemeErrorKind {
    WrapperDepthExceeded {
        limit: usize,
    },
    TotalDecompressionLimitExceeded {
        requested: usize,
        limit: usize,
    },
    TotalDecompressedLengthOverflow,
    NotWebDataContainer,
    WebDataEntryIndexOutOfBounds {
        index: usize,
        entry_count: usize,
    },
    AllocationFailed {
        requested: usize,
    },
    WrapperDecode {
        kind: WrapperKind,
        source: WrapperCodecError,
    },
    UnityFs(UnityFsError),
    RawWeb(RawWebError),
    WebFile(WebFileError),
    SerializedFile(SerializedFileError),
    DetectedKindChanged {
        detected: FileKind,
        parsed: RawWebKind,
    },
    ArchiveReadUnsupported,
}

impl Display for SchemeErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrapperDepthExceeded { limit } => {
                write!(formatter, "compressed wrapper nesting exceeds limit {limit}")
            }
            Self::TotalDecompressionLimitExceeded { requested, limit } => write!(
                formatter,
                "nested wrappers decoded {requested} total bytes, exceeding limit {limit}"
            ),
            Self::TotalDecompressedLengthOverflow => {
                write!(formatter, "total decompressed byte count overflows")
            }
            Self::NotWebDataContainer => {
                write!(formatter, "cannot dispatch an entry from a non-WebData file")
            }
            Self::WebDataEntryIndexOutOfBounds { index, entry_count } => write!(
                formatter,
                "WebData entry index {index} is outside the {entry_count} parsed entries"
            ),
            Self::AllocationFailed { requested } => {
                write!(formatter, "could not allocate {requested} wrapper records")
            }
            Self::WrapperDecode { kind, source } => {
                write!(formatter, "{kind:?} wrapper: {source}")
            }
            Self::UnityFs(error) => write!(formatter, "UnityFS parser: {error}"),
            Self::RawWeb(error) => write!(formatter, "UnityRaw/UnityWeb parser: {error}"),
            Self::WebFile(error) => write!(formatter, "Unity WebData parser: {error}"),
            Self::SerializedFile(error) => write!(formatter, "SerializedFile parser: {error}"),
            Self::DetectedKindChanged { detected, parsed } => write!(
                formatter,
                "scheme detector selected {detected:?}, but the parsed raw/web signature was {parsed:?}"
            ),
            Self::ArchiveReadUnsupported => write!(
                formatter,
                "UnityArchive is recognized by the source baseline, whose reader is unsupported"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FileKind, SchemeErrorKind, SchemeReader, SchemeReaderLimits, WrapperKind};
    use crate::compression::wrappers::{
        compress_gzip, decompress_brotli, has_brotli_file_signature,
    };

    fn write_bits(bytes: &mut Vec<u8>, bit_position: &mut usize, value: u32, count: usize) {
        for bit in 0..count {
            let byte_index = *bit_position / 8;
            if bytes.len() <= byte_index {
                bytes.resize(byte_index + 1, 0);
            }
            if value & (1 << bit) != 0 {
                bytes[byte_index] |= 1 << (*bit_position % 8);
            }
            *bit_position += 1;
        }
    }

    fn unity_marked_brotli_fixture(output: &[u8], window_bits: u8) -> Vec<u8> {
        let marker = b"UnityWeb Compressed Content (brotli)";
        let mut bytes = Vec::new();
        let mut bit_position = 0;

        // RFC 7932 stream header, followed by an empty metadata meta-block.
        let (window_code, window_code_bits) = match window_bits {
            10 => (33, 7),
            11 => (49, 7),
            12 => (65, 7),
            13 => (81, 7),
            14 => (97, 7),
            15 => (113, 7),
            16 => (0, 1),
            17 => (1, 7),
            18 => (3, 4),
            19 => (5, 4),
            20 => (7, 4),
            21 => (9, 4),
            22 => (11, 4),
            23 => (13, 4),
            24 => (15, 4),
            _ => panic!("invalid fixture WBITS"),
        };
        write_bits(&mut bytes, &mut bit_position, window_code, window_code_bits);
        write_bits(&mut bytes, &mut bit_position, 0, 1); // ISLAST
        write_bits(&mut bytes, &mut bit_position, 3, 2); // MNIBBLES=0
        write_bits(&mut bytes, &mut bit_position, 0, 1); // reserved
        write_bits(&mut bytes, &mut bit_position, 1, 2); // MSKIPBYTES=1
        write_bits(&mut bytes, &mut bit_position, marker.len() as u32 - 1, 8);
        while bit_position & 7 != 0 {
            write_bits(&mut bytes, &mut bit_position, 0, 1);
        }
        bytes.extend_from_slice(marker);
        bit_position += marker.len() * 8;

        // Non-final uncompressed meta-block containing the requested payload.
        let length_minus_one = output.len().checked_sub(1).unwrap() as u32;
        write_bits(&mut bytes, &mut bit_position, 0, 1); // ISLAST=0
        write_bits(&mut bytes, &mut bit_position, 0, 2); // MNIBBLES=4
        write_bits(&mut bytes, &mut bit_position, length_minus_one, 16);
        write_bits(&mut bytes, &mut bit_position, 1, 1); // ISUNCOMPRESSED
        while bit_position & 7 != 0 {
            write_bits(&mut bytes, &mut bit_position, 0, 1);
        }
        bytes.extend_from_slice(output);
        bit_position += output.len() * 8;

        // Final empty meta-block.
        write_bits(&mut bytes, &mut bit_position, 1, 1); // ISLAST
        write_bits(&mut bytes, &mut bit_position, 1, 1); // ISLASTEMPTY
        while bit_position & 7 != 0 {
            write_bits(&mut bytes, &mut bit_position, 0, 1);
        }
        bytes
    }

    #[test]
    fn dispatches_nested_gzip_wrappers_to_the_inner_resource() {
        let once = compress_gzip(b"inner payload", 1024).unwrap();
        let twice = compress_gzip(&once, 1024).unwrap();
        let file = SchemeReader::read(&twice).unwrap();

        assert_eq!(file.kind(), FileKind::Resource);
        assert_eq!(file.bytes(), b"inner payload");
        assert_eq!(
            file.wrappers()
                .iter()
                .map(|layer| layer.kind)
                .collect::<Vec<_>>(),
            [WrapperKind::Gzip, WrapperKind::Gzip]
        );
        assert!(matches!(
            file.parse().unwrap(),
            super::ParsedFile::Resource(_)
        ));
    }

    #[test]
    fn recursively_dispatches_webdata_entries() {
        let payload = compress_gzip(b"embedded asset", 1024).unwrap();
        let name = b"main.asset";
        let header_size = 16 + 4 + 4 + 4 + 1 + name.len();
        let mut input = b"UnityWebData1.0\0".to_vec();
        input.extend_from_slice(&(header_size as i32).to_le_bytes());
        input.extend_from_slice(&(header_size as i32).to_le_bytes());
        input.extend_from_slice(&(payload.len() as i32).to_le_bytes());
        input.push(name.len() as u8);
        input.extend_from_slice(name);
        input.extend_from_slice(&payload);

        let web_file = SchemeReader::read(&input).unwrap();
        assert_eq!(web_file.kind(), FileKind::UnityWebData);
        let entry = web_file.parse_web_data_entry(0).unwrap();
        assert_eq!(entry.bytes(), b"embedded asset");
        assert_eq!(entry.wrappers()[0].kind, WrapperKind::Gzip);
        assert!(matches!(
            entry.parse().unwrap(),
            super::ParsedFile::Resource(_)
        ));
    }

    #[test]
    fn dispatches_a_spec_encoded_unity_brotli_metadata_wrapper() {
        for window_bits in 10..=24 {
            let input = unity_marked_brotli_fixture(b"webdata payload", window_bits);
            assert!(has_brotli_file_signature(&input), "WBITS={window_bits}");
            assert_eq!(
                decompress_brotli(&input, 1024).unwrap(),
                b"webdata payload",
                "WBITS={window_bits}"
            );
        }

        let input = unity_marked_brotli_fixture(b"webdata payload", 18);
        let file = SchemeReader::read(&input).unwrap();
        assert_eq!(file.kind(), FileKind::Resource);
        assert_eq!(file.bytes(), b"webdata payload");
        assert_eq!(file.wrappers()[0].kind, WrapperKind::Brotli);
    }

    #[test]
    fn readability_checks_signatures_without_decoding() {
        assert!(SchemeReader::is_readable(b"\x1f\x8b"));
        assert!(!SchemeReader::is_readable(b"arbitrary resource"));
        let mut archive = vec![0; 32];
        archive[..12].copy_from_slice(b"UnityArchive");
        assert!(SchemeReader::is_readable(&archive));
    }

    #[test]
    fn enforces_recursive_and_cumulative_wrapper_limits() {
        let once = compress_gzip(b"data", 1024).unwrap();
        let twice = compress_gzip(&once, 1024).unwrap();

        let depth_error = SchemeReader::read_with_limits(
            &twice,
            SchemeReaderLimits {
                max_wrapper_depth: 1,
                ..SchemeReaderLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            depth_error.kind(),
            SchemeErrorKind::WrapperDepthExceeded { limit: 1 }
        ));

        let total_error = SchemeReader::read_with_limits(
            &once,
            SchemeReaderLimits {
                max_total_decompressed_bytes: 2,
                ..SchemeReaderLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            total_error.kind(),
            SchemeErrorKind::TotalDecompressionLimitExceeded { limit: 2, .. }
        ));
    }

    #[test]
    fn follows_the_source_bundle_and_webdata_detection_order() {
        for (magic, expected) in [
            (b"UnityFS".as_slice(), FileKind::UnityFs),
            (b"UnityRaw".as_slice(), FileKind::UnityRaw),
            (b"UnityWeb".as_slice(), FileKind::UnityWebBundle),
            (b"UnityArchive".as_slice(), FileKind::UnityArchive),
        ] {
            let mut bytes = vec![0; 32];
            bytes[..magic.len()].copy_from_slice(magic);
            assert_eq!(SchemeReader::read(&bytes).unwrap().kind(), expected);
        }

        let web_data = b"UnityWebData1.0\0\x14\0\0\0";
        assert_eq!(
            SchemeReader::read(web_data).unwrap().kind(),
            FileKind::UnityWebData
        );
    }

    #[test]
    fn reports_the_baseline_archive_reader_as_explicitly_unsupported() {
        let mut bytes = vec![0; 32];
        bytes[..12].copy_from_slice(b"UnityArchive");
        let file = SchemeReader::read(&bytes).unwrap();
        assert_eq!(file.kind(), FileKind::UnityArchive);
        assert!(matches!(
            file.parse().unwrap_err().kind(),
            SchemeErrorKind::ArchiveReadUnsupported
        ));
    }

    #[test]
    fn recognizes_a_source_generation_22_serialized_file_header() {
        let mut bytes = vec![0; 61];
        bytes[8..12].copy_from_slice(&22i32.to_be_bytes());
        bytes[20..24].copy_from_slice(&13i32.to_be_bytes());
        bytes[24..32].copy_from_slice(&61u64.to_be_bytes());
        assert_eq!(
            SchemeReader::read(&bytes).unwrap().kind(),
            FileKind::SerializedFile
        );
    }
}
