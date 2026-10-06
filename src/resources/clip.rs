//! `ClipExtensions`.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/ClipExtensions.cs`. A clip is "set" when any of
//! its streamed, dense, or (version-gated) constant curves carry data. Legacy clips also keep a
//! raw value array that reconstructs generic bindings through a CRC classification.

/// `CRC(PositionX)`.
pub const CRC_POSITION_X: u32 = 4_174_552_735;
/// `CRC(QuaternionX)`.
pub const CRC_QUATERNION_X: u32 = 2_211_994_246;
/// `CRC(ScaleX)`.
pub const CRC_SCALE_X: u32 = 1_512_518_241;
/// `ClassIDType.Transform`.
pub const CLASS_ID_TRANSFORM: i32 = 4;
/// `ClassIDType.Animator`.
pub const CLASS_ID_ANIMATOR: i32 = 95;
/// `kBindTransformPosition`.
pub const ATTRIBUTE_TRANSFORM_POSITION: u32 = 1;
/// `kBindTransformRotation`.
pub const ATTRIBUTE_TRANSFORM_ROTATION: u32 = 2;
/// `kBindTransformScale`.
pub const ATTRIBUTE_TRANSFORM_SCALE: u32 = 3;

/// One `ValueArray` entry.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ValueArrayEntry {
    /// `ID`.
    pub id: u32,
    /// `TypeID`.
    pub type_id: u32,
}

/// A reconstructed generic binding.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GenericBindingView {
    /// `Path`.
    pub path: u32,
    /// `Attribute`.
    pub attribute: u32,
    /// `ClassID`.
    pub class_id: i32,
}

/// A clip-constant view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClipView {
    /// `StreamedClip.IsSet`.
    pub streamed_is_set: bool,
    /// `DenseClip.IsSet`.
    pub dense_is_set: bool,
    /// `Has_ConstantClip`.
    pub has_constant_clip: bool,
    /// `ConstantClip.IsSet`.
    pub constant_is_set: bool,
    /// `Binding` value array: `None` when the clip has no binding.
    pub binding: Option<Vec<ValueArrayEntry>>,
}

impl ClipView {
    /// `ClipExtensions.IsSet`.
    pub fn is_set(&self) -> bool {
        self.streamed_is_set
            || self.dense_is_set
            || (self.has_constant_clip && self.constant_is_set)
    }

    /// `ClipExtensions.ConvertValueArrayToGenericBinding`.
    pub fn convert_value_array_to_generic_binding(&self) -> Vec<GenericBindingView> {
        let Some(values) = &self.binding else {
            return Vec::new();
        };
        let mut bindings = Vec::new();
        let mut index = 0;
        while index < values.len() {
            let entry = values[index];
            if entry.type_id == CRC_POSITION_X {
                bindings.push(GenericBindingView {
                    path: entry.id,
                    attribute: ATTRIBUTE_TRANSFORM_POSITION,
                    class_id: CLASS_ID_TRANSFORM,
                });
                index += 3;
            } else if entry.type_id == CRC_QUATERNION_X {
                bindings.push(GenericBindingView {
                    path: entry.id,
                    attribute: ATTRIBUTE_TRANSFORM_ROTATION,
                    class_id: CLASS_ID_TRANSFORM,
                });
                index += 4;
            } else if entry.type_id == CRC_SCALE_X {
                bindings.push(GenericBindingView {
                    path: entry.id,
                    attribute: ATTRIBUTE_TRANSFORM_SCALE,
                    class_id: CLASS_ID_TRANSFORM,
                });
                index += 3;
            } else {
                bindings.push(GenericBindingView {
                    path: 0,
                    attribute: entry.id,
                    class_id: CLASS_ID_ANIMATOR,
                });
                index += 1;
            }
        }
        bindings
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe_value_array() -> Vec<ValueArrayEntry> {
        vec![
            ValueArrayEntry {
                id: 100,
                type_id: CRC_POSITION_X,
            },
            ValueArrayEntry::default(),
            ValueArrayEntry::default(),
            ValueArrayEntry {
                id: 200,
                type_id: CRC_QUATERNION_X,
            },
            ValueArrayEntry::default(),
            ValueArrayEntry::default(),
            ValueArrayEntry::default(),
            ValueArrayEntry {
                id: 300,
                type_id: CRC_SCALE_X,
            },
            ValueArrayEntry::default(),
            ValueArrayEntry::default(),
            ValueArrayEntry {
                id: 400,
                type_id: 1234,
            },
        ]
    }

    /// Byte-for-byte differential against the reference `ClipExtensions` (probe
    /// `Temp/opencode/clip2probe`).
    #[test]
    fn clip_matches_the_reference_runtime() {
        let is_set_fixture = include_str!("clip_is_set_fixture.txt");
        let mut is_set_rows = 0;
        for line in is_set_fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "CIS", "{line}");
            let has_constant = parts[2] == "1";
            let base = ClipView {
                streamed_is_set: false,
                dense_is_set: false,
                has_constant_clip: has_constant,
                constant_is_set: false,
                binding: None,
            };
            assert_eq!(base.is_set(), parts[3] == "1", "{line}");
            assert_eq!(
                ClipView {
                    streamed_is_set: true,
                    ..base.clone()
                }
                .is_set(),
                parts[4] == "1",
                "{line}"
            );
            assert_eq!(
                ClipView {
                    dense_is_set: true,
                    ..base.clone()
                }
                .is_set(),
                parts[5] == "1",
                "{line}"
            );
            assert_eq!(
                ClipView {
                    constant_is_set: true,
                    ..base.clone()
                }
                .is_set(),
                parts[6] == "1",
                "{line}"
            );
            is_set_rows += 1;
        }
        assert_eq!(is_set_rows, 9);

        let binding_fixture = include_str!("clip_binding_fixture.txt");
        let mut captured: Vec<GenericBindingView> = Vec::new();
        let mut current: Vec<GenericBindingView> = Vec::new();
        let mut binding_rows = 0;
        for line in binding_fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            match parts[0] {
                "CB" => {
                    if !current.is_empty() {
                        captured = current.clone();
                    }
                    current.clear();
                    binding_rows += 1;
                }
                "CBB" => current.push(GenericBindingView {
                    path: parts[3].parse().unwrap(),
                    attribute: parts[4].parse().unwrap(),
                    class_id: parts[5].parse().unwrap(),
                }),
                other => panic!("unexpected fixture tag {other} in {line}"),
            }
        }
        if !current.is_empty() {
            captured = current;
        }
        assert_eq!(binding_rows, 4);

        // The fixture's last version has no binding; the earlier ones share one expected set.
        let with_binding = ClipView {
            binding: Some(probe_value_array()),
            ..ClipView::default()
        };
        let converted = with_binding.convert_value_array_to_generic_binding();
        assert_eq!(converted.len(), 4);
        assert_eq!(converted, captured);

        let without_binding = ClipView {
            binding: None,
            ..ClipView::default()
        };
        assert!(without_binding
            .convert_value_array_to_generic_binding()
            .is_empty());
    }

    #[test]
    fn value_array_strides_tolerate_truncated_tails() {
        // Both runtimes only read the classification entry before skipping ahead, so a
        // tail cut mid-stride still yields its binding instead of failing.
        let truncated = ClipView {
            binding: Some(vec![ValueArrayEntry {
                id: 7,
                type_id: CRC_QUATERNION_X,
            }]),
            ..ClipView::default()
        };
        assert_eq!(
            truncated.convert_value_array_to_generic_binding(),
            vec![GenericBindingView {
                path: 7,
                attribute: ATTRIBUTE_TRANSFORM_ROTATION,
                class_id: CLASS_ID_TRANSFORM,
            }]
        );
        let short_pair = ClipView {
            binding: Some(vec![
                ValueArrayEntry {
                    id: 9,
                    type_id: CRC_POSITION_X,
                },
                ValueArrayEntry::default(),
            ]),
            ..ClipView::default()
        };
        assert_eq!(
            short_pair.convert_value_array_to_generic_binding(),
            vec![GenericBindingView {
                path: 9,
                attribute: ATTRIBUTE_TRANSFORM_POSITION,
                class_id: CLASS_ID_TRANSFORM,
            }]
        );
        // A present-but-empty value array converts to no bindings.
        let empty = ClipView {
            binding: Some(Vec::new()),
            ..ClipView::default()
        };
        assert!(empty.convert_value_array_to_generic_binding().is_empty());
    }
}
