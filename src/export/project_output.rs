//! Add-only, path-confined writes for Unity project sidecar output.

use std::error::Error;
use std::ffi::OsString;
use std::fmt::{self, Display, Formatter};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::meta::{UnityMetaFile, UnityMetaFileError};
use super::unity_yaml::UnityYamlWriteLimits;

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
            Self::InvalidRelativePath { reason } => {
                write!(formatter, "unsafe project output path: {reason}")
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
        }
    }
}

impl Error for UnityProjectOutputError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::CreateOutputRoot { source, .. }
            | Self::CreateOutputDirectory { source, .. }
            | Self::CreateSidecar { source, .. }
            | Self::WriteSidecar { source, .. } => Some(source),
            Self::SerializeMeta(source) => Some(source),
            _ => None,
        }
    }
}

/// Write one `.meta` sidecar beneath `output_root` without deleting or replacing existing files.
///
/// The asset itself is not written by this function. `relative_asset_path` is validated as a
/// portable relative path, existing directory links are rejected, and the sidecar is created with
/// `create_new` so a collision is an error rather than an overwrite.
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
    for component in &components[..components.len() - 1] {
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

    let mut sidecar_name = OsString::new();
    sidecar_name
        .try_reserve(components[components.len() - 1].len() + ".meta".len())
        .map_err(|_| UnityProjectOutputError::AllocationFailed {
            resource: "Unity meta sidecar filename",
            requested: components[components.len() - 1].len() + ".meta".len(),
        })?;
    sidecar_name.push(components[components.len() - 1]);
    sidecar_name.push(".meta");
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

fn validated_path_components(
    path: &str,
    limits: UnityProjectOutputLimits,
) -> Result<Vec<&str>, UnityProjectOutputError> {
    if path.len() > limits.max_relative_path_bytes {
        return Err(UnityProjectOutputError::RelativePathTooLong {
            byte_count: path.len(),
            limit: limits.max_relative_path_bytes,
        });
    }
    if path.is_empty() {
        return Err(UnityProjectOutputError::InvalidRelativePath {
            reason: "path is empty",
        });
    }
    let mut components = Vec::new();
    for component in path.split(['/', '\\']) {
        if components.len() >= limits.max_path_components {
            return Err(UnityProjectOutputError::TooManyPathComponents {
                count: components.len().saturating_add(1),
                limit: limits.max_path_components,
            });
        }
        if component.is_empty()
            || component == "."
            || component == ".."
            || component.contains([':', '<', '>', '"', '|', '?', '*'])
            || component.chars().any(char::is_control)
            || component.ends_with([' ', '.'])
            || is_windows_device_name(component)
        {
            return Err(UnityProjectOutputError::InvalidRelativePath {
                reason: "path must contain only ordinary, portable relative components",
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
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{write_unity_meta_sidecar, UnityProjectOutputError, UnityProjectOutputLimits};
    use crate::export::meta::{UnityMetaFile, UnityMetaFileOptions};
    use crate::export::unity_yaml::{UnityYamlMappingStyle, UnityYamlNode, UnityYamlWriteLimits};

    struct TestOutputRoot(PathBuf);

    impl TestOutputRoot {
        fn new() -> Self {
            static NEXT_ID: AtomicU64 = AtomicU64::new(0);
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "assetripper-meta-output-{}-{id}",
                std::process::id()
            ));
            fs::create_dir(&path).unwrap();
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
            b"fileFormatVersion: 2\nguid: 00000001000000000000000000000000\nDefaultImporter: {}\n"
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
}
