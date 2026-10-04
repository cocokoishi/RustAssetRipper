//! Scene file/index/path rules ported from `SceneHelpers`.
//!
//! Scene files are named `maindata` (index 0, only before Unity 5.3) and `levelN` otherwise,
//! with the index shifted by one across the 5.3 boundary: `levelN` is scene `N + 1` before 5.3
//! and scene `N` from 5.3 on. The level-name shape is `^level(0|([1-9][0-9]*))$`, so `level01`
//! never matches. Overflowing digit strings fail explicitly here; the source would throw.
//!
//! Build-settings scenes are deduplicated by exact path: a scene whose path string appears at
//! any other position is a duplicate. A missing settings list is never a duplicate, and an
//! out-of-range index is reported instead of throwing like the source indexer would.
//!
//! Collection-to-path mapping strips the file extension, keeps `Assets/`- and
//! `Library/PackageCache/`-rooted paths (minus extension), searches rooted paths for the
//! first embedded `Assets/` (falling back to `Library/PackageCache/`), and joins anything
//! else below `Assets/Scenes`. Over-long indices from stale builds and empty scene names map
//! to nothing. Rootedness follows the host filesystem in the source, so callers supply it as
//! a fact; all comparisons are ordinal, and emitted separators are `/` (Unity asset paths)
//! with platform joining left to the path planners.
//!
//! Scene-compatible assets are GameObjects, level-game managers, components (including
//! MonoBehaviours only when they sit on a GameObject), and prefab instances.

use crate::processing::scene_naming::SceneUnityVersion;

/// File stem of the main-data scene, valid only before Unity 5.3.
pub const MAIN_SCENE_NAME: &str = "maindata";
/// File stem prefix of numbered scenes.
pub const LEVEL_SCENE_PREFIX: &str = "level";
/// Asset-path prefix kept verbatim by scene-path mapping.
pub const ASSETS_PREFIX: &str = "Assets/";
/// Package-cache prefix kept verbatim by scene-path mapping.
pub const PACKAGE_CACHE_PREFIX: &str = "Library/PackageCache/";

/// `SceneHelpers.HasMainData`: versions before 5.3 use the main-data layout.
pub fn has_main_data(version: SceneUnityVersion) -> bool {
    version.major < 5 || (version.major == 5 && version.minor < 3)
}

/// The `^level(0|([1-9][0-9]*))$` tail check behind `SceneNameFormat`.
pub fn is_level_scene_name(tail: &str) -> bool {
    if tail == "0" {
        return true;
    }
    let mut chars = tail.chars();
    match chars.next() {
        Some(first) if ('1'..='9').contains(&first) => chars.all(|c| c.is_ascii_digit()),
        _ => false,
    }
}

/// `SceneHelpers.TryGetFileNameToSceneIndex`.
///
/// Returns the scene index, or `None` when the name matches no scene (the source `false`
/// path) or its digits overflow (the source would throw).
pub fn file_name_to_scene_index(name: &str, version: SceneUnityVersion) -> Option<i32> {
    if has_main_data(version) {
        if name == MAIN_SCENE_NAME {
            return Some(0);
        }
        if let Some(tail) = name.strip_prefix(LEVEL_SCENE_PREFIX) {
            if is_level_scene_name(tail) {
                return tail.parse::<i32>().ok()?.checked_add(1);
            }
        }
        return None;
    }
    if let Some(tail) = name.strip_prefix(LEVEL_SCENE_PREFIX) {
        if is_level_scene_name(tail) {
            return tail.parse::<i32>().ok();
        }
    }
    None
}

/// `SceneHelpers.SceneIndexToFileName`, the inverse mapping.
pub fn scene_index_to_file_name(index: i32, version: SceneUnityVersion) -> String {
    if has_main_data(version) {
        if index == 0 {
            return MAIN_SCENE_NAME.to_owned();
        }
        return format!("{LEVEL_SCENE_PREFIX}{}", index - 1);
    }
    format!("{LEVEL_SCENE_PREFIX}{index}")
}

/// `SceneHelpers.IsSceneDuplicate` over caller-supplied build-settings scene paths.
///
/// `None` means no settings were supplied (never a duplicate in source) or the index is out
/// of range (the source indexer would throw there).
pub fn is_scene_duplicate(scene_index: usize, build_scenes: Option<&[&str]>) -> Option<bool> {
    let scenes = build_scenes?;
    let name = scenes.get(scene_index)?;
    Some(
        scenes
            .iter()
            .enumerate()
            .any(|(other, other_name)| other != scene_index && *other_name == *name),
    )
}

/// Strip the file extension the way `Path.GetExtension` plus range-trim does.
///
/// A missing extension trims nothing (`[..^0]` is the whole string in source).
pub fn strip_file_extension(path: &str) -> &str {
    match path.rfind('.') {
        Some(dot) => &path[..dot],
        None => path,
    }
}

/// `SceneHelpers.TryGetScenePath` for one collection.
///
/// `collection_name` is the file name, `original_version` its version, `build_scenes` the
/// settings list (`None` stands in for null settings), and `is_absolute` the host
/// rootedness verdict for the resolved scene path. The returned asset path uses `/`.
pub fn scene_path_for_collection(
    collection_name: &str,
    original_version: SceneUnityVersion,
    build_scenes: Option<&[&str]>,
    is_absolute: bool,
) -> Option<String> {
    let scenes = build_scenes?;
    let index = file_name_to_scene_index(collection_name, original_version)? as usize;
    if index >= scenes.len() {
        // A stale scene file surviving a rebuild maps to nothing.
        return None;
    }
    let scene_path = scenes[index];
    let without_extension = strip_file_extension(scene_path);
    if scene_path.starts_with(ASSETS_PREFIX) || scene_path.starts_with(PACKAGE_CACHE_PREFIX) {
        return Some(without_extension.to_owned());
    }
    if is_absolute {
        // Absolute project paths may embed Assets/ anywhere; package cache is the fallback.
        // The NOTE in source admits a wrong cut when Assets/ appears in a directory name.
        let start = without_extension
            .find(ASSETS_PREFIX)
            .or_else(|| without_extension.find(PACKAGE_CACHE_PREFIX))?;
        return Some(without_extension[start..].to_owned());
    }
    if scene_path.is_empty() {
        // Unity emits an empty name when a build includes no scenes.
        return None;
    }
    Some(format!("Assets/Scenes/{scene_path}"))
}

/// The asset kinds `SceneHelpers.IsSceneCompatible` dispatches on, in source arm order.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SceneCandidateKind {
    GameObject,
    LevelGameManager,
    MonoBehaviour { on_game_object: bool },
    Component,
    PrefabInstance,
    Other,
}

/// `SceneHelpers.IsSceneCompatible`.
pub const fn is_scene_compatible(kind: SceneCandidateKind) -> bool {
    match kind {
        SceneCandidateKind::GameObject
        | SceneCandidateKind::LevelGameManager
        | SceneCandidateKind::Component
        | SceneCandidateKind::PrefabInstance => true,
        SceneCandidateKind::MonoBehaviour { on_game_object } => on_game_object,
        SceneCandidateKind::Other => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        file_name_to_scene_index, has_main_data, is_level_scene_name, is_scene_compatible,
        is_scene_duplicate, scene_index_to_file_name, scene_path_for_collection,
        strip_file_extension, SceneCandidateKind, ASSETS_PREFIX,
    };
    use crate::processing::scene_naming::SceneUnityVersion;

    fn version(text: &str) -> SceneUnityVersion {
        SceneUnityVersion::parse(text).unwrap()
    }

    #[test]
    fn main_data_layout_changes_at_unity_53() {
        let legacy = version("5.2.4f1");
        let boundary = version("5.3.0f1");
        assert!(has_main_data(legacy));
        assert!(!has_main_data(boundary));
        assert_eq!(file_name_to_scene_index("maindata", legacy), Some(0));
        assert_eq!(file_name_to_scene_index("maindata", boundary), None);
        // levelN shifts by one across the boundary.
        assert_eq!(file_name_to_scene_index("level0", legacy), Some(1));
        assert_eq!(file_name_to_scene_index("level0", boundary), Some(0));
        assert_eq!(file_name_to_scene_index("level3", legacy), Some(4));
        assert_eq!(file_name_to_scene_index("level3", boundary), Some(3));
        // The tail shape rejects padded and non-numeric names on both sides.
        for name in ["level", "level01", "level1a", "Level2", "mainscene"] {
            assert_eq!(file_name_to_scene_index(name, legacy), None, "{name}");
            assert_eq!(file_name_to_scene_index(name, boundary), None, "{name}");
        }
        assert!(is_level_scene_name("0"));
        assert!(is_level_scene_name("10"));
        assert!(!is_level_scene_name("01"));
        assert!(!is_level_scene_name(""));
    }

    #[test]
    fn index_to_file_name_round_trips_on_both_layouts() {
        let legacy = version("5.2.4f1");
        let modern = version("2022.3.1f1");
        assert_eq!(scene_index_to_file_name(0, legacy), "maindata");
        assert_eq!(scene_index_to_file_name(1, legacy), "level0");
        assert_eq!(scene_index_to_file_name(0, modern), "level0");
        for name in ["maindata", "level0", "level9"] {
            let index = file_name_to_scene_index(name, legacy).unwrap();
            assert_eq!(scene_index_to_file_name(index, legacy), name);
        }
        for index in [0, 1, 42] {
            let name = scene_index_to_file_name(index, modern);
            assert_eq!(file_name_to_scene_index(&name, modern), Some(index));
        }
    }

    #[test]
    fn duplicates_need_a_second_position_and_valid_inputs() {
        let scenes = ["Assets/A.unity", "Assets/B.unity", "Assets/A.unity"];
        assert_eq!(is_scene_duplicate(0, Some(&scenes)), Some(true));
        assert_eq!(is_scene_duplicate(1, Some(&scenes)), Some(false));
        assert_eq!(is_scene_duplicate(2, Some(&scenes)), Some(true));
        // No settings is never a duplicate; an out-of-range index is reported.
        assert_eq!(is_scene_duplicate(0, None), None);
        assert_eq!(is_scene_duplicate(9, Some(&scenes)), None);
    }

    #[test]
    fn collection_paths_follow_prefix_rooted_and_bare_branches() {
        let modern = version("2022.3.1f1");
        let scenes = ["Assets/Scenes/Main.unity", "Assets/A.unity"];
        // Prefixed paths keep their stem.
        assert_eq!(
            scene_path_for_collection("level0", modern, Some(&scenes), false),
            Some("Assets/Scenes/Main".to_owned())
        );
        let package = ["Library/PackageCache/com.x/y.unity"];
        assert_eq!(
            scene_path_for_collection("level0", modern, Some(&package), false),
            Some("Library/PackageCache/com.x/y".to_owned())
        );
        // Absolute paths cut at the first embedded prefix, package cache as fallback.
        let absolute_scenes = ["C:/b/keep.unity", "C:/b/Assets/A.unity"];
        assert_eq!(
            scene_path_for_collection("level1", modern, Some(&absolute_scenes), true),
            Some("Assets/A".to_owned())
        );
        let absolute_package = ["C:/b/keep.unity", "C:/b/Library/PackageCache/c.unity"];
        assert_eq!(
            scene_path_for_collection("level1", modern, Some(&absolute_package), true),
            Some("Library/PackageCache/c".to_owned())
        );
        let absolute_miss = ["C:/b/keep.unity", "C:/b/nope.unity"];
        assert_eq!(
            scene_path_for_collection("level1", modern, Some(&absolute_miss), true),
            None
        );
        // Bare names join below Assets/Scenes; empty names and stale indices map to nothing.
        assert_eq!(
            scene_path_for_collection("level0", modern, Some(&["main"]), false),
            Some("Assets/Scenes/main".to_owned())
        );
        assert_eq!(
            scene_path_for_collection("level0", modern, Some(&[""]), false),
            None
        );
        assert_eq!(
            scene_path_for_collection("level9", modern, Some(&scenes), false),
            None
        );
        assert_eq!(
            scene_path_for_collection("unknown", modern, Some(&scenes), false),
            None
        );
        assert_eq!(
            scene_path_for_collection("level0", modern, None, false),
            None
        );
        // Extension stripping trims the last suffix only, or nothing.
        assert_eq!(strip_file_extension("a.b.unity"), "a.b");
        assert_eq!(strip_file_extension("plain"), "plain");
        assert_eq!(ASSETS_PREFIX, "Assets/");
    }

    #[test]
    fn compatibility_follows_the_source_switch_order() {
        use SceneCandidateKind::{
            Component, GameObject, LevelGameManager, MonoBehaviour, Other, PrefabInstance,
        };
        assert!(is_scene_compatible(GameObject));
        assert!(is_scene_compatible(LevelGameManager));
        assert!(is_scene_compatible(Component));
        assert!(is_scene_compatible(PrefabInstance));
        assert!(is_scene_compatible(MonoBehaviour {
            on_game_object: true
        }));
        assert!(!is_scene_compatible(MonoBehaviour {
            on_game_object: false
        }));
        assert!(!is_scene_compatible(Other));
    }
}
