//! File-writing destinations for the remaining export collection families.
//!
//! The `AssetExportCollection` unique-name flow lives in `asset_path`, and the scene and
//! manager fixed destinations live in `project_plan`. The families below select their
//! destinations differently, all fully determined by the frozen snapshot:
//!
//! - `UnknownExportCollection.Export` joins the project directory with `AssetRipper`,
//!   `UnknownAssets`, the asset class name, and `$"{Asset.Name}.unknown`, then resolves a
//!   unique file name with the static `FileSystem.GetUniqueName` overload. Neither the class
//!   name nor the asset name is character-fixed. No `.meta` sidecar is written.
//! - `UnreadableExportCollection.Export` follows the same shape under `UnreadableAssets` with
//!   a `.unreadable` extension, except the asset name first passes through
//!   `FixInvalidPathCharacters`. No `.meta` sidecar is written.
//! - `ScriptExportCollectionBase.GetExportSubPath` builds the `.cs` destination from the
//!   assembly, namespace, and class names: the assembly loses a trailing `.dll` (ordinal,
//!   case sensitive), first-pass assemblies map to a `Plugins` folder and all others to
//!   `Scripts`, namespace dots become platform directory separators, the joined folder passes
//!   through `FixInvalidPathCharacters`, and the file name is the fixed class name plus `.cs`.
//!   Both script collections write below the `Assets` directory.
//!
//! None of these planners touch the disk; unique-name resolution goes through the shared
//! filesystem probe, and the script collision rules (skip versus overwrite) stay the caller's
//! responsibility as recorded in `collection_kind`.

use super::asset_path::{
    fix_invalid_path_characters, get_unique_name, join_path, AssetPathError, ExportFileSystem,
    HostPathPlatform, MAX_ASSET_FILE_NAME_LENGTH,
};

/// The `AssetRipper` container directory for unknown and unreadable payloads.
pub const DIAGNOSTIC_CONTAINER_DIRECTORY: &str = "AssetRipper";
/// The `UnknownExportCollection` subdirectory.
pub const UNKNOWN_ASSETS_DIRECTORY: &str = "UnknownAssets";
/// The `UnreadableExportCollection` subdirectory.
pub const UNREADABLE_ASSETS_DIRECTORY: &str = "UnreadableAssets";
/// The `UnknownExportCollection` file extension.
pub const UNKNOWN_ASSET_EXTENSION: &str = "unknown";
/// The `UnreadableExportCollection` file extension.
pub const UNREADABLE_ASSET_EXTENSION: &str = "unreadable";
/// `SpecialFileNames.AssemblyExtension`.
pub const ASSEMBLY_EXTENSION: &str = ".dll";
/// The scripts folder for ordinary assemblies.
pub const SCRIPTS_FOLDER: &str = "Scripts";
/// The scripts folder for first-pass assemblies.
pub const PLUGINS_FOLDER: &str = "Plugins";
/// The `Assets` directory scripts are written below.
pub const ASSETS_DIRECTORY: &str = "Assets";
/// The script file extension.
pub const SCRIPT_EXTENSION: &str = "cs";

/// Destinations selected by the unknown/unreadable exporters.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedDiagnosticPaths {
    pub subdirectory: String,
    pub file_name: String,
    pub file_path: String,
    pub relative_path: String,
}

/// Destinations selected by `ScriptExportCollectionBase.GetExportSubPath`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedScriptPaths {
    /// The project-joined parent directory the baseline creates.
    pub subdirectory: String,
    pub file_name: String,
    pub file_path: String,
    /// The `Assets`-relative path with source host separators normalized. POSIX backslashes in
    /// assembly, namespace, or class names remain literal path characters.
    pub relative_asset_path: String,
}

/// `SpecialFileNames.RemoveAssemblyFileExtension`: strip one trailing `.dll`, ordinally.
pub fn remove_assembly_file_extension(assembly: &str) -> &str {
    assembly
        .strip_suffix(ASSEMBLY_EXTENSION)
        .unwrap_or(assembly)
}

/// The `GetScriptsFolderName` mapping: first-pass assemblies go to `Plugins`.
pub fn scripts_folder_name(assembly_folder: &str) -> &'static str {
    match assembly_folder {
        "Assembly-CSharp-firstpass"
        | "Assembly - CSharp - firstpass"
        | "Assembly-UnityScript-firstpass"
        | "Assembly - UnityScript - firstpass" => PLUGINS_FOLDER,
        _ => SCRIPTS_FOLDER,
    }
}

/// Mirror `ScriptExportCollectionBase.GetExportSubPath` without touching the disk.
///
/// Returns the `Assets`-relative folder and the `.cs` file name. Joining them below the
/// project directory is left to [`plan_script_export_paths`].
pub fn script_export_sub_path(
    assembly_name: &str,
    namespace_name: &str,
    class_name: &str,
    platform: HostPathPlatform,
) -> Result<(String, String), AssetPathError> {
    let assembly_folder = remove_assembly_file_extension(assembly_name);
    let scripts_folder = scripts_folder_name(assembly_folder);
    let separator = platform.separator();
    let namespace_folder: String = namespace_name
        .chars()
        .map(|character| {
            if character == '.' {
                separator
            } else {
                character
            }
        })
        .collect();
    let joined_folder = join_path(
        &join_path(scripts_folder, assembly_folder, platform),
        &namespace_folder,
        platform,
    );
    let folder_path = fix_invalid_path_characters(&joined_folder, platform)?;
    let fixed_class = fix_invalid_path_characters(class_name, platform)?;
    let mut file_name = String::with_capacity(fixed_class.len() + 1 + SCRIPT_EXTENSION.len());
    file_name.push_str(&fixed_class);
    file_name.push('.');
    file_name.push_str(SCRIPT_EXTENSION);
    Ok((folder_path, file_name))
}

/// Mirror the destination selection of both script collections without touching the disk.
pub fn plan_script_export_paths(
    assembly_name: &str,
    namespace_name: &str,
    class_name: &str,
    platform: HostPathPlatform,
    project_directory: &str,
) -> Result<PlannedScriptPaths, AssetPathError> {
    let (folder_path, file_name) =
        script_export_sub_path(assembly_name, namespace_name, class_name, platform)?;
    let subdirectory = join_path(
        &join_path(project_directory, ASSETS_DIRECTORY, platform),
        &folder_path,
        platform,
    );
    let file_path = join_path(&subdirectory, &file_name, platform);
    let relative_folder = match platform {
        HostPathPlatform::Windows => folder_path.replace('\\', "/"),
        HostPathPlatform::Unix => folder_path.clone(),
    };
    let relative_asset_path = join_path(
        ASSETS_DIRECTORY,
        &join_path(&relative_folder, &file_name, HostPathPlatform::Unix),
        HostPathPlatform::Unix,
    );
    Ok(PlannedScriptPaths {
        subdirectory,
        file_name,
        file_path,
        relative_asset_path,
    })
}

/// Shared destination selection of the unknown/unreadable exporters.
struct DiagnosticPlanRequest<'a> {
    project_directory: &'a str,
    diagnostic_directory: &'a str,
    class_name: &'a str,
    file_name: &'a str,
    extension: &'a str,
    platform: HostPathPlatform,
}

fn plan_diagnostic_paths(
    request: DiagnosticPlanRequest<'_>,
    state: &mut super::asset_path::UniqueNameState,
    file_system: &impl ExportFileSystem,
) -> Result<PlannedDiagnosticPaths, AssetPathError> {
    let DiagnosticPlanRequest {
        project_directory,
        diagnostic_directory,
        class_name,
        file_name,
        extension,
        platform,
    } = request;
    let subdirectory = join_path(
        &join_path(
            &join_path(project_directory, DIAGNOSTIC_CONTAINER_DIRECTORY, platform),
            diagnostic_directory,
            platform,
        ),
        class_name,
        platform,
    );
    let mut initial_name = String::with_capacity(file_name.len() + 1 + extension.len());
    initial_name.push_str(file_name);
    initial_name.push('.');
    initial_name.push_str(extension);
    let unique_name = get_unique_name(
        state,
        platform,
        file_system,
        &subdirectory,
        &initial_name,
        MAX_ASSET_FILE_NAME_LENGTH,
    )?;
    let file_path = join_path(&subdirectory, &unique_name, platform);
    let relative_path = join_path(
        &join_path(
            &join_path(
                DIAGNOSTIC_CONTAINER_DIRECTORY,
                diagnostic_directory,
                HostPathPlatform::Unix,
            ),
            class_name,
            HostPathPlatform::Unix,
        ),
        &unique_name,
        HostPathPlatform::Unix,
    );
    Ok(PlannedDiagnosticPaths {
        subdirectory,
        file_name: unique_name,
        file_path,
        relative_path,
    })
}

/// Mirror `UnknownExportCollection.Export` destination selection without touching the disk.
///
/// Neither the class name nor the asset name is character-fixed by the baseline.
pub fn plan_unknown_export_paths(
    project_directory: &str,
    class_name: &str,
    asset_name: &str,
    platform: HostPathPlatform,
    state: &mut super::asset_path::UniqueNameState,
    file_system: &impl ExportFileSystem,
) -> Result<PlannedDiagnosticPaths, AssetPathError> {
    plan_diagnostic_paths(
        DiagnosticPlanRequest {
            project_directory,
            diagnostic_directory: UNKNOWN_ASSETS_DIRECTORY,
            class_name,
            file_name: asset_name,
            extension: UNKNOWN_ASSET_EXTENSION,
            platform,
        },
        state,
        file_system,
    )
}

/// Mirror `UnreadableExportCollection.Export` destination selection without touching the disk.
///
/// The asset name passes through `FixInvalidPathCharacters`; the class name is used verbatim.
pub fn plan_unreadable_export_paths(
    project_directory: &str,
    class_name: &str,
    asset_name: &str,
    platform: HostPathPlatform,
    state: &mut super::asset_path::UniqueNameState,
    file_system: &impl ExportFileSystem,
) -> Result<PlannedDiagnosticPaths, AssetPathError> {
    let fixed_name = fix_invalid_path_characters(asset_name, platform)?;
    plan_diagnostic_paths(
        DiagnosticPlanRequest {
            project_directory,
            diagnostic_directory: UNREADABLE_ASSETS_DIRECTORY,
            class_name,
            file_name: &fixed_name,
            extension: UNREADABLE_ASSET_EXTENSION,
            platform,
        },
        state,
        file_system,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        plan_script_export_paths, plan_unknown_export_paths, plan_unreadable_export_paths,
        remove_assembly_file_extension, script_export_sub_path, scripts_folder_name,
        ExportFileSystem, HostPathPlatform,
    };
    use crate::export::asset_path::{join_path, UniqueNameState};
    use std::collections::BTreeSet;

    #[derive(Default)]
    struct TestFileSystem {
        directories: BTreeSet<String>,
        files: BTreeSet<String>,
    }

    impl TestFileSystem {
        fn joined(directory: &str, name: &str) -> String {
            join_path(directory, name, HostPathPlatform::Windows)
        }

        fn add_directory(&mut self, directory: &str) -> &mut Self {
            self.directories.insert(directory.to_owned());
            self
        }

        fn add_file(&mut self, directory: &str, name: &str) -> &mut Self {
            self.files.insert(Self::joined(directory, name));
            self
        }
    }

    impl ExportFileSystem for TestFileSystem {
        fn directory_exists(&self, directory: &str) -> bool {
            self.directories.contains(directory)
        }

        fn file_exists(&self, directory: &str, name: &str) -> bool {
            self.files.contains(&Self::joined(directory, name))
        }
    }

    #[test]
    fn assembly_helpers_follow_special_file_names_exactly() {
        assert_eq!(
            remove_assembly_file_extension("Assembly-CSharp.dll"),
            "Assembly-CSharp"
        );
        assert_eq!(
            remove_assembly_file_extension("Assembly-CSharp"),
            "Assembly-CSharp"
        );
        // Ordinal and case sensitive: only a trailing lowercase `.dll` is removed.
        assert_eq!(remove_assembly_file_extension("Plugin.DLL"), "Plugin.DLL");
        assert_eq!(
            remove_assembly_file_extension("nested.dll.bak"),
            "nested.dll.bak"
        );
        for firstpass in [
            "Assembly-CSharp-firstpass",
            "Assembly - CSharp - firstpass",
            "Assembly-UnityScript-firstpass",
            "Assembly - UnityScript - firstpass",
        ] {
            assert_eq!(scripts_folder_name(firstpass), "Plugins", "{firstpass}");
        }
        assert_eq!(scripts_folder_name("Assembly-CSharp"), "Scripts");
        assert_eq!(scripts_folder_name("Assembly-CSharp.dll"), "Scripts");
    }

    #[test]
    fn script_sub_paths_follow_assembly_namespace_and_class() {
        let (folder, file) = script_export_sub_path(
            "Assembly-CSharp.dll",
            "My.Game",
            "Player",
            HostPathPlatform::Windows,
        )
        .unwrap();
        assert_eq!(folder, "Scripts\\Assembly-CSharp\\My\\Game");
        assert_eq!(file, "Player.cs");

        let (folder, file) = script_export_sub_path(
            "Assembly-CSharp-firstpass.dll",
            "",
            "A<B>",
            HostPathPlatform::Windows,
        )
        .unwrap();
        assert_eq!(folder, "Plugins\\Assembly-CSharp-firstpass");
        assert_eq!(file, "A_B_.cs");
    }

    #[test]
    fn script_plans_join_below_the_assets_directory() {
        let planned = plan_script_export_paths(
            "Assembly-CSharp.dll",
            "My.Game",
            "Player",
            HostPathPlatform::Windows,
            "Project",
        )
        .unwrap();
        assert_eq!(
            planned.subdirectory,
            "Project\\Assets\\Scripts\\Assembly-CSharp\\My\\Game"
        );
        assert_eq!(planned.file_name, "Player.cs");
        assert_eq!(
            planned.file_path,
            "Project\\Assets\\Scripts\\Assembly-CSharp\\My\\Game\\Player.cs"
        );
        assert_eq!(
            planned.relative_asset_path,
            "Assets/Scripts/Assembly-CSharp/My/Game/Player.cs"
        );
    }

    #[test]
    fn hostile_names_flow_through_the_planners_like_the_source() {
        // Parity pin, not an endorsement: the frozen exporters sanitize none
        // of these shapes either. Unknown names stay fully verbatim,
        // unreadable names keep separators and dots through path fixing, and
        // script namespace dots become separators (so `..` components cannot
        // arise there, but empty ones can). Reaching disk additionally needs a
        // malicious bundle, and unknown assets need the non-default
        // unreadable-assets export enabled.
        let file_system = TestFileSystem::default();
        let mut state = UniqueNameState::default();
        let planned = plan_unknown_export_paths(
            "Project",
            "MonoBehaviour",
            "../../evil",
            HostPathPlatform::Windows,
            &mut state,
            &file_system,
        )
        .unwrap();
        assert_eq!(
            planned.subdirectory,
            "Project\\AssetRipper\\UnknownAssets\\MonoBehaviour"
        );
        assert_eq!(planned.file_name, "../../evil.unknown");

        let planned = plan_unreadable_export_paths(
            "Project",
            "Texture2D",
            "../../evil",
            HostPathPlatform::Windows,
            &mut state,
            &file_system,
        )
        .unwrap();
        assert_eq!(
            planned.subdirectory,
            "Project\\AssetRipper\\UnreadableAssets\\Texture2D"
        );
        assert_eq!(planned.file_name, "../../evil.unreadable");

        let (folder, file) = script_export_sub_path(
            "Assembly-CSharp.dll",
            "a..b",
            "Player",
            HostPathPlatform::Windows,
        )
        .unwrap();
        assert_eq!(folder, "Scripts\\Assembly-CSharp\\a\\\\b");
        assert_eq!(file, "Player.cs");
    }

    #[test]
    fn unknown_paths_keep_names_verbatim_with_unique_resolution() {
        let mut file_system = TestFileSystem::default();
        let mut state = UniqueNameState::default();
        // The name is used verbatim: no invalid-character fixing happens here.
        let planned = plan_unknown_export_paths(
            "Project",
            "MonoBehaviour",
            "A<B>",
            HostPathPlatform::Windows,
            &mut state,
            &file_system,
        )
        .unwrap();
        assert_eq!(
            planned.subdirectory,
            "Project\\AssetRipper\\UnknownAssets\\MonoBehaviour"
        );
        assert_eq!(planned.file_name, "A<B>.unknown");
        assert_eq!(
            planned.relative_path,
            "AssetRipper/UnknownAssets/MonoBehaviour/A<B>.unknown"
        );

        file_system
            .add_directory("Project\\AssetRipper\\UnknownAssets\\MonoBehaviour")
            .add_file(
                "Project\\AssetRipper\\UnknownAssets\\MonoBehaviour",
                "A<B>.unknown",
            );
        let planned = plan_unknown_export_paths(
            "Project",
            "MonoBehaviour",
            "A<B>",
            HostPathPlatform::Windows,
            &mut state,
            &file_system,
        )
        .unwrap();
        assert_eq!(planned.file_name, "A<B>_0.unknown");
    }

    #[test]
    fn unreadable_paths_fix_the_asset_name_before_unique_resolution() {
        let file_system = TestFileSystem::default();
        let mut state = UniqueNameState::default();
        let planned = plan_unreadable_export_paths(
            "Project",
            "Texture2D",
            "a<b>",
            HostPathPlatform::Windows,
            &mut state,
            &file_system,
        )
        .unwrap();
        assert_eq!(
            planned.subdirectory,
            "Project\\AssetRipper\\UnreadableAssets\\Texture2D"
        );
        assert_eq!(planned.file_name, "a_b_.unreadable");
        assert_eq!(
            planned.relative_path,
            "AssetRipper/UnreadableAssets/Texture2D/a_b_.unreadable"
        );
    }
}
