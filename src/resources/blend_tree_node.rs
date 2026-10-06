//! `BlendTreeNodeConstantExtensions` plus the `BlendTree` blend-type read.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/BlendTreeNodeConstantExtensions.cs` and
//! `BlendTreeExtensions.cs`. Blend trees store 1D thresholds, 2D positions, and direct-blend
//! parameter ids in different sub-objects depending on the Unity version and the blend type,
//! so the accessors dispatch on both. The `BlendTree` blend type itself is stored as `Int32`
//! on newer versions and `UInt32` on older ones; that representation choice arrives as a
//! caller fact (the generated `Has_BlendType_Int32` gate stays caller-side).

use crate::numerics::Vec2;

/// `BlendTreeType_1`.
pub const BLEND_TREE_SIMPLE_1D: i32 = 0;
/// `BlendTreeType_1.SimpleDirectional2D`.
pub const BLEND_TREE_SIMPLE_DIRECTIONAL_2D: i32 = 1;
/// `BlendTreeType_1.FreeformDirectional2D`.
pub const BLEND_TREE_FREEFORM_DIRECTIONAL_2D: i32 = 2;
/// `BlendTreeType_1.FreeformCartesian2D`.
pub const BLEND_TREE_FREEFORM_CARTESIAN_2D: i32 = 3;
/// `BlendTreeType_1.Direct`.
pub const BLEND_TREE_DIRECT: i32 = 4;

/// A blend-tree node view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BlendTreeNodeView {
    /// `BlendType`.
    pub blend_type: i32,
    /// `Blend1dData.Data.ChildThresholdArray` when the version serializes it.
    pub blend1d_thresholds: Option<Vec<f32>>,
    /// `ChildThresholdArray` for versions without `Blend1dData`.
    pub child_thresholds: Option<Vec<f32>>,
    /// `Blend2dData.Data.ChildPositionArray`.
    pub blend2d_positions: Option<Vec<Vec2>>,
    /// `BlendDirectData.Data.ChildBlendEventIDArray`.
    pub blend_direct_ids: Option<Vec<u32>>,
    /// `ChildIndices`.
    pub child_indices: Vec<u32>,
}

/// A failure while reading a blend-tree node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BlendTreeNodeError {
    /// An index fell outside the array it addresses.
    IndexOutOfRange {
        /// The requested index.
        index: usize,
        /// The available length.
        length: usize,
    },
    /// `GetMinThreshold`/`GetMaxThreshold` found no thresholds.
    EmptyThresholds,
}

impl core::fmt::Display for BlendTreeNodeError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::IndexOutOfRange { index, length } => {
                write!(formatter, "blend-tree index {index} is outside {length}")
            }
            Self::EmptyThresholds => write!(formatter, "blend-tree node has no thresholds"),
        }
    }
}

impl std::error::Error for BlendTreeNodeError {}

impl BlendTreeNodeView {
    fn thresholds(&self) -> Option<&Vec<f32>> {
        if let Some(thresholds) = &self.blend1d_thresholds {
            if self.blend_type == BLEND_TREE_SIMPLE_1D {
                return Some(thresholds);
            }
            return None;
        }
        self.child_thresholds.as_ref()
    }

    /// `BlendTreeNodeConstantExtensions.GetThreshold`.
    pub fn get_threshold(&self, index: usize) -> Result<f32, BlendTreeNodeError> {
        if let Some(thresholds) = self.thresholds() {
            return thresholds
                .get(index)
                .copied()
                .ok_or(BlendTreeNodeError::IndexOutOfRange {
                    index,
                    length: thresholds.len(),
                });
        }
        Ok(0.0)
    }

    /// `BlendTreeNodeConstantExtensions.GetPosition`.
    pub fn get_position(&self, index: usize) -> Result<Vec2, BlendTreeNodeError> {
        if let Some(positions) = &self.blend2d_positions {
            if self.blend_type != BLEND_TREE_SIMPLE_1D && self.blend_type != BLEND_TREE_DIRECT {
                return positions
                    .get(index)
                    .copied()
                    .ok_or(BlendTreeNodeError::IndexOutOfRange {
                        index,
                        length: positions.len(),
                    });
            }
        }
        Ok(Vec2::default())
    }

    /// `BlendTreeNodeConstantExtensions.GetMinThreshold`.
    pub fn get_min_threshold(&self) -> Result<f32, BlendTreeNodeError> {
        if let Some(thresholds) = &self.blend1d_thresholds {
            if self.blend_type == BLEND_TREE_SIMPLE_1D {
                return thresholds
                    .iter()
                    .copied()
                    .reduce(f32::min)
                    .ok_or(BlendTreeNodeError::EmptyThresholds);
            }
        }
        Ok(0.0)
    }

    /// `BlendTreeNodeConstantExtensions.GetMaxThreshold`.
    pub fn get_max_threshold(&self) -> Result<f32, BlendTreeNodeError> {
        if let Some(thresholds) = &self.blend1d_thresholds {
            if self.blend_type == BLEND_TREE_SIMPLE_1D {
                return thresholds
                    .iter()
                    .copied()
                    .reduce(f32::max)
                    .ok_or(BlendTreeNodeError::EmptyThresholds);
            }
        }
        Ok(1.0)
    }

    /// `BlendTreeNodeConstantExtensions.TryGetDirectBlendParameter`.
    pub fn try_get_direct_blend_parameter(
        &self,
        index: usize,
    ) -> Result<Option<u32>, BlendTreeNodeError> {
        if let Some(ids) = &self.blend_direct_ids {
            if self.blend_type == BLEND_TREE_DIRECT {
                let parameter =
                    ids.get(index)
                        .copied()
                        .ok_or(BlendTreeNodeError::IndexOutOfRange {
                            index,
                            length: ids.len(),
                        })?;
                return Ok(Some(parameter));
            }
        }
        Ok(None)
    }

    /// `BlendTreeNodeConstantExtensions.IsBlendTree`.
    pub fn is_blend_tree(&self) -> bool {
        !self.child_indices.is_empty()
    }
}

/// `BlendTreeExtensions.GetBlendType`: the stored `BlendTreeType_1` discriminant.
///
/// `has_int32` answers `Has_BlendType_Int32`; exactly one of the two slots is present per
/// version. The `UInt32` read wraps to `i32` exactly like the source enum conversion.
pub fn blend_tree_blend_type(has_int32: bool, int32_value: i32, uint32_value: u32) -> i32 {
    if has_int32 {
        int32_value
    } else {
        uint32_value as i32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(blend_type: i32) -> BlendTreeNodeView {
        BlendTreeNodeView {
            blend_type,
            blend1d_thresholds: Some(vec![0.25, 0.75, 1.5]),
            child_thresholds: None,
            blend2d_positions: Some(vec![
                Vec2 { x: 1.0, y: 2.0 },
                Vec2 { x: 3.0, y: 4.0 },
                Vec2 { x: 5.0, y: 6.0 },
            ]),
            blend_direct_ids: Some(vec![11, 12, 13]),
            child_indices: vec![0, 1],
        }
    }

    /// Byte-for-byte differential against the reference `BlendTreeNodeConstantExtensions` (probe
    /// `Temp/opencode/btprobe`).
    #[test]
    fn blend_tree_node_matches_the_reference_runtime() {
        let fixture = include_str!("blend_tree_node_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "BTN", "{line}");
            let blend_type: i32 = parts[3].parse().unwrap();
            let view = node(blend_type);
            assert_eq!(view.blend1d_thresholds.is_some(), parts[4] == "1", "{line}");
            assert_eq!(view.blend2d_positions.is_some(), parts[5] == "1", "{line}");
            assert_eq!(view.blend_direct_ids.is_some(), parts[6] == "1", "{line}");
            assert_eq!(view.child_thresholds.is_some(), parts[7] == "1", "{line}");
            assert_eq!(view.blend_type, blend_type, "{line}");

            assert_eq!(
                view.get_threshold(0).unwrap().to_bits(),
                parts[8].parse::<u32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                view.get_threshold(1).unwrap().to_bits(),
                parts[9].parse::<u32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                view.get_min_threshold().unwrap().to_bits(),
                parts[10].parse::<u32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                view.get_max_threshold().unwrap().to_bits(),
                parts[11].parse::<u32>().unwrap(),
                "{line}"
            );
            let position = view.get_position(0).unwrap();
            assert_eq!(
                position.x.to_bits(),
                parts[12].parse::<u32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                position.y.to_bits(),
                parts[13].parse::<u32>().unwrap(),
                "{line}"
            );
            let direct = view.try_get_direct_blend_parameter(0).unwrap();
            match direct {
                Some(parameter) => {
                    assert_eq!(parts[14], "1", "{line}");
                    assert_eq!(parameter, parts[15].parse::<u32>().unwrap(), "{line}");
                }
                None => {
                    assert_eq!(parts[14], "0", "{line}");
                    assert_eq!(parts[15], "-1", "{line}");
                }
            }
            assert_eq!(view.is_blend_tree(), parts[16] == "1", "{line}");
            rows += 1;
        }
        assert_eq!(rows, 15);
    }

    /// `BlendTreeExtensions.GetBlendType` reads the versioned representation: `Int32`
    /// when present, otherwise the wrapping `UInt32` slot.
    #[test]
    fn blend_type_follows_the_int32_presence_gate() {
        // The Int32 slot wins whenever the gate says it exists.
        assert_eq!(blend_tree_blend_type(true, BLEND_TREE_DIRECT, 0), 4);
        assert_eq!(blend_tree_blend_type(true, BLEND_TREE_SIMPLE_1D, 4), 0);
        // Otherwise the UInt32 slot converts, wrapping like the source enum cast.
        assert_eq!(
            blend_tree_blend_type(false, 0, BLEND_TREE_FREEFORM_CARTESIAN_2D as u32),
            3
        );
        assert_eq!(blend_tree_blend_type(false, 0, u32::MAX), -1);
    }
}
