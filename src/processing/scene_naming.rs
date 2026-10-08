//! Scene file-name, version, and BuildSettings path rules.

/// Unity version fields needed by the scene collection naming rules.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SceneUnityVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl SceneUnityVersion {
    /// Parse the numeric Unity version prefix, accepting release suffixes such as `f1` or `p2`.
    pub fn parse(value: &str) -> Option<Self> {
        let bytes = value.as_bytes();
        let mut cursor = 0;
        let major = parse_decimal_component(bytes, &mut cursor)?;
        if bytes.get(cursor) != Some(&b'.') {
            return None;
        }
        cursor += 1;
        let minor = parse_decimal_component(bytes, &mut cursor)?;
        if bytes.get(cursor) != Some(&b'.') {
            return None;
        }
        cursor += 1;
        let patch = parse_decimal_component(bytes, &mut cursor)?;
        let suffix = &bytes[cursor..];
        if !suffix.is_empty()
            && (!suffix[0].is_ascii_alphabetic()
                || !suffix[1..].iter().all(u8::is_ascii_alphanumeric))
        {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
        })
    }

    /// Unity versions before 5.3 store the first scene under `maindata`.
    pub fn has_main_data(self) -> bool {
        self.major < 5 || (self.major == 5 && self.minor < 3)
    }
}

/// Map a serialized scene collection name to its BuildSettings index.
pub fn file_name_to_scene_index(name: &str, version: SceneUnityVersion) -> Option<usize> {
    if version.has_main_data() && name == "maindata" {
        return Some(0);
    }
    let suffix = name.strip_prefix("level")?;
    if suffix.is_empty()
        || (suffix.len() > 1 && suffix.starts_with('0'))
        || !suffix.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let level = suffix.parse::<i32>().ok()?;
    let index = if version.has_main_data() {
        level.checked_add(1)?
    } else {
        level
    };
    usize::try_from(index).ok()
}

/// Map a BuildSettings index to the source scene collection name.
pub fn scene_index_to_file_name(index: usize, version: SceneUnityVersion) -> Option<String> {
    if version.has_main_data() {
        if index == 0 {
            Some("maindata".to_owned())
        } else {
            Some(format!("level{}", index - 1))
        }
    } else {
        Some(format!("level{index}"))
    }
}

/// Recover the source path represented by one BuildSettings scene entry.
///
/// Paths under `Assets/` and `Library/PackageCache/` lose their extension. Rooted paths are
/// accepted only when one of those source prefixes occurs in them. Other nonempty paths are
/// placed under `Assets/Scenes/`, matching the source fallback.
pub fn scene_path_from_build_settings(scene_path: &str) -> Option<String> {
    if scene_path.starts_with("Assets/") || scene_path.starts_with("Library/PackageCache/") {
        return Some(remove_extension(scene_path).to_owned());
    }
    if is_rooted_path(scene_path) {
        for prefix in ["Assets/", "Library/PackageCache/"] {
            if let Some(index) = scene_path.find(prefix) {
                return Some(remove_extension(&scene_path[index..]).to_owned());
            }
        }
        return None;
    }
    if scene_path.is_empty() {
        return None;
    }
    Some(format!("Assets/Scenes/{scene_path}"))
}

/// Resolve a collection name through BuildSettings, returning no path for stale scene files.
pub fn scene_path_for_collection(
    collection_name: &str,
    version: SceneUnityVersion,
    build_settings_scenes: &[String],
) -> Option<String> {
    let index = file_name_to_scene_index(collection_name, version)?;
    let path = build_settings_scenes.get(index)?;
    scene_path_from_build_settings(path)
}

/// True when another BuildSettings entry has the exact same path.
pub fn is_scene_duplicate(scene_index: usize, scenes: &[String]) -> Option<bool> {
    let scene = scenes.get(scene_index)?;
    Some(
        scenes
            .iter()
            .enumerate()
            .any(|(index, candidate)| index != scene_index && candidate == scene),
    )
}

fn parse_decimal_component(bytes: &[u8], cursor: &mut usize) -> Option<u32> {
    let start = *cursor;
    while bytes.get(*cursor).is_some_and(u8::is_ascii_digit) {
        *cursor += 1;
    }
    if start == *cursor {
        return None;
    }
    std::str::from_utf8(&bytes[start..*cursor])
        .ok()?
        .parse()
        .ok()
}

fn remove_extension(path: &str) -> &str {
    let filename_start = path.rfind(['/', '\\']).map_or(0, |index| index + 1);
    let filename = &path[filename_start..];
    let Some(extension_start) = filename.rfind('.') else {
        return path;
    };
    if matches!(filename, "." | "..") || extension_start == 0 {
        return path;
    }
    &path[..filename_start + extension_start]
}

fn is_rooted_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with('/')
        || path.starts_with('\\')
        || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
}

#[cfg(test)]
mod tests {
    use super::{
        file_name_to_scene_index, is_scene_duplicate, scene_index_to_file_name,
        scene_path_for_collection, scene_path_from_build_settings, SceneUnityVersion,
    };

    fn version(value: &str) -> SceneUnityVersion {
        SceneUnityVersion::parse(value).unwrap()
    }

    #[test]
    fn applies_the_main_data_boundary_and_scene_name_pattern() {
        let old = version("5.2.4f1");
        let boundary = version("5.3.0f1");
        assert!(old.has_main_data());
        assert!(!boundary.has_main_data());
        assert_eq!(file_name_to_scene_index("maindata", old), Some(0));
        assert_eq!(file_name_to_scene_index("level0", old), Some(1));
        assert_eq!(file_name_to_scene_index("level0", boundary), Some(0));
        assert_eq!(
            scene_index_to_file_name(0, old).as_deref(),
            Some("maindata")
        );
        assert_eq!(scene_index_to_file_name(1, old).as_deref(), Some("level0"));
        assert_eq!(
            scene_index_to_file_name(0, boundary).as_deref(),
            Some("level0")
        );
        for invalid in [
            "Level0", "level", "level00", "level-1", "level1x", "maindata",
        ] {
            assert_eq!(file_name_to_scene_index(invalid, boundary), None);
        }
        assert_eq!(
            file_name_to_scene_index("level184467440737095516160", boundary),
            None
        );
        assert_eq!(file_name_to_scene_index("level2147483648", boundary), None);
        assert_eq!(file_name_to_scene_index("level2147483647", old), None);
    }

    #[test]
    fn scene_collection_names_match_reference_runtime_fixture() {
        for line in include_str!("scene_naming_fixture.txt").lines() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<_> = line.split('|').collect();
            match fields.as_slice() {
                ["INDEX", version_text, name, "True", index_text] => {
                    let version = version(version_text);
                    assert_eq!(
                        file_name_to_scene_index(name, version),
                        Some(index_text.parse().unwrap()),
                        "source row {line:?}"
                    );
                }
                ["INDEX", version_text, name, "False", "-1"] => {
                    assert_eq!(
                        file_name_to_scene_index(name, version(version_text)),
                        None,
                        "source row {line:?}"
                    );
                }
                ["FILENAME", version_text, index_text, name] => {
                    assert_eq!(
                        scene_index_to_file_name(
                            index_text.parse().unwrap(),
                            version(version_text)
                        )
                        .as_deref(),
                        Some(*name),
                        "source row {line:?}"
                    );
                }
                _ => panic!("invalid SceneHelpers fixture row {line:?}"),
            }
        }
    }

    #[test]
    fn parses_release_versions_without_accepting_malformed_numeric_fields() {
        assert_eq!(
            SceneUnityVersion::parse("2022.3.1f1"),
            Some(SceneUnityVersion {
                major: 2022,
                minor: 3,
                patch: 1,
            })
        );
        for invalid in ["", "5.3", "x.3.1f1", "5..1f1", "5.3.x", "5.3.1!"] {
            assert_eq!(SceneUnityVersion::parse(invalid), None);
        }
    }

    #[test]
    fn maps_build_settings_paths_and_rejects_stale_scene_indexes() {
        let old = version("5.2.4f1");
        let scenes = vec![
            "Assets/Scenes/Main.unity".to_owned(),
            "Assets/Scenes/Next.unity".to_owned(),
        ];
        assert_eq!(
            scene_path_for_collection("maindata", old, &scenes).as_deref(),
            Some("Assets/Scenes/Main")
        );
        assert_eq!(
            scene_path_for_collection("level0", old, &scenes).as_deref(),
            Some("Assets/Scenes/Next")
        );
        assert_eq!(scene_path_for_collection("level1", old, &scenes), None);
        assert_eq!(scene_path_for_collection("other", old, &scenes), None);
    }

    #[test]
    fn normalizes_build_settings_paths_like_the_source() {
        assert_eq!(
            scene_path_from_build_settings("Assets/Scenes/Level.unity").as_deref(),
            Some("Assets/Scenes/Level")
        );
        assert_eq!(
            scene_path_from_build_settings("Library/PackageCache/pkg/Scene.unity").as_deref(),
            Some("Library/PackageCache/pkg/Scene")
        );
        assert_eq!(
            scene_path_from_build_settings("C:\\project\\Assets/Scenes/Level.unity").as_deref(),
            Some("Assets/Scenes/Level")
        );
        assert_eq!(
            scene_path_from_build_settings("/project/Library/PackageCache/pkg/Scene.unity")
                .as_deref(),
            Some("Library/PackageCache/pkg/Scene")
        );
        assert_eq!(
            scene_path_from_build_settings("C:\\project\\Outside\\Level.unity"),
            None
        );
        assert_eq!(scene_path_from_build_settings(""), None);
        assert_eq!(
            scene_path_from_build_settings("Menu.unity").as_deref(),
            Some("Assets/Scenes/Menu.unity")
        );
    }

    #[test]
    fn compares_duplicate_build_settings_paths_exactly() {
        let scenes = vec![
            "Assets/First.unity".to_owned(),
            "Assets/Shared.unity".to_owned(),
            "Assets/Shared.unity".to_owned(),
            "assets/shared.unity".to_owned(),
        ];
        assert_eq!(is_scene_duplicate(0, &scenes), Some(false));
        assert_eq!(is_scene_duplicate(1, &scenes), Some(true));
        assert_eq!(is_scene_duplicate(2, &scenes), Some(true));
        assert_eq!(is_scene_duplicate(3, &scenes), Some(false));
        assert_eq!(is_scene_duplicate(4, &scenes), None);
    }
}
