//! Sprite bitmap conversion gates and atlas coordinates.
//!
//! Mirrors `AssetRipper.Export.Modules.Textures/SpriteConverter.cs` (`Supported`,
//! `TryConvertToBitmap`) and `SpriteExtensions.GetSpriteCoordinatesInAtlas`.
//! The converter resolves the sprite's texture through
//! `SpriteExtensions.TryGetTexture` (the `RD.Texture` pointer) and delegates to the
//! texture pipeline; an atlas-backed sprite (`SpriteAtlasP is { }`) and a sprite
//! with no resolvable texture both report failure with `DirectBitmap.Empty`.
//!
//! Collection-backed pointer resolution (`TryGetAsset`), the texture bitmap, and
//! the atlas render-data lookup stay caller-side: these kernels take the resolved
//! facts so the source shapes stay auditable without a managed collection.

/// The class ID of `Sprite` (class 213).
pub const SPRITE_CLASS_ID: i32 = 213;

/// `SpriteConverter.Supported` over caller facts.
///
/// `has_texture` is whether `TryGetTexture` resolved a texture;
/// `texture_integrity` is that texture's `CheckAssetIntegrity`;
/// `has_atlas` records the `SpriteAtlasP is { }` branch. Without a texture the
/// source returns `false` on both the atlas and the no-atlas paths.
pub fn sprite_supported(has_texture: bool, texture_integrity: bool, _has_atlas: bool) -> bool {
    if has_texture {
        texture_integrity
    } else {
        false
    }
}

/// `SpriteConverter.TryConvertToBitmap` over caller facts.
///
/// `has_texture` is whether `TryGetTexture` resolved a texture;
/// `texture_convert_ok` is the delegated `TextureConverter.TryConvertToBitmap`
/// outcome; `has_atlas` records the `SpriteAtlasP is { }` branch. Atlas-backed
/// and texture-less sprites both fail with `DirectBitmap.Empty`.
pub fn sprite_try_convert_to_bitmap(
    has_texture: bool,
    texture_convert_ok: bool,
    _has_atlas: bool,
) -> bool {
    if has_texture {
        texture_convert_ok
    } else {
        false
    }
}

/// Sprite coordinates inside its atlas.
///
/// Mirrors `SpriteExtensions.GetSpriteCoordinatesInAtlas`: the caller selects the
/// atlas rect/offset when the atlas holds the sprite's render-data key
/// (`atlas is not null && Has_RenderDataKey && RenderDataMap.TryGetValue`), else
/// the sprite's own render-data rect/offset. `sprite_rect` is the sprite `Rect`
/// (`[x, y, width, height]`; only the size feeds the math); `rd_rect`/`rd_offset`
/// are the sprite render-data rect/offset; `atlas_rect`/`atlas_offset` are the
/// atlas entry used when `atlas_hit` is set. `pivot` is the stored pivot used
/// when `has_pivot` is set, else it is derived from the sprite center plus
/// `offset`. `border` is used when `has_border` is set, else zeros; stored zero
/// components are preserved rather than shifted.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SpriteAtlasCoordinates {
    /// The atlas rect (`[x, y, width, height]`).
    pub rect: [f32; 4],
    /// The atlas pivot (`[x, y]`).
    pub pivot: [f32; 2],
    /// The atlas border (`[left, bottom, right, top]`).
    pub border: [f32; 4],
}

/// Computes [`SpriteAtlasCoordinates`] following the source order exactly.
#[allow(clippy::too_many_arguments)]
pub fn get_sprite_coordinates_in_atlas(
    sprite_rect: [f32; 4],
    rd_rect: [f32; 4],
    rd_offset: [f32; 2],
    atlas_hit: bool,
    atlas_rect: [f32; 4],
    atlas_offset: [f32; 2],
    has_pivot: bool,
    pivot: [f32; 2],
    offset: [f32; 2],
    has_border: bool,
    border: [f32; 4],
) -> SpriteAtlasCoordinates {
    let (atlas_rect, crop_bot_left) = if atlas_hit {
        (atlas_rect, atlas_offset)
    } else {
        (rd_rect, rd_offset)
    };
    let sprite_size = [sprite_rect[2], sprite_rect[3]];
    let atlas_size = [atlas_rect[2], atlas_rect[3]];
    let size_delta = [
        sprite_size[0] - atlas_size[0],
        sprite_size[1] - atlas_size[1],
    ];
    let crop_top_right = [
        size_delta[0] - crop_bot_left[0],
        size_delta[1] - crop_bot_left[1],
    ];

    let pivot = if has_pivot {
        pivot
    } else {
        let center = [sprite_size[0] / 2.0, sprite_size[1] / 2.0];
        let pivot_offset = [center[0] + offset[0], center[1] + offset[1]];
        [
            pivot_offset[0] / sprite_size[0],
            pivot_offset[1] / sprite_size[1],
        ]
    };

    let pivot_position = [pivot[0] * sprite_size[0], pivot[1] * sprite_size[1]];
    let atlas_pivot_position = [
        pivot_position[0] - crop_bot_left[0],
        pivot_position[1] - crop_bot_left[1],
    ];
    let atlas_pivot = [
        atlas_pivot_position[0] / atlas_size[0],
        atlas_pivot_position[1] / atlas_size[1],
    ];

    let atlas_border = if has_border {
        [
            if border[0] == 0.0 {
                0.0
            } else {
                border[0] - crop_bot_left[0]
            },
            if border[1] == 0.0 {
                0.0
            } else {
                border[1] - crop_bot_left[1]
            },
            if border[2] == 0.0 {
                0.0
            } else {
                border[2] - crop_top_right[0]
            },
            if border[3] == 0.0 {
                0.0
            } else {
                border[3] - crop_top_right[1]
            },
        ]
    } else {
        [0.0, 0.0, 0.0, 0.0]
    };

    SpriteAtlasCoordinates {
        rect: atlas_rect,
        pivot: atlas_pivot,
        border: atlas_border,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sprite_class_id_matches_the_source() {
        assert_eq!(SPRITE_CLASS_ID, 213);
    }

    #[test]
    fn supported_follows_the_texture_integrity() {
        // A resolved texture reports its own integrity.
        assert!(sprite_supported(true, true, false));
        assert!(sprite_supported(true, true, true));
        assert!(!sprite_supported(true, false, false));
        assert!(!sprite_supported(true, false, true));
        // Without a texture both the atlas and the no-atlas paths fail.
        assert!(!sprite_supported(false, true, false));
        assert!(!sprite_supported(false, true, true));
        assert!(!sprite_supported(false, false, false));
        assert!(!sprite_supported(false, false, true));
    }

    #[test]
    fn convert_delegates_to_the_texture_pipeline() {
        // A resolved texture reports the delegated conversion outcome.
        assert!(sprite_try_convert_to_bitmap(true, true, false));
        assert!(sprite_try_convert_to_bitmap(true, true, true));
        assert!(!sprite_try_convert_to_bitmap(true, false, false));
        assert!(!sprite_try_convert_to_bitmap(true, false, true));
        // Atlas-backed and texture-less sprites fail with an empty bitmap.
        assert!(!sprite_try_convert_to_bitmap(false, true, false));
        assert!(!sprite_try_convert_to_bitmap(false, true, true));
        assert!(!sprite_try_convert_to_bitmap(false, false, false));
        assert!(!sprite_try_convert_to_bitmap(false, false, true));
    }

    #[test]
    fn atlas_coordinates_follow_the_render_data_rect() {
        // 64x32 sprite, direct pivot, full border, no atlas: the RD rect wins.
        let out = get_sprite_coordinates_in_atlas(
            [10.0, 20.0, 64.0, 32.0],
            [1.0, 2.0, 32.0, 16.0],
            [4.0, 2.0],
            false,
            [0.0, 0.0, 8.0, 8.0],
            [1.0, 1.0],
            true,
            [0.5, 0.5],
            [0.0, 0.0],
            true,
            [1.0, 2.0, 3.0, 4.0],
        );
        assert_eq!(out.rect, [1.0, 2.0, 32.0, 16.0]);
        assert_eq!(out.pivot, [0.875, 0.875]);
        assert_eq!(out.border, [-3.0, 0.0, -25.0, -10.0]);
    }

    #[test]
    fn atlas_hit_overrides_the_render_data_rect() {
        // The atlas entry wins; stored zero border components are preserved.
        let out = get_sprite_coordinates_in_atlas(
            [0.0, 0.0, 64.0, 32.0],
            [0.0, 0.0, 10.0, 10.0],
            [1.0, 1.0],
            true,
            [0.0, 0.0, 32.0, 16.0],
            [4.0, 2.0],
            true,
            [0.5, 0.5],
            [0.0, 0.0],
            true,
            [0.0, 5.0, 0.0, 6.0],
        );
        assert_eq!(out.rect, [0.0, 0.0, 32.0, 16.0]);
        assert_eq!(out.pivot, [0.875, 0.875]);
        assert_eq!(out.border, [0.0, 3.0, 0.0, -8.0]);
    }

    #[test]
    fn atlas_coordinates_derive_a_missing_pivot() {
        // No stored pivot or border: the pivot comes from center + offset.
        let out = get_sprite_coordinates_in_atlas(
            [0.0, 0.0, 64.0, 32.0],
            [0.0, 0.0, 32.0, 16.0],
            [0.0, 0.0],
            false,
            [0.0, 0.0, 8.0, 8.0],
            [1.0, 1.0],
            false,
            [0.0, 0.0],
            [8.0, -4.0],
            false,
            [9.0, 9.0, 9.0, 9.0],
        );
        assert_eq!(out.rect, [0.0, 0.0, 32.0, 16.0]);
        assert_eq!(out.pivot, [1.25, 0.75]);
        assert_eq!(out.border, [0.0, 0.0, 0.0, 0.0]);
    }
}
