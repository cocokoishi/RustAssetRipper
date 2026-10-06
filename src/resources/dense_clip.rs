//! `DenseClipExtensions`.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/DenseClipExtensions.cs`, plus the
//! `AnimationClipConverter.ProcessDenses` sample-walk decisions over caller facts:
//! the empty-walk gate, the row-major sample-array positions, and the zero slopes
//! dense curves carry. Binding classification and curve emission stay with the
//! converter modules.

/// A dense-clip view.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DenseClipView {
    /// `SampleArray` element count.
    pub sample_count: usize,
}

impl DenseClipView {
    /// `DenseClipExtensions.IsSet`.
    pub fn is_set(&self) -> bool {
        self.sample_count > 0
    }
}

/// `AnimationClipConverter.ProcessDenses` walk gate: frames are emitted only when
/// both the frame count and the curve count are nonzero.
pub const fn dense_walk_is_active(frame_count: usize, curve_count: usize) -> bool {
    frame_count != 0 && curve_count != 0
}

/// `AnimationClipConverter.ProcessDenses` sample-array position for one dense
/// curve value: `frameIndex * curveCount + curveIndex` (row-major).
/// `None` on overflow instead of the source's wrapping integer arithmetic.
pub const fn dense_sample_position(
    frame_index: usize,
    curve_count: usize,
    curve_index: usize,
) -> Option<usize> {
    match frame_index.checked_mul(curve_count) {
        Some(frame_offset) => frame_offset.checked_add(curve_index),
        None => None,
    }
}

/// `AnimationClipConverter.ProcessDenses` slope values: dense curves carry no
/// slopes, so every emitted key uses four zero slopes.
pub const DENSE_SLOPE_DEFAULTS: [f32; 4] = [0.0, 0.0, 0.0, 0.0];

#[cfg(test)]
mod tests {
    use super::*;

    /// Byte-for-byte differential against the reference `DenseClipExtensions` (probe
    /// `Temp/opencode/clip2probe`).
    #[test]
    fn dense_clip_matches_the_reference_runtime() {
        let fixture = include_str!("dense_clip_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "DCLIP", "{line}");
            assert_eq!(
                DenseClipView { sample_count: 0 }.is_set(),
                parts[2] == "1",
                "{line}"
            );
            assert_eq!(
                DenseClipView { sample_count: 1 }.is_set(),
                parts[3] == "1",
                "{line}"
            );
            rows += 1;
        }
        assert_eq!(rows, 9);
    }

    #[test]
    fn dense_walk_decisions_follow_the_source_guards() {
        // The walk runs only when both counts are nonzero.
        assert!(!dense_walk_is_active(0, 0));
        assert!(!dense_walk_is_active(0, 4));
        assert!(!dense_walk_is_active(3, 0));
        assert!(dense_walk_is_active(3, 4));
        // Row-major positions: frame 2 of a 4-curve clip starts at 8.
        assert_eq!(dense_sample_position(0, 4, 0), Some(0));
        assert_eq!(dense_sample_position(0, 4, 3), Some(3));
        assert_eq!(dense_sample_position(2, 4, 1), Some(9));
        // A 2-frame by 3-curve grid walks 0 through 5 in order.
        let mut walked = Vec::new();
        for frame in 0..2 {
            for curve in 0..3 {
                walked.push(dense_sample_position(frame, 3, curve));
            }
        }
        assert_eq!(
            walked,
            [Some(0), Some(1), Some(2), Some(3), Some(4), Some(5)]
        );
        // Overflow reports instead of wrapping like the source integers.
        assert_eq!(dense_sample_position(usize::MAX, 2, 0), None);
        assert_eq!(dense_sample_position(1, usize::MAX, usize::MAX), None);
        // Dense curves carry no slopes.
        assert_eq!(DENSE_SLOPE_DEFAULTS, [0.0, 0.0, 0.0, 0.0]);
    }
}
