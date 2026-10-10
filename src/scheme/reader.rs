use std::borrow::Cow;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::compression::wrappers::{
    decompress_brotli, decompress_brotli_stream, decompress_brotli_stream_to, decompress_gzip,
    decompress_gzip_stream, decompress_gzip_stream_to, has_brotli_file_signature, has_gzip_magic,
};
use crate::container::raw_web::{RawWebBundle, RawWebKind};
use crate::container::unityfs::UnityFsBundle;
use crate::container::web_file::WebFile;
use crate::io::stream::{open_logical_file, SmartStream};
use crate::serialized_file::file::SerializedFile;
use crate::serialized_file::format_version::FormatVersion;

use super::*;
#[derive(Debug)]
pub enum ParsedFile<'a> {
    UnityFs(UnityFsBundle<'a>),
    RawWeb(RawWebBundle<'a>),
    WebData(WebFile<'a>),
    SerializedFile(SerializedFile<'a>),
    Resource(&'a [u8]),
}

pub struct SchemeReader;
/// Input bytes after any recognized GZip/Brotli wrappers have been removed.
///
/// The original input is borrowed when no wrapper exists. For wrapped input only the latest
/// decoded layer is retained, so nested wrappers do not keep every intermediate expansion alive.
#[derive(Debug)]
pub struct DispatchedFile<'a> {
    pub(super) contents: Cow<'a, [u8]>,
    pub(super) kind: FileKind,
    pub(super) wrappers: Vec<WrapperLayer>,
    pub(super) limits: SchemeReaderLimits,
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

    /// Dispatch one directory entry from any supported container through the same registry.
    /// The returned file borrows from this container when the entry is stored without decoding.
    pub fn dispatch_entry(&self, index: usize) -> Result<DispatchedFile<'_>, SchemeError> {
        match self.kind {
            FileKind::UnityFs => {
                let ParsedFile::UnityFs(bundle) = self.parse()? else {
                    return Err(SchemeError::new(SchemeErrorKind::NotContainer));
                };
                let entry = bundle
                    .read_entry(index)
                    .map_err(|error| SchemeError::new(SchemeErrorKind::UnityFs(error)))?;
                SchemeReader::read_cow_with_limits(entry, self.limits)
            }
            FileKind::UnityRaw | FileKind::UnityWebBundle => {
                let ParsedFile::RawWeb(bundle) = self.parse()? else {
                    return Err(SchemeError::new(SchemeErrorKind::NotContainer));
                };
                let entry = bundle
                    .into_entry(index)
                    .map_err(|error| SchemeError::new(SchemeErrorKind::RawWeb(error)))?;
                SchemeReader::read_cow_with_limits(entry, self.limits)
            }
            FileKind::UnityWebData => self.parse_web_data_entry(index),
            _ => Err(SchemeError::new(SchemeErrorKind::NotContainer)),
        }
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
        SchemeReader::read_cow_with_limits(Cow::Borrowed(bytes), self.limits)
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
impl SchemeReader {
    /// Probe a raw VFS file header without reading the complete file or decoding wrappers.
    ///
    /// This is for source-style directory inventory, which checks file and bundle headers
    /// directly. `prefix` should contain at least the first 32 bytes when available.
    pub fn probe_raw_kind(prefix: &[u8], total_length: u64) -> FileKind {
        detect_file_kind_with_length(prefix, total_length)
    }

    /// Detect the source scheme and decode any outer GZip/Brotli wrappers recursively.
    pub fn read(input: &[u8]) -> Result<DispatchedFile<'_>, SchemeError> {
        Self::read_with_limits(input, SchemeReaderLimits::default())
    }

    /// Open a regular path or its numbered split parts and dispatch the logical file.
    ///
    /// This is the bounded seekable streaming entry point: see [`Self::read_path_streaming`].
    pub fn read_path(
        path: impl AsRef<Path>,
        limits: SchemeReaderLimits,
    ) -> Result<DispatchedFile<'static>, SchemeError> {
        Self::read_path_streaming(path, limits)
    }

    /// Read a logical path with bounded seekable streaming.
    ///
    /// The source `SchemeReader.LoadFile` opens a `SmartStream` and lets each scheme read what it
    /// needs instead of buffering the whole input first. This entry point keeps that shape:
    ///
    /// 1. It opens the logical file (a regular file or its assembled numbered `.splitN` parts) and
    ///    probes a bounded prefix for scheme detection, so detection never needs the whole input.
    /// 2. When the prefix is a GZip or Brotli wrapper, the layer is decompressed *directly from the
    ///    seekable stream* through the streaming codec entry points, so the compressed input is
    ///    never materialized. The decoded layer is bounded per layer and cumulatively exactly like
    ///    the in-memory dispatcher; any deeper wrapper is handled by that same bounded state
    ///    machine after the outer layer has been decoded.
    /// 3. Otherwise the finally dispatched layer is materialized, because every container and leaf
    ///    parser in this port reads contiguous bytes.
    pub fn read_path_streaming(
        path: impl AsRef<Path>,
        limits: SchemeReaderLimits,
    ) -> Result<DispatchedFile<'static>, SchemeError> {
        let mut stream = open_logical_file(path)
            .map_err(|error| SchemeError::new(SchemeErrorKind::StreamInput(error)))?;
        Self::read_stream(&mut stream, limits)
    }

    /// Read and dispatch a seekable stream through the same bounded streaming path as a local
    /// file. Outer GZip/Brotli wrappers are decoded from the stream without first copying their
    /// compressed input; the final container or leaf is materialized under `max_input_bytes`
    /// because the current format parsers consume contiguous byte slices.
    pub fn read_stream<R: Read + Seek>(
        stream: &mut R,
        limits: SchemeReaderLimits,
    ) -> Result<DispatchedFile<'static>, SchemeError> {
        let stream_length = stream
            .seek(SeekFrom::End(0))
            .map_err(|error| SchemeError::new(SchemeErrorKind::StreamInput(error)))?;
        if stream_length > u64::try_from(limits.max_input_bytes).unwrap_or(u64::MAX) {
            return Err(SchemeError::new(SchemeErrorKind::InputSizeLimitExceeded {
                requested: stream_length,
                limit: limits.max_input_bytes,
            }));
        }

        let prefix = read_stream_prefix(stream, stream_length)?;
        if let Some(kind) = detect_primary_file_kind_with_length(&prefix, stream_length) {
            let contents = Self::materialize_stream(stream, stream_length)?;
            return Ok(DispatchedFile {
                contents: Cow::Owned(contents),
                kind,
                wrappers: Vec::new(),
                limits,
            });
        }

        let wrapper_kind = if has_gzip_magic(&prefix) {
            Some(WrapperKind::Gzip)
        } else if has_brotli_file_signature(&prefix) {
            Some(WrapperKind::Brotli)
        } else {
            None
        };
        let Some(wrapper_kind) = wrapper_kind else {
            let contents = Self::materialize_stream(stream, stream_length)?;
            let kind = detect_file_kind_with_length(&contents, stream_length);
            return Ok(DispatchedFile {
                contents: Cow::Owned(contents),
                kind,
                wrappers: Vec::new(),
                limits,
            });
        };

        if limits.max_wrapper_depth == 0 {
            return Err(SchemeError::new(SchemeErrorKind::WrapperDepthExceeded {
                limit: 0,
            }));
        }
        stream
            .seek(SeekFrom::Start(0))
            .map_err(|error| SchemeError::new(SchemeErrorKind::StreamInput(error)))?;
        let output = match wrapper_kind {
            WrapperKind::Gzip => {
                decompress_gzip_stream(&mut *stream, limits.max_decompressed_bytes_per_layer)
            }
            WrapperKind::Brotli => {
                decompress_brotli_stream(&mut *stream, limits.max_decompressed_bytes_per_layer)
            }
        }
        .map_err(|error| {
            SchemeError::new(SchemeErrorKind::WrapperDecode {
                kind: wrapper_kind,
                source: error,
            })
        })?;
        if output.len() > limits.max_total_decompressed_bytes {
            return Err(SchemeError::new(
                SchemeErrorKind::TotalDecompressionLimitExceeded {
                    requested: output.len(),
                    limit: limits.max_total_decompressed_bytes,
                },
            ));
        }
        let input_size = usize::try_from(stream_length).map_err(|_| {
            SchemeError::new(SchemeErrorKind::InputLengthNotRepresentable {
                length: stream_length,
            })
        })?;
        let output_size = output.len();
        let layer = WrapperLayer {
            kind: wrapper_kind,
            input_size,
            output_size,
        };
        Self::read_cow_with_state(Cow::Owned(output), limits, vec![layer], output_size)
    }

    /// Strip outer wrappers into bounded temporary streams when the final layer is a
    /// SerializedFile. `None` leaves non-SerializedFile dispatch on the existing scheme path.
    ///
    /// Unlike `read_stream`, this route does not keep the fully decoded layer in a `Vec`; each
    /// wrapper is decoded in bounded chunks to a temporary `SmartStream`, which the downstream
    /// SerializedFile parser can read by range.
    pub(crate) fn unwrap_serialized_file_stream(
        mut stream: SmartStream,
        limits: SchemeReaderLimits,
    ) -> Result<Option<SmartStream>, SchemeError> {
        let mut wrapper_depth = 0usize;
        let mut total_decompressed = 0usize;
        loop {
            let stream_length = stream
                .length()
                .map_err(|error| SchemeError::new(SchemeErrorKind::StreamInput(error)))?;
            if stream_length > u64::try_from(limits.max_input_bytes).unwrap_or(u64::MAX) {
                return Err(SchemeError::new(SchemeErrorKind::InputSizeLimitExceeded {
                    requested: stream_length,
                    limit: limits.max_input_bytes,
                }));
            }
            let prefix = read_smart_stream_prefix(&stream, stream_length)?;
            if is_serialized_file_with_length(&prefix, stream_length) {
                return Ok(Some(stream));
            }
            if detect_primary_file_kind_with_length(&prefix, stream_length).is_some() {
                return Ok(None);
            }

            let wrapper_kind = if has_gzip_magic(&prefix) {
                Some(WrapperKind::Gzip)
            } else if has_brotli_file_signature(&prefix) {
                Some(WrapperKind::Brotli)
            } else {
                None
            };
            let Some(wrapper_kind) = wrapper_kind else {
                return Ok(None);
            };
            if wrapper_depth >= limits.max_wrapper_depth {
                return Err(SchemeError::new(SchemeErrorKind::WrapperDepthExceeded {
                    limit: limits.max_wrapper_depth,
                }));
            }

            stream
                .seek(SeekFrom::Start(0))
                .map_err(|error| SchemeError::new(SchemeErrorKind::StreamInput(error)))?;
            let mut output = SmartStream::create_temp()
                .map_err(|error| SchemeError::new(SchemeErrorKind::StreamOutput(error)))?;
            let output_size = match wrapper_kind {
                WrapperKind::Gzip => decompress_gzip_stream_to(
                    &mut stream,
                    &mut output,
                    limits.max_decompressed_bytes_per_layer,
                ),
                WrapperKind::Brotli => decompress_brotli_stream_to(
                    &mut stream,
                    &mut output,
                    limits.max_decompressed_bytes_per_layer,
                ),
            }
            .map_err(|source| {
                SchemeError::new(SchemeErrorKind::WrapperDecode {
                    kind: wrapper_kind,
                    source,
                })
            })?;
            let total_after_layer =
                total_decompressed.checked_add(output_size).ok_or_else(|| {
                    SchemeError::new(SchemeErrorKind::TotalDecompressedLengthOverflow)
                })?;
            if total_after_layer > limits.max_total_decompressed_bytes {
                return Err(SchemeError::new(
                    SchemeErrorKind::TotalDecompressionLimitExceeded {
                        requested: total_after_layer,
                        limit: limits.max_total_decompressed_bytes,
                    },
                ));
            }
            if output_size > limits.max_input_bytes {
                return Err(SchemeError::new(SchemeErrorKind::InputSizeLimitExceeded {
                    requested: u64::try_from(output_size).unwrap_or(u64::MAX),
                    limit: limits.max_input_bytes,
                }));
            }
            output
                .flush()
                .map_err(|error| SchemeError::new(SchemeErrorKind::StreamOutput(error)))?;
            output
                .seek(SeekFrom::Start(0))
                .map_err(|error| SchemeError::new(SchemeErrorKind::StreamOutput(error)))?;
            total_decompressed = total_after_layer;
            wrapper_depth = wrapper_depth.checked_add(1).ok_or_else(|| {
                SchemeError::new(SchemeErrorKind::TotalDecompressedLengthOverflow)
            })?;
            stream = output;
        }
    }

    /// Check whether a seekable input is a direct UnityFS container without reading its body.
    pub(super) fn is_direct_unityfs_stream<R: Read + Seek>(
        stream: &mut R,
        limits: SchemeReaderLimits,
    ) -> Result<bool, SchemeError> {
        Ok(probe_direct_stream_kind(stream, limits)? == Some(FileKind::UnityFs))
    }

    /// Materialize the remaining seekable layer into bounded storage.
    fn materialize_stream<R: Read + Seek>(
        stream: &mut R,
        length: u64,
    ) -> Result<Vec<u8>, SchemeError> {
        let length = usize::try_from(length).map_err(|_| {
            SchemeError::new(SchemeErrorKind::InputLengthNotRepresentable { length })
        })?;
        stream
            .seek(SeekFrom::Start(0))
            .map_err(|error| SchemeError::new(SchemeErrorKind::StreamInput(error)))?;
        let mut contents = Vec::new();
        contents.try_reserve_exact(length).map_err(|_| {
            SchemeError::new(SchemeErrorKind::InputAllocationFailed { requested: length })
        })?;
        contents.resize(length, 0);
        stream
            .read_exact(&mut contents)
            .map_err(|error| SchemeError::new(SchemeErrorKind::StreamInput(error)))?;
        Ok(contents)
    }

    /// Read one local regular or split-file input and mount it into the virtual filesystem.
    ///
    /// Containers are expanded through `mount_container_tree`; standalone resources and
    /// SerializedFiles are mounted under their logical input filename. Directory and batch
    /// discovery are handled by a higher-level caller.
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
        Self::read_cow_with_limits(Cow::Borrowed(input), limits)
    }

    pub(super) fn read_cow_with_limits<'a>(
        contents: Cow<'a, [u8]>,
        limits: SchemeReaderLimits,
    ) -> Result<DispatchedFile<'a>, SchemeError> {
        Self::read_cow_with_state(contents, limits, Vec::new(), 0)
    }

    /// The in-memory dispatcher, continued from an already-recorded wrapper state.
    ///
    /// [`Self::read_path_streaming`] uses this to hand a streamed wrapper layer to the same
    /// detection/limit logic without re-decoding it or double-counting its bytes.
    fn read_cow_with_state<'a>(
        mut contents: Cow<'a, [u8]>,
        limits: SchemeReaderLimits,
        mut wrappers: Vec<WrapperLayer>,
        mut total_decompressed: usize,
    ) -> Result<DispatchedFile<'a>, SchemeError> {
        if contents.len() > limits.max_input_bytes {
            return Err(SchemeError::new(SchemeErrorKind::InputSizeLimitExceeded {
                requested: u64::try_from(contents.len()).unwrap_or(u64::MAX),
                limit: limits.max_input_bytes,
            }));
        }

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

/// The bounded prefix read from a seekable logical input for scheme detection.
///
/// 64 bytes cover every detection input: the 0x20-byte bundle-header stream minimum, the WebData
/// signature, and the Brotli marker's bit-level header plus its 34-byte signature.
const STREAM_PROBE_BYTES: usize = 64;

/// Probe a bounded prefix of a seekable logical file without materializing the whole input.
fn read_stream_prefix<R: Read + Seek>(stream: &mut R, length: u64) -> Result<Vec<u8>, SchemeError> {
    let prefix_length = usize::try_from(length)
        .unwrap_or(usize::MAX)
        .min(STREAM_PROBE_BYTES);
    stream
        .seek(SeekFrom::Start(0))
        .map_err(|error| SchemeError::new(SchemeErrorKind::StreamInput(error)))?;
    let mut prefix = vec![0u8; prefix_length];
    stream
        .read_exact(&mut prefix)
        .map_err(|error| SchemeError::new(SchemeErrorKind::StreamInput(error)))?;
    Ok(prefix)
}

fn read_smart_stream_prefix(stream: &SmartStream, length: u64) -> Result<Vec<u8>, SchemeError> {
    let prefix_length = usize::try_from(length)
        .unwrap_or(usize::MAX)
        .min(STREAM_PROBE_BYTES);
    let mut prefix = vec![0u8; prefix_length];
    stream
        .read_exact_at(0, &mut prefix)
        .map_err(|error| SchemeError::new(SchemeErrorKind::StreamInput(error)))?;
    Ok(prefix)
}

/// Determine the primary file kind from a seekable stream without reading its body.
/// A wrapper returns `None` so callers that need its decoded leaf can use the full dispatcher.
pub(super) fn probe_direct_stream_kind<R: Read + Seek>(
    stream: &mut R,
    limits: SchemeReaderLimits,
) -> Result<Option<FileKind>, SchemeError> {
    let stream_length = stream
        .seek(SeekFrom::End(0))
        .map_err(|error| SchemeError::new(SchemeErrorKind::StreamInput(error)))?;
    if stream_length > u64::try_from(limits.max_input_bytes).unwrap_or(u64::MAX) {
        return Err(SchemeError::new(SchemeErrorKind::InputSizeLimitExceeded {
            requested: stream_length,
            limit: limits.max_input_bytes,
        }));
    }
    let prefix = read_stream_prefix(stream, stream_length)?;
    if let Some(kind) = detect_primary_file_kind_with_length(&prefix, stream_length) {
        return Ok(Some(kind));
    }
    if is_serialized_file_with_length(&prefix, stream_length) {
        return Ok(Some(FileKind::SerializedFile));
    }
    if has_gzip_magic(&prefix) || has_brotli_file_signature(&prefix) {
        return Ok(None);
    }
    Ok(Some(FileKind::Resource))
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
    detect_file_kind_with_length(input, input.len() as u64)
}

fn detect_file_kind_with_length(input: &[u8], total_length: u64) -> FileKind {
    if let Some(kind) = detect_primary_file_kind_with_length(input, total_length) {
        return kind;
    }
    if is_serialized_file_with_length(input, total_length) {
        FileKind::SerializedFile
    } else {
        FileKind::Resource
    }
}

fn detect_primary_file_kind(input: &[u8]) -> Option<FileKind> {
    detect_primary_file_kind_with_length(input, input.len() as u64)
}

fn detect_primary_file_kind_with_length(input: &[u8], total_length: u64) -> Option<FileKind> {
    if has_bundle_signature_with_length(input, b"UnityFS", total_length) {
        Some(FileKind::UnityFs)
    } else if has_bundle_signature_with_length(input, b"UnityRaw", total_length) {
        Some(FileKind::UnityRaw)
    } else if has_bundle_signature_with_length(input, b"UnityWeb", total_length) {
        Some(FileKind::UnityWebBundle)
    } else if has_bundle_signature_with_length(input, b"UnityArchive", total_length) {
        Some(FileKind::UnityArchive)
    } else if WebFile::is_web_file(input) {
        Some(FileKind::UnityWebData)
    } else {
        None
    }
}

/// The source `BundleHeader.IsBundleHeader` minimum applies to the whole stream length,
/// not the buffered prefix: a short prefix of a long stream still detects when it holds
/// the magic and its terminator. Full-buffer callers pass the buffer length, which keeps
/// their behavior unchanged.
fn has_bundle_signature_with_length(input: &[u8], magic: &[u8], total_length: u64) -> bool {
    const SOURCE_MINIMUM_STREAM_LENGTH: u64 = 0x20;
    total_length >= SOURCE_MINIMUM_STREAM_LENGTH
        && input.get(..magic.len()) == Some(magic)
        && input.get(magic.len()) == Some(&0)
}

/// Source-compatible subset of `SerializedFileHeader.IsSerializedFileHeader`.
fn is_serialized_file(input: &[u8]) -> bool {
    is_serialized_file_with_length(input, input.len() as u64)
}

fn is_serialized_file_with_length(input: &[u8], total_length: u64) -> bool {
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

    metadata_size >= 13 && file_size >= 16 + 13 && total_length == file_size
}
