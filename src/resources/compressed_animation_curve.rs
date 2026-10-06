//! `CompressedAnimationCurveExtensions.Unpack`.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/CompressedAnimationCurveExtensions.cs`.
//! A compressed quaternion curve stores times as packed hundredths, rotations as packed
//! quaternions, and slopes as a packed float array with a four-float stride; the unpacker rebuilds
//! the destination quaternion curve's keyframes and copies the loop settings. The slope stride
//! decision lives in a dedicated kernel; short time/slope arrays are explicit errors where the
//! source indexes past the end.

use crate::numerics::Quat;

use super::animation_curve::{curve_presence, CURVE_LOOP_CYCLE_WITH_OFFSET, ROTATION_ORDER_ZXY};
use super::keyframe_quaternion::{set_values_slopes_scalar, KeyframeQuaternionView};
use super::packed_bit_vector::{PackedBitVector, PackedBitVectorError, PackedBitVectorLimits};
use super::packed_float_vector::{PackedBitVectorSingle, PackedFloatVectorError};
use super::packed_quat_vector::{PackedBitVectorQuaternion, PackedQuatVectorError};
use super::vertex_data_blob::UnityVersion;

/// The compressed curve fields needed for unpacking.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CompressedAnimationCurveView {
    /// `Path`.
    pub path: String,
    /// `PreInfinity`.
    pub pre_infinity: i32,
    /// `PostInfinity`.
    pub post_infinity: i32,
    /// `Times`.
    pub times: PackedBitVector,
    /// `Values`.
    pub values: PackedBitVectorQuaternion,
    /// `Slopes`.
    pub slopes: PackedBitVectorSingle,
}

/// A quaternion curve (the unpack destination).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QuaternionCurveView {
    /// `Path`.
    pub path: String,
    /// `PreInfinity`.
    pub pre_infinity: i32,
    /// `PostInfinity`.
    pub post_infinity: i32,
    /// `RotationOrder`.
    pub rotation_order: Option<i32>,
    /// `Curve`.
    pub curve: Vec<KeyframeQuaternionView>,
}

impl QuaternionCurveView {
    /// Build a view whose optional slots match the version gate.
    pub fn for_version(version: UnityVersion) -> Self {
        Self {
            rotation_order: curve_presence(version).rotation_order.then_some(0),
            ..Self::default()
        }
    }
}

/// A failure while unpacking a compressed animation curve.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CompressedAnimationCurveError {
    /// The packed time vector failed.
    Times(PackedBitVectorError),
    /// The packed rotation vector failed.
    Values(PackedQuatVectorError),
    /// The packed slope vector failed.
    Slopes(PackedFloatVectorError),
    /// The times array was shorter than the rotation count.
    TruncatedTimes {
        /// The rotations requested.
        needed: usize,
        /// The times available.
        available: usize,
    },
    /// The slopes array was too short for one keyframe.
    TruncatedSlopes {
        /// The slopes requested.
        needed: usize,
        /// The slopes available.
        available: usize,
    },
}

impl core::fmt::Display for CompressedAnimationCurveError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Times(source) => write!(formatter, "compressed time vector: {source}"),
            Self::Values(source) => write!(formatter, "compressed rotation vector: {source}"),
            Self::Slopes(source) => write!(formatter, "compressed slope vector: {source}"),
            Self::TruncatedTimes { needed, available } => write!(
                formatter,
                "compressed curve needs {needed} times but only {available} are available"
            ),
            Self::TruncatedSlopes { needed, available } => write!(
                formatter,
                "compressed curve needs {needed} slopes but only {available} are available"
            ),
        }
    }
}

impl std::error::Error for CompressedAnimationCurveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Times(source) => Some(source),
            Self::Values(source) => Some(source),
            Self::Slopes(source) => Some(source),
            _ => None,
        }
    }
}

impl From<PackedBitVectorError> for CompressedAnimationCurveError {
    fn from(value: PackedBitVectorError) -> Self {
        Self::Times(value)
    }
}

impl From<PackedQuatVectorError> for CompressedAnimationCurveError {
    fn from(value: PackedQuatVectorError) -> Self {
        Self::Values(value)
    }
}

impl From<PackedFloatVectorError> for CompressedAnimationCurveError {
    fn from(value: PackedFloatVectorError) -> Self {
        Self::Slopes(value)
    }
}

/// Slope offsets for one keyframe.
///
/// The packed array strides four floats per keyframe with a four-float lead, so
/// keyframe `i` reads its in-slope at `i * 4` and its out-slope at `i * 4 + 4`
/// (the source's `slopes[j - 4 .. j + 4]` with `j = 4 + i * 4`). Short arrays are
/// an explicit [`CompressedAnimationCurveError::TruncatedSlopes`].
fn slope_window(
    keyframe_index: usize,
    slope_count: usize,
) -> Result<(usize, usize), CompressedAnimationCurveError> {
    let in_start =
        keyframe_index
            .checked_mul(4)
            .ok_or(CompressedAnimationCurveError::TruncatedSlopes {
                needed: usize::MAX,
                available: slope_count,
            })?;
    let needed = in_start
        .checked_add(8)
        .ok_or(CompressedAnimationCurveError::TruncatedSlopes {
            needed: usize::MAX,
            available: slope_count,
        })?;
    if needed > slope_count {
        return Err(CompressedAnimationCurveError::TruncatedSlopes {
            needed,
            available: slope_count,
        });
    }
    Ok((in_start, in_start + 4))
}

/// `CompressedAnimationCurveExtensions.Unpack`.
pub fn unpack(
    compressed: &CompressedAnimationCurveView,
    version: UnityVersion,
    destination: &mut QuaternionCurveView,
    limits: PackedBitVectorLimits,
) -> Result<(), CompressedAnimationCurveError> {
    let times_values = compressed.times.unpack_ints(limits)?;
    let times: Vec<f32> = times_values
        .iter()
        .map(|value| *value as f32 * 0.01)
        .collect();
    let rotations = compressed.values.unpack(limits)?;
    let slopes = compressed.slopes.unpack(limits)?;

    destination.curve.clear();
    for (i, rotation) in rotations.iter().enumerate() {
        let time = *times
            .get(i)
            .ok_or(CompressedAnimationCurveError::TruncatedTimes {
                needed: rotations.len(),
                available: times.len(),
            })?;
        let (in_start, out_start) = slope_window(i, slopes.len())?;
        let value = Quat {
            x: rotation.x,
            y: rotation.y,
            z: rotation.z,
            w: rotation.w,
        };
        let in_slope = Quat {
            x: slopes[in_start],
            y: slopes[in_start + 1],
            z: slopes[in_start + 2],
            w: slopes[in_start + 3],
        };
        let out_slope = Quat {
            x: slopes[out_start],
            y: slopes[out_start + 1],
            z: slopes[out_start + 2],
            w: slopes[out_start + 3],
        };
        let mut keyframe = KeyframeQuaternionView::for_version(version);
        set_values_slopes_scalar(
            &mut keyframe,
            version,
            time,
            value,
            in_slope,
            out_slope,
            1.0 / 3.0,
        );
        destination.curve.push(keyframe);
    }

    destination.path = compressed.path.clone();
    destination.pre_infinity = CURVE_LOOP_CYCLE_WITH_OFFSET;
    destination.post_infinity = CURVE_LOOP_CYCLE_WITH_OFFSET;
    if let Some(slot) = &mut destination.rotation_order {
        *slot = ROTATION_ORDER_ZXY;
    }
    destination.pre_infinity = compressed.pre_infinity;
    destination.post_infinity = compressed.post_infinity;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::packed_quat_vector::Quaternion as PackedQuaternion;
    use super::*;

    fn build_compressed() -> CompressedAnimationCurveView {
        let limits = PackedBitVectorLimits::default();
        let mut times = PackedBitVector::default();
        times.pack_ints(&[0, 50, 100, 150], limits).unwrap();
        let mut values = PackedBitVectorQuaternion::default();
        values
            .pack(
                &[
                    PackedQuaternion {
                        x: 0.1,
                        y: 0.2,
                        z: 0.3,
                        w: 0.4,
                    },
                    PackedQuaternion {
                        x: -0.4,
                        y: 0.3,
                        z: 0.2,
                        w: 0.1,
                    },
                    PackedQuaternion {
                        x: 0.5,
                        y: -0.5,
                        z: 0.5,
                        w: 0.5,
                    },
                    PackedQuaternion {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                        w: 1.0,
                    },
                ],
                limits,
            )
            .unwrap();
        let mut slopes = PackedBitVectorSingle::default();
        let slope_data: Vec<f32> = (0..20).map(|i| i as f32 * 0.1).collect();
        slopes.pack(&slope_data, limits).unwrap();
        CompressedAnimationCurveView {
            path: "Root/Bone".to_string(),
            pre_infinity: 5,
            post_infinity: 7,
            times,
            values,
            slopes,
        }
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

    /// Byte-for-byte differential against the reference `CompressedAnimationCurveExtensions`
    /// (probe `Temp/opencode/cacprobe`).
    #[test]
    fn compressed_animation_curve_unpack_matches_the_reference_runtime() {
        let fixture = include_str!("compressed_animation_curve_fixture.txt");
        let compressed = build_compressed();
        let limits = PackedBitVectorLimits::default();
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "CAC", "{line}");
            let version = UnityVersion::parse(parts[1]).unwrap();
            let rotation_order = parts[2] == "1";
            assert_eq!(
                curve_presence(version).rotation_order,
                rotation_order,
                "{line}"
            );
            let mut destination = QuaternionCurveView::for_version(version);
            unpack(&compressed, version, &mut destination, limits).unwrap();

            assert_eq!(destination.path, parts[3], "{line}");
            assert_eq!(
                destination.pre_infinity,
                parts[4].parse::<i32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                destination.post_infinity,
                parts[5].parse::<i32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                destination.rotation_order.unwrap_or(0),
                parts[6].parse::<i32>().unwrap(),
                "{line}"
            );

            let expected: Vec<&str> = parts[7].split(';').collect();
            assert_eq!(destination.curve.len(), expected.len(), "{line}");
            for (keyframe, row) in destination.curve.iter().zip(expected) {
                let values: Vec<&str> = row.split(',').collect();
                assert_eq!(
                    keyframe.time.to_bits(),
                    values[0].parse::<u32>().unwrap(),
                    "{line}"
                );
                assert_quat(line, keyframe.value, &values[1..5]);
                assert_quat(line, keyframe.in_slope, &values[5..9]);
                assert_quat(line, keyframe.out_slope, &values[9..13]);
                match &keyframe.in_weight {
                    Some(value) => assert_quat(line, *value, &values[13..17]),
                    None => assert_eq!(&values[13..17], &["-", "-", "-", "-"], "{line}"),
                }
                match &keyframe.out_weight {
                    Some(value) => assert_quat(line, *value, &values[17..21]),
                    None => assert_eq!(&values[17..21], &["-", "-", "-", "-"], "{line}"),
                }
                assert_eq!(
                    keyframe.tangent_mode,
                    values[21].parse::<i32>().unwrap(),
                    "{line}"
                );
                assert_eq!(
                    keyframe.weighted_mode.unwrap_or(0),
                    values[22].parse::<i32>().unwrap(),
                    "{line}"
                );
            }
            rows += 1;
        }
        assert_eq!(rows, 4);
    }

    fn build_compressed_with(
        time_values: &[i32],
        slope_values: &[f32],
    ) -> CompressedAnimationCurveView {
        let limits = PackedBitVectorLimits::default();
        let mut compressed = build_compressed();
        let mut times = PackedBitVector::default();
        times.pack_ints(time_values, limits).unwrap();
        compressed.times = times;
        let mut slopes = PackedBitVectorSingle::default();
        slopes.pack(slope_values, limits).unwrap();
        compressed.slopes = slopes;
        compressed
    }

    #[test]
    fn slope_window_follows_the_four_float_stride() {
        assert_eq!(slope_window(0, 8).unwrap(), (0, 4));
        assert_eq!(slope_window(1, 12).unwrap(), (4, 8));
        assert_eq!(slope_window(3, 20).unwrap(), (12, 16));
        assert_eq!(
            slope_window(0, 7).unwrap_err(),
            CompressedAnimationCurveError::TruncatedSlopes {
                needed: 8,
                available: 7,
            }
        );
        assert_eq!(
            slope_window(2, 15).unwrap_err(),
            CompressedAnimationCurveError::TruncatedSlopes {
                needed: 16,
                available: 15,
            }
        );
        assert!(matches!(
            slope_window(usize::MAX / 4 + 1, 0).unwrap_err(),
            CompressedAnimationCurveError::TruncatedSlopes {
                needed: usize::MAX,
                ..
            }
        ));
    }

    #[test]
    fn unpack_reports_short_time_and_slope_arrays() {
        let limits = PackedBitVectorLimits::default();
        let version = UnityVersion::parse("2019.4.0f1").unwrap();
        // Four rotations but a single time.
        let compressed =
            build_compressed_with(&[0], &(0..20).map(|i| i as f32 * 0.1).collect::<Vec<_>>());
        let mut destination = QuaternionCurveView::for_version(version);
        assert_eq!(
            unpack(&compressed, version, &mut destination, limits).unwrap_err(),
            CompressedAnimationCurveError::TruncatedTimes {
                needed: 4,
                available: 1,
            }
        );
        // Matching times but only two keyframes of slopes.
        let compressed = build_compressed_with(
            &[0, 50, 100, 150],
            &(0..8).map(|i| i as f32 * 0.1).collect::<Vec<_>>(),
        );
        let mut destination = QuaternionCurveView::for_version(version);
        assert_eq!(
            unpack(&compressed, version, &mut destination, limits).unwrap_err(),
            CompressedAnimationCurveError::TruncatedSlopes {
                needed: 12,
                available: 8,
            }
        );
    }

    #[test]
    fn unpack_accepts_an_empty_curve() {
        let limits = PackedBitVectorLimits::default();
        let version = UnityVersion::parse("5.0.0f1").unwrap();
        let compressed = CompressedAnimationCurveView {
            path: "Root/Bone".to_string(),
            pre_infinity: 5,
            post_infinity: 7,
            ..CompressedAnimationCurveView::default()
        };
        let mut destination = QuaternionCurveView::for_version(version);
        unpack(&compressed, version, &mut destination, limits).unwrap();
        assert!(destination.curve.is_empty());
        assert_eq!(destination.path, "Root/Bone");
        assert_eq!(destination.pre_infinity, 5);
        assert_eq!(destination.post_infinity, 7);
    }
}
