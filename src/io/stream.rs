//! Seekable views used to read logical files from byte ranges or split file parts.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// A stream that supports both sequential reads and repositioning.
pub trait ReadSeek: Read + Seek {}

impl<T: Read + Seek> ReadSeek for T {}

/// A bounded logical view over a seekable source stream.
pub struct PartialStream<S> {
    source: S,
    base_offset: u64,
    length: u64,
    position: u64,
}

impl<S: Read + Seek> PartialStream<S> {
    pub fn new(mut source: S, base_offset: u64, length: u64) -> io::Result<Self> {
        let end = base_offset.checked_add(length).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "partial stream range overflows",
            )
        })?;
        let original_position = source.stream_position()?;
        let source_length = source.seek(SeekFrom::End(0))?;
        source.seek(SeekFrom::Start(original_position))?;
        if end > source_length {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "partial stream range {base_offset}..{end} exceeds source length {source_length}"
                ),
            ));
        }

        Ok(Self {
            source,
            base_offset,
            length,
            position: 0,
        })
    }

    pub fn length(&self) -> u64 {
        self.length
    }

    pub fn position(&self) -> u64 {
        self.position
    }

    pub fn into_inner(self) -> S {
        self.source
    }
}

impl<S: Read + Seek> Read for PartialStream<S> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let available = self.length.saturating_sub(self.position);
        let requested = buffer
            .len()
            .min(usize::try_from(available).unwrap_or(usize::MAX));
        if requested == 0 {
            return Ok(0);
        }
        let absolute_position = self.base_offset.checked_add(self.position).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "stream position overflows")
        })?;
        self.source.seek(SeekFrom::Start(absolute_position))?;
        let read = self.source.read(&mut buffer[..requested])?;
        self.position = self.position.checked_add(read as u64).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "partial stream position overflows",
            )
        })?;
        Ok(read)
    }
}

impl<S: Read + Seek> Seek for PartialStream<S> {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let next = seek_target(self.position, self.length, from)?;
        self.position = next;
        Ok(next)
    }
}

/// A logical file composed of ordered, seekable streams such as `.split0`, `.split1`, etc.
pub struct MultiFileStream {
    streams: Vec<Box<dyn ReadSeek + Send>>,
    stream_ends: Vec<u64>,
    length: u64,
    position: u64,
}

const DEFAULT_MAX_PARTS: usize = 1_000_000;

/// Open a regular file or assemble its contiguous numbered `.splitN` parts as one stream.
pub fn open_logical_file(path: impl AsRef<Path>) -> io::Result<Box<dyn ReadSeek + Send>> {
    let path = path.as_ref();
    if let Some(base_path) = split_base_path(path) {
        return open_split_file(&base_path, DEFAULT_MAX_PARTS)
            .map(|stream| Box::new(stream) as Box<dyn ReadSeek + Send>);
    }

    match File::open(path) {
        Ok(file) => Ok(Box::new(file)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            open_split_file(path, DEFAULT_MAX_PARTS)
                .map(|stream| Box::new(stream) as Box<dyn ReadSeek + Send>)
        }
        Err(error) => Err(error),
    }
}

/// Return the logical base path for a `.splitN` part path, if the suffix is recognized.
pub fn logical_file_path(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    split_base_path(path).unwrap_or_else(|| path.to_path_buf())
}

/// Return the logical filename, removing a `.splitN` suffix when present.
pub fn logical_file_name(path: impl AsRef<Path>) -> Option<String> {
    logical_file_path(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
}

fn split_base_path(path: &Path) -> Option<PathBuf> {
    let file_name = path.file_name()?.to_str()?;
    let (base, suffix) = file_name.rsplit_once(".split")?;
    if base.is_empty() || suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(path.with_file_name(base))
}

fn open_split_file(base_path: &Path, max_parts: usize) -> io::Result<MultiFileStream> {
    let directory = base_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let base_name = base_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "split file has no valid UTF-8 filename",
            )
        })?;
    let prefix = format!("{base_name}.split");
    let mut parts = Vec::<(i32, PathBuf)>::new();

    for result in std::fs::read_dir(directory)? {
        let entry = result?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Some(suffix) = name.strip_prefix(&prefix) else {
            continue;
        };
        if !entry.file_type()?.is_file() {
            continue;
        }
        if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("split part name {name:?} has a non-numeric suffix"),
            ));
        }
        let index = suffix.parse::<i32>().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("split part index in {name:?} is outside the supported range"),
            )
        })?;
        if parts.len() >= max_parts {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("split part count exceeds configured limit {max_parts}"),
            ));
        }
        parts.try_reserve(1).map_err(|_| {
            io::Error::new(
                io::ErrorKind::OutOfMemory,
                "could not allocate split part paths",
            )
        })?;
        parts.push((index, entry.path()));
    }

    if parts.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no split parts found for {}", base_path.display()),
        ));
    }
    parts.sort_unstable_by_key(|(index, _)| *index);
    for (expected, (actual, path)) in parts.iter().enumerate() {
        if usize::try_from(*actual) != Ok(expected) {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "split part {} is missing before {}",
                    expected,
                    path.display()
                ),
            ));
        }
    }

    let mut streams: Vec<Box<dyn ReadSeek + Send>> = Vec::new();
    streams.try_reserve_exact(parts.len()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::OutOfMemory,
            "could not allocate split file handles",
        )
    })?;
    for (_, part_path) in parts {
        streams.push(Box::new(File::open(part_path)?));
    }
    MultiFileStream::from_boxed(streams)
}

impl MultiFileStream {
    pub fn new<I, S>(streams: I) -> io::Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: ReadSeek + Send + 'static,
    {
        Self::new_with_max_parts(streams, DEFAULT_MAX_PARTS)
    }

    pub fn new_with_max_parts<I, S>(streams: I, max_parts: usize) -> io::Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: ReadSeek + Send + 'static,
    {
        let mut boxed_streams: Vec<Box<dyn ReadSeek + Send>> = Vec::new();
        for stream in streams {
            if boxed_streams.len() >= max_parts {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("split-stream part count exceeds configured limit {max_parts}"),
                ));
            }
            boxed_streams.try_reserve(1).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::OutOfMemory,
                    "could not allocate split-stream parts",
                )
            })?;
            boxed_streams.push(Box::new(stream));
        }
        Self::from_boxed_with_max_parts(boxed_streams, max_parts)
    }

    pub fn from_boxed(streams: Vec<Box<dyn ReadSeek + Send>>) -> io::Result<Self> {
        Self::from_boxed_with_max_parts(streams, DEFAULT_MAX_PARTS)
    }

    fn from_boxed_with_max_parts(
        streams: Vec<Box<dyn ReadSeek + Send>>,
        max_parts: usize,
    ) -> io::Result<Self> {
        if streams.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a multi-file stream requires at least one part",
            ));
        }
        if streams.len() > max_parts {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("split-stream part count exceeds configured limit {max_parts}"),
            ));
        }

        let mut stream_ends = Vec::new();
        stream_ends.try_reserve_exact(streams.len()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::OutOfMemory,
                "could not allocate split-stream index",
            )
        })?;
        let mut length = 0u64;
        let mut streams = streams;
        for stream in &mut streams {
            let position = stream.stream_position()?;
            let stream_length = stream.seek(SeekFrom::End(0))?;
            stream.seek(SeekFrom::Start(position))?;
            length = length.checked_add(stream_length).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "multi-file length overflows")
            })?;
            stream_ends.push(length);
        }

        Ok(Self {
            streams,
            stream_ends,
            length,
            position: 0,
        })
    }

    pub fn length(&self) -> u64 {
        self.length
    }

    pub fn position(&self) -> u64 {
        self.position
    }
}

impl Read for MultiFileStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() || self.position >= self.length {
            return Ok(0);
        }

        let mut total_read = 0usize;
        while total_read < buffer.len() && self.position < self.length {
            let stream_index = self
                .stream_ends
                .partition_point(|end| *end <= self.position);
            let stream_start = if stream_index == 0 {
                0
            } else {
                self.stream_ends[stream_index - 1]
            };
            let stream_end = self.stream_ends[stream_index];
            let local_position = self.position - stream_start;
            let remaining_in_part = stream_end - self.position;
            let remaining_in_buffer = buffer.len() - total_read;
            let requested =
                remaining_in_buffer.min(usize::try_from(remaining_in_part).unwrap_or(usize::MAX));

            self.streams[stream_index].seek(SeekFrom::Start(local_position))?;
            let read =
                self.streams[stream_index].read(&mut buffer[total_read..total_read + requested])?;
            self.position = self.position.checked_add(read as u64).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "multi-file position overflows")
            })?;
            total_read += read;
            if read < requested {
                break;
            }
        }

        Ok(total_read)
    }
}

impl Seek for MultiFileStream {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let next = seek_target(self.position, self.length, from)?;
        self.position = next;
        Ok(next)
    }
}

fn seek_target(position: u64, length: u64, from: SeekFrom) -> io::Result<u64> {
    let base = match from {
        SeekFrom::Start(value) => return Ok(value),
        SeekFrom::Current(_) => position,
        SeekFrom::End(_) => length,
    };
    let offset = match from {
        SeekFrom::Start(_) => unreachable!(),
        SeekFrom::Current(offset) | SeekFrom::End(offset) => i128::from(offset),
    };
    let target = i128::from(base) + offset;
    u64::try_from(target).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "seek position is negative or exceeds the address space",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::{logical_file_name, logical_file_path, open_logical_file};
    use super::{MultiFileStream, PartialStream};
    use crate::compression::wrappers::compress_gzip;
    use crate::scheme::{FileKind, SchemeReader, SchemeReaderLimits};
    use std::io::{Cursor, Read, Seek, SeekFrom};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    fn create_test_directory(label: &str) -> PathBuf {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let parent = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("stream-tests");
        std::fs::create_dir_all(&parent).unwrap();
        let path = parent.join(format!(
            "{label}-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        path
    }

    fn remove_test_files(directory: &Path, names: &[&str]) {
        for name in names {
            std::fs::remove_file(directory.join(name)).unwrap();
        }
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn partial_stream_reads_and_seeks_only_inside_its_range() {
        let source = Cursor::new(b"0123456789".to_vec());
        let mut partial = PartialStream::new(source, 2, 5).unwrap();
        let mut first = [0; 2];
        assert_eq!(partial.read(&mut first).unwrap(), 2);
        assert_eq!(&first, b"23");
        assert_eq!(partial.seek(SeekFrom::End(-1)).unwrap(), 4);
        let mut last = [0; 2];
        assert_eq!(partial.read(&mut last).unwrap(), 1);
        assert_eq!(&last[..1], b"6");
        assert_eq!(partial.read(&mut last).unwrap(), 0);
    }

    #[test]
    fn partial_stream_rejects_overflow_and_out_of_bounds_ranges() {
        let source = Cursor::new(b"abc".to_vec());
        assert!(PartialStream::new(source, 2, 2).is_err());
        let source = Cursor::new(b"abc".to_vec());
        assert!(PartialStream::new(source, u64::MAX, 1).is_err());
    }

    #[test]
    fn multi_file_stream_reads_across_parts_and_seeks_logically() {
        let parts = [b"abc".as_slice(), b"".as_slice(), b"defg".as_slice()]
            .into_iter()
            .map(|part| Cursor::new(part.to_vec()));
        let mut stream = MultiFileStream::new(parts).unwrap();
        let mut prefix = [0; 5];
        assert_eq!(stream.read(&mut prefix).unwrap(), 5);
        assert_eq!(&prefix, b"abcde");
        assert_eq!(stream.seek(SeekFrom::End(-2)).unwrap(), 5);
        let mut suffix = [0; 3];
        assert_eq!(stream.read(&mut suffix).unwrap(), 2);
        assert_eq!(&suffix[..2], b"fg");
    }

    #[test]
    fn multi_file_stream_rejects_empty_parts_and_negative_seeks() {
        assert!(MultiFileStream::new(std::iter::empty::<Cursor<Vec<u8>>>()).is_err());
        let mut stream = MultiFileStream::new([Cursor::new(b"a".to_vec())]).unwrap();
        assert!(stream.seek(SeekFrom::Current(-1)).is_err());
        assert!(MultiFileStream::new_with_max_parts(
            [Cursor::new(b"a".to_vec()), Cursor::new(b"b".to_vec())],
            1
        )
        .is_err());
    }

    #[test]
    fn scheme_reader_dispatches_a_wrapper_split_across_file_parts() {
        let encoded = compress_gzip(b"split stream payload", 1024).unwrap();
        let split = encoded.len() / 2;
        let parts = [encoded[..split].to_vec(), encoded[split..].to_vec()]
            .into_iter()
            .map(Cursor::new);
        let mut stream = MultiFileStream::new(parts).unwrap();

        let file = SchemeReader::read_stream(&mut stream, SchemeReaderLimits::default()).unwrap();
        assert_eq!(file.kind(), FileKind::Resource);
        assert_eq!(file.bytes(), b"split stream payload");
    }

    #[test]
    fn scheme_reader_opens_numbered_split_paths_in_numeric_order() {
        let directory = create_test_directory("contiguous");
        let base = directory.join("sample.bundle");
        let encoded = compress_gzip(b"path-based split payload", 1024).unwrap();
        let split = encoded.len() / 2;
        std::fs::write(directory.join("sample.bundle.split0"), &encoded[..split]).unwrap();
        std::fs::write(directory.join("sample.bundle.split1"), &encoded[split..]).unwrap();
        let part_path = directory.join("sample.bundle.split1");

        assert_eq!(logical_file_path(&part_path), base);
        assert_eq!(
            logical_file_name(&part_path).as_deref(),
            Some("sample.bundle")
        );
        let file = SchemeReader::read_path(&part_path, SchemeReaderLimits::default()).unwrap();
        assert_eq!(file.kind(), FileKind::Resource);
        assert_eq!(file.bytes(), b"path-based split payload");

        let mut opened = open_logical_file(&base).unwrap();
        let mut joined = Vec::new();
        opened.read_to_end(&mut joined).unwrap();
        assert_eq!(joined, encoded);
        remove_test_files(
            &directory,
            &["sample.bundle.split0", "sample.bundle.split1"],
        );
    }

    #[test]
    fn rejects_numbered_split_paths_with_missing_parts() {
        let directory = create_test_directory("gap");
        let base = directory.join("sample.bundle");
        std::fs::write(directory.join("sample.bundle.split0"), b"first").unwrap();
        std::fs::write(directory.join("sample.bundle.split2"), b"third").unwrap();

        let error = match open_logical_file(&base) {
            Ok(_) => panic!("gapped split parts must be rejected"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        assert!(error.to_string().contains("split part 1 is missing"));
        remove_test_files(
            &directory,
            &["sample.bundle.split0", "sample.bundle.split2"],
        );
    }

    #[test]
    fn scheme_reader_opens_regular_paths_directly() {
        let directory = create_test_directory("regular");
        let path = directory.join("sample.resource");
        std::fs::write(&path, b"regular file payload").unwrap();

        let file = SchemeReader::read_path(&path, SchemeReaderLimits::default()).unwrap();
        assert_eq!(file.kind(), FileKind::Resource);
        assert_eq!(file.bytes(), b"regular file payload");
        remove_test_files(&directory, &["sample.resource"]);
    }
}
