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
        let file = open_os_read(path.as_ref())?;
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
        let file = self
            .file
            .lock()
            .map_err(|_| io::Error::other("file range handle mutex was poisoned"))?;
        // The baseline `RandomAccessStream` reads through `RandomAccess.Read(handle, buffer,
        // position)`, so a range view never disturbs the parent handle's cursor. Keep the shared
        // handle locked while using the platform positioned read; the Windows standard-library
        // implementation may update the file cursor, so read_file_at restores it before return.
        let read = read_file_at(&file, &mut buffer[..requested], absolute_position)?;
        self.position = self.position.checked_add(read as u64).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "file range position overflows")
        })?;
        Ok(read)
    }
}

/// Read at an absolute offset using the platform positional file API.
///
/// Mirrors the baseline `RandomAccessStream`'s `RandomAccess.Read(Handle, buffer, position)`,
/// which never uses or changes the parent stream position.
#[cfg(windows)]
fn read_file_at(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
    use std::os::windows::fs::FileExt;
    let mut file = file;
    let original_position = file.stream_position()?;
    let read = file.seek_read(buffer, offset);
    let restore = file.seek(SeekFrom::Start(original_position));
    match (read, restore) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Ok(count), Ok(_)) => Ok(count),
    }
}

#[cfg(unix)]
fn read_file_at(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
    use std::os::unix::fs::FileExt;
    file.read_at(buffer, offset)
}

#[cfg(not(any(unix, windows)))]
fn read_file_at(file: &File, buffer: &mut [u8], offset: u64) -> io::Result<usize> {
    let mut file = file;
    let original_position = file.stream_position()?;
    let read = file
        .seek(SeekFrom::Start(offset))
        .and_then(|_| file.read(buffer));
    let restore = file.seek(SeekFrom::Start(original_position));
    match (read, restore) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Ok(count), Ok(_)) => Ok(count),
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
            SmartStreamBackend::File(Arc::new(Mutex::new(open_os_read(path.as_ref())?))),
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
        Self::create_temp_in(&std::env::temp_dir())
    }

    /// Create a temporary smart stream in `directory`.
    ///
    /// The reference implementation only offers `CreateTemp()` against
    /// `Path.GetTempPath()`; the directory-parameterized form exists so callers (and tests)
    /// that already own a writable scratch directory do not depend on the process-wide
    /// temporary directory being writable.
    pub fn create_temp_in(directory: &Path) -> io::Result<Self> {
        static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(0);
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

    /// `Stream.SetLength`: resize the backing store, zero-filling growth like the source.
    ///
    /// Memory streams clamp a past-the-end position down to the new length
    /// (`MemoryStream.SetLength`); file and temporary streams preserve the position
    /// (`FileStream.SetLength` leaves the cursor, which may then sit past the end).
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
                cursor.set_position(cursor.position().min(length as u64));
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
///
/// The part cursor mirrors the baseline `MultiFileStream` state machine (`m_currentBegin`,
/// `m_currentEnd`, `m_streamIndex`, `NextStream`, `UpdateCurrentStream`) so that a single
/// `Read` call returns only what the current part can supply, exactly like the source.
pub struct MultiFileStream {
    streams: Vec<Box<dyn ReadSeek + Send>>,
    stream_ends: Vec<u64>,
    length: u64,
    position: u64,
    index: usize,
    current_begin: u64,
    current_end: u64,
}

const DEFAULT_MAX_PARTS: usize = 1_000_000;

/// Open a regular file or assemble its contiguous numbered `.splitN` parts as one stream.
pub fn open_logical_file(path: impl AsRef<Path>) -> io::Result<Box<dyn ReadSeek + Send>> {
    let path = path.as_ref();
    if let Some(base_path) = split_base_path(path) {
        return open_split_file(&base_path, DEFAULT_MAX_PARTS)
            .map(|stream| Box::new(stream) as Box<dyn ReadSeek + Send>);
    }

    match open_os_read(path) {
        Ok(file) => Ok(Box::new(file)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            open_split_file(path, DEFAULT_MAX_PARTS)
                .map(|stream| Box::new(stream) as Box<dyn ReadSeek + Send>)
        }
        Err(error) => Err(error),
    }
}

/// Open a filesystem input using the reference `File.OpenRead` share contract.
///
/// On Windows, AssetRipper shares read access only while the stream is open. On Unix,
/// .NET uses an advisory shared `flock` for `FileShare.Read`; mirror that lock while
/// accepting that Unix writers using a compatible shared lock and unlink operations
/// are not blocked. Unix targets without Rustix `flock` support use their native
/// read-only open semantics, as do other non-Windows platforms.
fn open_os_read(path: &Path) -> io::Result<File> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;

        const FILE_SHARE_READ: u32 = 0x0000_0001;
        OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(path)
    }
    #[cfg(unix)]
    {
        let file = File::open(path)?;
        #[cfg(not(any(
            target_os = "solaris",
            target_os = "espidf",
            target_os = "horizon",
            target_os = "vita"
        )))]
        {
            return match rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockShared)
            {
                Ok(()) => Ok(file),
                Err(error) if error == rustix::io::Errno::WOULDBLOCK => Err(error.into()),
                Err(_) => {
                    // .NET treats unsupported advisory locking as best-effort and still opens
                    // the file; only a conflicting lock rejects the read handle.
                    Ok(file)
                }
            };
        }
        #[cfg(any(
            target_os = "solaris",
            target_os = "espidf",
            target_os = "horizon",
            target_os = "vita"
        ))]
        {
            Ok(file)
        }
    }
    #[cfg(not(any(windows, unix)))]
    {
        File::open(path)
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
/// exactly `{base}.split<canonical decimal index>`. The source enumerates a loose
/// `*.split*` glob, but before sorting it requires the exact names
/// `{base}.split0` through `{base}.split{count - 1}`. A source-runtime probe confirms
/// that padded names such as `.split00` are rejected when `.split0` is missing.
/// Rust rejects noncanonical numeric spellings and nonnumeric decoys explicitly,
/// before they can change the assembled byte stream. Overflowing indices fail as
/// the source's `int.Parse` does. Called by `open_split_file`; unrelated names
/// return `None` so they stay ignored.
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
    Some(index.and_then(|index| {
        if index.to_string() == suffix {
            Ok(index)
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("split part index in {entry_name:?} is not canonical"),
            ))
        }
    }))
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
    let mut candidates = Vec::<(String, PathBuf)>::new();

    for result in std::fs::read_dir(directory)? {
        let entry = result?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if !name
            .strip_prefix(base_name)
            .is_some_and(|suffix| suffix.starts_with(".split"))
        {
            continue;
        }
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
        if candidates.len() >= max_parts {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("split part count exceeds configured limit {max_parts}"),
            ));
        }
        candidates.try_reserve(1).map_err(|_| {
            io::Error::new(
                io::ErrorKind::OutOfMemory,
                "could not allocate split part paths",
            )
        })?;
        candidates.push((name, entry.path()));
    }

    if candidates.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no split parts found for {}", base_path.display()),
        ));
    }

    // The reference first enumerates `base.split*`, then requires exact canonical
    // names for every index below the candidate count before it sorts or opens any
    // stream. This rejects padded aliases (`split00`) and decoys as missing the
    // corresponding canonical part instead of parsing them as numeric indexes.
    candidates.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    for expected in 0..candidates.len() {
        let expected_name = format!("{base_name}.split{expected}");
        if candidates
            .binary_search_by(|(name, _)| name.as_str().cmp(&expected_name))
            .is_err()
        {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "split part {expected} is missing: expected {}",
                    directory.join(expected_name).display()
                ),
            ));
        }
    }

    let mut parts = Vec::<(i32, PathBuf)>::new();
    parts.try_reserve_exact(candidates.len()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::OutOfMemory,
            "could not allocate indexed split part paths",
        )
    })?;
    for (name, path) in candidates {
        let index = split_part_index(base_name, &name)
            .expect("a split candidate starts with the expected base and suffix")?;
        parts.push((index, path));
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
        streams.push(Box::new(open_os_read(&part_path)?));
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

        let mut stream = Self {
            streams,
            stream_ends,
            length,
            position: 0,
            index: 0,
            current_begin: 0,
            current_end: 0,
        };
        stream.update_current_stream()?;
        Ok(stream)
    }

    pub fn length(&self) -> u64 {
        self.length
    }

    pub fn position(&self) -> u64 {
        self.position
    }

    /// The byte length of part `index`.
    fn part_length(&self, index: usize) -> u64 {
        let begin = if index == 0 {
            0
        } else {
            self.stream_ends[index - 1]
        };
        self.stream_ends[index] - begin
    }

    /// `MultiFileStream.UpdateCurrentStream`: rescan parts for `position`, synchronizing the
    /// sub-stream cursor. When the position is past every part end, the source leaves the last
    /// part selected with its begin subtracted from the total.
    fn update_current_stream(&mut self) -> io::Result<()> {
        let mut begin = 0u64;
        let mut end;
        for index in 0..self.streams.len() {
            self.index = index;
            end = begin + self.part_length(index);
            if end > self.position {
                self.current_begin = begin;
                self.current_end = end;
                self.streams[index].seek(SeekFrom::Start(self.position - begin))?;
                return Ok(());
            }
            begin = end;
        }
        let last_length = self.part_length(self.index);
        self.current_begin = begin - last_length;
        self.current_end = begin;
        self.streams[self.index].seek(SeekFrom::Start(self.position - self.current_begin))?;
        Ok(())
    }

    /// `MultiFileStream.NextStream`: advance to the next part, if any.
    fn next_stream(&mut self) -> io::Result<()> {
        let next = self.index + 1;
        if next < self.streams.len() {
            self.current_begin = self.current_end;
            self.index = next;
            self.streams[next].seek(SeekFrom::Start(0))?;
            self.current_end = self.current_begin + self.part_length(next);
        }
        Ok(())
    }
}

impl Read for MultiFileStream {
    /// `MultiFileStream.Read(byte[], int, int)`: one call reads only from the current part.
    ///
    /// A request that spans a part boundary returns what the current part can supply; callers
    /// that need a full buffer (`Read::read_exact`, `Read::read_to_end`, `CopyTo`) loop over the
    /// short reads, exactly like the baseline `Stream.ReadExactly` / `CopyTo` callers.
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = self.streams[self.index].read(buffer)?;
        self.position = self.position.checked_add(read as u64).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "multi-file position overflows")
        })?;
        if self.position == self.current_end {
            self.next_stream()?;
        }
        Ok(read)
    }
}

impl Seek for MultiFileStream {
    /// `MultiFileStream.Position` setter plus `UpdateCurrentStream`.
    ///
    /// The end-relative offset itself keeps the documented BCL-correct decision in
    /// [`seek_target`]; the part bookkeeping that follows is the source's.
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let next = seek_target(self.position, self.length, from)?;
        self.position = next;
        if next < self.current_begin || next >= self.current_end {
            self.update_current_stream()?;
        } else {
            self.streams[self.index].seek(SeekFrom::Start(next - self.current_begin))?;
        }
        Ok(next)
    }
}

/// Shared end-relative seek decision for the partial, file-range, and multi-file views:
/// `End` offsets add to the length (BCL semantics), so `End(0)` is the end, negative
/// offsets count back, and positive offsets may run past the end (reads then clamp).
/// This deliberately follows neither `RandomAccessStream.Seek` nor `MultiFileStream.Seek`,
/// which both subtract their offset (`Length - offset`); the port stays BCL-correct on
/// every view instead.
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
    #[cfg(all(
        unix,
        not(any(
            target_os = "solaris",
            target_os = "espidf",
            target_os = "horizon",
            target_os = "vita"
        ))
    ))]
    use super::open_os_read;
    use super::{
        logical_file_name, logical_file_path, open_logical_file, split_base_path, split_part_index,
        FileRangeStream, MultiFileStream, PartialStream, SmartStream, SmartStreamBackend,
        SmartStreamType,
    };
    use crate::compression::wrappers::compress_gzip;
    use crate::scheme::{FileKind, SchemeReader, SchemeReaderLimits};
    #[cfg(all(
        unix,
        not(any(
            target_os = "solaris",
            target_os = "espidf",
            target_os = "horizon",
            target_os = "vita"
        ))
    ))]
    use std::fs::OpenOptions;
    #[cfg(all(
        unix,
        not(any(
            target_os = "solaris",
            target_os = "espidf",
            target_os = "horizon",
            target_os = "vita"
        ))
    ))]
    use std::io;
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

    fn read_one_byte(read: &mut impl Read) -> Option<u8> {
        let mut byte = [0; 1];
        match read.read(&mut byte).unwrap() {
            1 => Some(byte[0]),
            0 => None,
            _ => unreachable!("one-byte reads cannot return more than one byte"),
        }
    }

    fn source_multifile_fixture_fields(prefix: &str) -> Vec<&'static str> {
        include_str!("multifile_path_fixture.txt")
            .lines()
            .find(|line| line.starts_with(prefix))
            .unwrap_or_else(|| panic!("missing source MultiFile fixture row {prefix:?}"))
            .split('|')
            .collect()
    }

    #[cfg(windows)]
    fn source_local_filesystem_fixture_fields(prefix: &str) -> Vec<&'static str> {
        include_str!("local_filesystem_fixture.txt")
            .lines()
            .find(|line| line.starts_with(prefix))
            .unwrap_or_else(|| panic!("missing source LocalFileSystem fixture row {prefix:?}"))
            .split('|')
            .collect()
    }

    fn decode_hex_fixture(value: &str) -> Vec<u8> {
        assert!(value.len().is_multiple_of(2));
        value
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let high = (pair[0] as char).to_digit(16).unwrap();
                let low = (pair[1] as char).to_digit(16).unwrap();
                u8::try_from((high << 4) | low).unwrap()
            })
            .collect()
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

    /// End-relative seeks follow BCL semantics on both partial views: `End(0)` is the
    /// end, negative offsets count back, and positive offsets may run past the end
    /// (reads then clamp to zero). This locks the port against the source
    /// `RandomAccessStream.Seek` inversion (`Length - offset`).
    #[test]
    fn partial_views_seek_from_end_with_standard_offsets() {
        let source = Cursor::new(b"0123456789".to_vec());
        let mut partial = PartialStream::new(source, 2, 5).unwrap();
        assert_eq!(partial.seek(SeekFrom::End(0)).unwrap(), 5);
        assert_eq!(partial.seek(SeekFrom::End(-2)).unwrap(), 3);
        assert_eq!(partial.seek(SeekFrom::End(2)).unwrap(), 7);
        let mut tail = [0; 4];
        assert_eq!(partial.read(&mut tail).unwrap(), 0);
        assert!(partial.seek(SeekFrom::End(i64::MIN)).is_err());

        let directory = create_test_directory("partial-end-seek");
        let path = directory.join("ranges.bin");
        std::fs::write(&path, b"0123456789").unwrap();
        let file = std::fs::File::open(&path).unwrap();
        let mut range = FileRangeStream::from_file(file, 2, 5).unwrap();
        assert_eq!(range.seek(SeekFrom::End(0)).unwrap(), 5);
        assert_eq!(range.seek(SeekFrom::End(-2)).unwrap(), 3);
        assert_eq!(range.seek(SeekFrom::End(2)).unwrap(), 7);
        assert_eq!(range.read(&mut tail).unwrap(), 0);
        assert!(range.seek(SeekFrom::End(i64::MIN)).is_err());
        remove_test_files(&directory, &["ranges.bin"]);
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
    fn multi_file_stream_reads_one_part_per_call_like_the_source() {
        // `MultiFileStream.Read` reads only from the current part and advances afterwards, so a
        // request larger than the remaining part bytes returns a short read and the next call
        // continues in the following part.
        let parts = [b"abc".as_slice(), b"".as_slice(), b"defg".as_slice()]
            .into_iter()
            .map(|part| Cursor::new(part.to_vec()));
        let mut stream = MultiFileStream::new(parts).unwrap();
        let mut prefix = [0; 5];
        assert_eq!(stream.read(&mut prefix).unwrap(), 3);
        assert_eq!(&prefix[..3], b"abc");
        // The empty middle part yields a zero-length read at the boundary, like the source's
        // `NextStream` walk.
        assert_eq!(stream.read(&mut prefix).unwrap(), 0);
        assert_eq!(stream.read(&mut prefix).unwrap(), 4);
        assert_eq!(&prefix[..4], b"defg");
        assert_eq!(stream.read(&mut prefix).unwrap(), 0);

        // ReadExactly loops over the short reads, so it still crosses the boundary.
        let parts = [b"abc".as_slice(), b"defg".as_slice()]
            .into_iter()
            .map(|part| Cursor::new(part.to_vec()));
        let mut stream = MultiFileStream::new(parts).unwrap();
        let mut exact = [0; 7];
        stream.read_exact(&mut exact).unwrap();
        assert_eq!(&exact, b"abcdefg");

        let parts = [b"abc".as_slice(), b"".as_slice(), b"defg".as_slice()]
            .into_iter()
            .map(|part| Cursor::new(part.to_vec()));
        let mut stream = MultiFileStream::new(parts).unwrap();
        assert_eq!(stream.seek(SeekFrom::End(-2)).unwrap(), 5);
        let mut suffix = [0; 3];
        assert_eq!(stream.read(&mut suffix).unwrap(), 2);
        assert_eq!(&suffix[..2], b"fg");
    }

    /// End-relative seeks follow BCL semantics on the multi-file view, like on the
    /// partial views: `End(0)` is the end, negative offsets count back, and positive
    /// offsets may run past the end (reads then clamp to zero). This locks the port
    /// against the source `MultiFileStream.Seek` inversion (`Length - offset`).
    #[test]
    fn multi_file_stream_seeks_from_end_with_standard_offsets() {
        let reference = source_multifile_fixture_fields("SEEK_END|");
        assert_eq!(reference, ["SEEK_END", "10", "10", "12", "8"]);

        let parts = [b"abc".as_slice(), b"".as_slice(), b"defg".as_slice()]
            .into_iter()
            .map(|part| Cursor::new(part.to_vec()));
        let mut stream = MultiFileStream::new(parts).unwrap();
        assert_eq!(stream.length(), 7);
        assert_eq!(stream.seek(SeekFrom::End(0)).unwrap(), 7);
        assert_eq!(stream.seek(SeekFrom::End(-2)).unwrap(), 5);
        assert_eq!(stream.seek(SeekFrom::End(2)).unwrap(), 9);
        let mut tail = [0; 4];
        assert_eq!(stream.read(&mut tail).unwrap(), 0);
        assert!(stream.seek(SeekFrom::End(i64::MIN)).is_err());
    }

    #[test]
    fn multi_file_paths_match_reference_runtime_fixture() {
        for (label, path) in [
            ("plain", "stream.bundle"),
            ("split0", "stream.bundle.split0"),
            ("split007", "stream.bundle.split007"),
            ("split-plus", "stream.bundle.split+1"),
            ("split-upper", "stream.bundle.SPLIT0"),
            ("split-no-index", "stream.bundle.split"),
        ] {
            let record = source_multifile_fixture_fields(&format!("IS_MULTI|{label}|"));
            assert_eq!(record.len(), 3);
            assert_eq!(
                record[2] == "True",
                split_base_path(Path::new(path)).is_some()
            );
        }

        let split_record = source_multifile_fixture_fields("SPLIT_PART|");
        assert_eq!(
            split_record,
            [
                "SPLIT_PART",
                "stream.bundle",
                "stream.bundle",
                "616C7068613A62657461"
            ]
        );
        let directory = create_test_directory("reference-multifile-path");
        let regular_path = directory
            .join(split_record[1])
            .with_file_name("regular.bundle");
        let split_base = directory.join(split_record[1]);
        let split0 = PathBuf::from(format!("{}.split0", split_base.display()));
        let split1 = PathBuf::from(format!("{}.split1", split_base.display()));
        std::fs::write(
            &regular_path,
            decode_hex_fixture(source_multifile_fixture_fields("REGULAR|")[3]),
        )
        .unwrap();
        std::fs::write(&split0, b"alpha:").unwrap();
        std::fs::write(&split1, b"beta").unwrap();
        let mut base_open = open_logical_file(&split_base).unwrap();
        let mut base_open_bytes = Vec::new();
        base_open.read_to_end(&mut base_open_bytes).unwrap();
        assert_eq!(base_open_bytes, decode_hex_fixture(split_record[3]));

        let mut regular = open_logical_file(&regular_path).unwrap();
        let mut regular_bytes = Vec::new();
        regular.read_to_end(&mut regular_bytes).unwrap();
        assert_eq!(
            regular_bytes,
            decode_hex_fixture(source_multifile_fixture_fields("REGULAR|")[3],)
        );

        let part_path = PathBuf::from(format!("{}.split1", split_base.display()));
        assert_eq!(logical_file_path(&part_path), split_base);
        assert_eq!(
            logical_file_name(&part_path).as_deref(),
            Some(split_record[2])
        );
        let mut joined = open_logical_file(&part_path).unwrap();
        let mut joined_bytes = Vec::new();
        joined.read_to_end(&mut joined_bytes).unwrap();
        assert_eq!(joined_bytes, decode_hex_fixture(split_record[3]));

        let boundary = source_multifile_fixture_fields("BOUNDARY|");
        assert_eq!(boundary, ["BOUNDARY", "1", "3A", "2", "6265", "8"]);
        let mut boundary_stream = MultiFileStream::new([
            Cursor::new(b"alpha:".to_vec()),
            Cursor::new(b"beta".to_vec()),
        ])
        .unwrap();
        assert_eq!(boundary_stream.seek(SeekFrom::Start(5)).unwrap(), 5);
        let mut boundary_bytes = Vec::new();
        boundary_stream.read_to_end(&mut boundary_bytes).unwrap();
        assert_eq!(boundary_bytes, b":beta");

        let order_record = source_multifile_fixture_fields("NUMERIC_ORDER|");
        let ordered_base = directory.join("ordered.bundle");
        for index in 0..=10 {
            std::fs::write(
                format!("{}.split{index}", ordered_base.display()),
                [b'A' + u8::try_from(index).unwrap()],
            )
            .unwrap();
        }
        let mut ordered = open_logical_file(&ordered_base).unwrap();
        let mut ordered_bytes = Vec::new();
        ordered.read_to_end(&mut ordered_bytes).unwrap();
        assert_eq!(
            std::str::from_utf8(&ordered_bytes).unwrap(),
            order_record[1]
        );

        let gap_record = source_multifile_fixture_fields("GAP|");
        let gap_base = directory.join("gap.bundle");
        std::fs::write(format!("{}.split0", gap_base.display()), b"first").unwrap();
        std::fs::write(format!("{}.split2", gap_base.display()), b"third").unwrap();
        assert_eq!(gap_record[1], "REJECTED");
        let gap_error = match open_logical_file(&gap_base) {
            Ok(_) => panic!("gapped split parts must be rejected"),
            Err(error) => error,
        };
        assert_eq!(gap_error.kind(), std::io::ErrorKind::NotFound);

        let padded_record = source_multifile_fixture_fields("ZERO_PADDED_INDEX|");
        let padded_base = directory.join("padded.bundle");
        std::fs::write(format!("{}.split00", padded_base.display()), b"zero").unwrap();
        std::fs::write(format!("{}.split1", padded_base.display()), b"one").unwrap();
        assert_eq!(padded_record[1], "REJECTED");
        let padded_error = match open_logical_file(&padded_base) {
            Ok(_) => panic!("zero-padded split paths must be rejected"),
            Err(error) => error,
        };
        assert_eq!(padded_error.kind(), std::io::ErrorKind::NotFound);
        assert!(padded_error.to_string().contains("padded.bundle.split0"));

        drop(base_open);
        drop(regular);
        drop(joined);
        drop(ordered);

        remove_test_files(
            &directory,
            &[
                "regular.bundle",
                "stream.bundle.split0",
                "stream.bundle.split1",
                "ordered.bundle.split0",
                "ordered.bundle.split1",
                "ordered.bundle.split2",
                "ordered.bundle.split3",
                "ordered.bundle.split4",
                "ordered.bundle.split5",
                "ordered.bundle.split6",
                "ordered.bundle.split7",
                "ordered.bundle.split8",
                "ordered.bundle.split9",
                "ordered.bundle.split10",
                "gap.bundle.split0",
                "gap.bundle.split2",
                "padded.bundle.split00",
                "padded.bundle.split1",
            ],
        );
    }

    #[cfg(windows)]
    #[test]
    fn physical_file_read_sharing_matches_local_filesystem_runtime_fixture() {
        let directory = create_test_directory("source-file-sharing");
        let regular_path = directory.join("write-while-open.bin");
        std::fs::write(&regular_path, b"source-bytes").unwrap();
        let write_record = source_local_filesystem_fixture_fields("WRITE_WHILE_OPEN|");
        let read_record = source_local_filesystem_fixture_fields("READ_AFTER_WRITE_ATTEMPT|");
        let delete_record = source_local_filesystem_fixture_fields("DELETE_WHILE_OPEN|");
        assert_eq!(write_record[1], "REJECTED");
        assert_eq!(delete_record[1], "REJECTED");

        let mut regular_stream = open_logical_file(&regular_path).unwrap();
        assert!(std::fs::write(&regular_path, b"replacement").is_err());
        let mut regular_bytes = Vec::new();
        regular_stream.read_to_end(&mut regular_bytes).unwrap();
        assert_eq!(hex_encode(&regular_bytes), read_record[1]);
        assert!(std::fs::remove_file(&regular_path).is_err());
        drop(regular_stream);
        std::fs::write(&regular_path, b"after-close").unwrap();
        std::fs::remove_file(&regular_path).unwrap();

        let split_base = directory.join("split.bundle");
        let split0 = PathBuf::from(format!("{}.split0", split_base.display()));
        let split1 = PathBuf::from(format!("{}.split1", split_base.display()));
        std::fs::write(&split0, b"split-left:").unwrap();
        std::fs::write(&split1, b"split-right").unwrap();
        let split_write_record = source_local_filesystem_fixture_fields("WRITE_SPLIT_WHILE_OPEN|");
        let split_delete_record =
            source_local_filesystem_fixture_fields("DELETE_SPLIT_WHILE_OPEN|");
        let split_read_record =
            source_local_filesystem_fixture_fields("SPLIT_READ_AFTER_MUTATION_ATTEMPTS|");
        assert_eq!(split_write_record[1], "REJECTED");
        assert_eq!(split_delete_record[1], "REJECTED");

        let mut split_stream = open_logical_file(&split_base).unwrap();
        assert!(std::fs::write(&split0, b"replacement").is_err());
        assert!(std::fs::remove_file(&split1).is_err());
        let mut split_bytes = Vec::new();
        split_stream.read_to_end(&mut split_bytes).unwrap();
        assert_eq!(hex_encode(&split_bytes), split_read_record[1]);
        drop(split_stream);
        remove_test_files(&directory, &["split.bundle.split0", "split.bundle.split1"]);
    }

    #[cfg(all(
        unix,
        not(any(
            target_os = "solaris",
            target_os = "espidf",
            target_os = "horizon",
            target_os = "vita"
        ))
    ))]
    #[test]
    fn physical_file_read_sharing_matches_dotnet_unix_advisory_lock_contract() {
        use rustix::fs::{flock, FlockOperation};
        use rustix::io::Errno;

        let directory = create_test_directory("source-file-sharing-unix");
        let path = directory.join("retained.bundle");
        std::fs::write(&path, b"source-bytes").unwrap();

        let blocker = OpenOptions::new().read(true).open(&path).unwrap();
        assert_eq!(
            flock(&blocker, FlockOperation::NonBlockingLockExclusive),
            Ok(())
        );
        let blocked_read = open_os_read(&path).unwrap_err();
        assert_eq!(blocked_read.kind(), io::ErrorKind::WouldBlock);
        assert_eq!(flock(&blocker, FlockOperation::Unlock), Ok(()));
        drop(blocker);

        let mut reader = open_os_read(&path).unwrap();
        let mut compatible_writer = OpenOptions::new().write(true).open(&path).unwrap();
        // File.WriteAllBytes uses FileShare.Read on .NET 10, so its shared lock is
        // compatible with the retained reader's shared lock.
        assert_eq!(
            flock(&compatible_writer, FlockOperation::NonBlockingLockShared),
            Ok(())
        );
        compatible_writer.set_len(0).unwrap();
        compatible_writer.write_all(b"replacement").unwrap();

        let exclusive_writer = OpenOptions::new().read(true).open(&path).unwrap();
        assert_eq!(
            flock(&exclusive_writer, FlockOperation::NonBlockingLockExclusive),
            Err(Errno::WOULDBLOCK)
        );

        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"replacement");

        std::fs::remove_file(&path).unwrap();
        reader.seek(SeekFrom::Start(0)).unwrap();
        bytes.clear();
        reader.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"replacement");

        drop((reader, compatible_writer, exclusive_writer));
        std::fs::remove_dir(&directory).unwrap();
    }

    #[cfg(windows)]
    fn hex_encode(bytes: &[u8]) -> String {
        const DIGITS: &[u8; 16] = b"0123456789ABCDEF";
        let mut encoded = String::with_capacity(bytes.len().saturating_mul(2));
        for byte in bytes {
            encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
            encoded.push(char::from(DIGITS[usize::from(byte & 0x0F)]));
        }
        encoded
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
        assert_eq!(stream.read(&mut prefix).unwrap(), 3);
        assert_eq!(&prefix[..3], b"raw");
        assert_eq!(reference.position().unwrap(), 3);
        assert_eq!(stream.to_vec(10).unwrap(), b"raw bundle");
        assert_eq!(stream.position().unwrap(), 3);
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
        drop(file);
        drop(opened);
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

    /// The source requires canonical `{base}.splitN` directory entry names before
    /// its numeric sort; leading-zero aliases and nonnumeric decoys must not become
    /// accepted parts even though the input path recognizer accepts `.split007`.
    #[test]
    fn split_part_names_resolve_strictly_numeric_indices() {
        assert_eq!(split_part_index("data", "data.split0").unwrap().unwrap(), 0);
        assert_eq!(split_part_index("data", "data.split7").unwrap().unwrap(), 7);
        assert!(split_part_index("data", "other.split0").is_none());
        assert!(split_part_index("data", "data.split").unwrap().is_err());
        assert!(split_part_index("data", "data.split+1").unwrap().is_err());
        assert!(split_part_index("data", "data.split 1").unwrap().is_err());
        assert!(split_part_index("data", "data.split007").unwrap().is_err());
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
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        assert!(error.to_string().contains("split part 2 is missing"));
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
        // Memory truncation clamps a past-the-end position, like `MemoryStream`.
        assert_eq!(source.position().unwrap(), 3);
    }

    /// `SetLength` position rules by backend: memory streams clamp to the new length
    /// while file streams preserve the cursor, each matching its source stream type.
    #[test]
    fn smart_stream_set_len_clamps_memory_but_preserves_file_positions() {
        let mut memory = SmartStream::create_memory();
        memory.write_all(b"abcdef").unwrap();
        memory.seek(SeekFrom::Start(4)).unwrap();
        memory.set_len(3).unwrap();
        assert_eq!(memory.length().unwrap(), 3);
        assert_eq!(memory.position().unwrap(), 3);
        // Growth zero-fills and leaves the position alone.
        memory.set_len(6).unwrap();
        assert_eq!(memory.to_vec(6).unwrap(), b"abc\0\0\0");
        assert_eq!(memory.position().unwrap(), 3);
        // Truncating to zero resets the position, so reuse starts at the beginning.
        memory.set_len(0).unwrap();
        assert_eq!(memory.position().unwrap(), 0);

        let mut file = SmartStream::create_temp_in(crate::test_support::test_temp_root()).unwrap();
        file.write_all(b"abcdef").unwrap();
        file.seek(SeekFrom::Start(5)).unwrap();
        file.set_len(3).unwrap();
        assert_eq!(file.length().unwrap(), 3);
        assert_eq!(file.position().unwrap(), 5);
        file.free_reference();
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
        drop(nested);
        drop(partial);
        remove_test_files(&directory, &["input.bin"]);
    }

    #[test]
    fn temporary_smart_stream_is_removed_after_its_last_reference() {
        let mut owner = SmartStream::create_temp_in(crate::test_support::test_temp_root()).unwrap();
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

    #[test]
    fn stream_operations_match_reference_runtime() {
        fn seq(n: u8) -> Vec<u8> {
            (0..n).collect()
        }

        fn read_n(read: &mut impl Read, n: usize) -> Vec<u8> {
            let mut buf = vec![0; n];
            let mut got = 0;
            while got < n {
                let r = read.read(&mut buf[got..]).unwrap();
                if r == 0 {
                    break;
                }
                got += r;
            }
            buf.truncate(got);
            buf
        }

        fn read_byte(read: &mut impl Read) -> Option<u8> {
            let mut b = [0; 1];
            loop {
                let r = read.read(&mut b).unwrap();
                if r == 1 {
                    return Some(b[0]);
                }
                if r == 0 {
                    return None;
                }
            }
        }

        // Probe Temp/opencode/streamprobe (.NET SDK 10.0.401, shipped
        // IO.Files.dll): S1 SmartStream memory ops, S2 temp write/read, S3
        // null Length plus loud Read, M1 split cross-boundary, P1 range ops,
        // R1 bounded file range. Committed as io/stream_fixture.txt.
        let fixture = include_str!("stream_fixture.txt");
        let mut reads: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        for line in fixture.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }
            if line.starts_with("STREAM_READ|") {
                let parts: Vec<&str> = line.splitn(3, '|').collect();
                assert_eq!(parts.len(), 3, "bad STREAM_READ {line}");
                reads.insert(parts[1].to_string(), parts[2].to_string());
            }
        }

        // S1: memory reads, positions, and EOF match.
        {
            let dump = reads.get("S1").expect("S1 dump");
            assert!(dump.contains("LEN=64"), "S1 {dump}");
            assert!(dump.contains("R1:got=10|00010203040506070809"), "S1 {dump}");
            let mut s = SmartStream::from_memory(seq(64));
            assert_eq!(s.length().unwrap(), 64);
            assert_eq!(read_n(&mut s, 10), (0..10).collect::<Vec<u8>>());
            assert_eq!(s.position().unwrap(), 10);
            s.seek(SeekFrom::Start(20)).unwrap();
            assert_eq!(read_n(&mut s, 10), (20..30).collect::<Vec<u8>>());
            assert_eq!(s.position().unwrap(), 30);
            assert_eq!(read_byte(&mut s), Some(30));
            s.seek(SeekFrom::End(-5)).unwrap();
            assert_eq!(s.position().unwrap(), 59);
            assert_eq!(read_n(&mut s, 5), (59..64).collect::<Vec<u8>>());
            assert_eq!(read_n(&mut s, 1).len(), 0);
            assert!(dump.contains("R2:got=10|1415161718191A1B1C1D"), "S1 {dump}");
            assert!(dump.contains("RB=30"), "S1 {dump}");
            assert!(dump.contains("R3:got=5|3B3C3D3E3F"), "S1 {dump}");
            assert!(dump.contains("REOF:got=0"), "S1 {dump}");
        }

        // S2: temp write/read round trip matches.
        {
            let dump = reads.get("S2").expect("S2 dump");
            assert!(dump.contains("LEN=32"), "S2 {dump}");
            let mut s = SmartStream::create_temp_in(crate::test_support::test_temp_root()).unwrap();
            s.write_all(&seq(32)).unwrap();
            s.seek(SeekFrom::Start(0)).unwrap();
            assert_eq!(s.length().unwrap(), 32);
            assert_eq!(read_n(&mut s, 32), seq(32));
            assert_eq!(s.position().unwrap(), 32);
        }

        // S3: null Length is zero and reads fail loud on both sides
        // (reference throws NullReferenceException).
        {
            let dump = reads.get("S3").expect("S3 dump");
            assert!(dump.contains("LEN=0"), "S3 {dump}");
            assert!(
                dump.contains("THROW:System.NullReferenceException"),
                "S3 {dump}"
            );
            let mut s = SmartStream::create_null();
            assert_eq!(s.length().unwrap(), 0);
            assert!(s.read(&mut [0; 10]).is_err(), "S3 Rust fails loud");
        }

        // M1: split reads across the part boundary match.
        {
            let dump = reads.get("M1").expect("M1 dump");
            assert!(dump.contains("LEN=32"), "M1 {dump}");
            assert!(
                dump.contains("R1:got=20|000102030405060708090A0B0C0D0E0F10111213"),
                "M1 {dump}"
            );
            let dir = create_test_directory("stream-m1");
            let base = dir.join("part");
            let mut first = seq(16);
            let mut second: Vec<u8> = (16..32).collect();
            std::fs::write(base.with_extension("split0"), &first).unwrap();
            std::fs::write(base.with_extension("split1"), &second).unwrap();
            first.append(&mut second);
            let mut s = open_logical_file(&base).unwrap();
            assert_eq!(read_n(&mut s, 20), first[..20].to_vec());
            assert_eq!(s.stream_position().unwrap(), 20);
            s.seek(SeekFrom::Start(24)).unwrap();
            assert_eq!(read_n(&mut s, 8), first[24..32].to_vec());
            s.seek(SeekFrom::Start(10)).unwrap();
            assert_eq!(read_byte(&mut s), Some(10));
            assert_eq!(s.stream_position().unwrap(), 11);
            assert!(dump.contains("R2:got=8|18191A1B1C1D1E1F"), "M1 {dump}");
            assert!(dump.contains("RB=10"), "M1 {dump}");
            drop(s);
            remove_test_files(&dir, &["part.split0", "part.split1"]);
        }

        // P1: post-Seek range ops match. The reference reads-before-first-seek
        // go to the base stream's current position (PartialStream never
        // positions on construction; only Seek/Begin/End add the base offset),
        // so callers must Seek first. Rust positions on every access, which is
        // strictly more correct and identical after any Seek; the difference is
        // documented here, not hidden.
        {
            let dump = reads.get("P1").expect("P1 dump");
            assert!(dump.contains("LEN=20"), "P1 {dump}");
            let mut s = PartialStream::new(Cursor::new(seq(64)), 10, 20).unwrap();
            assert_eq!(s.length(), 20);
            assert_eq!(read_n(&mut s, 20), (10..30).collect::<Vec<u8>>());
            s.seek(SeekFrom::Start(5)).unwrap();
            assert_eq!(read_byte(&mut s), Some(15));
            s.seek(SeekFrom::End(-2)).unwrap();
            assert_eq!(s.position(), 18);
            assert_eq!(read_n(&mut s, 2), vec![28, 29]);
            assert_eq!(read_n(&mut s, 1).len(), 0);
            assert!(dump.contains("RB=15"), "P1 {dump}");
            assert!(dump.contains("R2:got=2|1C1D"), "P1 {dump}");
            assert!(dump.contains("REOF:got=0"), "P1 {dump}");
        }

        // R1: bounded file-range reads and positions match (OS positional reads,
        // same observable bytes).
        {
            let dump = reads.get("R1").expect("R1 dump");
            assert!(dump.contains("LEN=20"), "R1 {dump}");
            assert!(
                dump.contains("R:got=20|0A0B0C0D0E0F101112131415161718191A1B1C1D"),
                "R1 {dump}"
            );
            let dir = create_test_directory("stream-r1");
            let fp = dir.join("rand.bin");
            std::fs::write(&fp, seq(64)).unwrap();
            let file = std::fs::File::open(&fp).unwrap();
            let mut s = FileRangeStream::from_file(file, 10, 20).unwrap();
            assert_eq!(s.length(), 20);
            assert_eq!(read_n(&mut s, 20), (10..30).collect::<Vec<u8>>());
            s.seek(SeekFrom::Start(5)).unwrap();
            assert_eq!(read_byte(&mut s), Some(15));
            assert!(dump.contains("RB=15"), "R1 {dump}");
            drop(s);
            remove_test_files(&dir, &["rand.bin"]);
        }
    }

    /// Fields of one `STREAM` row in the reference-runtime IO fixture.
    fn io_stream_fixture_fields(case: &str) -> Vec<&'static str> {
        let prefix = format!("STREAM|{case}|");
        include_str!("io_differential_fixture.txt")
            .lines()
            .find(|line| line.starts_with(&prefix))
            .unwrap_or_else(|| panic!("missing reference IO fixture row {case:?}"))
            .split('|')
            .collect()
    }

    fn io_fixture_value(fields: &[&str], key: &str) -> String {
        let prefix = format!("{key}=");
        fields
            .iter()
            .find_map(|field| field.strip_prefix(&prefix))
            .unwrap_or_else(|| panic!("missing reference IO fixture field {key:?}"))
            .to_owned()
    }

    fn io_fixture_hex(value: &str) -> Vec<u8> {
        decode_hex_fixture(value)
    }

    /// Reference-runtime differential for the stream rows of
    /// `src/io/io_differential_fixture.txt` (probe `tools/probes/io-differential` against the
    /// shipped reference `AssetRipper.IO.Files.dll`).
    ///
    /// R2 pins the baseline `RandomAccessStream` contract: a range view reads through the parent
    /// handle positionally, so sibling views and the parent's own cursor never disturb each other.
    /// R3 pins its inverted end-relative seek. M2 pins `MultiFileStream.Read` returning only the
    /// current part's bytes while `ReadExactly` still crosses the boundary. P2/S4/S5 pin
    /// `PartialStream` and `SmartStream` observables.
    #[test]
    fn stream_observables_match_the_reference_runtime_fixture() {
        // R2: shared-handle ranges.
        {
            let fields = io_stream_fixture_fields("R2");
            assert_eq!(io_fixture_value(&fields, "first_len"), "20");
            assert_eq!(io_fixture_value(&fields, "second_len"), "16");
            assert_eq!(
                io_fixture_value(&fields, "parent_position_after_child_reads"),
                "40/40/40"
            );
            let expected_a = io_fixture_hex(
                io_fixture_value(&fields, "read_a")
                    .split(':')
                    .nth(1)
                    .unwrap(),
            );
            let expected_b = io_fixture_hex(
                io_fixture_value(&fields, "read_b")
                    .split(':')
                    .nth(1)
                    .unwrap(),
            );
            let expected_c = io_fixture_hex(
                io_fixture_value(&fields, "read_c")
                    .split(':')
                    .nth(1)
                    .unwrap(),
            );
            assert_eq!(io_fixture_value(&fields, "first_position"), "8");
            assert_eq!(io_fixture_value(&fields, "second_position"), "8");

            let dir = create_test_directory("stream-r2");
            let fp = dir.join("random.bin");
            std::fs::write(&fp, (0..64u8).collect::<Vec<u8>>()).unwrap();

            let mut parent = SmartStream::open_read(&fp).unwrap();
            parent.seek(SeekFrom::Start(40)).unwrap();
            let mut current = parent.create_partial(10, 20).unwrap();
            let mut sibling = parent.create_partial(30, 16).unwrap();
            assert_eq!(current.length().unwrap(), 20);
            assert_eq!(sibling.length().unwrap(), 16);

            let mut buffer = [0u8; 12];
            assert_eq!(current.read(&mut buffer[..8]).unwrap(), 8);
            assert_eq!(&buffer[..8], expected_a.as_slice());
            assert_eq!(parent.position().unwrap(), 40, "child read moved parent");
            assert_eq!(sibling.read(&mut buffer[..8]).unwrap(), 8);
            assert_eq!(&buffer[..8], expected_b.as_slice());
            assert_eq!(parent.position().unwrap(), 40, "sibling read moved parent");
            assert_eq!(current.position().unwrap(), 8);
            assert_eq!(sibling.position().unwrap(), 8);

            current.seek(SeekFrom::Start(5)).unwrap();
            assert_eq!(current.read(&mut buffer).unwrap(), 12);
            assert_eq!(&buffer, expected_c.as_slice());
            assert_eq!(parent.position().unwrap(), 40, "seek+read moved parent");

            let mut parent_bytes = [0u8; 4];
            assert_eq!(parent.read(&mut parent_bytes).unwrap(), 4);
            assert_eq!(parent_bytes, [0x28, 0x29, 0x2A, 0x2B]);
            assert_eq!(parent.position().unwrap(), 44);
            drop((current, sibling, parent));
            remove_test_files(&dir, &["random.bin"]);
        }

        // R3: the source's inverted end-relative offset (`Length - offset`).
        {
            let fields = io_stream_fixture_fields("R3");
            assert_eq!(io_fixture_value(&fields, "seek_end0"), "20");
            assert_eq!(io_fixture_value(&fields, "seek_end_minus2"), "22");
            assert_eq!(io_fixture_value(&fields, "seek_end_plus2"), "18");
        }

        // M2: one part per `Read`, but `ReadExactly` crosses.
        {
            let fields = io_stream_fixture_fields("M2");
            assert_eq!(io_fixture_value(&fields, "len"), "16");
            let dir = create_test_directory("stream-m2");
            let base = dir.join("parts.bundle");
            let split0 = PathBuf::from(format!("{}.split0", base.display()));
            let split1 = PathBuf::from(format!("{}.split1", base.display()));
            std::fs::write(&split0, (0..10u8).collect::<Vec<u8>>()).unwrap();
            std::fs::write(&split1, (10..16u8).collect::<Vec<u8>>()).unwrap();
            let mut stream = open_logical_file(&base).unwrap();
            assert_eq!(stream.stream_position().unwrap(), 0);
            assert_eq!(stream.seek(SeekFrom::Start(4)).unwrap(), 4);

            let mut buffer = [0u8; 12];
            assert_eq!(stream.read(&mut buffer).unwrap(), 6);
            assert_eq!(&buffer[..6], &[4, 5, 6, 7, 8, 9]);
            assert_eq!(stream.stream_position().unwrap(), 10);
            assert_eq!(stream.read(&mut buffer).unwrap(), 6);
            assert_eq!(&buffer[..6], &[10, 11, 12, 13, 14, 15]);
            assert_eq!(stream.stream_position().unwrap(), 16);
            assert_eq!(stream.read(&mut buffer).unwrap(), 0);
            assert_eq!(stream.read(&mut buffer).unwrap(), 0);

            stream.seek(SeekFrom::Start(4)).unwrap();
            let mut exact = [0u8; 12];
            stream.read_exact(&mut exact).unwrap();
            assert_eq!(exact, [4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]);

            stream.seek(SeekFrom::Start(7)).unwrap();
            assert_eq!(read_one_byte(&mut stream), Some(7));
            assert_eq!(stream.stream_position().unwrap(), 8);
            drop(stream);
            remove_test_files(&dir, &["parts.bundle.split0", "parts.bundle.split1"]);
        }

        // P2: partial range reads, byte reads, and end-relative seeks.
        {
            let fields = io_stream_fixture_fields("P2");
            assert_eq!(io_fixture_value(&fields, "len"), "20");
            let mut partial =
                PartialStream::new(Cursor::new((0..64u8).collect::<Vec<u8>>()), 10, 20).unwrap();
            partial.seek(SeekFrom::Start(0)).unwrap();
            let mut buffer = [0u8; 64];
            let read = partial.read(&mut buffer).unwrap();
            assert_eq!(read, 20);
            assert_eq!(&buffer[..20], &(10..30u8).collect::<Vec<u8>>()[..]);
            assert_eq!(partial.position(), 20);
            partial.seek(SeekFrom::Start(5)).unwrap();
            assert_eq!(read_one_byte(&mut partial), Some(15));
            let mut tail = [0u8; 8];
            assert_eq!(partial.read(&mut tail).unwrap(), 8);
            assert_eq!(tail, [16, 17, 18, 19, 20, 21, 22, 23]);
        }

        // S4/S5: memory and null SmartStream modes.
        {
            let fields = io_stream_fixture_fields("S4");
            assert_eq!(io_fixture_value(&fields, "len"), "4");
            assert_eq!(
                SmartStream::from_memory(vec![1, 2, 3, 4]).stream_type(),
                SmartStreamType::Memory
            );
            let mut memory = SmartStream::from_memory(vec![1, 2, 3, 4]);
            assert_eq!(memory.read(&mut [0u8; 8]).unwrap(), 4);

            let fields = io_stream_fixture_fields("S5");
            assert_eq!(
                io_fixture_value(&fields, "read"),
                "THROW:NullReferenceException"
            );
            let mut null_stream = SmartStream::create_null();
            assert_eq!(null_stream.length().unwrap(), 0);
            assert!(null_stream.is_null());
            assert!(null_stream.read(&mut [0u8; 1]).is_err());
        }
    }
}
