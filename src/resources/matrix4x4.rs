//! `Matrix4x4fExtensions`.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/Matrix4x4fExtensions.cs`. The serialized
//! `Matrix4x4f` and the runtime `Matrix4x4` have transposed memory layouts, so `CopyValues` stores
//! the transpose and `CastToStruct` reads it back.
//!
//! Also mirrors `MatrixParameterExtensions`: the shader matrix-parameter view pins the fixed
//! fields (`NameIndex`, `ArraySize`) and the caller-supplied type, offset, and row count. The
//! `name` argument is unused in the source (commented out) and `ColumnCount` does not exist on
//! the source subclass (its default is 4), so neither is modeled.
//!
//! Differential fixture: `matrix4x4_fixture.txt` (probe `Temp/opencode/miscprobe`).

/// `Matrix4x4fExtensions.CopyValues`: store the transpose of a row-major `Matrix4x4` into the
/// serialized row-major fields `E00..E33`.
pub fn copy_values(fields: &mut [f32; 16], matrix: &[f32; 16]) {
    for row in 0..4 {
        for column in 0..4 {
            fields[row * 4 + column] = matrix[column * 4 + row];
        }
    }
}

/// `Matrix4x4fExtensions.CastToStruct`: reconstruct the row-major `Matrix4x4` from the serialized
/// fields.
pub fn cast_to_struct(fields: &[f32; 16]) -> [f32; 16] {
    let mut matrix = [0.0f32; 16];
    for row in 0..4 {
        for column in 0..4 {
            matrix[row * 4 + column] = fields[column * 4 + row];
        }
    }
    matrix
}

/// A shader matrix-parameter view.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MatrixParameterView {
    /// `NameIndex`.
    pub name_index: i32,
    /// `OffsetInConstantBuffer`.
    pub offset_in_constant_buffer: i32,
    /// `ArraySize`.
    pub array_size: i32,
    /// `Type` as the raw `ShaderParamType` discriminant.
    pub param_type: i8,
    /// `RowCount`.
    pub row_count: i8,
}

impl MatrixParameterView {
    /// `MatrixParameterExtensions.SetValues`.
    ///
    /// The `name` argument is unused in the source (the assignment is commented out), so it
    /// is not taken; `ColumnCount` is likewise absent. The row count narrows to a byte
    /// exactly like the source cast.
    pub fn set_values(&mut self, param_type: i8, offset_in_constant_buffer: i32, row_count: i32) {
        self.name_index = -1;
        self.offset_in_constant_buffer = offset_in_constant_buffer;
        self.array_size = 0;
        self.param_type = param_type;
        self.row_count = row_count as i8;
    }

    /// `MatrixParameterExtensions.GetType_`: the raw `ShaderParamType` discriminant.
    pub fn get_type(&self) -> i8 {
        self.param_type
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Byte-for-byte differential against the reference `Matrix4x4fExtensions` (probe
    /// `Temp/opencode/miscprobe`).
    #[test]
    fn matrix4x4_transpose_matches_the_reference_runtime() {
        let fixture = include_str!("matrix4x4_fixture.txt");
        let matrix: [f32; 16] = core::array::from_fn(|index| (index + 1) as f32);
        let mut fields = [0.0f32; 16];
        copy_values(&mut fields, &matrix);
        let back = cast_to_struct(&fields);

        let mut raw_rows = 0;
        let mut round_rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "M", "{line}");
            let index: usize = parts[2].parse().unwrap();
            let expected: u32 = parts[3].parse().unwrap();
            match parts[1] {
                "raw" => {
                    assert_eq!(fields[index].to_bits(), expected, "{line}");
                    raw_rows += 1;
                }
                "round" => {
                    assert_eq!(back[index].to_bits(), expected, "{line}");
                    round_rows += 1;
                }
                other => panic!("unknown matrix phase {other}"),
            }
        }
        assert_eq!(raw_rows, 16);
        assert_eq!(round_rows, 16);
        assert_eq!(back, matrix);
    }

    #[test]
    fn matrix_parameter_set_values_matches_the_source_shape() {
        use crate::resources::vector::SHADER_PARAM_TYPE_FLOAT;
        let mut view = MatrixParameterView {
            name_index: 3,
            offset_in_constant_buffer: 7,
            array_size: 5,
            param_type: 2,
            row_count: 2,
        };
        view.set_values(SHADER_PARAM_TYPE_FLOAT, 16, 4);
        assert_eq!(
            view,
            MatrixParameterView {
                name_index: -1,
                offset_in_constant_buffer: 16,
                array_size: 0,
                param_type: SHADER_PARAM_TYPE_FLOAT,
                row_count: 4,
            }
        );
        assert_eq!(view.get_type(), SHADER_PARAM_TYPE_FLOAT);
        // The row count narrows to a byte exactly like the source cast.
        view.set_values(SHADER_PARAM_TYPE_FLOAT, 0, 260);
        assert_eq!(view.row_count, 4);
    }
}
