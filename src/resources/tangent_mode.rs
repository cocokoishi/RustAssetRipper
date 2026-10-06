//! `TangentMode` mapping between the serialized and per-version forms.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions.Enums.Keyframe.TangentMode`. The "2" and "5"
//! forms pack the low/high tangent flags into different bit positions, so the conversion shifts the
//! high constant field between bit 2 (pre-5.5) and bit 4 (5.5+). The named low/high parts and the
//! broken-bit mixes are pinned as constants; the low parts equal the base members.

use super::vertex_data_blob::UnityVersion;

/// `TangentMode.Broken`.
pub const BROKEN: i32 = 1;
/// `TangentMode.Auto`.
pub const AUTO: i32 = 2;
/// `TangentMode.Linear`.
pub const LINEAR: i32 = 4;
/// `TangentMode.Constant`.
pub const CONSTANT: i32 = 6;
/// `TangentMode.ClampedAuto`.
pub const CLAMPED_AUTO: i32 = 8;
/// `TangentMode.FreeSmooth` (`LFree | HFree`).
pub const FREE_SMOOTH: i32 = 0;
/// `TangentMode.FreeFree` (`Broken | LFree | HFree`).
pub const FREE_FREE: i32 = 1;
/// `TangentMode.HAuto` (`Auto << 4`).
pub const H_AUTO: i32 = 32;
/// `TangentMode.HLinear` (`Linear << 4`).
pub const H_LINEAR: i32 = 64;
/// `TangentMode.HConstant` (`Constant << 4`).
pub const H_CONSTANT: i32 = 96;
/// `TangentMode.HClampedAuto` (`ClampedAuto << 4`).
pub const H_CLAMPED_AUTO: i32 = 128;
/// `TangentMode.ClampedAutoBoth` (`LClampedAuto | HClampedAuto`).
pub const CLAMPED_AUTO_BOTH: i32 = 136;
/// `TangentMode.AutoBoth` (`LAuto | HAuto`).
pub const AUTO_BOTH: i32 = 34;
/// `TangentMode.FreeLinear` (`Broken | LFree | HLinear`).
pub const FREE_LINEAR: i32 = 65;
/// `TangentMode.FreeConstant` (`Broken | LFree | HConstant`).
pub const FREE_CONSTANT: i32 = 97;
/// `TangentMode.LinearFree` (`Broken | LLinear | HFree`).
pub const LINEAR_FREE: i32 = 5;
/// `TangentMode.LinearLinear` (`Broken | LLinear | HLinear`).
pub const LINEAR_LINEAR: i32 = 69;
/// `TangentMode.LinearConstant` (`Broken | LLinear | HConstant`).
pub const LINEAR_CONSTANT: i32 = 101;
/// `TangentMode.ConstantFree` (`Broken | LConstant | HFree`).
pub const CONSTANT_FREE: i32 = 7;
/// `TangentMode.ConstantLinear` (`Broken | LConstant | HLinear`).
pub const CONSTANT_LINEAR: i32 = 71;
/// `TangentMode.ConstantConstant` (`Broken | LConstant | HConstant`).
pub const CONSTANT_CONSTANT: i32 = 103;

/// `TangentModeExtensions.TangentMode5Relevant`: 5.5.0 and greater.
pub fn tangent_mode_5_relevant(version: UnityVersion) -> bool {
    version.at_least_at(5, 5)
}

/// `TangentModeExtensions.ToTangent`.
pub fn to_tangent(value: i32, version: UnityVersion) -> i32 {
    if tangent_mode_5_relevant(version) {
        to_tangent_mode5(value)
    } else {
        to_tangent_mode2(value)
    }
}

/// `TangentModeExtensions.ToTangentMode2`.
pub fn to_tangent_mode2(value: i32) -> i32 {
    let mask = CONSTANT << 4;
    let mask2 = CONSTANT << 2;
    (value & !mask2) | ((value & mask) >> 2)
}

/// `TangentModeExtensions.ToTangentMode5`, which is also the whole body of
/// `TangentMode5Extensions.ToTangentMode` (both are identity casts).
pub fn to_tangent_mode5(value: i32) -> i32 {
    value
}

/// `TangentMode2Extensions.ToTangentMode` (the inverse of [`to_tangent_mode2`]).
pub fn tangent_mode2_to_tangent(value: i32) -> i32 {
    let mask = CONSTANT << 2;
    (value & !mask) | ((value & mask) << 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Byte-for-byte differential against the reference `TangentModeExtensions` (probe
    /// `Temp/opencode/animprobe`) for every value in `0..256`.
    #[test]
    fn tangent_mode_matches_the_reference_runtime() {
        let fixture = include_str!("tangent_mode_fixture.txt");
        let v50 = UnityVersion::parse("5.0.0f1").unwrap();
        let v54 = UnityVersion::parse("5.4.0f1").unwrap();
        let v55 = UnityVersion::parse("5.5.0f1").unwrap();
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "T", "{line}");
            let value: i32 = parts[1].parse().unwrap();
            assert_eq!(
                to_tangent_mode2(value),
                parts[2].parse::<i32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                to_tangent_mode5(value),
                parts[3].parse::<i32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                to_tangent(value, v50),
                parts[4].parse::<i32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                to_tangent(value, v54),
                parts[5].parse::<i32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                to_tangent(value, v55),
                parts[6].parse::<i32>().unwrap(),
                "{line}"
            );
            rows += 1;
        }
        assert_eq!(rows, 256);
    }

    #[test]
    fn tangent_mode2_round_trips_through_to_tangent_mode() {
        for value in 0..256 {
            let mode2 = to_tangent_mode2(value);
            assert_eq!(tangent_mode2_to_tangent(mode2), value & !0x18);
        }
    }

    #[test]
    fn tangent_mode_mixes_match_the_source_bit_layout() {
        // High parts shift the base members by four.
        assert_eq!(H_AUTO, AUTO << 4);
        assert_eq!(H_LINEAR, LINEAR << 4);
        assert_eq!(H_CONSTANT, CONSTANT << 4);
        assert_eq!(H_CLAMPED_AUTO, CLAMPED_AUTO << 4);
        // Mixes set the broken bit with the low/high parts.
        assert_eq!(CLAMPED_AUTO_BOTH, CLAMPED_AUTO | H_CLAMPED_AUTO);
        assert_eq!(AUTO_BOTH, AUTO | H_AUTO);
        assert_eq!(FREE_LINEAR, BROKEN | H_LINEAR);
        assert_eq!(FREE_CONSTANT, BROKEN | H_CONSTANT);
        assert_eq!(LINEAR_FREE, BROKEN | LINEAR);
        assert_eq!(LINEAR_LINEAR, BROKEN | LINEAR | H_LINEAR);
        assert_eq!(LINEAR_CONSTANT, BROKEN | LINEAR | H_CONSTANT);
        assert_eq!(CONSTANT_FREE, BROKEN | CONSTANT);
        assert_eq!(CONSTANT_LINEAR, BROKEN | CONSTANT | H_LINEAR);
        assert_eq!(CONSTANT_CONSTANT, BROKEN | CONSTANT | H_CONSTANT);
        // Spot literals pin the snapshot arithmetic.
        assert_eq!(
            (
                H_AUTO,
                H_LINEAR,
                H_CONSTANT,
                H_CLAMPED_AUTO,
                CLAMPED_AUTO_BOTH,
                AUTO_BOTH
            ),
            (32, 64, 96, 128, 136, 34)
        );
        assert_eq!(
            (
                FREE_LINEAR,
                FREE_CONSTANT,
                LINEAR_FREE,
                LINEAR_LINEAR,
                LINEAR_CONSTANT
            ),
            (65, 97, 5, 69, 101)
        );
        assert_eq!(
            (CONSTANT_FREE, CONSTANT_LINEAR, CONSTANT_CONSTANT),
            (7, 71, 103)
        );
    }
}
