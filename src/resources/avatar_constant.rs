//! `AvatarConstantExtensions`.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/AvatarConstantExtensions.cs`. A default pose and
//! a separate skeleton-name id array were introduced in Unity 4.3; before that the skeleton pose
//! and the skeleton's own id array are the only sources.

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
}
