//! Fixed export destinations for scene and manager collections.
//!
//! Unlike `AssetExportCollection`, these families do not resolve unique file names:
//!
//! - `SceneExportCollection.Export` joins the project directory with `$"{Scene.Path}.unity"`
//!   and creates the parent folder. When the scene index is a duplicate and the file already
//!   exists, the baseline logs a warning and skips the export instead.
//! - `ManagerExportCollection.Export` joins the project directory with `ProjectSettings` and a
//!   corrected `$"{name}.asset"` file name, then writes unconditionally with no `.meta` sidecar.
//!   `GetCorrectName` renames `PlayerSettings` (or class name `129`) to `ProjectSettings`,
//!   `NavMeshProjectSettings` to `NavMeshAreas`, and `PhysicsManager` to `DynamicsManager`.
//!
//! This module plans those destinations without touching the disk. Duplicate-scene detection
//! needs the export container and `BuildSettings`, so it stays the caller's responsibility.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

use super::asset_path::{join_path, HostPathPlatform};

/// `SceneExportCollection.ExportExtension`.
pub const SCENE_EXPORT_EXTENSION: &str = "unity";
/// The `ProjectSettings` directory used by `ManagerExportCollection.Export`.
pub const PROJECT_SETTINGS_DIRECTORY: &str = "ProjectSettings";

/// Error returned when a fixed export destination cannot be planned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixedPathError {
    /// `Scene.Path` is empty, so no `$"{path}.unity"` destination exists.
    EmptyScenePath,
    /// A bounded allocation for a planned path could not be reserved.
    AllocationFailed { resource: &'static str },
}

impl Display for FixedPathError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyScenePath => {
                formatter.write_str("scene export paths need a non-empty scene path")
            }
            Self::AllocationFailed { resource } => {
                write!(formatter, "could not allocate {resource}")
            }
        }
    }
}

impl Error for FixedPathError {}

/// The destinations selected by `SceneExportCollection.Export`.
///
/// `file_path` is `Path.Join(project_directory, $"{scene_path}.unity")` and `folder_path` is
/// its parent folder, which the baseline creates before writing. `relative_path` is the
/// project-relative form with the baseline's separators preserved.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedScenePaths {
    pub folder_path: String,
    pub file_path: String,
    pub relative_path: String,
}

/// The destinations selected by `ManagerExportCollection.Export`, which never emits `.meta`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedManagerPaths {
    pub subdirectory: String,
    pub file_name: String,
    pub file_path: String,
    pub relative_path: String,
}

/// Mirror the destination selection of `SceneExportCollection.Export` without touching the disk.
pub fn plan_scene_export_paths(
    scene_path: &str,
    platform: HostPathPlatform,
    project_directory: &str,
) -> Result<PlannedScenePaths, FixedPathError> {
    if scene_path.is_empty() {
        return Err(FixedPathError::EmptyScenePath);
    }
    let mut relative_path = String::new();
    relative_path
        .try_reserve(scene_path.len() + 1 + SCENE_EXPORT_EXTENSION.len())
        .map_err(|_| FixedPathError::AllocationFailed {
            resource: "export scene path",
        })?;
    relative_path.push_str(scene_path);
    relative_path.push('.');
    relative_path.push_str(SCENE_EXPORT_EXTENSION);
    let file_path = join_path(project_directory, &relative_path, platform);
    let folder_path = match parent_directory(&file_path, platform) {
        Some(parent) if !parent.is_empty() => parent.to_owned(),
        // `Path.GetDirectoryName` reports no folder for a bare file name; the baseline would
        // fail creating it, so the planner falls back to the project directory explicitly.
        _ => project_directory.to_owned(),
    };
    Ok(PlannedScenePaths {
        folder_path,
        file_path,
        relative_path,
    })
}

/// `ManagerExportCollection.GetCorrectName`, which renames three manager class names.
pub fn manager_export_base_name(class_name: &str) -> &str {
    match class_name {
        "PlayerSettings" | "129" => PROJECT_SETTINGS_DIRECTORY,
        "NavMeshProjectSettings" => "NavMeshAreas",
        "PhysicsManager" => "DynamicsManager",
        _ => class_name,
    }
}

/// Mirror the destination selection of `ManagerExportCollection.Export` without touching the disk.
pub fn plan_manager_export_paths(
    class_name: &str,
    platform: HostPathPlatform,
    project_directory: &str,
) -> Result<PlannedManagerPaths, FixedPathError> {
    let base_name = manager_export_base_name(class_name);
    let mut file_name = String::new();
    file_name
        .try_reserve(base_name.len() + ".asset".len())
        .map_err(|_| FixedPathError::AllocationFailed {
            resource: "export manager file name",
        })?;
    file_name.push_str(base_name);
    file_name.push_str(".asset");
    let subdirectory = join_path(project_directory, PROJECT_SETTINGS_DIRECTORY, platform);
    let file_path = join_path(&subdirectory, &file_name, platform);
    let relative_path = join_path(PROJECT_SETTINGS_DIRECTORY, &file_name, platform);
    Ok(PlannedManagerPaths {
        subdirectory,
        file_name,
        file_path,
        relative_path,
    })
}

/// The `Path.GetDirectoryName` prefix before the last directory separator, if any.
fn parent_directory(path: &str, platform: HostPathPlatform) -> Option<&str> {
    let bytes = path.as_bytes();
    for index in (0..bytes.len()).rev() {
        let character = bytes[index] as char;
        if character == '/' || (platform == HostPathPlatform::Windows && character == '\\') {
            return Some(&path[..index]);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{
        manager_export_base_name, plan_manager_export_paths, plan_scene_export_paths,
        FixedPathError, HostPathPlatform, PROJECT_SETTINGS_DIRECTORY, SCENE_EXPORT_EXTENSION,
    };

    #[test]
    fn scene_paths_append_the_unity_extension_below_the_project_directory() {
        let planned =
            plan_scene_export_paths("Assets/Scenes/level0", HostPathPlatform::Windows, "Project")
                .unwrap();
        assert_eq!(planned.relative_path, "Assets/Scenes/level0.unity");
        assert_eq!(planned.file_path, "Project\\Assets/Scenes/level0.unity");
        assert_eq!(planned.folder_path, "Project\\Assets/Scenes");
        assert_eq!(SCENE_EXPORT_EXTENSION, "unity");
    }

    #[test]
    fn scene_paths_reject_empty_paths_and_fall_back_without_a_folder() {
        assert_eq!(
            plan_scene_export_paths("", HostPathPlatform::Windows, "Project"),
            Err(FixedPathError::EmptyScenePath)
        );
        // A bare scene name has no parent folder, so the project directory is reported.
        let planned = plan_scene_export_paths("level0", HostPathPlatform::Unix, "Project").unwrap();
        assert_eq!(planned.relative_path, "level0.unity");
        assert_eq!(planned.file_path, "Project/level0.unity");
        assert_eq!(planned.folder_path, "Project");
    }

    #[test]
    fn manager_base_names_follow_the_source_correction_table() {
        assert_eq!(
            manager_export_base_name("PlayerSettings"),
            "ProjectSettings"
        );
        assert_eq!(manager_export_base_name("129"), "ProjectSettings");
        assert_eq!(
            manager_export_base_name("NavMeshProjectSettings"),
            "NavMeshAreas"
        );
        assert_eq!(
            manager_export_base_name("PhysicsManager"),
            "DynamicsManager"
        );
        assert_eq!(manager_export_base_name("RenderSettings"), "RenderSettings");
        assert_eq!(manager_export_base_name(""), "");
        assert_eq!(PROJECT_SETTINGS_DIRECTORY, "ProjectSettings");
    }

    #[test]
    fn manager_paths_use_the_fixed_project_settings_destination() {
        let planned =
            plan_manager_export_paths("PhysicsManager", HostPathPlatform::Windows, "Project")
                .unwrap();
        assert_eq!(planned.subdirectory, "Project\\ProjectSettings");
        assert_eq!(planned.file_name, "DynamicsManager.asset");
        assert_eq!(
            planned.file_path,
            "Project\\ProjectSettings\\DynamicsManager.asset"
        );
        assert_eq!(
            planned.relative_path,
            "ProjectSettings\\DynamicsManager.asset"
        );

        let planned =
            plan_manager_export_paths("PlayerSettings", HostPathPlatform::Unix, "Project").unwrap();
        assert_eq!(planned.file_name, "ProjectSettings.asset");
        assert_eq!(
            planned.relative_path,
            "ProjectSettings/ProjectSettings.asset"
        );
    }
}
