//! Web preview image dispatch.
//!
//! Mirrors the `#region Image` dispatch in
//! `AssetRipper.GUI.Web/Pages/Assets/AssetAPI.cs` (`HasImageData`,
//! `GetImageBitmap`, `GetRawImageData`). The bitmap builders themselves stay
//! caller-side (`TextureConverter`, `SpriteConverter`, `TerrainHeatmap`); these
//! kernels route an asset kind to its source so the dispatch stays auditable
//! without managed objects or HTTP plumbing.

use super::sprite::sprite_supported;

/// The asset kinds the preview image dispatch distinguishes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreviewAssetKind {
    /// `IImageTexture` (any convertible texture).
    ImageTexture,
    /// `SpriteInformationObject` (previews through its texture).
    SpriteInfo,
    /// `ISprite` (previews through `SpriteConverter`).
    Sprite,
    /// `ITerrainData` (previews through `TerrainHeatmap`).
    Terrain,
    /// Anything else (never has image data).
    Other,
}

/// Facts feeding the `ISprite` arm of [`has_preview_image_data`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpritePreviewFacts {
    /// Whether `TryGetTexture` resolved a texture.
    pub has_texture: bool,
    /// That texture's `CheckAssetIntegrity`.
    pub texture_integrity: bool,
    /// Whether the sprite is atlas-backed (`SpriteAtlasP is { }`).
    pub has_atlas: bool,
}

/// `AssetAPI.HasImageData` over caller facts.
///
/// `texture_integrity` is the `CheckAssetIntegrity` of the texture (or the
/// sprite-information texture); `terrain_heights` is the terrain heightmap
/// entry count.
pub fn has_preview_image_data(
    kind: PreviewAssetKind,
    texture_integrity: bool,
    sprite: SpritePreviewFacts,
    terrain_heights: usize,
) -> bool {
    match kind {
        PreviewAssetKind::ImageTexture | PreviewAssetKind::SpriteInfo => texture_integrity,
        PreviewAssetKind::Sprite => sprite_supported(
            sprite.has_texture,
            sprite.texture_integrity,
            sprite.has_atlas,
        ),
        PreviewAssetKind::Terrain => terrain_heights > 0,
        PreviewAssetKind::Other => false,
    }
}

/// Where `AssetAPI.GetImageBitmap` draws the bitmap from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreviewBitmapSource {
    /// `TextureConverter.TryConvertToBitmap` (empty on failure).
    TextureConverter,
    /// `SpriteConverter.TryConvertToBitmap` (empty on failure).
    SpriteConverter,
    /// `TerrainHeatmap.GetBitmap`.
    TerrainHeatmap,
    /// `DirectBitmap.Empty`.
    Empty,
}

/// `AssetAPI.GetImageBitmap` routing over the asset kind.
pub fn preview_bitmap_source(kind: PreviewAssetKind) -> PreviewBitmapSource {
    match kind {
        PreviewAssetKind::ImageTexture | PreviewAssetKind::SpriteInfo => {
            PreviewBitmapSource::TextureConverter
        }
        PreviewAssetKind::Sprite => PreviewBitmapSource::SpriteConverter,
        PreviewAssetKind::Terrain => PreviewBitmapSource::TerrainHeatmap,
        PreviewAssetKind::Other => PreviewBitmapSource::Empty,
    }
}

/// Where `AssetAPI.GetRawImageData` draws the raw bytes from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreviewRawSource {
    /// `ITexture2D.GetImageData` (the texture bytes, possibly streamed).
    TextureBytes,
    /// The heightmap `Heights` span reinterpreted as bytes.
    TerrainHeights,
    /// No bytes.
    Empty,
}

/// `AssetAPI.GetRawImageData` routing over the asset kind.
///
/// The source narrows the texture arm to `ITexture2D`, so `is_texture_2d`
/// records whether an image texture is a plain 2D texture. Sprites read their
/// resolved texture's bytes (`sprite_has_texture`), falling back to nothing
/// when no texture resolves. Sprite-information objects always read their
/// texture's bytes.
pub fn preview_raw_source(
    kind: PreviewAssetKind,
    is_texture_2d: bool,
    sprite_has_texture: bool,
) -> PreviewRawSource {
    match kind {
        PreviewAssetKind::ImageTexture if is_texture_2d => PreviewRawSource::TextureBytes,
        PreviewAssetKind::SpriteInfo => PreviewRawSource::TextureBytes,
        PreviewAssetKind::Sprite if sprite_has_texture => PreviewRawSource::TextureBytes,
        PreviewAssetKind::Terrain => PreviewRawSource::TerrainHeights,
        _ => PreviewRawSource::Empty,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sprite_facts(has_texture: bool, integrity: bool, atlas: bool) -> SpritePreviewFacts {
        SpritePreviewFacts {
            has_texture,
            texture_integrity: integrity,
            has_atlas: atlas,
        }
    }

    #[test]
    fn presence_follows_integrity_and_content() {
        // Textures and sprite-info objects report their texture integrity.
        assert!(has_preview_image_data(
            PreviewAssetKind::ImageTexture,
            true,
            sprite_facts(false, false, false),
            0
        ));
        assert!(!has_preview_image_data(
            PreviewAssetKind::ImageTexture,
            false,
            sprite_facts(false, false, false),
            0
        ));
        assert!(has_preview_image_data(
            PreviewAssetKind::SpriteInfo,
            true,
            sprite_facts(false, false, false),
            0
        ));
        assert!(!has_preview_image_data(
            PreviewAssetKind::SpriteInfo,
            false,
            sprite_facts(false, false, false),
            0
        ));
        // Sprites delegate to the converter gate.
        assert!(has_preview_image_data(
            PreviewAssetKind::Sprite,
            false,
            sprite_facts(true, true, false),
            0
        ));
        assert!(!has_preview_image_data(
            PreviewAssetKind::Sprite,
            true,
            sprite_facts(true, false, false),
            0
        ));
        assert!(!has_preview_image_data(
            PreviewAssetKind::Sprite,
            true,
            sprite_facts(false, true, true),
            0
        ));
        // Terrains need at least one height entry.
        assert!(!has_preview_image_data(
            PreviewAssetKind::Terrain,
            true,
            sprite_facts(true, true, false),
            0
        ));
        assert!(has_preview_image_data(
            PreviewAssetKind::Terrain,
            false,
            sprite_facts(false, false, false),
            1
        ));
        // Anything else never previews.
        assert!(!has_preview_image_data(
            PreviewAssetKind::Other,
            true,
            sprite_facts(true, true, false),
            9
        ));
    }

    #[test]
    fn bitmap_routing_matches_the_source_switch() {
        assert_eq!(
            preview_bitmap_source(PreviewAssetKind::ImageTexture),
            PreviewBitmapSource::TextureConverter
        );
        assert_eq!(
            preview_bitmap_source(PreviewAssetKind::SpriteInfo),
            PreviewBitmapSource::TextureConverter
        );
        assert_eq!(
            preview_bitmap_source(PreviewAssetKind::Sprite),
            PreviewBitmapSource::SpriteConverter
        );
        assert_eq!(
            preview_bitmap_source(PreviewAssetKind::Terrain),
            PreviewBitmapSource::TerrainHeatmap
        );
        assert_eq!(
            preview_bitmap_source(PreviewAssetKind::Other),
            PreviewBitmapSource::Empty
        );
    }

    #[test]
    fn raw_routing_narrows_textures_to_2d() {
        assert_eq!(
            preview_raw_source(PreviewAssetKind::ImageTexture, true, false),
            PreviewRawSource::TextureBytes
        );
        assert_eq!(
            preview_raw_source(PreviewAssetKind::ImageTexture, false, false),
            PreviewRawSource::Empty
        );
        // Sprite-info objects always read their texture; the flags are inert.
        assert_eq!(
            preview_raw_source(PreviewAssetKind::SpriteInfo, true, false),
            PreviewRawSource::TextureBytes
        );
        assert_eq!(
            preview_raw_source(PreviewAssetKind::SpriteInfo, false, false),
            PreviewRawSource::TextureBytes
        );
        // Sprites read their resolved texture's bytes, else nothing.
        assert_eq!(
            preview_raw_source(PreviewAssetKind::Sprite, true, true),
            PreviewRawSource::TextureBytes
        );
        assert_eq!(
            preview_raw_source(PreviewAssetKind::Sprite, true, false),
            PreviewRawSource::Empty
        );
        assert_eq!(
            preview_raw_source(PreviewAssetKind::Terrain, true, false),
            PreviewRawSource::TerrainHeights
        );
        assert_eq!(
            preview_raw_source(PreviewAssetKind::Other, true, true),
            PreviewRawSource::Empty
        );
    }
}
