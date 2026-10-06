//! `AvatarExtensions` plus the humanoid muscle binding names.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/AvatarExtensions.cs` (a bone is resolved by
//! looking its hash up in the avatar's transform-origin-skeleton map) and
//! `.../Enums/AnimationClip/HumanoidMuscleType.cs` with its `Bones` sub-tables: the versioned
//! `Update` shifts and the `ToAttributeString` binding names consumed by the animation-clip
//! converter for humanoid curves.
//!
//! Recorded opens: `GenericBinding.GetHumanoidMuscle` (the attribute-to-muscle cast) and the
//! converter call site stay caller-side; out-of-range muscle ids read `None` here where the
//! source throws `ArgumentException`.

use std::collections::HashMap;

use super::vertex_data_blob::UnityVersion;

/// `AvatarExtensions.FindBonePath`.
pub fn find_bone_path(tos: &HashMap<u32, String>, hash: u32) -> Option<&str> {
    tos.get(&hash).map(String::as_str)
}

/// `LimbType.Last`: the four limbs scaled by the 7 transform channels.
pub const LIMB_TYPE_LAST: i32 = 4;
/// `MuscleType.Last`: 55 named muscles.
pub const MUSCLE_TYPE_LAST: i32 = 55;
/// `ArmType.Last`: both hands.
pub const ARM_TYPE_LAST: i32 = 2;
/// `FingerType.Last`: thumb through little.
pub const FINGER_TYPE_LAST: i32 = 5;
/// `FingerDoFType.Last`: four degrees of freedom per finger.
pub const FINGER_DOF_TYPE_LAST: i32 = 4;
/// `TDoFBoneType.Last`: 21 bones with translation degrees of freedom.
pub const TDOF_BONE_TYPE_LAST: i32 = 21;
/// `BoneType.Last`: 25 named bones.
pub const BONE_TYPE_LAST: i32 = 25;

/// `HumanoidMuscleType.Motion`.
pub const HUMANOID_MUSCLE_MOTION: i32 = 0;
/// `HumanoidMuscleType.Root`: `Motion + 7`.
pub const HUMANOID_MUSCLE_ROOT: i32 = HUMANOID_MUSCLE_MOTION + 7;
/// `HumanoidMuscleType.Limbs`: `Root + 7`.
pub const HUMANOID_MUSCLE_LIMBS: i32 = HUMANOID_MUSCLE_ROOT + 7;
/// `HumanoidMuscleType.Muscles`: `Limbs + LimbType.Last * 7`.
pub const HUMANOID_MUSCLE_MUSCLES: i32 = HUMANOID_MUSCLE_LIMBS + LIMB_TYPE_LAST * 7;
/// `HumanoidMuscleType.Fingers`: `Muscles + MuscleType.Last`.
pub const HUMANOID_MUSCLE_FINGERS: i32 = HUMANOID_MUSCLE_MUSCLES + MUSCLE_TYPE_LAST;
/// `HumanoidMuscleType.TDoFBones`: `Fingers + ArmType.Last * FingerType.Last * FingerDoFType.Last`.
pub const HUMANOID_MUSCLE_TDOF_BONES: i32 =
    HUMANOID_MUSCLE_FINGERS + ARM_TYPE_LAST * FINGER_TYPE_LAST * FINGER_DOF_TYPE_LAST;
/// `HumanoidMuscleType.Last`: `TDoFBones + TDoFBoneType.Last * 3`.
pub const HUMANOID_MUSCLE_LAST: i32 = HUMANOID_MUSCLE_TDOF_BONES + TDOF_BONE_TYPE_LAST * 3;

/// `BoneType` variant names in discriminant order, as `BoneType.ToAttributeString` renders them.
const BONE_TYPE_NAMES: [&str; BONE_TYPE_LAST as usize] = [
    "Hips",
    "LeftUpperLeg",
    "RightUpperLeg",
    "LeftLowerLeg",
    "RightLowerLeg",
    "LeftFoot",
    "RightFoot",
    "Spine",
    "Chest",
    "UpperChest",
    "Neck",
    "Head",
    "LeftShoulder",
    "RightShoulder",
    "LeftUpperArm",
    "RightUpperArm",
    "LeftLowerArm",
    "RightLowerArm",
    "LeftHand",
    "RightHand",
    "LeftToes",
    "RightToes",
    "LeftEye",
    "RightEye",
    "Jaw",
];

/// `MuscleType.ToAttributeString` in discriminant order.
const MUSCLE_TYPE_NAMES: [&str; MUSCLE_TYPE_LAST as usize] = [
    "Spine Front-Back",
    "Spine Left-Right",
    "Spine Twist Left-Right",
    "Chest Front-Back",
    "Chest Left-Right",
    "Chest Twist Left-Right",
    "UpperChest Front-Back",
    "UpperChest Left-Right",
    "UpperChest Twist Left-Right",
    "Neck Nod Down-Up",
    "Neck Tilt Left-Right",
    "Neck Turn Left-Right",
    "Head Nod Down-Up",
    "Head Tilt Left-Right",
    "Head Turn Left-Right",
    "Left Eye Down-Up",
    "Left Eye In-Out",
    "Right Eye Down-Up",
    "Right Eye In-Out",
    "Jaw Close",
    "Jaw Left-Right",
    "Left Upper Leg Front-Back",
    "Left Upper Leg In-Out",
    "Left Upper Leg Twist In-Out",
    "Left Lower Leg Stretch",
    "Left Lower Leg Twist In-Out",
    "Left Foot Up-Down",
    "Left Foot Twist In-Out",
    "Left Toes Up-Down",
    "Right Upper Leg Front-Back",
    "Right Upper Leg In-Out",
    "Right Upper Leg Twist In-Out",
    "Right Lower Leg Stretch",
    "Right Lower Leg Twist In-Out",
    "Right Foot Up-Down",
    "Right Foot Twist In-Out",
    "Right Toes Up-Down",
    "Left Shoulder Down-Up",
    "Left Shoulder Front-Back",
    "Left Arm Down-Up",
    "Left Arm Front-Back",
    "Left Arm Twist In-Out",
    "Left Forearm Stretch",
    "Left Forearm Twist In-Out",
    "Left Hand Down-Up",
    "Left Hand In-Out",
    "Right Shoulder Down-Up",
    "Right Shoulder Front-Back",
    "Right Arm Down-Up",
    "Right Arm Front-Back",
    "Right Arm Twist In-Out",
    "Right Forearm Stretch",
    "Right Forearm Twist In-Out",
    "Right Hand Down-Up",
    "Right Hand In-Out",
];

/// `LimbType.ToBoneType` as `BoneType` discriminants.
const LIMB_TO_BONE: [i32; LIMB_TYPE_LAST as usize] = [5, 6, 18, 19];
/// `ArmType.ToBoneType` as `BoneType` discriminants.
const ARM_TO_BONE: [i32; ARM_TYPE_LAST as usize] = [18, 19];
/// `TDoFBoneType.ToBoneType` as `BoneType` discriminants.
const TDOF_TO_BONE: [i32; TDOF_BONE_TYPE_LAST as usize] = [
    7, 8, 9, 10, 11, 1, 3, 5, 20, 2, 4, 6, 21, 12, 14, 16, 18, 13, 15, 17, 19,
];
/// `FingerType` variant names in discriminant order.
const FINGER_TYPE_NAMES: [&str; FINGER_TYPE_LAST as usize] =
    ["Thumb", "Index", "Middle", "Ring", "Little"];
/// `FingerDoFType.ToAttributeString` in discriminant order.
const FINGER_DOF_NAMES: [&str; FINGER_DOF_TYPE_LAST as usize] =
    ["1 Stretched", "Spread", "2 Stretched", "3 Stretched"];

/// Upper chest exists on Unity 5.6 and later (`BoneTypeExtensions.IsIncludeUpperChest`).
fn includes_upper_chest(version: UnityVersion) -> bool {
    version.at_least_at(5, 6)
}

/// The 2017.3 head/limb TDoF gates (`TDoFBoneTypeExtensions.IsInclude*`).
fn includes_tdof_bone(version: UnityVersion) -> bool {
    version.at_least_at(2017, 3)
}

/// `MuscleType.Update`: pre-5.6 ids at or after `UpperchestFrontBack` shift by 3.
fn update_muscle(muscle: i32, version: UnityVersion) -> i32 {
    if !includes_upper_chest(version) && muscle >= 6 {
        muscle + 3
    } else {
        muscle
    }
}

/// `TDoFBoneType.Update`: the UpperChest (+1), Head (+1), and four limb (+3 each) shifts.
fn update_tdof_bone(tdof: i32, version: UnityVersion) -> i32 {
    let mut value = tdof;
    if !includes_upper_chest(version) && value >= 2 {
        value += 1;
    }
    if !includes_tdof_bone(version) {
        if value >= 4 {
            value += 1;
        }
        if value >= 6 {
            value += 3;
        }
        if value >= 10 {
            value += 3;
        }
        if value >= 14 {
            value += 3;
        }
        if value >= 18 {
            value += 3;
        }
    }
    value
}

/// `HumanoidMuscleType.Update`: shifts muscle and TDoF ids onto the version's layout.
/// Pure arithmetic like the source; ids below `Muscles` pass through untouched.
pub fn update_humanoid_muscle(muscle: i32, version: UnityVersion) -> i32 {
    let mut value = muscle;
    if value >= HUMANOID_MUSCLE_MUSCLES {
        value = HUMANOID_MUSCLE_MUSCLES + update_muscle(value - HUMANOID_MUSCLE_MUSCLES, version);
        if value >= HUMANOID_MUSCLE_TDOF_BONES {
            value = HUMANOID_MUSCLE_TDOF_BONES
                + update_tdof_bone(value - HUMANOID_MUSCLE_TDOF_BONES, version);
        }
    }
    value
}

/// The seven transform postfixes (`GetTransformPostfix`): `T.x`..`T.z`, `Q.x`..`Q.w`.
fn transform_postfix(index: i32) -> Option<&'static str> {
    match index {
        0 => Some("T.x"),
        1 => Some("T.y"),
        2 => Some("T.z"),
        3 => Some("Q.x"),
        4 => Some("Q.y"),
        5 => Some("Q.z"),
        6 => Some("Q.w"),
        _ => None,
    }
}

/// The three TDoF postfixes (`GetTDoFTransformPostfix`).
fn tdof_postfix(index: i32) -> Option<&'static str> {
    match index {
        0 => Some("TDOF.x"),
        1 => Some("TDOF.y"),
        2 => Some("TDOF.z"),
        _ => None,
    }
}

fn bone_name(bone: i32) -> Option<&'static str> {
    BONE_TYPE_NAMES.get(bone as usize).copied()
}

/// `HumanoidMuscleType.ToAttributeString`: the humanoid binding name for a muscle id.
///
/// Ids outside `[Motion, Last)` read `None` where the source throws `ArgumentException`.
pub fn humanoid_muscle_attribute(muscle: i32) -> Option<String> {
    if muscle < HUMANOID_MUSCLE_MOTION {
        return None;
    }
    if muscle < HUMANOID_MUSCLE_ROOT {
        let delta = muscle - HUMANOID_MUSCLE_MOTION;
        return transform_postfix(delta).map(|postfix| format!("Motion{postfix}"));
    }
    if muscle < HUMANOID_MUSCLE_LIMBS {
        let delta = muscle - HUMANOID_MUSCLE_ROOT;
        return transform_postfix(delta).map(|postfix| format!("Root{postfix}"));
    }
    if muscle < HUMANOID_MUSCLE_MUSCLES {
        let delta = muscle - HUMANOID_MUSCLE_LIMBS;
        let bone = bone_name(*LIMB_TO_BONE.get((delta / 7) as usize)?)?;
        return transform_postfix(delta % 7).map(|postfix| format!("{bone}{postfix}"));
    }
    if muscle < HUMANOID_MUSCLE_FINGERS {
        let delta = muscle - HUMANOID_MUSCLE_MUSCLES;
        let name = MUSCLE_TYPE_NAMES.get(delta as usize)?;
        return Some((*name).to_owned());
    }
    if muscle < HUMANOID_MUSCLE_TDOF_BONES {
        let delta = muscle - HUMANOID_MUSCLE_FINGERS;
        let arm_size = FINGER_TYPE_LAST * FINGER_DOF_TYPE_LAST;
        let bone = bone_name(*ARM_TO_BONE.get((delta / arm_size) as usize)?)?;
        let finger = FINGER_TYPE_NAMES.get((delta % arm_size / FINGER_DOF_TYPE_LAST) as usize)?;
        let dof = FINGER_DOF_NAMES.get((delta % FINGER_DOF_TYPE_LAST) as usize)?;
        return Some(format!("{bone}.{finger}.{dof}"));
    }
    if muscle < HUMANOID_MUSCLE_LAST {
        let delta = muscle - HUMANOID_MUSCLE_TDOF_BONES;
        let bone = bone_name(*TDOF_TO_BONE.get((delta / 3) as usize)?)?;
        return tdof_postfix(delta % 3).map(|postfix| format!("{bone}{postfix}"));
    }
    None
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

    /// `HumanoidMuscleType` boundaries mirror the source enum arithmetic.
    #[test]
    fn humanoid_muscle_boundaries_match_the_source_enum() {
        assert_eq!(HUMANOID_MUSCLE_MOTION, 0);
        assert_eq!(HUMANOID_MUSCLE_ROOT, 7);
        assert_eq!(HUMANOID_MUSCLE_LIMBS, 14);
        assert_eq!(HUMANOID_MUSCLE_MUSCLES, 42);
        assert_eq!(HUMANOID_MUSCLE_FINGERS, 97);
        assert_eq!(HUMANOID_MUSCLE_TDOF_BONES, 137);
        assert_eq!(HUMANOID_MUSCLE_LAST, 200);
    }

    /// Spot coverage of every `ToAttributeString` band plus the out-of-range edges.
    #[test]
    fn humanoid_muscle_attributes_name_every_band() {
        // Motion and Root bands.
        assert_eq!(humanoid_muscle_attribute(0).as_deref(), Some("MotionT.x"));
        assert_eq!(humanoid_muscle_attribute(6).as_deref(), Some("MotionQ.w"));
        assert_eq!(humanoid_muscle_attribute(7).as_deref(), Some("RootT.x"));
        assert_eq!(humanoid_muscle_attribute(13).as_deref(), Some("RootQ.w"));
        // Limb band: bone name plus transform postfix.
        assert_eq!(
            humanoid_muscle_attribute(14).as_deref(),
            Some("LeftFootT.x")
        );
        assert_eq!(
            humanoid_muscle_attribute(20).as_deref(),
            Some("LeftFootQ.w")
        );
        assert_eq!(
            humanoid_muscle_attribute(21).as_deref(),
            Some("RightFootT.x")
        );
        assert_eq!(
            humanoid_muscle_attribute(28).as_deref(),
            Some("LeftHandT.x")
        );
        assert_eq!(
            humanoid_muscle_attribute(41).as_deref(),
            Some("RightHandQ.w")
        );
        // Muscle band: the literal muscle names.
        assert_eq!(
            humanoid_muscle_attribute(42).as_deref(),
            Some("Spine Front-Back")
        );
        assert_eq!(humanoid_muscle_attribute(61).as_deref(), Some("Jaw Close"));
        assert_eq!(
            humanoid_muscle_attribute(96).as_deref(),
            Some("Right Hand In-Out")
        );
        // Finger band: arm bone, finger, and degree of freedom.
        assert_eq!(
            humanoid_muscle_attribute(97).as_deref(),
            Some("LeftHand.Thumb.1 Stretched")
        );
        assert_eq!(
            humanoid_muscle_attribute(116).as_deref(),
            Some("LeftHand.Little.3 Stretched")
        );
        assert_eq!(
            humanoid_muscle_attribute(136).as_deref(),
            Some("RightHand.Little.3 Stretched")
        );
        // TDoF band: bone plus translation postfix.
        assert_eq!(
            humanoid_muscle_attribute(137).as_deref(),
            Some("SpineTDOF.x")
        );
        assert_eq!(
            humanoid_muscle_attribute(199).as_deref(),
            Some("RightHandTDOF.z")
        );
        // Outside [Motion, Last) the source throws; the kernel reads None.
        assert_eq!(humanoid_muscle_attribute(-1), None);
        assert_eq!(humanoid_muscle_attribute(200), None);
    }

    /// The versioned `Update` shifts for pre-5.6 and pre-2017.3 layouts.
    #[test]
    fn humanoid_muscle_update_shifts_old_layouts() {
        let modern = UnityVersion::parse("2022.3.1f1").unwrap();
        let chest_only = UnityVersion::parse("5.6.0f1").unwrap();
        let legacy = UnityVersion::parse("5.0.0f1").unwrap();
        // Below Muscles everything passes through on every version.
        assert_eq!(update_humanoid_muscle(13, legacy), 13);
        assert_eq!(update_humanoid_muscle(41, legacy), 41);
        // UpperChest muscles shift by 3 before 5.6 only.
        assert_eq!(update_humanoid_muscle(48, legacy), 51);
        assert_eq!(update_humanoid_muscle(48, chest_only), 48);
        assert_eq!(update_humanoid_muscle(48, modern), 48);
        // A Head TDoF id walks every applicable gate on legacy layouts. Note the muscle
        // stage shifts first (99 -> 102 pre-5.6), so the TDoF stage starts from 7, not 4.
        assert_eq!(update_humanoid_muscle(141, modern), 141);
        assert_eq!(update_humanoid_muscle(141, chest_only), 142);
        assert_eq!(update_humanoid_muscle(141, legacy), 158);
    }

    /// Out-of-range ids update and then read `None`, where the source throws.
    #[test]
    fn out_of_range_muscles_update_then_read_none() {
        let modern = UnityVersion::parse("2022.3.1f1").unwrap();
        let legacy = UnityVersion::parse("5.0.0f1").unwrap();
        // An empty skeleton map resolves nothing.
        assert_eq!(find_bone_path(&HashMap::new(), 12345), None);
        // Past `Last` the update arithmetic still runs, but no band names the result.
        for muscle in [200, 1000] {
            assert!(update_humanoid_muscle(muscle, modern) >= HUMANOID_MUSCLE_LAST);
            assert_eq!(
                humanoid_muscle_attribute(update_humanoid_muscle(muscle, modern)),
                None
            );
            assert_eq!(
                humanoid_muscle_attribute(update_humanoid_muscle(muscle, legacy)),
                None
            );
        }
    }
}
