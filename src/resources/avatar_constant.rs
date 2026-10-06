//! `AvatarConstantExtensions` plus state-motion leaf resolution.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/AvatarConstantExtensions.cs`. A default pose and
//! a separate skeleton-name id array were introduced in Unity 4.3; before that the skeleton pose
//! and the skeleton's own id array are the only sources.
//!
//! Also mirrors the caller-independent index math of
//! `VirtualAnimationFactory.CreateMotion`: resolving a blend-leaf clip id through the state's
//! `LeafInfoArray` to a controller clip index. It lives here (rather than with the controller
//! processor) because only this pure math ports — asset creation, the null-motion rules around
//! it, and clip resolution stay caller-side.

/// A skeleton-pose-constant view (the part the accessors select on).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SkeletonPoseView {
    /// `Data.ID`, used to distinguish the selected pose in tests.
    pub id: Vec<u32>,
}

/// An avatar-constant view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AvatarConstantView {
    /// `DefaultPose`: present only from Unity 4.3.
    pub default_pose: Option<SkeletonPoseView>,
    /// `SkeletonPose` (`AvatarSkeletonPose` on old versions).
    pub skeleton_pose: SkeletonPoseView,
    /// `SkeletonNameIDArray`: present only from Unity 4.3.
    pub skeleton_name_id_array: Option<Vec<u32>>,
    /// `Skeleton.Data.ID`.
    pub skeleton_data_id: Vec<u32>,
}

impl AvatarConstantView {
    /// `AvatarConstantExtensions.GetDefaultPose`.
    pub fn get_default_pose(&self) -> &SkeletonPoseView {
        self.default_pose.as_ref().unwrap_or(&self.skeleton_pose)
    }

    /// `AvatarConstantExtensions.GetSkeletonNameIDArray`.
    pub fn get_skeleton_name_id_array(&self) -> &[u32] {
        self.skeleton_name_id_array
            .as_deref()
            .unwrap_or(&self.skeleton_data_id)
    }
}

/// A `LeafInfoConstant` entry as the motion resolver reads it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LeafInfoEntry {
    /// `IndexOffset`, added to the id position.
    pub index_offset: u32,
    /// `IDArray`: the clip ids covered by this entry.
    pub id_array: Vec<u32>,
}

/// `VirtualAnimationFactory.CreateMotion` clip-index resolution over a state's leaf data.
///
/// With a `LeafInfoArray`, the first entry whose `IDArray` contains `clip_id` wins and the
/// index is `IndexOffset + position`, wrapping like the source unchecked cast; no match
/// reads -1. Without the array the raw clip id wraps to `i32` (so `u32::MAX` also reads
/// -1). Callers treat -1 as a null motion and resolve non-negative indexes against the
/// controller clips.
pub fn resolve_motion_clip_index(
    has_leaf_info_array: bool,
    leaf_infos: &[LeafInfoEntry],
    clip_id: u32,
) -> i32 {
    if !has_leaf_info_array {
        return clip_id as i32;
    }
    for leaf in leaf_infos {
        if let Some(position) = leaf.id_array.iter().position(|id| *id == clip_id) {
            return (leaf.index_offset as i32).wrapping_add(position as i32);
        }
    }
    -1
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Byte-for-byte differential against the reference `AvatarConstantExtensions` (probe
    /// `Temp/opencode/avatarprobe`).
    #[test]
    fn avatar_constant_matches_the_reference_runtime() {
        let fixture = include_str!("avatar_constant_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "AVC", "{line}");
            let view = AvatarConstantView {
                default_pose: (parts[2] == "1").then_some(SkeletonPoseView { id: vec![1] }),
                skeleton_pose: SkeletonPoseView { id: vec![2] },
                skeleton_name_id_array: (parts[3] == "1").then_some(vec![3, 4]),
                skeleton_data_id: vec![5, 6],
            };
            let pose_source = if view.get_default_pose().id == [1] {
                "DefaultPose"
            } else {
                "SkeletonPose"
            };
            let name_source = if view.get_skeleton_name_id_array() == [3, 4] {
                "SkeletonNameIDArray"
            } else {
                "SkeletonID"
            };
            assert_eq!(pose_source, parts[4], "{line}");
            assert_eq!(name_source, parts[5], "{line}");
            rows += 1;
        }
        assert_eq!(rows, 7);
    }

    /// `CreateMotion` leaf resolution: first-match offset arithmetic, the -1 miss, and the
    /// raw wrap without a leaf array.
    #[test]
    fn motion_clip_index_resolution_matches_the_source_branches() {
        let leaves = [
            LeafInfoEntry {
                index_offset: 10,
                id_array: vec![4, 5],
            },
            LeafInfoEntry {
                index_offset: 20,
                id_array: vec![5, 6],
            },
        ];
        // The first entry holding the id wins: offset plus position.
        assert_eq!(resolve_motion_clip_index(true, &leaves, 4), 10);
        assert_eq!(resolve_motion_clip_index(true, &leaves, 5), 11);
        assert_eq!(resolve_motion_clip_index(true, &leaves, 6), 21);
        // No entry holding the id reads -1, as does an empty array.
        assert_eq!(resolve_motion_clip_index(true, &leaves, 7), -1);
        assert_eq!(resolve_motion_clip_index(true, &[], 4), -1);
        // Without the array the raw id wraps like the unchecked cast.
        assert_eq!(resolve_motion_clip_index(false, &leaves, 4), 4);
        assert_eq!(resolve_motion_clip_index(false, &leaves, u32::MAX), -1);
    }
}
