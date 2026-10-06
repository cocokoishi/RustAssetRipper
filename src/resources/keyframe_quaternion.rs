//! `KeyframeExtensions` Quaternionf-keyframe defaults.
//!
//! Mirrors the `IKeyframe_Quaternionf` overloads of
//! `AssetRipper.SourceGenerated.Extensions/KeyframeExtensions.Quaternionf.cs`. The quaternion
//! weight can come from another quaternion, a scalar broadcast to all four lanes, or four explicit
//! components; the slope overloads use the free-free tangent mode.

use crate::numerics::Quat;

use super::keyframe::keyframe_presence;
use super::tangent_mode::{to_tangent, FREE_FREE, FREE_SMOOTH};
use super::vector_extensions::{copy_values_quaternion, set_quaternion_components};
use super::vertex_data_blob::UnityVersion;

/// A Quaternionf keyframe view with the optional fields as slots.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KeyframeQuaternionView {
    /// `Time`.
    pub time: f32,
    /// `Value`.
    pub value: Quat,
    /// `InSlope`.
    pub in_slope: Quat,
    /// `OutSlope`.
    pub out_slope: Quat,
    /// `InWeight`.
    pub in_weight: Option<Quat>,
    /// `OutWeight`.
    pub out_weight: Option<Quat>,
    /// `TangentMode`.
    pub tangent_mode: i32,
    /// `WeightedMode`.
    pub weighted_mode: Option<i32>,
}

impl KeyframeQuaternionView {
    /// Build a view whose optional slots match the version gate.
    pub fn for_version(version: UnityVersion) -> Self {
        let presence = keyframe_presence(version);
        let zero = Quat {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 0.0,
        };
        Self {
            in_weight: presence.in_weight.then_some(zero),
            out_weight: presence.out_weight.then_some(zero),
            weighted_mode: presence.weighted_mode.then_some(0),
            ..Self::default()
        }
    }

    fn apply_base(&mut self, version: UnityVersion, time: f32, value: Quat, tangent: i32) {
        self.time = time;
        copy_values_quaternion(&mut self.value, value);
        self.tangent_mode = to_tangent(tangent, version);
        if let Some(slot) = &mut self.weighted_mode {
            *slot = 0;
        }
    }

    fn set_weight_quat(&mut self, weight: Quat) {
        if let Some(slot) = &mut self.in_weight {
            copy_values_quaternion(slot, weight);
        }
        if let Some(slot) = &mut self.out_weight {
            copy_values_quaternion(slot, weight);
        }
    }

    fn set_weight_scalar(&mut self, weight: f32) {
        if let Some(slot) = &mut self.in_weight {
            set_quaternion_components(slot, weight, weight, weight, weight);
        }
        if let Some(slot) = &mut self.out_weight {
            set_quaternion_components(slot, weight, weight, weight, weight);
        }
    }

    fn set_weight_components(&mut self, x: f32, y: f32, z: f32, w: f32) {
        if let Some(slot) = &mut self.in_weight {
            set_quaternion_components(slot, x, y, z, w);
        }
        if let Some(slot) = &mut self.out_weight {
            set_quaternion_components(slot, x, y, z, w);
        }
    }
}

/// `SetValues(version, time, value, weight)` with a quaternion weight.
pub fn set_values_weight_quat(
    keyframe: &mut KeyframeQuaternionView,
    version: UnityVersion,
    time: f32,
    value: Quat,
    weight: Quat,
) {
    keyframe.apply_base(version, time, value, FREE_SMOOTH);
    keyframe.set_weight_quat(weight);
}

/// `SetValues(version, time, value, weight)` with a scalar weight.
pub fn set_values_weight_scalar(
    keyframe: &mut KeyframeQuaternionView,
    version: UnityVersion,
    time: f32,
    value: Quat,
    weight: f32,
) {
    keyframe.apply_base(version, time, value, FREE_SMOOTH);
    keyframe.set_weight_scalar(weight);
}

/// `SetValues(version, time, value, weightX, weightY, weightZ, weightW)`.
#[allow(clippy::too_many_arguments)]
pub fn set_values_weight_components(
    keyframe: &mut KeyframeQuaternionView,
    version: UnityVersion,
    time: f32,
    value: Quat,
    weight_x: f32,
    weight_y: f32,
    weight_z: f32,
    weight_w: f32,
) {
    keyframe.apply_base(version, time, value, FREE_SMOOTH);
    keyframe.set_weight_components(weight_x, weight_y, weight_z, weight_w);
}

/// `SetValues(version, time, value, inSlope, outSlope, weight)` with a quaternion weight.
pub fn set_values_slopes_quat(
    keyframe: &mut KeyframeQuaternionView,
    version: UnityVersion,
    time: f32,
    value: Quat,
    in_slope: Quat,
    out_slope: Quat,
    weight: Quat,
) {
    keyframe.in_slope = in_slope;
    keyframe.out_slope = out_slope;
    keyframe.apply_base(version, time, value, FREE_FREE);
    keyframe.set_weight_quat(weight);
}

/// `SetValues(version, time, value, inSlope, outSlope, weight)` with a scalar weight.
pub fn set_values_slopes_scalar(
    keyframe: &mut KeyframeQuaternionView,
    version: UnityVersion,
    time: f32,
    value: Quat,
    in_slope: Quat,
    out_slope: Quat,
    weight: f32,
) {
    keyframe.in_slope = in_slope;
    keyframe.out_slope = out_slope;
    keyframe.apply_base(version, time, value, FREE_FREE);
    keyframe.set_weight_scalar(weight);
}

/// `SetValues(version, time, value, inSlope, outSlope, weightX, weightY, weightZ, weightW)`.
#[allow(clippy::too_many_arguments)]
pub fn set_values_slopes_components(
    keyframe: &mut KeyframeQuaternionView,
    version: UnityVersion,
    time: f32,
    value: Quat,
    in_slope: Quat,
    out_slope: Quat,
    weight_x: f32,
    weight_y: f32,
    weight_z: f32,
    weight_w: f32,
) {
    keyframe.in_slope = in_slope;
    keyframe.out_slope = out_slope;
    keyframe.apply_base(version, time, value, FREE_FREE);
    keyframe.set_weight_components(weight_x, weight_y, weight_z, weight_w);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quat(x: f32, y: f32, z: f32, w: f32) -> Quat {
        Quat { x, y, z, w }
    }

    fn assert_quat(line: &str, actual: Quat, tokens: &[&str]) {
        assert_eq!(
            actual.x.to_bits(),
            tokens[0].parse::<u32>().unwrap(),
            "{line}"
        );
        assert_eq!(
            actual.y.to_bits(),
            tokens[1].parse::<u32>().unwrap(),
            "{line}"
        );
        assert_eq!(
            actual.z.to_bits(),
            tokens[2].parse::<u32>().unwrap(),
            "{line}"
        );
        assert_eq!(
            actual.w.to_bits(),
            tokens[3].parse::<u32>().unwrap(),
            "{line}"
        );
    }

    fn assert_dump(line: &str, keyframe: &KeyframeQuaternionView, dump: &[&str]) {
        assert_eq!(
            keyframe.time.to_bits(),
            dump[0].parse::<u32>().unwrap(),
            "{line}"
        );
        assert_quat(line, keyframe.value, &dump[1..5]);
        assert_quat(line, keyframe.in_slope, &dump[5..9]);
        assert_quat(line, keyframe.out_slope, &dump[9..13]);
        match &keyframe.in_weight {
            Some(value) => assert_quat(line, *value, &dump[13..17]),
            None => assert_eq!(&dump[13..17], &["-", "-", "-", "-"], "{line}"),
        }
        match &keyframe.out_weight {
            Some(value) => assert_quat(line, *value, &dump[17..21]),
            None => assert_eq!(&dump[17..21], &["-", "-", "-", "-"], "{line}"),
        }
        assert_eq!(
            keyframe.tangent_mode,
            dump[21].parse::<i32>().unwrap(),
            "{line}"
        );
        assert_eq!(
            keyframe.weighted_mode.unwrap_or(0),
            dump[22].parse::<i32>().unwrap(),
            "{line}"
        );
    }

    /// Byte-for-byte differential against the reference `KeyframeExtensions.Quaternionf` (probe
    /// `Temp/opencode/qprobe`).
    #[test]
    fn keyframe_quaternion_set_values_matches_the_reference_runtime() {
        let fixture = include_str!("keyframe_quaternion_fixture.txt");
        let value = quat(0.1, 0.2, 0.3, 0.4);
        let weight = quat(0.5, 0.6, 0.7, 0.8);
        let in_slope = quat(1.0, 1.1, 1.2, 1.3);
        let out_slope = quat(2.0, 2.1, 2.2, 2.3);
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "Q", "{line}");
            let version = UnityVersion::parse(parts[1]).unwrap();
            let presence = keyframe_presence(version);
            assert_eq!(presence.in_weight, parts[2] == "1", "{line}");
            assert_eq!(presence.out_weight, parts[3] == "1", "{line}");
            assert_eq!(presence.weighted_mode, parts[4] == "1", "{line}");

            let mut keyframe = KeyframeQuaternionView::for_version(version);
            match parts[5] {
                "weightQuat" => set_values_weight_quat(&mut keyframe, version, 1.5, value, weight),
                "weightScalar" => {
                    set_values_weight_scalar(&mut keyframe, version, 1.5, value, 0.25)
                }
                "weightComponents" => {
                    set_values_weight_components(
                        &mut keyframe,
                        version,
                        1.5,
                        value,
                        0.1,
                        0.2,
                        0.3,
                        0.4,
                    );
                }
                "slopesQuat" => set_values_slopes_quat(
                    &mut keyframe,
                    version,
                    1.5,
                    value,
                    in_slope,
                    out_slope,
                    weight,
                ),
                "slopesScalar" => set_values_slopes_scalar(
                    &mut keyframe,
                    version,
                    1.5,
                    value,
                    in_slope,
                    out_slope,
                    0.25,
                ),
                "slopesComponents" => set_values_slopes_components(
                    &mut keyframe,
                    version,
                    1.5,
                    value,
                    in_slope,
                    out_slope,
                    0.1,
                    0.2,
                    0.3,
                    0.4,
                ),
                other => panic!("unknown quaternion keyframe kind {other}"),
            }
            let dump: Vec<&str> = parts[6].split(',').collect();
            assert_dump(line, &keyframe, &dump);
            rows += 1;
        }
        assert_eq!(rows, 48);
    }

    #[test]
    fn set_values_resets_junk_but_keeps_untouched_slopes() {
        let version = UnityVersion::parse("2019.4.3f1").unwrap();
        let mut keyframe = KeyframeQuaternionView {
            time: 9.0,
            value: quat(9.0, 9.0, 9.0, 9.0),
            in_slope: quat(4.0, 5.0, 6.0, 7.0),
            out_slope: quat(8.0, 9.0, 10.0, 11.0),
            in_weight: Some(quat(9.0, 9.0, 9.0, 9.0)),
            out_weight: Some(quat(9.0, 9.0, 9.0, 9.0)),
            tangent_mode: 9,
            weighted_mode: Some(9),
        };
        let value = quat(0.1, 0.2, 0.3, 0.4);
        let weight = quat(0.5, 0.6, 0.7, 0.8);
        set_values_weight_quat(&mut keyframe, version, 1.5, value, weight);
        // The weight-only overload rewrites its fields exactly; the slopes it
        // does not own survive, like the source.
        assert_eq!(
            keyframe,
            KeyframeQuaternionView {
                time: 1.5,
                value,
                in_slope: quat(4.0, 5.0, 6.0, 7.0),
                out_slope: quat(8.0, 9.0, 10.0, 11.0),
                in_weight: Some(weight),
                out_weight: Some(weight),
                tangent_mode: 0,
                weighted_mode: Some(0),
            }
        );
        // The scalar overload broadcasts one weight to all four lanes.
        set_values_weight_scalar(&mut keyframe, version, 2.5, value, 0.25);
        assert_eq!(keyframe.in_weight, Some(quat(0.25, 0.25, 0.25, 0.25)));
        assert_eq!(keyframe.out_weight, Some(quat(0.25, 0.25, 0.25, 0.25)));
        // The slopes overload writes its own slopes and still resets the rest.
        set_values_slopes_quat(
            &mut keyframe,
            version,
            3.5,
            value,
            quat(1.0, 1.1, 1.2, 1.3),
            quat(2.0, 2.1, 2.2, 2.3),
            weight,
        );
        assert_eq!(keyframe.time.to_bits(), 3.5f32.to_bits());
        assert_eq!(keyframe.in_slope, quat(1.0, 1.1, 1.2, 1.3));
        assert_eq!(keyframe.out_slope, quat(2.0, 2.1, 2.2, 2.3));
        assert_eq!(keyframe.tangent_mode, 1);
        assert_eq!(keyframe.weighted_mode, Some(0));
    }
}
