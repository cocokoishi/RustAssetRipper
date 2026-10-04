//! `TangentMode` mapping between the serialized and per-version forms.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions.Enums.Keyframe.TangentMode`. The "2" and "5"
//! forms pack the low/high tangent flags into different bit positions, so the conversion shifts the
//! high constant field between bit 2 (pre-5.5) and bit 4 (5.5+).

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

/// `TangentModeExtensions.ToTangentMode5`.
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
}
