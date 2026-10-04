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
}
