//! Add-only, path-confined writes for Unity project sidecar output.
//!
//! Ports the output mechanics of `ExportCollection.ExportMeta` and
//! `AssetExportCollection.Export`
//! (`AssetRipper/Source/AssetRipper.Export.UnityProjects/ExportCollection.cs`,
//! `AssetRipper/Source/AssetRipper.Export.UnityProjects/AssetExportCollection.cs`):
//!
//! - Sidecars land beside the asset as `{filePath}.meta`, serialized without a BOM,
//!   tag, or version, with formatted keys.
//! - Asset bytes are written before the `.meta` record is built; a failed export
//!   writes neither file, mirroring the source `false` return. The export
//!   subdirectory itself is still created first (`Directory.Create(subPath)` runs
//!   ahead of `ExportInner`), so a failed export leaves the empty directory behind.
//! - A zero export GUID fails the meta step while the asset bytes stay written,
//!   matching the source construction order (`Meta(GUID, importer)` after export).
//! - Unique-name resolution and the `.meta` length reservation live in `asset_path`;
//!   the importer record (including its bundle-name propagation) is a caller fact.
//! - Single-asset reference rules (`AssetExportCollection<T>.Contains`, `GetExportID`,
//!   `CreateExportPointer`): member identity and pointer locality over caller facts;
//!   the main export ID, collection GUID, and exporter type flow in from `ids.rs` and
//!   `export_type.rs` through the caller.
//!
//! Survey closures, so later rounds do not re-litigate them:
//! - The base `ExportCollection.ExportAsset` overload has no baseline callers; the same
//!   step kernel covers its shape (precomputed path, throwing exporter).
//! - `Meta`'s `Importer.IncludesImporter` version gate is commented out upstream ("for
//!   now, assume true"), so the importer record is always written.
//! - Open: the base `CreateImporter` also stamps `MainObjectFileID` with the asset's main
//!   export ID, but no Rust importer builder emits that field yet; it belongs to the
//!   per-exporter importer construction, not this tail.
//!
//! Deliberately stricter than the source, by pipeline design:
//! - Both files are created add-only; the source overwrites existing files.
//! - Every relative path is confined (no traversal, absolute paths, device names,
//!   or unsafe components) and each created ancestor is verified to be a real
//!   directory under the canonical output root. A file blocking a directory, or a
//!   file at the output root, fails instead of writing outside the project.
//! - The random per-collection source GUID is replaced by a caller-supplied nonzero
//!   GUID so repeated exports stay deterministic.

use std::error::Error;
use std::ffi::OsString;
use std::fmt::{self, Display, Formatter};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::asset_path::HostPathPlatform;
use super::meta::{UnityMetaFile, UnityMetaFileError, UnityMetaFileOptions};
use super::meta_emission::decide_asset_collection_export;
use super::meta_emission::export_meta_mechanics::META_EXTENSION;
use super::unity_yaml::{UnityYamlNode, UnityYamlWriteLimits};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnityProjectOutputLimits {
    pub max_relative_path_bytes: usize,
    pub max_path_components: usize,
}

impl Default for UnityProjectOutputLimits {
    fn default() -> Self {
        Self {
            max_relative_path_bytes: 32 * 1024,
            max_path_components: 256,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnityMetaSidecarReport {
    pub path: PathBuf,
    pub byte_count: usize,
}

#[derive(Debug)]
pub enum UnityProjectOutputError {
    RelativePathTooLong {
        byte_count: usize,
        limit: usize,
    },
    TooManyPathComponents {
        count: usize,
        limit: usize,
    },
    InvalidRelativePath {
        path: String,
        reason: &'static str,
    },
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    CreateOutputRoot {
        path: PathBuf,
        source: io::Error,
    },
    OutputRootIsNotDirectory {
        path: PathBuf,
    },
    CreateOutputDirectory {
        path: PathBuf,
        source: io::Error,
    },
    UnsafeOutputAncestor {
        path: PathBuf,
    },
    SerializeMeta(UnityMetaFileError),
    SidecarAlreadyExists {
        path: PathBuf,
    },
    CreateSidecar {
        path: PathBuf,
        source: io::Error,
    },
    WriteSidecar {
        path: PathBuf,
        source: io::Error,
    },
    CreateAsset {
        path: PathBuf,
        source: io::Error,
    },
    WriteAsset {
        path: PathBuf,
        source: io::Error,
    },
}

impl Display for UnityProjectOutputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::RelativePathTooLong { byte_count, limit } => write!(
                formatter,
                "project output relative path uses {byte_count} bytes, exceeding limit {limit}"
            ),
            Self::TooManyPathComponents { count, limit } => write!(
                formatter,
                "project output path has {count} components, exceeding limit {limit}"
            ),
            Self::InvalidRelativePath { path, reason } => {
                write!(formatter, "unsafe project output path {path:?}: {reason}")
            }
            Self::AllocationFailed { resource, requested } => {
                write!(formatter, "could not allocate {requested} entries for {resource}")
            }
            Self::CreateOutputRoot { path, source } => {
                write!(formatter, "could not create project output root {path:?}: {source}")
            }
            Self::OutputRootIsNotDirectory { path } => {
                write!(formatter, "project output root {path:?} is not a directory")
            }
            Self::CreateOutputDirectory { path, source } => {
                write!(formatter, "could not create project output directory {path:?}: {source}")
            }
            Self::UnsafeOutputAncestor { path } => write!(
                formatter,
                "project output ancestor {path:?} is a link, is not a directory, or escapes the output root"
            ),
            Self::SerializeMeta(source) => Display::fmt(source, formatter),
            Self::SidecarAlreadyExists { path } => {
                write!(formatter, "refusing to replace existing Unity meta sidecar {path:?}")
            }
            Self::CreateSidecar { path, source } => {
                write!(formatter, "could not create Unity meta sidecar {path:?}: {source}")
            }
            Self::WriteSidecar { path, source } => {
                write!(formatter, "could not write Unity meta sidecar {path:?}: {source}")
            }
            Self::CreateAsset { path, source } => {
                write!(formatter, "could not create Unity asset file {path:?}: {source}")
            }
            Self::WriteAsset { path, source } => {
                write!(formatter, "could not write Unity asset file {path:?}: {source}")
            }
        }
    }
}

impl Error for UnityProjectOutputError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::CreateOutputRoot { source, .. }
            | Self::CreateOutputDirectory { source, .. }
            | Self::CreateSidecar { source, .. }
            | Self::CreateAsset { source, .. }
            | Self::WriteSidecar { source, .. }
            | Self::WriteAsset { source, .. } => Some(source),
            Self::SerializeMeta(source) => Some(source),
            _ => None,
        }
    }
}

/// Write one `.meta` sidecar beneath `output_root` without deleting or replacing existing files.
///
/// The asset itself is not written by this function. `relative_asset_path` is validated using
/// the host's path separators, existing directory links are rejected, and the sidecar is created
/// with `create_new` so a collision is an error rather than an overwrite.
pub fn write_unity_meta_sidecar(
    output_root: &Path,
    relative_asset_path: &str,
    meta: UnityMetaFile,
    path_limits: UnityProjectOutputLimits,
    yaml_limits: UnityYamlWriteLimits,
) -> Result<UnityMetaSidecarReport, UnityProjectOutputError> {
    let components = validated_path_components(relative_asset_path, path_limits)?;
    let contents = meta
        .to_yaml(yaml_limits)
        .map_err(UnityProjectOutputError::SerializeMeta)?;

    let current_directory =
        ensure_output_directory(output_root, &components[..components.len() - 1])?;

    let sidecar_name = meta_sidecar_file_name(components[components.len() - 1])?;
    let sidecar_path = current_directory.join(sidecar_name);
    let mut sidecar = match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&sidecar_path)
    {
        Ok(file) => file,
        Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {
            return Err(UnityProjectOutputError::SidecarAlreadyExists { path: sidecar_path });
        }
        Err(source) => {
            return Err(UnityProjectOutputError::CreateSidecar {
                path: sidecar_path,
                source,
            });
        }
    };
    sidecar.write_all(contents.as_bytes()).map_err(|source| {
        UnityProjectOutputError::WriteSidecar {
            path: sidecar_path.clone(),
            source,
        }
    })?;

    Ok(UnityMetaSidecarReport {
        path: sidecar_path,
        byte_count: contents.len(),
    })
}

/// Create the output root and one confined subdirectory level per component, rejecting
/// links, files, and escapes, and return the deepest directory. Shared by asset bytes
/// and `.meta` sidecars so both land under the same validated walk.
fn ensure_output_directory(
    output_root: &Path,
    dir_components: &[&str],
) -> Result<PathBuf, UnityProjectOutputError> {
    fs::create_dir_all(output_root).map_err(|source| {
        UnityProjectOutputError::CreateOutputRoot {
            path: output_root.to_path_buf(),
            source,
        }
    })?;
    let canonical_root = fs::canonicalize(output_root).map_err(|source| {
        UnityProjectOutputError::CreateOutputRoot {
            path: output_root.to_path_buf(),
            source,
        }
    })?;
    if !fs::metadata(&canonical_root)
        .map_err(|source| UnityProjectOutputError::CreateOutputRoot {
            path: canonical_root.clone(),
            source,
        })?
        .is_dir()
    {
        return Err(UnityProjectOutputError::OutputRootIsNotDirectory {
            path: canonical_root,
        });
    }

    let mut current_directory = canonical_root.clone();
    for component in dir_components {
        let candidate = current_directory.join(component);
        match fs::create_dir(&candidate) {
            Ok(()) => {}
            Err(source) if source.kind() == io::ErrorKind::AlreadyExists => {}
            Err(source) => {
                return Err(UnityProjectOutputError::CreateOutputDirectory {
                    path: candidate,
                    source,
                });
            }
        }
        let metadata = fs::symlink_metadata(&candidate).map_err(|source| {
            UnityProjectOutputError::CreateOutputDirectory {
                path: candidate.clone(),
                source,
            }
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(UnityProjectOutputError::UnsafeOutputAncestor { path: candidate });
        }
        current_directory = fs::canonicalize(&candidate).map_err(|_| {
            UnityProjectOutputError::UnsafeOutputAncestor {
                path: candidate.clone(),
            }
        })?;
        if !current_directory.starts_with(&canonical_root) {
            return Err(UnityProjectOutputError::UnsafeOutputAncestor {
                path: current_directory,
            });
        }
    }
    Ok(current_directory)
}

/// Caller facts for one `AssetExportCollection.Export` step: the planned host-relative
/// relative asset path, the caller-owned export GUID, and the importer record.
///
/// The source mints a random GUID per collection; the Rust pipeline requires a
/// caller-supplied nonzero GUID instead so repeated exports stay deterministic.
pub struct AssetExportWithMetaPlan {
    /// Host-relative asset path including the planned unique file name.
    pub relative_asset_path: String,
    /// The export GUID (rejected when zero).
    pub guid: [u8; 16],
    /// The importer class name, e.g. `NativeFormatImporter`.
    pub importer_class: String,
    /// The importer mapping node.
    pub importer: UnityYamlNode,
    /// Folder/license/timestamp options for the `.meta` envelope.
    pub meta_options: UnityMetaFileOptions,
}

/// The outcome of one asset-plus-`.meta` export step.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssetExportStep {
    /// `ExportInner` failed: neither asset bytes nor meta are written, mirroring the
    /// source `false` return.
    SkippedFailedExport,
    /// Asset bytes plus the add-only `.meta` sidecar were written.
    Exported {
        /// The written asset file.
        asset_path: PathBuf,
        /// Bytes handed to the asset writer.
        asset_bytes: usize,
        /// The written sidecar report.
        sidecar: UnityMetaSidecarReport,
    },
}

/// Run one asset-plus-`.meta` export step over caller facts, mirroring
/// `AssetExportCollection.Export`: asset bytes first, then `Meta(GUID, importer)`
/// beside them as `{file}.meta`. A failed export writes nothing; both files are
/// created add-only (the source overwrites, which the pipeline refuses by design).
pub fn export_asset_with_meta(
    output_root: &Path,
    plan: AssetExportWithMetaPlan,
    export_succeeded: bool,
    write_asset_bytes: impl FnOnce(&Path) -> io::Result<usize>,
    path_limits: UnityProjectOutputLimits,
    yaml_limits: UnityYamlWriteLimits,
) -> Result<AssetExportStep, UnityProjectOutputError> {
    let components = validated_path_components(&plan.relative_asset_path, path_limits)?;
    let (dir_components, [file_name]) = components.split_at(components.len() - 1) else {
        return Err(UnityProjectOutputError::InvalidRelativePath {
            path: plan.relative_asset_path.clone(),
            reason: "path must contain only ordinary relative components",
        });
    };
    // The source creates the export subdirectory before the asset writer runs
    // (`Directory.Create(subPath)` ahead of `ExportInner`), so a failed export still
    // leaves the (empty) directory behind. Only file writes stay gated.
    let directory = ensure_output_directory(output_root, dir_components)?;
    // No site writes the sidecar for a failed asset export. The shared outcome kernel owns
    // this gate so its two spellings cannot diverge.
    if !decide_asset_collection_export(export_succeeded).emits_meta() {
        return Ok(AssetExportStep::SkippedFailedExport);
    }
    let asset_path = directory.join(file_name);
    let asset_bytes = write_asset_bytes_to_new_file(&asset_path, write_asset_bytes)?;
    let meta = UnityMetaFile::new(
        plan.guid,
        &plan.importer_class,
        plan.importer,
        plan.meta_options,
    )
    .map_err(UnityProjectOutputError::SerializeMeta)?;
    let sidecar = write_unity_meta_sidecar(
        output_root,
        &plan.relative_asset_path,
        meta,
        path_limits,
        yaml_limits,
    )?;
    Ok(AssetExportStep::Exported {
        asset_path,
        asset_bytes,
        sidecar,
    })
}

/// Write a project-owned file such as `ProjectSettings/ProjectVersion.txt` or
/// `Packages/manifest.json` below the output root. The path is validated with the
/// same traversal and symlink checks as asset exports, and the destination is
/// created add-only.
pub fn write_new_project_file(
    output_root: &Path,
    relative_path: &str,
    bytes: &[u8],
    path_limits: UnityProjectOutputLimits,
) -> Result<PathBuf, UnityProjectOutputError> {
    write_new_project_stream(
        output_root,
        relative_path,
        &mut io::Cursor::new(bytes),
        bytes.len() as u64,
        path_limits,
    )
}

pub(super) fn validate_project_output_path(
    relative_path: &str,
    limits: UnityProjectOutputLimits,
) -> Result<(), UnityProjectOutputError> {
    validated_path_components(relative_path, limits).map(|_| ())
}

pub(super) fn create_project_directory(
    output_root: &Path,
    relative_path: &str,
    limits: UnityProjectOutputLimits,
) -> Result<PathBuf, UnityProjectOutputError> {
    let components = validated_path_components(relative_path, limits)?;
    ensure_output_directory(output_root, &components)
}

/// Copy exactly a preflighted, bounded stream without allocating its payload.
/// The same confined directory walk and add-only creation protect ordinary files
/// and original StreamingAssets files; a short read remains an export failure.
pub(super) fn write_new_project_stream(
    output_root: &Path,
    relative_path: &str,
    source: &mut impl io::Read,
    expected_bytes: u64,
    path_limits: UnityProjectOutputLimits,
) -> Result<PathBuf, UnityProjectOutputError> {
    let components = validated_path_components(relative_path, path_limits)?;
    let (directory_components, [file_name]) = components.split_at(components.len() - 1) else {
        return Err(UnityProjectOutputError::InvalidRelativePath {
            path: relative_path.to_owned(),
            reason: "path must contain only ordinary relative components",
        });
    };
    let directory = ensure_output_directory(output_root, directory_components)?;
    let path = directory.join(file_name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|source| UnityProjectOutputError::CreateAsset {
            path: path.clone(),
            source,
        })?;
    let mut copy = || -> io::Result<()> {
        let mut buffer = [0u8; 64 * 1024];
        let mut remaining = expected_bytes;
        while remaining != 0 {
            let count = remaining.min(buffer.len() as u64) as usize;
            source.read_exact(&mut buffer[..count])?;
            file.write_all(&buffer[..count])?;
            remaining -= count as u64;
        }
        file.flush()
    };
    copy().map_err(|source| UnityProjectOutputError::WriteAsset {
        path: path.clone(),
        source,
    })?;
    Ok(path)
}

/// Create one asset file add-only, run the caller writer into it, and flush before
/// returning the written byte count. A writer failure leaves the partial asset behind
/// with no `.meta` beside it, matching the source asset-first order where a throwing
/// exporter aborts before `ExportMeta`.
fn write_asset_bytes_to_new_file(
    asset_path: &Path,
    write_asset_bytes: impl FnOnce(&Path) -> io::Result<usize>,
) -> Result<usize, UnityProjectOutputError> {
    let mut asset_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(asset_path)
        .map_err(|source| UnityProjectOutputError::CreateAsset {
            path: asset_path.to_path_buf(),
            source,
        })?;
    let asset_bytes =
        write_asset_bytes(asset_path).map_err(|source| UnityProjectOutputError::WriteAsset {
            path: asset_path.to_path_buf(),
            source,
        })?;
    asset_file
        .flush()
        .map_err(|source| UnityProjectOutputError::WriteAsset {
            path: asset_path.to_path_buf(),
            source,
        })?;
    drop(asset_file);
    Ok(asset_bytes)
}

/// `AssetExportCollection<T>.Contains`: identity with the collection asset.
/// `AssetInfo` equality is resolved caller-side; only the outcome is modeled here.
pub const fn single_asset_contains(is_same_asset: bool) -> bool {
    is_same_asset
}

/// `AssetExportCollection<T>.GetExportID`: the main export ID for the member asset
/// (resolved caller-side via `ExportIdHandler`), `ArgumentException` otherwise.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SingleAssetExportId {
    /// The member asset: take the main export ID.
    MainExportId,
    /// Anything else: throw `ArgumentException`.
    ThrowsArgument,
}

/// Selects the `GetExportID` behavior from asset identity.
pub const fn single_asset_export_id(is_same_asset: bool) -> SingleAssetExportId {
    if is_same_asset {
        SingleAssetExportId::MainExportId
    } else {
        SingleAssetExportId::ThrowsArgument
    }
}

/// `AssetExportCollection<T>.CreateExportPointer`: local requests mint the bare file-ID
/// pointer; non-local requests attach the collection GUID and exporter type (both caller
/// facts); foreign assets fail through the ID rule first, on either locality.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SingleAssetExportPointer {
    /// Mint `MetaPtr(exportID)`.
    LocalFileId,
    /// Mint `MetaPtr(exportID, GUID, exporter type)`.
    RemoteWithCollectionIdentity,
    /// Throw `ArgumentException` via the ID rule.
    ThrowsArgument,
}

/// Selects the pointer behavior from locality and asset identity.
pub const fn single_asset_export_pointer(
    is_local: bool,
    is_same_asset: bool,
) -> SingleAssetExportPointer {
    if !is_same_asset {
        SingleAssetExportPointer::ThrowsArgument
    } else if is_local {
        SingleAssetExportPointer::LocalFileId
    } else {
        SingleAssetExportPointer::RemoteWithCollectionIdentity
    }
}

/// `ExportCollection.ExportMeta` sidecar naming (`ExportCollection.cs`, `MetaExtension`):
/// the sidecar appends `.meta` to the full asset path, never replacing the extension, so
/// `Icon.png` maps to `Icon.png.meta` and extensionless names gain the suffix directly.
fn meta_sidecar_file_name(file_name: &str) -> Result<OsString, UnityProjectOutputError> {
    let mut sidecar_name = OsString::new();
    sidecar_name
        .try_reserve(file_name.len() + META_EXTENSION.len())
        .map_err(|_| UnityProjectOutputError::AllocationFailed {
            resource: "Unity meta sidecar filename",
            requested: file_name.len() + META_EXTENSION.len(),
        })?;
    sidecar_name.push(file_name);
    sidecar_name.push(META_EXTENSION);
    Ok(sidecar_name)
}

fn validated_path_components(
    path: &str,
    limits: UnityProjectOutputLimits,
) -> Result<Vec<&str>, UnityProjectOutputError> {
    let platform = if cfg!(windows) {
        HostPathPlatform::Windows
    } else {
        HostPathPlatform::Unix
    };
    validated_path_components_for_platform(path, limits, platform)
}

fn validated_path_components_for_platform(
    path: &str,
    limits: UnityProjectOutputLimits,
    platform: HostPathPlatform,
) -> Result<Vec<&str>, UnityProjectOutputError> {
    if path.len() > limits.max_relative_path_bytes {
        return Err(UnityProjectOutputError::RelativePathTooLong {
            byte_count: path.len(),
            limit: limits.max_relative_path_bytes,
        });
    }
    if path.is_empty() {
        return Err(UnityProjectOutputError::InvalidRelativePath {
            path: path.to_owned(),
            reason: "path is empty",
        });
    }
    let mut components = Vec::new();
    let is_separator = |character| {
        character == '/' || (platform == HostPathPlatform::Windows && character == '\\')
    };
    for component in path.split(is_separator) {
        if components.len() >= limits.max_path_components {
            return Err(UnityProjectOutputError::TooManyPathComponents {
                count: components.len().saturating_add(1),
                limit: limits.max_path_components,
            });
        }
        let invalid_windows_component = platform == HostPathPlatform::Windows
            && (component.contains([':', '<', '>', '"', '|', '?', '*'])
                || component.ends_with([' ', '.'])
                || is_windows_device_name(component));
        if component.is_empty()
            || component == "."
            || component == ".."
            || invalid_windows_component
            || component.chars().any(char::is_control)
        {
            return Err(UnityProjectOutputError::InvalidRelativePath {
                path: path.to_owned(),
                reason: "path must contain only ordinary relative components",
            });
        }
        components
            .try_reserve(1)
            .map_err(|_| UnityProjectOutputError::AllocationFailed {
                resource: "project output path components",
                requested: components.len().saturating_add(1),
            })?;
        components.push(component);
    }
    Ok(components)
}

fn is_windows_device_name(component: &str) -> bool {
    let stem = component
        .split_once('.')
        .map_or(component, |(stem, _)| stem)
        .trim_end_matches([' ', '.']);
    if ["CON", "PRN", "AUX", "NUL"].contains(&stem.to_ascii_uppercase().as_str()) {
        return true;
    }
    let bytes = stem.as_bytes();
    bytes.len() == 4
        && (bytes[..3].eq_ignore_ascii_case(b"COM") || bytes[..3].eq_ignore_ascii_case(b"LPT"))
        && matches!(bytes[3], b'1'..=b'9')
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::{meta_sidecar_file_name, write_unity_meta_sidecar};
    use super::{
        single_asset_contains, single_asset_export_id, single_asset_export_pointer,
        SingleAssetExportId, SingleAssetExportPointer,
    };
    use super::{validated_path_components_for_platform, HostPathPlatform};
    use super::{UnityProjectOutputError, UnityProjectOutputLimits};
    use crate::export::meta::{UnityMetaFile, UnityMetaFileOptions};
    use crate::export::unity_yaml::{UnityYamlMappingStyle, UnityYamlNode, UnityYamlWriteLimits};

    struct TestOutputRoot(PathBuf);

    impl TestOutputRoot {
        fn new() -> Self {
            let path = crate::test_support::unique_test_directory("meta-output");
            Self(path)
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for TestOutputRoot {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn meta() -> UnityMetaFile {
        UnityMetaFile::new(
            [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            "DefaultImporter",
            UnityYamlNode::mapping(UnityYamlMappingStyle::Block),
            UnityMetaFileOptions {
                has_license_data: false,
                ..Default::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn meta_sidecar_write_is_add_only_and_preserves_siblings() {
        let root = TestOutputRoot::new();
        fs::write(root.path().join("keep.txt"), b"keep").unwrap();
        let report = write_unity_meta_sidecar(
            root.path(),
            "Assets/Textures/Icon.png",
            meta(),
            UnityProjectOutputLimits::default(),
            UnityYamlWriteLimits::default(),
        )
        .unwrap();

        let canonical_root = fs::canonicalize(root.path()).unwrap();
        assert_eq!(
            report.path,
            canonical_root
                .join("Assets")
                .join("Textures")
                .join("Icon.png.meta")
        );
        let bytes = fs::read(&report.path).unwrap();
        assert_eq!(
            bytes,
            b"fileFormatVersion: 2\nguid: 10000000000000000000000000000000\nDefaultImporter: {}\n"
        );
        assert_eq!(report.byte_count, bytes.len());
        assert_eq!(fs::read(root.path().join("keep.txt")).unwrap(), b"keep");
        assert!(!root.path().join("Assets/Textures/Icon.png").exists());
    }

    #[test]
    fn meta_sidecar_refuses_existing_file_without_modifying_it() {
        let root = TestOutputRoot::new();
        let sidecar = root.path().join("Assets/Textures/Icon.png.meta");
        fs::create_dir_all(sidecar.parent().unwrap()).unwrap();
        fs::write(&sidecar, b"existing user file").unwrap();

        assert!(matches!(
            write_unity_meta_sidecar(
                root.path(),
                "Assets/Textures/Icon.png",
                meta(),
                UnityProjectOutputLimits::default(),
                UnityYamlWriteLimits::default(),
            ),
            Err(UnityProjectOutputError::SidecarAlreadyExists { .. })
        ));
        assert_eq!(fs::read(sidecar).unwrap(), b"existing user file");
    }

    #[test]
    fn meta_sidecar_rejects_traversal_absolute_device_and_invalid_paths_before_writing() {
        let root = TestOutputRoot::new();
        for path in [
            "../outside.asset",
            "Assets/../outside.asset",
            "C:\\outside.asset",
            "\\\\server\\share\\outside.asset",
            "Assets/NUL.txt",
            "Assets/Bad?.asset",
            "Assets//duplicate-separator.asset",
        ] {
            assert!(
                matches!(
                    write_unity_meta_sidecar(
                        root.path(),
                        path,
                        meta(),
                        UnityProjectOutputLimits::default(),
                        UnityYamlWriteLimits::default(),
                    ),
                    Err(UnityProjectOutputError::InvalidRelativePath { .. })
                ),
                "path should be rejected: {path:?}"
            );
        }
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn output_path_components_follow_host_backslash_semantics() {
        let path = "Assets/Folder\\Literal/Icon.png";
        let limits = UnityProjectOutputLimits::default();
        assert_eq!(
            validated_path_components_for_platform(path, limits, HostPathPlatform::Unix).unwrap(),
            vec!["Assets", "Folder\\Literal", "Icon.png"]
        );
        assert_eq!(
            validated_path_components_for_platform(path, limits, HostPathPlatform::Windows)
                .unwrap(),
            vec!["Assets", "Folder", "Literal", "Icon.png"]
        );
    }

    #[test]
    fn unsafe_output_path_error_includes_the_rejected_path() {
        let error = validated_path_components_for_platform(
            "Assets/CON.txt",
            UnityProjectOutputLimits::default(),
            HostPathPlatform::Windows,
        )
        .unwrap_err();

        assert!(matches!(
            &error,
            UnityProjectOutputError::InvalidRelativePath { path, .. }
                if path == "Assets/CON.txt"
        ));
        assert!(error.to_string().contains("Assets/CON.txt"));
    }

    #[test]
    fn project_asset_and_meta_writers_follow_host_backslash_semantics() {
        let root = TestOutputRoot::new();
        let asset = write_new_project_file(
            root.path(),
            "Assets/Folder\\Literal/Icon.png",
            b"image bytes",
            UnityProjectOutputLimits::default(),
        )
        .unwrap();
        let sidecar = write_unity_meta_sidecar(
            root.path(),
            "Assets/Folder\\Literal/Icon.png",
            meta(),
            UnityProjectOutputLimits::default(),
            UnityYamlWriteLimits::default(),
        )
        .unwrap();

        let expected_asset = fs::canonicalize(root.path())
            .unwrap()
            .join("Assets")
            .join("Folder\\Literal")
            .join("Icon.png");
        assert_eq!(asset, expected_asset);
        assert_eq!(fs::read(&asset).unwrap(), b"image bytes");
        assert_eq!(sidecar.path, expected_asset.with_file_name("Icon.png.meta"));
        assert!(fs::read(&sidecar.path)
            .unwrap()
            .starts_with(b"fileFormatVersion: 2\n"));
    }

    #[test]
    fn meta_sidecar_enforces_path_byte_and_component_limits() {
        let root = TestOutputRoot::new();
        assert!(matches!(
            write_unity_meta_sidecar(
                root.path(),
                "Assets/Textures/Icon.png",
                meta(),
                UnityProjectOutputLimits {
                    max_relative_path_bytes: 8,
                    ..Default::default()
                },
                UnityYamlWriteLimits::default(),
            ),
            Err(UnityProjectOutputError::RelativePathTooLong { .. })
        ));
        assert!(matches!(
            write_unity_meta_sidecar(
                root.path(),
                "Assets/Textures/Icon.png",
                meta(),
                UnityProjectOutputLimits {
                    max_path_components: 2,
                    ..Default::default()
                },
                UnityYamlWriteLimits::default(),
            ),
            Err(UnityProjectOutputError::TooManyPathComponents { .. })
        ));
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }

    use super::{
        export_asset_with_meta, write_new_project_file, AssetExportStep, AssetExportWithMetaPlan,
    };

    #[test]
    fn project_auxiliary_files_are_confined_and_add_only() {
        let root = TestOutputRoot::new();
        let path = write_new_project_file(
            root.path(),
            "ProjectSettings/ProjectVersion.txt",
            b"m_EditorVersion: 2022.3.0f1\n",
            UnityProjectOutputLimits::default(),
        )
        .unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"m_EditorVersion: 2022.3.0f1\n");
        assert!(matches!(
            write_new_project_file(
                root.path(),
                "ProjectSettings/ProjectVersion.txt",
                b"replacement",
                UnityProjectOutputLimits::default(),
            ),
            Err(UnityProjectOutputError::CreateAsset { .. })
        ));
        assert!(matches!(
            write_new_project_file(
                root.path(),
                "../outside.txt",
                b"unsafe",
                UnityProjectOutputLimits::default(),
            ),
            Err(UnityProjectOutputError::InvalidRelativePath { .. })
        ));
        assert_eq!(fs::read(&path).unwrap(), b"m_EditorVersion: 2022.3.0f1\n");
        assert!(!root.path().parent().unwrap().join("outside.txt").exists());
    }

    fn export_plan(path: &str) -> AssetExportWithMetaPlan {
        AssetExportWithMetaPlan {
            relative_asset_path: path.to_owned(),
            guid: [9; 16],
            importer_class: "NativeFormatImporter".to_owned(),
            importer: UnityYamlNode::mapping(UnityYamlMappingStyle::Block),
            meta_options: UnityMetaFileOptions {
                has_license_data: false,
                time_created_unix_seconds: Some(1_700_000_000),
                ..Default::default()
            },
        }
    }

    #[test]
    fn failed_exports_write_neither_asset_nor_meta() {
        let root = TestOutputRoot::new();
        assert_eq!(
            export_asset_with_meta(
                root.path(),
                export_plan("Assets/Textures/Icon.png"),
                false,
                |_| panic!("a failed export must not write asset bytes"),
                UnityProjectOutputLimits::default(),
                UnityYamlWriteLimits::default(),
            )
            .unwrap(),
            AssetExportStep::SkippedFailedExport
        );
        // The source creates the subdirectory before the asset writer runs, so a failed
        // export leaves the empty directory tree but no files.
        assert!(root.path().join("Assets/Textures").is_dir());
        assert!(!root.path().join("Assets/Textures/Icon.png").exists());
        assert!(!root.path().join("Assets/Textures/Icon.png.meta").exists());
    }

    #[test]
    fn successful_exports_write_asset_bytes_before_the_sidecar() {
        let root = TestOutputRoot::new();
        let step = export_asset_with_meta(
            root.path(),
            export_plan("Assets/Textures/Icon.png"),
            true,
            |path| fs::write(path, b"asset-bytes").map(|()| 11),
            UnityProjectOutputLimits::default(),
            UnityYamlWriteLimits::default(),
        )
        .unwrap();
        let canonical_root = fs::canonicalize(root.path()).unwrap();
        let asset_path = canonical_root
            .join("Assets")
            .join("Textures")
            .join("Icon.png");
        let AssetExportStep::Exported {
            asset_path: planned,
            asset_bytes,
            sidecar,
        } = step
        else {
            panic!("a successful export must report both files");
        };
        assert_eq!(planned, asset_path);
        assert_eq!(asset_bytes, 11);
        assert_eq!(fs::read(&asset_path).unwrap(), b"asset-bytes");
        assert_eq!(
            sidecar.path,
            canonical_root
                .join("Assets")
                .join("Textures")
                .join("Icon.png.meta")
        );
        // The sidecar carries the planned GUID and importer record.
        let sidecar_text = fs::read_to_string(&sidecar.path).unwrap();
        assert!(
            sidecar_text.contains("guid: 90909090909090909090909090909090"),
            "{sidecar_text}"
        );
        assert!(
            sidecar_text.contains("NativeFormatImporter: {}"),
            "{sidecar_text}"
        );
        assert_eq!(sidecar.byte_count, sidecar_text.len());
    }

    #[test]
    fn existing_files_are_refused_without_overwriting() {
        let root = TestOutputRoot::new();
        let asset_path = root.path().join("Icon.png");
        fs::write(&asset_path, b"existing user bytes").unwrap();
        assert!(matches!(
            export_asset_with_meta(
                root.path(),
                export_plan("Icon.png"),
                true,
                |path| fs::write(path, b"new bytes").map(|()| 9),
                UnityProjectOutputLimits::default(),
                UnityYamlWriteLimits::default(),
            ),
            Err(UnityProjectOutputError::CreateAsset { .. })
        ));
        assert_eq!(fs::read(&asset_path).unwrap(), b"existing user bytes");
        assert!(!root.path().join("Icon.png.meta").exists());

        // A pre-existing sidecar fails after the asset lands, mirroring the
        // source asset-first order while keeping both files add-only.
        let root = TestOutputRoot::new();
        let sidecar_path = root.path().join("Icon.png.meta");
        fs::write(&sidecar_path, b"existing user meta").unwrap();
        assert!(matches!(
            export_asset_with_meta(
                root.path(),
                export_plan("Icon.png"),
                true,
                |path| fs::write(path, b"new bytes").map(|()| 9),
                UnityProjectOutputLimits::default(),
                UnityYamlWriteLimits::default(),
            ),
            Err(UnityProjectOutputError::SidecarAlreadyExists { .. })
        ));
        assert_eq!(fs::read(&sidecar_path).unwrap(), b"existing user meta");
    }

    #[test]
    fn failing_asset_writers_leave_the_partial_asset_without_a_sidecar() {
        let root = TestOutputRoot::new();
        let outcome = export_asset_with_meta(
            root.path(),
            export_plan("Icon.png"),
            true,
            |path| -> std::io::Result<usize> {
                std::fs::write(path, b"partial")?;
                Err(std::io::Error::other("writer blew up"))
            },
            UnityProjectOutputLimits::default(),
            UnityYamlWriteLimits::default(),
        );
        assert!(
            matches!(outcome, Err(UnityProjectOutputError::WriteAsset { .. })),
            "a throwing exporter aborts before the meta step"
        );
        // The source asset-first order: written bytes (partial here) land before any
        // meta record is built, so no sidecar exists beside the partial asset.
        assert_eq!(fs::read(root.path().join("Icon.png")).unwrap(), b"partial");
        assert!(!root.path().join("Icon.png.meta").exists());
    }

    #[test]
    fn zero_guids_fail_the_meta_step_with_the_asset_already_written() {
        let root = TestOutputRoot::new();
        let mut plan = export_plan("Icon.png");
        plan.guid = [0; 16];
        assert!(matches!(
            export_asset_with_meta(
                root.path(),
                plan,
                true,
                |path| fs::write(path, b"asset-bytes").map(|()| 11),
                UnityProjectOutputLimits::default(),
                UnityYamlWriteLimits::default(),
            ),
            Err(UnityProjectOutputError::SerializeMeta(_))
        ));
        // Source order writes asset bytes before the meta record is built.
        assert_eq!(
            fs::read(root.path().join("Icon.png")).unwrap(),
            b"asset-bytes"
        );
        assert!(!root.path().join("Icon.png.meta").exists());
    }

    #[test]
    fn file_at_the_output_root_fails_before_any_write() {
        let root = TestOutputRoot::new();
        let blocker = root.path().join("blocking-file");
        fs::write(&blocker, b"not a directory").unwrap();
        assert!(matches!(
            write_unity_meta_sidecar(
                &blocker,
                "Icon.png",
                meta(),
                UnityProjectOutputLimits::default(),
                UnityYamlWriteLimits::default(),
            ),
            Err(UnityProjectOutputError::CreateOutputRoot { .. })
        ));
        assert_eq!(fs::read(&blocker).unwrap(), b"not a directory");
    }

    #[test]
    fn file_blocking_a_subdirectory_fails_as_an_unsafe_ancestor() {
        let root = TestOutputRoot::new();
        fs::write(root.path().join("Assets"), b"not a directory").unwrap();
        assert!(matches!(
            write_unity_meta_sidecar(
                root.path(),
                "Assets/Icon.png",
                meta(),
                UnityProjectOutputLimits::default(),
                UnityYamlWriteLimits::default(),
            ),
            Err(UnityProjectOutputError::UnsafeOutputAncestor { .. })
        ));
        assert_eq!(
            fs::read(root.path().join("Assets")).unwrap(),
            b"not a directory"
        );
    }

    #[test]
    fn asset_steps_reject_unsafe_paths_before_touching_the_filesystem() {
        let root = TestOutputRoot::new();
        assert!(matches!(
            export_asset_with_meta(
                root.path(),
                export_plan("../outside.asset"),
                true,
                |_| panic!("an unsafe path must not reach the asset writer"),
                UnityProjectOutputLimits::default(),
                UnityYamlWriteLimits::default(),
            ),
            Err(UnityProjectOutputError::InvalidRelativePath { .. })
        ));
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn sidecar_names_append_the_meta_suffix_without_replacing_extensions() {
        use std::ffi::OsString;
        // The source concatenates `$"{filePath}{MetaExtension}"`: suffix-append, never
        // stem replacement, so multi-dot and extensionless names keep their full text.
        for (file_name, sidecar_name) in [
            ("Icon.png", "Icon.png.meta"),
            ("data.tar.gz", "data.tar.gz.meta"),
            ("archive", "archive.meta"),
            (".meta", ".meta.meta"),
        ] {
            assert_eq!(
                meta_sidecar_file_name(file_name).unwrap(),
                OsString::from(sidecar_name),
                "{file_name:?}"
            );
        }
    }

    #[test]
    fn single_asset_reference_rules_follow_identity_and_locality() {
        assert!(single_asset_contains(true));
        assert!(!single_asset_contains(false));
        assert_eq!(
            single_asset_export_id(true),
            SingleAssetExportId::MainExportId
        );
        assert_eq!(
            single_asset_export_id(false),
            SingleAssetExportId::ThrowsArgument
        );
        // Foreign assets fail through the ID rule first, on either locality.
        assert_eq!(
            single_asset_export_pointer(true, false),
            SingleAssetExportPointer::ThrowsArgument
        );
        assert_eq!(
            single_asset_export_pointer(false, false),
            SingleAssetExportPointer::ThrowsArgument
        );
        // Members mint the bare file-ID pointer locally...
        assert_eq!(
            single_asset_export_pointer(true, true),
            SingleAssetExportPointer::LocalFileId
        );
        // ...and attach the collection GUID plus exporter type remotely.
        assert_eq!(
            single_asset_export_pointer(false, true),
            SingleAssetExportPointer::RemoteWithCollectionIdentity
        );
    }
}
