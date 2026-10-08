//! Export root validation and preparation, ported from the reference GUI export guard.
//!
//! `AssetRipper.GUI.Web/GameFileLoader.cs` prepares the export root before every
//! export (`Export` at lines 60-76, `ExportPrimaryContent` at lines 78-99):
//!
//! ```text
//! if (IsLoaded && IsValidExportDirectory(path))
//! {
//!     if (IsNonEmptyDirectory(path))
//!     {
//!         if (!await UserConsentsToDeletion())
//!         {
//!             Logger.Info(LogCategory.Export, "User declined to delete existing export directory. Aborting export.");
//!             return;
//!         }
//!         Directory.Delete(path, true);
//!     }
//!     Directory.CreateDirectory(path);
//!     ... export ...
//! }
//! ```
//!
//! `IsValidExportDirectory` (108-122) rejects an empty path and any path whose
//! `Path.GetFileName` is exactly `Desktop`, `Documents`, or `Downloads`.
//! `IsNonEmptyDirectory` (124-127) is
//! `Directory.Exists(path) && Directory.EnumerateFileSystemEntries(path).Any()`.
//! In headless mode `UserConsentsToDeletion` (129-142) returns true, so the reference
//! CLI-equivalent behavior deletes and recreates a non-empty export directory.
//!
//! This module ports the guard and both consent outcomes. The interactive confirmation
//! has no Rust counterpart, so `prepare_export_root` defaults to the reference's
//! declined arm: a hard error that leaves the existing directory untouched. The
//! `--delete-existing` flag selects the consented arm
//! (`Directory.Delete(path, true)` followed by `Directory.CreateDirectory(path)`).
//!
//! `source_file_name` mirrors .NET `Path.GetFileName` rather than
//! `std::path::Path::file_name`, because the guard decision depends on its exact root
//! semantics: a trailing separator yields an empty name, `C:` and `C:\` yield an empty
//! name, and `\` is a separator only on Windows. The implementation follows the
//! published .NET 10 sources (`Path.cs` `GetFileName`, `Path.Windows.cs`
//! `IsPathRooted`/`GetPathRoot`, `PathInternal.Windows.cs` `GetRootLength`,
//! `Path.Unix.cs` `IsPathRooted`/`GetPathRoot`): the name is the span after
//! `max(rootLength, indexOfLastSeparator + 1)`.
//!
//! The committed differential fixture `project_guard_fixture.txt` records the
//! reference runtime results for a Windows host; the Windows rows are replayed by
//! `reference_export_root_guard_rows_match_the_rust_port`. The Unix root rules are
//! source-derived and covered by `source_file_name_matches_unix_path_source_semantics`.

use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// One code unit of a platform path string (UTF-16 on Windows, bytes on Unix).
trait PathUnit: Copy + PartialEq {
    fn is_separator(self) -> bool;
    // Drive-letter handling exists only in `PathInternal.Windows.GetRootLength`; the Unix
    // root rule never inspects one, so these stay out of non-Windows builds.
    #[cfg(windows)]
    fn is_drive_letter(self) -> bool;
    #[cfg(windows)]
    fn from_ascii(byte: u8) -> Self;
}

impl PathUnit for u8 {
    fn is_separator(self) -> bool {
        self == b'/'
    }

    #[cfg(windows)]
    fn is_drive_letter(self) -> bool {
        self.is_ascii_alphabetic()
    }

    #[cfg(windows)]
    fn from_ascii(byte: u8) -> Self {
        byte
    }
}

impl PathUnit for u16 {
    fn is_separator(self) -> bool {
        self == u16::from(b'\\') || self == u16::from(b'/')
    }

    #[cfg(windows)]
    fn is_drive_letter(self) -> bool {
        // `PathInternal.Windows.IsValidDriveChar` accepts only `A-Z`/`a-z`.
        matches!(self, 0x41..=0x5A | 0x61..=0x7A)
    }

    #[cfg(windows)]
    fn from_ascii(byte: u8) -> Self {
        u16::from(byte)
    }
}

/// `Path.GetFileName` for the host platform: the path suffix after the last separator,
/// clamped to the path root.
pub fn source_file_name(path: &Path) -> OsString {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        let units: Vec<u16> = path.as_os_str().encode_wide().collect();
        OsString::from_wide(file_name_units(&units))
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let units = path.as_os_str().as_bytes();
        OsString::from_vec(file_name_units(units).to_vec())
    }
}

/// System directory names the reference rejects as the final path segment.
///
/// `GameFileLoader.IsValidExportDirectory` compares ordinally and case sensitively, so
/// `desktop` exports while `Desktop` does not.
pub const PROTECTED_EXPORT_DIRECTORY_NAMES: [&str; 3] = ["Desktop", "Documents", "Downloads"];

/// The reference's system-directory name check (`directoryName is "Desktop" or ...`).
pub fn is_system_directory_name(name: &OsStr) -> bool {
    PROTECTED_EXPORT_DIRECTORY_NAMES
        .iter()
        .any(|protected| name == OsStr::new(*protected))
}

/// True when the reference `IsValidExportDirectory` accepts the path.
///
/// The reference logs and returns false; the port returns the reason as an error.
pub fn validate_export_directory(path: &Path) -> Result<(), ExportRootError> {
    if path.as_os_str().is_empty() {
        return Err(ExportRootError::EmptyPath);
    }
    if is_system_directory_name(&source_file_name(path)) {
        return Err(ExportRootError::SystemDirectory(path.to_path_buf()));
    }
    Ok(())
}

/// `Directory.Exists(path) && Directory.EnumerateFileSystemEntries(path).Any()`.
///
/// Missing paths, plain files, and unreadable directories are not non-empty
/// directories. Symbolic links are followed, matching `Directory.Exists`.
pub fn is_non_empty_directory(path: &Path) -> bool {
    match fs::read_dir(path) {
        Ok(mut entries) => entries.next().is_some(),
        Err(_) => false,
    }
}

/// Validate and create the export root, mirroring the reference export guard.
///
/// `delete_existing` selects the reference's consented arm: an existing non-empty
/// directory is deleted and recreated, so the export always starts from an empty root.
/// Without it the reference's declined arm is reproduced as an error and the existing
/// directory is left exactly as it was.
pub fn prepare_export_root(path: &Path, delete_existing: bool) -> Result<(), ExportRootError> {
    validate_export_directory(path)?;
    if is_non_empty_directory(path) {
        if !delete_existing {
            return Err(ExportRootError::NonEmptyDirectory(path.to_path_buf()));
        }
        remove_existing_root(path)?;
    }
    fs::create_dir_all(path).map_err(|source| ExportRootError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn remove_existing_root(path: &Path) -> Result<(), ExportRootError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| ExportRootError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let removed = if metadata.file_type().is_symlink() {
        // `Directory.Delete(path, true)` removes a directory link itself and never
        // descends into its target.
        remove_directory_link(path)
    } else {
        fs::remove_dir_all(path)
    };
    removed.map_err(|source| ExportRootError::Io {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(unix)]
fn remove_directory_link(path: &Path) -> io::Result<()> {
    fs::remove_file(path)
}

#[cfg(windows)]
fn remove_directory_link(path: &Path) -> io::Result<()> {
    fs::remove_dir(path)
}

/// The reason `prepare_export_root` or `validate_export_directory` rejected a root.
#[derive(Debug)]
pub enum ExportRootError {
    /// The reference logs `Export path is empty` and declines the export.
    EmptyPath,
    /// The reference logs `Export path '...' is a system directory` and declines.
    SystemDirectory(PathBuf),
    /// The reference's consent was declined (or is unavailable) for a non-empty root.
    NonEmptyDirectory(PathBuf),
    /// A filesystem operation on the export root failed.
    Io { path: PathBuf, source: io::Error },
}

impl fmt::Display for ExportRootError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPath => formatter.write_str("export path is empty"),
            Self::SystemDirectory(path) => {
                write!(formatter, "export path '{}' is a system directory", path.display())
            }
            Self::NonEmptyDirectory(path) => write!(
                formatter,
                "User declined to delete existing export directory. Aborting export. \
                 (export path {} already exists and is not empty; pass --delete-existing to delete it)",
                path.display()
            ),
            Self::Io { path, source } => {
                write!(formatter, "failed to prepare export root {}: {source}", path.display())
            }
        }
    }
}

impl Error for ExportRootError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::EmptyPath | Self::SystemDirectory(_) | Self::NonEmptyDirectory(_) => None,
        }
    }
}

fn file_name_units<T: PathUnit>(path: &[T]) -> &[T] {
    let root = root_length(path);
    let start = match path.iter().rposition(|unit| unit.is_separator()) {
        Some(index) if index >= root => index + 1,
        _ => root,
    };
    &path[start..]
}

#[cfg(unix)]
fn root_length<T: PathUnit>(path: &[T]) -> usize {
    // `Path.Unix.cs`: rooted exactly when the path starts with the directory separator,
    // and the root is then that single separator.
    if path.first().is_some_and(|unit| unit.is_separator()) {
        1
    } else {
        0
    }
}

#[cfg(windows)]
fn root_length<T: PathUnit>(path: &[T]) -> usize {
    // `PathInternal.Windows.GetRootLength` with the device-prefix constants
    // (`DevicePrefixLength` 4, `UncPrefixLength` 2, `UncExtendedPrefixLength` 8).
    const DEVICE_PREFIX_LENGTH: usize = 4;
    const UNC_PREFIX_LENGTH: usize = 2;
    const UNC_EXTENDED_PREFIX_LENGTH: usize = 8;

    let length = path.len();
    let device = is_device(path);
    let device_unc = device && is_device_unc(path);
    let mut i = 0;
    if (!device || device_unc) && length > 0 && path[0].is_separator() {
        if device_unc || (length > 1 && path[1].is_separator()) {
            i = if device_unc {
                UNC_EXTENDED_PREFIX_LENGTH
            } else {
                UNC_PREFIX_LENGTH
            };
            let mut separators = 2u32;
            let mut index = i;
            while index < length
                && (!path[index].is_separator() || {
                    separators -= 1;
                    separators > 0
                })
            {
                index += 1;
            }
            i = index;
        } else {
            i = 1;
        }
    } else if device {
        i = DEVICE_PREFIX_LENGTH;
        while i < length && !path[i].is_separator() {
            i += 1;
        }
        if i < length && i > DEVICE_PREFIX_LENGTH && path[i].is_separator() {
            i += 1;
        }
    } else if length >= 2 && path[1] == T::from_ascii(b':') && path[0].is_drive_letter() {
        i = 2;
        if length > 2 && path[2].is_separator() {
            i += 1;
        }
    }
    i
}

#[cfg(windows)]
fn is_device<T: PathUnit>(path: &[T]) -> bool {
    is_extended(path)
        || (path.len() >= 4
            && path[0].is_separator()
            && path[1].is_separator()
            && (path[2] == T::from_ascii(b'.') || path[2] == T::from_ascii(b'?'))
            && path[3].is_separator())
}

#[cfg(windows)]
fn is_extended<T: PathUnit>(path: &[T]) -> bool {
    path.len() >= 4
        && path[0] == T::from_ascii(b'\\')
        && (path[1] == T::from_ascii(b'\\') || path[1] == T::from_ascii(b'?'))
        && path[2] == T::from_ascii(b'?')
        && path[3] == T::from_ascii(b'\\')
}

#[cfg(windows)]
fn is_device_unc<T: PathUnit>(path: &[T]) -> bool {
    path.len() >= 8
        && is_device(path)
        && path[7].is_separator()
        && path[4] == T::from_ascii(b'U')
        && path[5] == T::from_ascii(b'N')
        && path[6] == T::from_ascii(b'C')
}

#[cfg(test)]
mod tests {
    use super::{
        is_non_empty_directory, prepare_export_root, source_file_name, validate_export_directory,
        ExportRootError,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    fn unique_test_directory(label: &str) -> PathBuf {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let parent = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("test-tmp");
        fs::create_dir_all(&parent).unwrap();
        let path = parent.join(format!(
            "{label}-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        path
    }

    #[test]
    fn source_file_name_matches_dotnet_root_and_separator_semantics() {
        // These rows hold identically on the Windows and Unix implementations.
        let cases = [
            ("", ""),
            ("relative", "relative"),
            ("relative/nested", "nested"),
            ("a//b", "b"),
            ("a b c", "a b c"),
            (".", "."),
            ("..", ".."),
            ("...", "..."),
            ("file.txt ", "file.txt "),
            ("Desktop", "Desktop"),
            ("Desktop.txt", "Desktop.txt"),
            ("/root", "root"),
        ];
        for (input, expected) in cases {
            assert_eq!(
                source_file_name(Path::new(input)),
                std::ffi::OsString::from(expected),
                "input {input:?}"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn source_file_name_matches_windows_path_source_semantics() {
        // Root and volume quirks of `Path.cs` + `PathInternal.Windows.GetRootLength`.
        let cases = [
            ("C:", ""),
            ("C:\\", ""),
            ("C:\\a", "a"),
            ("C:a", "a"),
            ("C:/a", "a"),
            ("C:\\a\\b", "b"),
            ("a:b", "b"),
            ("a\\b:c", "b:c"),
            ("a:b\\c", "c"),
            ("\\root", "root"),
            ("\\\\root", ""),
            ("\\\\", ""),
            ("/", ""),
            ("//root", ""),
            ("///root", ""),
            ("/root/", ""),
            ("Desktop\\", ""),
            ("Desktop/", ""),
            ("\\\\server\\share", ""),
            ("\\\\server\\share\\file", "file"),
            ("\\\\.\\C:\\x", "x"),
            ("\\\\?\\C:\\x\\y", "y"),
            ("\\\\?\\UNC\\server\\share\\file", "file"),
        ];
        for (input, expected) in cases {
            assert_eq!(
                source_file_name(Path::new(input)),
                std::ffi::OsString::from(expected),
                "input {input:?}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn source_file_name_matches_unix_path_source_semantics() {
        // `Path.Unix.cs`: the root is `/` exactly when the path is rooted, and `\` is
        // an ordinary character.
        let cases = [
            ("/", ""),
            ("//root", "root"),
            ("///root", "root"),
            ("/root/", ""),
            ("/root", "root"),
            ("a/", ""),
            ("\\root", "\\root"),
            ("C:\\a", "C:\\a"),
            ("C:", "C:"),
        ];
        for (input, expected) in cases {
            assert_eq!(
                source_file_name(Path::new(input)),
                std::ffi::OsString::from(expected),
                "input {input:?}"
            );
        }
    }

    #[test]
    fn validate_export_directory_rejects_empty_and_system_directory_names() {
        assert!(matches!(
            validate_export_directory(Path::new("")).unwrap_err(),
            ExportRootError::EmptyPath
        ));
        for name in ["Desktop", "Documents", "Downloads"] {
            let error = validate_export_directory(Path::new(name)).unwrap_err();
            assert!(
                matches!(&error, ExportRootError::SystemDirectory(path) if path == Path::new(name)),
                "name {name:?} produced {error:?}"
            );
            assert_eq!(
                error.to_string(),
                format!("export path '{name}' is a system directory")
            );
        }
        for accepted in [
            "desktop",
            "Desktop.txt",
            "DesktopX",
            "Desktop ",
            "Desktop/",
            "Downloads/nested",
        ] {
            assert!(
                validate_export_directory(Path::new(accepted)).is_ok(),
                "path {accepted:?} should be accepted"
            );
        }
        // A trailing separator empties the file name, so the reference accepts it.
        assert!(validate_export_directory(Path::new("project/Documents/Desktop/")).is_ok());
        assert!(matches!(
            validate_export_directory(Path::new("project/Documents/Desktop")).unwrap_err(),
            ExportRootError::SystemDirectory(_)
        ));
    }

    #[test]
    fn is_non_empty_directory_requires_a_readable_directory_with_entries() {
        let root = unique_test_directory("guard-nonempty");
        fs::create_dir_all(&root).unwrap();

        let empty = root.join("empty-dir");
        fs::create_dir(&empty).unwrap();
        assert!(!is_non_empty_directory(&empty));

        let absent = root.join("absent-dir");
        assert!(!is_non_empty_directory(&absent));

        let file = root.join("a-file.txt");
        fs::write(&file, b"payload").unwrap();
        assert!(!is_non_empty_directory(&file));

        let with_file = root.join("dir-with-file");
        fs::create_dir(&with_file).unwrap();
        fs::write(with_file.join("payload.txt"), b"payload").unwrap();
        assert!(is_non_empty_directory(&with_file));
        assert!(!is_non_empty_directory(&with_file.join("payload.txt")));

        let with_subdir = root.join("dir-with-subdir");
        fs::create_dir(&with_subdir).unwrap();
        fs::create_dir(with_subdir.join("nested")).unwrap();
        assert!(is_non_empty_directory(&with_subdir));

        let with_hidden = root.join("dir-with-hidden");
        fs::create_dir(&with_hidden).unwrap();
        fs::write(with_hidden.join(".hidden"), b"payload").unwrap();
        assert!(is_non_empty_directory(&with_hidden));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn prepare_export_root_creates_missing_and_empty_directories() {
        let root = unique_test_directory("guard-prepare");
        fs::create_dir_all(&root).unwrap();

        let missing = root.join("missing");
        prepare_export_root(&missing, false).unwrap();
        assert!(missing.is_dir());
        assert!(!is_non_empty_directory(&missing));

        let existing = root.join("existing-empty");
        fs::create_dir(&existing).unwrap();
        prepare_export_root(&existing, false).unwrap();
        assert!(existing.is_dir());
        assert!(!is_non_empty_directory(&existing));

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn prepare_export_root_refuses_a_non_empty_directory_without_touching_it() {
        let root = unique_test_directory("guard-refuse");
        fs::create_dir_all(&root).unwrap();
        let existing = root.join("existing");
        fs::create_dir(&existing).unwrap();
        fs::write(existing.join("keep.txt"), b"untouched").unwrap();

        let error = prepare_export_root(&existing, false).unwrap_err();
        assert!(matches!(error, ExportRootError::NonEmptyDirectory(_)));
        let message = error.to_string();
        assert!(
            message.contains("User declined to delete existing export directory. Aborting export."),
            "message {message:?} must keep the source log sentence"
        );
        assert!(message.contains("--delete-existing"), "message {message:?}");
        assert_eq!(fs::read(existing.join("keep.txt")).unwrap(), b"untouched");
        assert_eq!(fs::read_dir(&existing).unwrap().count(), 1);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn prepare_export_root_deletes_a_non_empty_directory_when_consented() {
        let root = unique_test_directory("guard-delete");
        fs::create_dir_all(&root).unwrap();
        let existing = root.join("existing");
        fs::create_dir(&existing).unwrap();
        fs::create_dir(existing.join("nested")).unwrap();
        fs::write(existing.join("nested").join("keep.txt"), b"replaced").unwrap();

        prepare_export_root(&existing, true).unwrap();
        assert!(existing.is_dir());
        assert!(!is_non_empty_directory(&existing));
        assert!(!existing.join("nested").exists());

        // A file where the export root should be is not a non-empty directory, so the
        // reference reaches `Directory.CreateDirectory` and fails there.
        let file_path = root.join("root-is-a-file.txt");
        fs::write(&file_path, b"payload").unwrap();
        assert!(matches!(
            prepare_export_root(&file_path, true).unwrap_err(),
            ExportRootError::Io { .. }
        ));
        assert_eq!(fs::read(&file_path).unwrap(), b"payload");

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn prepare_export_root_rejects_system_directories_without_filesystem_changes() {
        let error = prepare_export_root(Path::new("Desktop"), true).unwrap_err();
        assert!(matches!(error, ExportRootError::SystemDirectory(_)));
        let error =
            prepare_export_root(Path::new("project/Documents/Downloads"), true).unwrap_err();
        assert!(matches!(error, ExportRootError::SystemDirectory(_)));
    }

    #[test]
    fn prepare_export_root_replaces_a_directory_link_without_touching_the_target() {
        let root = unique_test_directory("guard-symlink");
        fs::create_dir_all(&root).unwrap();
        let target = root.join("target");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep.txt"), b"target payload").unwrap();
        let link = root.join("link");
        if symlink_directory(&target, &link).is_err() {
            // Creating a directory symbolic link needs privileges that the test host may
            // not grant. The reference behavior stays source-derived in that case.
            fs::remove_dir_all(&root).unwrap();
            return;
        }

        // `Directory.Exists` follows links, so a populated link target is non-empty.
        assert!(is_non_empty_directory(&link));
        assert!(validate_export_directory(&link).is_ok());

        prepare_export_root(&link, true).unwrap();
        assert!(link.is_dir());
        assert!(!is_non_empty_directory(&link));
        assert_eq!(
            fs::read(target.join("keep.txt")).unwrap(),
            b"target payload"
        );

        fs::remove_dir_all(&root).unwrap();
    }

    #[cfg(unix)]
    fn symlink_directory(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    #[cfg(windows)]
    fn symlink_directory(target: &Path, link: &Path) -> std::io::Result<()> {
        std::os::windows::fs::symlink_dir(target, link)
    }

    struct FixtureRow {
        check: String,
        platform: String,
        input: String,
        expected: String,
    }

    fn fixture_rows() -> Vec<FixtureRow> {
        include_str!("project_guard_fixture.txt")
            .lines()
            .filter(|line| !line.starts_with('#') && !line.is_empty())
            .map(|line| {
                let mut cells = line.split('\t');
                let row = FixtureRow {
                    check: cells.next().unwrap().to_string(),
                    platform: cells.next().unwrap().to_string(),
                    input: cells.next().unwrap().to_string(),
                    expected: cells.next().unwrap().to_string(),
                };
                assert!(
                    cells.next().is_none(),
                    "unexpected cell in fixture line {line:?}"
                );
                row
            })
            .collect()
    }

    fn substitute(value: &str, work_root: &Path) -> String {
        value
            .replace("{tmp}", &work_root.to_string_lossy())
            .replace(
                "{tmpname}",
                &work_root
                    .file_name()
                    .expect("work root has a file name")
                    .to_string_lossy(),
            )
            .replace("{empty}", "")
    }

    /// Replays the reference-runtime guard fixture produced by
    /// `tools/probes/project-guard` against this port.
    #[test]
    fn reference_export_root_guard_rows_match_the_rust_port() {
        let rows = fixture_rows();
        let work_root = unique_test_directory("guard-replay");
        fs::create_dir_all(&work_root).unwrap();
        fs::create_dir(work_root.join("project")).unwrap();
        fs::create_dir_all(work_root.join("project").join("Documents").join("Desktop")).unwrap();
        fs::create_dir(work_root.join("empty-dir")).unwrap();
        let with_file = work_root.join("dir-with-file");
        fs::create_dir(&with_file).unwrap();
        fs::write(with_file.join("payload.txt"), b"payload").unwrap();
        let with_subdir = work_root.join("dir-with-subdir");
        fs::create_dir(&with_subdir).unwrap();
        fs::create_dir(with_subdir.join("nested")).unwrap();
        let with_hidden = work_root.join("dir-with-hidden");
        fs::create_dir(&with_hidden).unwrap();
        fs::write(with_hidden.join(".hidden"), b"payload").unwrap();
        fs::write(work_root.join("a-file.txt"), b"payload").unwrap();

        let mut counts = [0usize; 4];
        for row in &rows {
            // The fixture was produced on a Windows host; every row except the headless
            // consent row depends on Windows `Path.GetFileName` semantics.
            if row.platform != "any" && !cfg!(windows) {
                continue;
            }
            let input = substitute(&row.input, &work_root);
            match row.check.as_str() {
                "valid" => {
                    let actual = if validate_export_directory(Path::new(&input)).is_ok() {
                        "valid"
                    } else {
                        "invalid"
                    };
                    assert_eq!(actual, row.expected, "valid row {input:?}");
                    counts[0] += 1;
                }
                "nonempty" => {
                    if row.expected == "skip" {
                        continue;
                    }
                    let actual = if is_non_empty_directory(Path::new(&input)) {
                        "nonempty"
                    } else {
                        "empty"
                    };
                    assert_eq!(actual, row.expected, "nonempty row {input:?}");
                    counts[1] += 1;
                }
                "filename" => {
                    let actual = source_file_name(Path::new(&input));
                    assert_eq!(
                        actual,
                        std::ffi::OsString::from(substitute(&row.expected, &work_root)),
                        "filename row {input:?}"
                    );
                    counts[2] += 1;
                }
                "consent" => {
                    // Headless consent is the reference's delete-and-recreate arm, and
                    // `--delete-existing` is its Rust equivalent.
                    assert_eq!(row.expected, "true", "consent row {input:?}");
                    let existing = work_root.join("consented");
                    fs::create_dir(&existing).unwrap();
                    fs::write(existing.join("keep.txt"), b"replaced").unwrap();
                    prepare_export_root(&existing, true).unwrap();
                    assert!(!is_non_empty_directory(&existing));
                    counts[3] += 1;
                }
                other => panic!("unknown fixture check {other:?}"),
            }
        }

        if cfg!(windows) {
            assert_eq!(counts, [22, 7, 48, 1], "executed fixture rows");
        } else {
            // Only the platform-independent consent row runs off Windows.
            assert_eq!(counts, [0, 0, 0, 1], "executed fixture rows");
        }

        fs::remove_dir_all(&work_root).unwrap();
    }
}
