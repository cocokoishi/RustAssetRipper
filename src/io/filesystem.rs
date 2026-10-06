//! In-memory virtual filesystem used by containers and recovered Unity files.
//!
//! Covers the `VirtualFileSystem` CRUD, enumeration, `Count`, `Clear`, `ReadAllBytes`/
//! `WriteAllBytes`, UTF-8 `ReadAllText`/`WriteAllText`, and `TemporaryDirectory` rules.
//!
//! Deliberately open: the `Encoding` text overloads (only UTF-8 ports), GUID-named
//! temporary file/directory creation, `DeleteTemporaryDirectory` (which needs the
//! still-unported recursive directory delete), real-filesystem mounting, and the
//! `GetUniqueName` uniqueness-counter loop with its reserved-name gate (both need
//! live directory state; only the length stage ports, as
//! `truncate_file_name_to_byte_length`).

use std::collections::BTreeMap;
use std::io::{self, Read, Seek, SeekFrom, Write};

use crate::io::stream::{MultiFileStream, SmartStream};

/// Resource limits for paths and entries held by [`VirtualFileSystem`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VirtualFileSystemLimits {
    pub max_path_bytes: usize,
    pub max_path_components: usize,
    /// Includes the root directory, matching the source `VirtualFileSystem.Count` behavior.
    pub max_entries: usize,
}

impl Default for VirtualFileSystemLimits {
    fn default() -> Self {
        Self {
            max_path_bytes: 1024 * 1024,
            max_path_components: 4096,
            max_entries: 1_000_001,
        }
    }
}

/// Whether directory enumeration visits only immediate children or the full subtree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SearchOption {
    TopDirectoryOnly,
    AllDirectories,
}

/// `FileSystem.GetUniqueName` length stage: when the UTF-8 byte count of `file_name`
/// exceeds `max_name_length`, the extension is preserved and the stem is truncated
/// to fit, backing off to the last full UTF-8 sequence
/// (`Utf8Truncation.TruncateToUTF8ByteLength`). Names within budget pass through
/// untouched.
///
/// The extension follows `System.IO.Path.GetExtension` (the dot-inclusive suffix of
/// the last path segment, empty when there is no dot). The uniqueness-counter loop
/// and the reserved-name gate stay caller-side: they need live directory state.
/// One deliberate difference is documented, not changed: a budget the extension
/// alone already exceeds saturates to an empty stem here, while the source
/// negative length throws; real callers always pass room for the extension.
pub fn truncate_file_name_to_byte_length(file_name: &str, max_name_length: usize) -> String {
    if file_name.len() <= max_name_length {
        return file_name.to_owned();
    }
    let extension = file_extension(file_name);
    let stem = &file_name[..file_name.len() - extension.len()];
    let stem_budget = max_name_length.saturating_sub(extension.len());
    format!(
        "{}{extension}",
        truncate_to_utf8_byte_length(stem, stem_budget)
    )
}

/// The dot-inclusive suffix of the last path segment, or empty when it has no dot.
fn file_extension(file_name: &str) -> &str {
    let segment = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    match segment.rfind('.') {
        Some(dot) => &segment[dot..],
        None => "",
    }
}

/// `Utf8Truncation.TruncateToUTF8ByteLength`: keep at most `max_length` bytes,
/// backing off over a split multi-byte sequence to its start.
fn truncate_to_utf8_byte_length(text: &str, max_length: usize) -> &str {
    let bytes = text.as_bytes();
    if max_length >= bytes.len() {
        return text;
    }
    // The next byte starts a sequence, so the boundary is already valid.
    if bytes[max_length] & 0b1100_0000 != 0b1000_0000 {
        return &text[..max_length];
    }
    // Otherwise back off over the continuation bytes and the sequence start.
    // The ASCII arm below is unreachable for valid UTF-8 (a continuation byte
    // always follows a sequence start) and mirrors the source loop as written.
    let mut valid_length = max_length;
    for index in (0..max_length).rev() {
        let byte = bytes[index];
        if byte & 0b1100_0000 == 0b1000_0000 {
            valid_length -= 1;
        } else {
            if byte & 0b1000_0000 == 0b1000_0000 {
                valid_length -= 1;
            }
            break;
        }
    }
    &text[..valid_length]
}

#[derive(Default)]
struct DirectoryEntry {
    directories: BTreeMap<String, DirectoryEntry>,
    files: BTreeMap<String, SmartStream>,
}

/// Case-sensitive, slash-normalized in-memory filesystem.
///
/// Directory creation is recursive. File creation requires its parent directory to exist,
/// matching the local AssetRipper virtual filesystem. Open handles retain their stream after a
/// file is deleted or the filesystem is cleared.
pub struct VirtualFileSystem {
    root: DirectoryEntry,
    entry_count: usize,
    limits: VirtualFileSystemLimits,
    /// The `TemporaryDirectory` path backing temporary-file creation (default `"/temp"`).
    temporary_directory: String,
}

impl VirtualFileSystem {
    pub fn new() -> Self {
        Self {
            root: DirectoryEntry::default(),
            entry_count: 1,
            limits: VirtualFileSystemLimits::default(),
            temporary_directory: String::from("/temp"),
        }
    }

    pub fn with_limits(limits: VirtualFileSystemLimits) -> io::Result<Self> {
        if limits.max_entries == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "virtual filesystem entry limit must include the root directory",
            ));
        }
        Ok(Self {
            root: DirectoryEntry::default(),
            entry_count: 1,
            limits,
            temporary_directory: String::from("/temp"),
        })
    }

    /// Number of files and directories, including the root directory.
    pub fn count(&self) -> usize {
        self.entry_count
    }

    pub fn limits(&self) -> VirtualFileSystemLimits {
        self.limits
    }

    /// Removes every file and directory while leaving the root directory present.
    pub fn clear(&mut self) {
        self.root.directories.clear();
        self.root.files.clear();
        self.entry_count = 1;
    }

    /// The `TemporaryDirectory` path backing temporary-file creation.
    pub fn temporary_directory(&self) -> &str {
        &self.temporary_directory
    }

    /// Set `TemporaryDirectory`, mirroring the source setter: blank or whitespace-only
    /// values are ignored; anything else is stored as a full path, resolved against the
    /// process working directory when it is not already rooted. A working directory
    /// that cannot be read leaves a relative value stored as given.
    pub fn set_temporary_directory(&mut self, value: &str) {
        if value.trim().is_empty() {
            return;
        }
        if value.starts_with('/') || value.starts_with('\\') {
            self.temporary_directory = value.to_owned();
            return;
        }
        match std::env::current_dir() {
            Ok(current) => {
                let mut joined = current.to_string_lossy().into_owned();
                if !joined.ends_with('/') && !joined.ends_with('\\') {
                    joined.push('/');
                }
                joined.push_str(value);
                self.temporary_directory = joined;
            }
            Err(_) => self.temporary_directory = value.to_owned(),
        }
    }

    /// Creates all directories named by `path`; an empty or root path is a no-op.
    pub fn create_directory(&mut self, path: Option<&str>) -> io::Result<()> {
        let normalized = normalize_path(path, self.limits)?;
        let parts = path_parts(&normalized, self.limits)?;

        let mut existing = &self.root;
        let mut missing_count = 0usize;
        for (index, part) in parts.iter().enumerate() {
            match existing.directories.get(*part) {
                Some(directory) => existing = directory,
                None => {
                    missing_count = parts.len() - index;
                    break;
                }
            }
        }
        let resulting_count = self
            .entry_count
            .checked_add(missing_count)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "entry count overflows"))?;
        if resulting_count > self.limits.max_entries {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "creating {missing_count} directories would exceed virtual filesystem entry limit {}",
                    self.limits.max_entries
                ),
            ));
        }

        let mut current = &mut self.root;
        let entry_count = &mut self.entry_count;

        for part in parts {
            if !current.directories.contains_key(part) {
                current
                    .directories
                    .insert(part.to_owned(), DirectoryEntry::default());
                *entry_count += 1;
            }
            current = current
                .directories
                .get_mut(part)
                .expect("directory was inserted or checked above");
        }
        Ok(())
    }

    pub fn directory_exists(&self, path: Option<&str>) -> io::Result<bool> {
        let normalized = normalize_path(path, self.limits)?;
        let parts = path_parts(&normalized, self.limits)?;
        Ok(find_directory(&self.root, &parts).is_some())
    }

    pub fn file_exists(&self, path: Option<&str>) -> io::Result<bool> {
        let normalized = normalize_path(path, self.limits)?;
        let parts = path_parts(&normalized, self.limits)?;
        let Some((file_name, directories)) = parts.split_last() else {
            return Ok(false);
        };
        Ok(find_directory(&self.root, directories)
            .is_some_and(|directory| directory.files.contains_key(*file_name)))
    }

    /// Opens a file for reading and writing, creating it or truncating an existing file.
    pub fn create_file(&mut self, path: &str) -> io::Result<SmartStream> {
        let normalized = normalize_path(Some(path), self.limits)?;
        let parts = path_parts(&normalized, self.limits)?;
        let Some((file_name, directories)) = parts.split_last() else {
            return Err(invalid_path("file path cannot be empty"));
        };
        let max_entries = self.limits.max_entries;
        let directory = find_directory_mut(&mut self.root, directories).ok_or_else(|| {
            not_found(format!(
                "parent directory of '{}' does not exist",
                canonical_path(&parts)
            ))
        })?;

        if let Some(stream) = directory.files.get(*file_name) {
            stream.set_len(0)?;
            return Ok(stream.create_reference());
        }

        ensure_entry_capacity(self.entry_count, max_entries)?;
        let stream = SmartStream::create_memory();
        directory
            .files
            .insert((*file_name).to_owned(), stream.clone());
        self.entry_count += 1;
        Ok(stream)
    }

    /// Inserts an already-owned readable stream, creating missing parent directories as one
    /// entry-limit-checked operation. Existing files are never silently replaced.
    pub fn insert_file_stream_with_parents(
        &mut self,
        path: &str,
        stream: SmartStream,
    ) -> io::Result<()> {
        if stream.is_null() || !stream.can_read() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "inserted virtual file stream must be readable and non-null",
            ));
        }
        let normalized = normalize_path(Some(path), self.limits)?;
        let parts = path_parts(&normalized, self.limits)?;
        let Some((file_name, directories)) = parts.split_last() else {
            return Err(invalid_path("file path cannot be empty"));
        };

        let mut existing = &self.root;
        let mut missing_start = None;
        for (index, part) in directories.iter().enumerate() {
            match existing.directories.get(*part) {
                Some(directory) => existing = directory,
                None => {
                    missing_start = Some(index);
                    break;
                }
            }
        }
        if missing_start.is_none() && existing.files.contains_key(*file_name) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("virtual file '{}' already exists", canonical_path(&parts)),
            ));
        }
        let missing_directory_count = missing_start
            .map(|index| directories.len() - index)
            .unwrap_or(0);
        let added_entries = missing_directory_count.checked_add(1).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "virtual entry count overflows")
        })?;
        let resulting_count = self.entry_count.checked_add(added_entries).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "virtual entry count overflows")
        })?;
        if resulting_count > self.limits.max_entries {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "inserting the file and {missing_directory_count} parent directories would exceed virtual filesystem entry limit {}",
                    self.limits.max_entries
                ),
            ));
        }

        let mut current = &mut self.root;
        let entry_count = &mut self.entry_count;
        for part in directories {
            if !current.directories.contains_key(*part) {
                current
                    .directories
                    .insert((*part).to_owned(), DirectoryEntry::default());
                *entry_count += 1;
            }
            current = current
                .directories
                .get_mut(*part)
                .expect("directory was inserted or checked above");
        }
        current.files.insert((*file_name).to_owned(), stream);
        *entry_count += 1;
        Ok(())
    }

    pub fn open_file(&self, path: &str) -> io::Result<SmartStream> {
        let normalized = normalize_path(Some(path), self.limits)?;
        let parts = path_parts(&normalized, self.limits)?;
        let Some((file_name, directories)) = parts.split_last() else {
            return Err(invalid_path("file path cannot be empty"));
        };
        let full_path = canonical_path(&parts);
        find_directory(&self.root, directories)
            .and_then(|directory| directory.files.get(*file_name))
            .map(SmartStream::create_reference)
            .ok_or_else(|| not_found(format!("file '{full_path}' not found")))
    }

    /// Opens a direct file or joins a contiguous `.splitN` group from this virtual filesystem.
    pub fn open_logical_file(&self, path: &str) -> io::Result<SmartStream> {
        let normalized = normalize_path(Some(path), self.limits)?;
        let split_base = logical_split_base(&normalized);
        let base_path = split_base.unwrap_or(&normalized);
        if split_base.is_none() {
            match self.open_file(&normalized) {
                Ok(stream) => return Ok(stream),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }

        let split_parts = self.find_contiguous_split_parts(base_path)?;
        if split_parts.is_empty() {
            return Err(not_found(format!("logical file '{base_path}' not found")));
        }
        let mut streams = Vec::new();
        streams.try_reserve_exact(split_parts.len()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::OutOfMemory,
                "could not allocate logical file stream parts",
            )
        })?;
        for part_path in split_parts {
            streams.push(self.open_file(&part_path)?);
        }
        let multi = MultiFileStream::new_with_max_parts(streams, self.limits.max_entries)?;
        SmartStream::from_reader(Box::new(multi))
    }

    /// Checks whether a direct file or a contiguous `.splitN` group exists.
    pub fn logical_file_exists(&self, path: &str) -> io::Result<bool> {
        let normalized = normalize_path(Some(path), self.limits)?;
        let split_base = logical_split_base(&normalized);
        let base_path = split_base.unwrap_or(&normalized);
        if split_base.is_none() && self.file_exists(Some(&normalized))? {
            return Ok(true);
        }
        match self.find_contiguous_split_parts(base_path) {
            Ok(parts) => Ok(!parts.is_empty()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn find_contiguous_split_parts(&self, base_path: &str) -> io::Result<Vec<String>> {
        let (directory, base_name) = base_path
            .rsplit_once('/')
            .map_or((None, base_path), |(directory, name)| {
                ((!directory.is_empty()).then_some(directory), name)
            });
        if base_name.is_empty() {
            return Err(invalid_path("logical file path cannot be empty"));
        }
        let siblings = self.enumerate_files(directory, "*", SearchOption::TopDirectoryOnly)?;
        let mut indexed_parts = Vec::<(usize, String)>::new();
        for sibling in siblings {
            let Some(file_name) = sibling.rsplit('/').next() else {
                continue;
            };
            let Some(index) = split_sibling_part_index(base_name, file_name) else {
                continue;
            };
            let index = index?;
            indexed_parts.try_reserve(1).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::OutOfMemory,
                    "could not allocate logical file part index",
                )
            })?;
            indexed_parts.push((index, sibling));
        }
        indexed_parts.sort_unstable_by_key(|(index, _)| *index);
        for (expected, (actual, _)) in indexed_parts.iter().enumerate() {
            if *actual != expected {
                return Err(not_found(format!(
                    "logical file '{base_path}' is missing split part {expected}"
                )));
            }
        }
        let mut paths = Vec::new();
        paths.try_reserve_exact(indexed_parts.len()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::OutOfMemory,
                "could not allocate logical file paths",
            )
        })?;
        paths.extend(indexed_parts.into_iter().map(|(_, path)| path));
        Ok(paths)
    }

    /// Deletes a file if present. Existing open stream references remain usable.
    pub fn delete_file(&mut self, path: &str) -> io::Result<bool> {
        let normalized = normalize_path(Some(path), self.limits)?;
        let parts = path_parts(&normalized, self.limits)?;
        let Some((file_name, directories)) = parts.split_last() else {
            return Err(invalid_path("file path cannot be empty"));
        };
        let directory = find_directory_mut(&mut self.root, directories).ok_or_else(|| {
            not_found(format!(
                "parent directory of '{}' does not exist",
                canonical_path(&parts)
            ))
        })?;
        let removed = directory.files.remove(*file_name).is_some();
        if removed {
            self.entry_count -= 1;
        }
        Ok(removed)
    }

    /// Reads a file from byte zero, leaving its shared stream position at end-of-file.
    pub fn read_all_bytes(&self, path: &str, maximum_bytes: usize) -> io::Result<Vec<u8>> {
        let mut stream = self.open_file(path)?;
        let length = stream.length()?;
        if length > u64::try_from(maximum_bytes).unwrap_or(u64::MAX) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("file length {length} exceeds read limit {maximum_bytes}"),
            ));
        }
        let length = usize::try_from(length)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "file is too large"))?;
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(length).map_err(|_| {
            io::Error::new(
                io::ErrorKind::OutOfMemory,
                "could not allocate file contents",
            )
        })?;
        bytes.resize(length, 0);
        stream.seek(SeekFrom::Start(0))?;
        stream.read_exact(&mut bytes)?;
        Ok(bytes)
    }

    pub fn write_all_bytes(&mut self, path: &str, bytes: &[u8]) -> io::Result<()> {
        let mut stream = self.create_file(path)?;
        stream.set_len(u64::try_from(bytes.len()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "file contents are too large")
        })?)?;
        stream.seek(SeekFrom::Start(0))?;
        stream.write_all(bytes)
    }

    /// `File.ReadAllText(path)`: UTF-8 text decoded from the whole file, bounded like
    /// [`VirtualFileSystem::read_all_bytes`]. Non-UTF-8 content is an explicit
    /// `InvalidData` error rather than a lossy replacement. The `Encoding` overloads
    /// stay open: only UTF-8 has a portable counterpart here.
    pub fn read_all_text(&self, path: &str, maximum_bytes: usize) -> io::Result<String> {
        let bytes = self.read_all_bytes(path, maximum_bytes)?;
        String::from_utf8(bytes).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("file '{path}' is not valid UTF-8: {error}"),
            )
        })
    }

    /// `File.WriteAllText(path, contents)`: UTF-8 bytes written through
    /// [`VirtualFileSystem::write_all_bytes`], so the parent directory must exist and
    /// an existing file is truncated first.
    pub fn write_all_text(&mut self, path: &str, contents: &str) -> io::Result<()> {
        self.write_all_bytes(path, contents.as_bytes())
    }

    pub fn enumerate_directories(
        &self,
        path: Option<&str>,
        search_pattern: &str,
        search_option: SearchOption,
    ) -> io::Result<Vec<String>> {
        if search_pattern != "*" {
            return Err(unsupported_pattern(search_pattern));
        }
        let normalized = normalize_path(path, self.limits)?;
        let parts = path_parts(&normalized, self.limits)?;
        let start = find_directory(&self.root, &parts).ok_or_else(|| {
            not_found(format!("directory '{}' not found", canonical_path(&parts)))
        })?;
        let start_path = canonical_path(&parts);
        let mut result = Vec::new();

        match search_option {
            SearchOption::TopDirectoryOnly => {
                result.extend(
                    start
                        .directories
                        .keys()
                        .map(|name| join_virtual_path(&start_path, name)),
                );
            }
            SearchOption::AllDirectories => {
                let mut stack = Vec::new();
                for (name, child) in &start.directories {
                    stack.push((child, join_virtual_path(&start_path, name)));
                }
                while let Some((directory, directory_path)) = stack.pop() {
                    result.push(directory_path.clone());
                    for (name, child) in &directory.directories {
                        stack.push((child, join_virtual_path(&directory_path, name)));
                    }
                }
            }
        }
        result.sort();
        Ok(result)
    }

    pub fn enumerate_files(
        &self,
        path: Option<&str>,
        search_pattern: &str,
        search_option: SearchOption,
    ) -> io::Result<Vec<String>> {
        let extension = file_extension_pattern(search_pattern)?;
        let normalized = normalize_path(path, self.limits)?;
        let parts = path_parts(&normalized, self.limits)?;
        let start = find_directory(&self.root, &parts).ok_or_else(|| {
            not_found(format!("directory '{}' not found", canonical_path(&parts)))
        })?;
        let start_path = canonical_path(&parts);
        let mut result = Vec::new();

        match search_option {
            SearchOption::TopDirectoryOnly => {
                collect_files(start, &start_path, extension, &mut result);
            }
            SearchOption::AllDirectories => {
                let mut stack = vec![(start, start_path)];
                while let Some((directory, directory_path)) = stack.pop() {
                    collect_files(directory, &directory_path, extension, &mut result);
                    for (name, child) in &directory.directories {
                        stack.push((child, join_virtual_path(&directory_path, name)));
                    }
                }
            }
        }
        result.sort();
        Ok(result)
    }
}

impl Default for VirtualFileSystem {
    fn default() -> Self {
        Self::new()
    }
}

fn normalize_path(path: Option<&str>, limits: VirtualFileSystemLimits) -> io::Result<String> {
    let path = path.unwrap_or("");
    if path.len() > limits.max_path_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "virtual path has {} bytes, exceeding limit {}",
                path.len(),
                limits.max_path_bytes
            ),
        ));
    }
    let mut normalized = String::new();
    normalized.try_reserve_exact(path.len()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::OutOfMemory,
            "could not allocate virtual path",
        )
    })?;
    normalized.extend(path.chars().map(
        |character| {
            if character == '\\' {
                '/'
            } else {
                character
            }
        },
    ));
    Ok(normalized)
}

fn path_parts(path: &str, limits: VirtualFileSystemLimits) -> io::Result<Vec<&str>> {
    let mut parts = Vec::new();
    for part in path.split('/').filter(|part| !part.is_empty()) {
        if parts.len() >= limits.max_path_components {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "virtual path exceeds the {} component limit",
                    limits.max_path_components
                ),
            ));
        }
        parts.try_reserve(1).map_err(|_| {
            io::Error::new(
                io::ErrorKind::OutOfMemory,
                "could not allocate virtual path components",
            )
        })?;
        parts.push(part);
    }
    Ok(parts)
}

fn find_directory<'a>(root: &'a DirectoryEntry, parts: &[&str]) -> Option<&'a DirectoryEntry> {
    let mut current = root;
    for part in parts {
        current = current.directories.get(*part)?;
    }
    Some(current)
}

fn find_directory_mut<'a>(
    root: &'a mut DirectoryEntry,
    parts: &[&str],
) -> Option<&'a mut DirectoryEntry> {
    let mut current = root;
    for part in parts {
        current = current.directories.get_mut(*part)?;
    }
    Some(current)
}

fn ensure_entry_capacity(entry_count: usize, maximum: usize) -> io::Result<()> {
    if entry_count >= maximum {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("virtual filesystem entry limit {maximum} exceeded"),
        ));
    }
    Ok(())
}

fn canonical_path(parts: &[&str]) -> String {
    if parts.is_empty() {
        "/".to_owned()
    } else {
        format!("/{}", parts.join("/"))
    }
}

fn join_virtual_path(parent: &str, name: &str) -> String {
    if parent == "/" {
        format!("/{name}")
    } else {
        format!("{parent}/{name}")
    }
}

fn logical_split_base(path: &str) -> Option<&str> {
    let file_name = path.rsplit('/').next()?;
    let (base_name, suffix) = file_name.rsplit_once(".split")?;
    if base_name.is_empty()
        || suffix.is_empty()
        || !suffix.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    path.get(..path.len().checked_sub(suffix.len() + ".split".len())?)
}

/// Split-part suffix decision for virtual sibling scans: the numeric part index
/// when `file_name` is exactly `{base}.split<digits>`, or `None` for unrelated
/// siblings. This recognizes with a known base, unlike `logical_split_base`,
/// which derives the base from the name.
///
/// Unlike the on-disk opener (`io::stream`), which rejects decoy names such as
/// `data.split+1` because they would corrupt an assembled group, sibling scans
/// must ignore them: batched path-list inputs share virtual directories, so one
/// input's oddly-named files can never disturb another input's group.
/// Overflowing indices are an error. Called by `find_contiguous_split_parts`.
fn split_sibling_part_index(base_name: &str, file_name: &str) -> Option<io::Result<usize>> {
    let suffix = file_name.strip_prefix(base_name)?.strip_prefix(".split")?;
    if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(suffix.parse::<usize>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("split part index in '{file_name}' exceeds the address space"),
        )
    }))
}

fn file_extension_pattern(pattern: &str) -> io::Result<&str> {
    if pattern == "*" {
        return Ok("");
    }
    let Some(extension) = pattern.strip_prefix("*.") else {
        return Err(unsupported_pattern(pattern));
    };
    if extension.is_empty() || extension.contains(['*', '?']) {
        return Err(unsupported_pattern(pattern));
    }
    Ok(&pattern[1..])
}

fn collect_files(
    directory: &DirectoryEntry,
    directory_path: &str,
    extension: &str,
    output: &mut Vec<String>,
) {
    output.extend(
        directory
            .files
            .keys()
            .filter(|name| name.ends_with(extension))
            .map(|name| join_virtual_path(directory_path, name)),
    );
}

fn invalid_path(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.to_owned())
}

fn not_found(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, message)
}

fn unsupported_pattern(pattern: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        format!("virtual filesystem does not support search pattern '{pattern}'"),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        split_sibling_part_index, SearchOption, VirtualFileSystem, VirtualFileSystemLimits,
    };
    use crate::io::stream::SmartStream;
    use std::io::{Read, Seek, SeekFrom, Write};

    #[test]
    fn root_and_path_normalization_match_virtual_paths() {
        let mut filesystem = VirtualFileSystem::new();
        assert_eq!(filesystem.count(), 1);
        assert!(filesystem.directory_exists(None).unwrap());
        assert!(filesystem.directory_exists(Some("\\")).unwrap());
        filesystem
            .create_directory(Some("\\assets//textures/"))
            .unwrap();
        assert!(filesystem
            .directory_exists(Some("/assets/textures"))
            .unwrap());
        assert_eq!(filesystem.count(), 3);
        assert_eq!(
            filesystem
                .enumerate_directories(None, "*", SearchOption::AllDirectories)
                .unwrap(),
            ["/assets", "/assets/textures"]
        );
    }

    #[test]
    fn logical_files_join_contiguous_split_parts_and_prefer_direct_files() {
        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/data")).unwrap();
        filesystem
            .write_all_bytes("/data/stream.resS.split0", b"first ")
            .unwrap();
        filesystem
            .write_all_bytes("/data/stream.resS.split1", b"second")
            .unwrap();
        filesystem
            .write_all_bytes("/data/direct.resS", b"direct")
            .unwrap();
        filesystem
            .write_all_bytes("/data/direct.resS.split0", b"split")
            .unwrap();
        filesystem
            .write_all_bytes("/data/gap.resS.split0", b"first")
            .unwrap();
        filesystem
            .write_all_bytes("/data/gap.resS.split2", b"third")
            .unwrap();

        let mut stream = filesystem.open_logical_file("/data/stream.resS").unwrap();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"first second");
        assert!(filesystem
            .logical_file_exists("/data/stream.resS.split1")
            .unwrap());

        let mut direct = filesystem.open_logical_file("/data/direct.resS").unwrap();
        direct.seek(SeekFrom::Start(0)).unwrap();
        let mut direct_bytes = Vec::new();
        direct.read_to_end(&mut direct_bytes).unwrap();
        assert_eq!(direct_bytes, b"direct");
        assert!(!filesystem.logical_file_exists("/data/gap.resS").unwrap());
        let gap_error = match filesystem.open_logical_file("/data/gap.resS") {
            Ok(_) => panic!("a gapped split group must not open"),
            Err(error) => error,
        };
        assert_eq!(gap_error.kind(), std::io::ErrorKind::NotFound);
        assert!(!filesystem
            .logical_file_exists("/data/missing.resS")
            .unwrap());
    }

    /// The sibling-scan part decision: strict `{base}.split<digits>` names
    /// resolve to their index while unrelated siblings — including decoy names
    /// the on-disk opener rejects — stay ignored instead of disturbing a group.
    #[test]
    fn split_sibling_names_resolve_strictly_numeric_indices() {
        assert_eq!(
            split_sibling_part_index("data", "data.split0")
                .unwrap()
                .unwrap(),
            0
        );
        assert_eq!(
            split_sibling_part_index("data", "data.split007")
                .unwrap()
                .unwrap(),
            7
        );
        assert!(split_sibling_part_index("data", "other.split0").is_none());
        assert!(split_sibling_part_index("data", "database.split0").is_none());
        assert!(split_sibling_part_index("data", "data.split").is_none());
        assert!(split_sibling_part_index("data", "data.split+1").is_none());
        assert!(split_sibling_part_index("data", "data.split 1").is_none());
    }

    /// Grouping with decoys: batched inputs share virtual directories, so a
    /// decoy sibling must neither join the group nor break its assembly, and it
    /// stays readable as the regular file it is.
    #[test]
    fn split_grouping_ignores_decoy_siblings() {
        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/batch")).unwrap();
        filesystem
            .write_all_bytes("/batch/group.resS.split0", b"first ")
            .unwrap();
        filesystem
            .write_all_bytes("/batch/group.resS.split1", b"second")
            .unwrap();
        filesystem
            .write_all_bytes("/batch/group.resS.split+1", b"decoy")
            .unwrap();
        filesystem
            .write_all_bytes("/batch/other.resS.split0", b"other")
            .unwrap();

        let mut stream = filesystem.open_logical_file("/batch/group.resS").unwrap();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"first second");
        assert_eq!(
            filesystem
                .read_all_bytes("/batch/group.resS.split+1", 5)
                .unwrap(),
            b"decoy"
        );
        let mut other = filesystem.open_logical_file("/batch/other.resS").unwrap();
        let mut other_bytes = Vec::new();
        other.read_to_end(&mut other_bytes).unwrap();
        assert_eq!(other_bytes, b"other");
    }

    #[test]
    fn creating_files_requires_existing_parent_and_truncates_existing_contents() {
        let mut filesystem = VirtualFileSystem::new();
        assert_eq!(
            filesystem
                .create_file("/missing/file")
                .err()
                .unwrap()
                .kind(),
            std::io::ErrorKind::NotFound
        );
        filesystem.create_directory(Some("/assets")).unwrap();
        filesystem
            .write_all_bytes("/assets/file.bin", b"old-data")
            .unwrap();
        let mut reference = filesystem.create_file("assets/file.bin").unwrap();
        assert_eq!(reference.length().unwrap(), 0);
        reference.seek(SeekFrom::Start(0)).unwrap();
        reference.write_all(b"new").unwrap();
        assert_eq!(
            filesystem.read_all_bytes("/assets/file.bin", 3).unwrap(),
            b"new"
        );
        assert_eq!(filesystem.count(), 3);
    }

    #[test]
    fn deletion_and_clear_preserve_already_open_streams() {
        let mut filesystem = VirtualFileSystem::new();
        filesystem.write_all_bytes("/held", b"contents").unwrap();
        let mut open_reference = filesystem.open_file("held").unwrap();
        assert!(filesystem.delete_file("/held").unwrap());
        assert!(!filesystem.file_exists(Some("/held")).unwrap());
        let mut bytes = Vec::new();
        open_reference.seek(SeekFrom::Start(0)).unwrap();
        open_reference.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"contents");

        filesystem.create_directory(Some("/tree/branch")).unwrap();
        filesystem
            .write_all_bytes("/tree/branch/file", b"still open")
            .unwrap();
        let mut open_reference = filesystem.open_file("/tree/branch/file").unwrap();
        filesystem.clear();
        assert_eq!(filesystem.count(), 1);
        bytes.clear();
        open_reference.seek(SeekFrom::Start(0)).unwrap();
        open_reference.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"still open");
    }

    #[test]
    fn file_enumeration_supports_top_level_recursive_and_extension_patterns() {
        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/assets/nested")).unwrap();
        filesystem.write_all_bytes("/assets/a.png", b"a").unwrap();
        filesystem
            .write_all_bytes("/assets/nested/b.png", b"b")
            .unwrap();
        filesystem
            .write_all_bytes("/assets/nested/c.PNG", b"c")
            .unwrap();

        assert_eq!(
            filesystem
                .enumerate_files(Some("assets"), "*", SearchOption::TopDirectoryOnly)
                .unwrap(),
            ["/assets/a.png"]
        );
        assert_eq!(
            filesystem
                .enumerate_files(Some("/assets"), "*.png", SearchOption::AllDirectories)
                .unwrap(),
            ["/assets/a.png", "/assets/nested/b.png"]
        );
        assert_eq!(
            filesystem
                .enumerate_directories(Some("/assets"), "*", SearchOption::TopDirectoryOnly)
                .unwrap(),
            ["/assets/nested"]
        );
        assert_eq!(
            filesystem
                .enumerate_files(Some("/assets"), "b*", SearchOption::AllDirectories)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::Unsupported
        );
    }

    #[test]
    fn path_and_entry_limits_fail_before_mutation() {
        let limits = VirtualFileSystemLimits {
            max_path_bytes: 16,
            max_path_components: 2,
            max_entries: 3,
        };
        let mut filesystem = VirtualFileSystem::with_limits(limits).unwrap();
        filesystem.create_directory(Some("/one/two")).unwrap();
        assert_eq!(filesystem.count(), 3);
        assert_eq!(
            filesystem
                .create_directory(Some("/three"))
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidData
        );
        assert_eq!(
            filesystem
                .create_directory(Some("/one/two/three"))
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidInput
        );
        assert_eq!(
            filesystem
                .create_directory(Some("/this/path/is/too/long"))
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidInput
        );
        assert_eq!(filesystem.count(), 3);

        let mut limited = VirtualFileSystem::with_limits(VirtualFileSystemLimits {
            max_entries: 2,
            ..VirtualFileSystemLimits::default()
        })
        .unwrap();
        assert_eq!(
            limited
                .create_directory(Some("/parent/child"))
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidData
        );
        assert_eq!(limited.count(), 1);
        assert!(!limited.directory_exists(Some("/parent")).unwrap());
    }

    #[test]
    fn inserting_a_stream_creates_parent_directories_and_rejects_replacement() {
        let mut filesystem = VirtualFileSystem::new();
        filesystem
            .insert_file_stream_with_parents(
                "/assets/nested/file.bin",
                SmartStream::from_memory(b"first".to_vec()),
            )
            .unwrap();
        assert_eq!(filesystem.count(), 4);
        assert_eq!(
            filesystem
                .read_all_bytes("/assets/nested/file.bin", 32)
                .unwrap(),
            b"first"
        );

        assert_eq!(
            filesystem
                .insert_file_stream_with_parents(
                    "/assets/nested/file.bin",
                    SmartStream::from_memory(b"replacement".to_vec()),
                )
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::AlreadyExists
        );
        assert_eq!(filesystem.count(), 4);
        assert_eq!(
            filesystem
                .read_all_bytes("/assets/nested/file.bin", 32)
                .unwrap(),
            b"first"
        );
    }

    #[test]
    fn inserting_a_stream_checks_all_missing_entries_before_mutation() {
        let mut filesystem = VirtualFileSystem::with_limits(VirtualFileSystemLimits {
            max_entries: 3,
            ..VirtualFileSystemLimits::default()
        })
        .unwrap();
        assert_eq!(
            filesystem
                .insert_file_stream_with_parents(
                    "/one/two/file.bin",
                    SmartStream::from_memory(b"data".to_vec()),
                )
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidData
        );
        assert_eq!(filesystem.count(), 1);
        assert!(!filesystem.directory_exists(Some("/one")).unwrap());
    }

    #[test]
    fn text_helpers_round_trip_utf8_through_the_byte_paths() {
        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/docs")).unwrap();
        filesystem
            .write_all_text("/docs/readme.txt", "héllo")
            .unwrap();
        assert_eq!(
            filesystem.read_all_text("/docs/readme.txt", 64).unwrap(),
            "héllo"
        );
        // Overwriting with shorter text truncates first, like WriteAllBytes.
        filesystem.write_all_text("/docs/readme.txt", "hi").unwrap();
        assert_eq!(
            filesystem.read_all_text("/docs/readme.txt", 64).unwrap(),
            "hi"
        );
        // Missing files and over-limit reads report through the byte path.
        assert_eq!(
            filesystem
                .read_all_text("/docs/absent.txt", 64)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::NotFound
        );
        assert_eq!(
            filesystem
                .read_all_text("/docs/readme.txt", 1)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidData
        );
        // Non-UTF-8 bytes are an explicit error, never lossy text.
        filesystem
            .write_all_bytes("/docs/raw.bin", &[0xff, 0xfe])
            .unwrap();
        let error = filesystem.read_all_text("/docs/raw.bin", 64).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("/docs/raw.bin"));
        // Writing text still requires an existing parent directory.
        assert_eq!(
            filesystem
                .write_all_text("/nodir/file.txt", "x")
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::NotFound
        );
    }

    #[test]
    fn temporary_directory_defaults_and_ignores_blank_values() {
        let mut filesystem = VirtualFileSystem::new();
        assert_eq!(filesystem.temporary_directory(), "/temp");
        for blank in ["", "   ", "\t\n"] {
            filesystem.set_temporary_directory(blank);
            assert_eq!(filesystem.temporary_directory(), "/temp", "{blank:?}");
        }
        filesystem.set_temporary_directory("/scratch/tmp");
        assert_eq!(filesystem.temporary_directory(), "/scratch/tmp");
        // Relative values resolve against the process working directory.
        filesystem.set_temporary_directory("relative-tmp");
        let resolved = filesystem.temporary_directory().to_owned();
        assert!(
            resolved.ends_with("/relative-tmp") || resolved == "relative-tmp",
            "{resolved}"
        );
    }

    /// The `GetUniqueName` length stage against the reference `GetUniqueNameTests`
    /// rows (`AssetRipper.IO.Files.Tests`): those tests use a missing directory, so
    /// every expectation exercises exactly this stage.
    #[test]
    fn file_name_truncation_matches_the_unique_name_tests() {
        use super::truncate_file_name_to_byte_length as truncate;
        // Multibyte characters back off to the last full sequence.
        assert_eq!(truncate("A文.ext", 4), ".ext");
        assert_eq!(truncate("A文.ext", 5), "A.ext");
        assert_eq!(truncate("A文.ext", 6), "A.ext");
        assert_eq!(truncate("A文.ext", 7), "A.ext");
        assert_eq!(truncate("A文.ext", 8), "A文.ext");
        assert_eq!(truncate("A文.ext", 9), "A文.ext");
        // The extension is preserved while the stem shrinks.
        assert_eq!(truncate("A文.exte", 8), "A.exte");
        assert_eq!(truncate("A文.exte", 9), "A文.exte");
        // Extensionless names truncate the whole stem.
        assert_eq!(truncate("A文", 3), "A");
        assert_eq!(truncate("A文", 4), "A文");
        // Names within budget pass through untouched.
        assert_eq!(truncate("plain.txt", 9), "plain.txt");
        assert_eq!(truncate("", 0), "");
    }
}
