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
//! `sprite_outline::generate_mesh_outlines` ports the source `MeshOutlineGenerator`
//! over caller-provided vertices and triangles. The caller still owns versioned
//! `VertexData`/index-buffer extraction and the subsequent PPU, pivot, and atlas
//! packing transforms. `GeneratePhysicsShape` and the `SetBoneGeometry` body
//! (including its `VertexDataBlob` reads and vertex/index/weight copies) remain
//! open. The source itself carries a `#warning TODO` for `Edges`; it does not
//! generate that list.
//!
//! Also ported is the `SpriteProcessor.ProcessSprite` offset recalculation (pivot
//! offset and texture-rect offset over caller facts); the atlas detachment and the
//! rect/pivot/border copies stay caller-side. The `SettingsRaw` packing-settings
//! bit positions shared by `SpriteAtlasData` and `SpriteRenderData` are pinned as
//! raw discriminants; only the generated packing-enum member names stay open.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;

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

/// `SpriteProcessor.ProcessSprite` pivot offset over caller facts.
///
/// The offset in pixels of the pivot to the center of the sprite rect:
/// `(pivot - 0.5) * rect size`, in the source's exact operation order.
/// Consumes the atlas coordinates' rect size and pivot; the caller writes the
/// result back to the sprite `Offset`.
pub fn sprite_pivot_offset(pivot: [f32; 2], rect_size: [f32; 2]) -> [f32; 2] {
    [
        (pivot[0] - 0.5) * rect_size[0],
        (pivot[1] - 0.5) * rect_size[1],
    ]
}

/// `SpriteProcessor.ProcessSprite` texture-rect offset over caller facts.
///
/// The offset in pixels of the render-data texture rect to the sprite rect.
/// The caller writes the result back to `RD.TextureRectOffset`; the atlas
/// detachment (`SpriteAtlasP = null`) and the rect/pivot/border copies stay
/// caller-side with the generated sprite objects.
pub fn sprite_texture_rect_offset(
    texture_rect_origin: [f32; 2],
    rect_origin: [f32; 2],
) -> [f32; 2] {
    [
        texture_rect_origin[0] - rect_origin[0],
        texture_rect_origin[1] - rect_origin[1],
    ]
}

/// `SettingsRaw` packed flag shared by `SpriteAtlasData` and `SpriteRenderData`.
///
/// Both extensions read bit 0 (`IsPacked`); the packing-enum member names stay
/// open (see the module docs), so this pins the position, not the spelling.
pub fn sprite_settings_raw_is_packed(settings_raw: u32) -> bool {
    settings_raw & 1 != 0
}

/// `SettingsRaw` packing-mode discriminant: bit 1.
pub fn sprite_settings_raw_packing_mode(settings_raw: u32) -> u32 {
    (settings_raw >> 1) & 1
}

/// `SettingsRaw` packing-rotation discriminant: bits 2-5.
pub fn sprite_settings_raw_packing_rotation(settings_raw: u32) -> u32 {
    (settings_raw >> 2) & 0xF
}

/// `SettingsRaw` mesh-type discriminant: bit 6.
pub fn sprite_settings_raw_mesh_type(settings_raw: u32) -> u32 {
    (settings_raw >> 6) & 1
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

/// Failure while deriving outlines from sprite render-data triangles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpriteOutlineError {
    /// A non-degenerate input triangle refers to a vertex outside the supplied
    /// vertex array. `triangle_index` is its position in the input triangle list.
    InvalidVertexIndex {
        /// Position of the bad triangle in the input list.
        triangle_index: usize,
        /// Out-of-range vertex index.
        vertex_index: usize,
        /// Number of supplied vertices.
        vertex_count: usize,
    },
    /// The source algorithm found a connected triangle component with no
    /// boundary edge from which it can start an outline.
    NoBoundary {
        /// Position of the first triangle in the input component.
        triangle_index: usize,
    },
    /// The source boundary walk repeated a state without closing at its first
    /// vertex. The input cannot produce a finite source-compatible outline.
    TraversalCycle {
        /// Position of the first triangle in the input component.
        triangle_index: usize,
    },
}

impl fmt::Display for SpriteOutlineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidVertexIndex {
                triangle_index,
                vertex_index,
                vertex_count,
            } => write!(
                formatter,
                "sprite triangle {triangle_index} references vertex {vertex_index}, but only {vertex_count} vertices were supplied"
            ),
            Self::NoBoundary { triangle_index } => write!(
                formatter,
                "sprite triangle component starting at {triangle_index} has no outline boundary"
            ),
            Self::TraversalCycle { triangle_index } => write!(
                formatter,
                "sprite outline walk for component starting at {triangle_index} did not close"
            ),
        }
    }
}

impl std::error::Error for SpriteOutlineError {}

/// Generates the source `MeshOutlineGenerator` outline loops from 3D vertices
/// and triangle indices, projecting the final points to the XY plane.
///
/// Degenerate triangles (any repeated vertex index) are discarded, as in the
/// source constructor. Adjacency follows the source's directed shared-edge rule;
/// triangles that share only a vertex remain separate components. The output
/// order and boundary walk preserve source ordering. Coordinate scaling, pivot
/// translation, and atlas packing rotation are caller operations.
pub fn generate_mesh_outlines(
    vertices: &[[f32; 3]],
    triangles: &[[usize; 3]],
) -> Result<Vec<Vec<[f32; 2]>>, SpriteOutlineError> {
    let mut valid_triangles = Vec::with_capacity(triangles.len());
    let mut source_indices = Vec::with_capacity(triangles.len());
    for (triangle_index, triangle) in triangles.iter().copied().enumerate() {
        if triangle[0] == triangle[1] || triangle[0] == triangle[2] || triangle[1] == triangle[2] {
            continue;
        }
        for vertex_index in triangle {
            if vertex_index >= vertices.len() {
                return Err(SpriteOutlineError::InvalidVertexIndex {
                    triangle_index,
                    vertex_index,
                    vertex_count: vertices.len(),
                });
            }
        }
        valid_triangles.push(triangle);
        source_indices.push(triangle_index);
    }

    let mut directed_edges: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    let mut undirected_edges: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (triangle_index, triangle) in valid_triangles.iter().copied().enumerate() {
        for (start, end) in triangle_edges(triangle) {
            directed_edges
                .entry((start, end))
                .or_default()
                .push(triangle_index);
            undirected_edges
                .entry(undirected_edge(start, end))
                .or_default()
                .push(triangle_index);
        }
    }

    let mut adjacency = vec![Vec::new(); valid_triangles.len()];
    for (triangle_index, triangle) in valid_triangles.iter().copied().enumerate() {
        let mut neighbors = HashSet::new();
        for (start, end) in triangle_edges(triangle) {
            if let Some(matches) = directed_edges.get(&(end, start)) {
                neighbors.extend(
                    matches
                        .iter()
                        .copied()
                        .filter(|&other| other != triangle_index),
                );
            }
        }
        let mut neighbors: Vec<_> = neighbors.into_iter().collect();
        neighbors.sort_unstable();
        adjacency[triangle_index] = neighbors;
    }

    let mut visited = vec![false; valid_triangles.len()];
    let mut component_order = vec![usize::MAX; valid_triangles.len()];
    let mut outlines = Vec::new();
    let mut start = 0;
    while start < valid_triangles.len() {
        if visited[start] {
            start += 1;
            continue;
        }
        let mut component = Vec::new();
        let mut pending = VecDeque::from([start]);
        visited[start] = true;
        while let Some(triangle_index) = pending.pop_front() {
            component.push(triangle_index);
            for &neighbor in &adjacency[triangle_index] {
                if !visited[neighbor] {
                    visited[neighbor] = true;
                    pending.push_back(neighbor);
                }
            }
        }

        for (order, &triangle_index) in component.iter().enumerate() {
            component_order[triangle_index] = order;
        }
        let outsiders = generate_outsiders(
            &component,
            &component_order,
            &valid_triangles,
            &undirected_edges,
        );
        if outsiders.is_empty() {
            return Err(SpriteOutlineError::NoBoundary {
                triangle_index: source_indices[start],
            });
        }

        let outline = generate_outline_indices(
            &component,
            &outsiders,
            &valid_triangles,
            source_indices[start],
        )?;
        outlines.push(
            outline
                .into_iter()
                .map(|vertex_index| {
                    let vertex = vertices[vertex_index];
                    [vertex[0], vertex[1]]
                })
                .collect(),
        );
        for &triangle_index in &component {
            component_order[triangle_index] = usize::MAX;
        }
        start += 1;
    }
    Ok(outlines)
}

#[derive(Clone, Copy)]
struct SpriteOutlineOutside {
    triangle: usize,
    member: usize,
}

fn triangle_edges(triangle: [usize; 3]) -> [(usize, usize); 3] {
    [
        (triangle[0], triangle[1]),
        (triangle[1], triangle[2]),
        (triangle[2], triangle[0]),
    ]
}

fn undirected_edge(first: usize, second: usize) -> (usize, usize) {
    if first < second {
        (first, second)
    } else {
        (second, first)
    }
}

fn generate_outsiders(
    component: &[usize],
    component_order: &[usize],
    triangles: &[[usize; 3]],
    undirected_edges: &HashMap<(usize, usize), Vec<usize>>,
) -> Vec<SpriteOutlineOutside> {
    let mut outsiders = Vec::new();
    for &triangle_index in component {
        let triangle = triangles[triangle_index];
        let mut candidates = HashSet::new();
        for (first, second) in triangle_edges(triangle) {
            if let Some(matches) = undirected_edges.get(&undirected_edge(first, second)) {
                candidates.extend(matches.iter().copied().filter(|candidate| {
                    *candidate != triangle_index && component_order[*candidate] != usize::MAX
                }));
            }
        }
        let mut candidates: Vec<_> = candidates.into_iter().collect();
        candidates.sort_unstable_by_key(|&candidate| component_order[candidate]);

        let mut neighbor_count = 0;
        let (mut xy, mut yz, mut zx) = (true, true, true);
        let mut has_three_neighbors = false;
        for candidate in candidates {
            let check = triangles[candidate];
            if check.contains(&triangle[0]) {
                if check.contains(&triangle[1]) {
                    xy = false;
                } else if check.contains(&triangle[2]) {
                    zx = false;
                } else {
                    continue;
                }
            } else if check.contains(&triangle[1]) && check.contains(&triangle[2]) {
                yz = false;
            } else {
                continue;
            }
            neighbor_count += 1;
            if neighbor_count == 3 {
                has_three_neighbors = true;
                break;
            }
        }
        if has_three_neighbors {
            continue;
        }

        let member = if xy {
            if zx {
                2
            } else {
                0
            }
        } else if yz {
            1
        } else {
            2
        };
        outsiders.push(SpriteOutlineOutside {
            triangle: triangle_index,
            member,
        });
    }
    outsiders
}

fn generate_outline_indices(
    component: &[usize],
    outsiders: &[SpriteOutlineOutside],
    triangles: &[[usize; 3]],
    source_triangle_index: usize,
) -> Result<Vec<usize>, SpriteOutlineError> {
    let first_outside = outsiders[0];
    let first_triangle = triangles[first_outside.triangle];
    let first = first_triangle[first_outside.member];
    let second = first_triangle[(first_outside.member + 1) % 3];
    let mut outline = vec![first, second];
    let mut last_triangle = first_outside.triangle;
    let mut last_member = first_outside.member + 1;
    let mut last_vertex = first;
    let mut visited = HashSet::new();

    loop {
        let state = (last_triangle, last_member % 3, last_vertex);
        if !visited.insert(state) {
            return Err(SpriteOutlineError::TraversalCycle {
                triangle_index: source_triangle_index,
            });
        }

        if let Some(outside) =
            get_next_outsider(last_triangle, last_member, component, outsiders, triangles)
        {
            last_triangle = outside.triangle;
            last_member = outside.member + 1;
            last_vertex = triangles[outside.triangle][outside.member];
        } else {
            last_member += 1;
            if triangles[last_triangle][last_member % 3] == last_vertex && last_vertex != first {
                break;
            }
        }

        let next_vertex = triangles[last_triangle][last_member % 3];
        if next_vertex == first {
            break;
        }
        outline.push(next_vertex);
    }
    Ok(outline)
}

fn get_next_outsider(
    triangle_index: usize,
    member: usize,
    component: &[usize],
    outsiders: &[SpriteOutlineOutside],
    triangles: &[[usize; 3]],
) -> Option<SpriteOutlineOutside> {
    let vertex = triangles[triangle_index][member % 3];
    outsiders.iter().copied().find(|outside| {
        triangles[outside.triangle][outside.member] == vertex
            && is_connected_neighbors(
                triangle_index,
                outside.triangle,
                vertex,
                component,
                triangles,
            )
    })
}

fn is_connected_neighbors(
    first_triangle: usize,
    target_triangle: usize,
    vertex: usize,
    component: &[usize],
    triangles: &[[usize; 3]],
) -> bool {
    if has_reverse_edge(triangles[first_triangle], triangles[target_triangle]) {
        return true;
    }

    let mut current = first_triangle;
    let mut visited = HashSet::new();
    while let Some(next) = get_next_neighbor(current, vertex, component, triangles) {
        if triangles[next] == triangles[target_triangle] {
            return true;
        }
        if !visited.insert(next) {
            return false;
        }
        current = next;
    }
    false
}

fn get_next_neighbor(
    triangle_index: usize,
    vertex: usize,
    component: &[usize],
    triangles: &[[usize; 3]],
) -> Option<usize> {
    let triangle = triangles[triangle_index];
    let member = triangle.iter().position(|&value| value == vertex)?;
    let next_vertex = triangle[(member + 1) % 3];
    component.iter().copied().find(|&candidate| {
        let check = triangles[candidate];
        check != triangle && check.contains(&vertex) && check.contains(&next_vertex)
    })
}

fn has_reverse_edge(first: [usize; 3], second: [usize; 3]) -> bool {
    triangle_edges(first).into_iter().any(|(start, end)| {
        triangle_edges(second)
            .into_iter()
            .any(|(other_start, other_end)| start == other_end && end == other_start)
    })
}

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
/// version carries an outline list. [`generate_mesh_outlines`] implements its
/// triangle-walk core; the caller still extracts the versioned geometry and
/// applies the PPU, pivot, and packing transforms.
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
/// Every field is a value to write or a gated action to run. Render-data
/// extraction, physics-shape generation, and bone-geometry extraction remain
/// caller-side.
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
    fn mesh_outlines_match_the_frozen_source_generator() {
        let vertices = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [2.0, 0.0, 0.0],
            [2.0, 1.0, 0.0],
        ];

        // The frozen MeshOutlineGenerator.cs and Vector3i.cs were compiled
        // unchanged in a small .NET harness. These cover a single face, a
        // joined strip, separate and vertex-touching components, same-winding
        // duplicates, and oppositely-wound coincident faces.
        assert_eq!(
            generate_mesh_outlines(&vertices, &[[0, 1, 2]]),
            Ok(vec![vec![[1.0, 1.0], [0.0, 0.0], [1.0, 0.0]]]),
        );
        assert_eq!(
            generate_mesh_outlines(&vertices, &[[0, 1, 2], [0, 2, 3]]),
            Ok(vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0],]]),
        );
        assert_eq!(
            generate_mesh_outlines(&vertices, &[[0, 1, 2], [0, 2, 3], [1, 4, 5], [1, 5, 2]],),
            Ok(vec![vec![
                [0.0, 0.0],
                [1.0, 0.0],
                [2.0, 0.0],
                [2.0, 1.0],
                [1.0, 1.0],
                [0.0, 1.0],
            ]]),
        );
        assert_eq!(
            generate_mesh_outlines(&vertices, &[[0, 1, 2], [3, 4, 5]]),
            Ok(vec![
                vec![[1.0, 1.0], [0.0, 0.0], [1.0, 0.0]],
                vec![[2.0, 1.0], [0.0, 1.0], [2.0, 0.0]],
            ]),
        );
        assert_eq!(
            generate_mesh_outlines(&vertices, &[[0, 1, 2], [0, 1, 2]]),
            Ok(vec![
                vec![[1.0, 1.0], [0.0, 0.0], [1.0, 0.0]],
                vec![[1.0, 1.0], [0.0, 0.0], [1.0, 0.0]],
            ]),
        );
        assert_eq!(
            generate_mesh_outlines(&vertices, &[[0, 1, 2], [2, 1, 0]]),
            Ok(vec![vec![[1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]),
        );
        assert_eq!(
            generate_mesh_outlines(&vertices, &[[0, 1, 2], [0, 3, 4]]),
            Ok(vec![
                vec![[1.0, 1.0], [0.0, 0.0], [1.0, 0.0]],
                vec![[2.0, 0.0], [0.0, 0.0], [0.0, 1.0]],
            ]),
        );
    }

    #[test]
    fn mesh_outlines_reject_bad_indices_and_closed_components() {
        assert_eq!(
            generate_mesh_outlines(&[[0.0, 0.0, 0.0]], &[[0, 1, 2]]),
            Err(SpriteOutlineError::InvalidVertexIndex {
                triangle_index: 0,
                vertex_index: 1,
                vertex_count: 1,
            }),
        );

        let tetrahedron = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ];
        assert_eq!(
            generate_mesh_outlines(&tetrahedron, &[[0, 2, 1], [0, 1, 3], [1, 2, 3], [2, 0, 3]],),
            Err(SpriteOutlineError::NoBoundary { triangle_index: 0 }),
        );
    }

    #[test]
    fn mesh_outline_discards_degenerate_triangles_before_index_validation() {
        assert_eq!(
            generate_mesh_outlines(&[], &[[usize::MAX, 2, usize::MAX]]),
            Ok(Vec::new()),
        );
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
    fn processor_offsets_follow_the_pivot_and_rect_origins() {
        // Centered pivot: no offset whatever the rect size.
        assert_eq!(sprite_pivot_offset([0.5, 0.5], [64.0, 32.0]), [0.0, 0.0]);
        // Corner pivots shift by half the rect size, in exact source order.
        assert_eq!(
            sprite_pivot_offset([0.0, 0.0], [64.0, 32.0]),
            [-32.0, -16.0]
        );
        assert_eq!(sprite_pivot_offset([1.0, 1.0], [10.0, 20.0]), [5.0, 10.0]);
        // Texture-rect offset is the origin difference.
        assert_eq!(
            sprite_texture_rect_offset([1.0, 2.0], [10.0, 20.0]),
            [-9.0, -18.0]
        );
        assert_eq!(
            sprite_texture_rect_offset([4.0, 4.0], [4.0, 4.0]),
            [0.0, 0.0]
        );
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

    /// Byte-for-byte differential against the shipped `SpriteExtensions`
    /// over live generated objects (probe `Temp/opencode/spriteprobe`): real
    /// `Sprite` instances (2022.3, 5.6, 4.5) built through the generated
    /// `Create` API with `m_` backing fields, plus a real `SpriteAtlas`
    /// whose `RenderDataMap` is empty or holds the sprite's own key. All six
    /// probed cases (RD path, stored-zero pivot, atlas miss with
    /// zero-preserving border, atlas hit with border shift, key-less 5.6
    /// sprite, derived-pivot 4.5 sprite) must reproduce rect, pivot, and
    /// border exactly, with the probed `Has_*` version gates selecting the
    /// stored-vs-derived branches.
    #[test]
    fn atlas_coordinates_match_the_reference_runtime() {
        fn bits_to_f32(text: &str) -> f32 {
            f32::from_bits(u32::from_str_radix(text, 16).expect("float bits"))
        }

        fn parse_floats(text: &str) -> Vec<f32> {
            if text == "-" {
                return Vec::new();
            }
            text.split(',').map(bits_to_f32).collect()
        }

        let fixture = include_str!("sprite_atlas_fixture.txt");
        let mut gates: Vec<(String, bool, bool)> = Vec::new();
        struct Case {
            tag: String,
            version: String,
            atlas_hit: bool,
            rect: [f32; 4],
            offset: [f32; 2],
            pivot: Vec<f32>,
            border: Vec<f32>,
            rd_rect: [f32; 4],
            rd_offset: [f32; 2],
            hit_rect: [f32; 4],
            hit_offset: [f32; 2],
            expected_rect: [f32; 4],
            expected_pivot: [f32; 2],
            expected_border: [f32; 4],
        }
        let mut cases: Vec<Case> = Vec::new();
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(rest) = line.strip_prefix("SGATE|") {
                let mut parts = rest.split('|');
                let version = parts.next().expect("a version").to_owned();
                let flags = parts.next().expect("gate flags");
                let mut has_pivot = false;
                let mut has_border = false;
                for flag in flags.split(',') {
                    let mut kv = flag.split('=');
                    match (kv.next(), kv.next()) {
                        (Some("Has_Pivot"), Some(value)) => has_pivot = value == "True",
                        (Some("Has_Border"), Some(value)) => has_border = value == "True",
                        _ => {}
                    }
                }
                gates.push((version, has_pivot, has_border));
                continue;
            }
            let mut parts = line.splitn(4, '|');
            assert_eq!(parts.next(), Some("SCASE"), "{line}");
            let tag = parts.next().expect("a probe tag");
            if cases.is_empty() || cases.last().expect("a case").tag != tag {
                cases.push(Case {
                    tag: tag.to_owned(),
                    version: String::new(),
                    atlas_hit: false,
                    rect: [0.0; 4],
                    offset: [0.0; 2],
                    pivot: Vec::new(),
                    border: Vec::new(),
                    rd_rect: [0.0; 4],
                    rd_offset: [0.0; 2],
                    hit_rect: [0.0; 4],
                    hit_offset: [0.0; 2],
                    expected_rect: [0.0; 4],
                    expected_pivot: [0.0; 2],
                    expected_border: [0.0; 4],
                });
            }
            let case = cases.last_mut().expect("a case");
            match parts.next().expect("a row kind") {
                "in" => {
                    let mut fields = parts.next().expect("input fields").split('|');
                    case.version = fields.next().expect("a version").to_owned();
                    fields.next().expect("an atlas flag");
                    case.atlas_hit = fields.next().expect("a map flag") == "1";
                    let rect = parse_floats(fields.next().expect("a rect"));
                    case.rect = [rect[0], rect[1], rect[2], rect[3]];
                    let offset = parse_floats(fields.next().expect("an offset"));
                    case.offset = [offset[0], offset[1]];
                    case.pivot = parse_floats(fields.next().expect("a pivot"));
                    case.border = parse_floats(fields.next().expect("a border"));
                    let rd_rect = parse_floats(fields.next().expect("an RD rect"));
                    case.rd_rect = [rd_rect[0], rd_rect[1], rd_rect[2], rd_rect[3]];
                    let rd_offset = parse_floats(fields.next().expect("an RD offset"));
                    case.rd_offset = [rd_offset[0], rd_offset[1]];
                    let hit_rect = parse_floats(fields.next().expect("a hit rect"));
                    case.hit_rect = [hit_rect[0], hit_rect[1], hit_rect[2], hit_rect[3]];
                    let hit_offset = parse_floats(fields.next().expect("a hit offset"));
                    case.hit_offset = [hit_offset[0], hit_offset[1]];
                }
                "ok" => assert_eq!(parts.next(), Some("1"), "{line}"),
                "rect" => {
                    let mut fields = parts.next().expect("rect fields").split('|');
                    case.expected_rect = [
                        bits_to_f32(fields.next().expect("x")),
                        bits_to_f32(fields.next().expect("y")),
                        bits_to_f32(fields.next().expect("w")),
                        bits_to_f32(fields.next().expect("h")),
                    ];
                }
                "pivot" => {
                    let mut fields = parts.next().expect("pivot fields").split('|');
                    case.expected_pivot = [
                        bits_to_f32(fields.next().expect("x")),
                        bits_to_f32(fields.next().expect("y")),
                    ];
                }
                "border" => {
                    let mut fields = parts.next().expect("border fields").split('|');
                    case.expected_border = [
                        bits_to_f32(fields.next().expect("l")),
                        bits_to_f32(fields.next().expect("b")),
                        bits_to_f32(fields.next().expect("r")),
                        bits_to_f32(fields.next().expect("t")),
                    ];
                }
                other => panic!("unexpected row kind {other}"),
            }
        }
        assert_eq!(cases.len(), 6, "all probed atlas cases are asserted");
        for case in &cases {
            let (has_pivot, has_border) = gates
                .iter()
                .find(|(version, _, _)| *version == case.version)
                .map(|(_, has_pivot, has_border)| (*has_pivot, *has_border))
                .expect("a probed version gate");
            let pivot = if has_pivot {
                if case.pivot.is_empty() {
                    // Stored pivot left at its default: the reference reads (0, 0).
                    [0.0, 0.0]
                } else {
                    assert_eq!(case.pivot.len(), 2, "{tag}", tag = case.tag);
                    [case.pivot[0], case.pivot[1]]
                }
            } else {
                assert!(case.pivot.is_empty(), "{tag}", tag = case.tag);
                [0.0, 0.0]
            };
            let border = if has_border {
                if case.border.is_empty() {
                    [0.0, 0.0, 0.0, 0.0]
                } else {
                    [
                        case.border[0],
                        case.border[1],
                        case.border[2],
                        case.border[3],
                    ]
                }
            } else {
                [0.0, 0.0, 0.0, 0.0]
            };
            let out = get_sprite_coordinates_in_atlas(
                case.rect,
                case.rd_rect,
                case.rd_offset,
                case.atlas_hit,
                case.hit_rect,
                case.hit_offset,
                has_pivot,
                pivot,
                case.offset,
                has_border,
                border,
            );
            assert_eq!(out.rect, case.expected_rect, "{tag}", tag = case.tag);
            assert_eq!(out.pivot, case.expected_pivot, "{tag}", tag = case.tag);
            assert_eq!(out.border, case.expected_border, "{tag}", tag = case.tag);
        }
    }

    /// Differential against the shipped `SpriteConverter` over live generated
    /// objects (probe `Temp/opencode/spriteprobe`, fixture
    /// `sprite_convert_fixture.txt`): a real 4x4 RGBA32 texture registered in
    /// a `ProcessedAssetCollection` plus real sprites whose `RD.Texture` PPtr
    /// links (FileID 0, PathID 44), nulls (PathID 0), or dangles (PathID 999).
    /// The replay feeds the probed resolution facts through `sprite_supported`
    /// and `sprite_try_convert_to_bitmap`, delegating the linked texture bytes
    /// through the real `try_convert_to_bitmap` (flip on, no unpack, like the
    /// source's non-cubemap path), and must reproduce both decisions plus
    /// every output pixel.
    #[test]
    fn sprite_conversion_matches_the_reference_runtime() {
        use crate::resources::texture_converter::{try_convert_to_bitmap, TextureImage};

        fn decode_hex(text: &str) -> Vec<u8> {
            text.as_bytes()
                .chunks_exact(2)
                .map(|pair| {
                    let digit = |c: u8| (c as char).to_digit(16).expect("hex digit") as u8;
                    digit(pair[0]) << 4 | digit(pair[1])
                })
                .collect()
        }

        let fixture = include_str!("sprite_convert_fixture.txt");
        let mut input: Vec<u8> = Vec::new();
        struct Case {
            tag: String,
            path_id: i64,
            supported: bool,
            ok: bool,
            width: usize,
            height: usize,
            depth: usize,
            expected: Vec<u8>,
        }
        let mut cases: Vec<Case> = Vec::new();
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.splitn(4, '|');
            assert_eq!(parts.next(), Some("CCONV"), "{line}");
            let tag = parts.next().expect("a probe tag");
            if tag == "texinput" {
                assert_eq!(parts.next(), Some("4"), "{line}");
                let mut fields = parts.next().expect("input fields").split('|');
                assert_eq!(fields.next(), Some("4"), "{line}");
                input = decode_hex(fields.next().expect("input hex"));
                assert_eq!(input.len(), 64, "{line}");
                continue;
            }
            if cases.is_empty() || cases.last().expect("a case").tag != tag {
                cases.push(Case {
                    tag: tag.to_owned(),
                    path_id: -1,
                    supported: false,
                    ok: false,
                    width: 0,
                    height: 0,
                    depth: 0,
                    expected: Vec::new(),
                });
            }
            let case = cases.last_mut().expect("a case");
            match parts.next().expect("a row kind") {
                "pptr" => {
                    let mut fields = parts.next().expect("pptr fields").split('|');
                    assert_eq!(fields.next(), Some("0"), "{line}");
                    case.path_id = fields.next().expect("a path ID").parse().unwrap();
                }
                "supported" => case.supported = parts.next() == Some("1"),
                "ok" => case.ok = parts.next() == Some("1"),
                "size" => {
                    let mut fields = parts.next().expect("size fields").split('|');
                    case.width = fields.next().expect("a width").parse().unwrap();
                    case.height = fields.next().expect("a height").parse().unwrap();
                    case.depth = fields.next().expect("a depth").parse().unwrap();
                }
                "pixels" => case.expected = decode_hex(parts.next().expect("pixel hex")),
                other => panic!("unexpected row kind {other}"),
            }
        }
        assert_eq!(cases.len(), 3, "all probed sprite conversions are asserted");
        for case in &cases {
            // FileID 0 resolves inside the sprite's own collection; only the
            // registered texture PathID (44) is present there.
            let has_texture = case.path_id == 44;
            // Inline RGBA32 bytes are present and nonempty, so the texture
            // passes `CheckAssetIntegrity` exactly when it resolves.
            let texture_integrity = has_texture;
            assert_eq!(
                sprite_supported(has_texture, texture_integrity, false),
                case.supported,
                "{tag}",
                tag = case.tag,
            );
            let delegate_ok = if has_texture {
                let image = TextureImage {
                    texture_format: 4, // RGBA32, like the probed texture.
                    graphics_format: 0,
                    width: 4,
                    height: 4,
                    depth: 1,
                    image_size: input.len(),
                    data: &input,
                };
                match try_convert_to_bitmap(&image, true, false) {
                    Ok(bitmap) => {
                        assert_eq!(bitmap.width, case.width, "{tag}", tag = case.tag);
                        assert_eq!(bitmap.height, case.height, "{tag}", tag = case.tag);
                        assert_eq!(bitmap.depth, case.depth, "{tag}", tag = case.tag);
                        assert_eq!(bitmap.pixels, case.expected, "{tag}", tag = case.tag);
                        true
                    }
                    Err(error) => panic!("{tag} delegate failed: {error}", tag = case.tag),
                }
            } else {
                assert!(case.expected.is_empty(), "{tag}", tag = case.tag);
                false
            };
            assert_eq!(
                sprite_try_convert_to_bitmap(has_texture, delegate_ok, false),
                case.ok,
                "{tag}",
                tag = case.tag,
            );
        }
    }

    #[test]
    fn settings_raw_decodes_the_shared_packing_positions() {
        // Bit 0 is the packed flag; bit 1 the mode; bits 2-5 the rotation; bit 6
        // the mesh type. Higher bits and the enum spellings stay out of scope.
        assert_eq!(
            (
                sprite_settings_raw_is_packed(0),
                sprite_settings_raw_packing_mode(0),
                sprite_settings_raw_packing_rotation(0),
                sprite_settings_raw_mesh_type(0),
            ),
            (false, 0, 0, 0)
        );
        assert_eq!(
            (
                sprite_settings_raw_is_packed(0x28),
                sprite_settings_raw_packing_mode(0x28),
                sprite_settings_raw_packing_rotation(0x28),
                sprite_settings_raw_mesh_type(0x28),
            ),
            (false, 0, 10, 0)
        );
        assert_eq!(
            (
                sprite_settings_raw_is_packed(0x5F),
                sprite_settings_raw_packing_mode(0x5F),
                sprite_settings_raw_packing_rotation(0x5F),
                sprite_settings_raw_mesh_type(0x5F),
            ),
            (true, 1, 7, 1)
        );
        assert_eq!(
            (
                sprite_settings_raw_is_packed(u32::MAX),
                sprite_settings_raw_packing_mode(u32::MAX),
                sprite_settings_raw_packing_rotation(u32::MAX),
                sprite_settings_raw_mesh_type(u32::MAX),
            ),
            (true, 1, 15, 1)
        );
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

    /// Differential against the shipped `FillSpriteMetaData` over live
    /// generated objects (probe `Temp/opencode/smetaprobe`, fixture
    /// `sprite_metadata_fixture.txt`): real 2022.3 sprites (plain, boned,
    /// atlas-hit, atlas-miss) with `SpriteMetaData.Create` instances. The
    /// replay feeds the probed atlas inputs (fixed per probe design, pinned
    /// in the fixture header) plus the probed gates, PPU, and input bones
    /// through `fill_sprite_metadata` and must reproduce every filled field:
    /// the atlas/miss rect, pivot, and border, `Custom` alignment, zero
    /// tessellation, the border copy, the gate decisions, the PPU-scaled
    /// bones with the sprite-Rect root offset, and the carried sprite ID.
    /// The outline/physics/vertex/index/weight bodies stay open; on these
    /// vertex-less sprites the reference leaves them empty, which the replay
    /// asserts as the empty result of the open bodies.
    #[test]
    fn sprite_metadata_fill_matches_the_reference_runtime() {
        fn word(text: &str) -> f32 {
            f32::from_bits(u32::from_str_radix(text, 16).expect("hex word"))
        }

        fn bit(flag: &str) -> bool {
            match flag {
                "0" => false,
                "1" => true,
                other => panic!("unexpected bit {other}"),
            }
        }

        struct Case {
            tag: String,
            atlas_hit: bool,
            instance_gates: [bool; 4],
            sprite_gates: [bool; 2],
            ppu: f32,
            rect_size: [f32; 2],
            input_bones: Vec<SpriteBoneFacts>,
            name: String,
            rect: [f32; 4],
            pivot: [f32; 2],
            border: Option<[f32; 4]>,
            tessellation: i32,
            outline: usize,
            physics: usize,
            bones: usize,
            sprite_id: bool,
            vertices: usize,
            indices: usize,
            weights: usize,
            output_bones: Vec<SpriteBoneFacts>,
        }

        let fixture = include_str!("sprite_metadata_fixture.txt");
        let mut cases: Vec<Case> = Vec::new();
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut cells = line.split('|');
            let prefix = cells.next().expect("a prefix");
            let tag = cells.next().expect("a case").to_owned();
            if cases.is_empty() || cases.last().expect("a case").tag != tag {
                cases.push(Case {
                    tag: tag.clone(),
                    atlas_hit: tag.ends_with("hit"),
                    instance_gates: [false; 4],
                    sprite_gates: [false; 2],
                    ppu: 0.0,
                    rect_size: [0.0; 2],
                    input_bones: Vec::new(),
                    name: String::new(),
                    rect: [0.0; 4],
                    pivot: [0.0; 2],
                    border: None,
                    tessellation: 0,
                    outline: 0,
                    physics: 0,
                    bones: 0,
                    sprite_id: false,
                    vertices: 0,
                    indices: 0,
                    weights: 0,
                    output_bones: Vec::new(),
                });
            }
            let case = cases.last_mut().expect("a case");
            if prefix == "SPPU" {
                case.ppu = word(cells.next().expect("ppu"));
                case.rect_size = [
                    word(cells.next().expect("width")),
                    word(cells.next().expect("height")),
                ];
                continue;
            }
            if prefix == "SBONEIN" || prefix == "SBONE" {
                let index: usize = cells.next().expect("an index").parse().expect("index");
                let bone = SpriteBoneFacts {
                    parent_id: cells.next().expect("a parent").parse().expect("parent"),
                    position: [
                        word(cells.next().expect("x")),
                        word(cells.next().expect("y")),
                    ],
                    length: word(cells.next().expect("length")),
                };
                if prefix == "SBONEIN" {
                    assert_eq!(index, case.input_bones.len(), "{line}");
                    case.input_bones.push(bone);
                } else {
                    assert_eq!(index, case.output_bones.len(), "{line}");
                    case.output_bones.push(bone);
                }
                continue;
            }
            assert_eq!(prefix, "SMETA", "{line}");
            match cells.next().expect("a row kind") {
                "smetaclass" => {}
                "sgate" => {
                    for (slot, cell) in cells.enumerate() {
                        case.instance_gates[slot] = bit(cell);
                    }
                }
                "sprgate" => {
                    for (slot, cell) in cells.enumerate() {
                        case.sprite_gates[slot] = bit(cell);
                    }
                }
                "entry" => {
                    case.name = cells.next().expect("a name").to_owned();
                    case.rect = [
                        word(cells.next().expect("rx")),
                        word(cells.next().expect("ry")),
                        word(cells.next().expect("rw")),
                        word(cells.next().expect("rh")),
                    ];
                    assert_eq!(
                        cells.next().expect("alignment"),
                        SPRITE_ALIGNMENT_CUSTOM.to_string()
                    );
                    case.pivot = [
                        word(cells.next().expect("px")),
                        word(cells.next().expect("py")),
                    ];
                    let border = cells.next().expect("border");
                    case.border = if border == "null" {
                        None
                    } else {
                        Some([
                            word(border),
                            word(cells.next().expect("b1")),
                            word(cells.next().expect("b2")),
                            word(cells.next().expect("b3")),
                        ])
                    };
                    case.tessellation = cells
                        .next()
                        .expect("tessellation")
                        .parse()
                        .expect("tessellation int");
                    assert_eq!(case.tessellation, SPRITE_TESSELLATION_DETAIL);
                    case.outline = cells.next().expect("outline").parse().expect("count");
                    case.physics = cells.next().expect("physics").parse().expect("count");
                    case.bones = cells.next().expect("bones").parse().expect("count");
                    case.sprite_id = bit(cells.next().expect("sprite id"));
                    case.vertices = cells.next().expect("vertices").parse().expect("count");
                    case.indices = cells.next().expect("indices").parse().expect("count");
                    case.weights = cells.next().expect("weights").parse().expect("count");
                }
                _ => panic!("unexpected row {line}"),
            }
        }
        assert_eq!(cases.len(), 3, "all probed fill cases are asserted");
        for case in &cases {
            let out = fill_sprite_metadata(
                [10.0, 20.0, 100.0, 50.0],
                [0.0, 0.0, 64.0, 64.0],
                [2.0, 3.0],
                case.atlas_hit,
                [5.0, 6.0, 32.0, 32.0],
                [0.0, 1.0],
                true,
                [0.25, 0.75],
                [1.0, -2.0],
                true,
                [4.0, 5.0, 6.0, 7.0],
                case.border.is_some(),
                case.instance_gates[0],
                case.instance_gates[1],
                case.sprite_gates[1],
                case.instance_gates[2],
                case.sprite_gates[0],
                case.instance_gates[3],
                case.ppu,
                &case.input_bones,
                "probe-id",
            );
            assert_eq!(out.rect, case.rect, "{tag}", tag = case.tag);
            assert_eq!(out.pivot, case.pivot, "{tag}", tag = case.tag);
            assert_eq!(
                out.alignment,
                SPRITE_ALIGNMENT_CUSTOM,
                "{tag}",
                tag = case.tag
            );
            assert_eq!(
                out.tessellation_detail,
                SPRITE_TESSELLATION_DETAIL,
                "{tag}",
                tag = case.tag
            );
            match case.border {
                Some(border) => {
                    assert!(out.copy_border, "{tag}", tag = case.tag);
                    assert_eq!(out.border, border, "{tag}", tag = case.tag);
                }
                None => assert!(!out.copy_border, "{tag}", tag = case.tag),
            }
            assert_eq!(
                out.generate_outline,
                case.instance_gates[0],
                "{tag}",
                tag = case.tag
            );
            assert_eq!(
                out.generate_physics_shape,
                case.instance_gates[1] && case.sprite_gates[1],
                "{tag}",
                tag = case.tag
            );
            let fill_bones =
                case.instance_gates[2] && case.sprite_gates[0] && case.instance_gates[3];
            assert_eq!(out.fill_bones, fill_bones, "{tag}", tag = case.tag);
            assert_eq!(out.scaled_bones.len(), case.bones, "{tag}", tag = case.tag);
            for (scaled, expected) in out.scaled_bones.iter().zip(&case.output_bones) {
                assert_eq!(scaled, expected, "{tag}", tag = case.tag);
            }
            // The source mints a random GUID; the kernel carries the caller
            // ID, so the replay pins the assignment arm, not the value.
            assert_eq!(
                out.sprite_id.is_some(),
                case.sprite_id,
                "{tag}",
                tag = case.tag
            );
            if case.sprite_id {
                assert_eq!(
                    out.sprite_id.as_deref(),
                    Some("probe-id"),
                    "{tag}",
                    tag = case.tag
                );
            }
            // The vertex-less reference leaves every open body empty.
            assert_eq!(case.outline, 0, "{tag}", tag = case.tag);
            assert_eq!(case.physics, 0, "{tag}", tag = case.tag);
            assert_eq!(case.vertices, 0, "{tag}", tag = case.tag);
            assert_eq!(case.indices, 0, "{tag}", tag = case.tag);
            assert_eq!(case.weights, 0, "{tag}", tag = case.tag);
            assert_eq!(case.name, "ProbeSprite", "{tag}", tag = case.tag);
        }
    }
}
