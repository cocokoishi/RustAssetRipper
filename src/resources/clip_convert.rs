//! `AnimationClipConverter` orchestration math.
//!
//! Ports the caller-independent calculations of
//! `AssetRipper.Processing/AnimationClips/AnimationClipConverter.cs`:
//!
//! - `InitializeBindingsCache`: expands the binding-constant list into a per-curve
//!   binding map, using the transform dimension for transform bindings. The
//!   `IsTransform` precedence (`Transform` always; `RectTransform` only with a valid
//!   transform type) is reproduced exactly.
//! - `CalculateLastConstantTime`: the stop-time adjustment for constant curves,
//!   including the banker's-rounding frame math and the PPtr-curve special case.
//! - `ProcessStreams` first-frame fix (`frames[0].Time = 0` past the first frame).
//! - `ProcessDenses` frame-time math and the `ProcessConstant` one-or-two-frames
//!   rule, plus the `DefaultFloatWeight` constant.
//!
//! The curve-emission half (`AddTransformCurve`, `AddDefaultCurve`,
//! `AddCustomCurve`, keyframe creation on clip objects) needs mutable generated clip
//! objects and stays open. `ROUND` fixture rows come from the `sclipprobe`
//! differential; the remaining arithmetic is verified against synthetic inputs.

use std::error::Error;
use std::fmt::{self, Display};

use super::custom_curve::{CustomCurveError, TransformType};
use super::streamed_curve::StreamedFrame;

/// `ClassIDType.Transform`.
pub const CLASS_ID_TRANSFORM: i32 = 4;
/// `ClassIDType.RectTransform`.
pub const CLASS_ID_RECT_TRANSFORM: i32 = 224;

/// `AnimationClipConverter.DefaultFloatWeight` (`1/3`).
pub const DEFAULT_FLOAT_WEIGHT: f32 = 1.0 / 3.0;

/// One generic binding as the converter reads it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ConverterBinding {
    /// Binding path hash.
    pub path: u32,
    /// Binding attribute.
    pub attribute: u32,
    /// Binding class ID.
    pub class_id: i32,
    /// Binding custom type discriminant.
    pub custom_type: u8,
    /// Whether the binding addresses PPtr curves.
    pub is_pptr_curve: bool,
}

/// An animation-clip conversion failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClipConvertError {
    /// A streamed curve index names no cached binding, like the source
    /// `ArgumentException`.
    MissingBinding {
        /// The curve index.
        curve_index: i32,
    },
    /// A transform binding carries an invalid transform type.
    InvalidTransform(CustomCurveError),
}

impl Display for ClipConvertError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingBinding { curve_index } => write!(
                formatter,
                "binding with index {curve_index} hasn't been found"
            ),
            Self::InvalidTransform(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for ClipConvertError {}

impl From<CustomCurveError> for ClipConvertError {
    fn from(error: CustomCurveError) -> Self {
        Self::InvalidTransform(error)
    }
}

/// `GenericBindingExtensions.IsTransform`, preserving the source precedence:
/// `Transform` is always a transform binding; `RectTransform` only with a valid
/// transform type.
pub fn is_transform_binding(class_id: i32, attribute: u32) -> bool {
    class_id == CLASS_ID_TRANSFORM
        || (class_id == CLASS_ID_RECT_TRANSFORM && TransformType(attribute as i32).is_valid())
}

/// `AnimationClipConverter.InitializeBindingsCache`: maps each curve index to its
/// binding position, expanding transform bindings by their dimension.
pub fn initialize_bindings_cache(
    bindings: &[ConverterBinding],
) -> Result<Vec<usize>, ClipConvertError> {
    let mut cache = Vec::new();
    for (position, binding) in bindings.iter().enumerate() {
        let dimension = if binding.class_id == CLASS_ID_TRANSFORM {
            TransformType(binding.attribute as i32).dimension()?
        } else {
            1
        };
        cache.extend(std::iter::repeat_n(position, dimension));
    }
    Ok(cache)
}

/// Looks a curve index up in an initialized bindings cache.
pub fn cached_binding(cache: &[usize], curve_index: i32) -> Result<usize, ClipConvertError> {
    usize::try_from(curve_index)
        .ok()
        .and_then(|index| cache.get(index).copied())
        .ok_or(ClipConvertError::MissingBinding { curve_index })
}

/// `float.Round` with the source banker's (to-even) rounding, preserving the input
/// sign on zero exactly like the runtime (`-0.5` rounds to `-0.0`).
pub fn bankers_round(value: f32) -> f32 {
    let rounded = f64::from(value).round_ties_even() as f32;
    if rounded == 0.0 {
        rounded.copysign(value)
    } else {
        rounded
    }
}

/// Truncates a rounded frame count exactly like the source `(int)` cast for
/// in-range values. Out-of-range inputs saturate where the reference conversion is
/// platform-defined; frame counts never reach that range.
pub fn truncate_frame_count(value: f32) -> i32 {
    value as i32
}

/// `AnimationClipConverter.CalculateLastConstantTime`.
///
/// `is_pptr_curve` answers `IsPPtrCurve` for a binding-cache position; a missing
/// binding is an explicit error like the source throw.
pub fn calculate_last_constant_time(
    frames: &[StreamedFrame],
    stop_time: f32,
    sample_rate: f32,
    cache: &[usize],
    bindings: &[ConverterBinding],
    is_pptr_curve: impl Fn(usize) -> Option<bool>,
) -> Result<f32, ClipConvertError> {
    if stop_time == 0.0 || frames.len() <= 1 {
        return Ok(stop_time);
    }
    let last_frame = truncate_frame_count(bankers_round(stop_time * sample_rate));
    let last_streamed = &frames[frames.len() - 2];
    let last_streamed_frame = if last_streamed.time > 0.0 {
        truncate_frame_count(bankers_round(last_streamed.time * sample_rate))
    } else {
        0
    };
    if last_frame - last_streamed_frame == 1 {
        for curve in &last_streamed.curves {
            let position = cached_binding(cache, curve.index)?;
            let binding = bindings
                .get(position)
                .ok_or(ClipConvertError::MissingBinding {
                    curve_index: curve.index,
                })?;
            let pptr = is_pptr_curve(position).unwrap_or(binding.is_pptr_curve);
            if pptr {
                return Ok(if last_streamed.time > 0.0 {
                    last_streamed.time
                } else {
                    0.0
                });
            }
        }
    }
    Ok(stop_time)
}

/// The `ProcessStreams` first-frame fix: past the first frame, frame zero starts at
/// time zero so PPtr curves open at `Time = 0` instead of `float.MinValue`.
pub fn fix_first_frame_time(frames: &mut [StreamedFrame]) {
    if frames.len() > 1 {
        frames[0].time = 0.0;
    }
}

/// `ProcessDenses` frame time: `frameIndex / sampleRate + beginTime`.
pub fn dense_frame_time(frame_index: i32, sample_rate: f32, begin_time: f32) -> f32 {
    frame_index as f32 / sample_rate + begin_time
}

/// `ProcessConstant` frame rule: one keyframe when the constant time is zero, two
/// (`0` and `lastFrame`) otherwise.
pub fn constant_frame_count(last_frame: f32) -> usize {
    if 0.0 == last_frame {
        1
    } else {
        2
    }
}

#[cfg(test)]
mod tests {
    use super::super::custom_curve::BindingCustomType;
    use super::super::streamed_curve::StreamedCurveKey;
    use super::*;

    fn binding(
        class_id: i32,
        attribute: u32,
        custom_type: u8,
        is_pptr_curve: bool,
    ) -> ConverterBinding {
        ConverterBinding {
            path: 0,
            attribute,
            class_id,
            custom_type,
            is_pptr_curve,
        }
    }

    fn frame(time: f32, indexes: &[i32]) -> StreamedFrame {
        StreamedFrame {
            time,
            curves: indexes
                .iter()
                .map(|&index| StreamedCurveKey {
                    index,
                    coefficient: [0.0; 3],
                    right_sided_limit: 0.0,
                    left_sided_limit: 0.0,
                    exact_left_sided_limit: true,
                    in_slope: 0.0,
                    out_slope: 0.0,
                })
                .collect(),
        }
    }

    #[test]
    fn transform_precedence_matches_the_snapshot() {
        // Plain Transform bindings are always transform bindings.
        assert!(is_transform_binding(4, 0));
        assert!(is_transform_binding(4, 9));
        // RectTransform needs a valid transform type.
        assert!(is_transform_binding(224, 1));
        assert!(is_transform_binding(224, 4));
        assert!(!is_transform_binding(224, 0));
        assert!(!is_transform_binding(224, 5));
        // Anything else is not a transform binding.
        assert!(!is_transform_binding(95, 1));
        assert!(!is_transform_binding(21, 1));
    }

    #[test]
    fn bindings_cache_expands_by_transform_dimension() {
        let bindings = [
            binding(4, 1, BindingCustomType::TRANSFORM.0, false),
            binding(95, 0, BindingCustomType::NONE.0, false),
            binding(4, 2, BindingCustomType::TRANSFORM.0, false),
        ];
        assert_eq!(
            initialize_bindings_cache(&bindings),
            Ok(vec![0, 0, 0, 1, 2, 2, 2, 2])
        );
        assert_eq!(cached_binding(&[0, 1], 1), Ok(1));
        assert_eq!(
            cached_binding(&[0, 1], 7),
            Err(ClipConvertError::MissingBinding { curve_index: 7 })
        );
        assert_eq!(
            cached_binding(&[0, 1], -1),
            Err(ClipConvertError::MissingBinding { curve_index: -1 })
        );
        assert!(initialize_bindings_cache(&[]).unwrap().is_empty());
        // An invalid transform type is an explicit error.
        assert!(matches!(
            initialize_bindings_cache(&[binding(4, 9, BindingCustomType::TRANSFORM.0, false)]),
            Err(ClipConvertError::InvalidTransform(_))
        ));
    }

    /// Differential against the runtime `float.Round` (probe `Temp/opencode/sclipprobe`).
    #[test]
    fn bankers_round_matches_the_reference_runtime() {
        let fixture = include_str!("streamed_curve_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split('|');
            if parts.clone().next() != Some("ROUND") {
                continue;
            }
            assert_eq!(parts.next(), Some("ROUND"), "{line}");
            let input = f32::from_bits(
                u32::from_str_radix(parts.next().expect("an input word"), 16).unwrap(),
            );
            let expected = u32::from_str_radix(parts.next().expect("a result word"), 16).unwrap();
            assert!(parts.next().is_none(), "{line}");
            assert_eq!(bankers_round(input).to_bits(), expected, "{line}");
            rows += 1;
        }
        assert_eq!(rows, 9, "all probed rounding rows are asserted");
    }

    #[test]
    fn last_constant_time_follows_the_source_branches() {
        let bindings = [binding(95, 0, BindingCustomType::NONE.0, false)];
        let cache = [0];
        // A zero stop time and a single frame short-circuit.
        assert_eq!(
            calculate_last_constant_time(
                &[frame(0.0, &[0]), frame(1.0, &[0])],
                0.0,
                30.0,
                &cache,
                &bindings,
                |_| Some(false)
            ),
            Ok(0.0)
        );
        assert_eq!(
            calculate_last_constant_time(&[frame(1.0, &[0])], 2.0, 30.0, &cache, &bindings, |_| {
                Some(false)
            }),
            Ok(2.0)
        );
        // A one-frame gap without PPtr curves keeps the stop time: round(31) - round(30) == 1.
        let frames = [
            frame(f32::MIN, &[0]),
            frame(1.0, &[0]),
            frame(f32::INFINITY, &[]),
        ];
        assert_eq!(
            calculate_last_constant_time(&frames, 31.0 / 30.0, 30.0, &cache, &bindings, |_| Some(
                false
            )),
            Ok(31.0 / 30.0)
        );
        // A one-frame gap with a PPtr curve returns the last streamed time.
        assert_eq!(
            calculate_last_constant_time(&frames, 31.0 / 30.0, 30.0, &cache, &bindings, |_| Some(
                true
            )),
            Ok(1.0)
        );
        // An undecided lookup falls back to the binding flag.
        let pptr_bindings = [binding(95, 0, BindingCustomType::NONE.0, true)];
        assert_eq!(
            calculate_last_constant_time(
                &frames,
                31.0 / 30.0,
                30.0,
                &cache,
                &pptr_bindings,
                |_| None
            ),
            Ok(1.0)
        );
        // A wider gap keeps the stop time even with PPtr curves.
        let wide = [
            frame(0.0, &[0]),
            frame(0.5, &[0]),
            frame(f32::INFINITY, &[]),
        ];
        assert_eq!(
            calculate_last_constant_time(&wide, 2.0, 30.0, &cache, &bindings, |_| Some(true)),
            Ok(2.0)
        );
        // A missing binding is an explicit error.
        assert_eq!(
            calculate_last_constant_time(&frames, 31.0 / 30.0, 30.0, &[], &bindings, |_| Some(
                true
            )),
            Err(ClipConvertError::MissingBinding { curve_index: 0 })
        );
    }

    #[test]
    fn first_frame_fix_dense_times_and_constant_counts() {
        let mut frames = [frame(5.0, &[0]), frame(1.0, &[0])];
        fix_first_frame_time(&mut frames);
        assert_eq!(frames[0].time, 0.0);
        assert_eq!(frames[1].time, 1.0);
        let mut single = [frame(5.0, &[0])];
        fix_first_frame_time(&mut single);
        assert_eq!(single[0].time, 5.0);
        assert_eq!(dense_frame_time(3, 30.0, 0.5), 3.0 / 30.0 + 0.5);
        assert_eq!(constant_frame_count(0.0), 1);
        assert_eq!(constant_frame_count(2.0), 2);
        assert_eq!(DEFAULT_FLOAT_WEIGHT.to_bits(), (1.0f32 / 3.0).to_bits());
    }
}
