//! `QuaternionfExtensions` plus the editor euler-hint computation.
//!
//! Mirrors the cast/copy/identity helpers for the serialized quaternion subtype.
//!
//! Also mirrors the `EditorFormatConverterAsync` euler-hint assignment: `LocalEulerAnglesHint`
//! is recomputed from `LocalRotation` through the degrees `ToEulerAngle` conversion. It lives
//! here (rather than with the transform views) because only this pure rotation math ports —
//! field presence and transform assembly stay caller-side.
//!
//! Differential fixture: `vector_fixture.txt` (probe `Temp/opencode/vecprobe`).

use crate::numerics::Quat;

/// A quaternion view.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct QuaternionView {
    /// `X`.
    pub x: f32,
    /// `Y`.
    pub y: f32,
    /// `Z`.
    pub z: f32,
    /// `W`.
    pub w: f32,
}

impl QuaternionView {
    /// `QuaternionfExtensions.CopyValues`.
    pub fn copy_values(&mut self, x: f32, y: f32, z: f32, w: f32) {
        self.x = x;
        self.y = y;
        self.z = z;
        self.w = w;
    }

    /// `QuaternionfExtensions.SetIdentity`.
    pub fn set_identity(&mut self) {
        self.copy_values(0.0, 0.0, 0.0, 1.0);
    }

    /// `QuaternionfExtensions.CastToStruct`.
    pub fn cast_to_struct(&self) -> [f32; 4] {
        [self.x, self.y, self.z, self.w]
    }
}

/// `EditorFormatConverterAsync` euler-hint computation: `LocalEulerAnglesHint` from a
/// `LocalRotation` (x, y, z, w) via the degrees `ToEulerAngle` conversion.
///
/// Total like the source: no input validation, and callers decide field presence.
pub fn euler_hint_from_rotation(rotation: [f32; 4]) -> [f32; 3] {
    let quaternion = Quat {
        x: rotation[0],
        y: rotation[1],
        z: rotation[2],
        w: rotation[3],
    };
    let euler = quaternion.to_euler_angle(true);
    [euler.x, euler.y, euler.z]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Byte-for-byte differential against the reference `QuaternionfExtensions` (probe
    /// `Temp/opencode/vecprobe`).
    #[test]
    fn quaternion_operations_match_the_reference_runtime() {
        let fixture = include_str!("vector_fixture.txt");
        let mut view = QuaternionView {
            x: 1.0,
            y: 2.0,
            z: 3.0,
            w: 4.0,
        };
        view.set_identity();
        assert_eq!(view.cast_to_struct(), [0.0, 0.0, 0.0, 1.0]);
        view.copy_values(0.1, 0.2, 0.3, 0.4);
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            if parts[0] != "Q" {
                continue;
            }
            let expected: Vec<u32> = parts[1..].iter().map(|v| v.parse().unwrap()).collect();
            let actual: Vec<u32> = view
                .cast_to_struct()
                .iter()
                .map(|value| value.to_bits())
                .collect();
            assert_eq!(actual, expected, "{line}");
            rows += 1;
        }
        assert_eq!(rows, 1);
    }

    /// The euler-hint conversion recovers degree angles through the forward path.
    #[test]
    fn euler_hint_from_rotation_matches_axis_rotations() {
        use crate::numerics::Vec3;

        fn hint_for(angles: [f32; 3]) -> [f32; 3] {
            let euler = Vec3 {
                x: angles[0],
                y: angles[1],
                z: angles[2],
            };
            let quaternion = euler.to_quaternion(true);
            euler_hint_from_rotation([quaternion.x, quaternion.y, quaternion.z, quaternion.w])
        }

        fn close(actual: [f32; 3], expected: [f32; 3]) {
            for (lane, want) in actual.iter().zip(expected.iter()) {
                assert!(
                    (lane - want).abs() < 1e-3,
                    "got {actual:?}, wanted {expected:?}"
                );
            }
        }

        // Identity stays at the origin; axis and compound angles round-trip like the
        // numerics reference vectors.
        close(hint_for([0.0, 0.0, 0.0]), [0.0, 0.0, 0.0]);
        close(hint_for([0.0, 90.0, 0.0]), [0.0, 90.0, 0.0]);
        close(hint_for([90.0, 0.0, 0.0]), [90.0, 0.0, 0.0]);
        close(hint_for([0.0, 45.0, 0.0]), [0.0, 45.0, 0.0]);
        close(hint_for([-67.0, 45.0, -162.0]), [-67.0, 45.0, -162.0]);
    }
}
