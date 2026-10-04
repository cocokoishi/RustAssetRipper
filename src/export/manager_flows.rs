//! Manager-family export flows ported from the project collections.
//!
//! Manager assets export to fixed `ProjectSettings/<Corrected>.asset` destinations (planned
//! in `project_plan.rs`) with three flow rules of their own: the export writes the asset and
//! always reports success without any `.meta` sidecar or unique-name resolution, the export
//! ID is the constant 1 for the owned asset (anything else fails), and export pointers are
//! never created (always throwing).
//!
//! Editor build settings add one step before that flow: every listed scene carrying a GUID
//! slot receives the container's GUID for its extension-stripped path, reusing the
//! scene-path lookup in `project_container.rs` and the extension rule in `scene_helpers.rs`.
//! Scenes without a GUID slot keep whatever they hold.
//!
//! Scene assets form their own one-member collection: the name is `{Scene} (SceneAsset)`,
//! a missing target scene fails instead of exporting, the export itself writes nothing yet
//! still reports success, membership is asset identity, and both ID and pointer lookups
//! ignore their arguments — the ID is the main export ID of the `DefaultAsset` class and
//! the pointer always carries the target scene GUID as `Meta`, even for local references.
//! That class ID lives in the external generated package, so callers supply it.

use super::ids::{get_main_export_id, ExportIdError};

/// The fixed export ID of a manager asset.
pub const MANAGER_EXPORT_ID: i64 = 1;

/// The display suffix of scene-asset collections.
pub const SCENE_ASSET_NAME_SUFFIX: &str = " (SceneAsset)";

/// Why a manager-family lookup fails.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ManagerFlowError {
    /// The asset is not the owned manager asset.
    UnknownAsset,
    /// Manager collections never create export pointers.
    PointersUnsupported,
    /// The scene asset has no target scene.
    MissingTargetScene,
}

impl core::fmt::Display for ManagerFlowError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnknownAsset => write!(formatter, "the asset is not the manager asset"),
            Self::PointersUnsupported => {
                write!(formatter, "manager collections have no export pointers")
            }
            Self::MissingTargetScene => write!(formatter, "the scene asset has no target scene"),
        }
    }
}

impl std::error::Error for ManagerFlowError {}

/// Mirror `ManagerExportCollection.GetExportID`: the constant 1 for the owned asset.
pub fn manager_export_id(is_owned_asset: bool) -> Result<i64, ManagerFlowError> {
    if is_owned_asset {
        Ok(MANAGER_EXPORT_ID)
    } else {
        Err(ManagerFlowError::UnknownAsset)
    }
}

/// Mirror `ManagerExportCollection.CreateExportPointer`: always throwing.
pub fn manager_export_pointer() -> Result<core::convert::Infallible, ManagerFlowError> {
    Err(ManagerFlowError::PointersUnsupported)
}

/// One build-settings scene entry for GUID stamping.
#[derive(Clone, Debug, PartialEq)]
pub struct BuildSceneEntry {
    /// `scene.Has_Guid()`: only slotted scenes receive the container GUID.
    pub has_guid_slot: bool,
    /// The settings scene path, extension included.
    pub path: String,
    /// The current GUID words, replaced on stamping.
    pub guid: [u32; 4],
}

/// Mirror `SetSceneGuidValues`: stamp slotted scenes with the container GUID of their
/// extension-stripped path, returning how many entries changed.
pub fn stamp_scene_guids(
    scenes: &mut [BuildSceneEntry],
    lookup: impl Fn(&str) -> [u32; 4],
) -> usize {
    let mut stamped = 0;
    for scene in scenes.iter_mut() {
        if !scene.has_guid_slot {
            continue;
        }
        scene.guid = lookup(super::scene_helpers::strip_file_extension(&scene.path));
        stamped += 1;
    }
    stamped
}

/// Mirror the scene-asset display name: `{Scene} (SceneAsset)`.
pub fn scene_asset_name(scene_name: &str) -> String {
    format!("{scene_name}{SCENE_ASSET_NAME_SUFFIX}")
}

/// Mirror the static scene-asset export ID: the main export ID of the `DefaultAsset` class.
pub fn scene_asset_export_id(default_asset_class_id: i32) -> Result<i64, ExportIdError> {
    get_main_export_id(default_asset_class_id, 0)
}

#[cfg(test)]
mod tests {
    use super::{
        manager_export_id, manager_export_pointer, scene_asset_export_id, scene_asset_name,
        stamp_scene_guids, BuildSceneEntry, ManagerFlowError, MANAGER_EXPORT_ID,
        SCENE_ASSET_NAME_SUFFIX,
    };

    #[test]
    fn manager_ids_and_pointers_follow_fixed_rules() {
        assert_eq!(MANAGER_EXPORT_ID, 1);
        assert_eq!(manager_export_id(true), Ok(1));
        assert_eq!(
            manager_export_id(false),
            Err(ManagerFlowError::UnknownAsset)
        );
        assert_eq!(
            manager_export_pointer(),
            Err(ManagerFlowError::PointersUnsupported)
        );
    }

    #[test]
    fn scene_guids_stamp_slotted_entries_by_stripped_path() {
        let mut scenes = [
            BuildSceneEntry {
                has_guid_slot: true,
                path: "Assets/A.unity".to_owned(),
                guid: [0, 0, 0, 0],
            },
            BuildSceneEntry {
                has_guid_slot: false,
                path: "Assets/B.unity".to_owned(),
                guid: [7, 7, 7, 7],
            },
            BuildSceneEntry {
                has_guid_slot: true,
                path: "Assets/A.unity".to_owned(),
                guid: [0, 0, 0, 0],
            },
        ];
        let stamped = stamp_scene_guids(&mut scenes, |stripped| {
            if stripped == "Assets/A" {
                [1, 2, 3, 4]
            } else {
                [0, 0, 0, 0]
            }
        });
        assert_eq!(stamped, 2);
        assert_eq!(scenes[0].guid, [1, 2, 3, 4]);
        // Unslotted scenes keep whatever they hold.
        assert_eq!(scenes[1].guid, [7, 7, 7, 7]);
        assert_eq!(scenes[2].guid, [1, 2, 3, 4]);
    }

    #[test]
    fn scene_asset_names_and_ids_follow_collection_rules() {
        assert_eq!(scene_asset_name("Main"), "Main (SceneAsset)");
        assert_eq!(SCENE_ASSET_NAME_SUFFIX, " (SceneAsset)");
        // The DefaultAsset class ID arrives from the generated package; any class ID maps
        // through the main export ID rule.
        assert_eq!(scene_asset_export_id(33), Ok(3_300_000));
    }
}
