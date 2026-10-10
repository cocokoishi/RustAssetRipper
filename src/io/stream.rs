//! Seekable views used to read logical files from byte ranges or split file parts.

use std::fs::File;
use std::fs::OpenOptions;
use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use super::local_filesystem::LocalFileSystem;

/// A stream that supports both sequential reads and repositioning.
pub trait ReadSeek: Read + Seek {}

impl<T: Read + Seek> ReadSeek for T {}

/// A bounded logical view over a seekable source stream.
pub struct PartialStream<S> {
    source: S,
    base_offset: u64,
    length: u64,
    /// Logical position observed from the wrapped stream. This can start below zero when the
    /// source stream's cursor is before `base_offset`, which is how the reference PartialStream
    /// behaves before its first seek.
    position: i128,
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
            position: i128::from(original_position) - i128::from(base_offset),
        })
    }

    pub fn length(&self) -> u64 {
        self.length
    }

    pub fn position(&self) -> i128 {
        self.position
    }

    /// `PartialStream.ReadByte` delegates directly to its parent without applying the range
    /// bound. The reference implementation exposes this quirk, so callers that specifically
    /// request a byte read get the same behavior as the source.
    pub fn read_byte(&mut self) -> io::Result<Option<u8>> {
        let mut byte = [0];
        let read = self.source.read(&mut byte)?;
        self.position += i128::try_from(read).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "partial stream position overflows",
            )
        })?;
        Ok((read == 1).then_some(byte[0]))
    }

    pub fn into_inner(self) -> S {
        self.source
    }
}

impl<S: Read + Seek> Read for PartialStream<S> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        // The source caps Read using Length - Position, then reads at the wrapped stream's
        // current cursor. It does not position the parent at `base_offset` on construction.
        let available = (i128::from(self.length) - self.position).max(0);
        let requested = buffer
            .len()
            .min(usize::try_from(available).unwrap_or(usize::MAX));
        if requested == 0 {
            return Ok(0);
        }
        let read = self.source.read(&mut buffer[..requested])?;
        self.position += i128::try_from(read).map_err(|_| {
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
        let base = i128::from(self.base_offset);
        let absolute_position = match from {
            SeekFrom::Start(position) => base + i128::from(position),
            SeekFrom::Current(offset) => {
                i128::from(self.base_offset) + self.position + i128::from(offset)
            }
            SeekFrom::End(offset) => base + i128::from(self.length) + i128::from(offset),
        };
        let absolute_position = u64::try_from(absolute_position).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek position is negative or exceeds the address space",
            )
        })?;
        let next_absolute = self.source.seek(SeekFrom::Start(absolute_position))?;
        self.position = i128::from(next_absolute) - base;
        u64::try_from(self.position).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "logical partial stream position is negative or exceeds the address space",
            )
        })
    }
}

/// Bounded write-through (`PartialStream.Write`): the source throws when the
/// write would end past the range, otherwise the bytes land at the current
/// logical position. `SetLength` stays unsupported, matching the source
/// `NotSupportedException`.
impl<S: Read + Seek + Write> Write for PartialStream<S> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let end = self.position
            + i128::try_from(buffer.len()).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "partial stream's position is out of range",
                )
            })?;
        if end > i128::from(self.length) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "partial stream's position is out of range",
            ));
        }
        let written = self.source.write(buffer)?;
        self.position += i128::try_from(written).map_err(|_| {
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
        let next = source_seek_target(self.position, self.length, from)?;
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

    /// Opens a regular file or its contiguous numbered `.splitN` parts as one logical stream.
    ///
    /// Regular files retain the positional file backend so nested ranges stay file-backed;
    /// split families use the seekable multi-file reader and copy only requested partial ranges.
    pub fn open_read_multi(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        if let Some(base_path) = split_base_path(path) {
            let stream = open_split_file(&base_path, DEFAULT_MAX_PARTS)?;
            return Self::from_reader(Box::new(stream));
        }

        match open_os_read(path) {
            Ok(file) => Ok(Self::new(
                SmartStreamBackend::File(Arc::new(Mutex::new(file))),
                SmartStreamType::File,
                true,
                false,
            )),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let stream = open_split_file(path, DEFAULT_MAX_PARTS)?;
                Self::from_reader(Box::new(stream))
            }
            Err(error) => Err(error),
        }
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
        let filesystem = LocalFileSystem::global();
        let path = filesystem.create_temporary_file()?;
        let file = filesystem.open_temporary_stream_file(&path)?;
        Ok(Self::new(
            SmartStreamBackend::Temporary(TemporaryFile {
                file: Some(file),
                path,
            }),
            SmartStreamType::File,
            true,
            true,
        ))
    }

    /// Create a temporary smart stream in `directory`.
    ///
    /// The directory-parameterized form exists for callers (and tests) that already own a
    /// writable scratch directory. Product `CreateTemp()` uses LocalFileSystem's application-
    /// local/system-fallback temporary-directory policy.
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

    pub(crate) fn create_partial_file_backed(
        &self,
        offset: u64,
        length: u64,
    ) -> io::Result<(Self, bool)> {
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
            return Ok((
                Self::new(
                    SmartStreamBackend::Range(range),
                    SmartStreamType::File,
                    true,
                    false,
                ),
                true,
            ));
        }
        if let SmartStreamBackend::Range(range) = &*backend {
            let partial = range.subrange(offset, length)?;
            return Ok((
                Self::new(
                    SmartStreamBackend::Range(partial),
                    SmartStreamType::File,
                    true,
                    false,
                ),
                true,
            ));
        }
        if let SmartStreamBackend::Temporary(temporary) = &mut *backend {
            let file = temporary.file_mut()?;
            let original_position = file.stream_position()?;
            let result = (|| {
                let source_length = file.seek(SeekFrom::End(0))?;
                validate_file_range(source_length, offset, length)?;
                file.seek(SeekFrom::Start(offset))?;
                let mut output = SmartStream::create_temp()?;
                let mut remaining = length;
                let mut buffer = [0u8; 64 * 1024];
                while remaining > 0 {
                    let chunk_size = buffer
                        .len()
                        .min(usize::try_from(remaining).unwrap_or(usize::MAX));
                    file.read_exact(&mut buffer[..chunk_size])?;
                    output.write_all(&buffer[..chunk_size])?;
                    remaining -= chunk_size as u64;
                }
                output.flush()?;
                output.seek(SeekFrom::Start(0))?;
                Ok(output)
            })();
            let restore = file.seek(SeekFrom::Start(original_position));
            let output = match (result, restore) {
                (Err(error), _) => return Err(error),
                (Ok(_), Err(error)) => return Err(error),
                (Ok(output), Ok(_)) => output,
            };
            return Ok((output, false));
        }

        drop(backend);
        self.create_partial(offset, length)
            .map(|partial| (partial, false))
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

/// Open a local input through the source-compatible LocalFileSystem adapter.
fn open_os_read(path: &Path) -> io::Result<File> {
    LocalFileSystem::global().open_read(path)
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
    /// The source computes an end-relative target as `Length - offset`, so positive offsets
    /// seek backward and negative offsets can move past the logical end.
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let next = source_seek_target(self.position, self.length, from)?;
        self.position = next;
        if next < self.current_begin || next >= self.current_end {
            self.update_current_stream()?;
        } else {
            self.streams[self.index].seek(SeekFrom::Start(next - self.current_begin))?;
        }
        Ok(next)
    }
}

/// End-relative seek behavior shared by the baseline `RandomAccessStream` and
/// `MultiFileStream`: their `Seek(End, offset)` implementations use `Length - offset`.
/// `PartialStream` and ordinary `Stream` implementations keep standard signed offsets.
fn source_seek_target(position: u64, length: u64, from: SeekFrom) -> io::Result<u64> {
    let target = match from {
        SeekFrom::Start(value) => i128::from(value),
        SeekFrom::Current(offset) => i128::from(position) + i128::from(offset),
        SeekFrom::End(offset) => i128::from(length) - i128::from(offset),
    };
    let target = i64::try_from(target).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "seek position is negative or exceeds the reference stream range",
        )
    })?;
    u64::try_from(target).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "seek position is negative or exceeds the reference stream range",
        )
    })
}

#[cfg(test)]
mod tests {
    include!("stream_tests_01.rs");
}
