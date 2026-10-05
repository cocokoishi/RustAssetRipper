//! `RectfExtensions`.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/RectfExtensions.cs`. The serialized `Rectf`
//! holds `X`/`Y`/`Width`/`Height`; `CopyValues` fills them from a runtime rectangle and the
//! accessors expose the center/position/size.
//!
//! Differential fixture: `rectf_fixture.txt` (probe `Temp/opencode/miscprobe`).

/// A rect view.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RectfView {
    /// `X`.
    pub x: f32,
    /// `Y`.
    pub y: f32,
    /// `Width`.
    pub width: f32,
    /// `Height`.
    pub height: f32,
}

impl RectfView {
    /// `RectfExtensions.CopyValues` (the runtime `RectangleF` is `x`, `y`, `width`, `height`).
    pub fn copy_values(&mut self, x: f32, y: f32, width: f32, height: f32) {
        self.x = x;
        self.y = y;
        self.width = width;
        self.height = height;
    }

    /// `RectfExtensions.Center`.
    pub fn center(&self) -> [f32; 2] {
        [self.x + self.width / 2.0, self.y + self.height / 2.0]
    }

    /// `RectfExtensions.Position`.
    pub fn position(&self) -> [f32; 2] {
        [self.x, self.y]
    }

    /// `RectfExtensions.Size`.
    pub fn size(&self) -> [f32; 2] {
        [self.width, self.height]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Byte-for-byte differential against the reference `RectfExtensions` (probe
    /// `Temp/opencode/miscprobe`).
    #[test]
    fn rectf_copy_values_matches_the_reference_runtime() {
        let fixture = include_str!("rectf_fixture.txt");
        let mut rect = RectfView::default();
        rect.copy_values(1.5, 2.5, 3.5, 4.5);
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "R", "{line}");
            let expected: u32 = parts[2].parse().unwrap();
            let value = match parts[1] {
                "X" => rect.x,
                "Y" => rect.y,
                "Width" => rect.width,
                "Height" => rect.height,
                other => panic!("unknown rect field {other}"),
            };
            assert_eq!(value.to_bits(), expected, "{line}");
            rows += 1;
        }
        assert_eq!(rows, 4);
        assert_eq!(rect.position(), [1.5, 2.5]);
        assert_eq!(rect.size(), [3.5, 4.5]);
        assert_eq!(rect.center(), [1.5 + 1.75, 2.5 + 2.25]);
    }
}
