//! Sprite bitmap conversion gates, atlas coordinates, and sprite-metadata fill.
//!
//! Mirrors `AssetRipper.Export.Modules.Textures/SpriteConverter.cs` (`Supported`,
//! `TryConvertToBitmap`), `SpriteExtensions.GetSpriteCoordinatesInAtlas`, and the
//! decision kernel of
//! `AssetRipper.SourceGenerated.Extensions/SpriteMetaDataExtensions.cs`
//! (`FillSpriteMetaData`, lines 26-71).
//! The converter resolves the sprite's texture through
//! `SpriteExtensions.TryGetTexture` (the `RD.Texture` pointer) and delegates to the
//! texture pipeline; an atlas-backed sprite (`SpriteAtlasP is { }`) and a sprite
//! with no resolvable texture both report failure with `DirectBitmap.Empty`.
//!
//! Collection-backed pointer resolution (`TryGetAsset`), the texture bitmap, and
//! the atlas render-data lookup stay caller-side: these kernels take the resolved
//! facts so the source shapes stay auditable without a managed collection.
//!
//! The `FillSpriteMetaData` mesh-vertex extraction internals stay open and are not
//! ported here: the `SetBoneGeometry` body (including the `VertexDataBlob`
//! reads and the vertex/index/weight copies), the `GenerateOutline` and
//! `GeneratePhysicsShape` bodies (including `MeshOutlineGenerator` and the
//! packing-rotation fixup), and the edge list (the source carries a
//! `#warning TODO` for `Edges`). This module reports *whether* the outline, the
//! physics shape, and the bone payload must be produced, plus the Bone PPU
//! scaling math; the caller owns the vertex extraction.

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

/// Discriminant of `SpriteAlignment.Custom` written by `FillSpriteMetaData`.
///
/// Mirrors `AssetRipper.SourceGenerated.Extensions/SpriteMetaDataExtensions.cs`
/// (`FillSpriteMetaData`: `instance.Alignment = (int)SpriteAlignment.Custom`).
/// The generated `SpriteAlignment` enum assembly is build-produced, so the kernel
/// pins the literal: Unity numbers the enum `Center = 0` through `Custom = 9`.
pub const SPRITE_ALIGNMENT_CUSTOM: i32 = 9;

/// `TessellationDetail` value written verbatim by `FillSpriteMetaData`
/// (`instance.TessellationDetail = 0`).
pub const SPRITE_TESSELLATION_DETAIL: i32 = 0;

/// One sprite bone as caller facts for the `FillSpriteMetaData` bone gate.
///
/// `position` is `ISpriteBone.Position` (`[x, y]`), `length` is
/// `ISpriteBone.Length`, and `parent_id` is `ISpriteBone.ParentId` (the source
/// treats `-1` as the root bone).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SpriteBoneFacts {
    /// `ISpriteBone.ParentId`; `-1` marks the root bone.
    pub parent_id: i32,
    /// `ISpriteBone.Position` (`[x, y]`).
    pub position: [f32; 2],
    /// `ISpriteBone.Length`.
    pub length: f32,
}

/// Scales one bone exactly like the `FillSpriteMetaData` bone loop.
///
/// Mirrors `SpriteMetaDataExtensions.cs` lines 46-57: `Position.Scale(PPU)` and
/// `Length *= PPU` apply to every bone, then the root bone (`parent_id == -1`)
/// is shifted by half the sprite `Rect` size. `rect_size` is the sprite `Rect`
/// size (`[width, height]`), matching the source's `sprite.Rect.Width / 2` and
/// `sprite.Rect.Height / 2` (not the atlas rect). The source scales the sprite
/// bones in place before copying them over; the kernel instead returns the
/// scaled copy and leaves the caller's slice untouched.
pub fn scale_sprite_bone_for_metadata(
    bone: SpriteBoneFacts,
    pixels_to_units: f32,
    rect_size: [f32; 2],
) -> SpriteBoneFacts {
    let scaled = [
        bone.position[0] * pixels_to_units,
        bone.position[1] * pixels_to_units,
    ];
    let position = if bone.parent_id == -1 {
        [
            scaled[0] + rect_size[0] / 2.0,
            scaled[1] + rect_size[1] / 2.0,
        ]
    } else {
        scaled
    };
    SpriteBoneFacts {
        parent_id: bone.parent_id,
        position,
        length: bone.length * pixels_to_units,
    }
}

/// Outline gate of `FillSpriteMetaData` (`instance.Has_Outline()`).
///
/// Unlike the physics-shape gate there is no sprite-side check: the source
/// derives the outline from the sprite render data whenever the instance
/// version carries an outline list. The outline body itself
/// (`GenerateOutline`, `MeshOutlineGenerator`) stays open; this only reports
/// whether the caller must run it.
pub fn should_generate_sprite_outline(instance_has_outline: bool) -> bool {
    instance_has_outline
}

/// Physics-shape gate of `FillSpriteMetaData`
/// (`instance.Has_PhysicsShape() && sprite.Has_PhysicsShape()`).
///
/// The empty-shape early-out (`sprite.PhysicsShape.Count > 0`) lives inside the
/// `GeneratePhysicsShape` body, which stays open; this only reports whether the
/// caller must run it.
pub fn should_generate_sprite_physics_shape(
    instance_has_physics_shape: bool,
    sprite_has_physics_shape: bool,
) -> bool {
    instance_has_physics_shape && sprite_has_physics_shape
}

/// Bones gate of `FillSpriteMetaData`
/// (`instance.Has_Bones() && sprite.Has_Bones() && instance.Has_SpriteID()`).
///
/// When set, the caller copies the PPU-scaled bones in order, assigns the
/// sprite ID, and runs the still-open `SetBoneGeometry` vertex extraction.
pub fn should_fill_sprite_bones(
    instance_has_bones: bool,
    sprite_has_bones: bool,
    instance_has_sprite_id: bool,
) -> bool {
    instance_has_bones && sprite_has_bones && instance_has_sprite_id
}

/// Decision output of the `FillSpriteMetaData` kernel.
///
/// Every field is a value to write or a gated action to run; the mesh-vertex
/// extraction bodies (`GenerateOutline`, `GeneratePhysicsShape`,
/// `SetBoneGeometry`) stay open and caller-side.
#[derive(Clone, Debug, PartialEq)]
pub struct FillSpriteMetaDataOutput {
    /// `Rect` value the caller writes (`instance.Rect.CopyValues(rect)`).
    pub rect: [f32; 4],
    /// `Pivot` value the caller writes (`instance.Pivot.CopyValues(pivot)`).
    pub pivot: [f32; 2],
    /// `Border` value the caller writes when [`Self::copy_border`] is set.
    pub border: [f32; 4],
    /// Always [`SPRITE_ALIGNMENT_CUSTOM`].
    pub alignment: i32,
    /// Whether the caller performs `Border?.CopyValues(border)`: the source
    /// uses a null-conditional, so versions whose instance has no `Border`
    /// skip the copy.
    pub copy_border: bool,
    /// Always [`SPRITE_TESSELLATION_DETAIL`].
    pub tessellation_detail: i32,
    /// Whether the caller runs the (open) outline generation.
    pub generate_outline: bool,
    /// Whether the caller runs the (open) physics-shape generation.
    pub generate_physics_shape: bool,
    /// Whether the caller copies [`Self::scaled_bones`] in order, assigns
    /// [`Self::sprite_id`], and runs the (open) bone-geometry extraction.
    pub fill_bones: bool,
    /// PPU-scaled bone copies, populated only when [`Self::fill_bones`] is set.
    pub scaled_bones: Vec<SpriteBoneFacts>,
    /// Caller-supplied sprite ID, carried through only when
    /// [`Self::fill_bones`] is set.
    pub sprite_id: Option<String>,
}

/// Decision kernel of `FillSpriteMetaData`
/// (`AssetRipper.SourceGenerated.Extensions/SpriteMetaDataExtensions.cs`
/// lines 26-71) over caller facts.
///
/// The atlas `rect`/`pivot`/`border` come from [`get_sprite_coordinates_in_atlas`]
/// (the source's `sprite.GetSpriteCoordinatesInAtlas(atlas, ...)`); the
/// remaining coordinate-tuple arguments are forwarded unchanged, so see that
/// function for their meaning. On top of those:
///
/// * `Name` is copied verbatim by the caller (`instance.Name = sprite.Name`);
///   there is no transform to kernelize, so it is not carried here.
/// * `Rect`, `Pivot`, and `Border` are the coordinate-kernel outputs.
/// * `Alignment` is always [`SPRITE_ALIGNMENT_CUSTOM`]; `TessellationDetail` is
///   always [`SPRITE_TESSELLATION_DETAIL`].
/// * `instance_has_border` models the `Border?.` null-conditional: versions
///   whose instance has no `Border` skip that copy.
/// * `generate_outline`, `generate_physics_shape`, and `fill_bones` are the
///   `Has_Outline`, `Has_PhysicsShape`, and `Has_Bones`/`Has_SpriteID` gates.
/// * `pixels_to_units` is `sprite.PixelsToUnits`; `sprite_bones` holds the
///   sprite bones in order; the rect size for the root-bone offset is taken
///   from `sprite_rect` (`sprite.Rect.Width`/`Height`), matching the source.
/// * `caller_sprite_id` is the `SpriteID` the caller assigns. The source
///   generates a fresh random GUID (`Guid.NewGuid()`), which is
///   nondeterministic and independent of the sprite content, so a pure
///   decision kernel cannot reproduce it: the caller supplies the ID (random
///   or otherwise) and the kernel carries it through unchanged when the bones
///   gate is set, `None` otherwise.
#[allow(clippy::too_many_arguments)]
pub fn fill_sprite_metadata(
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
    instance_has_border: bool,
    instance_has_outline: bool,
    instance_has_physics_shape: bool,
    sprite_has_physics_shape: bool,
    instance_has_bones: bool,
    sprite_has_bones: bool,
    instance_has_sprite_id: bool,
    pixels_to_units: f32,
    sprite_bones: &[SpriteBoneFacts],
    caller_sprite_id: &str,
) -> FillSpriteMetaDataOutput {
    let coords = get_sprite_coordinates_in_atlas(
        sprite_rect,
        rd_rect,
        rd_offset,
        atlas_hit,
        atlas_rect,
        atlas_offset,
        has_pivot,
        pivot,
        offset,
        has_border,
        border,
    );
    let generate_outline = should_generate_sprite_outline(instance_has_outline);
    let generate_physics_shape =
        should_generate_sprite_physics_shape(instance_has_physics_shape, sprite_has_physics_shape);
    let fill_bones =
        should_fill_sprite_bones(instance_has_bones, sprite_has_bones, instance_has_sprite_id);
    let rect_size = [sprite_rect[2], sprite_rect[3]];
    let (scaled_bones, sprite_id) = if fill_bones {
        let scaled = sprite_bones
            .iter()
            .map(|bone| scale_sprite_bone_for_metadata(*bone, pixels_to_units, rect_size))
            .collect();
        (scaled, Some(caller_sprite_id.to_owned()))
    } else {
        (Vec::new(), None)
    };
    FillSpriteMetaDataOutput {
        rect: coords.rect,
        pivot: coords.pivot,
        border: coords.border,
        alignment: SPRITE_ALIGNMENT_CUSTOM,
        copy_border: instance_has_border,
        tessellation_detail: SPRITE_TESSELLATION_DETAIL,
        generate_outline,
        generate_physics_shape,
        fill_bones,
        scaled_bones,
        sprite_id,
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

    const FILL_BONES: [SpriteBoneFacts; 2] = [
        SpriteBoneFacts {
            parent_id: -1,
            position: [1.0, 2.0],
            length: 4.0,
        },
        SpriteBoneFacts {
            parent_id: 0,
            position: [3.0, -1.0],
            length: 2.0,
        },
    ];

    const FILL_SPRITE_ID: &str = "0123456789abcdef0123456789abcdef";

    #[allow(clippy::too_many_arguments)]
    fn fill_with_gates(
        instance_has_outline: bool,
        instance_has_physics_shape: bool,
        sprite_has_physics_shape: bool,
        instance_has_bones: bool,
        sprite_has_bones: bool,
        instance_has_sprite_id: bool,
    ) -> FillSpriteMetaDataOutput {
        fill_sprite_metadata(
            [0.0, 0.0, 64.0, 32.0],
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
            true,
            instance_has_outline,
            instance_has_physics_shape,
            sprite_has_physics_shape,
            instance_has_bones,
            sprite_has_bones,
            instance_has_sprite_id,
            0.5,
            &FILL_BONES,
            FILL_SPRITE_ID,
        )
    }

    #[test]
    fn fill_kernel_writes_fixed_values_and_copies() {
        // All gates on: coordinates match the no-atlas render-data path, the
        // fixed alignment/tessellation values hold, and every payload runs.
        let out = fill_with_gates(true, true, true, true, true, true);
        assert_eq!(out.alignment, SPRITE_ALIGNMENT_CUSTOM);
        assert_eq!(out.alignment, 9);
        assert_eq!(out.tessellation_detail, SPRITE_TESSELLATION_DETAIL);
        assert_eq!(out.tessellation_detail, 0);
        assert_eq!(out.rect, [1.0, 2.0, 32.0, 16.0]);
        assert_eq!(out.pivot, [0.875, 0.875]);
        assert_eq!(out.border, [-3.0, 0.0, -25.0, -10.0]);
        assert!(out.copy_border);
        assert!(out.generate_outline);
        assert!(out.generate_physics_shape);
        assert!(out.fill_bones);
        // Root bone: PPU scale then half the sprite Rect size as offset.
        assert_eq!(out.scaled_bones.len(), 2);
        assert_eq!(
            out.scaled_bones[0],
            SpriteBoneFacts {
                parent_id: -1,
                position: [32.5, 17.0],
                length: 2.0,
            }
        );
        // Child bone: PPU scale only.
        assert_eq!(
            out.scaled_bones[1],
            SpriteBoneFacts {
                parent_id: 0,
                position: [1.5, -0.5],
                length: 1.0,
            }
        );
        assert_eq!(out.sprite_id.as_deref(), Some(FILL_SPRITE_ID));
    }

    #[test]
    fn fill_kernel_gates_off_skip_payloads_but_keep_copies() {
        // All gates off: the unconditional copies still land, the gated
        // payloads stay empty.
        let out = fill_with_gates(false, false, false, false, false, false);
        assert_eq!(out.alignment, 9);
        assert_eq!(out.tessellation_detail, 0);
        assert_eq!(out.rect, [1.0, 2.0, 32.0, 16.0]);
        assert_eq!(out.pivot, [0.875, 0.875]);
        assert_eq!(out.border, [-3.0, 0.0, -25.0, -10.0]);
        assert!(!out.generate_outline);
        assert!(!out.generate_physics_shape);
        assert!(!out.fill_bones);
        assert!(out.scaled_bones.is_empty());
        assert_eq!(out.sprite_id, None);
    }

    #[test]
    fn outline_gate_follows_the_instance_flag() {
        assert!(should_generate_sprite_outline(true));
        assert!(!should_generate_sprite_outline(false));
        assert!(fill_with_gates(true, false, false, false, false, false).generate_outline);
        assert!(!fill_with_gates(false, false, false, false, false, false).generate_outline);
    }

    #[test]
    fn physics_gate_needs_both_sides() {
        assert!(should_generate_sprite_physics_shape(true, true));
        assert!(!should_generate_sprite_physics_shape(true, false));
        assert!(!should_generate_sprite_physics_shape(false, true));
        assert!(!should_generate_sprite_physics_shape(false, false));
        // A sprite-side shape alone never triggers generation.
        let out = fill_with_gates(false, false, true, false, false, false);
        assert!(!out.generate_physics_shape);
    }

    #[test]
    fn bones_gate_needs_all_three_flags() {
        for instance in [false, true] {
            for sprite in [false, true] {
                for id in [false, true] {
                    let want = instance && sprite && id;
                    assert_eq!(
                        should_fill_sprite_bones(instance, sprite, id),
                        want,
                        "instance={instance} sprite={sprite} id={id}",
                    );
                    let out = fill_with_gates(false, false, false, instance, sprite, id);
                    assert_eq!(out.fill_bones, want);
                    assert_eq!(out.sprite_id.is_some(), want);
                    assert_eq!(out.scaled_bones.len() == FILL_BONES.len(), want);
                }
            }
        }
    }

    #[test]
    fn bone_scaling_applies_ppu_and_root_offset() {
        // Root bone: scale, then shift by half the sprite Rect size.
        let root = scale_sprite_bone_for_metadata(
            SpriteBoneFacts {
                parent_id: -1,
                position: [1.0, 2.0],
                length: 4.0,
            },
            0.5,
            [64.0, 32.0],
        );
        assert_eq!(root.position, [32.5, 17.0]);
        assert_eq!(root.length, 2.0);
        assert_eq!(root.parent_id, -1);
        // Non-root bones (child or deeper) get the PPU scale with no offset.
        for parent_id in [0, 5] {
            let bone = scale_sprite_bone_for_metadata(
                SpriteBoneFacts {
                    parent_id,
                    position: [3.0, -1.0],
                    length: 2.0,
                },
                0.5,
                [64.0, 32.0],
            );
            assert_eq!(bone.position, [1.5, -0.5]);
            assert_eq!(bone.length, 1.0);
        }
    }

    #[test]
    fn fill_kernel_carries_the_caller_sprite_id_verbatim() {
        // The source mints a fresh random GUID; the kernel takes the ID as a
        // caller fact and must not transform it.
        let other = "fedcba9876543210fedcba9876543210";
        let out = fill_sprite_metadata(
            [0.0, 0.0, 64.0, 32.0],
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
            true,
            false,
            false,
            false,
            true,
            true,
            true,
            0.5,
            &FILL_BONES,
            other,
        );
        assert!(out.fill_bones);
        assert_eq!(out.sprite_id.as_deref(), Some(other));
    }

    #[test]
    fn fill_kernel_border_copy_follows_the_instance_nullable() {
        // `Border?.CopyValues`: versions without an instance Border skip it.
        let out = fill_sprite_metadata(
            [0.0, 0.0, 64.0, 32.0],
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
            false,
            false,
            false,
            false,
            false,
            false,
            false,
            0.5,
            &[],
            FILL_SPRITE_ID,
        );
        assert!(!out.copy_border);
        assert_eq!(out.border, [-3.0, 0.0, -25.0, -10.0]);
    }
}
