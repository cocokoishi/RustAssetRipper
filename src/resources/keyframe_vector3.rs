//! `KeyframeExtensions` Vector3f-keyframe defaults.
//!
//! Mirrors the `IKeyframe_Vector3f` overloads of
//! `AssetRipper.SourceGenerated.Extensions/KeyframeExtensions.Vector3f.cs`. The `InWeight`/
//! `OutWeight`/`WeightedMode` fields are only serialized from Unity 2018.1, matching the float
//! keyframe gate.

use crate::numerics::Vec3;

use super::keyframe::keyframe_presence;
use super::tangent_mode::{to_tangent, FREE_FREE, FREE_SMOOTH};
use super::vector_extensions::copy_values;
use super::vertex_data_blob::UnityVersion;

/// A Vector3f keyframe view with the optional fields as slots.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KeyframeVector3View {
    /// `Time`.
    pub time: f32,
    /// `Value`.
    pub value: Vec3,
    /// `InSlope`.
    pub in_slope: Vec3,
    /// `OutSlope`.
    pub out_slope: Vec3,
    /// `InWeight`.
    pub in_weight: Option<Vec3>,
    /// `OutWeight`.
    pub out_weight: Option<Vec3>,
    /// `TangentMode`.
    pub tangent_mode: i32,
    /// `WeightedMode`.
    pub weighted_mode: Option<i32>,
}

impl KeyframeVector3View {
    /// Build a view whose optional slots match the version gate.
    pub fn for_version(version: UnityVersion) -> Self {
        let presence = keyframe_presence(version);
        Self {
            in_weight: presence.in_weight.then_some(Vec3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }),
            out_weight: presence.out_weight.then_some(Vec3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }),
            weighted_mode: presence.weighted_mode.then_some(0),
            ..Self::default()
        }
    }

    fn apply_common(
        &mut self,
        version: UnityVersion,
        time: f32,
        value: Vec3,
        tangent: i32,
        weight: Vec3,
    ) {
        self.time = time;
        copy_values(&mut self.value, value);
        self.tangent_mode = to_tangent(tangent, version);
        if let Some(slot) = &mut self.weighted_mode {
            *slot = 0;
        }
        if let Some(slot) = &mut self.in_weight {
            copy_values(slot, weight);
        }
        if let Some(slot) = &mut self.out_weight {
            copy_values(slot, weight);
        }
    }
}

/// `KeyframeExtensions.SetValues(keyframe, version, time, value, weight)`.
pub fn set_values_weight(
    keyframe: &mut KeyframeVector3View,
    version: UnityVersion,
    time: f32,
    value: Vec3,
    weight: Vec3,
) {
    keyframe.apply_common(version, time, value, FREE_SMOOTH, weight);
}

/// `KeyframeExtensions.SetValues(keyframe, version, time, value, inSlope, outSlope, weight)`.
pub fn set_values_slopes(
    keyframe: &mut KeyframeVector3View,
    version: UnityVersion,
    time: f32,
    value: Vec3,
    in_slope: Vec3,
    out_slope: Vec3,
    weight: Vec3,
) {
    keyframe.in_slope = in_slope;
    keyframe.out_slope = out_slope;
    keyframe.apply_common(version, time, value, FREE_FREE, weight);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vec3(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3 { x, y, z }
    }

    fn assert_vec(line: &str, actual: Vec3, tokens: &[&str]) {
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
    }

    fn assert_dump(line: &str, keyframe: &KeyframeVector3View, dump: &[&str]) {
        assert_eq!(
            keyframe.time.to_bits(),
            dump[0].parse::<u32>().unwrap(),
            "{line}"
        );
        assert_vec(line, keyframe.value, &dump[1..4]);
        assert_vec(line, keyframe.in_slope, &dump[4..7]);
        assert_vec(line, keyframe.out_slope, &dump[7..10]);
        match &keyframe.in_weight {
            Some(value) => assert_vec(line, *value, &dump[10..13]),
            None => assert_eq!(&dump[10..13], &["-", "-", "-"], "{line}"),
        }
        match &keyframe.out_weight {
            Some(value) => assert_vec(line, *value, &dump[13..16]),
            None => assert_eq!(&dump[13..16], &["-", "-", "-"], "{line}"),
        }
        assert_eq!(
            keyframe.tangent_mode,
            dump[16].parse::<i32>().unwrap(),
            "{line}"
        );
        assert_eq!(
            keyframe.weighted_mode.unwrap_or(0),
            dump[17].parse::<i32>().unwrap(),
            "{line}"
        );
    }

    /// Byte-for-byte differential against the reference `KeyframeExtensions.Vector3f` (probe
    /// `Temp/opencode/v3probe`).
    #[test]
    fn keyframe_vector3_set_values_matches_the_reference_runtime() {
        let fixture = include_str!("keyframe_vector3_fixture.txt");
        let value = vec3(1.0, 2.0, 3.0);
        let weight = vec3(0.1, 0.2, 0.3);
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if !line.starts_with("V3|") {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            let version = UnityVersion::parse(parts[1]).unwrap();
            let presence = keyframe_presence(version);
            assert_eq!(presence.in_weight, parts[2] == "1", "{line}");
            assert_eq!(presence.out_weight, parts[3] == "1", "{line}");
            assert_eq!(presence.weighted_mode, parts[4] == "1", "{line}");

            let weight_dump: Vec<&str> = parts[5].split(',').collect();
            let mut keyframe = KeyframeVector3View::for_version(version);
            set_values_weight(&mut keyframe, version, 1.5, value, weight);
            assert_dump(line, &keyframe, &weight_dump);

            let slope_dump: Vec<&str> = parts[6].split(',').collect();
            let mut keyframe = KeyframeVector3View::for_version(version);
            set_values_slopes(
                &mut keyframe,
                version,
                1.5,
                value,
                vec3(4.0, 5.0, 6.0),
                vec3(7.0, 8.0, 9.0),
                weight,
            );
            assert_dump(line, &keyframe, &slope_dump);
            rows += 1;
        }
        assert_eq!(rows, 8);
    }
}
