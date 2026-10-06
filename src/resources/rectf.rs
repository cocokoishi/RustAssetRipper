//! `RectfExtensions` plus the sprite physics-shape point math.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/RectfExtensions.cs`. The serialized `Rectf`
//! holds `X`/`Y`/`Width`/`Height`; `CopyValues` fills them from a runtime rectangle and the
//! accessors expose the center/position/size.
//!
//! Also mirrors the per-point math of `SpriteMetaDataExtensions.GeneratePhysicsShape`: the
//! pivot shift from the atlas rect/pivot and the pixels-to-units point transform. The shape
//! list assembly, the empty-shape early-out, and the packing-rotation fixup stay caller-side
//! (the fixup switches on the generated packing-rotation enum); only this pure point math
//! ports.
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

/// `SpriteMetaDataExtensions.GeneratePhysicsShape` pivot shift.
///
/// `rect_size` is the atlas rect size (`[width, height]`); the half-rect term multiplies
/// by `0.5` exactly like the source.
pub fn sprite_physics_pivot_shift(rect_size: [f32; 2], pivot: [f32; 2]) -> [f32; 2] {
    [
        rect_size[0] * pivot[0] - rect_size[0] * 0.5,
        rect_size[1] * pivot[1] - rect_size[1] * 0.5,
    ]
}

/// `SpriteMetaDataExtensions.GeneratePhysicsShape` point transform: the source point scales
/// by the sprite's pixels-to-units, then shifts by [`sprite_physics_pivot_shift`].
pub fn sprite_physics_shape_point(
    point: [f32; 2],
    pixels_to_units: f32,
    pivot_shift: [f32; 2],
) -> [f32; 2] {
    [
        point[0] * pixels_to_units + pivot_shift[0],
        point[1] * pixels_to_units + pivot_shift[1],
    ]
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

    /// The physics-shape pivot shift centers on a centered pivot and the point transform
    /// scales before shifting.
    #[test]
    fn sprite_physics_shape_math_matches_the_source_order() {
        // A centered pivot needs no shift; a corner pivot shifts by half the rect.
        assert_eq!(
            sprite_physics_pivot_shift([10.0, 20.0], [0.5, 0.5]),
            [0.0, 0.0]
        );
        assert_eq!(
            sprite_physics_pivot_shift([10.0, 20.0], [0.0, 0.0]),
            [-5.0, -10.0]
        );
        assert_eq!(
            sprite_physics_pivot_shift([10.0, 20.0], [1.0, 0.25]),
            [5.0, -5.0]
        );
        // Points scale by pixels-to-units, then shift.
        let shift = sprite_physics_pivot_shift([10.0, 20.0], [0.0, 0.0]);
        assert_eq!(
            sprite_physics_shape_point([3.0, 4.0], 2.0, shift),
            [1.0, -2.0]
        );
        assert_eq!(
            sprite_physics_shape_point([0.0, 0.0], 100.0, [0.0, 0.0]),
            [0.0, 0.0]
        );
    }
}
