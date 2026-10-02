//! In-memory virtual filesystem used by containers and recovered Unity files.

use std::collections::BTreeMap;
use std::io::{self, Read, Seek, SeekFrom, Write};

use crate::io::stream::SmartStream;

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
}

impl VirtualFileSystem {
    pub fn new() -> Self {
        Self {
            root: DirectoryEntry::default(),
            entry_count: 1,
            limits: VirtualFileSystemLimits::default(),
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
    pub(crate) fn insert_file_stream_with_parents(
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
    use super::{SearchOption, VirtualFileSystem, VirtualFileSystemLimits};
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
}
