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
//! - `TryGetNextFrame`: the forward scan for the next frame holding a curve id.
//! - `AddAnimatorMuscleCurve`: the humanoid binding-name decision behind the emitted
//!   animator-muscle curve (`GetHumanoidMuscle` + `ToAttributeString`).
//! - `AddDefaultCurve` routing: the class-id dispatch (GameObject/Script/Engine paths)
//!   and the shared PPtr-vs-float keyframe split behind the script, engine, and
//!   custom-curve fallback emission sites.
//!
//! The curve-emission half that remains open needs mutable generated clip objects:
//! `AddTransformCurve` bucket writes (including the missing-`EulerCurves` skip),
//! attribute resolution (`GameObject.TryGetPath`, checksum-cache and `FieldHashes`
//! lookups with their `GetReversedPath` fallbacks), PPtr value mapping, and keyframe
//! creation on clip objects. `ROUND` fixture rows come from the `sclipprobe`
//! differential; the remaining arithmetic is verified against synthetic inputs.

use std::error::Error;
use std::fmt::{self, Display};

use super::avatar::{humanoid_muscle_attribute, update_humanoid_muscle};
use super::custom_curve::{CustomCurveError, TransformType};
use super::streamed_curve::StreamedFrame;
use super::vertex_data_blob::UnityVersion;

/// `ClassIDType.Transform`.
pub const CLASS_ID_TRANSFORM: i32 = 4;
/// `ClassIDType.RectTransform`.
pub const CLASS_ID_RECT_TRANSFORM: i32 = 224;
/// `ClassIDType.GameObject`.
pub const CLASS_ID_GAME_OBJECT: i32 = 1;
/// `ClassIDType.MonoBehaviour`.
pub const CLASS_ID_MONO_BEHAVIOUR: i32 = 114;

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

/// `AnimationClipConverter.TryGetNextFrame`: the first frame strictly after
/// `current_frame` holding a curve with `curve_id`, with its curve position.
///
/// Returns `None` where the source reports `false` (leaving its `nextFrame` null and
/// `curveIdx` at -1): no later frame, no matching curve, or a `current_frame` at the
/// address-space end.
pub fn try_get_next_frame(
    frames: &[StreamedFrame],
    current_frame: usize,
    curve_id: i32,
) -> Option<(usize, usize)> {
    let later = current_frame.checked_add(1)?;
    frames
        .iter()
        .enumerate()
        .skip(later)
        .find_map(|(frame_index, frame)| {
            frame
                .curves
                .iter()
                .position(|curve| curve.index == curve_id)
                .map(|curve_index| (frame_index, curve_index))
        })
}

/// `AddAnimatorMuscleCurve` attribute decision: `HumanoidMuscleType.ToAttributeString`
/// of `GenericBinding.GetHumanoidMuscle` (the binding attribute updated to the clip
/// version). The `u32` attribute wraps to `i32` exactly like the source unchecked enum
/// cast; ids outside the muscle table read `None`. Curve emission (empty path,
/// `Animator` class id, float keyframe) needs mutable clip objects and stays open.
pub fn animator_muscle_attribute(attribute: u32, version: UnityVersion) -> Option<String> {
    let muscle = update_humanoid_muscle(attribute as i32, version);
    humanoid_muscle_attribute(muscle)
}

/// `AnimationClipConverter.AddDefaultCurve` routing: the class id alone decides
/// which emission path a default (non-transform, non-custom) binding takes.
/// Attribute resolution, PPtr mapping, and keyframe creation on generated clip
/// objects stay open in the converter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DefaultCurveTarget {
    /// `AddGameObjectCurve`.
    GameObject,
    /// `AddScriptCurve`.
    Script,
    /// `AddEngineCurve`.
    Engine,
}

/// The source `switch` on the binding class id: `GameObject` and `MonoBehaviour`
/// take their dedicated paths, every other class id falls into the engine path.
pub const fn default_curve_target(class_id: i32) -> DefaultCurveTarget {
    if class_id == CLASS_ID_GAME_OBJECT {
        DefaultCurveTarget::GameObject
    } else if class_id == CLASS_ID_MONO_BEHAVIOUR {
        DefaultCurveTarget::Script
    } else {
        DefaultCurveTarget::Engine
    }
}

/// Keyframe flavor selected by `GenericBindingExtensions.IsPPtrCurve` (`IsPPtrCurve
/// != 0`, carried by the caller). The same split steers the custom-curve fallback,
/// the script path, and the engine path: PPtr curves index the PPtr mapping with an
/// integer key, everything else emits a float key with tangents.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyframeKind {
    /// `AddPPtrKeyframe`.
    PPtr,
    /// `AddFloatKeyframe`.
    Float,
}

/// The shared `IsPPtrCurve` branch behind the three PPtr/float emission sites.
pub const fn keyframe_kind(is_pptr_curve: bool) -> KeyframeKind {
    if is_pptr_curve {
        KeyframeKind::PPtr
    } else {
        KeyframeKind::Float
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

    #[test]
    fn next_frame_scan_skips_current_and_reports_positions() {
        let frames = [frame(0.0, &[0, 1]), frame(1.0, &[1, 0]), frame(2.0, &[2])];
        // The current frame never matches, even when it holds the id.
        assert_eq!(try_get_next_frame(&frames, 0, 0), Some((1, 1)));
        assert_eq!(try_get_next_frame(&frames, 0, 1), Some((1, 0)));
        assert_eq!(try_get_next_frame(&frames, 1, 2), Some((2, 0)));
        // Past the last holder, or with no holder at all, the scan misses.
        assert_eq!(try_get_next_frame(&frames, 1, 0), None);
        assert_eq!(try_get_next_frame(&frames, 2, 0), None);
        assert_eq!(try_get_next_frame(&frames, 0, 9), None);
        assert_eq!(try_get_next_frame(&[], 0, 0), None);
        assert_eq!(try_get_next_frame(&frames, usize::MAX, 0), None);
    }

    #[test]
    fn default_curve_routing_follows_the_source_switch() {
        assert_eq!(
            default_curve_target(CLASS_ID_GAME_OBJECT),
            DefaultCurveTarget::GameObject
        );
        assert_eq!(
            default_curve_target(CLASS_ID_MONO_BEHAVIOUR),
            DefaultCurveTarget::Script
        );
        // The source default arm catches everything else, including other component
        // class ids and out-of-range garbage.
        for class_id in [
            0,
            -1,
            CLASS_ID_TRANSFORM,
            CLASS_ID_RECT_TRANSFORM,
            95,
            137,
            100_000,
            i32::MIN,
            i32::MAX,
        ] {
            assert_eq!(
                default_curve_target(class_id),
                DefaultCurveTarget::Engine,
                "{class_id}"
            );
        }
        assert_eq!(CLASS_ID_GAME_OBJECT, 1);
        assert_eq!(CLASS_ID_MONO_BEHAVIOUR, 114);
    }

    #[test]
    fn keyframe_flavor_follows_the_pptr_flag() {
        // The custom-curve fallback, the script path, and the engine path all
        // branch on `IsPPtrCurve` before emitting.
        assert_eq!(keyframe_kind(true), KeyframeKind::PPtr);
        assert_eq!(keyframe_kind(false), KeyframeKind::Float);
    }

    #[test]
    fn animator_muscle_names_follow_the_updated_attribute() {
        let modern = UnityVersion::parse("2022.3.1f1").unwrap();
        let legacy = UnityVersion::parse("5.0.0f1").unwrap();
        assert_eq!(
            animator_muscle_attribute(42, modern).as_deref(),
            Some("Spine Front-Back")
        );
        // Attribute 48 is UpperChest on modern layouts but shifts pre-5.6.
        assert_eq!(
            animator_muscle_attribute(48, modern).as_deref(),
            Some("UpperChest Front-Back")
        );
        assert_eq!(
            animator_muscle_attribute(48, legacy).as_deref(),
            Some("Neck Nod Down-Up")
        );
        // Out-of-range attributes read None on every version.
        assert_eq!(animator_muscle_attribute(200, modern), None);
        assert_eq!(animator_muscle_attribute(u32::MAX, legacy), None);
    }
}
