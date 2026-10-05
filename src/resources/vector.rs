//! `Vector2fExtensions`, `Vector3fExtensions`, and `Vector4fExtensions`.
//!
//! Mirrors the scale/copy/zero/one/cast helpers for the serialized vector subtypes.
//!
//! Differential fixture: `vector_fixture.txt` (probe `Temp/opencode/vecprobe`).

/// A vector2 view.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vector2View {
    /// `X`.
    pub x: f32,
    /// `Y`.
    pub y: f32,
}

impl Vector2View {
    /// `Vector2fExtensions.Scale`.
    pub fn scale(&mut self, scalar: f32) {
        self.x *= scalar;
        self.y *= scalar;
    }

    /// `Vector2fExtensions.CopyValues`.
    pub fn copy_values(&mut self, x: f32, y: f32) {
        self.x = x;
        self.y = y;
    }

    /// `Vector2fExtensions.SetOne`.
    pub fn set_one(&mut self) {
        self.x = 1.0;
        self.y = 1.0;
    }

    /// `Vector2fExtensions.CastToStruct`.
    pub fn cast_to_struct(&self) -> [f32; 2] {
        [self.x, self.y]
    }
}

/// A vector3 view.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vector3View {
    /// `X`.
    pub x: f32,
    /// `Y`.
    pub y: f32,
    /// `Z`.
    pub z: f32,
}

impl Vector3View {
    /// `Vector3fExtensions.Scale`.
    pub fn scale(&mut self, scalar: f32) {
        self.x *= scalar;
        self.y *= scalar;
        self.z *= scalar;
    }

    /// `Vector3fExtensions.CopyValues`.
    pub fn copy_values(&mut self, x: f32, y: f32, z: f32) {
        self.x = x;
        self.y = y;
        self.z = z;
    }

    /// `Vector3fExtensions.SetZero`.
    pub fn set_zero(&mut self) {
        self.copy_values(0.0, 0.0, 0.0);
    }

    /// `Vector3fExtensions.SetOne`.
    pub fn set_one(&mut self) {
        self.copy_values(1.0, 1.0, 1.0);
    }

    /// `Vector3fExtensions.CastToStruct`.
    pub fn cast_to_struct(&self) -> [f32; 3] {
        [self.x, self.y, self.z]
    }
}

/// A vector4 view.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vector4View {
    /// `X`.
    pub x: f32,
    /// `Y`.
    pub y: f32,
    /// `Z`.
    pub z: f32,
    /// `W`.
    pub w: f32,
}

impl Vector4View {
    /// `Vector4fExtensions.Scale`.
    pub fn scale(&mut self, scalar: f32) {
        self.x *= scalar;
        self.y *= scalar;
        self.z *= scalar;
        self.w *= scalar;
    }

    /// `Vector4fExtensions.CopyValues`.
    pub fn copy_values(&mut self, x: f32, y: f32, z: f32, w: f32) {
        self.x = x;
        self.y = y;
        self.z = z;
        self.w = w;
    }

    /// `Vector4fExtensions.CastToStruct`.
    pub fn cast_to_struct(&self) -> [f32; 4] {
        [self.x, self.y, self.z, self.w]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(values: &[f32]) -> Vec<u32> {
        values.iter().map(|value| value.to_bits()).collect()
    }

    /// Byte-for-byte differential against the reference vector extensions (probe
    /// `Temp/opencode/vecprobe`).
    #[test]
    fn vector_operations_match_the_reference_runtime() {
        let fixture = include_str!("vector_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            let expected: Vec<u32> = parts[1..].iter().map(|v| v.parse().unwrap()).collect();
            let actual = match parts[0] {
                "V2" => {
                    let mut view = Vector2View::default();
                    view.set_one();
                    view.copy_values(5.0, 6.0);
                    bits(&view.cast_to_struct())
                }
                "V3" => {
                    let mut view = Vector3View::default();
                    view.set_zero();
                    view.set_one();
                    view.copy_values(7.0, 8.0, 9.0);
                    bits(&view.cast_to_struct())
                }
                "V4" => {
                    let mut view = Vector4View {
                        x: 1.0,
                        y: 2.0,
                        z: 3.0,
                        w: 4.0,
                    };
                    view.scale(2.0);
                    view.copy_values(5.0, 6.0, 7.0, 8.0);
                    bits(&view.cast_to_struct())
                }
                other => panic!("unknown vector kind {other}"),
            };
            assert_eq!(actual, expected, "{line}");
            rows += 1;
        }
        assert_eq!(rows, 4);
    }
}
