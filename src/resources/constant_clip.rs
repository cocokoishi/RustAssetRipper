//! `ConstantClipExtensions`.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/ConstantClipExtensions.cs`.

/// A constant-clip view.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ConstantClipView {
    /// `Data` element count.
    pub data_count: usize,
}

impl ConstantClipView {
    /// `ConstantClipExtensions.IsSet`.
    pub fn is_set(&self) -> bool {
        self.data_count > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Byte-for-byte differential against the reference `ConstantClipExtensions` (probe
    /// `Temp/opencode/clip2probe`).
    #[test]
    fn constant_clip_matches_the_reference_runtime() {
        let fixture = include_str!("constant_clip_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "CCLIP", "{line}");
            assert_eq!(
                ConstantClipView { data_count: 0 }.is_set(),
                parts[2] == "1",
                "{line}"
            );
            assert_eq!(
                ConstantClipView { data_count: 1 }.is_set(),
                parts[3] == "1",
                "{line}"
            );
            rows += 1;
        }
        assert_eq!(rows, 9);
    }
}
