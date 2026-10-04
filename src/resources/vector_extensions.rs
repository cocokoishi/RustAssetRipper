//! `Vector3f`/`Quaternionf` field helpers.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/Vector3fExtensions.cs` and
//! `QuaternionfExtensions.cs`. In Rust the generated single-precision vector/quaternion structs are
//! [`Vec3`]/[`Quat`], so the copy/cast helpers are direct assignments.

use crate::numerics::{Quat, Vec3};

/// `Vector3fExtensions.Scale`.
pub fn scale(vector: &mut Vec3, scalar: f32) {
    vector.x *= scalar;
    vector.y *= scalar;
    vector.z *= scalar;
}

/// `Vector3fExtensions.CopyValues`.
pub fn copy_values(vector: &mut Vec3, source: Vec3) {
    *vector = source;
}

/// `Vector3fExtensions.SetZero`.
pub fn set_zero(vector: &mut Vec3) {
    *vector = Vec3 {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
}

/// `Vector3fExtensions.SetOne`.
pub fn set_one(vector: &mut Vec3) {
    *vector = Vec3 {
        x: 1.0,
        y: 1.0,
        z: 1.0,
    };
}

/// `Vector3fExtensions.CastToStruct`.
pub fn cast_to_struct(vector: Vec3) -> Vec3 {
    vector
}

/// `QuaternionfExtensions.CastToStruct`.
pub fn cast_to_struct_quaternion(value: Quat) -> Quat {
    value
}

/// `QuaternionfExtensions.CopyValues`.
pub fn copy_values_quaternion(value: &mut Quat, source: Quat) {
    *value = source;
}

/// `QuaternionfExtensions.SetIdentity`.
pub fn set_identity(value: &mut Quat) {
    *value = Quat {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    };
}

/// The `SetValues(x, y, z, w)` used when a quaternion weight is filled from a scalar.
pub fn set_quaternion_components(value: &mut Quat, x: f32, y: f32, z: f32, w: f32) {
    *value = Quat { x, y, z, w };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Byte-for-byte differential against the reference `Vector3fExtensions` (probe
    /// `Temp/opencode/v3probe`).
    #[test]
    fn vector3f_operations_match_the_reference_runtime() {
        let fixture = include_str!("keyframe_vector3_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if !line.starts_with("VOP|") {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            let expected: Vec<u32> = parts[2].split(',').map(|v| v.parse().unwrap()).collect();
            let base = Vec3 {
                x: 1.0,
                y: 2.0,
                z: 3.0,
            };
            let mut vector = base;
            match parts[1] {
                "scale" => scale(&mut vector, 2.0),
                "zero" => set_zero(&mut vector),
                "one" => set_one(&mut vector),
                "cast" => {
                    vector = cast_to_struct(Vec3 {
                        x: 4.0,
                        y: 5.0,
                        z: 6.0,
                    });
                }
                other => panic!("unknown vector op {other}"),
            }
            assert_eq!(
                [vector.x.to_bits(), vector.y.to_bits(), vector.z.to_bits()],
                expected[..],
                "{line}"
            );
            rows += 1;
        }
        assert_eq!(rows, 4);
    }
}
