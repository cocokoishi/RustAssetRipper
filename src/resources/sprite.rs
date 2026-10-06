//! Sprite bitmap conversion gates.
//!
//! Mirrors `AssetRipper.Export.Modules.Textures/SpriteConverter.cs` (`Supported`,
//! `TryConvertToBitmap`). Both methods resolve the sprite's texture through
//! `SpriteExtensions.TryGetTexture` (the `RD.Texture` pointer) and delegate to the
//! texture pipeline; an atlas-backed sprite (`SpriteAtlasP is { }`) and a sprite
//! with no resolvable texture both report failure with `DirectBitmap.Empty`.
//!
//! Collection-backed pointer resolution (`TryGetAsset`) and the texture bitmap
//! itself stay caller-side: these kernels take the resolved facts so the
//! three-way source shape stays auditable without a managed collection.

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
}
