//! Streamed animation-curve keys and frames.
//!
//! Ports `AssetRipper.Processing/AnimationClips/Editor/StreamedCurveKey.cs` and
//! `StreamedFrame.cs`: key reads (index, three cubic coefficients, the right-sided
//! limit with the left limit initialized equal and exact), the slope calculation with
//! its exact double-precision arithmetic, and frame reads (time, curve count, the
//! consecutive-duplicate collapse keeping the last curve, and the 2017.1+ four-byte
//! alignment).
//!
//! Two deliberate differences from the reference, both explicit errors instead of
//! panics or unbounded allocation: truncated buffers fail with the byte offset, and
//! a curve count that cannot fit in the remaining buffer fails instead of allocating.
//! Each curve occupies exactly 20 bytes, so the bound is derived from the input, not
//! arbitrary.
//!
//! Differential fixture: `streamed_curve_fixture.txt` (probe `Temp/opencode/sclipprobe`,
//! real `StreamedCurveKey` instances calling `CalculateSlopes` and real
//! `StreamedFrame.Read` over crafted buffers).

use std::error::Error;
use std::fmt::{self, Display};

use super::vertex_data_blob::{EndianType, UnityVersion};

/// Bytes occupied by one serialized streamed-curve key: index plus four floats.
pub const STREAMED_CURVE_KEY_BYTES: usize = 20;

/// The arbitrary small value of `StreamedCurveKey.HasDifferentLimits`, converted to
/// double exactly like the source implicit conversion.
pub const ROUNDING_ERROR_THRESHOLD: f64 = 5e-7f32 as f64;

/// A streamed animation-curve decoding failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StreamedCurveError {
    /// The buffer ends before the value is complete.
    Truncated {
        /// The byte offset of the failed read.
        offset: usize,
        /// How many bytes the read needed.
        needed: usize,
        /// How many bytes the buffer holds.
        available: usize,
    },
    /// The frame declares a negative curve count.
    NegativeCurveCount {
        /// The declared count.
        count: i32,
    },
}

impl Display for StreamedCurveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated {
                offset,
                needed,
                available,
            } => write!(
                formatter,
                "streamed clip ends at byte {offset}: needed {needed} bytes but only {available} are available"
            ),
            Self::NegativeCurveCount { count } => {
                write!(formatter, "streamed frame declares a negative curve count {count}")
            }
        }
    }
}

impl Error for StreamedCurveError {}

/// One decoded streamed-curve key: `StreamedCurveKey`.
#[derive(Clone, Debug, PartialEq)]
pub struct StreamedCurveKey {
    /// Index into the clip binding constant.
    pub index: i32,
    /// Cubic coefficients between this key and the next.
    pub coefficient: [f32; 3],
    /// Value approached from the right (`RightSidedLimit`, also `Value`).
    pub right_sided_limit: f32,
    /// Value approached from the left; arithmetic estimates mark it inexact.
    pub left_sided_limit: f64,
    /// Whether the left limit came from the buffer rather than arithmetic.
    pub exact_left_sided_limit: bool,
    /// `InSlope`.
    pub in_slope: f32,
    /// `OutSlope`.
    pub out_slope: f32,
}

impl StreamedCurveKey {
    /// Reads one key at `offset`: index, three coefficients, right-sided limit.
    /// Returns the key and the offset just past it.
    pub fn read(
        input: &[u8],
        endian: EndianType,
        offset: usize,
    ) -> Result<(Self, usize), StreamedCurveError> {
        let mut cursor = offset;
        let index = read_i32(input, endian, &mut cursor)?;
        let mut coefficient = [0.0f32; 3];
        for slot in &mut coefficient {
            *slot = read_f32(input, endian, &mut cursor)?;
        }
        let right_sided_limit = read_f32(input, endian, &mut cursor)?;
        Ok((
            Self {
                index,
                coefficient,
                right_sided_limit,
                left_sided_limit: f64::from(right_sided_limit),
                exact_left_sided_limit: true,
                in_slope: 0.0,
                out_slope: 0.0,
            },
            cursor,
        ))
    }

    /// `StreamedCurveKey.Value`.
    pub const fn value(&self) -> f32 {
        self.right_sided_limit
    }

    /// `StreamedCurveKey.CalculateSlopes`.
    ///
    /// All intermediate arithmetic is double precision with single-precision casts at
    /// exactly the source positions, so results match bit-for-bit on finite inputs.
    /// The zero-coefficient test uses `==` like `Vector3 == default` (`-0.0` counts
    /// as zero, `NaN` as nonzero).
    pub fn calculate_slopes(
        &mut self,
        time: f32,
        next_time: f32,
        next: &mut Self,
        use_negative_infinity_slopes: bool,
    ) {
        if !(self.coefficient[0] == 0.0 && self.coefficient[1] == 0.0 && self.coefficient[2] == 0.0)
        {
            let delta = f64::from(next_time) - f64::from(time);
            let coeff_x = f64::from(self.coefficient[0]) * delta * delta;
            let coeff_y = f64::from(self.coefficient[1]) * delta;
            self.out_slope = self.coefficient[2];
            next.in_slope = (3.0 * coeff_x + 2.0 * coeff_y + f64::from(self.coefficient[2])) as f32;
            if use_negative_infinity_slopes
                && next.coefficient[0] == 0.0
                && next.coefficient[1] == 0.0
                && next.coefficient[2] == 0.0
            {
                next.left_sided_limit = (coeff_x + coeff_y + f64::from(self.coefficient[2]))
                    * delta
                    + f64::from(self.right_sided_limit);
                next.exact_left_sided_limit = false;
            }
            return;
        }
        if !use_negative_infinity_slopes {
            if self.right_sided_limit != next.right_sided_limit {
                self.out_slope = f32::INFINITY;
            }
            return;
        }
        let is_discontinuous = has_different_limits(self);
        if self.right_sided_limit == next.right_sided_limit {
            set_negative_infinity_slope(self, next, is_discontinuous);
        } else if is_discontinuous {
            set_negative_infinity_slope(self, next, true);
        } else {
            self.out_slope = f32::INFINITY;
        }
    }
}

/// `StreamedCurveKey.HasDifferentLimits`, with the source's exact conversions: the
/// first comparison promotes the right limit to double, and the threshold is the
/// double conversion of `5e-7f`.
fn has_different_limits(key: &StreamedCurveKey) -> bool {
    if f64::from(key.right_sided_limit) != key.left_sided_limit {
        if key.exact_left_sided_limit {
            return true;
        }
        let diff = (f64::from(key.right_sided_limit) - key.left_sided_limit).abs();
        let divisor = key
            .left_sided_limit
            .abs()
            .max(f64::from(key.right_sided_limit.abs()));
        if diff / divisor > ROUNDING_ERROR_THRESHOLD {
            return true;
        }
    }
    false
}

/// `StreamedCurveKey.SetNegInfSlope`.
fn set_negative_infinity_slope(
    key: &mut StreamedCurveKey,
    next: &mut StreamedCurveKey,
    is_discontinuous: bool,
) {
    key.out_slope = f32::NEG_INFINITY;
    if next.coefficient[0] == 0.0 && next.coefficient[1] == 0.0 && next.coefficient[2] == 0.0 {
        next.left_sided_limit = f64::from(key.right_sided_limit);
    }
    if is_discontinuous {
        key.right_sided_limit = key.left_sided_limit as f32;
    }
}

/// One decoded streamed frame: `StreamedFrame`.
#[derive(Clone, Debug, PartialEq)]
pub struct StreamedFrame {
    /// `Time`.
    pub time: f32,
    /// `Curves` after the consecutive-duplicate collapse.
    pub curves: Vec<StreamedCurveKey>,
}

impl StreamedFrame {
    /// Reads one frame at `offset`: time, curve count, keys with consecutive
    /// duplicates collapsed keeping the last, and the 2017.1+ alignment.
    /// Returns the frame and the offset just past the alignment padding.
    pub fn read(
        input: &[u8],
        endian: EndianType,
        version: UnityVersion,
        offset: usize,
    ) -> Result<(Self, usize), StreamedCurveError> {
        let mut cursor = offset;
        let time = read_f32(input, endian, &mut cursor)?;
        let count = read_i32(input, endian, &mut cursor)?;
        if count < 0 {
            return Err(StreamedCurveError::NegativeCurveCount { count });
        }
        let count = count as usize;
        let remaining = input.len().saturating_sub(cursor);
        if count > remaining / STREAMED_CURVE_KEY_BYTES {
            return Err(StreamedCurveError::Truncated {
                offset: cursor,
                needed: count * STREAMED_CURVE_KEY_BYTES,
                available: remaining,
            });
        }
        let mut curves = Vec::with_capacity(count);
        for _ in 0..count {
            let (key, next) = StreamedCurveKey::read(input, endian, cursor)?;
            cursor = next;
            if curves
                .last()
                .is_some_and(|previous: &StreamedCurveKey| previous.index == key.index)
            {
                curves.pop();
            }
            curves.push(key);
        }
        if version.at_least_at(2017, 1) {
            cursor += (4 - cursor % 4) % 4;
        }
        Ok((Self { time, curves }, cursor))
    }
}

fn read_bytes<'a>(
    input: &'a [u8],
    offset: &mut usize,
    needed: usize,
) -> Result<&'a [u8], StreamedCurveError> {
    let start = *offset;
    let end = start.saturating_add(needed);
    if end > input.len() || end < start {
        return Err(StreamedCurveError::Truncated {
            offset: start,
            needed,
            available: input.len().saturating_sub(start),
        });
    }
    *offset = end;
    Ok(&input[start..end])
}

fn read_i32(
    input: &[u8],
    endian: EndianType,
    offset: &mut usize,
) -> Result<i32, StreamedCurveError> {
    let bytes = read_bytes(input, offset, 4)?;
    let word = u32::from_le_bytes(bytes.try_into().expect("four bytes"));
    Ok(match endian {
        EndianType::Little => word as i32,
        EndianType::Big => word.swap_bytes() as i32,
    })
}

fn read_f32(
    input: &[u8],
    endian: EndianType,
    offset: &mut usize,
) -> Result<f32, StreamedCurveError> {
    let bytes = read_bytes(input, offset, 4)?;
    let word = u32::from_le_bytes(bytes.try_into().expect("four bytes"));
    Ok(match endian {
        EndianType::Little => f32::from_bits(word),
        EndianType::Big => f32::from_bits(word.swap_bytes()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex_f32(word: &str) -> f32 {
        f32::from_bits(u32::from_str_radix(word, 16).unwrap())
    }

    /// Inputs behind each `SKEY` fixture row: index, coefficients, value, time,
    /// next time, next key fields, negative-infinity support.
    #[allow(clippy::type_complexity)] // Differential tables carry input plus expected views.
    fn slope_inputs(tag: &str) -> (i32, [f32; 3], f32, f32, f32, (i32, [f32; 3], f32), bool) {
        match tag {
            "regular" => (
                3,
                [0.1, 0.2, 0.3],
                1.0,
                0.0,
                1.0,
                (3, [0.0, 0.0, 0.5], 2.0),
                false,
            ),
            "regular-neginf" => (
                3,
                [0.1, 0.2, 0.3],
                1.0,
                0.0,
                1.0,
                (5, [0.1, 0.2, 0.3], 2.0),
                true,
            ),
            "chained" => (
                5,
                [0.1, 0.2, 0.3],
                2.0,
                1.0,
                2.5,
                (5, [0.0, 0.0, 0.0], 4.0),
                true,
            ),
            "zero-equal" => (
                7,
                [0.0, 0.0, 0.0],
                1.5,
                0.0,
                2.0,
                (7, [0.0, 0.0, 0.0], 1.5),
                false,
            ),
            "zero-step" => (
                7,
                [0.0, 0.0, 0.0],
                1.0,
                0.0,
                2.0,
                (7, [0.0, 0.0, 0.0], 2.0),
                false,
            ),
            "zero-equal-neginf" => (
                7,
                [0.0, 0.0, 0.0],
                1.5,
                0.0,
                2.0,
                (7, [0.0, 0.0, 0.0], 1.5),
                true,
            ),
            "zero-step-neginf" => (
                7,
                [0.0, 0.0, 0.0],
                1.0,
                0.0,
                2.0,
                (7, [0.0, 0.0, 0.0], 2.0),
                true,
            ),
            "frac-time" => (
                9,
                [1.0, -0.5, 0.25],
                0.0,
                0.1,
                0.35,
                (9, [0.0, 0.0, 0.0], 1.0),
                false,
            ),
            _ => panic!("unexpected fixture tag {tag}"),
        }
    }

    fn make_key(index: i32, coefficient: [f32; 3], value: f32) -> StreamedCurveKey {
        StreamedCurveKey {
            index,
            coefficient,
            right_sided_limit: value,
            left_sided_limit: f64::from(value),
            exact_left_sided_limit: true,
            in_slope: 0.0,
            out_slope: 0.0,
        }
    }

    /// Byte-for-byte differential against the reference `StreamedCurveKey`
    /// (probe `Temp/opencode/sclipprobe`): real instances calling
    /// `CalculateSlopes`, with slopes, values, and next-key limits compared as
    /// exact bit words.
    #[test]
    fn slope_calculation_matches_the_reference_runtime() {
        let fixture = include_str!("streamed_curve_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split('|');
            if parts.clone().next() != Some("SKEY") {
                continue;
            }
            assert_eq!(parts.next(), Some("SKEY"), "{line}");
            let tag = parts.next().expect("a probe tag");
            let key_words: Vec<&str> = parts.next().expect("key words").split(',').collect();
            let next_words: Vec<&str> = parts.next().expect("next words").split(',').collect();
            assert!(parts.next().is_none(), "{line}");
            let (
                index,
                coefficient,
                value,
                time,
                next_time,
                (next_index, next_coeff, next_value),
                neg_inf,
            ) = slope_inputs(tag);
            // The chained row replays the arithmetic left-sided limit of the
            // regular-neginf row instead of a fresh read.
            let mut key = if tag == "chained" {
                let (_, _, _, _, _, (middle_index, middle_coeff, middle_value), _) =
                    slope_inputs("regular-neginf");
                let mut first = make_key(index, coefficient, value);
                let mut middle = make_key(middle_index, middle_coeff, middle_value);
                first.calculate_slopes(time - 1.0, time, &mut middle, true);
                middle
            } else {
                make_key(index, coefficient, value)
            };
            let mut next = make_key(next_index, next_coeff, next_value);
            key.calculate_slopes(time, next_time, &mut next, neg_inf);
            assert_eq!(
                key.in_slope.to_bits(),
                u32::from_str_radix(key_words[0], 16).unwrap(),
                "{tag} in-slope"
            );
            assert_eq!(
                key.out_slope.to_bits(),
                u32::from_str_radix(key_words[1], 16).unwrap(),
                "{tag} out-slope"
            );
            assert_eq!(
                key.value().to_bits(),
                u32::from_str_radix(key_words[2], 16).unwrap(),
                "{tag} value"
            );
            assert_eq!(
                next.in_slope.to_bits(),
                u32::from_str_radix(next_words[0], 16).unwrap(),
                "{tag} next in-slope"
            );
            assert_eq!(
                next.left_sided_limit.to_bits(),
                u64::from_str_radix(next_words[1], 16).unwrap(),
                "{tag} next limit"
            );
            assert_eq!(
                next.exact_left_sided_limit,
                next_words[2] == "1",
                "{tag} next exactness"
            );
            rows += 1;
        }
        assert_eq!(rows, 8, "all probed slope rows are asserted");
    }

    /// Frame inputs behind each `SFRAME` fixture row: time, curves, endianness,
    /// and the post-2017 alignment flag.
    #[allow(clippy::type_complexity)] // Differential tables carry input plus expected views.
    fn frame_inputs(tag: &str) -> (f32, Vec<(i32, [f32; 3], f32)>, EndianType, bool) {
        match tag {
            "dup-align" => (
                1.25,
                vec![
                    (3, [0.1, 0.2, 0.3], 1.0),
                    (3, [0.4, 0.5, 0.6], 1.5),
                    (7, [0.0, 0.0, 0.0], 2.0),
                ],
                EndianType::Little,
                true,
            ),
            "no-align" => (
                0.5,
                vec![(7, [0.0, 0.0, 0.0], 2.0)],
                EndianType::Little,
                false,
            ),
            "big-endian" => (2.0, vec![(3, [0.1, 0.2, 0.3], 1.0)], EndianType::Big, true),
            _ => panic!("unexpected fixture tag {tag}"),
        }
    }

    fn put_f32(buffer: &mut Vec<u8>, endian: EndianType, value: f32) {
        let word = value.to_bits();
        let word = match endian {
            EndianType::Little => word,
            EndianType::Big => word.swap_bytes(),
        };
        buffer.extend_from_slice(&word.to_le_bytes());
    }

    fn put_i32(buffer: &mut Vec<u8>, endian: EndianType, value: i32) {
        put_f32(buffer, endian, f32::from_bits(value as u32));
    }

    /// Differential against the reference `StreamedFrame.Read` over crafted
    /// buffers: duplicate collapse, endianness, and the alignment flag.
    #[test]
    fn frame_read_matches_the_reference_runtime() {
        let fixture = include_str!("streamed_curve_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split('|');
            if parts.clone().next() != Some("SFRAME") {
                continue;
            }
            assert_eq!(parts.next(), Some("SFRAME"), "{line}");
            let tag = parts.next().expect("a probe tag");
            let time_word = parts.next().expect("a time word");
            let count_word = parts.next().expect("a curve count");
            let (time, curves, endian, align) = frame_inputs(tag);
            let mut buffer = Vec::new();
            put_f32(&mut buffer, endian, time);
            put_i32(&mut buffer, endian, curves.len() as i32);
            for (index, coefficient, value) in &curves {
                put_i32(&mut buffer, endian, *index);
                for component in coefficient {
                    put_f32(&mut buffer, endian, *component);
                }
                put_f32(&mut buffer, endian, *value);
            }
            while buffer.len() % 4 != 0 {
                buffer.push(0xAA);
            }
            let version = UnityVersion {
                major: if align { 2019 } else { 5 },
                minor: if align { 4 } else { 0 },
                patch: 0,
            };
            let (frame, _) = StreamedFrame::read(&buffer, endian, version, 0).unwrap();
            assert_eq!(
                frame.time.to_bits(),
                u32::from_str_radix(time_word, 16).unwrap(),
                "{tag} time"
            );
            assert_eq!(
                frame.curves.len(),
                count_word.parse::<usize>().unwrap(),
                "{tag} count"
            );
            for (key, words) in frame.curves.iter().zip(parts) {
                let fields: Vec<&str> = words.split(',').collect();
                assert_eq!(key.index.to_string(), fields[0], "{tag} index");
                assert_eq!(hex_f32(&fields[1][0..8]), key.coefficient[0], "{tag} cx");
                assert_eq!(hex_f32(&fields[1][8..16]), key.coefficient[1], "{tag} cy");
                assert_eq!(hex_f32(&fields[1][16..24]), key.coefficient[2], "{tag} cz");
                assert_eq!(hex_f32(fields[2]), key.value(), "{tag} value");
                assert_eq!(
                    key.left_sided_limit,
                    f64::from(key.value()),
                    "{tag} fresh limit"
                );
                assert!(key.exact_left_sided_limit, "{tag} fresh exactness");
            }
            rows += 1;
        }
        assert_eq!(rows, 3, "all probed frame rows are asserted");
    }

    #[test]
    fn truncated_and_negative_frames_are_explicit_errors() {
        let version = UnityVersion {
            major: 2019,
            minor: 4,
            patch: 0,
        };
        let mut buffer = Vec::new();
        put_f32(&mut buffer, EndianType::Little, 1.0);
        put_i32(&mut buffer, EndianType::Little, 2);
        put_i32(&mut buffer, EndianType::Little, 3);
        assert!(matches!(
            StreamedFrame::read(&buffer, EndianType::Little, version, 0),
            Err(StreamedCurveError::Truncated { .. })
        ));
        assert!(matches!(
            StreamedCurveKey::read(&buffer, EndianType::Little, buffer.len() - 2),
            Err(StreamedCurveError::Truncated { .. })
        ));
        let mut negative = Vec::new();
        put_f32(&mut negative, EndianType::Little, 0.0);
        put_i32(&mut negative, EndianType::Little, -1);
        assert_eq!(
            StreamedFrame::read(&negative, EndianType::Little, version, 0),
            Err(StreamedCurveError::NegativeCurveCount { count: -1 })
        );
        let empty: &[u8] = &[];
        assert!(matches!(
            StreamedFrame::read(empty, EndianType::Little, version, 0),
            Err(StreamedCurveError::Truncated { .. })
        ));
    }

    #[test]
    fn rounding_threshold_matches_the_source_float_constant() {
        assert_eq!(ROUNDING_ERROR_THRESHOLD, f64::from(5e-7f32));
    }
}
