//! `Vector2fExtensions`, `Vector3fExtensions`, and `Vector4fExtensions`.
//!
//! Mirrors the scale/copy/zero/one/cast helpers for the serialized vector subtypes.
//!
//! Also mirrors `VectorParameterExtensions`: the constant-buffer slot fill and the parameter
//! type read. `ShaderParamType` discriminants are declared in the frozen baseline
//! (`Enums/Shader/ShaderParamType.cs`), so the values are pinned here, unlike the external
//! animator enums.
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

/// `ShaderParamType.Float`.
pub const SHADER_PARAM_TYPE_FLOAT: i8 = 0;
/// `ShaderParamType.Int`.
pub const SHADER_PARAM_TYPE_INT: i8 = 1;
/// `ShaderParamType.Bool`.
pub const SHADER_PARAM_TYPE_BOOL: i8 = 2;
/// `ShaderParamType.Half`.
pub const SHADER_PARAM_TYPE_HALF: i8 = 3;
/// `ShaderParamType.Short`.
pub const SHADER_PARAM_TYPE_SHORT: i8 = 4;
/// `ShaderParamType.UInt`.
pub const SHADER_PARAM_TYPE_UINT: i8 = 5;
/// `ShaderParamType.TypeCount`.
pub const SHADER_PARAM_TYPE_COUNT: i8 = 6;

/// A shader vector-parameter view.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VectorParameterView {
    /// `NameIndex`.
    pub name_index: i32,
    /// `OffsetInConstantBuffer`.
    pub offset_in_constant_buffer: i32,
    /// `ArraySize`.
    pub array_size: i32,
    /// `Type` as the raw `ShaderParamType` discriminant.
    pub param_type: i8,
    /// `Dim`.
    pub dim: i8,
}

impl VectorParameterView {
    /// `VectorParameterExtensions.SetValues`.
    ///
    /// The `name` argument is unused in the source (the assignment is commented out), so it
    /// is not taken. The `columns` value narrows to a byte exactly like the source cast.
    pub fn set_values(&mut self, param_type: i8, offset_in_constant_buffer: i32, columns: i32) {
        self.name_index = -1;
        self.offset_in_constant_buffer = offset_in_constant_buffer;
        self.array_size = 0;
        self.param_type = param_type;
        self.dim = columns as i8;
    }

    /// `VectorParameterExtensions.GetType_`: the raw `ShaderParamType` discriminant.
    pub fn get_type(&self) -> i8 {
        self.param_type
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
            if parts[0] == "Q" {
                // The quaternion row is covered by `resources::quaternion`.
                continue;
            }
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
        assert_eq!(rows, 3);
    }

    /// `VectorParameterExtensions` slot fill and type read, with the baseline enum values.
    #[test]
    fn vector_parameter_set_values_matches_the_source_assignments() {
        assert_eq!(SHADER_PARAM_TYPE_FLOAT, 0);
        assert_eq!(SHADER_PARAM_TYPE_INT, 1);
        assert_eq!(SHADER_PARAM_TYPE_BOOL, 2);
        assert_eq!(SHADER_PARAM_TYPE_HALF, 3);
        assert_eq!(SHADER_PARAM_TYPE_SHORT, 4);
        assert_eq!(SHADER_PARAM_TYPE_UINT, 5);
        assert_eq!(SHADER_PARAM_TYPE_COUNT, 6);

        let mut view = VectorParameterView::default();
        view.set_values(SHADER_PARAM_TYPE_FLOAT, 16, 4);
        assert_eq!(view.name_index, -1);
        assert_eq!(view.offset_in_constant_buffer, 16);
        assert_eq!(view.array_size, 0);
        assert_eq!(view.get_type(), SHADER_PARAM_TYPE_FLOAT);
        // Refilling overwrites every field, and wide columns narrow like the source cast.
        view.set_values(SHADER_PARAM_TYPE_UINT, 0, 300);
        assert_eq!(view.offset_in_constant_buffer, 0);
        assert_eq!(view.dim, 44);
        assert_eq!(view.get_type(), SHADER_PARAM_TYPE_UINT);
    }
}
