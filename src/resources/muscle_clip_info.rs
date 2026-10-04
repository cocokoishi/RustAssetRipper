//! `MuscleClipInfoExtensions`.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/MuscleClipInfoExtensions.cs`. The initialization
//! either applies the neutral defaults or copies every humanoid root-motion field from a
//! `MuscleClipConstant`.

/// The `MuscleClipConstant` source fields.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MuscleClipConstantView {
    /// `StartTime`.
    pub start_time: f32,
    /// `StopTime`.
    pub stop_time: f32,
    /// `OrientationOffsetY`.
    pub orientation_offset_y: f32,
    /// `Level`.
    pub level: f32,
    /// `CycleOffset`.
    pub cycle_offset: f32,
    /// `LoopTime`.
    pub loop_time: bool,
    /// `LoopBlend`.
    pub loop_blend: bool,
    /// `LoopBlendOrientation`.
    pub loop_blend_orientation: bool,
    /// `LoopBlendPositionY`.
    pub loop_blend_position_y: bool,
    /// `LoopBlendPositionXZ`.
    pub loop_blend_position_xz: bool,
    /// `KeepOriginalOrientation`.
    pub keep_original_orientation: bool,
    /// `KeepOriginalPositionY`.
    pub keep_original_position_y: bool,
    /// `KeepOriginalPositionXZ`.
    pub keep_original_position_xz: bool,
    /// `HeightFromFeet`.
    pub height_from_feet: bool,
}

/// The `MuscleClipInfo` destination fields.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MuscleClipInfoView {
    /// `StartTime`.
    pub start_time: f32,
    /// `StopTime`.
    pub stop_time: f32,
    /// `OrientationOffsetY`.
    pub orientation_offset_y: f32,
    /// `Level`.
    pub level: f32,
    /// `CycleOffset`.
    pub cycle_offset: f32,
    /// `LoopTime`.
    pub loop_time: bool,
    /// `LoopBlend`.
    pub loop_blend: bool,
    /// `LoopBlendOrientation`.
    pub loop_blend_orientation: bool,
    /// `LoopBlendPositionY`.
    pub loop_blend_position_y: bool,
    /// `LoopBlendPositionXZ`.
    pub loop_blend_position_xz: bool,
    /// `KeepOriginalOrientation`.
    pub keep_original_orientation: bool,
    /// `KeepOriginalPositionY`.
    pub keep_original_position_y: bool,
    /// `KeepOriginalPositionXZ`.
    pub keep_original_position_xz: bool,
    /// `HeightFromFeet`.
    pub height_from_feet: bool,
    /// `AdditiveReferencePoseTime`.
    pub additive_reference_pose_time: f32,
    /// `HasAdditiveReferencePose`.
    pub has_additive_reference_pose: bool,
    /// `Mirror`.
    pub mirror: bool,
}

/// `MuscleClipInfoExtensions.Initialize(info)`.
pub fn initialize(info: &mut MuscleClipInfoView) {
    info.stop_time = 1.0;
    info.keep_original_position_y = true;
}

/// `MuscleClipInfoExtensions.Initialize(info, muscleConst)`.
pub fn initialize_from_constant(
    info: &mut MuscleClipInfoView,
    muscle_const: &MuscleClipConstantView,
) {
    info.additive_reference_pose_time = 0.0;
    info.start_time = muscle_const.start_time;
    info.stop_time = muscle_const.stop_time;
    info.orientation_offset_y = muscle_const.orientation_offset_y;
    info.level = muscle_const.level;
    info.cycle_offset = muscle_const.cycle_offset;
    info.has_additive_reference_pose = false;
    info.loop_time = muscle_const.loop_time;
    info.loop_blend = muscle_const.loop_blend;
    info.loop_blend_orientation = muscle_const.loop_blend_orientation;
    info.loop_blend_position_y = muscle_const.loop_blend_position_y;
    info.loop_blend_position_xz = muscle_const.loop_blend_position_xz;
    info.keep_original_orientation = muscle_const.keep_original_orientation;
    info.keep_original_position_y = muscle_const.keep_original_position_y;
    info.keep_original_position_xz = muscle_const.keep_original_position_xz;
    info.height_from_feet = muscle_const.height_from_feet;
    info.mirror = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn constant() -> MuscleClipConstantView {
        MuscleClipConstantView {
            start_time: 1.1,
            stop_time: 2.2,
            orientation_offset_y: 3.3,
            level: 4.4,
            cycle_offset: 5.5,
            loop_time: true,
            loop_blend: true,
            loop_blend_orientation: true,
            loop_blend_position_y: true,
            loop_blend_position_xz: true,
            keep_original_orientation: true,
            keep_original_position_y: true,
            keep_original_position_xz: true,
            height_from_feet: true,
        }
    }

    /// Byte-for-byte differential against the reference `MuscleClipInfoExtensions` (probe
    /// `Temp/opencode/mcprobe`).
    #[test]
    fn muscle_clip_info_initialize_matches_the_reference_runtime() {
        let fixture = include_str!("muscle_clip_info_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "MC", "{line}");
            let fields: Vec<&str> = parts[2].split(',').collect();
            assert_eq!(fields.len(), 17, "{line}");

            let mut info = MuscleClipInfoView::default();
            initialize_from_constant(&mut info, &constant());
            assert_eq!(
                info.start_time.to_bits(),
                fields[0].parse::<u32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                info.stop_time.to_bits(),
                fields[1].parse::<u32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                info.orientation_offset_y.to_bits(),
                fields[2].parse::<u32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                info.level.to_bits(),
                fields[3].parse::<u32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                info.cycle_offset.to_bits(),
                fields[4].parse::<u32>().unwrap(),
                "{line}"
            );
            assert_eq!(info.loop_time, fields[5] == "1", "{line}");
            assert_eq!(info.loop_blend, fields[6] == "1", "{line}");
            assert_eq!(info.loop_blend_orientation, fields[7] == "1", "{line}");
            assert_eq!(info.loop_blend_position_y, fields[8] == "1", "{line}");
            assert_eq!(info.loop_blend_position_xz, fields[9] == "1", "{line}");
            assert_eq!(info.keep_original_orientation, fields[10] == "1", "{line}");
            assert_eq!(info.keep_original_position_y, fields[11] == "1", "{line}");
            assert_eq!(info.keep_original_position_xz, fields[12] == "1", "{line}");
            assert_eq!(info.height_from_feet, fields[13] == "1", "{line}");
            assert_eq!(
                info.additive_reference_pose_time.to_bits(),
                fields[14].parse::<u32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                info.has_additive_reference_pose,
                fields[15] == "1",
                "{line}"
            );
            assert_eq!(info.mirror, fields[16] == "1", "{line}");

            let mut plain = MuscleClipInfoView {
                stop_time: 9.9,
                keep_original_position_y: false,
                ..MuscleClipInfoView::default()
            };
            initialize(&mut plain);
            assert_eq!(
                plain.stop_time.to_bits(),
                parts[3].parse::<u32>().unwrap(),
                "{line}"
            );
            assert_eq!(plain.keep_original_position_y, parts[4] == "1", "{line}");
            rows += 1;
        }
        assert_eq!(rows, 5);
    }
}
