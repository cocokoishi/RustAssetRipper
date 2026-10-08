//! Local operating-system file access and temporary storage.
//!
//! This is the host-filesystem counterpart to [`super::filesystem::VirtualFileSystem`].
//! The reference `LocalFileSystem` is a thin `System.IO` adapter; its read sharing and
//! temporary-directory rules are kept here so stream and import code use one path.

use std::fs::{self, File, OpenOptions};
use std::hash::{BuildHasher, Hash, Hasher, RandomState};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use super::filesystem::SearchOption;

/// Text encodings supported by the local file adapter's text overloads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextEncoding {
    Utf8,
    Utf16Le,
    Utf16Be,
    Utf32Le,
    Utf32Be,
}

impl TextEncoding {
    fn preamble(self) -> &'static [u8] {
        match self {
            Self::Utf8 => b"\xEF\xBB\xBF",
            Self::Utf16Le => b"\xFF\xFE",
            Self::Utf16Be => b"\xFE\xFF",
            Self::Utf32Le => b"\xFF\xFE\x00\x00",
            Self::Utf32Be => b"\x00\x00\xFE\xFF",
        }
    }
}

/// A local filesystem adapter with the same lazy temporary-directory policy as the source.
pub struct LocalFileSystem {
    temporary_directory: Mutex<Option<PathBuf>>,
    executing_directory: PathBuf,
    system_temporary_directory: PathBuf,
}

impl LocalFileSystem {
    /// Returns the process-wide local filesystem used by `SmartStream::CreateTemp`.
    pub fn global() -> &'static Self {
        static INSTANCE: OnceLock<LocalFileSystem> = OnceLock::new();
        INSTANCE.get_or_init(Self::new)
    }

    /// Creates an adapter rooted at the current executable and operating-system temp path.
    pub fn new() -> Self {
        let executing_directory = std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_default();
        Self::with_roots(executing_directory, std::env::temp_dir())
    }

    /// Creates an adapter with explicit roots. This keeps the local/system fallback behavior
    /// testable without changing process-global environment or writing outside a scratch tree.
    pub fn with_roots(
        executing_directory: impl Into<PathBuf>,
        system_temporary_directory: impl Into<PathBuf>,
    ) -> Self {
        Self {
            temporary_directory: Mutex::new(None),
            executing_directory: executing_directory.into(),
            system_temporary_directory: system_temporary_directory.into(),
        }
    }

    /// Lazily selects an application-local temp directory, falling back to the system temp
    /// `AssetRipper` directory if the executable directory cannot be written.
    pub fn temporary_directory(&self) -> io::Result<PathBuf> {
        let mut slot = self
            .temporary_directory
            .lock()
            .map_err(|_| io::Error::other("local temporary-directory mutex was poisoned"))?;
        if let Some(path) = &*slot {
            return Ok(path.clone());
        }

        let token = new_guid_string();
        let local = self.executing_directory.join("temp").join(&token[..4]);
        let path = if probe_local_temporary_directory(&local).is_ok() {
            local
        } else {
            self.system_temporary_directory
                .join("AssetRipper")
                .join(&token[..4])
        };
        *slot = Some(path.clone());
        Ok(path)
    }

    /// Sets the temp directory to an absolute, normalized path. Blank values are ignored,
    /// matching the source setter.
    pub fn set_temporary_directory(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let path = path.as_ref();
        if path.as_os_str().to_string_lossy().trim().is_empty() {
            return Ok(());
        }
        let full_path = full_path(path)?;
        let mut slot = self
            .temporary_directory
            .lock()
            .map_err(|_| io::Error::other("local temporary-directory mutex was poisoned"))?;
        *slot = Some(full_path);
        Ok(())
    }

    /// Opens a source-compatible `File.OpenRead` handle.
    pub fn open_read(&self, path: impl AsRef<Path>) -> io::Result<File> {
        let mut options = OpenOptions::new();
        options.read(true);
        open_with_share(path.as_ref(), &mut options, FileShare::Read)
    }

    /// Opens an existing temporary file for read/write access with exclusive sharing, as
    /// `SmartStream.CreateTemp` does after `File.CreateTemporary` returns its path.
    pub(crate) fn open_temporary_stream_file(&self, path: impl AsRef<Path>) -> io::Result<File> {
        let mut options = OpenOptions::new();
        options.read(true).write(true);
        open_with_share(path.as_ref(), &mut options, FileShare::None)
    }

    /// Opens a source-compatible `File.Create` handle (read/write, create or truncate,
    /// exclusive sharing).
    pub fn create_file(&self, path: impl AsRef<Path>) -> io::Result<File> {
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(true);
        open_with_share(path.as_ref(), &mut options, FileShare::None)
    }

    /// Opens a source-compatible `File.OpenWrite` handle (create if missing, preserve existing
    /// contents, and start at position zero).
    pub fn open_write(&self, path: impl AsRef<Path>) -> io::Result<File> {
        let mut options = OpenOptions::new();
        options.write(true).create(true);
        open_with_share(path.as_ref(), &mut options, FileShare::None)
    }

    /// Reads a complete file through the retained-reader sharing contract.
    pub fn read_all_bytes(&self, path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
        let mut file = self.open_read(path)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    /// Reads complete text using BOM detection and UTF-8 as the default encoding.
    pub fn read_all_text(&self, path: impl AsRef<Path>) -> io::Result<String> {
        decode_text(&self.read_all_bytes(path)?, TextEncoding::Utf8)
    }

    /// Reads complete text with the supplied fallback encoding and BOM detection.
    pub fn read_all_text_with_encoding(
        &self,
        path: impl AsRef<Path>,
        encoding: TextEncoding,
    ) -> io::Result<String> {
        decode_text(&self.read_all_bytes(path)?, encoding)
    }

    /// Writes a complete file with the source `File.WriteAllBytes` sharing contract.
    pub fn write_all_bytes(&self, path: impl AsRef<Path>, bytes: &[u8]) -> io::Result<()> {
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        let mut file = open_with_share(path.as_ref(), &mut options, FileShare::Read)?;
        file.write_all(bytes)
    }

    /// Writes UTF-8 text without a preamble, matching the default WriteAllText overload.
    pub fn write_all_text(&self, path: impl AsRef<Path>, contents: &str) -> io::Result<()> {
        self.write_all_bytes(path, contents.as_bytes())
    }

    /// Writes text with the selected encoding and its standard byte-order preamble.
    pub fn write_all_text_with_encoding(
        &self,
        path: impl AsRef<Path>,
        contents: &str,
        encoding: TextEncoding,
    ) -> io::Result<()> {
        let mut bytes = Vec::new();
        let payload_capacity = match encoding {
            TextEncoding::Utf8 => Some(contents.len()),
            TextEncoding::Utf16Le | TextEncoding::Utf16Be => {
                contents.encode_utf16().count().checked_mul(2)
            }
            TextEncoding::Utf32Le | TextEncoding::Utf32Be => {
                contents.chars().count().checked_mul(4)
            }
        }
        .ok_or_else(|| io::Error::new(io::ErrorKind::OutOfMemory, "text output is too large"))?;
        let capacity = encoding
            .preamble()
            .len()
            .checked_add(payload_capacity)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::OutOfMemory, "text output is too large")
            })?;
        bytes
            .try_reserve_exact(capacity)
            .map_err(|_| io::Error::new(io::ErrorKind::OutOfMemory, "text output is too large"))?;
        bytes.extend_from_slice(encoding.preamble());
        match encoding {
            TextEncoding::Utf8 => bytes.extend_from_slice(contents.as_bytes()),
            TextEncoding::Utf16Le => encode_utf16(contents, true, &mut bytes),
            TextEncoding::Utf16Be => encode_utf16(contents, false, &mut bytes),
            TextEncoding::Utf32Le => encode_utf32(contents, true, &mut bytes),
            TextEncoding::Utf32Be => encode_utf32(contents, false, &mut bytes),
        }
        self.write_all_bytes(path, &bytes)
    }

    /// Implements `File.Delete`, which succeeds when the named file is absent.
    pub fn delete_file(&self, path: impl AsRef<Path>) -> io::Result<()> {
        match fs::remove_file(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }

    /// Implements `File.Exists`: directories and inaccessible/invalid paths return false.
    pub fn file_exists(&self, path: impl AsRef<Path>) -> bool {
        path.as_ref().is_file()
    }

    /// Creates a directory and any missing ancestors.
    pub fn create_directory(&self, path: impl AsRef<Path>) -> io::Result<()> {
        fs::create_dir_all(path)
    }

    /// Recursively deletes a directory tree, matching the local adapter's `Directory.Delete`.
    pub fn delete_directory(&self, path: impl AsRef<Path>) -> io::Result<()> {
        fs::remove_dir_all(path)
    }

    /// Implements `Directory.Exists`; inaccessible or invalid paths return false.
    pub fn directory_exists(&self, path: impl AsRef<Path>) -> bool {
        path.as_ref().is_dir()
    }

    /// Lazily returns files matching a pattern under a directory.
    ///
    /// Entries are sorted within each visited directory for repeatable Rust output;
    /// System.IO does not promise an enumeration order. The "*.*" pattern matches
    /// extensionless names as on modern .NET.
    pub fn enumerate_files(
        &self,
        path: impl AsRef<Path>,
        search_pattern: &str,
        search_option: SearchOption,
    ) -> io::Result<DirectoryEnumeration> {
        DirectoryEnumeration::new(path.as_ref(), search_pattern, search_option, false)
    }

    /// EnumerateFiles(path) convenience overload.
    pub fn enumerate_files_in(&self, path: impl AsRef<Path>) -> io::Result<DirectoryEnumeration> {
        self.enumerate_files(path, "*", SearchOption::TopDirectoryOnly)
    }

    /// EnumerateFiles(path, searchPattern) convenience overload.
    pub fn enumerate_files_matching(
        &self,
        path: impl AsRef<Path>,
        search_pattern: &str,
    ) -> io::Result<DirectoryEnumeration> {
        self.enumerate_files(path, search_pattern, SearchOption::TopDirectoryOnly)
    }

    /// Lazily returns directories matching a pattern under a directory.
    pub fn enumerate_directories(
        &self,
        path: impl AsRef<Path>,
        search_pattern: &str,
        search_option: SearchOption,
    ) -> io::Result<DirectoryEnumeration> {
        DirectoryEnumeration::new(path.as_ref(), search_pattern, search_option, true)
    }

    /// EnumerateDirectories(path) convenience overload.
    pub fn enumerate_directories_in(
        &self,
        path: impl AsRef<Path>,
    ) -> io::Result<DirectoryEnumeration> {
        self.enumerate_directories(path, "*", SearchOption::TopDirectoryOnly)
    }

    /// EnumerateDirectories(path, searchPattern) convenience overload.
    pub fn enumerate_directories_matching(
        &self,
        path: impl AsRef<Path>,
        search_pattern: &str,
    ) -> io::Result<DirectoryEnumeration> {
        self.enumerate_directories(path, search_pattern, SearchOption::TopDirectoryOnly)
    }

    /// GetFiles(path) convenience overload.
    pub fn get_files(&self, path: impl AsRef<Path>) -> io::Result<Vec<PathBuf>> {
        self.enumerate_files(path, "*", SearchOption::TopDirectoryOnly)?
            .collect()
    }

    /// GetFiles(path, searchPattern) convenience overload.
    pub fn get_files_matching(
        &self,
        path: impl AsRef<Path>,
        search_pattern: &str,
    ) -> io::Result<Vec<PathBuf>> {
        self.enumerate_files(path, search_pattern, SearchOption::TopDirectoryOnly)?
            .collect()
    }

    /// GetFiles(path, searchPattern, searchOption) convenience overload.
    pub fn get_files_with_options(
        &self,
        path: impl AsRef<Path>,
        search_pattern: &str,
        search_option: SearchOption,
    ) -> io::Result<Vec<PathBuf>> {
        self.enumerate_files(path, search_pattern, search_option)?
            .collect()
    }

    /// GetDirectories(path) convenience overload.
    pub fn get_directories(&self, path: impl AsRef<Path>) -> io::Result<Vec<PathBuf>> {
        self.enumerate_directories(path, "*", SearchOption::TopDirectoryOnly)?
            .collect()
    }

    /// GetDirectories(path, searchPattern) convenience overload.
    pub fn get_directories_matching(
        &self,
        path: impl AsRef<Path>,
        search_pattern: &str,
    ) -> io::Result<Vec<PathBuf>> {
        self.enumerate_directories(path, search_pattern, SearchOption::TopDirectoryOnly)?
            .collect()
    }

    /// GetDirectories(path, searchPattern, searchOption) convenience overload.
    pub fn get_directories_with_options(
        &self,
        path: impl AsRef<Path>,
        search_pattern: &str,
        search_option: SearchOption,
    ) -> io::Result<Vec<PathBuf>> {
        self.enumerate_directories(path, search_pattern, search_option)?
            .collect()
    }

    /// Returns the normalized absolute form of a local path.
    pub fn get_full_path(&self, path: impl AsRef<Path>) -> io::Result<PathBuf> {
        full_path(path.as_ref())
    }

    /// Implements the host platform's `Path.IsPathRooted` check.
    pub fn is_path_rooted(&self, path: impl AsRef<Path>) -> bool {
        path.as_ref()
            .components()
            .next()
            .is_some_and(|component| matches!(component, Component::Prefix(_) | Component::RootDir))
    }

    /// Joins path strings with the host separator, following concatenate semantics.
    pub fn join_paths<S: AsRef<str>>(&self, paths: &[S]) -> String {
        let mut result = String::new();
        for path in paths {
            let path = path.as_ref();
            if path.is_empty() {
                continue;
            }
            if result.is_empty() {
                result.push_str(path);
                continue;
            }
            let left_has_separator = result.ends_with(is_path_separator);
            let right = path.trim_start_matches(is_path_separator);
            if !left_has_separator && !right.is_empty() {
                result.push(std::path::MAIN_SEPARATOR);
            }
            result.push_str(right);
        }
        result
    }

    /// Returns the directory portion of a path, or None when it has no parent.
    pub fn get_directory_name(&self, path: impl AsRef<Path>) -> Option<PathBuf> {
        path.as_ref().parent().map(Path::to_path_buf)
    }

    /// Returns the dot-inclusive extension of the final path component.
    pub fn get_extension(&self, path: impl AsRef<Path>) -> String {
        let name = self.get_file_name(path);
        match name.rfind('.') {
            Some(index) => name[index..].to_owned(),
            None => String::new(),
        }
    }

    /// Returns the final path component, or an empty string for a trailing separator.
    pub fn get_file_name(&self, path: impl AsRef<Path>) -> String {
        let path = path.as_ref().as_os_str().to_string_lossy();
        if path.is_empty() || path.ends_with(is_path_separator) {
            return String::new();
        }
        path.rsplit(is_path_separator)
            .next()
            .unwrap_or_default()
            .to_owned()
    }

    /// Returns the final path component without its extension.
    pub fn get_file_name_without_extension(&self, path: impl AsRef<Path>) -> String {
        let name = self.get_file_name(path);
        match name.rfind('.') {
            Some(index) => name[..index].to_owned(),
            None => name,
        }
    }

    /// Returns path relative to another path, or the absolute target when roots differ.
    pub fn get_relative_path(
        &self,
        relative_to: impl AsRef<Path>,
        path: impl AsRef<Path>,
    ) -> io::Result<PathBuf> {
        let base = full_path(relative_to.as_ref())?;
        let target = full_path(path.as_ref())?;
        let base_components: Vec<_> = base.components().collect();
        let target_components: Vec<_> = target.components().collect();
        let common = base_components
            .iter()
            .zip(&target_components)
            .take_while(|(left, right)| path_component_eq(left, right))
            .count();
        if common == 0
            || base_components
                .first()
                .zip(target_components.first())
                .is_some_and(|(left, right)| !path_component_eq(left, right))
        {
            return Ok(target);
        }
        let mut relative = PathBuf::new();
        for _ in common..base_components.len() {
            relative.push("..");
        }
        for component in &target_components[common..] {
            relative.push(component.as_os_str());
        }
        if relative.as_os_str().is_empty() {
            relative.push(".");
        }
        Ok(relative)
    }

    /// Creates an empty GUID-named file under [`Self::temporary_directory`].
    pub fn create_temporary_file(&self) -> io::Result<PathBuf> {
        let directory = self.temporary_directory()?;
        self.create_directory(&directory)?;
        for _ in 0..128 {
            let path = directory.join(new_guid_string());
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            match open_with_share(&path, &mut options, FileShare::None) {
                Ok(file) => {
                    drop(file);
                    return Ok(path);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not create a unique temporary file",
        ))
    }

    /// Creates and returns a GUID-named child directory under [`Self::temporary_directory`].
    pub fn create_temporary_directory(&self) -> io::Result<PathBuf> {
        let parent = self.temporary_directory()?;
        self.create_directory(&parent)?;
        for _ in 0..128 {
            let token = new_guid_string();
            let path = parent.join(&token[..8]);
            match fs::create_dir(&path) {
                Ok(()) => return Ok(path),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not create a unique temporary directory",
        ))
    }

    /// Removes the current temporary directory tree if it exists.
    pub fn delete_temporary_directory(&self) -> io::Result<()> {
        let path = self.temporary_directory()?;
        if self.directory_exists(&path) {
            self.delete_directory(path)?;
        }
        Ok(())
    }
}

impl Default for LocalFileSystem {
    fn default() -> Self {
        Self::new()
    }
}

struct DirectoryFrame {
    entries: Vec<(PathBuf, std::ffi::OsString)>,
    index: usize,
}

/// Lazy directory walk returned by the LocalFileSystem EnumerateFiles and
/// EnumerateDirectories operations. Filesystem errors are yielded at the point the
/// entry is visited and terminate subsequent iteration.
pub struct DirectoryEnumeration {
    root: Option<PathBuf>,
    frames: Vec<DirectoryFrame>,
    deferred_directory: Option<PathBuf>,
    pattern: String,
    search_option: SearchOption,
    directories: bool,
    finished: bool,
}

impl DirectoryEnumeration {
    fn new(
        root: &Path,
        search_pattern: &str,
        search_option: SearchOption,
        directories: bool,
    ) -> io::Result<Self> {
        validate_search_pattern(search_pattern)?;
        Ok(Self {
            root: Some(root.to_path_buf()),
            frames: Vec::new(),
            deferred_directory: None,
            pattern: if search_pattern == "*.*" {
                "*".to_owned()
            } else {
                search_pattern.to_owned()
            },
            search_option,
            directories,
            finished: false,
        })
    }

    fn push_directory(&mut self, path: PathBuf) -> io::Result<()> {
        let mut entries = Vec::new();
        for entry in fs::read_dir(&path)? {
            let entry = entry?;
            entries.try_reserve(1).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::OutOfMemory,
                    "could not allocate directory enumeration entries",
                )
            })?;
            entries.push((entry.path(), entry.file_name()));
        }
        entries.sort_by(|left, right| left.1.cmp(&right.1));
        self.frames.try_reserve(1).map_err(|_| {
            io::Error::new(
                io::ErrorKind::OutOfMemory,
                "could not allocate directory enumeration stack",
            )
        })?;
        self.frames.push(DirectoryFrame { entries, index: 0 });
        Ok(())
    }
}

impl Iterator for DirectoryEnumeration {
    type Item = io::Result<PathBuf>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        if let Some(directory) = self.deferred_directory.take() {
            if let Err(error) = self.push_directory(directory) {
                self.finished = true;
                return Some(Err(error));
            }
        }
        loop {
            if self.frames.is_empty() {
                let Some(root) = self.root.take() else {
                    self.finished = true;
                    return None;
                };
                if let Err(error) = self.push_directory(root) {
                    self.finished = true;
                    return Some(Err(error));
                }
            }

            let entry = {
                let Some(frame) = self.frames.last_mut() else {
                    self.finished = true;
                    return None;
                };
                if frame.index == frame.entries.len() {
                    self.frames.pop();
                    continue;
                }
                let entry = frame.entries[frame.index].clone();
                frame.index += 1;
                entry
            };
            let (path, name) = entry;
            let file_type = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata.file_type(),
                Err(error) => {
                    self.finished = true;
                    return Some(Err(error));
                }
            };
            let metadata = match fs::metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    self.finished = true;
                    return Some(Err(error));
                }
            };

            if metadata.is_dir() {
                let matches = wildcard_matches(&self.pattern, &name.to_string_lossy());
                if self.search_option == SearchOption::AllDirectories && !file_type.is_symlink() {
                    if self.directories && matches {
                        self.deferred_directory = Some(path.clone());
                    } else if let Err(error) = self.push_directory(path.clone()) {
                        self.finished = true;
                        return Some(Err(error));
                    }
                }
                if self.directories && matches {
                    return Some(Ok(path));
                }
            } else if !self.directories && wildcard_matches(&self.pattern, &name.to_string_lossy())
            {
                return Some(Ok(path));
            }
        }
    }
}

#[derive(Clone, Copy)]
enum FileShare {
    None,
    Read,
}

fn open_with_share(path: &Path, options: &mut OpenOptions, share: FileShare) -> io::Result<File> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;

        const FILE_SHARE_READ: u32 = 0x0000_0001;
        options.share_mode(match share {
            FileShare::None => 0,
            FileShare::Read => FILE_SHARE_READ,
        });
        options.open(path)
    }
    #[cfg(unix)]
    {
        let file = options.open(path)?;
        #[cfg(not(any(
            target_os = "solaris",
            target_os = "espidf",
            target_os = "horizon",
            target_os = "vita"
        )))]
        {
            let operation = match share {
                FileShare::None => rustix::fs::FlockOperation::NonBlockingLockExclusive,
                FileShare::Read => rustix::fs::FlockOperation::NonBlockingLockShared,
            };
            return match rustix::fs::flock(&file, operation) {
                Ok(()) => Ok(file),
                Err(error) if error == rustix::io::Errno::WOULDBLOCK => Err(error.into()),
                Err(_) => Ok(file),
            };
        }
        #[cfg(any(
            target_os = "solaris",
            target_os = "espidf",
            target_os = "horizon",
            target_os = "vita"
        ))]
        {
            let _ = share;
            Ok(file)
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = share;
        options.open(path)
    }
}

fn probe_local_temporary_directory(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    let result = (|| {
        let marker_path = path.join(".WriteTest");
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        let mut marker = open_with_share(&marker_path, &mut options, FileShare::Read)?;
        marker.write_all(b"test")?;
        drop(marker);
        fs::remove_dir_all(path)
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(path);
    }
    result
}

fn decode_text(bytes: &[u8], fallback: TextEncoding) -> io::Result<String> {
    let (encoding, content) = if bytes.starts_with(b"\x00\x00\xFE\xFF") {
        (TextEncoding::Utf32Be, &bytes[4..])
    } else if bytes.starts_with(b"\xFF\xFE\x00\x00") {
        (TextEncoding::Utf32Le, &bytes[4..])
    } else if bytes.starts_with(b"\xEF\xBB\xBF") {
        (TextEncoding::Utf8, &bytes[3..])
    } else if bytes.starts_with(b"\xFE\xFF") {
        (TextEncoding::Utf16Be, &bytes[2..])
    } else if bytes.starts_with(b"\xFF\xFE") {
        (TextEncoding::Utf16Le, &bytes[2..])
    } else {
        (fallback, bytes)
    };
    match encoding {
        TextEncoding::Utf8 => Ok(String::from_utf8_lossy(content).into_owned()),
        TextEncoding::Utf16Le => Ok(decode_utf16(content, true)),
        TextEncoding::Utf16Be => Ok(decode_utf16(content, false)),
        TextEncoding::Utf32Le => Ok(decode_utf32(content, true)),
        TextEncoding::Utf32Be => Ok(decode_utf32(content, false)),
    }
}

fn decode_utf16(bytes: &[u8], little_endian: bool) -> String {
    let mut units = Vec::with_capacity(bytes.len().div_ceil(2));
    for pair in bytes.chunks_exact(2) {
        let pair = [pair[0], pair[1]];
        units.push(if little_endian {
            u16::from_le_bytes(pair)
        } else {
            u16::from_be_bytes(pair)
        });
    }
    let mut text: String = char::decode_utf16(units)
        .map(|character| character.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect();
    if !bytes.len().is_multiple_of(2) {
        text.push(char::REPLACEMENT_CHARACTER);
    }
    text
}

fn decode_utf32(bytes: &[u8], little_endian: bool) -> String {
    let mut text = String::with_capacity(bytes.len());
    for chunk in bytes.chunks_exact(4) {
        let value = if little_endian {
            u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
        } else {
            u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])
        };
        text.push(char::from_u32(value).unwrap_or(char::REPLACEMENT_CHARACTER));
    }
    if !bytes.len().is_multiple_of(4) {
        text.push(char::REPLACEMENT_CHARACTER);
    }
    text
}

fn encode_utf16(text: &str, little_endian: bool, output: &mut Vec<u8>) {
    for unit in text.encode_utf16() {
        let bytes = if little_endian {
            unit.to_le_bytes()
        } else {
            unit.to_be_bytes()
        };
        output.extend_from_slice(&bytes);
    }
}

fn encode_utf32(text: &str, little_endian: bool, output: &mut Vec<u8>) {
    for character in text.chars() {
        let value = u32::from(character);
        let bytes = if little_endian {
            value.to_le_bytes()
        } else {
            value.to_be_bytes()
        };
        output.extend_from_slice(&bytes);
    }
}

fn validate_search_pattern(pattern: &str) -> io::Result<()> {
    if pattern.is_empty() || pattern.contains('\0') || pattern.chars().any(is_path_separator) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "directory search pattern must be a nonempty file-name pattern",
        ));
    }
    Ok(())
}

fn wildcard_matches(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let name: Vec<char> = name.chars().collect();
    let (mut pattern_index, mut name_index) = (0, 0);
    let (mut star_index, mut retry_name_index) = (None, 0);
    while name_index < name.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == '?'
                || same_search_character(pattern[pattern_index], name[name_index]))
        {
            pattern_index += 1;
            name_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == '*' {
            star_index = Some(pattern_index);
            pattern_index += 1;
            retry_name_index = name_index;
        } else if let Some(star) = star_index {
            pattern_index = star + 1;
            retry_name_index += 1;
            name_index = retry_name_index;
        } else {
            return false;
        }
    }
    pattern[pattern_index..]
        .iter()
        .all(|character| *character == '*')
}

fn same_search_character(left: char, right: char) -> bool {
    #[cfg(windows)]
    {
        left.to_lowercase().eq(right.to_lowercase())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn is_path_separator(character: char) -> bool {
    character == std::path::MAIN_SEPARATOR
        || (cfg!(windows) && (character == '/' || character == '\\'))
}

fn path_component_eq(left: &Component<'_>, right: &Component<'_>) -> bool {
    #[cfg(windows)]
    {
        left.as_os_str()
            .to_string_lossy()
            .eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn full_path(path: &Path) -> io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() && !normalized.has_root() {
                    normalized.push("..");
                }
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    Ok(normalized)
}

fn new_guid_string() -> String {
    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nonce = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let now = SystemTime::now();
    let mut first = RandomState::new().build_hasher();
    now.hash(&mut first);
    std::process::id().hash(&mut first);
    nonce.hash(&mut first);
    std::thread::current().id().hash(&mut first);
    let mut second = RandomState::new().build_hasher();
    nonce.rotate_left(23).hash(&mut second);
    std::process::id().hash(&mut second);
    now.hash(&mut second);
    let mut value = ((first.finish() as u128) << 64) | u128::from(second.finish());
    let mut bytes = value.to_be_bytes();
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    value = u128::from_be_bytes(bytes);
    let hex = format!("{value:032x}");
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

#[cfg(test)]
mod tests {
    use super::{full_path, new_guid_string, LocalFileSystem, TextEncoding};
    use crate::io::filesystem::SearchOption;
    use std::io::{self, Read};
    #[cfg(windows)]
    use std::io::{Seek, Write};
    use std::path::Path;

    #[cfg(windows)]
    fn is_hex_name(name: &str, length: usize) -> bool {
        name.len() == length && name.bytes().all(|byte| byte.is_ascii_hexdigit())
    }

    #[cfg(windows)]
    fn is_guid_name(name: &str) -> bool {
        let parts: Vec<&str> = name.split('-').collect();
        parts.len() == 5
            && [8, 4, 4, 4, 12]
                .into_iter()
                .zip(parts)
                .all(|(length, part)| is_hex_name(part, length))
    }

    #[cfg(windows)]
    fn bytes_as_hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02X}")).collect()
    }

    #[test]
    fn local_text_io_handles_bom_detection_and_supported_encodings() {
        let root = crate::test_support::test_temp_root().join("local-fs-text");
        std::fs::create_dir_all(&root).unwrap();
        let filesystem = LocalFileSystem::with_roots(&root, root.join("system"));
        let path = root.join("text.txt");

        filesystem.write_all_text(&path, "café").unwrap();
        assert_eq!(filesystem.read_all_bytes(&path).unwrap(), b"caf\xC3\xA9");
        assert_eq!(filesystem.read_all_text(&path).unwrap(), "café");

        for encoding in [
            TextEncoding::Utf8,
            TextEncoding::Utf16Le,
            TextEncoding::Utf16Be,
            TextEncoding::Utf32Le,
            TextEncoding::Utf32Be,
        ] {
            filesystem
                .write_all_text_with_encoding(&path, "café🙂", encoding)
                .unwrap();
            assert_eq!(filesystem.read_all_text(&path).unwrap(), "café🙂");
            assert_eq!(
                filesystem
                    .read_all_text_with_encoding(&path, TextEncoding::Utf8)
                    .unwrap(),
                "café🙂",
                "a detected BOM takes precedence over the requested fallback encoding"
            );
        }

        filesystem.write_all_bytes(&path, b"bad\xFFtext").unwrap();
        assert_eq!(filesystem.read_all_text(&path).unwrap(), "bad\u{FFFD}text");
        filesystem
            .write_all_bytes(&path, b"\xFF\xFEa\x00\xFF")
            .unwrap();
        assert_eq!(filesystem.read_all_text(&path).unwrap(), "a\u{FFFD}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn local_directory_enumeration_matches_search_options_and_patterns() {
        let root = crate::test_support::test_temp_root().join("local-fs-enumeration");
        let nested = root.join("nested");
        std::fs::create_dir_all(nested.join("deep")).unwrap();
        std::fs::create_dir_all(root.join("other")).unwrap();
        for path in [
            root.join("alpha.txt"),
            root.join("BETA.TXT"),
            root.join("no-extension"),
            nested.join("inner.txt"),
            nested.join("deep").join("deep.txt"),
        ] {
            std::fs::write(path, b"fixture").unwrap();
        }
        let filesystem = LocalFileSystem::with_roots(&root, root.join("system"));
        let files = filesystem
            .enumerate_files(&root, "*.txt", SearchOption::TopDirectoryOnly)
            .unwrap()
            .collect::<io::Result<Vec<_>>>()
            .unwrap();
        let names: Vec<String> = files
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        #[cfg(windows)]
        assert_eq!(names, ["BETA.TXT", "alpha.txt"]);
        #[cfg(not(windows))]
        assert_eq!(names, ["alpha.txt"]);

        let all_files = filesystem
            .enumerate_files(&root, "*.txt", SearchOption::AllDirectories)
            .unwrap()
            .collect::<io::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(all_files.len(), if cfg!(windows) { 4 } else { 3 });
        let directories = filesystem
            .enumerate_directories(&root, "*", SearchOption::AllDirectories)
            .unwrap()
            .collect::<io::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(directories.len(), 3);
        assert!(filesystem
            .enumerate_files(&root, "", SearchOption::TopDirectoryOnly)
            .is_err());
        assert!(filesystem
            .enumerate_files(&root, "nested/*.txt", SearchOption::AllDirectories)
            .is_err());
        let missing = root.join("removed-after-enumerator-creation");
        let mut lazy = filesystem
            .enumerate_files(&missing, "*", SearchOption::TopDirectoryOnly)
            .unwrap();
        assert!(lazy.next().unwrap().is_err());
        assert!(lazy.next().is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn local_path_helpers_match_path_join_and_component_contracts() {
        let root = crate::test_support::test_temp_root().join("local-fs-paths");
        let filesystem = LocalFileSystem::with_roots(&root, root.join("system"));
        assert_eq!(
            filesystem.join_paths(&["alpha", "beta", "gamma"]),
            ["alpha", "beta", "gamma"].join(std::path::MAIN_SEPARATOR_STR)
        );
        assert_eq!(filesystem.get_file_name("alpha"), "alpha",);
        assert_eq!(filesystem.get_extension(".hidden"), ".hidden");
        assert_eq!(filesystem.get_file_name_without_extension("x.txt"), "x");
        assert_eq!(filesystem.get_file_name("alpha/beta/"), "");
        assert!(filesystem.is_path_rooted(std::path::MAIN_SEPARATOR.to_string()));
        assert_eq!(
            filesystem
                .get_relative_path(root.join("base"), root.join("base").join("a").join("b"))
                .unwrap(),
            Path::new("a").join("b")
        );
        assert!(filesystem.is_path_rooted("C:relative") == cfg!(windows));
    }

    #[test]
    fn local_file_methods_match_the_expected_sharing_and_delete_contracts() {
        let root = crate::test_support::test_temp_root().join("local-fs-methods");
        let filesystem = LocalFileSystem::with_roots(&root, root.join("system"));
        filesystem.create_directory(&root).unwrap();
        let path = root.join("input.bin");
        filesystem.write_all_bytes(&path, b"before").unwrap();
        {
            let mut reader = filesystem.open_read(&path).unwrap();
            let mut byte = [0];
            reader.read_exact(&mut byte).unwrap();
            assert_eq!(byte, [b'b']);
            let mut rest = Vec::new();
            reader.read_to_end(&mut rest).unwrap();
            assert_eq!(rest, b"efore");
        }
        filesystem.write_all_bytes(&path, b"after").unwrap();
        assert_eq!(filesystem.read_all_bytes(&path).unwrap(), b"after");
        filesystem.delete_file(&path).unwrap();
        filesystem.delete_file(&path).unwrap();
        assert!(!filesystem.file_exists(&path));
        filesystem.delete_directory(&root).unwrap();
    }

    #[test]
    fn local_temp_directory_prefers_executable_path_and_honors_setter_rules() {
        let root = crate::test_support::test_temp_root().join("local-fs-temp");
        let executing = root.join("application");
        let system = root.join("system");
        std::fs::create_dir_all(&executing).unwrap();
        let filesystem = LocalFileSystem::with_roots(&executing, &system);
        let selected = filesystem.temporary_directory().unwrap();
        assert!(selected.starts_with(executing.join("temp")));
        assert!(
            !selected.exists(),
            "the source probes then deletes this folder"
        );
        let file = filesystem.create_temporary_file().unwrap();
        assert!(file.starts_with(&selected));
        assert!(filesystem.file_exists(&file));
        let directory = filesystem.create_temporary_directory().unwrap();
        assert!(directory.starts_with(&selected));
        assert!(filesystem.directory_exists(&directory));

        filesystem
            .set_temporary_directory(root.join("custom"))
            .unwrap();
        let custom = full_path(&root.join("custom")).unwrap();
        assert_eq!(filesystem.temporary_directory().unwrap(), custom);
        filesystem.set_temporary_directory("   ").unwrap();
        assert_eq!(filesystem.temporary_directory().unwrap(), custom);
        filesystem.delete_temporary_directory().unwrap();
        assert!(!custom.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn local_temp_directory_falls_back_when_executable_path_is_not_a_directory() {
        let root = crate::test_support::test_temp_root().join("local-fs-fallback");
        std::fs::create_dir_all(&root).unwrap();
        let not_a_directory = root.join("not-a-directory");
        std::fs::write(&not_a_directory, b"file").unwrap();
        let system = root.join("system");
        let filesystem = LocalFileSystem::with_roots(&not_a_directory, &system);
        let selected = filesystem.temporary_directory().unwrap();
        assert!(selected.starts_with(system.join("AssetRipper")));
        let file = filesystem.create_temporary_file().unwrap();
        assert!(filesystem.file_exists(&file));
        filesystem.delete_temporary_directory().unwrap();
        assert!(!selected.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn local_file_path_helpers_normalize_components_and_guid_names() {
        let full = full_path(Path::new("alpha/../beta")).unwrap();
        assert!(full.ends_with("beta"));
        let first = new_guid_string();
        let second = new_guid_string();
        assert_eq!(first.len(), 36);
        assert_eq!(&first[14..15], "4");
        assert!(first != second);
    }

    #[cfg(windows)]
    #[test]
    fn local_filesystem_operations_match_the_frozen_runtime_fixture() {
        let expected: Vec<&str> = include_str!("local_filesystem_fixture.txt")
            .lines()
            .filter(|line| {
                !line.starts_with('#')
                    && [
                        "DEFAULT_TEMP|",
                        "SET_TEMP|",
                        "BLANK_SETTER|",
                        "TEMP_FILE|",
                        "TEMP_DIRECTORY|",
                        "OPEN_WRITE|",
                        "WRITE_WHILE_READ_OPEN|",
                        "DELETE_TEMP|",
                        "DELETE_MISSING_FILE|",
                        "PATH|",
                        "ENUM_FILES|",
                        "ENUMERATE_",
                        "ENUM_DIRS|",
                        "TEXT|",
                        "PATH_HELPERS|",
                    ]
                    .iter()
                    .any(|prefix| line.starts_with(prefix))
            })
            .collect();
        let root = crate::test_support::test_temp_root().join("local-fs-source-fixture");
        let executable = root.join("application");
        let system = root.join("system");
        std::fs::create_dir_all(&executable).unwrap();
        let filesystem = LocalFileSystem::with_roots(&executable, &system);
        let selected = filesystem.temporary_directory().unwrap();
        let local_name = selected.file_name().unwrap().to_string_lossy();
        let mut actual = vec![format!(
            "DEFAULT_TEMP|app-local={}|exists={}",
            selected.starts_with(executable.join("temp")) && is_hex_name(&local_name, 4),
            selected.exists()
        )];

        let configured = root.join("custom").join("..").join("temp");
        filesystem.set_temporary_directory(&configured).unwrap();
        let normalized = filesystem.temporary_directory().unwrap();
        let normalized_string = normalized.to_string_lossy();
        actual.push(format!(
            "SET_TEMP|full={}|normalized={}",
            normalized == full_path(&configured).unwrap(),
            !normalized_string.contains("..")
        ));
        filesystem.set_temporary_directory("   ").unwrap();
        actual.push(format!(
            "BLANK_SETTER|unchanged={}",
            filesystem.temporary_directory().unwrap() == normalized
        ));

        let file = filesystem.create_temporary_file().unwrap();
        actual.push(format!(
            "TEMP_FILE|inside={}|exists={}|length={}|guid={}",
            file.parent() == Some(normalized.as_path()),
            filesystem.file_exists(&file),
            std::fs::metadata(&file).unwrap().len(),
            is_guid_name(&file.file_name().unwrap().to_string_lossy())
        ));

        let child = filesystem.create_temporary_directory().unwrap();
        actual.push(format!(
            "TEMP_DIRECTORY|inside={}|guid8={}|exists={}",
            child.parent() == Some(normalized.as_path()),
            is_hex_name(&child.file_name().unwrap().to_string_lossy(), 8),
            filesystem.directory_exists(&child)
        ));

        let open_write = root.join("openwrite.bin");
        filesystem.write_all_bytes(&open_write, b"abcdef").unwrap();
        let mut writer = filesystem.open_write(&open_write).unwrap();
        let initial_position = writer.stream_position().unwrap();
        writer.write_all(b"XY").unwrap();
        drop(writer);
        actual.push(format!("OPEN_WRITE|initial-position={initial_position}"));
        let open_write_bytes = filesystem.read_all_bytes(&open_write).unwrap();
        actual.push(format!(
            "OPEN_WRITE|bytes={}",
            open_write_bytes
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<String>()
        ));

        let retained = root.join("retained.bin");
        filesystem.write_all_bytes(&retained, b"before").unwrap();
        let reader = filesystem.open_read(&retained).unwrap();
        let write_rejected = filesystem.write_all_bytes(&retained, b"after").is_err();
        drop(reader);
        actual.push(format!(
            "WRITE_WHILE_READ_OPEN|{}",
            if write_rejected {
                "REJECTED"
            } else {
                "ALLOWED"
            }
        ));

        let nested_file = child.join("nested").join("file.bin");
        filesystem
            .create_directory(nested_file.parent().unwrap())
            .unwrap();
        filesystem.write_all_bytes(&nested_file, b"temp").unwrap();
        filesystem.delete_temporary_directory().unwrap();
        actual.push(format!(
            "DELETE_TEMP|removed={}|repeat={}",
            !normalized.exists(),
            filesystem.delete_temporary_directory().is_ok()
        ));

        let missing = root.join("missing");
        let missing_delete =
            filesystem.delete_file(&missing).is_ok() && !filesystem.file_exists(&missing);
        actual.push(format!("DELETE_MISSING_FILE|no-op={missing_delete}"));
        actual.push(format!(
            "PATH|rooted-dot={}|rooted-root={}|full-relative={}",
            filesystem.is_path_rooted("."),
            filesystem.is_path_rooted(std::path::MAIN_SEPARATOR.to_string()),
            filesystem.get_full_path(".").unwrap().is_absolute()
        ));

        let enumeration_root = root.join("enumeration");
        let nested = enumeration_root.join("nested");
        let deep = nested.join("deep");
        let other = enumeration_root.join("other");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        for (path, contents) in [
            (enumeration_root.join("alpha.txt"), "alpha"),
            (enumeration_root.join("BETA.TXT"), "beta"),
            (enumeration_root.join("no-extension"), "none"),
            (enumeration_root.join(".hidden"), "hidden"),
            (nested.join("inner.txt"), "inner"),
            (deep.join("deep.txt"), "deep"),
            (other.join("other.bin"), "other"),
        ] {
            std::fs::write(path, contents).unwrap();
        }
        let relative_list = |paths: Vec<std::path::PathBuf>| {
            let mut paths: Vec<String> = paths
                .into_iter()
                .map(|path| {
                    path.strip_prefix(&enumeration_root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/")
                })
                .collect();
            paths.sort();
            paths.join(",")
        };
        actual.push(format!(
            "ENUM_FILES|all={}",
            relative_list(filesystem.get_files(&enumeration_root).unwrap())
        ));
        actual.push(format!(
            "ENUM_FILES|txt={}",
            relative_list(
                filesystem
                    .get_files_matching(&enumeration_root, "*.txt")
                    .unwrap()
            )
        ));
        actual.push(format!(
            "ENUM_FILES|star-dot-star={}",
            relative_list(
                filesystem
                    .get_files_matching(&enumeration_root, "*.*")
                    .unwrap()
            )
        ));
        actual.push(format!(
            "ENUM_FILES|recursive={}",
            relative_list(
                filesystem
                    .get_files_with_options(
                        &enumeration_root,
                        "*.txt",
                        SearchOption::AllDirectories
                    )
                    .unwrap()
            )
        ));
        actual.push(format!(
            "ENUMERATE_FILES|all={}",
            relative_list(
                filesystem
                    .enumerate_files_in(&enumeration_root)
                    .unwrap()
                    .collect::<io::Result<Vec<_>>>()
                    .unwrap()
            )
        ));
        actual.push(format!(
            "ENUMERATE_FILES|txt={}",
            relative_list(
                filesystem
                    .enumerate_files_matching(&enumeration_root, "*.txt")
                    .unwrap()
                    .collect::<io::Result<Vec<_>>>()
                    .unwrap()
            )
        ));
        actual.push(format!(
            "ENUMERATE_FILES|recursive={}",
            relative_list(
                filesystem
                    .enumerate_files(&enumeration_root, "*.txt", SearchOption::AllDirectories)
                    .unwrap()
                    .collect::<io::Result<Vec<_>>>()
                    .unwrap()
            )
        ));
        actual.push(format!(
            "ENUM_DIRS|top={}",
            relative_list(filesystem.get_directories(&enumeration_root).unwrap())
        ));
        actual.push(format!(
            "ENUM_DIRS|recursive={}",
            relative_list(
                filesystem
                    .get_directories_with_options(
                        &enumeration_root,
                        "*",
                        SearchOption::AllDirectories
                    )
                    .unwrap()
            )
        ));
        actual.push(format!(
            "ENUMERATE_DIRS|top={}",
            relative_list(
                filesystem
                    .enumerate_directories_in(&enumeration_root)
                    .unwrap()
                    .collect::<io::Result<Vec<_>>>()
                    .unwrap()
            )
        ));
        actual.push(format!(
            "ENUMERATE_DIRS|pattern={}",
            relative_list(
                filesystem
                    .enumerate_directories_matching(&enumeration_root, "*")
                    .unwrap()
                    .collect::<io::Result<Vec<_>>>()
                    .unwrap()
            )
        ));
        actual.push(format!(
            "ENUMERATE_DIRS|recursive={}",
            relative_list(
                filesystem
                    .enumerate_directories(&enumeration_root, "*", SearchOption::AllDirectories)
                    .unwrap()
                    .collect::<io::Result<Vec<_>>>()
                    .unwrap()
            )
        ));

        let text_path = root.join("text.txt");
        filesystem.write_all_text(&text_path, "café").unwrap();
        actual.push(format!(
            "TEXT|default-write={}",
            bytes_as_hex(&filesystem.read_all_bytes(&text_path).unwrap())
        ));
        filesystem
            .write_all_bytes(&text_path, b"\xEF\xBB\xBFcaf\xC3\xA9")
            .unwrap();
        actual.push(format!(
            "TEXT|utf8-bom-read={}",
            bytes_as_hex(filesystem.read_all_text(&text_path).unwrap().as_bytes())
        ));
        filesystem
            .write_all_bytes(&text_path, b"\xFF\xFEc\x00\xE9\x00")
            .unwrap();
        actual.push(format!(
            "TEXT|utf16-bom-read={}",
            bytes_as_hex(filesystem.read_all_text(&text_path).unwrap().as_bytes())
        ));
        let explicit_encoding_path = root.join("explicit-encoding.txt");
        filesystem
            .write_all_text_with_encoding(&explicit_encoding_path, "café", TextEncoding::Utf16Le)
            .unwrap();
        actual.push(format!(
            "TEXT|utf16-write={}",
            bytes_as_hex(&filesystem.read_all_bytes(&explicit_encoding_path).unwrap())
        ));

        actual.push(format!(
            "PATH_HELPERS|join={}",
            filesystem.join_paths(&["alpha", "beta", "gamma"])
        ));
        actual.push(format!(
            "PATH_HELPERS|join-rooted={}",
            filesystem.join_paths(&["alpha", "\\beta"])
        ));
        actual.push(format!(
            "PATH_HELPERS|directory={}",
            filesystem
                .get_directory_name("alpha\\beta.ext")
                .unwrap()
                .display()
        ));
        actual.push(format!(
            "PATH_HELPERS|extension={}",
            filesystem.get_extension("alpha\\beta.ext")
        ));
        actual.push(format!(
            "PATH_HELPERS|filename={}",
            filesystem.get_file_name("alpha\\beta.ext")
        ));
        actual.push(format!(
            "PATH_HELPERS|filename-no-extension={}",
            filesystem.get_file_name_without_extension("alpha\\beta.ext")
        ));
        actual.push(format!(
            "PATH_HELPERS|dotfile-extension={}",
            filesystem.get_extension(".hidden")
        ));
        actual.push(format!(
            "PATH_HELPERS|trailing-filename={}",
            filesystem.get_file_name("alpha\\beta\\")
        ));
        actual.push(format!(
            "PATH_HELPERS|rooted-drive-relative={}",
            filesystem.is_path_rooted("C:relative")
        ));
        let relative = filesystem
            .get_relative_path(root.join("base"), root.join("base").join("a").join("b.txt"))
            .unwrap()
            .to_string_lossy()
            .into_owned();
        actual.push(format!("PATH_HELPERS|relative={relative}"));

        let actual: Vec<String> = actual
            .into_iter()
            .map(|line| line.replace("=true", "=True").replace("=false", "=False"))
            .collect();
        assert_eq!(
            actual.iter().map(String::as_str).collect::<Vec<_>>(),
            expected
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
