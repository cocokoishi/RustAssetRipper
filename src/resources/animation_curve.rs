//! `AnimationCurveExtensions` single-float curve construction.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/AnimationCurveExtensions.cs` for the
//! `IAnimationCurve_Single` overloads used by the exporters: the default-rotation/loop envelope,
//! and the two-keyframe value and slope builders.

use super::keyframe::{
    keyframe_presence, set_values_slopes as set_keyframe_slopes,
    set_values_weight as set_keyframe_weight, KeyframeView,
};
use super::vertex_data_blob::UnityVersion;

/// `CurveLoopTypes.Constant`.
pub const CURVE_LOOP_CONSTANT: i32 = 0;
/// `CurveLoopTypes.Cycle`.
pub const CURVE_LOOP_CYCLE: i32 = 1;
/// `CurveLoopTypes.CycleWithOffset`.
pub const CURVE_LOOP_CYCLE_WITH_OFFSET: i32 = 2;
/// `CurveLoopTypes.Oscillate`.
pub const CURVE_LOOP_OSCILLATE: i32 = 3;
/// `CurveLoopTypes.Linear`.
pub const CURVE_LOOP_LINEAR: i32 = 4;

/// `RotationOrder.OrderXYZ`.
pub const ROTATION_ORDER_XYZ: i32 = 0;
/// `RotationOrder.OrderXZY`.
pub const ROTATION_ORDER_XZY: i32 = 1;
/// `RotationOrder.OrderYZX`.
pub const ROTATION_ORDER_YZX: i32 = 2;
/// `RotationOrder.OrderYXZ`.
pub const ROTATION_ORDER_YXZ: i32 = 3;
/// `RotationOrder.OrderZXY`.
pub const ROTATION_ORDER_ZXY: i32 = 4;
/// `RotationOrder.OrderZYX`.
pub const ROTATION_ORDER_ZYX: i32 = 5;

/// Which optional `AnimationCurve_Single` fields a version serializes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CurvePresence {
    /// `Has_RotationOrder`.
    pub rotation_order: bool,
}

/// The version gate for `RotationOrder`: Unity 5.4 and later.
pub fn curve_presence(version: UnityVersion) -> CurvePresence {
    CurvePresence {
        rotation_order: version.at_least_at(5, 4),
    }
}

/// A single-float animation curve view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnimationCurveView {
    /// `PreInfinity`.
    pub pre_infinity: i32,
    /// `PostInfinity`.
    pub post_infinity: i32,
    /// `RotationOrder`.
    pub rotation_order: Option<i32>,
    /// `Curve`.
    pub curve: Vec<KeyframeView>,
}

/// `AnimationCurveExtensions.SetDefaultRotationOrderAndCurveLoopType`.
pub fn set_default_rotation_order_and_curve_loop_type(curve: &mut AnimationCurveView) {
    curve.pre_infinity = CURVE_LOOP_CYCLE_WITH_OFFSET;
    curve.post_infinity = CURVE_LOOP_CYCLE_WITH_OFFSET;
    if let Some(slot) = &mut curve.rotation_order {
        *slot = ROTATION_ORDER_ZXY;
    }
}

fn new_keyframe(version: UnityVersion) -> KeyframeView {
    let presence = keyframe_presence(version);
    KeyframeView {
        in_weight: presence.in_weight.then_some(0.0),
        out_weight: presence.out_weight.then_some(0.0),
        weighted_mode: presence.weighted_mode.then_some(0),
        ..KeyframeView::default()
    }
}

/// `SetValues(version, defaultValue, defaultWeight)`: two keyframes at times 0 and 1.
pub fn set_values_default(
    curve: &mut AnimationCurveView,
    version: UnityVersion,
    default_value: f32,
    default_weight: f32,
) {
    set_default_rotation_order_and_curve_loop_type(curve);
    curve.curve.clear();
    for time in [0.0f32, 1.0] {
        let mut keyframe = new_keyframe(version);
        set_keyframe_weight(&mut keyframe, version, time, default_value, default_weight);
        curve.curve.push(keyframe);
    }
}

/// `SetValues(version, value1, value2, defaultWeight)`: two keyframes at times 0 and 1.
pub fn set_values_two(
    curve: &mut AnimationCurveView,
    version: UnityVersion,
    value1: f32,
    value2: f32,
    default_weight: f32,
) {
    set_default_rotation_order_and_curve_loop_type(curve);
    curve.curve.clear();
    for (time, value) in [(0.0f32, value1), (1.0, value2)] {
        let mut keyframe = new_keyframe(version);
        set_keyframe_weight(&mut keyframe, version, time, value, default_weight);
        curve.curve.push(keyframe);
    }
}

/// `SetValues(version, value1, inSlope1, outSlope1, value2, inSlope2, outSlope2, defaultWeight)`.
#[allow(clippy::too_many_arguments)]
pub fn set_values_slopes(
    curve: &mut AnimationCurveView,
    version: UnityVersion,
    value1: f32,
    in_slope1: f32,
    out_slope1: f32,
    value2: f32,
    in_slope2: f32,
    out_slope2: f32,
    default_weight: f32,
) {
    set_default_rotation_order_and_curve_loop_type(curve);
    curve.curve.clear();
    for (time, value, in_slope, out_slope) in [
        (0.0f32, value1, in_slope1, out_slope1),
        (1.0, value2, in_slope2, out_slope2),
    ] {
        let mut keyframe = new_keyframe(version);
        set_keyframe_slopes(
            &mut keyframe,
            version,
            time,
            value,
            in_slope,
            out_slope,
            default_weight,
        );
        curve.curve.push(keyframe);
    }
}

/// `CopyValues` of a source keyframe into a target-version keyframe: the target's own field
/// presence decides which optional fields survive, and an absent source field reads as its default.
fn copy_keyframe_for_version(version: UnityVersion, source: &KeyframeView) -> KeyframeView {
    let presence = keyframe_presence(version);
    KeyframeView {
        time: source.time,
        value: source.value,
        in_slope: source.in_slope,
        out_slope: source.out_slope,
        in_weight: presence.in_weight.then(|| source.in_weight.unwrap_or(0.0)),
        out_weight: presence
            .out_weight
            .then(|| source.out_weight.unwrap_or(0.0)),
        tangent_mode: source.tangent_mode,
        weighted_mode: presence
            .weighted_mode
            .then(|| source.weighted_mode.unwrap_or(0)),
    }
}

/// `SetValues(version, keyframe)`: one copied keyframe.
pub fn set_values_keyframe(
    curve: &mut AnimationCurveView,
    version: UnityVersion,
    keyframe: &KeyframeView,
) {
    set_default_rotation_order_and_curve_loop_type(curve);
    curve.curve.clear();
    curve
        .curve
        .push(copy_keyframe_for_version(version, keyframe));
}

/// `SetValues(version, keyframe1, keyframe2)`.
pub fn set_values_keyframe_pair(
    curve: &mut AnimationCurveView,
    version: UnityVersion,
    keyframe1: &KeyframeView,
    keyframe2: &KeyframeView,
) {
    set_default_rotation_order_and_curve_loop_type(curve);
    curve.curve.clear();
    curve
        .curve
        .push(copy_keyframe_for_version(version, keyframe1));
    curve
        .curve
        .push(copy_keyframe_for_version(version, keyframe2));
}

/// `SetValues(version, IReadOnlyList<IKeyframe_Single>)`.
pub fn set_values_keyframes(
    curve: &mut AnimationCurveView,
    version: UnityVersion,
    keyframes: &[KeyframeView],
) {
    set_default_rotation_order_and_curve_loop_type(curve);
    curve.curve = keyframes
        .iter()
        .map(|keyframe| copy_keyframe_for_version(version, keyframe))
        .collect();
}

/// `SetValues(version, IReadOnlyList<IKeyframe_Single>, preInfinity, postInfinity)`.
pub fn set_values_keyframes_enveloped(
    curve: &mut AnimationCurveView,
    version: UnityVersion,
    keyframes: &[KeyframeView],
    pre_infinity: i32,
    post_infinity: i32,
) {
    curve.pre_infinity = pre_infinity;
    curve.post_infinity = post_infinity;
    if let Some(slot) = &mut curve.rotation_order {
        *slot = ROTATION_ORDER_ZXY;
    }
    curve.curve = keyframes
        .iter()
        .map(|keyframe| copy_keyframe_for_version(version, keyframe))
        .collect();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named_constant(name: &str, kind: &str) -> i32 {
        match (kind, name) {
            ("loop", "Constant") => CURVE_LOOP_CONSTANT,
            ("loop", "Cycle") => CURVE_LOOP_CYCLE,
            ("loop", "CycleWithOffset") => CURVE_LOOP_CYCLE_WITH_OFFSET,
            ("loop", "Oscillate") => CURVE_LOOP_OSCILLATE,
            ("loop", "Linear") => CURVE_LOOP_LINEAR,
            ("rot", "OrderXYZ") => ROTATION_ORDER_XYZ,
            ("rot", "OrderXZY") => ROTATION_ORDER_XZY,
            ("rot", "OrderYZX") => ROTATION_ORDER_YZX,
            ("rot", "OrderYXZ") => ROTATION_ORDER_YXZ,
            ("rot", "OrderZXY") => ROTATION_ORDER_ZXY,
            ("rot", "OrderZYX") => ROTATION_ORDER_ZYX,
            _ => panic!("unknown {kind} {name}"),
        }
    }

    fn bits(value: Option<f32>) -> u32 {
        value.map(f32::to_bits).unwrap_or(0)
    }

    /// Byte-for-byte differential against the reference `AnimationCurveExtensions` (probe
    /// `Temp/opencode/curveprobe`).
    #[test]
    fn animation_curve_set_values_matches_the_reference_runtime() {
        let fixture = include_str!("animation_curve_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            match parts[0] {
                "LOOP" => {
                    assert_eq!(
                        named_constant(parts[1], "loop"),
                        parts[2].parse::<i32>().unwrap(),
                        "{line}"
                    );
                }
                "ROT" => {
                    assert_eq!(
                        named_constant(parts[1], "rot"),
                        parts[2].parse::<i32>().unwrap(),
                        "{line}"
                    );
                }
                "C" => {
                    let version = UnityVersion::parse(parts[1]).unwrap();
                    let rotation_order = parts[2] == "1";
                    assert_eq!(
                        curve_presence(version).rotation_order,
                        rotation_order,
                        "{line}"
                    );
                    let mut curve = AnimationCurveView {
                        rotation_order: rotation_order.then_some(0),
                        ..AnimationCurveView::default()
                    };
                    match parts[3] {
                        "defaultValue" => set_values_default(&mut curve, version, 0.5, 0.25),
                        "twoValue" => set_values_two(&mut curve, version, 3.0, 4.0, 1.0 / 3.0),
                        "slopes" => set_values_slopes(
                            &mut curve, version, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 0.75,
                        ),
                        other => panic!("unknown curve kind {other}"),
                    }
                    let envelope: Vec<&str> = parts[4].split(',').collect();
                    assert_eq!(
                        curve.pre_infinity,
                        envelope[0].parse::<i32>().unwrap(),
                        "{line}"
                    );
                    assert_eq!(
                        curve.post_infinity,
                        envelope[1].parse::<i32>().unwrap(),
                        "{line}"
                    );
                    assert_eq!(
                        curve.rotation_order.unwrap_or(0),
                        envelope[2].parse::<i32>().unwrap(),
                        "{line}"
                    );
                    let expected: Vec<&str> = parts[5].split(';').collect();
                    assert_eq!(curve.curve.len(), expected.len(), "{line}");
                    for (keyframe, row) in curve.curve.iter().zip(expected) {
                        let values: Vec<&str> = row.split(',').collect();
                        assert_eq!(
                            keyframe.time.to_bits(),
                            values[0].parse::<u32>().unwrap(),
                            "{line}"
                        );
                        assert_eq!(
                            keyframe.value.to_bits(),
                            values[1].parse::<u32>().unwrap(),
                            "{line}"
                        );
                        assert_eq!(
                            keyframe.in_slope.to_bits(),
                            values[2].parse::<u32>().unwrap(),
                            "{line}"
                        );
                        assert_eq!(
                            keyframe.out_slope.to_bits(),
                            values[3].parse::<u32>().unwrap(),
                            "{line}"
                        );
                        assert_eq!(
                            bits(keyframe.in_weight),
                            values[4].parse::<u32>().unwrap(),
                            "{line}"
                        );
                        assert_eq!(
                            bits(keyframe.out_weight),
                            values[5].parse::<u32>().unwrap(),
                            "{line}"
                        );
                        assert_eq!(
                            keyframe.tangent_mode,
                            values[6].parse::<i32>().unwrap(),
                            "{line}"
                        );
                        assert_eq!(
                            keyframe.weighted_mode.unwrap_or(0),
                            values[7].parse::<i32>().unwrap(),
                            "{line}"
                        );
                    }
                    rows += 1;
                }
                other => panic!("unknown fixture row {other}"),
            }
        }
        assert_eq!(rows, 24);
    }

    fn source_keyframe(source_version: UnityVersion) -> KeyframeView {
        let presence = keyframe_presence(source_version);
        KeyframeView {
            time: 0.25,
            value: 1.5,
            in_slope: 2.5,
            out_slope: 3.5,
            in_weight: presence.in_weight.then_some(4.5),
            out_weight: presence.out_weight.then_some(5.5),
            tangent_mode: 0,
            weighted_mode: presence.weighted_mode.then_some(0),
        }
    }

    fn assert_envelope_and_keyframes(
        line: &str,
        curve: &AnimationCurveView,
        envelope: &[&str],
        keyframes: &[&str],
    ) {
        assert_eq!(
            curve.pre_infinity,
            envelope[0].parse::<i32>().unwrap(),
            "{line}"
        );
        assert_eq!(
            curve.post_infinity,
            envelope[1].parse::<i32>().unwrap(),
            "{line}"
        );
        assert_eq!(
            curve.rotation_order.unwrap_or(0),
            envelope[2].parse::<i32>().unwrap(),
            "{line}"
        );
        assert_eq!(curve.curve.len(), keyframes.len(), "{line}");
        for (keyframe, row) in curve.curve.iter().zip(keyframes) {
            let values: Vec<&str> = row.split(',').collect();
            assert_eq!(
                keyframe.time.to_bits(),
                values[0].parse::<u32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                keyframe.value.to_bits(),
                values[1].parse::<u32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                keyframe.in_slope.to_bits(),
                values[2].parse::<u32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                keyframe.out_slope.to_bits(),
                values[3].parse::<u32>().unwrap(),
                "{line}"
            );
            let expected_in = values[4].parse::<i64>().unwrap();
            if expected_in == -1 {
                assert_eq!(keyframe.in_weight, None, "{line}");
            } else {
                assert_eq!(bits(keyframe.in_weight), expected_in as u32, "{line}");
            }
            let expected_out = values[5].parse::<i64>().unwrap();
            if expected_out == -1 {
                assert_eq!(keyframe.out_weight, None, "{line}");
            } else {
                assert_eq!(bits(keyframe.out_weight), expected_out as u32, "{line}");
            }
            assert_eq!(
                keyframe.tangent_mode,
                values[6].parse::<i32>().unwrap(),
                "{line}"
            );
            let expected_weighted = values[7].parse::<i64>().unwrap();
            if expected_weighted == -1 {
                assert_eq!(keyframe.weighted_mode, None, "{line}");
            } else {
                assert_eq!(
                    keyframe.weighted_mode,
                    Some(expected_weighted as i32),
                    "{line}"
                );
            }
        }
    }

    /// Byte-for-byte differential against the reference `AnimationCurveExtensions` copy overloads
    /// (probe `Temp/opencode/curve2probe`).
    #[test]
    fn animation_curve_copy_overloads_match_the_reference_runtime() {
        let fixture = include_str!("animation_curve_copy_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "CC", "{line}");
            let target = UnityVersion::parse(parts[1]).unwrap();
            let source_version = UnityVersion::parse(parts[2]).unwrap();
            let source = source_keyframe(source_version);
            let mut curve = AnimationCurveView {
                rotation_order: curve_presence(target).rotation_order.then_some(0),
                ..AnimationCurveView::default()
            };
            match parts[3] {
                "single" => set_values_keyframe(&mut curve, target, &source),
                "pair" => set_values_keyframe_pair(&mut curve, target, &source, &source),
                "list" => set_values_keyframes(&mut curve, target, &[source, source]),
                "enveloped" => set_values_keyframes_enveloped(
                    &mut curve,
                    target,
                    &[source],
                    CURVE_LOOP_LINEAR,
                    CURVE_LOOP_CONSTANT,
                ),
                other => panic!("unknown copy kind {other}"),
            }
            let envelope: Vec<&str> = parts[4].split(',').collect();
            let keyframes: Vec<&str> = parts[5].split(';').collect();
            assert_envelope_and_keyframes(line, &curve, &envelope, &keyframes);
            rows += 1;
        }
        assert_eq!(rows, 16);
    }
}
