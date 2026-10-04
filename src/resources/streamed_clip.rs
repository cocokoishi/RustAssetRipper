//! `StreamedClipExtensions`.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/StreamedClipExtensions.cs`.

/// A streamed-clip view.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StreamedClipView {
    /// `Data` element count.
    pub data_count: usize,
    /// `CurveCount_UInt32`: present on every version reachable through the factory, but modeled as
    /// optional to match the source's `Has_CurveCount_UInt32` gate.
    pub curve_count_u32: Option<u32>,
    /// `CurveCount_UInt16`.
    pub curve_count_u16: u16,
    /// `DiscreteCurveCount`.
    pub discrete_curve_count: u16,
}

impl StreamedClipView {
    /// `StreamedClipExtensions.IsSet`.
    pub fn is_set(&self) -> bool {
        self.data_count > 0
    }

    /// `StreamedClipExtensions.CurveCount`.
    pub fn curve_count(&self) -> i32 {
        match self.curve_count_u32 {
            Some(count) => count as i32,
            None => self.curve_count_u16 as i32 + self.discrete_curve_count as i32,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Byte-for-byte differential against the reference `StreamedClipExtensions` (probe
    /// `Temp/opencode/clip2probe`). Every reachable version serializes `CurveCount_UInt32`, so the
    /// `CurveCount_UInt16 + DiscreteCurveCount` fallback is implemented but not exercised by the
    /// reference runtime; it is asserted directly instead.
    #[test]
    fn streamed_clip_matches_the_reference_runtime() {
        let fixture = include_str!("streamed_clip_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "SCLIP", "{line}");
            let has_u32 = parts[2] == "1";
            let view = StreamedClipView {
                data_count: 1,
                curve_count_u32: has_u32.then_some(40),
                curve_count_u16: 7,
                discrete_curve_count: 3,
            };
            assert_eq!(
                StreamedClipView {
                    data_count: 0,
                    ..view
                }
                .is_set(),
                parts[3] == "1",
                "{line}"
            );
            assert_eq!(view.is_set(), parts[4] == "1", "{line}");
            assert_eq!(
                view.curve_count(),
                parts[5].parse::<i32>().unwrap(),
                "{line}"
            );
            rows += 1;
        }
        assert_eq!(rows, 9);

        let fallback = StreamedClipView {
            data_count: 1,
            curve_count_u32: None,
            curve_count_u16: 7,
            discrete_curve_count: 3,
        };
        assert_eq!(fallback.curve_count(), 10);
    }
}
