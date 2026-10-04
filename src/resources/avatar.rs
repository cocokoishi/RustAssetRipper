//! `AvatarExtensions`.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/AvatarExtensions.cs`. A bone is resolved by
//! looking its hash up in the avatar's transform-origin-skeleton map.

use std::collections::HashMap;

/// `AvatarExtensions.FindBonePath`.
pub fn find_bone_path(tos: &HashMap<u32, String>, hash: u32) -> Option<&str> {
    tos.get(&hash).map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Differential against the reference `AvatarExtensions` (probe `Temp/opencode/avatarprobe`),
    /// which instantiated real generated `Avatar` objects for seven versions.
    #[test]
    fn find_bone_path_matches_the_reference_runtime() {
        let fixture = include_str!("avatar_fixture.txt");
        let tos = HashMap::from([
            (12345u32, "Bone/PathA".to_string()),
            (22222u32, "Bone/PathB".to_string()),
        ]);
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "AV", "{line}");
            assert_eq!(find_bone_path(&tos, 12345), Some(parts[3]), "{line}");
            assert_eq!(find_bone_path(&tos, 22222), Some(parts[4]), "{line}");
            assert_eq!(find_bone_path(&tos, 999), None, "{line}");
            rows += 1;
        }
        assert_eq!(rows, 7);
    }
}
