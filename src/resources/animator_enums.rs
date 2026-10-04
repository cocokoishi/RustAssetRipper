//! Animator enum casts.
//!
//! Mirrors `LayerConstantExtensions.GetLayerBlendingMode`,
//! `ValueConstantExtensions.GetTypeValue`, and
//! `AnimatorControllerParameterExtensions.GetTypeValue`. Each is a direct cast of a serialized
//! integer to its enum, so the Rust side models the raw discriminant. The generated enum member
//! names live in the published `AssetRipper.SourceGenerated` package rather than the frozen
//! source snapshot, so only the discriminant identity is asserted here.

/// `AnimatorLayerBlendingMode` discriminant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AnimatorLayerBlendingMode(pub i32);

/// `AnimatorControllerParameterType` discriminant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AnimatorControllerParameterType(pub i32);

/// `LayerConstantExtensions.GetLayerBlendingMode`.
pub fn get_layer_blending_mode(raw: i32) -> AnimatorLayerBlendingMode {
    AnimatorLayerBlendingMode(raw)
}

/// `ValueConstantExtensions.GetTypeValue`.
pub fn get_value_constant_type(raw: u32) -> AnimatorControllerParameterType {
    AnimatorControllerParameterType(raw as i32)
}

/// `AnimatorControllerParameterExtensions.GetTypeValue`.
pub fn get_animator_controller_parameter_type(raw: i32) -> AnimatorControllerParameterType {
    AnimatorControllerParameterType(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Differential against the reference cast extensions (probe `Temp/opencode/anim2probe`).
    #[test]
    fn animator_enum_casts_match_the_reference_runtime() {
        let fixture = include_str!("animator_enums_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "AE", "{line}");
            let blending_mode = parts[2].parse::<i32>().unwrap();
            let value_type = parts[3].parse::<u32>().unwrap();
            let parameter_type = parts[4].parse::<i32>().unwrap();
            assert_eq!(
                get_layer_blending_mode(blending_mode).0,
                blending_mode,
                "{line}"
            );
            assert_eq!(
                get_value_constant_type(value_type).0,
                value_type as i32,
                "{line}"
            );
            assert_eq!(
                get_animator_controller_parameter_type(parameter_type).0,
                parameter_type,
                "{line}"
            );
            rows += 1;
        }
        assert_eq!(rows, 6);
    }
}
