//! `KeyframeExtensions` float-keyframe defaults.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/KeyframeExtensions.Float.cs`. The optional
//! `InWeight`/`OutWeight`/`WeightedMode` fields are only serialized from Unity 2018.1, so the view
//! models them as `Option` slots and the setters write only when the field exists.

use super::tangent_mode::{to_tangent, FREE_FREE, FREE_SMOOTH};
use super::vertex_data_blob::UnityVersion;

/// `WeightedMode.None`.
pub const WEIGHTED_MODE_NONE: i32 = 0;

/// `KeyframeExtensions.DefaultFloatWeight`.
pub const DEFAULT_FLOAT_WEIGHT: f32 = (1.0f64 / 3.0) as f32;

/// Which optional `Keyframe_Single` fields a version serializes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct KeyframePresence {
    /// `Has_InWeight`.
    pub in_weight: bool,
    /// `Has_OutWeight`.
    pub out_weight: bool,
    /// `Has_WeightedMode`.
    pub weighted_mode: bool,
}

/// The version gate for the optional keyframe fields: Unity 2018.1 and later.
pub fn keyframe_presence(version: UnityVersion) -> KeyframePresence {
    let present = version.at_least_at(2018, 1);
    KeyframePresence {
        in_weight: present,
        out_weight: present,
        weighted_mode: present,
    }
}

/// A keyframe view with the optional fields as slots.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct KeyframeView {
    /// `Time`.
    pub time: f32,
    /// `Value`.
    pub value: f32,
    /// `InSlope`.
    pub in_slope: f32,
    /// `OutSlope`.
    pub out_slope: f32,
    /// `InWeight`.
    pub in_weight: Option<f32>,
    /// `OutWeight`.
    pub out_weight: Option<f32>,
    /// `TangentMode`.
    pub tangent_mode: i32,
    /// `WeightedMode`.
    pub weighted_mode: Option<i32>,
}

impl KeyframeView {
    fn apply_common(
        &mut self,
        version: UnityVersion,
        time: f32,
        value: f32,
        tangent: i32,
        weight: f32,
    ) {
        self.time = time;
        self.value = value;
        self.tangent_mode = to_tangent(tangent, version);
        if let Some(slot) = &mut self.weighted_mode {
            *slot = WEIGHTED_MODE_NONE;
        }
        if let Some(slot) = &mut self.in_weight {
            *slot = weight;
        }
        if let Some(slot) = &mut self.out_weight {
            *slot = weight;
        }
    }
}

/// `KeyframeExtensions.SetValues(keyframe, version, time, value, weight)`.
pub fn set_values_weight(
    keyframe: &mut KeyframeView,
    version: UnityVersion,
    time: f32,
    value: f32,
    weight: f32,
) {
    keyframe.apply_common(version, time, value, FREE_SMOOTH, weight);
}

/// `KeyframeExtensions.SetValues(keyframe, version, time, value, inSlope, outSlope, weight)`.
#[allow(clippy::too_many_arguments)]
pub fn set_values_slopes(
    keyframe: &mut KeyframeView,
    version: UnityVersion,
    time: f32,
    value: f32,
    in_slope: f32,
    out_slope: f32,
    weight: f32,
) {
    keyframe.in_slope = in_slope;
    keyframe.out_slope = out_slope;
    keyframe.apply_common(version, time, value, FREE_FREE, weight);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view_for(presence: KeyframePresence) -> KeyframeView {
        KeyframeView {
            in_weight: presence.in_weight.then_some(0.0),
            out_weight: presence.out_weight.then_some(0.0),
            weighted_mode: presence.weighted_mode.then_some(0),
            ..KeyframeView::default()
        }
    }

    fn bits(value: Option<f32>) -> u32 {
        value.map(f32::to_bits).unwrap_or(0)
    }

    /// Byte-for-byte differential against the reference `KeyframeExtensions.Float` (probe
    /// `Temp/opencode/kfprobe`).
    #[test]
    fn keyframe_set_values_matches_the_reference_runtime() {
        let fixture = include_str!("keyframe_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "KF", "{line}");
            let version = UnityVersion::parse(parts[1]).unwrap();
            let presence = KeyframePresence {
                in_weight: parts[2] == "1",
                out_weight: parts[3] == "1",
                weighted_mode: parts[4] == "1",
            };
            assert_eq!(keyframe_presence(version), presence, "{line}");

            let weighted: Vec<&str> = parts[5].split(',').collect();
            let mut keyframe = view_for(presence);
            set_values_weight(&mut keyframe, version, 1.5, 2.5, 0.25);
            assert_keyframe(line, &keyframe, &weighted);

            let sloped: Vec<&str> = parts[6].split(',').collect();
            let mut keyframe = view_for(presence);
            set_values_slopes(&mut keyframe, version, 1.5, 2.5, 3.5, 4.5, 0.75);
            assert_keyframe(line, &keyframe, &sloped);
            rows += 1;
        }
        assert_eq!(rows, 11);
    }

    fn assert_keyframe(line: &str, keyframe: &KeyframeView, expected: &[&str]) {
        assert_eq!(
            keyframe.time.to_bits(),
            expected[0].parse::<u32>().unwrap(),
            "{line}"
        );
        assert_eq!(
            keyframe.value.to_bits(),
            expected[1].parse::<u32>().unwrap(),
            "{line}"
        );
        assert_eq!(
            keyframe.in_slope.to_bits(),
            expected[2].parse::<u32>().unwrap(),
            "{line}"
        );
        assert_eq!(
            keyframe.out_slope.to_bits(),
            expected[3].parse::<u32>().unwrap(),
            "{line}"
        );
        assert_eq!(
            bits(keyframe.in_weight),
            expected[4].parse::<u32>().unwrap(),
            "{line}"
        );
        assert_eq!(
            bits(keyframe.out_weight),
            expected[5].parse::<u32>().unwrap(),
            "{line}"
        );
        assert_eq!(
            keyframe.tangent_mode,
            expected[6].parse::<i32>().unwrap(),
            "{line}"
        );
        assert_eq!(
            keyframe.weighted_mode.unwrap_or(0),
            expected[7].parse::<i32>().unwrap(),
            "{line}"
        );
    }

    #[test]
    fn default_weight_bits_and_overload_shapes_match_the_source() {
        // `DefaultFloatWeight` is the double division narrowed to `f32`, not the
        // single-precision division: both round to the same word today, and the pin
        // keeps the source expression honest.
        assert_eq!(DEFAULT_FLOAT_WEIGHT.to_bits(), 0x3EAAAAAB);
        assert_eq!(DEFAULT_FLOAT_WEIGHT.to_bits(), (1.0f32 / 3.0).to_bits());

        let version = UnityVersion::parse("2019.4.3f1").unwrap();
        // The weight-only overload never touches the slopes, exactly like the source.
        let mut weighted = KeyframeView {
            in_slope: 11.0,
            out_slope: 22.0,
            ..view_for(keyframe_presence(version))
        };
        set_values_weight(&mut weighted, version, 1.0, 2.0, 0.5);
        assert_eq!(weighted.in_slope.to_bits(), 11.0f32.to_bits());
        assert_eq!(weighted.out_slope.to_bits(), 22.0f32.to_bits());
        assert_eq!(weighted.in_weight.map(f32::to_bits), Some(0.5f32.to_bits()));
        // The slope overload writes the slopes through the free-free tangent.
        let mut sloped = view_for(keyframe_presence(version));
        set_values_slopes(&mut sloped, version, 1.0, 2.0, 3.0, 4.0, 0.75);
        assert_eq!(sloped.in_slope.to_bits(), 3.0f32.to_bits());
        assert_eq!(sloped.out_slope.to_bits(), 4.0f32.to_bits());
        assert_eq!(sloped.tangent_mode, to_tangent(FREE_FREE, version));
    }

    #[test]
    fn presence_gate_and_weighted_none_match_the_source() {
        // The optional fields appear exactly at Unity 2018.1.
        let before = UnityVersion::parse("2017.4.0f1").unwrap();
        assert_eq!(keyframe_presence(before), KeyframePresence::default());
        let at = UnityVersion::parse("2018.1.0f1").unwrap();
        assert_eq!(
            keyframe_presence(at),
            KeyframePresence {
                in_weight: true,
                out_weight: true,
                weighted_mode: true,
            }
        );
        // `WeightedMode.None` is the zero discriminant the setters write.
        assert_eq!(WEIGHTED_MODE_NONE, 0);
    }
}
