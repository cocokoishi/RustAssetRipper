//! `Matrix4x4fExtensions`.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/Matrix4x4fExtensions.cs`. The serialized
//! `Matrix4x4f` and the runtime `Matrix4x4` have transposed memory layouts, so `CopyValues` stores
//! the transpose and `CastToStruct` reads it back.
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
}
