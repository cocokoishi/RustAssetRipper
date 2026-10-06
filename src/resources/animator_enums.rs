//! Animator enum casts and the synced-layer index.
//!
//! Mirrors `LayerConstantExtensions.GetLayerBlendingMode`,
//! `ValueConstantExtensions.GetTypeValue`, and
//! `AnimatorControllerParameterExtensions.GetTypeValue`. Each is a direct cast of a serialized
//! integer to its enum, so the Rust side models the raw discriminant. The generated enum member
//! names live in the published `AssetRipper.SourceGenerated` package rather than the frozen
//! source snapshot, so only the discriminant identity is asserted here.
//!
//! Also mirrors the `AnimatorControllerProcessor.InitializeLayer` synced-layer decision, which
//! needs no enum: unsynchronized layers read -1, synchronized ones their state-machine index.
//! The parameter-default dispatch in `InitializeParameter` stays open: it switches on the
//! external `AnimatorControllerParameterType` values, which the frozen baseline does not declare.
//!
//! Also mirrors the enum-free `AnimatorStateContext`/`AnimatorStateMachineContext` sentinel
//! and gate decisions: the `DefaultState` fallback, the transition-destination presence head
//! of `TryGetDestinationState`, and the Unity-5-only state-machine-destination branch. The
//! `IsUnity5` value itself is a generated presence fact
//! (`StateMachineConstant.Has_SelectorStateConstantArray`) and arrives caller-side, as do the
//! indexed state/state-machine searches, path surgery, and transition emission.

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

/// `AnimatorControllerProcessor.InitializeLayer` synced-layer index:
/// `StateMachineSynchronizedLayerIndex == 0` reads -1, otherwise the layer's
/// `StateMachineIndex` (wrapping like the source cast).
pub fn synced_layer_index(synchronized_layer_index: u32, state_machine_index: u32) -> i32 {
    if synchronized_layer_index == 0 {
        -1
    } else {
        state_machine_index as i32
    }
}

/// `AnimatorStateContext` constructor `DefaultStateIndex`:
/// `DefaultState == uint.MaxValue` reads 0, otherwise the default state
/// (wrapping like the source cast).
pub const fn default_state_index(default_state: u32) -> i32 {
    if default_state == u32::MAX {
        0
    } else {
        default_state as i32
    }
}

/// `AnimatorStateMachineContext.TryGetDestinationState` presence head: a
/// `DestinationState` of `uint.MaxValue` names no destination at all.
pub const fn has_transition_destination(destination_state: u32) -> bool {
    destination_state != u32::MAX
}

/// `AnimatorStateMachineContext.CreateAnimatorStateTransition` destination branch:
/// only Unity 5+ resolves a known destination index to a state machine.
pub const fn is_state_machine_destination(
    is_unity5: bool,
    state_machine_destination_index: i32,
) -> bool {
    is_unity5 && state_machine_destination_index != -1
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

    /// `InitializeLayer` synced-layer decision: -1 unless synchronized.
    #[test]
    fn synced_layer_index_matches_the_source_branch() {
        assert_eq!(synced_layer_index(0, 0), -1);
        assert_eq!(synced_layer_index(0, 7), -1);
        assert_eq!(synced_layer_index(1, 7), 7);
        assert_eq!(synced_layer_index(2, 0), 0);
        assert_eq!(synced_layer_index(3, u32::MAX), -1);
    }

    /// State-context sentinels and the Unity-5 destination gate.
    #[test]
    fn state_context_gates_match_the_source_branches() {
        // `uint.MaxValue` falls back to state zero, wrapping like the source cast.
        assert_eq!(default_state_index(u32::MAX), 0);
        assert_eq!(default_state_index(0), 0);
        assert_eq!(default_state_index(7), 7);
        assert_eq!(default_state_index(u32::MAX - 1), -2);
        // `uint.MaxValue` names no transition destination.
        assert!(!has_transition_destination(u32::MAX));
        assert!(has_transition_destination(0));
        assert!(has_transition_destination(30_000));
        // Only Unity 5+ with a known index lands on a state machine.
        assert!(!is_state_machine_destination(false, 3));
        assert!(!is_state_machine_destination(true, -1));
        assert!(!is_state_machine_destination(false, -1));
        assert!(is_state_machine_destination(true, 0));
        assert!(is_state_machine_destination(true, 3));
    }
}
