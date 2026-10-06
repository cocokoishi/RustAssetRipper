//! `ControllerConstantExtensions` plus the `Animator` update-mode read.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/ControllerConstantExtensions.cs`. Layers are
//! addressed by their state-machine index; synchronized layers share an index and are skipped by
//! the state-machine lookups.
//!
//! Also mirrors `AnimatorExtensions.GetUpdateMode`: the stored `UpdateMode` when the version
//! serializes it, else `Normal`. The `AnimatorUpdateMode` discriminant is the stable UnityEditor
//! API value; like the project-settings enums it is referenced from an external package rather
//! than declared in the frozen baseline, so it is recorded here as an external-API dependency.

/// A layer-constant view.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LayerConstantView {
    /// `StateMachineIndex`.
    pub state_machine_index: u32,
    /// `StateMachineSynchronizedLayerIndex`.
    pub state_machine_synchronized_layer_index: u32,
}

/// A controller-constant view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ControllerConstantView {
    /// `LayerArray`.
    pub layers: Vec<LayerConstantView>,
}

/// A failure while looking up a controller layer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ControllerConstantError {
    /// No layer had the requested state-machine index.
    LayerByStateMachineIndexNotFound {
        /// The requested state-machine index.
        index: u32,
    },
    /// The layer was not present in the controller.
    LayerNotPresent,
}

impl core::fmt::Display for ControllerConstantError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::LayerByStateMachineIndexNotFound { index } => {
                write!(formatter, "no layer has state-machine index {index}")
            }
            Self::LayerNotPresent => {
                write!(formatter, "the layer is not present in the controller")
            }
        }
    }
}

impl std::error::Error for ControllerConstantError {}

impl ControllerConstantView {
    /// `ControllerConstantExtensions.GetLayerByStateMachineIndex`.
    pub fn get_layer_by_state_machine_index(
        &self,
        index: u32,
    ) -> Result<LayerConstantView, ControllerConstantError> {
        self.layers
            .iter()
            .find(|layer| {
                layer.state_machine_index == index
                    && layer.state_machine_synchronized_layer_index == 0
            })
            .copied()
            .ok_or(ControllerConstantError::LayerByStateMachineIndexNotFound { index })
    }

    /// `ControllerConstantExtensions.GetLayerIndexByStateMachineIndex`.
    pub fn get_layer_index_by_state_machine_index(
        &self,
        index: u32,
    ) -> Result<(usize, LayerConstantView), ControllerConstantError> {
        self.layers
            .iter()
            .enumerate()
            .find(|(_, layer)| {
                layer.state_machine_index == index
                    && layer.state_machine_synchronized_layer_index == 0
            })
            .map(|(position, layer)| (position, *layer))
            .ok_or(ControllerConstantError::LayerByStateMachineIndexNotFound { index })
    }

    /// `ControllerConstantExtensions.GetLayerIndex`.
    pub fn get_layer_index(
        &self,
        layer: &LayerConstantView,
    ) -> Result<usize, ControllerConstantError> {
        self.layers
            .iter()
            .position(|candidate| {
                candidate.state_machine_index == layer.state_machine_index
                    && candidate.state_machine_synchronized_layer_index
                        == layer.state_machine_synchronized_layer_index
            })
            .ok_or(ControllerConstantError::LayerNotPresent)
    }
}

/// `AnimatorUpdateMode.Normal`, the fallback when `UpdateMode` is absent.
pub const ANIMATOR_UPDATE_MODE_NORMAL: i32 = 0;

/// `AnimatorExtensions.GetUpdateMode`: the stored `AnimatorUpdateMode` discriminant when
/// `has_update_mode` answers `Has_UpdateMode`, else `Normal`.
pub fn animator_update_mode(has_update_mode: bool, update_mode: i32) -> i32 {
    if has_update_mode {
        update_mode
    } else {
        ANIMATOR_UPDATE_MODE_NORMAL
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn controller() -> ControllerConstantView {
        ControllerConstantView {
            layers: vec![
                LayerConstantView {
                    state_machine_index: 0,
                    state_machine_synchronized_layer_index: 0,
                },
                LayerConstantView {
                    state_machine_index: 1,
                    state_machine_synchronized_layer_index: 0,
                },
                LayerConstantView {
                    state_machine_index: 1,
                    state_machine_synchronized_layer_index: 2,
                },
                LayerConstantView {
                    state_machine_index: 2,
                    state_machine_synchronized_layer_index: 0,
                },
            ],
        }
    }

    /// Byte-for-byte differential against the reference `ControllerConstantExtensions` (probe
    /// `Temp/opencode/ccprobe`).
    #[test]
    fn controller_constant_layers_match_the_reference_runtime() {
        let fixture = include_str!("controller_constant_fixture.txt");
        let controller = controller();
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "CC", "{line}");

            let check_layer =
                |cell: &str, result: Result<LayerConstantView, ControllerConstantError>| {
                    if cell == "ERR" {
                        assert!(result.is_err(), "{line}");
                    } else {
                        let tokens: Vec<&str> = cell.split(',').collect();
                        let layer = result.unwrap();
                        assert_eq!(
                            layer.state_machine_index,
                            tokens[0].parse::<u32>().unwrap(),
                            "{line}"
                        );
                        assert_eq!(
                            layer.state_machine_synchronized_layer_index,
                            tokens[1].parse::<u32>().unwrap(),
                            "{line}"
                        );
                    }
                };

            check_layer(parts[2], controller.get_layer_by_state_machine_index(0));
            check_layer(parts[3], controller.get_layer_by_state_machine_index(1));
            check_layer(parts[4], controller.get_layer_by_state_machine_index(3));

            let index2 = controller.get_layer_index_by_state_machine_index(2);
            if parts[5] == "ERR" {
                assert!(index2.is_err(), "{line}");
            } else {
                let (position, layer) = index2.unwrap();
                assert_eq!(
                    position as i32 + layer.state_machine_index as i32 * 100,
                    parts[5].parse::<i32>().unwrap(),
                    "{line}"
                );
            }

            let layer1 = LayerConstantView {
                state_machine_index: 1,
                state_machine_synchronized_layer_index: 0,
            };
            let layer2 = LayerConstantView {
                state_machine_index: 1,
                state_machine_synchronized_layer_index: 2,
            };
            let unknown = LayerConstantView {
                state_machine_index: 9,
                state_machine_synchronized_layer_index: 0,
            };
            assert_eq!(
                controller
                    .get_layer_index(&layer1)
                    .ok()
                    .map(|index| index as i32),
                parts[6].parse::<i32>().ok(),
                "{line}"
            );
            assert_eq!(
                controller
                    .get_layer_index(&layer2)
                    .ok()
                    .map(|index| index as i32),
                parts[7].parse::<i32>().ok(),
                "{line}"
            );
            assert_eq!(
                controller.get_layer_index(&unknown).is_err(),
                parts[8] == "ERR",
                "{line}"
            );
            rows += 1;
        }
        assert_eq!(rows, 2);
    }

    #[test]
    fn animator_update_mode_defaults_to_normal_without_the_field() {
        assert_eq!(ANIMATOR_UPDATE_MODE_NORMAL, 0);
        assert_eq!(animator_update_mode(false, 2), 0);
        assert_eq!(animator_update_mode(false, 0), 0);
        // A present field passes through untouched, including non-default modes.
        assert_eq!(animator_update_mode(true, 0), 0);
        assert_eq!(animator_update_mode(true, 1), 1);
        assert_eq!(animator_update_mode(true, 2), 2);
    }
}
