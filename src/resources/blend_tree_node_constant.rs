//! `BlendTreeNodeConstantExtensions`.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/BlendTreeNodeConstantExtensions.cs`. Blend
//! nodes carry their threshold/position/parameter children in one of several version-dependent
//! sub-structures, and the accessors pick the right one (or fall back) based on the blend type.

/// `BlendTreeType_1.Simple1D`.
pub const BLEND_TREE_TYPE_SIMPLE_1D: i32 = 0;
/// `BlendTreeType_1.Direct`.
pub const BLEND_TREE_TYPE_DIRECT: i32 = 4;

/// A blend-tree-node-constant view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlendTreeNodeConstantView {
    /// `BlendType` as the raw `BlendTreeType_1` discriminant.
    pub blend_type: i32,
    /// `Blend1dData.Data.ChildThresholdArray`: present only on versions that serialize it.
    pub blend_1d_thresholds: Option<Vec<f32>>,
    /// `ChildThresholdArray`: present only on old versions.
    pub child_thresholds: Option<Vec<f32>>,
    /// `Blend2dData.Data.ChildPositionArray`: present only on versions that serialize it.
    pub child_positions: Option<Vec<(f32, f32)>>,
    /// `BlendDirectData.Data.ChildBlendEventIDArray`: present only on versions that serialize it.
    pub direct_blend_event_ids: Option<Vec<u32>>,
    /// `ChildIndices`.
    pub child_indices: Vec<i32>,
}

impl BlendTreeNodeConstantView {
    /// `BlendTreeNodeConstantExtensions.GetThreshold`.
    pub fn get_threshold(&self, index: usize) -> f32 {
        if let Some(thresholds) = &self.blend_1d_thresholds {
            if self.blend_type == BLEND_TREE_TYPE_SIMPLE_1D {
                return thresholds[index];
            }
        } else if let Some(thresholds) = &self.child_thresholds {
            return thresholds[index];
        }
        0.0
    }

    /// `BlendTreeNodeConstantExtensions.GetPosition`.
    pub fn get_position(&self, index: usize) -> (f32, f32) {
        if let Some(positions) = &self.child_positions {
            if self.blend_type != BLEND_TREE_TYPE_SIMPLE_1D
                && self.blend_type != BLEND_TREE_TYPE_DIRECT
            {
                return positions[index];
            }
        }
        (0.0, 0.0)
    }

    /// `BlendTreeNodeConstantExtensions.GetMinThreshold`.
    ///
    /// The reference throws on an empty threshold array; this returns `f32::INFINITY` instead, as
    /// documenting the malformed input is preferable to an unchecked panic.
    pub fn get_min_threshold(&self) -> f32 {
        if let Some(thresholds) = &self.blend_1d_thresholds {
            if self.blend_type == BLEND_TREE_TYPE_SIMPLE_1D {
                return thresholds.iter().copied().fold(f32::INFINITY, f32::min);
            }
        }
        0.0
    }

    /// `BlendTreeNodeConstantExtensions.GetMaxThreshold`.
    ///
    /// The reference throws on an empty threshold array; this returns `f32::NEG_INFINITY` instead,
    /// as documenting the malformed input is preferable to an unchecked panic.
    pub fn get_max_threshold(&self) -> f32 {
        if let Some(thresholds) = &self.blend_1d_thresholds {
            if self.blend_type == BLEND_TREE_TYPE_SIMPLE_1D {
                return thresholds.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            }
        }
        1.0
    }

    /// `BlendTreeNodeConstantExtensions.TryGetDirectBlendParameter`.
    pub fn try_get_direct_blend_parameter(&self, index: usize) -> Option<u32> {
        if let Some(parameters) = &self.direct_blend_event_ids {
            if self.blend_type == BLEND_TREE_TYPE_DIRECT {
                return Some(parameters[index]);
            }
        }
        None
    }

    /// `BlendTreeNodeConstantExtensions.IsBlendTree`.
    pub fn is_blend_tree(&self) -> bool {
        !self.child_indices.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(value: f32) -> u32 {
        value.to_bits()
    }

    /// Byte-for-byte differential against the reference `BlendTreeNodeConstantExtensions` (probe
    /// `Temp/opencode/bt2probe`).
    #[test]
    fn blend_tree_node_matches_the_reference_runtime() {
        let fixture = include_str!("blend_tree_node_constant_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "BTN", "{line}");
            let present = |cell: &str| cell == "1";

            let mut view = BlendTreeNodeConstantView {
                blend_type: BLEND_TREE_TYPE_SIMPLE_1D,
                blend_1d_thresholds: present(parts[2]).then_some(vec![0.25, 0.75]),
                child_positions: present(parts[3]).then_some(vec![(1.5, 2.5), (3.5, 4.5)]),
                direct_blend_event_ids: present(parts[4]).then_some(vec![11, 22]),
                child_thresholds: present(parts[5]).then_some(vec![0.1, 0.9]),
                child_indices: Vec::new(),
            };

            assert_eq!(
                bits(view.get_threshold(0)),
                parts[6].parse::<u32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                bits(view.get_threshold(1)),
                parts[7].parse::<u32>().unwrap(),
                "{line}"
            );
            view.blend_type = 3;
            assert_eq!(
                bits(view.get_threshold(0)),
                parts[8].parse::<u32>().unwrap(),
                "{line}"
            );
            view.blend_type = BLEND_TREE_TYPE_SIMPLE_1D;
            assert_eq!(
                bits(view.get_min_threshold()),
                parts[9].parse::<u32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                bits(view.get_max_threshold()),
                parts[10].parse::<u32>().unwrap(),
                "{line}"
            );

            view.blend_type = 3;
            let (x, y) = view.get_position(0);
            assert_eq!(
                (bits(x), bits(y)),
                (
                    parts[11].parse::<u32>().unwrap(),
                    parts[12].parse::<u32>().unwrap()
                ),
                "{line}"
            );
            view.blend_type = BLEND_TREE_TYPE_SIMPLE_1D;
            let (x, y) = view.get_position(0);
            assert_eq!(
                (bits(x), bits(y)),
                (
                    parts[13].parse::<u32>().unwrap(),
                    parts[14].parse::<u32>().unwrap()
                ),
                "{line}"
            );

            view.blend_type = BLEND_TREE_TYPE_DIRECT;
            assert_eq!(
                view.try_get_direct_blend_parameter(0),
                (parts[15] == "1").then_some(parts[16].parse::<u32>().unwrap()),
                "{line}"
            );
            view.blend_type = BLEND_TREE_TYPE_SIMPLE_1D;
            assert_eq!(
                view.try_get_direct_blend_parameter(0),
                (parts[17] == "1").then_some(parts[18].parse::<u32>().unwrap()),
                "{line}"
            );

            view.blend_type = BLEND_TREE_TYPE_SIMPLE_1D;
            assert_eq!(view.is_blend_tree(), parts[19] == "1", "{line}");
            view.child_indices.push(5);
            assert_eq!(view.is_blend_tree(), parts[20] == "1", "{line}");
            rows += 1;
        }
        assert_eq!(rows, 7);
    }
}
