//! `QuaternionfExtensions`.
//!
//! Mirrors the cast/copy/identity helpers for the serialized quaternion subtype.
//!
//! Differential fixture: `vector_fixture.txt` (probe `Temp/opencode/vecprobe`).

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
}
