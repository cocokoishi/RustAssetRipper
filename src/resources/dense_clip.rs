//! `DenseClipExtensions`.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/DenseClipExtensions.cs`.

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
}
