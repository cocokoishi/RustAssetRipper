//! Seekable views used to read logical files from byte ranges or split file parts.

use std::fs::File;
use std::fs::OpenOptions;
use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

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

/// Bounded write-through (`PartialStream.Write`): the source throws when the
/// write would end past the range, otherwise the bytes land at the current
/// logical position. `SetLength` stays unsupported, matching the source
/// `NotSupportedException`.
impl<S: Read + Seek + Write> Write for PartialStream<S> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let end = self
            .position
            .checked_add(buffer.len() as u64)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "partial stream's position is out of range",
                )
            })?;
        if end > self.length {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "partial stream's position is out of range",
            ));
        }
        let absolute_position = self.base_offset.checked_add(self.position).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "stream position overflows")
        })?;
        self.source.seek(SeekFrom::Start(absolute_position))?;
        let written = self.source.write(buffer)?;
        self.position = self.position.checked_add(written as u64).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "partial stream position overflows",
            )
        })?;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.source.flush()
    }
}

/// A read-only file range whose logical position is independent from sibling views.
pub struct FileRangeStream {
    file: Arc<Mutex<File>>,
    base_offset: u64,
    length: u64,
    position: u64,
}

impl FileRangeStream {
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let file = File::open(path)?;
        let length = file.metadata()?.len();
        Self::from_file(file, 0, length)
    }

    pub fn from_file(file: File, base_offset: u64, length: u64) -> io::Result<Self> {
        Self::from_shared_file(Arc::new(Mutex::new(file)), base_offset, length)
    }

    fn from_shared_file(file: Arc<Mutex<File>>, base_offset: u64, length: u64) -> io::Result<Self> {
        let file_length = file
            .lock()
            .map_err(|_| io::Error::other("file range handle mutex was poisoned"))?
            .metadata()?
            .len();
        validate_file_range(file_length, base_offset, length)?;
        Ok(Self {
            file,
            base_offset,
            length,
            position: 0,
        })
    }

    pub fn subrange(&self, offset: u64, length: u64) -> io::Result<Self> {
        validate_file_range(self.length, offset, length)?;
        let base_offset = self.base_offset.checked_add(offset).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "file range offset overflows")
        })?;
        Ok(Self {
            file: Arc::clone(&self.file),
            base_offset,
            length,
            position: 0,
        })
    }

    pub fn length(&self) -> u64 {
        self.length
    }
}

impl Read for FileRangeStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let available = self.length.saturating_sub(self.position);
        let requested = buffer
            .len()
            .min(usize::try_from(available).unwrap_or(usize::MAX));
        if requested == 0 {
            return Ok(0);
        }
        let absolute_position = self.base_offset.checked_add(self.position).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "file range position overflows")
        })?;
        let mut file = self
            .file
            .lock()
            .map_err(|_| io::Error::other("file range handle mutex was poisoned"))?;
        file.seek(SeekFrom::Start(absolute_position))?;
        let read = file.read(&mut buffer[..requested])?;
        self.position = self.position.checked_add(read as u64).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "file range position overflows")
        })?;
        Ok(read)
    }
}

impl Seek for FileRangeStream {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let next = seek_target(self.position, self.length, from)?;
        self.position = next;
        Ok(next)
    }
}

fn validate_file_range(file_length: u64, offset: u64, length: u64) -> io::Result<()> {
    let end = offset
        .checked_add(length)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "file range overflows"))?;
    if end > file_length {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("file range {offset}..{end} exceeds file length {file_length}"),
        ));
    }
    Ok(())
}

const DEFAULT_MAX_PARTIAL_COPY_BYTES: usize = 512 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SmartStreamType {
    Null,
    Memory,
    File,
}

#[derive(Clone)]
pub struct SmartStream {
    inner: Option<Arc<SmartStreamShared>>,
    stream_type: SmartStreamType,
}

struct SmartStreamShared {
    backend: Mutex<SmartStreamBackend>,
    can_read: bool,
    can_write: bool,
}

enum SmartStreamBackend {
    Memory(Cursor<Vec<u8>>),
    File(Arc<Mutex<File>>),
    Reader(Box<dyn ReadSeek + Send>),
    Range(FileRangeStream),
    Temporary(TemporaryFile),
}

struct TemporaryFile {
    file: Option<File>,
    path: PathBuf,
}

impl TemporaryFile {
    fn file_mut(&mut self) -> io::Result<&mut File> {
        self.file
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "temporary file is closed"))
    }
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        drop(self.file.take());
        let _ = std::fs::remove_file(&self.path);
    }
}

impl Read for SmartStreamBackend {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Memory(cursor) => cursor.read(buffer),
            Self::File(file) => file
                .lock()
                .map_err(|_| io::Error::other("smart file mutex was poisoned"))?
                .read(buffer),
            Self::Reader(reader) => reader.read(buffer),
            Self::Range(range) => range.read(buffer),
            Self::Temporary(file) => file.file_mut()?.read(buffer),
        }
    }
}

impl Write for SmartStreamBackend {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        match self {
            Self::Memory(cursor) => cursor.write(buffer),
            Self::File(file) => file
                .lock()
                .map_err(|_| io::Error::other("smart file mutex was poisoned"))?
                .write(buffer),
            Self::Reader(_) => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "reader-backed stream is read-only",
            )),
            Self::Range(_) => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "file range stream is read-only",
            )),
            Self::Temporary(file) => file.file_mut()?.write(buffer),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Memory(cursor) => cursor.flush(),
            Self::File(file) => file
                .lock()
                .map_err(|_| io::Error::other("smart file mutex was poisoned"))?
                .flush(),
            Self::Reader(_) => Ok(()),
            Self::Range(_) => Ok(()),
            Self::Temporary(file) => file.file_mut()?.flush(),
        }
    }
}

impl Seek for SmartStreamBackend {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        match self {
            Self::Memory(cursor) => cursor.seek(from),
            Self::File(file) => file
                .lock()
                .map_err(|_| io::Error::other("smart file mutex was poisoned"))?
                .seek(from),
            Self::Reader(reader) => reader.seek(from),
            Self::Range(range) => range.seek(from),
            Self::Temporary(file) => file.file_mut()?.seek(from),
        }
    }
}

impl SmartStream {
    pub fn create_null() -> Self {
        Self {
            inner: None,
            stream_type: SmartStreamType::Null,
        }
    }

    pub fn create_memory() -> Self {
        Self::new(
            SmartStreamBackend::Memory(Cursor::new(Vec::new())),
            SmartStreamType::Memory,
            true,
            true,
        )
    }

    pub fn from_memory(bytes: Vec<u8>) -> Self {
        Self::new(
            SmartStreamBackend::Memory(Cursor::new(bytes)),
            SmartStreamType::Memory,
            true,
            false,
        )
    }

    pub fn from_memory_range(
        bytes: &[u8],
        offset: u64,
        length: u64,
        writable: bool,
    ) -> io::Result<Self> {
        let source_length = u64::try_from(bytes.len()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "memory source length is too large",
            )
        })?;
        validate_file_range(source_length, offset, length)?;
        let offset = usize::try_from(offset).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "memory range offset is too large",
            )
        })?;
        let length = usize::try_from(length).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "memory range length is too large",
            )
        })?;
        let mut range = Vec::new();
        range.try_reserve_exact(length).map_err(|_| {
            io::Error::new(
                io::ErrorKind::OutOfMemory,
                "could not allocate memory range",
            )
        })?;
        range.extend_from_slice(&bytes[offset..offset + length]);
        Ok(Self::new(
            SmartStreamBackend::Memory(Cursor::new(range)),
            SmartStreamType::Memory,
            true,
            writable,
        ))
    }

    pub fn open_read(path: impl AsRef<Path>) -> io::Result<Self> {
        Ok(Self::new(
            SmartStreamBackend::File(Arc::new(Mutex::new(File::open(path)?))),
            SmartStreamType::File,
            true,
            false,
        ))
    }

    /// Wraps an owned seekable reader as a shared, read-only file stream.
    ///
    /// The reader is positioned at byte zero before it is retained.
    pub fn from_reader(mut reader: Box<dyn ReadSeek + Send>) -> io::Result<Self> {
        reader.seek(SeekFrom::Start(0))?;
        reader.seek(SeekFrom::End(0))?;
        reader.seek(SeekFrom::Start(0))?;
        Ok(Self::new(
            SmartStreamBackend::Reader(reader),
            SmartStreamType::File,
            true,
            false,
        ))
    }

    pub fn create_temp() -> io::Result<Self> {
        static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir();
        for _ in 0..128 {
            let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
            let path = directory.join(format!("assetripper-{}-{id}.tmp", std::process::id()));
            match OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(file) => {
                    return Ok(Self::new(
                        SmartStreamBackend::Temporary(TemporaryFile {
                            file: Some(file),
                            path,
                        }),
                        SmartStreamType::File,
                        true,
                        true,
                    ));
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not create a unique temporary stream file",
        ))
    }

    fn new(
        backend: SmartStreamBackend,
        stream_type: SmartStreamType,
        can_read: bool,
        can_write: bool,
    ) -> Self {
        Self {
            inner: Some(Arc::new(SmartStreamShared {
                backend: Mutex::new(backend),
                can_read,
                can_write,
            })),
            stream_type,
        }
    }

    pub fn create_reference(&self) -> Self {
        self.clone()
    }

    pub fn assign(&mut self, source: &Self) {
        self.free_reference();
        self.inner = source.inner.as_ref().map(Arc::clone);
        self.stream_type = source.stream_type;
    }

    pub fn move_from(&mut self, source: &mut Self) {
        self.free_reference();
        self.inner = source.inner.take();
        self.stream_type = std::mem::replace(&mut source.stream_type, SmartStreamType::Null);
    }

    pub fn free_reference(&mut self) {
        self.inner = None;
        self.stream_type = SmartStreamType::Null;
    }

    pub fn is_null(&self) -> bool {
        self.inner.is_none()
    }

    pub fn ref_count(&self) -> usize {
        self.inner.as_ref().map_or(0, Arc::strong_count)
    }

    pub fn stream_type(&self) -> SmartStreamType {
        self.stream_type
    }

    pub fn can_read(&self) -> bool {
        self.inner.as_ref().is_some_and(|inner| inner.can_read)
    }

    pub fn can_write(&self) -> bool {
        self.inner.as_ref().is_some_and(|inner| inner.can_write)
    }

    pub fn can_seek(&self) -> bool {
        !self.is_null()
    }

    pub fn length(&self) -> io::Result<u64> {
        let Some(_) = self.inner else {
            return Ok(0);
        };
        let mut backend = self.lock_backend()?;
        let position = backend.seek(SeekFrom::Current(0))?;
        let length = backend.seek(SeekFrom::End(0));
        let restore = backend.seek(SeekFrom::Start(position));
        match (length, restore) {
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Ok(length), Ok(_)) => Ok(length),
        }
    }

    pub fn position(&self) -> io::Result<u64> {
        match self.inner {
            None => Ok(0),
            Some(_) => self.lock_backend()?.seek(SeekFrom::Current(0)),
        }
    }

    /// Reads exactly into `buffer` from an absolute byte offset without changing the shared
    /// stream position. The seek and read are performed under one backend lock.
    pub fn read_exact_at(&self, offset: u64, buffer: &mut [u8]) -> io::Result<()> {
        self.ensure_non_null()?;
        if !self.can_read() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "stream is not readable",
            ));
        }
        let buffer_length = u64::try_from(buffer.len()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "read length does not fit in u64",
            )
        })?;
        offset.checked_add(buffer_length).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "absolute read range overflows")
        })?;

        let mut backend = self.lock_backend()?;
        let original_position = backend.seek(SeekFrom::Current(0))?;
        let result = (|| {
            backend.seek(SeekFrom::Start(offset))?;
            backend.read_exact(buffer)
        })();
        let restore = backend.seek(SeekFrom::Start(original_position));
        match (result, restore) {
            (Err(error), _) => Err(error),
            (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(_)) => Ok(()),
        }
    }

    pub fn set_len(&self, length: u64) -> io::Result<()> {
        let mut backend = self.lock_backend()?;
        if !self.can_write() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "stream is not writable",
            ));
        }
        match &mut *backend {
            SmartStreamBackend::Memory(cursor) => {
                let length = usize::try_from(length).map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "memory stream length cannot fit in address space",
                    )
                })?;
                let bytes = cursor.get_mut();
                if length > bytes.len() {
                    bytes.try_reserve(length - bytes.len()).map_err(|_| {
                        io::Error::new(io::ErrorKind::OutOfMemory, "could not grow memory stream")
                    })?;
                }
                bytes.resize(length, 0);
                Ok(())
            }
            SmartStreamBackend::File(file) => file
                .lock()
                .map_err(|_| io::Error::other("smart file mutex was poisoned"))?
                .set_len(length),
            SmartStreamBackend::Reader(_) => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "reader-backed stream is read-only",
            )),
            SmartStreamBackend::Range(_) => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "file range stream is read-only",
            )),
            SmartStreamBackend::Temporary(file) => file.file_mut()?.set_len(length),
        }
    }

    pub fn to_vec(&self, maximum_bytes: usize) -> io::Result<Vec<u8>> {
        self.ensure_non_null()?;
        if !self.can_read() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "stream is not readable",
            ));
        }
        let mut backend = self.lock_backend()?;
        let original_position = backend.seek(SeekFrom::Current(0))?;
        let result = (|| {
            let length = backend.seek(SeekFrom::End(0))?;
            if length > u64::try_from(maximum_bytes).unwrap_or(u64::MAX) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("stream length {length} exceeds copy limit {maximum_bytes}"),
                ));
            }
            let length = usize::try_from(length).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidData, "stream length is too large")
            })?;
            backend.seek(SeekFrom::Start(0))?;
            let mut output = Vec::new();
            output.try_reserve_exact(length).map_err(|_| {
                io::Error::new(io::ErrorKind::OutOfMemory, "could not allocate stream copy")
            })?;
            output.resize(length, 0);
            backend.read_exact(&mut output)?;
            Ok(output)
        })();
        let restore = backend.seek(SeekFrom::Start(original_position));
        match (result, restore) {
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
            (Ok(output), Ok(_)) => Ok(output),
        }
    }

    pub fn create_partial(&self, offset: u64, length: u64) -> io::Result<Self> {
        self.create_partial_with_limit(offset, length, DEFAULT_MAX_PARTIAL_COPY_BYTES)
    }

    pub fn create_partial_with_limit(
        &self,
        offset: u64,
        length: u64,
        maximum_copy_bytes: usize,
    ) -> io::Result<Self> {
        self.ensure_non_null()?;
        if !self.can_read() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "stream is not readable",
            ));
        }
        let mut backend = self.lock_backend()?;
        if let SmartStreamBackend::File(file) = &*backend {
            let range = FileRangeStream::from_shared_file(Arc::clone(file), offset, length)?;
            return Ok(Self::new(
                SmartStreamBackend::Range(range),
                SmartStreamType::File,
                true,
                false,
            ));
        }
        if let SmartStreamBackend::Range(range) = &*backend {
            let partial = range.subrange(offset, length)?;
            return Ok(Self::new(
                SmartStreamBackend::Range(partial),
                SmartStreamType::File,
                true,
                false,
            ));
        }
        if length > u64::try_from(maximum_copy_bytes).unwrap_or(u64::MAX) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("partial stream length {length} exceeds copy limit {maximum_copy_bytes}"),
            ));
        }
        let length_usize = usize::try_from(length).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidData, "partial stream is too large")
        })?;
        let original_position = backend.seek(SeekFrom::Current(0))?;
        let result = (|| {
            let source_length = backend.seek(SeekFrom::End(0))?;
            validate_file_range(source_length, offset, length)?;
            backend.seek(SeekFrom::Start(offset))?;
            let mut output = Vec::new();
            output.try_reserve_exact(length_usize).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::OutOfMemory,
                    "could not allocate partial stream",
                )
            })?;
            output.resize(length_usize, 0);
            backend.read_exact(&mut output)?;
            Ok(output)
        })();
        let restore = backend.seek(SeekFrom::Start(original_position));
        let bytes = match (result, restore) {
            (Err(error), _) => return Err(error),
            (Ok(_), Err(error)) => return Err(error),
            (Ok(bytes), Ok(_)) => bytes,
        };
        Ok(Self::from_memory(bytes))
    }

    fn lock_backend(&self) -> io::Result<std::sync::MutexGuard<'_, SmartStreamBackend>> {
        let inner = self
            .inner
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "null smart stream"))?;
        inner
            .backend
            .lock()
            .map_err(|_| io::Error::other("smart stream mutex was poisoned"))
    }

    fn ensure_non_null(&self) -> io::Result<()> {
        if self.is_null() {
            Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "null smart stream",
            ))
        } else {
            Ok(())
        }
    }
}

impl Default for SmartStream {
    fn default() -> Self {
        Self::create_null()
    }
}

impl Read for SmartStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.ensure_non_null()?;
        if !self.can_read() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "stream is not readable",
            ));
        }
        self.lock_backend()?.read(buffer)
    }
}

impl Write for SmartStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.ensure_non_null()?;
        if !self.can_write() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "stream is not writable",
            ));
        }
        self.lock_backend()?.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.inner {
            None => Ok(()),
            Some(_) => self.lock_backend()?.flush(),
        }
    }
}

impl Seek for SmartStream {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        self.lock_backend()?.seek(from)
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

/// `MultiFileStream` part-name decision: an entry belongs to `base_name` only as
/// exactly `{base}.split<digits>`, returning the numeric index (`IsMultiFile`
/// plus the `SplitNameComparer` index rule).
///
/// One deliberate difference is documented, not changed: the source lists
/// directory entries with the loose `*.split*` glob and parses trailing digits
/// anywhere after `.split`, so `data.split+5` sorts as part 5 and its bytes are
/// incorporated. This port fails closed on such decoy names instead, because
/// real split parts are always strictly numeric and the alternative is silent
/// bundle corruption. Overflowing indices fail the same way the source
/// `int.Parse` throws. Called by `open_split_file`; non-matching names return
/// `None` so unrelated files stay ignored.
fn split_part_index(base_name: &str, entry_name: &str) -> Option<io::Result<i32>> {
    let suffix = entry_name.strip_prefix(base_name)?.strip_prefix(".split")?;
    if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        return Some(Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("split part name {entry_name:?} has a non-numeric suffix"),
        )));
    }
    let index = suffix.parse::<i32>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("split part index in {entry_name:?} is outside the supported range"),
        )
    });
    Some(index)
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
    let mut parts = Vec::<(i32, PathBuf)>::new();

    for result in std::fs::read_dir(directory)? {
        let entry = result?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let index = match split_part_index(base_name, &name) {
            None => continue,
            Some(result) => result?,
        };
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("split part {name:?} is a symbolic link"),
            ));
        }
        if !file_type.is_file() {
            continue;
        }
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
    use super::{
        logical_file_name, logical_file_path, open_logical_file, split_part_index, FileRangeStream,
        MultiFileStream, PartialStream, SmartStream, SmartStreamBackend, SmartStreamType,
    };
    use crate::compression::wrappers::compress_gzip;
    use crate::scheme::{FileKind, SchemeReader, SchemeReaderLimits};
    use std::io::{Cursor, Read, Seek, SeekFrom, Write};
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

    /// Bounded write-through (`PartialStream.Write`): bytes land at the logical
    /// position and advance it, while a write ending past the range fails with
    /// the source untouched.
    #[test]
    fn partial_stream_writes_inside_its_range_only() {
        let source = Cursor::new(b"0123456789".to_vec());
        let mut partial = PartialStream::new(source, 2, 5).unwrap();
        assert_eq!(partial.seek(SeekFrom::Start(1)).unwrap(), 1);
        assert_eq!(partial.write(b"abc").unwrap(), 3);
        assert_eq!(partial.position(), 4);
        // A write ending exactly at the range end is accepted.
        assert_eq!(partial.write(b"d").unwrap(), 1);
        assert_eq!(partial.position(), 5);
        // Past-the-end writes fail and leave the source untouched.
        assert_eq!(partial.position(), partial.length());
        assert!(partial.write(b"e").is_err());
        assert_eq!(partial.position(), 5);
        let source = partial.into_inner().into_inner();
        assert_eq!(source, b"012abcd789");
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
    fn smart_stream_wraps_a_split_reader_as_shared_read_only_file_storage() {
        let parts = [b"raw".as_slice(), b" bundle".as_slice()]
            .into_iter()
            .map(|part| Cursor::new(part.to_vec()));
        let reader = MultiFileStream::new(parts).unwrap();
        let mut stream = SmartStream::from_reader(Box::new(reader)).unwrap();
        let reference = stream.create_reference();

        assert_eq!(stream.length().unwrap(), 10);
        assert!(!stream.can_write());
        let mut prefix = [0; 4];
        assert_eq!(stream.read(&mut prefix).unwrap(), 4);
        assert_eq!(&prefix, b"raw ");
        assert_eq!(reference.position().unwrap(), 4);
        assert_eq!(stream.to_vec(10).unwrap(), b"raw bundle");
        assert_eq!(stream.position().unwrap(), 4);
        assert!(stream.write_all(b"x").is_err());
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

    /// The part-name decision: strict `{base}.split<digits>` shapes resolve to
    /// their index, unrelated names stay ignored, and decoy suffixes fail
    /// instead of sorting as a part the way the source trailing-digit parse would.
    #[test]
    fn split_part_names_resolve_strictly_numeric_indices() {
        assert_eq!(split_part_index("data", "data.split0").unwrap().unwrap(), 0);
        assert_eq!(
            split_part_index("data", "data.split007").unwrap().unwrap(),
            7
        );
        assert!(split_part_index("data", "other.split0").is_none());
        assert!(split_part_index("data", "data.split").unwrap().is_err());
        assert!(split_part_index("data", "data.split+1").unwrap().is_err());
        assert!(split_part_index("data", "data.split 1").unwrap().is_err());
        assert!(split_part_index("data", "data.split99999999999999999999")
            .unwrap()
            .is_err());
    }

    #[test]
    fn split_assembly_rejects_decoy_part_names() {
        let directory = create_test_directory("decoy");
        let base = directory.join("sample.bundle");
        std::fs::write(directory.join("sample.bundle.split0"), b"first").unwrap();
        std::fs::write(directory.join("sample.bundle.split1"), b"second").unwrap();
        std::fs::write(directory.join("sample.bundle.split+1"), b"decoy").unwrap();

        let error = match open_logical_file(&base) {
            Ok(_) => panic!("decoy split part names must be rejected"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("non-numeric suffix"));
        remove_test_files(
            &directory,
            &[
                "sample.bundle.split0",
                "sample.bundle.split1",
                "sample.bundle.split+1",
            ],
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

    #[test]
    fn file_range_streams_share_a_handle_but_keep_independent_positions() {
        let directory = create_test_directory("file-range");
        let path = directory.join("ranges.bin");
        std::fs::write(&path, b"abcdef").unwrap();
        {
            let mut whole = FileRangeStream::open(&path).unwrap();
            let mut view = whole.subrange(1, 4).unwrap();
            let mut nested = whole.subrange(2, 2).unwrap();

            let mut left = [0; 2];
            assert_eq!(view.read(&mut left).unwrap(), 2);
            assert_eq!(&left, b"bc");
            let mut middle = [0; 2];
            assert_eq!(nested.read(&mut middle).unwrap(), 2);
            assert_eq!(&middle, b"cd");
            assert_eq!(view.seek(SeekFrom::End(-1)).unwrap(), 3);
            let mut last = [0; 1];
            assert_eq!(view.read(&mut last).unwrap(), 1);
            assert_eq!(&last, b"e");
            let mut prefix = [0; 2];
            assert_eq!(whole.read(&mut prefix).unwrap(), 2);
            assert_eq!(&prefix, b"ab");
        }
        remove_test_files(&directory, &["ranges.bin"]);
    }

    #[test]
    fn file_range_stream_rejects_overflow_and_out_of_bounds_ranges() {
        let directory = create_test_directory("file-range-bounds");
        let path = directory.join("ranges.bin");
        std::fs::write(&path, b"abc").unwrap();
        let file = std::fs::File::open(&path).unwrap();
        assert!(FileRangeStream::from_file(file, u64::MAX, 1).is_err());
        let file = std::fs::File::open(&path).unwrap();
        assert!(FileRangeStream::from_file(file, 2, 2).is_err());
        remove_test_files(&directory, &["ranges.bin"]);
    }

    #[test]
    fn smart_stream_null_and_memory_modes_report_capabilities() {
        let mut null = SmartStream::create_null();
        assert!(null.is_null());
        assert_eq!(null.ref_count(), 0);
        assert_eq!(null.stream_type(), SmartStreamType::Null);
        assert!(!null.can_read());
        assert!(!null.can_write());
        assert!(!null.can_seek());
        assert_eq!(null.length().unwrap(), 0);
        assert_eq!(null.position().unwrap(), 0);
        null.flush().unwrap();
        assert_eq!(
            null.read(&mut [0]).unwrap_err().kind(),
            std::io::ErrorKind::NotConnected
        );

        let memory = SmartStream::from_memory(b"data".to_vec());
        assert!(!memory.is_null());
        assert_eq!(memory.stream_type(), SmartStreamType::Memory);
        assert!(memory.can_read());
        assert!(!memory.can_write());
        assert!(memory.can_seek());
        assert_eq!(memory.length().unwrap(), 4);
        assert!(SmartStream::create_memory().can_write());
        let range = SmartStream::from_memory_range(b"abcdef", 2, 2, true).unwrap();
        assert!(range.can_write());
        assert_eq!(range.to_vec(2).unwrap(), b"cd");
    }

    /// The null-stream member matrix against the reference `SmartStreamTests`
    /// (`DisposedStreamThrowsForManyMembers`, `FreedStreamsMustBeNull`): total
    /// members stay total on null streams, every I/O member fails instead of
    /// touching a backend, and a freed stream rejoins the same matrix.
    #[test]
    fn null_smart_stream_matrix_matches_the_disposed_contract() {
        let mut null = SmartStream::create_null();
        null.flush().unwrap();
        assert!(!null.can_read());
        assert!(!null.can_write());
        assert!(!null.can_seek());
        assert_eq!(null.position().unwrap(), 0);
        assert_eq!(null.length().unwrap(), 0);
        assert_eq!(null.ref_count(), 0);
        assert!(null.is_null());
        assert_eq!(null.stream_type(), SmartStreamType::Null);
        assert_eq!(
            null.read(&mut [0]).unwrap_err().kind(),
            std::io::ErrorKind::NotConnected
        );
        assert_eq!(
            null.write(&[0]).unwrap_err().kind(),
            std::io::ErrorKind::NotConnected
        );
        assert_eq!(
            null.seek(SeekFrom::Start(0)).unwrap_err().kind(),
            std::io::ErrorKind::NotConnected
        );
        assert_eq!(
            null.set_len(2).unwrap_err().kind(),
            std::io::ErrorKind::NotConnected
        );
        assert_eq!(
            null.to_vec(8).unwrap_err().kind(),
            std::io::ErrorKind::NotConnected
        );
        assert!(matches!(
            null.create_partial(0, 0),
            Err(error) if error.kind() == std::io::ErrorKind::NotConnected
        ));
        assert_eq!(
            null.read_exact_at(0, &mut [0; 1]).unwrap_err().kind(),
            std::io::ErrorKind::NotConnected
        );

        let mut stream = SmartStream::from_memory(b"data".to_vec());
        stream.free_reference();
        assert_eq!(stream.ref_count(), 0);
        assert!(stream.is_null());
        assert_eq!(stream.stream_type(), SmartStreamType::Null);
        assert_eq!(stream.length().unwrap(), 0);
        assert_eq!(
            stream.to_vec(4).unwrap_err().kind(),
            std::io::ErrorKind::NotConnected
        );
    }

    #[test]
    fn smart_stream_references_share_position_and_release_ownership() {
        let mut first = SmartStream::from_memory(b"abcd".to_vec());
        let mut second = first.create_reference();
        assert_eq!(first.ref_count(), 2);
        let mut byte = [0; 1];
        assert_eq!(first.read(&mut byte).unwrap(), 1);
        assert_eq!(&byte, b"a");
        assert_eq!(second.read(&mut byte).unwrap(), 1);
        assert_eq!(&byte, b"b");

        let mut moved = SmartStream::create_null();
        moved.move_from(&mut second);
        assert!(second.is_null());
        assert_eq!(moved.ref_count(), 2);
        moved.free_reference();
        assert_eq!(moved.stream_type(), SmartStreamType::Null);
        assert_eq!(first.ref_count(), 1);
    }

    #[test]
    fn smart_stream_copy_and_partial_operations_preserve_source_position() {
        let mut source = SmartStream::create_memory();
        source.write_all(b"abcdef").unwrap();
        source.seek(SeekFrom::Start(4)).unwrap();
        assert_eq!(source.to_vec(6).unwrap(), b"abcdef");
        assert_eq!(source.position().unwrap(), 4);

        let partial = source.create_partial_with_limit(1, 3, 3).unwrap();
        assert_eq!(source.position().unwrap(), 4);
        assert_eq!(partial.to_vec(3).unwrap(), b"bcd");
        assert!(source.create_partial_with_limit(0, 4, 3).is_err());
        assert!(source.create_partial_with_limit(5, 2, 2).is_err());

        source.set_len(3).unwrap();
        assert_eq!(source.to_vec(3).unwrap(), b"abc");
        assert_eq!(source.position().unwrap(), 4);
    }

    #[test]
    fn smart_stream_read_exact_at_preserves_position_on_success_and_failure() {
        let mut stream = SmartStream::from_memory(b"abcdef".to_vec());
        stream.seek(SeekFrom::Start(4)).unwrap();

        let mut bytes = [0; 2];
        stream.read_exact_at(1, &mut bytes).unwrap();
        assert_eq!(&bytes, b"bc");
        assert_eq!(stream.position().unwrap(), 4);

        let mut out_of_bounds = [0; 3];
        assert!(stream.read_exact_at(5, &mut out_of_bounds).is_err());
        assert_eq!(stream.position().unwrap(), 4);

        assert_eq!(
            stream
                .read_exact_at(u64::MAX, &mut [0; 1])
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidInput
        );
        assert_eq!(stream.position().unwrap(), 4);
    }

    #[test]
    fn smart_stream_file_partial_keeps_its_shared_handle_after_parent_drop() {
        let directory = create_test_directory("smart-file-partial");
        let path = directory.join("input.bin");
        std::fs::write(&path, b"abcdef").unwrap();
        let source = SmartStream::open_read(&path).unwrap();
        let partial = source.create_partial_with_limit(1, 4, 0).unwrap();
        assert_eq!(partial.stream_type(), SmartStreamType::File);
        assert!(!partial.can_write());
        let nested = partial.create_partial_with_limit(1, 2, 0).unwrap();
        assert_eq!(nested.stream_type(), SmartStreamType::File);
        assert_eq!(nested.to_vec(2).unwrap(), b"cd");
        drop(source);

        assert_eq!(partial.to_vec(4).unwrap(), b"bcde");
        drop(partial);
        remove_test_files(&directory, &["input.bin"]);
    }

    #[test]
    fn temporary_smart_stream_is_removed_after_its_last_reference() {
        let mut owner = SmartStream::create_temp().unwrap();
        let path = {
            let inner = owner.inner.as_ref().unwrap();
            let backend = inner.backend.lock().unwrap();
            let SmartStreamBackend::Temporary(file) = &*backend else {
                panic!("temporary constructor returned a different backend");
            };
            file.path.clone()
        };
        owner.write_all(b"temporary bytes").unwrap();
        owner.seek(SeekFrom::Start(0)).unwrap();
        let mut reference = owner.create_reference();
        drop(owner);
        assert!(path.exists());
        let mut bytes = [0; 9];
        assert_eq!(reference.read(&mut bytes).unwrap(), 9);
        assert_eq!(&bytes, b"temporary");
        drop(reference);
        assert!(!path.exists());
    }
}
