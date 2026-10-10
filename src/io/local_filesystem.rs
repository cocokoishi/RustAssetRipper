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
    /// System.Text code page 20127, US-ASCII.
    Ascii,
    /// System.Text code page 28591, ISO-8859-1.
    Latin1,
    Utf8,
    /// System.Text code page 65000, UTF-7 (`Encoding.UTF7`).
    Utf7,
    Utf16Le,
    Utf16Be,
    Utf32Le,
    Utf32Be,
}

impl TextEncoding {
    pub const fn code_page(self) -> u32 {
        match self {
            Self::Ascii => 20_127,
            Self::Latin1 => 28_591,
            Self::Utf8 => 65_001,
            Self::Utf7 => 65_000,
            Self::Utf16Le => 1_200,
            Self::Utf16Be => 1_201,
            Self::Utf32Le => 12_000,
            Self::Utf32Be => 12_001,
        }
    }

    pub const fn from_code_page(code_page: u32) -> Option<Self> {
        match code_page {
            20_127 => Some(Self::Ascii),
            28_591 => Some(Self::Latin1),
            65_001 => Some(Self::Utf8),
            65_000 => Some(Self::Utf7),
            1_200 => Some(Self::Utf16Le),
            1_201 => Some(Self::Utf16Be),
            12_000 => Some(Self::Utf32Le),
            12_001 => Some(Self::Utf32Be),
            _ => None,
        }
    }

    fn preamble(self) -> &'static [u8] {
        match self {
            Self::Ascii | Self::Latin1 | Self::Utf7 => &[],
            Self::Utf8 => b"\xEF\xBB\xBF",
            Self::Utf16Le => b"\xFF\xFE",
            Self::Utf16Be => b"\xFE\xFF",
            Self::Utf32Le => b"\xFF\xFE\x00\x00",
            Self::Utf32Be => b"\x00\x00\xFE\xFF",
        }
    }
}

/// Fallback behavior corresponding to `EncoderFallback` or `DecoderFallback`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TextFallback {
    /// Emit these characters for each invalid sequence or unrepresentable UTF-16 unit.
    Replacement(String),
    /// Fail instead of replacing the invalid or unrepresentable unit.
    Exception,
    /// UTF-7's default decoder maps an invalid input byte to the same-valued Latin-1 character.
    /// This behavior is distinct from its replacement fallback when a caller customizes it.
    Utf7DefaultDecoder,
}

/// Encoder and decoder fallback policies carried by a `System.Text.Encoding` value.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextEncodingFallbacks {
    pub encoder: TextFallback,
    pub decoder: TextFallback,
}

impl Default for TextEncodingFallbacks {
    fn default() -> Self {
        Self::for_encoding(TextEncoding::Utf8)
    }
}

impl TextEncodingFallbacks {
    /// Returns the fallback strings used by the source runtime for this encoding's standard
    /// instance. In particular, US-ASCII uses `?` for both decoder and encoder replacement.
    pub fn for_encoding(encoding: TextEncoding) -> Self {
        Self {
            encoder: TextFallback::Replacement("?".to_owned()),
            decoder: match encoding {
                TextEncoding::Ascii => TextFallback::Replacement("?".to_owned()),
                TextEncoding::Utf7 => TextFallback::Utf7DefaultDecoder,
                _ => TextFallback::Replacement("\u{FFFD}".to_owned()),
            },
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
        self.read_all_text_with_encoding(path, TextEncoding::Utf8)
    }

    /// Reads complete text with the supplied fallback encoding and BOM detection.
    pub fn read_all_text_with_encoding(
        &self,
        path: impl AsRef<Path>,
        encoding: TextEncoding,
    ) -> io::Result<String> {
        self.read_all_text_with_encoding_and_fallbacks(
            path,
            encoding,
            &TextEncodingFallbacks::for_encoding(encoding),
        )
    }

    /// Reads text with explicit encoder/decoder fallback policies and source-compatible BOM
    /// detection. When a BOM selects a different encoding, the detected encoding's default
    /// decoder fallback is used; a matching BOM keeps the supplied encoding and its fallback.
    pub fn read_all_text_with_encoding_and_fallbacks(
        &self,
        path: impl AsRef<Path>,
        encoding: TextEncoding,
        fallbacks: &TextEncodingFallbacks,
    ) -> io::Result<String> {
        decode_text_with_fallbacks(&self.read_all_bytes(path)?, encoding, fallbacks, true)
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
        self.write_all_text_with_encoding_and_fallbacks(
            path,
            contents,
            encoding,
            &TextEncodingFallbacks::for_encoding(encoding),
        )
    }

    /// Writes text using explicit encoder/decoder fallback policies and the selected encoding's
    /// preamble, as the `File.WriteAllText(path, contents, encoding)` overload does.
    pub fn write_all_text_with_encoding_and_fallbacks(
        &self,
        path: impl AsRef<Path>,
        contents: &str,
        encoding: TextEncoding,
        fallbacks: &TextEncodingFallbacks,
    ) -> io::Result<()> {
        // `File.WriteAllText(path, contents, encoding)` opens with create/truncate before its
        // StreamWriter encodes the supplied text. Preserve that observable empty-file result
        // when an exception fallback rejects an unrepresentable character.
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        let mut file = open_with_share(path.as_ref(), &mut options, FileShare::Read)?;
        let bytes = encode_text_with_fallbacks(contents, encoding, fallbacks, true)?;
        file.write_all(&bytes)
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
        if root.as_os_str().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "directory enumeration path must not be empty",
            ));
        }
        // System.IO.Directory.EnumerateFiles/EnumerateDirectories validates that the path
        // exists when the enumerable is requested, while it defers opening the iterator until
        // MoveNext. Keep the metadata check separate from read_dir so files still fail lazily.
        fs::metadata(root)?;
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
                    // A directory can disappear after the source-compatible call-time
                    // existence check. The .NET iterator yields no items in that case.
                    if error.kind() == io::ErrorKind::NotFound {
                        return None;
                    }
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

/// Decodes text while keeping the two source entry points distinct: local `File.ReadAllText`
/// performs Unicode BOM detection, while the virtual filesystem calls `Encoding.GetString`
/// directly and therefore does not remove a leading U+FEFF.
pub(crate) fn decode_text_with_fallbacks(
    bytes: &[u8],
    fallback_encoding: TextEncoding,
    fallbacks: &TextEncodingFallbacks,
    detect_preamble: bool,
) -> io::Result<String> {
    let detected = detect_preamble
        .then(|| detect_text_preamble(bytes))
        .flatten();
    let (encoding, content, detected_fallbacks) = match detected {
        Some((encoding, preamble_length)) => (
            encoding,
            &bytes[preamble_length..],
            (encoding != fallback_encoding).then(|| TextEncodingFallbacks::for_encoding(encoding)),
        ),
        None => (fallback_encoding, bytes, None),
    };
    let active_fallbacks = detected_fallbacks.as_ref().unwrap_or(fallbacks);

    match encoding {
        TextEncoding::Ascii => decode_single_byte(content, encoding, active_fallbacks),
        TextEncoding::Latin1 => decode_latin1(content),
        TextEncoding::Utf8 => decode_utf8(content, encoding, active_fallbacks),
        TextEncoding::Utf7 => decode_utf7(content, active_fallbacks),
        TextEncoding::Utf16Le => decode_utf16(content, true, encoding, active_fallbacks),
        TextEncoding::Utf16Be => decode_utf16(content, false, encoding, active_fallbacks),
        TextEncoding::Utf32Le => decode_utf32(content, true, encoding, active_fallbacks),
        TextEncoding::Utf32Be => decode_utf32(content, false, encoding, active_fallbacks),
    }
}

fn detect_text_preamble(bytes: &[u8]) -> Option<(TextEncoding, usize)> {
    if bytes.starts_with(b"\x00\x00\xFE\xFF") {
        Some((TextEncoding::Utf32Be, 4))
    } else if bytes.starts_with(b"\xFF\xFE\x00\x00") {
        Some((TextEncoding::Utf32Le, 4))
    } else if bytes.starts_with(b"\xEF\xBB\xBF") {
        Some((TextEncoding::Utf8, 3))
    } else if bytes.starts_with(b"\xFE\xFF") {
        Some((TextEncoding::Utf16Be, 2))
    } else if bytes.starts_with(b"\xFF\xFE") {
        Some((TextEncoding::Utf16Le, 2))
    } else {
        None
    }
}

fn decoder_error(code_page: u32, byte_offset: usize, invalid_length: usize) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "decoder fallback for code page {code_page} at byte offset {byte_offset} ({invalid_length} invalid byte(s))"
        ),
    )
}

fn encoder_error(code_page: u32, utf16_offset: usize) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("encoder fallback for code page {code_page} at UTF-16 offset {utf16_offset}"),
    )
}

fn reserve_string(output: &mut String, additional: usize) -> io::Result<()> {
    output.try_reserve(additional).map_err(|error| {
        io::Error::new(
            io::ErrorKind::OutOfMemory,
            format!("could not reserve decoded text: {error}"),
        )
    })
}

fn reserve_bytes(output: &mut Vec<u8>, additional: usize) -> io::Result<()> {
    output.try_reserve(additional).map_err(|error| {
        io::Error::new(
            io::ErrorKind::OutOfMemory,
            format!("could not reserve encoded text: {error}"),
        )
    })
}

fn append_decoder_fallback(
    output: &mut String,
    fallbacks: &TextEncodingFallbacks,
    encoding: TextEncoding,
    byte_offset: usize,
    invalid_length: usize,
) -> io::Result<()> {
    match &fallbacks.decoder {
        TextFallback::Replacement(replacement) => {
            reserve_string(output, replacement.len())?;
            output.push_str(replacement);
            Ok(())
        }
        TextFallback::Exception => Err(decoder_error(
            encoding.code_page(),
            byte_offset,
            invalid_length,
        )),
        TextFallback::Utf7DefaultDecoder => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "UTF-7 default byte fallback used outside the UTF-7 decoder",
        )),
    }
}

fn append_valid_text(output: &mut String, text: &str) -> io::Result<()> {
    reserve_string(output, text.len())?;
    output.push_str(text);
    Ok(())
}

fn decode_single_byte(
    bytes: &[u8],
    encoding: TextEncoding,
    fallbacks: &TextEncodingFallbacks,
) -> io::Result<String> {
    let mut output = String::new();
    reserve_string(&mut output, bytes.len())?;
    for (offset, byte) in bytes.iter().copied().enumerate() {
        if byte <= 0x7f {
            output.push(char::from(byte));
        } else {
            append_decoder_fallback(&mut output, fallbacks, encoding, offset, 1)?;
        }
    }
    Ok(output)
}

fn decode_latin1(bytes: &[u8]) -> io::Result<String> {
    let capacity = bytes.len().checked_mul(2).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::OutOfMemory,
            "decoded Latin-1 text is too large",
        )
    })?;
    let mut output = String::new();
    reserve_string(&mut output, capacity)?;
    for byte in bytes {
        output.push(char::from(*byte));
    }
    Ok(output)
}

fn decode_utf8(
    bytes: &[u8],
    encoding: TextEncoding,
    fallbacks: &TextEncodingFallbacks,
) -> io::Result<String> {
    let mut output = String::new();
    reserve_string(&mut output, bytes.len())?;
    let mut offset = 0;
    while offset < bytes.len() {
        match std::str::from_utf8(&bytes[offset..]) {
            Ok(text) => {
                append_valid_text(&mut output, text)?;
                break;
            }
            Err(error) => {
                let valid_length = error.valid_up_to();
                if valid_length > 0 {
                    let valid = std::str::from_utf8(&bytes[offset..offset + valid_length])
                        .expect("Utf8Error::valid_up_to identifies valid text");
                    append_valid_text(&mut output, valid)?;
                }
                let invalid_length = error
                    .error_len()
                    .unwrap_or(bytes.len() - offset - valid_length);
                append_decoder_fallback(
                    &mut output,
                    fallbacks,
                    encoding,
                    offset + valid_length,
                    invalid_length,
                )?;
                offset += valid_length + invalid_length;
            }
        }
    }
    Ok(output)
}

fn utf7_base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn append_utf7_invalid_byte(
    output: &mut String,
    fallbacks: &TextEncodingFallbacks,
    byte: u8,
    offset: usize,
) -> io::Result<()> {
    match &fallbacks.decoder {
        TextFallback::Utf7DefaultDecoder => {
            reserve_string(output, 2)?;
            output.push(char::from(byte));
            Ok(())
        }
        TextFallback::Replacement(replacement) => {
            reserve_string(output, replacement.len())?;
            output.push_str(replacement);
            Ok(())
        }
        TextFallback::Exception => Err(decoder_error(65_000, offset, 1)),
    }
}

fn append_utf7_unit(
    output: &mut String,
    pending_high: &mut Option<u16>,
    unit: u16,
) -> io::Result<()> {
    if let Some(high) = pending_high.take() {
        if (0xDC00..=0xDFFF).contains(&unit) {
            let scalar = 0x1_0000 + ((u32::from(high) - 0xD800) << 10) + (u32::from(unit) - 0xDC00);
            let character = char::from_u32(scalar).expect("UTF-7 surrogate pair is a scalar");
            reserve_string(output, character.len_utf8())?;
            output.push(character);
            return Ok(());
        }
        reserve_string(output, char::REPLACEMENT_CHARACTER.len_utf8())?;
        output.push(char::REPLACEMENT_CHARACTER);
    }

    if (0xD800..=0xDBFF).contains(&unit) {
        *pending_high = Some(unit);
    } else if (0xDC00..=0xDFFF).contains(&unit) {
        reserve_string(output, char::REPLACEMENT_CHARACTER.len_utf8())?;
        output.push(char::REPLACEMENT_CHARACTER);
    } else {
        let character =
            char::from_u32(u32::from(unit)).expect("non-surrogate UTF-7 unit is a scalar");
        reserve_string(output, character.len_utf8())?;
        output.push(character);
    }
    Ok(())
}

fn decode_utf7(bytes: &[u8], fallbacks: &TextEncodingFallbacks) -> io::Result<String> {
    let mut output = String::new();
    reserve_string(&mut output, bytes.len())?;
    let mut offset = 0;
    while offset < bytes.len() {
        let byte = bytes[offset];
        if byte != b'+' {
            if byte <= 0x7f {
                output.push(char::from(byte));
            } else {
                append_utf7_invalid_byte(&mut output, fallbacks, byte, offset)?;
            }
            offset += 1;
            continue;
        }

        if bytes.len() - offset < 2 {
            // The source UTF7 decoder discards a terminal shift introducer.
            break;
        }
        if bytes[offset + 1] == b'-' {
            output.push('+');
            offset += 2;
            continue;
        }
        if utf7_base64_value(bytes[offset + 1]).is_none() {
            // A malformed shift falls back the first non-base64 byte and resumes in direct mode.
            append_utf7_invalid_byte(&mut output, fallbacks, bytes[offset + 1], offset + 1)?;
            offset += 2;
            continue;
        }

        let mut cursor = offset + 1;
        let mut bit_buffer = 0u32;
        let mut bit_count = 0u32;
        let mut pending_high = None;
        while cursor < bytes.len() {
            let Some(value) = utf7_base64_value(bytes[cursor]) else {
                break;
            };
            bit_buffer = (bit_buffer << 6) | u32::from(value);
            bit_count += 6;
            if bit_count >= 16 {
                bit_count -= 16;
                let unit = ((bit_buffer >> bit_count) & 0xffff) as u16;
                bit_buffer &= (1u32 << bit_count).wrapping_sub(1);
                append_utf7_unit(&mut output, &mut pending_high, unit)?;
            }
            cursor += 1;
        }
        if pending_high.is_some() {
            // UTF7Encoding emits U+FFFD for an unmatched decoded surrogate, independent of the
            // caller's byte DecoderFallback.
            reserve_string(&mut output, char::REPLACEMENT_CHARACTER.len_utf8())?;
            output.push(char::REPLACEMENT_CHARACTER);
        }
        if cursor < bytes.len() && bytes[cursor] == b'-' {
            cursor += 1;
        }
        offset = cursor;
    }
    Ok(output)
}

fn is_utf7_direct(character: char) -> bool {
    matches!(
        character,
        ' '
            | '\''
            | '('..=')'
            | ','..='/'
            | '0'..='9'
            | ':'
            | '?'
            | 'A'..='Z'
            | 'a'..='z'
    )
}

fn push_utf7_base64(output: &mut Vec<u8>, value: u32) -> io::Result<()> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    reserve_bytes(output, 1)?;
    output.push(ALPHABET[(value & 0x3f) as usize]);
    Ok(())
}

fn append_utf7_unit_to_shift(
    output: &mut Vec<u8>,
    bit_buffer: &mut u32,
    bit_count: &mut u32,
    unit: u16,
) -> io::Result<()> {
    *bit_buffer = (*bit_buffer << 16) | u32::from(unit);
    *bit_count += 16;
    while *bit_count >= 6 {
        *bit_count -= 6;
        push_utf7_base64(output, *bit_buffer >> *bit_count)?;
        *bit_buffer &= (1u32 << *bit_count).wrapping_sub(1);
    }
    Ok(())
}

fn finish_utf7_shift(
    output: &mut Vec<u8>,
    in_shift: &mut bool,
    bit_buffer: &mut u32,
    bit_count: &mut u32,
) -> io::Result<()> {
    if !*in_shift {
        return Ok(());
    }
    if *bit_count > 0 {
        push_utf7_base64(output, *bit_buffer << (6 - *bit_count))?;
    }
    reserve_bytes(output, 1)?;
    output.push(b'-');
    *in_shift = false;
    *bit_buffer = 0;
    *bit_count = 0;
    Ok(())
}

fn encode_utf7(text: &str, output: &mut Vec<u8>) -> io::Result<()> {
    let mut in_shift = false;
    let mut bit_buffer = 0u32;
    let mut bit_count = 0u32;
    for character in text.chars() {
        if character == '+' && in_shift {
            append_utf7_unit_to_shift(output, &mut bit_buffer, &mut bit_count, u16::from(b'+'))?;
        } else if character == '+' {
            finish_utf7_shift(output, &mut in_shift, &mut bit_buffer, &mut bit_count)?;
            reserve_bytes(output, 2)?;
            output.extend_from_slice(b"+-");
        } else if is_utf7_direct(character) {
            finish_utf7_shift(output, &mut in_shift, &mut bit_buffer, &mut bit_count)?;
            reserve_bytes(output, 1)?;
            output.push(character as u8);
        } else {
            if !in_shift {
                reserve_bytes(output, 1)?;
                output.push(b'+');
                in_shift = true;
            }
            let mut units = [0u16; 2];
            for unit in character.encode_utf16(&mut units).iter().copied() {
                append_utf7_unit_to_shift(output, &mut bit_buffer, &mut bit_count, unit)?;
            }
        }
    }
    finish_utf7_shift(output, &mut in_shift, &mut bit_buffer, &mut bit_count)
}

fn decode_utf16(
    bytes: &[u8],
    little_endian: bool,
    encoding: TextEncoding,
    fallbacks: &TextEncodingFallbacks,
) -> io::Result<String> {
    let mut output = String::new();
    reserve_string(&mut output, bytes.len())?;
    let mut offset = 0;
    while bytes.len() - offset >= 2 {
        let unit = read_u16(&bytes[offset..offset + 2], little_endian);
        if (0xD800..=0xDBFF).contains(&unit) {
            if bytes.len() - offset >= 4 {
                let low = read_u16(&bytes[offset + 2..offset + 4], little_endian);
                if (0xDC00..=0xDFFF).contains(&low) {
                    let scalar =
                        0x1_0000 + ((u32::from(unit) - 0xD800) << 10) + (u32::from(low) - 0xDC00);
                    let character =
                        char::from_u32(scalar).expect("surrogate pair is a Unicode scalar");
                    reserve_string(&mut output, character.len_utf8())?;
                    output.push(character);
                    offset += 4;
                    continue;
                }
            }
            append_decoder_fallback(&mut output, fallbacks, encoding, offset, 2)?;
        } else if (0xDC00..=0xDFFF).contains(&unit) {
            append_decoder_fallback(&mut output, fallbacks, encoding, offset, 2)?;
        } else {
            let character = char::from_u32(u32::from(unit)).expect("non-surrogate u16 is a scalar");
            reserve_string(&mut output, character.len_utf8())?;
            output.push(character);
        }
        offset += 2;
    }
    if offset < bytes.len() {
        append_decoder_fallback(
            &mut output,
            fallbacks,
            encoding,
            offset,
            bytes.len() - offset,
        )?;
    }
    Ok(output)
}

fn read_u16(bytes: &[u8], little_endian: bool) -> u16 {
    let pair = [bytes[0], bytes[1]];
    if little_endian {
        u16::from_le_bytes(pair)
    } else {
        u16::from_be_bytes(pair)
    }
}

fn decode_utf32(
    bytes: &[u8],
    little_endian: bool,
    encoding: TextEncoding,
    fallbacks: &TextEncodingFallbacks,
) -> io::Result<String> {
    let mut output = String::new();
    reserve_string(&mut output, bytes.len())?;
    let mut offset = 0;
    while bytes.len() - offset >= 4 {
        let chunk = [
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ];
        let value = if little_endian {
            u32::from_le_bytes(chunk)
        } else {
            u32::from_be_bytes(chunk)
        };
        if let Some(character) = char::from_u32(value) {
            output.push(character);
        } else {
            append_decoder_fallback(&mut output, fallbacks, encoding, offset, 4)?;
        }
        offset += 4;
    }
    if offset < bytes.len() {
        append_decoder_fallback(
            &mut output,
            fallbacks,
            encoding,
            offset,
            bytes.len() - offset,
        )?;
    }
    Ok(output)
}

fn append_encoder_fallback(
    output: &mut Vec<u8>,
    fallbacks: &TextEncodingFallbacks,
    encoding: TextEncoding,
    utf16_offset: usize,
) -> io::Result<()> {
    match &fallbacks.encoder {
        TextFallback::Exception => Err(encoder_error(encoding.code_page(), utf16_offset)),
        TextFallback::Replacement(replacement) => {
            let maximum = match encoding {
                TextEncoding::Ascii => 0x7f,
                TextEncoding::Latin1 => 0xff,
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "encoder fallback requested for a Unicode encoding",
                    ));
                }
            };
            let mut replacement_units = 0usize;
            for unit in replacement.encode_utf16() {
                if u32::from(unit) > maximum {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!(
                            "encoder fallback replacement is not representable in code page {}; recursive fallback at UTF-16 offset {utf16_offset}",
                            encoding.code_page()
                        ),
                    ));
                }
                replacement_units = replacement_units.checked_add(1).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::OutOfMemory, "encoder fallback is too large")
                })?;
            }
            reserve_bytes(output, replacement_units)?;
            output.extend(replacement.encode_utf16().map(|unit| unit as u8));
            Ok(())
        }
        TextFallback::Utf7DefaultDecoder => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "UTF-7 decoder fallback cannot be used as an encoder fallback",
        )),
    }
}

/// Encodes text for either `File.WriteAllText` (with its preamble) or the virtual filesystem
/// `Encoding.GetBytes` overload (without a preamble).
pub(crate) fn encode_text_with_fallbacks(
    text: &str,
    encoding: TextEncoding,
    fallbacks: &TextEncodingFallbacks,
    include_preamble: bool,
) -> io::Result<Vec<u8>> {
    let preamble = if include_preamble {
        encoding.preamble()
    } else {
        &[]
    };
    let mut output = Vec::new();
    reserve_bytes(&mut output, preamble.len())?;
    output.extend_from_slice(preamble);

    match encoding {
        TextEncoding::Ascii | TextEncoding::Latin1 => {
            let maximum = if encoding == TextEncoding::Ascii {
                0x7f
            } else {
                0xff
            };
            for (utf16_offset, unit) in text.encode_utf16().enumerate() {
                if u32::from(unit) <= maximum {
                    reserve_bytes(&mut output, 1)?;
                    output.push(unit as u8);
                } else {
                    append_encoder_fallback(&mut output, fallbacks, encoding, utf16_offset)?;
                }
            }
        }
        TextEncoding::Utf8 => {
            reserve_bytes(&mut output, text.len())?;
            output.extend_from_slice(text.as_bytes());
        }
        TextEncoding::Utf7 => encode_utf7(text, &mut output)?,
        TextEncoding::Utf16Le | TextEncoding::Utf16Be => {
            let little_endian = encoding == TextEncoding::Utf16Le;
            for unit in text.encode_utf16() {
                let bytes = if little_endian {
                    unit.to_le_bytes()
                } else {
                    unit.to_be_bytes()
                };
                reserve_bytes(&mut output, bytes.len())?;
                output.extend_from_slice(&bytes);
            }
        }
        TextEncoding::Utf32Le | TextEncoding::Utf32Be => {
            let little_endian = encoding == TextEncoding::Utf32Le;
            for character in text.chars() {
                let value = u32::from(character);
                let bytes = if little_endian {
                    value.to_le_bytes()
                } else {
                    value.to_be_bytes()
                };
                reserve_bytes(&mut output, bytes.len())?;
                output.extend_from_slice(&bytes);
            }
        }
    }
    Ok(output)
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
    include!("local_filesystem_tests_01.rs");
}
