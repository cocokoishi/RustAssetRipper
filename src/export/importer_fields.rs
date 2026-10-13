//! Importer field values ported from the frozen baseline's `CreateImporter` overrides.
//!
//! Every export collection writes its `.meta` sidecar around a generated importer object.
//! The importer classes themselves come from the external `AssetRipper.SourceGenerated`
//! package, so this module ports only what the snapshot determines exactly:
//!
//! - The `AssetBundleName` propagation shared by eight importers: when the generated importer
//!   exposes the field (`Has_AssetBundleName_R`) and the source asset has a non-null bundle
//!   name, the name is copied verbatim. The source checks null only, so an empty string would
//!   propagate too; that case is preserved rather than "fixed".
//! - The unconditional literal defaults of `ImporterFactory.GenerateTextureImporter`: mipmap
//!   fades, bump height scale, compression quality, sprite geometry defaults, alpha handling,
//!   cubemap scaling, and the single `DefaultTexturePlatform` platform-settings block.
//! - The `PluginImporter` platform settings written by `ScriptExportCollection.OnAssemblyExported`.
//! - The `PrefabExportCollection` importer branch: a legacy prefab marker selects the native
//!   format importer, otherwise a `PrefabImporter`.
//! - The `CreateImporter` dispatch table: the base `AssetExportCollection<T>` default plus all
//!   nine overrides found by source search, with each collection mapped to its importer kind
//!   and factory. Asset-copied fields and generated `Has_*` gates stay open; only the
//!   dispatch shape and the unconditional literals below are pinned.
//! - The `MonoImporter` execution-order narrowing: `(short)script.ExecutionOrder`, which wraps
//!   exactly like Rust's `as` conversion because C# explicit conversions are unchecked by default.
//! - The `TextureExportCollection.AddSprites` importer decisions: Single versus Multiple mode
//!   (with the shipped `SpriteImportMode`/`SpriteAlignment` discriminants read off the
//!   generated DLL by reflection), the sprite-supplied extrude/mesh-type/pivot/border/
//!   pixels-per-unit, and the sprite-sheet build gate. Version-gated application to a
//!   generated importer object stays caller-side.
//!
//! Everything that copies asset state (`TextureImporterData` texture reads, font metrics,
//! audio settings, video dimensions, shader texture maps) or branches on generated
//! `Has_*` version gates stays open: the member lists and the gate thresholds live in the
//! generated package, and porting them from memory would be guessing.

use std::error::Error;
use std::io;

use crate::export::ids::get_main_export_id;
use crate::export::unity_yaml::{UnityYamlNode, UnityYamlNodeKind, UnityYamlScalar};
use crate::numerics::BoneWeight4;
use crate::resources::mesh::{MeshVertexBytesSource, MeshView};
use crate::resources::sprite::{
    generate_sprite_render_data_outlines_from_value, sprite_settings_raw_is_packed,
    sprite_settings_raw_packing_rotation, transform_sprite_outline_points, SpriteOutlineTransform,
};
use crate::resources::vertex_data_blob::{EndianType, UnityVersion, VertexDataBlob};
use crate::serialized_file::object::SerializedValue;
use crate::serialized_file::object_workspace::ObjectWorkspace;
use crate::serialized_file::reference_graph::{MultiFilePPtrResolution, MultiFileReferenceGraph};
use crate::serialized_file::workspace::SerializedFileWorkspace;

/// Copy an asset bundle name onto an importer exactly like the eight baseline overrides.
///
/// The copy happens only when the generated importer exposes the field and the source name is
/// non-null. An empty-but-present name propagates, matching the source null check.
pub fn propagate_asset_bundle_name(
    importer_has_field: bool,
    source_bundle_name: Option<&str>,
) -> Option<String> {
    if importer_has_field {
        source_bundle_name.map(str::to_owned)
    } else {
        None
    }
}

/// Truncate a script execution order like `(short)script.ExecutionOrder`.
///
/// Both C# unchecked explicit conversions and Rust `as` casts wrap modulo 2^16.
pub fn truncate_execution_order(script_execution_order: i32) -> i16 {
    script_execution_order as i16
}

/// Which importer `PrefabExportCollection.CreateImporter` selects.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PrefabImporterBranch {
    /// A legacy prefab marker: the base native format importer.
    LegacyPrefabAsset,
    /// A modern prefab instance: a `PrefabImporter` carrying the root bundle name.
    ModernPrefabImporter,
}

/// Baseline `SpriteImportMode` discriminants (read off the shipped
/// `AssetRipper.SourceGenerated.dll` with a reflection probe: None 0, Single 1,
/// Multiple 2, Polygon 3). `AddSprites` writes only Single and Multiple.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SpriteImportMode {
    /// One sprite (or none): per-sprite sheet of exactly one entry.
    Single = 1,
    /// Several sprites, or one sprite that does not cover its texture.
    Multiple = 2,
}

/// Baseline `SpriteAlignment` discriminants used by `AddSprites` (Center 0,
/// Custom 9 in the shipped enums).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SpriteAlignment {
    /// The default and multi-sprite alignment.
    Center = 0,
    /// The single matching-sprite alignment.
    Custom = 9,
}

/// Baseline `SpriteMeshType.FullRect` discriminant (0 in the shipped enums).
/// The empty-sprite path writes it; otherwise the sprite render-data mesh type
/// (an opaque generated integer) passes through untouched.
pub const SPRITE_MESH_TYPE_FULL_RECT: i32 = 0;

/// Baseline `TextureImporterType.Sprite` discriminant (8 in the shipped enums),
/// written by every non-empty `AddSprites` path.
pub const TEXTURE_IMPORTER_TYPE_SPRITE: i32 = 8;

/// Caller facts for one sprite in `TextureExportCollection.AddSprites`.
#[derive(Clone, Debug, PartialEq)]
pub struct SpriteFacts {
    /// Sprite `Name`, copied into a Multiple-mode SpriteMetaData entry.
    pub name: String,
    /// `TextureExportCollection.GetExportID` for this sprite in collection encounter order.
    pub export_id: Option<i64>,
    /// The sprite `Rect` (`[x, y, width, height]`).
    pub rect: [f32; 4],
    /// The processed Sprite `Rect` size used by `SetBoneGeometry` after `SpriteProcessor`.
    pub source_rect_size: [f32; 2],
    /// The sprite render-data texture rect in the same layout.
    pub texture_rect: [f32; 4],
    /// Whether the sprite name equals the texture name.
    pub name_matches_texture: bool,
    /// The sprite `Extrude`.
    pub extrude: u32,
    /// The sprite render-data mesh type (opaque generated integer).
    pub mesh_type: i32,
    /// Whether the sprite exposes a pivot.
    pub has_pivot: bool,
    /// The sprite pivot.
    pub pivot: [f32; 2],
    /// Whether the sprite exposes a border.
    pub has_border: bool,
    /// The sprite border.
    pub border: [f32; 4],
    /// The sprite `PixelsToUnits`.
    pub pixels_to_units: f32,
    /// Generated outlines after the source PPU, pivot, and packing transforms.
    pub outline: Vec<Vec<[f32; 2]>>,
    /// Physics shapes after the source PPU, pivot, and packing transforms.
    pub physics_shape: Vec<Vec<[f32; 2]>>,
    /// Source `m_Bones`; `Some([])` preserves a present-but-empty array.
    pub bones: Option<Vec<SpriteBoneData>>,
    /// Stable importer SpriteID for bone metadata, derived from source file/object identity.
    pub sprite_id: Option<String>,
    /// Geometry copied by `SpriteMetaDataExtensions.SetBoneGeometry`.
    pub bone_geometry: SpriteBoneGeometry,
}

/// Bone metadata copied from one source SpriteBone after the baseline PPU/root transform.
#[derive(Clone, Debug, PartialEq)]
pub struct SpriteBoneData {
    /// Bone name.
    pub name: String,
    /// Position in Sprite importer coordinates.
    pub position: [f32; 3],
    /// Bone rotation quaternion.
    pub rotation: [f32; 4],
    /// Bone length in Sprite importer units.
    pub length: f32,
    /// Parent index (`-1` for a root bone).
    pub parent_id: i32,
    /// Optional source Color32.
    pub color: Option<[u8; 4]>,
    /// Optional source bone GUID.
    pub guid: Option<String>,
}

/// Geometry emitted beside Sprite bone metadata.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpriteBoneGeometry {
    /// Source vertices transformed into importer coordinates.
    pub vertices: Vec<[f32; 2]>,
    /// Signed 16-bit indices read from the source little-endian index buffer.
    pub indices: Vec<i16>,
    /// Decoded per-vertex skin values.
    pub weights: Vec<BoneWeight4>,
}

/// The importer fields `AddSprites` assigns, before version-gated application.
///
/// Fields the source leaves untouched (the texture type on the empty path, which
/// keeps the `ImporterFactory` default) are reported as `None`/false so callers
/// can distinguish "write" from "leave alone".
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpriteImporterDecision {
    /// `SpriteModeE`.
    pub mode: SpriteImportMode,
    /// Whether `TextureTypeE` is forced to Sprite (empty path leaves it alone).
    pub texture_type_is_sprite: bool,
    /// `SpriteExtrude`.
    pub extrude: u32,
    /// `SpriteMeshType`.
    pub mesh_type: i32,
    /// `Alignment`.
    pub alignment: SpriteAlignment,
    /// `SpritePivot` when the importer exposes it (and the sprite supplies one
    /// on the single-sprite path).
    pub pivot: Option<[f32; 2]>,
    /// `SpriteBorder` when both sides expose it (single-sprite path only).
    pub border: Option<[f32; 4]>,
    /// `SpritePixelsToUnits`.
    pub pixels_to_units: f32,
    /// Whether `AddSpriteSheet`/`AddIDToName` run (`m_exportSprites` with sprites).
    pub build_sprite_sheet: bool,
}

/// `TextureExportCollection.AddSprites` over caller facts.
///
/// With no sprites the importer takes Single mode, extrude 1, `FullRect`, center
/// alignment, a centered pivot, and 100 pixels per unit, leaving the texture
/// type at its factory default. With exactly one sprite the mode is Single only
/// when the sprite rect equals its render-data texture rect and the sprite name
/// equals the texture name, else Multiple; the sprite's extrude, mesh type,
/// pivot, border, and pixels-per-unit apply. With several sprites the first
/// sprite supplies extrude, mesh type, and pixels-per-unit under Multiple mode
/// with center alignment. The sprite sheet and ID table build only when
/// `export_sprites` is set and at least one sprite is present.
pub fn decide_sprite_importer(
    sprites: &[SpriteFacts],
    export_sprites: bool,
    importer_has_pivot: bool,
    importer_has_border: bool,
) -> SpriteImporterDecision {
    match sprites {
        [] => SpriteImporterDecision {
            mode: SpriteImportMode::Single,
            texture_type_is_sprite: false,
            extrude: texture_importer::SPRITE_EXTRUDE as u32,
            mesh_type: SPRITE_MESH_TYPE_FULL_RECT,
            alignment: SpriteAlignment::Center,
            pivot: importer_has_pivot.then_some([0.5, 0.5]),
            border: None,
            pixels_to_units: texture_importer::SPRITE_PIXELS_TO_UNITS,
            build_sprite_sheet: false,
        },
        [sprite] => {
            let single = sprite.rect == sprite.texture_rect && sprite.name_matches_texture;
            SpriteImporterDecision {
                mode: if single {
                    SpriteImportMode::Single
                } else {
                    SpriteImportMode::Multiple
                },
                texture_type_is_sprite: true,
                extrude: sprite.extrude,
                mesh_type: sprite.mesh_type,
                alignment: SpriteAlignment::Custom,
                pivot: (importer_has_pivot && sprite.has_pivot).then_some(sprite.pivot),
                border: (importer_has_border && sprite.has_border).then_some(sprite.border),
                pixels_to_units: sprite.pixels_to_units,
                build_sprite_sheet: export_sprites,
            }
        }
        [first, ..] => SpriteImporterDecision {
            mode: SpriteImportMode::Multiple,
            texture_type_is_sprite: true,
            extrude: first.extrude,
            mesh_type: first.mesh_type,
            alignment: SpriteAlignment::Center,
            pivot: importer_has_pivot.then_some([0.5, 0.5]),
            border: None,
            pixels_to_units: first.pixels_to_units,
            build_sprite_sheet: export_sprites,
        },
    }
}

/// The `EmitPrefabAsset` branch: `Prefab is IPrefabMarker` selects the legacy asset path.
pub fn select_prefab_importer(is_prefab_marker: bool) -> PrefabImporterBranch {
    if is_prefab_marker {
        PrefabImporterBranch::LegacyPrefabAsset
    } else {
        PrefabImporterBranch::ModernPrefabImporter
    }
}

/// Class ID of Sprite objects whose render data can point at a texture.
const SPRITE_OBJECT_CLASS_ID: i32 = 213;

/// Collect `AddSprites` caller facts for workspace sprites whose render-data texture
/// resolves to one texture object.
///
/// This mirrors the `SpriteProcessor` association (`sprite.RD.Texture.TryGetAsset`) for the
/// importer path only; asset grouping stays open. Facts follow workspace file/object encounter
/// order, matching the source `SpriteInformationObject` insertion sequence. Source file version
/// and endianness select the render-data geometry decoder. When sprite-sheet export is enabled,
/// malformed outline, physics-shape, or bone geometry is an error rather than a silently omitted
/// field. Bone geometry decodes inline vertex bytes or the resolved external stream selected by
/// `SpriteRenderData.m_StreamData`.
#[allow(clippy::too_many_arguments)]
pub fn collect_sprite_facts_for_texture(
    workspace: &SerializedFileWorkspace,
    objects: &ObjectWorkspace,
    graph: &MultiFileReferenceGraph<'_>,
    texture_file_index: usize,
    texture_object_index: usize,
    texture_name: &str,
    build_sprite_sheet: bool,
    mut resolve_stream_vertex_bytes: impl FnMut(
        usize,
        i64,
        &SerializedValue,
        UnityVersion,
        EndianType,
    ) -> Result<Option<Vec<u8>>, Box<dyn Error>>,
) -> Result<Vec<SpriteFacts>, Box<dyn Error>> {
    let mut facts = Vec::new();
    for sprite in objects
        .objects()
        .iter()
        .filter(|object| object.factory.class_id == SPRITE_OBJECT_CLASS_ID)
    {
        let Ok(payload) = &sprite.payload else {
            continue;
        };
        let Some(source_file) = workspace.files().get(sprite.file_index) else {
            return Err(invalid_sprite_data(format!(
                "Sprite object {} references absent SerializedFile {}",
                sprite.object_index, sprite.file_index
            )));
        };
        let source_version = source_file
            .metadata
            .prefix
            .unity_version_text
            .as_deref()
            .and_then(UnityVersion::parse);
        let Some(mut link) =
            read_sprite_texture_link(&payload.root, source_version.as_ref(), texture_name)
                .map_err(|error| {
                    invalid_sprite_data(format!(
                        "Sprite object {} texture metadata could not be read: {error}",
                        sprite.object_index
                    ))
                })?
        else {
            continue;
        };
        let target = graph
            .references_from_object(sprite.file_index, sprite.object_index)
            .find(|edge| {
                edge.file_id == link.texture_file_id && edge.path_id == link.texture_path_id
            });
        let Some(edge) = target else {
            continue;
        };
        let MultiFilePPtrResolution::Object {
            target_file_index,
            target_object_index,
        } = edge.resolution
        else {
            continue;
        };
        if target_file_index != texture_file_index || target_object_index != texture_object_index {
            continue;
        }
        if build_sprite_sheet {
            let source_version = source_version.ok_or_else(|| {
                invalid_sprite_data(format!(
                    "Sprite object {} has no parseable source Unity version",
                    sprite.object_index
                ))
            })?;
            let endian = match source_file.metadata.prefix.body_endian {
                crate::io::binary::Endian::Little => EndianType::Little,
                crate::io::binary::Endian::Big => EndianType::Big,
            };
            let stream_bytes = resolve_stream_vertex_bytes(
                sprite.file_index,
                sprite.path_id,
                link.render_data,
                source_version,
                endian,
            )?;
            link.facts.outline = generate_sprite_render_data_outlines_from_value(
                link.render_data,
                source_version,
                endian,
                stream_bytes.as_deref(),
            )
            .map_err(|error| {
                invalid_sprite_data(format!(
                    "Sprite object {} render-data outline failed: {error}",
                    sprite.object_index
                ))
            })?
            .unwrap_or_default();
            transform_sprite_outline_points(&mut link.facts.outline, link.outline_transform);
            link.facts.physics_shape =
                read_sprite_physics_shape(&payload.root, link.outline_transform).map_err(
                    |error| {
                        invalid_sprite_data(format!(
                            "Sprite object {} physics shape could not be read: {error}",
                            sprite.object_index
                        ))
                    },
                )?;
            link.facts.bones = read_sprite_bones(
                &payload.root,
                link.facts.pixels_to_units,
                link.facts.source_rect_size,
            )
            .map_err(|error| {
                invalid_sprite_data(format!(
                    "Sprite object {} bone metadata could not be read: {error}",
                    sprite.object_index
                ))
            })?;
            if link.facts.bones.is_some() {
                link.facts.sprite_id = Some(sprite_importer_id(sprite.file_index, sprite.path_id));
                link.facts.bone_geometry = read_sprite_bone_geometry(
                    link.render_data,
                    source_version,
                    endian,
                    stream_bytes.as_deref(),
                    link.facts.pixels_to_units,
                    link.facts.source_rect_size,
                )
                .map_err(|error| {
                    invalid_sprite_data(format!(
                        "Sprite object {} bone geometry could not be read: {error}",
                        sprite.object_index
                    ))
                })?;
            }
        }
        facts
            .try_reserve(1)
            .map_err(|_| invalid_sprite_data("could not allocate sprite facts"))?;
        link.facts.export_id = Some(sprite_export_id_for_ordinal(facts.len())?);
        facts.push(link.facts);
    }
    Ok(facts)
}

/// Whether a built importer mapping exposes a field (the caller-side equivalent of the
/// generated `Has_*` gates for the sprite pivot/border copies).
pub fn importer_node_has_field(importer: &UnityYamlNode, name: &str) -> bool {
    let UnityYamlNodeKind::Mapping { entries, .. } = &importer.kind else {
        return false;
    };
    entries.iter().any(|(key, _)| yaml_key_is(key, name))
}

/// Apply an `AddSprites` decision to a built importer mapping, overwriting the default
/// sprite fields in place so field order is preserved. Fields the decision leaves alone
/// (the texture type on the empty path) are never touched. Missing target fields fail
/// loudly: the pivot/border gates are derived from the node itself, and every other
/// assigned field exists in all versioned importer layouts.
pub fn apply_sprite_importer_decision(
    importer: &mut UnityYamlNode,
    decision: &SpriteImporterDecision,
) -> Result<(), Box<dyn Error>> {
    set_importer_signed(importer, "spriteMode", i64::from(decision.mode as i32))?;
    set_importer_signed(importer, "spriteExtrude", i64::from(decision.extrude))?;
    set_importer_signed(importer, "spriteMeshType", i64::from(decision.mesh_type))?;
    set_importer_signed(importer, "alignment", i64::from(decision.alignment as i32))?;
    if let Some(pivot) = decision.pivot {
        set_importer_point(importer, "spritePivot", &[("x", pivot[0]), ("y", pivot[1])])?;
    }
    if let Some(border) = decision.border {
        set_importer_point(
            importer,
            "spriteBorder",
            &[
                ("x", border[0]),
                ("y", border[1]),
                ("z", border[2]),
                ("w", border[3]),
            ],
        )?;
    }
    set_importer_float(importer, "spritePixelsToUnits", decision.pixels_to_units)?;
    if decision.texture_type_is_sprite {
        set_importer_signed(
            importer,
            "textureType",
            i64::from(TEXTURE_IMPORTER_TYPE_SPRITE),
        )?;
    }
    Ok(())
}

/// Writes the source outline, physics-shape, and bone fields for a Single-mode sheet, or
/// source-ordered per-sprite metadata and export-name mappings for legacy Multiple-mode layouts.
pub fn apply_sprite_sheet_metadata_to_importer(
    importer: &mut UnityYamlNode,
    sprites: &[SpriteFacts],
    decision: &SpriteImporterDecision,
) -> Result<(), Box<dyn Error>> {
    if !decision.build_sprite_sheet {
        return Ok(());
    }
    let Some(sprite_sheet) = yaml_mapping_field_mut(importer, "spriteSheet") else {
        return Ok(());
    };
    if decision.mode == SpriteImportMode::Multiple {
        let mut metadata =
            UnityYamlNode::sequence(crate::export::unity_yaml::UnityYamlSequenceStyle::Block);
        for sprite in sprites {
            metadata.push(sprite_meta_data_to_yaml(sprite, sprite_sheet)?)?;
        }
        let Some(sprites_field) = yaml_mapping_field_mut(sprite_sheet, "sprites") else {
            return Err(invalid_sprite_data(
                "multiple SpriteSheet layout has no sprites field",
            ));
        };
        *sprites_field = metadata;
        write_sprite_export_name_table(importer, sprites)?;
        return Ok(());
    }
    let [sprite] = sprites else {
        return Err(invalid_sprite_data(
            "Single SpriteSheet mode requires exactly one sprite fact",
        ));
    };
    if let Some(outline_field) = yaml_mapping_field_mut(sprite_sheet, "outline") {
        *outline_field = sprite_outline_to_yaml(&sprite.outline)?;
    }
    write_sprite_physics_shape(sprite_sheet, &sprite.physics_shape)?;
    write_sprite_bone_fields(sprite_sheet, sprite)
}

fn sprite_meta_data_to_yaml(
    sprite: &SpriteFacts,
    sprite_sheet: &UnityYamlNode,
) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut metadata =
        UnityYamlNode::mapping(crate::export::unity_yaml::UnityYamlMappingStyle::Block);
    metadata.insert_field("serializedVersion", signed_node(2))?;
    metadata.insert_field("name", UnityYamlNode::string(sprite.name.clone()))?;
    let mut rect = UnityYamlNode::mapping(crate::export::unity_yaml::UnityYamlMappingStyle::Block);
    rect.insert_field("serializedVersion", signed_node(2))?;
    for (name, value) in [
        ("x", sprite.rect[0]),
        ("y", sprite.rect[1]),
        ("width", sprite.rect[2]),
        ("height", sprite.rect[3]),
    ] {
        rect.insert_field(name, float_node(value))?;
    }
    metadata.insert_field("rect", rect)?;
    metadata.insert_field("alignment", signed_node(SpriteAlignment::Custom as i64))?;
    metadata.insert_field("pivot", sprite_point_node(sprite.pivot)?)?;
    if sprite.has_border {
        metadata.insert_field("border", sprite_border_node(sprite.border)?)?;
    }
    if importer_node_has_field(sprite_sheet, "outline") {
        metadata.insert_field("outline", sprite_outline_to_yaml(&sprite.outline)?)?;
    }
    if importer_node_has_field(sprite_sheet, "physicsShape") {
        metadata.insert_field(
            "physicsShape",
            sprite_outline_to_yaml(&sprite.physics_shape)?,
        )?;
    }
    metadata.insert_field("tessellationDetail", signed_node(0))?;
    for (name, value) in sprite_bone_fields_to_yaml(sprite, sprite_sheet)? {
        metadata.insert_field(name, value)?;
    }
    if importer_node_has_field(sprite_sheet, "internalID") {
        let export_id = sprite
            .export_id
            .ok_or_else(|| invalid_sprite_data("Sprite metadata has no planned export ID"))?;
        metadata.insert_field("internalID", signed_node(export_id))?;
    }
    Ok(metadata)
}

fn write_sprite_bone_fields(
    sprite_sheet: &mut UnityYamlNode,
    sprite: &SpriteFacts,
) -> Result<(), Box<dyn Error>> {
    if sprite.bones.is_none()
        || !importer_node_has_field(sprite_sheet, "bones")
        || !importer_node_has_field(sprite_sheet, "spriteID")
    {
        return Ok(());
    }
    for (name, value) in sprite_bone_fields_to_yaml(sprite, sprite_sheet)? {
        let Some(field) = yaml_mapping_field_mut(sprite_sheet, &name) else {
            continue;
        };
        *field = value;
    }
    Ok(())
}

fn sprite_bone_fields_to_yaml(
    sprite: &SpriteFacts,
    target_layout: &UnityYamlNode,
) -> Result<Vec<(String, UnityYamlNode)>, Box<dyn Error>> {
    let Some(bones) = sprite.bones.as_deref() else {
        return Ok(Vec::new());
    };
    if !importer_node_has_field(target_layout, "bones")
        || !importer_node_has_field(target_layout, "spriteID")
    {
        return Ok(Vec::new());
    }
    let sprite_id = sprite
        .sprite_id
        .as_ref()
        .ok_or_else(|| invalid_sprite_data("Sprite bone metadata has no planned SpriteID"))?;
    let mut fields = vec![
        ("bones".to_owned(), sprite_bones_to_yaml(bones)?),
        (
            "spriteID".to_owned(),
            UnityYamlNode::string(sprite_id.clone()),
        ),
    ];
    if importer_node_has_field(target_layout, "vertices") {
        fields.push((
            "vertices".to_owned(),
            sprite_bone_vertices_to_yaml(&sprite.bone_geometry.vertices)?,
        ));
    }
    if importer_node_has_field(target_layout, "indices") {
        fields.push((
            "indices".to_owned(),
            sprite_bone_indices_to_yaml(&sprite.bone_geometry.indices)?,
        ));
    }
    // The frozen baseline still has a TODO for SpriteMetaData.Edges. Its default empty list is
    // retained instead of inventing edge data from the triangulation.
    if importer_node_has_field(target_layout, "edges") {
        fields.push(("edges".to_owned(), empty_sprite_sequence()));
    }
    if importer_node_has_field(target_layout, "weights") {
        fields.push((
            "weights".to_owned(),
            sprite_bone_weights_to_yaml(&sprite.bone_geometry.weights)?,
        ));
    }
    Ok(fields)
}

fn sprite_bones_to_yaml(bones: &[SpriteBoneData]) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut sequence = empty_sprite_sequence();
    for bone in bones {
        let mut node =
            UnityYamlNode::mapping(crate::export::unity_yaml::UnityYamlMappingStyle::Block);
        node.insert_field("name", UnityYamlNode::string(bone.name.clone()))?;
        node.insert_field("position", vector3_node(bone.position)?)?;
        node.insert_field("rotation", vector4_node(bone.rotation)?)?;
        node.insert_field("length", float_node(bone.length))?;
        node.insert_field("parentId", signed_node(i64::from(bone.parent_id)))?;
        if let Some(color) = bone.color {
            let mut color_node =
                UnityYamlNode::mapping(crate::export::unity_yaml::UnityYamlMappingStyle::Flow);
            for (name, value) in [
                ("r", color[0]),
                ("g", color[1]),
                ("b", color[2]),
                ("a", color[3]),
            ] {
                color_node.insert_field(
                    name,
                    UnityYamlNode::scalar(UnityYamlScalar::Unsigned(u64::from(value))),
                )?;
            }
            node.insert_field("color", color_node)?;
        }
        if let Some(guid) = &bone.guid {
            node.insert_field("guid", UnityYamlNode::string(guid.clone()))?;
        }
        sequence.push(node)?;
    }
    Ok(sequence)
}

fn sprite_bone_vertices_to_yaml(vertices: &[[f32; 2]]) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut sequence = empty_sprite_sequence();
    for vertex in vertices {
        sequence.push(sprite_point_node(*vertex)?)?;
    }
    Ok(sequence)
}

fn sprite_bone_indices_to_yaml(indices: &[i16]) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut sequence = empty_sprite_sequence();
    for index in indices {
        sequence.push(signed_node(i64::from(*index)))?;
    }
    Ok(sequence)
}

fn sprite_bone_weights_to_yaml(weights: &[BoneWeight4]) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut sequence = empty_sprite_sequence();
    for weight in weights {
        let mut node =
            UnityYamlNode::mapping(crate::export::unity_yaml::UnityYamlMappingStyle::Block);
        for (index, value) in weight.weights.iter().enumerate() {
            node.insert_field(format!("weight[{index}]"), float_node(*value))?;
        }
        for (index, value) in weight.indices.iter().enumerate() {
            node.insert_field(
                format!("boneIndex[{index}]"),
                signed_node(i64::from(*value)),
            )?;
        }
        sequence.push(node)?;
    }
    Ok(sequence)
}

fn vector3_node(vector: [f32; 3]) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut node = UnityYamlNode::mapping(crate::export::unity_yaml::UnityYamlMappingStyle::Flow);
    for (name, value) in [("x", vector[0]), ("y", vector[1]), ("z", vector[2])] {
        node.insert_field(name, float_node(value))?;
    }
    Ok(node)
}

fn vector4_node(vector: [f32; 4]) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut node = UnityYamlNode::mapping(crate::export::unity_yaml::UnityYamlMappingStyle::Flow);
    for (name, value) in [
        ("x", vector[0]),
        ("y", vector[1]),
        ("z", vector[2]),
        ("w", vector[3]),
    ] {
        node.insert_field(name, float_node(value))?;
    }
    Ok(node)
}

fn empty_sprite_sequence() -> UnityYamlNode {
    UnityYamlNode::sequence(crate::export::unity_yaml::UnityYamlSequenceStyle::Block)
}

fn sprite_export_id_for_ordinal(ordinal: usize) -> Result<i64, Box<dyn Error>> {
    let modifier = ordinal
        .checked_mul(2)
        .and_then(|modifier| u32::try_from(modifier).ok())
        .ok_or_else(|| invalid_sprite_data("Sprite export ID modifier overflows"))?;
    Ok(get_main_export_id(SPRITE_OBJECT_CLASS_ID, modifier)?)
}

fn write_sprite_export_name_table(
    importer: &mut UnityYamlNode,
    sprites: &[SpriteFacts],
) -> Result<(), Box<dyn Error>> {
    if let Some(table) = yaml_mapping_field_mut(importer, "internalIDToNameTable") {
        for sprite in sprites {
            let export_id = sprite
                .export_id
                .ok_or_else(|| invalid_sprite_data("Sprite name table has no planned export ID"))?;
            let mut id_pair =
                UnityYamlNode::mapping(crate::export::unity_yaml::UnityYamlMappingStyle::Block);
            id_pair.insert_field("first", signed_node(i64::from(SPRITE_OBJECT_CLASS_ID)))?;
            id_pair.insert_field("second", signed_node(export_id))?;
            let mut entry =
                UnityYamlNode::mapping(crate::export::unity_yaml::UnityYamlMappingStyle::Block);
            entry.insert_field("first", id_pair)?;
            entry.insert_field("second", UnityYamlNode::string(sprite.name.clone()))?;
            table.push(entry)?;
        }
    } else if let Some(table) = yaml_mapping_field_mut(importer, "fileIDToRecycleName") {
        for sprite in sprites {
            let export_id = sprite
                .export_id
                .ok_or_else(|| invalid_sprite_data("Sprite name table has no planned export ID"))?;
            let UnityYamlNodeKind::Mapping { entries, .. } = &mut table.kind else {
                return Err(invalid_sprite_data("fileIDToRecycleName must be a mapping"));
            };
            let key = export_id.to_string();
            let duplicate = entries.iter().any(|(field, _)| yaml_key_is(field, &key));
            if duplicate {
                return Err(invalid_sprite_data(format!(
                    "duplicate Sprite export ID {export_id} in fileIDToRecycleName"
                )));
            }
            table.insert_field(key, UnityYamlNode::string(sprite.name.clone()))?;
        }
    }
    Ok(())
}

fn signed_node(value: i64) -> UnityYamlNode {
    UnityYamlNode::scalar(UnityYamlScalar::Signed(value))
}

fn float_node(value: f32) -> UnityYamlNode {
    UnityYamlNode::scalar(UnityYamlScalar::Float32(value))
}

fn sprite_point_node(point: [f32; 2]) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut node = UnityYamlNode::mapping(crate::export::unity_yaml::UnityYamlMappingStyle::Flow);
    node.insert_field("x", float_node(point[0]))?;
    node.insert_field("y", float_node(point[1]))?;
    Ok(node)
}

fn sprite_border_node(border: [f32; 4]) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut node = UnityYamlNode::mapping(crate::export::unity_yaml::UnityYamlMappingStyle::Flow);
    for (name, value) in [
        ("x", border[0]),
        ("y", border[1]),
        ("z", border[2]),
        ("w", border[3]),
    ] {
        node.insert_field(name, float_node(value))?;
    }
    Ok(node)
}

fn write_sprite_physics_shape(
    sprite_sheet: &mut UnityYamlNode,
    physics_shape: &[Vec<[f32; 2]>],
) -> Result<(), Box<dyn Error>> {
    let Some(physics_shape_field) = yaml_mapping_field_mut(sprite_sheet, "physicsShape") else {
        return Ok(());
    };
    *physics_shape_field = sprite_outline_to_yaml(physics_shape)?;
    Ok(())
}

fn sprite_outline_to_yaml(outlines: &[Vec<[f32; 2]>]) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut outline_node =
        UnityYamlNode::sequence(crate::export::unity_yaml::UnityYamlSequenceStyle::Block);
    for outline in outlines {
        let mut points =
            UnityYamlNode::sequence(crate::export::unity_yaml::UnityYamlSequenceStyle::Block);
        for point in outline {
            let mut vector =
                UnityYamlNode::mapping(crate::export::unity_yaml::UnityYamlMappingStyle::Flow);
            vector.insert_field(
                "x",
                UnityYamlNode::scalar(UnityYamlScalar::Float32(point[0])),
            )?;
            vector.insert_field(
                "y",
                UnityYamlNode::scalar(UnityYamlScalar::Float32(point[1])),
            )?;
            points.push(vector)?;
        }
        outline_node.push(points)?;
    }
    Ok(outline_node)
}

fn yaml_mapping_field_mut<'a>(
    node: &'a mut UnityYamlNode,
    name: &str,
) -> Option<&'a mut UnityYamlNode> {
    let UnityYamlNodeKind::Mapping { entries, .. } = &mut node.kind else {
        return None;
    };
    entries
        .iter_mut()
        .find(|(key, _)| yaml_key_is(key, name))
        .map(|(_, value)| value)
}

fn read_sprite_physics_shape(
    root: &SerializedValue,
    transform: SpriteOutlineTransform,
) -> Result<Vec<Vec<[f32; 2]>>, Box<dyn Error>> {
    let root = sprite_value_scope(root)?;
    let value = match root.unique_field("m_PhysicsShape") {
        Ok(Some(value)) => value,
        Ok(None) => return Ok(Vec::new()),
        Err(error) => {
            return Err(invalid_sprite_data(format!(
                "m_PhysicsShape lookup failed: {error}"
            )));
        }
    };
    let SerializedValue::Array(source_shapes) = value else {
        return Err(invalid_sprite_data(format!(
            "m_PhysicsShape must be an array, found {}",
            sprite_value_kind(value)
        )));
    };
    let mut shapes = Vec::new();
    shapes
        .try_reserve_exact(source_shapes.len())
        .map_err(|_| invalid_sprite_data("could not allocate physics-shape polygons"))?;
    for (shape_index, source_points) in source_shapes.iter().enumerate() {
        let SerializedValue::Array(source_points) = source_points else {
            return Err(invalid_sprite_data(format!(
                "m_PhysicsShape polygon {shape_index} must be an array, found {}",
                sprite_value_kind(source_points)
            )));
        };
        let mut points = Vec::new();
        points
            .try_reserve_exact(source_points.len())
            .map_err(|_| invalid_sprite_data("could not allocate physics-shape points"))?;
        for (point_index, source_point) in source_points.iter().enumerate() {
            let point_value = sprite_value_scope(source_point).map_err(|error| {
                invalid_sprite_data(format!(
                    "m_PhysicsShape polygon {shape_index} point {point_index} is not a Vector2 struct: {error}"
                ))
            })?;
            let read_component = |axis| -> Result<f32, Box<dyn Error>> {
                let component = point_value
                    .unique_field(axis)
                    .map_err(|error| {
                        invalid_sprite_data(format!(
                            "m_PhysicsShape polygon {shape_index} point {point_index} {axis} lookup failed: {error}"
                        ))
                    })?
                    .ok_or_else(|| {
                        invalid_sprite_data(format!(
                            "m_PhysicsShape polygon {shape_index} point {point_index} has no {axis}"
                        ))
                    })?;
                sprite_f32(component).ok_or_else(|| {
                    invalid_sprite_data(format!(
                        "m_PhysicsShape polygon {shape_index} point {point_index} {axis} is not a float"
                    ))
                })
            };
            let point = [read_component("x")?, read_component("y")?];
            points.push(point);
        }
        shapes.push(points);
    }
    transform_sprite_outline_points(&mut shapes, transform);
    Ok(shapes)
}

fn sprite_value_kind(value: &SerializedValue) -> &'static str {
    match value {
        SerializedValue::Bool(_) => "bool",
        SerializedValue::I8(_) => "SInt8",
        SerializedValue::U8(_) => "UInt8",
        SerializedValue::I16(_) => "SInt16",
        SerializedValue::U16(_) => "UInt16",
        SerializedValue::Char(_) => "char",
        SerializedValue::I32(_) => "SInt32",
        SerializedValue::U32(_) => "UInt32",
        SerializedValue::I64(_) => "SInt64",
        SerializedValue::U64(_) => "UInt64",
        SerializedValue::F32(_) => "float",
        SerializedValue::F64(_) => "double",
        SerializedValue::StringBytes(_) => "string bytes",
        SerializedValue::Bytes(_) => "byte data",
        SerializedValue::Array(_) => "array",
        SerializedValue::Struct(_) => "struct",
        SerializedValue::PPtr { .. } => "PPtr",
        SerializedValue::ManagedReferenceId { .. } => "managed reference ID",
        SerializedValue::ManagedReferenceRegistry(_) => "managed reference registry",
        SerializedValue::Raw { .. } => "raw data",
    }
}

/// One sprite render-data texture link: caller facts plus the bundle texture pointer.
struct SpriteTextureLink<'a> {
    facts: SpriteFacts,
    texture_file_id: i32,
    texture_path_id: i64,
    render_data: &'a SerializedValue,
    outline_transform: SpriteOutlineTransform,
}

/// Read `AddSprites` caller facts from one Sprite payload, resolving the sprite name against
/// the queried texture. Returns `Ok(None)` when the sprite is atlas-packed (recovery stays open)
/// or when the render-data texture pointer is null. Missing required fields, duplicate fields,
/// and malformed value shapes are errors, not skipped sprites.
///
/// Crucially, the facts carry the *processed* rect/pivot/border: `SpriteProcessor.ProcessSprite`
/// recalculates them from the render data (`GetSpriteCoordinatesInAtlas` plus copies), and
/// `AddSprites` reads those processed values — not the stale bundle `m_Rect`/`m_Pivot`.
fn read_sprite_texture_link<'a>(
    root: &'a SerializedValue,
    source_version: Option<&UnityVersion>,
    texture_name: &str,
) -> Result<Option<SpriteTextureLink<'a>>, Box<dyn Error>> {
    use crate::resources::sprite::get_sprite_coordinates_in_atlas;
    // Atlas-packed sprites need render-data recovery from the atlas map first, which
    // stays open; only atlas-null sprites take the direct render-data path.
    match strict_sprite_field(root, "m_SpriteAtlas")? {
        None => {}
        Some(SerializedValue::PPtr {
            file_id: 0,
            path_id: 0,
        }) => {}
        Some(SerializedValue::PPtr { .. }) => return Ok(None),
        Some(value) => {
            return Err(invalid_sprite_data(format!(
                "Sprite.m_SpriteAtlas has kind {}, expected PPtr",
                sprite_value_kind(value)
            )))
        }
    }
    let rd_value = strict_required_sprite_field(root, "m_RD")?;
    let rd = sprite_value_scope(rd_value)?;
    let texture_value = strict_required_sprite_field(rd, "texture")?;
    let (texture_file_id, texture_path_id) = match texture_value {
        SerializedValue::PPtr { file_id, path_id } => (*file_id, *path_id),
        value => {
            return Err(invalid_sprite_data(format!(
                "Sprite.m_RD.texture has kind {}, expected PPtr",
                sprite_value_kind(value)
            )))
        }
    };
    if texture_path_id == 0 {
        return Ok(None);
    }
    let sprite_rect = strict_sprite_rect4(root, "m_Rect")?;
    let texture_rect = strict_sprite_rect4(rd, "textureRect")?;
    let texture_rect_offset = strict_sprite_point2(rd, "textureRectOffset")?;
    let offset = strict_sprite_point2(root, "m_Offset")?;
    let settings_raw_value = strict_required_sprite_field(rd, "settingsRaw")?;
    let mesh_type = sprite_mesh_type(settings_raw_value).ok_or_else(|| {
        invalid_sprite_data(format!(
            "Sprite.m_RD.settingsRaw has kind {}, expected integer",
            sprite_value_kind(settings_raw_value)
        ))
    })?;
    let settings_raw = sprite_u32(settings_raw_value).ok_or_else(|| {
        invalid_sprite_data(format!(
            "Sprite.m_RD.settingsRaw has kind {}, expected unsigned integer",
            sprite_value_kind(settings_raw_value)
        ))
    })?;
    let stored_pivot = strict_sprite_optional_point2(root, "m_Pivot")?;
    let stored_border = strict_sprite_optional_point4(root, "m_Border")?;
    // `Has_Pivot`/`Has_Border` are layout gates, not value presence: every supported
    // layout carries the fields, so a present bundle value always counts; missing bundle
    // values on ancient layouts fall back to the derived/zero coordinates below.
    let layout_has_pivot = stored_pivot.is_some()
        || source_version.is_some_and(|version| (version.major, version.minor) >= (5, 0));
    let layout_has_border = stored_border.is_some()
        || source_version.is_some_and(|version| (version.major, version.minor) >= (5, 0));
    let coords = get_sprite_coordinates_in_atlas(
        sprite_rect,
        texture_rect,
        texture_rect_offset,
        false,
        [0.0; 4],
        [0.0; 2],
        stored_pivot.is_some(),
        stored_pivot.unwrap_or([0.0, 0.0]),
        offset,
        stored_border.is_some(),
        stored_border.unwrap_or([0.0, 0.0, 0.0, 0.0]),
    );
    let name_value = strict_required_sprite_field(root, "m_Name")?;
    let name = match name_value {
        SerializedValue::StringBytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        value => {
            return Err(invalid_sprite_data(format!(
                "Sprite.m_Name has kind {}, expected string",
                sprite_value_kind(value)
            )))
        }
    };
    let extrude_value = strict_required_sprite_field(root, "m_Extrude")?;
    let extrude = sprite_u32(extrude_value).ok_or_else(|| {
        invalid_sprite_data(format!(
            "Sprite.m_Extrude has kind {}, expected integer",
            sprite_value_kind(extrude_value)
        ))
    })?;
    let pixels_to_units_value = strict_required_sprite_field(root, "m_PixelsToUnits")?;
    let pixels_to_units = sprite_f32(pixels_to_units_value).ok_or_else(|| {
        invalid_sprite_data(format!(
            "Sprite.m_PixelsToUnits has kind {}, expected float",
            sprite_value_kind(pixels_to_units_value)
        ))
    })?;
    Ok(Some(SpriteTextureLink {
        facts: SpriteFacts {
            name: name.clone(),
            export_id: None,
            rect: coords.rect,
            source_rect_size: [coords.rect[2], coords.rect[3]],
            texture_rect,
            name_matches_texture: name == texture_name,
            extrude,
            mesh_type,
            has_pivot: layout_has_pivot,
            pivot: coords.pivot,
            has_border: layout_has_border,
            border: coords.border,
            pixels_to_units,
            outline: Vec::new(),
            physics_shape: Vec::new(),
            bones: None,
            sprite_id: None,
            bone_geometry: SpriteBoneGeometry::default(),
        },
        texture_file_id,
        texture_path_id,
        render_data: rd,
        outline_transform: SpriteOutlineTransform {
            rect_size: [coords.rect[2], coords.rect[3]],
            pivot: coords.pivot,
            pixels_to_units,
            is_packed: sprite_settings_raw_is_packed(settings_raw),
            packing_rotation: sprite_settings_raw_packing_rotation(settings_raw),
        },
    }))
}

fn strict_sprite_rect4(root: &SerializedValue, name: &str) -> Result<[f32; 4], Box<dyn Error>> {
    let value = strict_required_sprite_field(root, name)?;
    Ok([
        strict_sprite_component(value, name, "x")?,
        strict_sprite_component(value, name, "y")?,
        strict_sprite_component(value, name, "width")?,
        strict_sprite_component(value, name, "height")?,
    ])
}

fn strict_sprite_point2(root: &SerializedValue, name: &str) -> Result<[f32; 2], Box<dyn Error>> {
    let value = strict_required_sprite_field(root, name)?;
    strict_sprite_point2_value(value, name)
}

fn strict_sprite_optional_point2(
    root: &SerializedValue,
    name: &str,
) -> Result<Option<[f32; 2]>, Box<dyn Error>> {
    strict_sprite_field(root, name)?
        .map(|value| strict_sprite_point2_value(value, name))
        .transpose()
}

fn strict_sprite_point2_value(
    value: &SerializedValue,
    name: &str,
) -> Result<[f32; 2], Box<dyn Error>> {
    Ok([
        strict_sprite_component(value, name, "x")?,
        strict_sprite_component(value, name, "y")?,
    ])
}

fn strict_sprite_optional_point4(
    root: &SerializedValue,
    name: &str,
) -> Result<Option<[f32; 4]>, Box<dyn Error>> {
    strict_sprite_field(root, name)?
        .map(|value| strict_sprite_point4_value(value, name))
        .transpose()
}

fn strict_sprite_point4_value(
    value: &SerializedValue,
    name: &str,
) -> Result<[f32; 4], Box<dyn Error>> {
    Ok([
        strict_sprite_component(value, name, "x")?,
        strict_sprite_component(value, name, "y")?,
        strict_sprite_component(value, name, "z")?,
        strict_sprite_component(value, name, "w")?,
    ])
}

fn strict_sprite_component(
    value: &SerializedValue,
    field: &str,
    component: &str,
) -> Result<f32, Box<dyn Error>> {
    strict_f32_field(value, component).map_err(|error| {
        invalid_sprite_data(format!(
            "Sprite field {field}.{component} could not be read: {error}"
        ))
    })
}

fn sprite_importer_id(file_index: usize, path_id: i64) -> String {
    // SpriteMetaDataExtensions uses Guid.NewGuid() here. A file/object identity pair provides
    // the same valid 32-hex-digit identifier shape while keeping repeat exports stable.
    format!("{:016x}{:016x}", file_index as u64, path_id as u64)
}

fn read_sprite_bones(
    sprite: &SerializedValue,
    pixels_to_units: f32,
    source_rect_size: [f32; 2],
) -> Result<Option<Vec<SpriteBoneData>>, Box<dyn Error>> {
    let Some(value) = strict_sprite_field(sprite, "m_Bones")? else {
        return Ok(None);
    };
    let SerializedValue::Array(values) = value else {
        return Err(invalid_sprite_data(format!(
            "Sprite m_Bones has kind {}, expected array",
            sprite_value_kind(value)
        )));
    };
    let mut bones = Vec::new();
    bones
        .try_reserve_exact(values.len())
        .map_err(|_| invalid_sprite_data("could not allocate Sprite bone metadata"))?;
    for (index, value) in values.iter().enumerate() {
        let bone = read_sprite_bone(value).map_err(|error| {
            invalid_sprite_data(format!("Sprite m_Bones[{index}] is invalid: {error}"))
        })?;
        let mut position = bone.position;
        for component in &mut position {
            *component *= pixels_to_units;
        }
        let length = bone.length * pixels_to_units;
        if bone.parent_id == -1 {
            position[0] += source_rect_size[0] / 2.0;
            position[1] += source_rect_size[1] / 2.0;
        }
        bones.push(SpriteBoneData {
            name: bone.name,
            position,
            rotation: bone.rotation,
            length,
            parent_id: bone.parent_id,
            color: bone.color,
            guid: bone.guid,
        });
    }
    Ok(Some(bones))
}

struct UnscaledSpriteBone {
    name: String,
    position: [f32; 3],
    rotation: [f32; 4],
    length: f32,
    parent_id: i32,
    color: Option<[u8; 4]>,
    guid: Option<String>,
}

fn read_sprite_bone(value: &SerializedValue) -> Result<UnscaledSpriteBone, Box<dyn Error>> {
    let name = match strict_required_sprite_field(value, "name")? {
        SerializedValue::StringBytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        other => {
            return Err(invalid_sprite_data(format!(
                "SpriteBone.name has kind {}, expected string",
                sprite_value_kind(other)
            )))
        }
    };
    let position = read_sprite_vector(value, "position", 3)?;
    let rotation = read_sprite_vector(value, "rotation", 4)?;
    let length = strict_f32_field(value, "length")?;
    let parent_id = strict_i32_field(value, "parentId")?;
    let color = strict_sprite_field(value, "color")?
        .map(read_sprite_color32)
        .transpose()?;
    let guid = strict_sprite_field(value, "guid")?
        .map(|value| match value {
            SerializedValue::StringBytes(bytes) => Ok(String::from_utf8_lossy(bytes).into_owned()),
            other => Err(invalid_sprite_data(format!(
                "SpriteBone.guid has kind {}, expected string",
                sprite_value_kind(other)
            ))),
        })
        .transpose()?;
    Ok(UnscaledSpriteBone {
        name,
        position: [position[0], position[1], position[2]],
        rotation: [rotation[0], rotation[1], rotation[2], rotation[3]],
        length,
        parent_id,
        color,
        guid,
    })
}

fn read_sprite_vector(
    value: &SerializedValue,
    field: &str,
    dimension: usize,
) -> Result<Vec<f32>, Box<dyn Error>> {
    let vector = strict_required_sprite_field(value, field)?;
    let components: &[&str] = match dimension {
        3 => &["x", "y", "z"],
        4 => &["x", "y", "z", "w"],
        _ => return Err(invalid_sprite_data("unsupported Sprite vector dimension")),
    };
    let mut result = Vec::with_capacity(dimension);
    for component in components {
        result.push(strict_f32_field(vector, component)?);
    }
    Ok(result)
}

fn read_sprite_color32(value: &SerializedValue) -> Result<[u8; 4], Box<dyn Error>> {
    let mut channels = [0; 4];
    for (index, name) in ["r", "g", "b", "a"].into_iter().enumerate() {
        let channel = strict_required_sprite_field(value, name)?;
        channels[index] = match channel {
            SerializedValue::U8(value) => *value,
            SerializedValue::I8(value) if *value >= 0 => *value as u8,
            SerializedValue::U16(value) if *value <= u8::MAX as u16 => *value as u8,
            SerializedValue::U32(value) if *value <= u32::from(u8::MAX) => *value as u8,
            other => {
                return Err(invalid_sprite_data(format!(
                    "SpriteBone.color.{name} has kind {}, expected byte",
                    sprite_value_kind(other)
                )))
            }
        };
    }
    Ok(channels)
}

fn strict_sprite_field<'a>(
    value: &'a SerializedValue,
    name: &str,
) -> Result<Option<&'a SerializedValue>, Box<dyn Error>> {
    let scope = sprite_value_scope(value)?;
    scope.unique_field(name).map_err(|error| {
        invalid_sprite_data(format!("could not read Sprite field {name}: {error}"))
    })
}

fn strict_required_sprite_field<'a>(
    value: &'a SerializedValue,
    name: &str,
) -> Result<&'a SerializedValue, Box<dyn Error>> {
    strict_sprite_field(value, name)?
        .ok_or_else(|| invalid_sprite_data(format!("Sprite data is missing field {name}")))
}

fn sprite_value_scope(value: &SerializedValue) -> Result<&SerializedValue, Box<dyn Error>> {
    if !matches!(value, SerializedValue::Struct(_)) {
        return Err(invalid_sprite_data(format!(
            "expected Sprite struct, found {}",
            sprite_value_kind(value)
        )));
    }
    match value.unique_field("data") {
        Ok(Some(data)) => Ok(data),
        Ok(None) => Ok(value),
        Err(error) => Err(invalid_sprite_data(format!(
            "could not read Sprite data wrapper: {error}"
        ))),
    }
}

fn strict_f32_field(value: &SerializedValue, name: &str) -> Result<f32, Box<dyn Error>> {
    let field = strict_required_sprite_field(value, name)?;
    sprite_f32(field).ok_or_else(|| {
        invalid_sprite_data(format!(
            "Sprite field {name} has kind {}, expected float",
            sprite_value_kind(field)
        ))
    })
}

fn strict_i32_field(value: &SerializedValue, name: &str) -> Result<i32, Box<dyn Error>> {
    let field = strict_required_sprite_field(value, name)?;
    match field {
        SerializedValue::I8(value) => Ok(i32::from(*value)),
        SerializedValue::I16(value) => Ok(i32::from(*value)),
        SerializedValue::I32(value) => Ok(*value),
        SerializedValue::U8(value) => Ok(i32::from(*value)),
        SerializedValue::U16(value) => Ok(i32::from(*value)),
        SerializedValue::U32(value) if *value <= i32::MAX as u32 => Ok(*value as i32),
        other => Err(invalid_sprite_data(format!(
            "Sprite field {name} has kind {}, expected 32-bit integer",
            sprite_value_kind(other)
        ))),
    }
}

fn read_sprite_bone_geometry(
    render_data: &SerializedValue,
    version: UnityVersion,
    endian: EndianType,
    resolved_stream_vertex_bytes: Option<&[u8]>,
    pixels_to_units: f32,
    source_rect_size: [f32; 2],
) -> Result<SpriteBoneGeometry, Box<dyn Error>> {
    let mesh = MeshView::from_render_data_value(render_data, version, endian).map_err(|error| {
        invalid_sprite_data(format!("could not read SpriteRenderData: {error}"))
    })?;
    let byte_source = mesh.vertex_data_bytes_source();
    let stream_is_set = mesh
        .stream_data
        .as_ref()
        .is_some_and(crate::resource::StreamedResourceInfo::is_set);
    let decoded = match mesh.vertex_data {
        Some(vertex_data) => VertexDataBlob::from_parts(
            vertex_data.channels,
            vertex_data.streams,
            match (byte_source, stream_is_set) {
                (MeshVertexBytesSource::Streamed, true) => resolved_stream_vertex_bytes
                    .ok_or_else(|| {
                        invalid_sprite_data(
                            "Sprite bone geometry requires unresolved streamed vertex bytes",
                        )
                    })?
                    .to_vec(),
                (MeshVertexBytesSource::Inline | MeshVertexBytesSource::Empty, _)
                | (MeshVertexBytesSource::Streamed, false) => vertex_data.data,
            },
            vertex_data.vertex_count,
            version,
            endian,
        )
        .and_then(|blob| blob.read_channels_for_mesh_merge())
        .map_err(|error| {
            invalid_sprite_data(format!("could not decode Sprite vertex data: {error}"))
        })?,
        None => None,
    };
    let mut geometry = SpriteBoneGeometry::default();
    if let Some(decoded) = decoded {
        geometry
            .vertices
            .try_reserve_exact(decoded.vertices.len())
            .map_err(|_| invalid_sprite_data("could not allocate Sprite bone vertices"))?;
        for vertex in decoded.vertices {
            geometry.vertices.push([
                vertex.x * pixels_to_units + source_rect_size[0] / 2.0,
                vertex.y * pixels_to_units + source_rect_size[1] / 2.0,
            ]);
        }
        geometry.weights = decoded.skin.unwrap_or_default();
    }
    geometry
        .indices
        .try_reserve_exact(mesh.index_buffer.len() / 2)
        .map_err(|_| invalid_sprite_data("could not allocate Sprite bone indices"))?;
    for pair in mesh.index_buffer.chunks_exact(2) {
        geometry
            .indices
            .push(i16::from_le_bytes([pair[0], pair[1]]));
    }
    Ok(geometry)
}

/// Sprite render-data mesh type: bit 6 of the render-data `settingsRaw`, matching the
/// witnessed imports (bit set reads back as the source `spriteMeshType` value).
fn sprite_mesh_type(value: &SerializedValue) -> Option<i32> {
    sprite_u32(value).map(|raw| ((raw >> 6) & 1) as i32)
}

fn sprite_u32(value: &SerializedValue) -> Option<u32> {
    match value {
        SerializedValue::U32(value) => Some(*value),
        SerializedValue::U8(value) => Some(u32::from(*value)),
        SerializedValue::U16(value) => Some(u32::from(*value)),
        SerializedValue::I32(value) if *value >= 0 => Some(*value as u32),
        _ => None,
    }
}

fn sprite_f32(value: &SerializedValue) -> Option<f32> {
    match value {
        SerializedValue::F32(value) => Some(*value),
        SerializedValue::I32(value) => Some(*value as f32),
        SerializedValue::U32(value) => Some(*value as f32),
        _ => None,
    }
}

fn yaml_key_is(key: &UnityYamlNode, name: &str) -> bool {
    match &key.kind {
        UnityYamlNodeKind::Scalar(crate::export::unity_yaml::UnityYamlScalar::PlainString(
            text,
        ))
        | UnityYamlNodeKind::Scalar(crate::export::unity_yaml::UnityYamlScalar::String(text)) => {
            text == name
        }
        _ => false,
    }
}

fn set_importer_signed(
    importer: &mut UnityYamlNode,
    name: &str,
    value: i64,
) -> Result<(), Box<dyn Error>> {
    set_importer_scalar(
        importer,
        name,
        UnityYamlNode::scalar(UnityYamlScalar::Signed(value)),
    )
}

fn set_importer_float(
    importer: &mut UnityYamlNode,
    name: &str,
    value: f32,
) -> Result<(), Box<dyn Error>> {
    // Importer float fields render with single precision like the source `float`
    // members; widening to `f64` first would print artifacts such as
    // `0.4999999701976776` instead of `0.49999997`.
    set_importer_scalar(
        importer,
        name,
        UnityYamlNode::scalar(UnityYamlScalar::Float32(value)),
    )
}

fn set_importer_point(
    importer: &mut UnityYamlNode,
    name: &str,
    components: &[(&str, f32)],
) -> Result<(), Box<dyn Error>> {
    let mut point = UnityYamlNode::mapping(crate::export::unity_yaml::UnityYamlMappingStyle::Flow);
    for (component, value) in components {
        point
            .insert_field(
                *component,
                UnityYamlNode::scalar(UnityYamlScalar::Float32(*value)),
            )
            .map_err(|_| invalid_sprite_data("could not build importer point"))?;
    }
    set_importer_scalar(importer, name, point)
}

fn set_importer_scalar(
    importer: &mut UnityYamlNode,
    name: &str,
    value: UnityYamlNode,
) -> Result<(), Box<dyn Error>> {
    let UnityYamlNodeKind::Mapping { entries, .. } = &mut importer.kind else {
        return Err(invalid_sprite_data("sprite importer is not a mapping"));
    };
    let Some((_, slot)) = entries.iter_mut().find(|(key, _)| yaml_key_is(key, name)) else {
        return Err(invalid_sprite_data(format!(
            "sprite importer has no {name} field"
        )));
    };
    *slot = value;
    Ok(())
}

fn invalid_sprite_data(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(io::Error::new(io::ErrorKind::InvalidData, message.into()))
}

/// Unconditional literals from `ImporterFactory.GenerateTextureImporter`.
pub mod texture_importer {
    /// `MipMaps.MipMapFadeDistanceStart`.
    pub const MIPS_FADE_DISTANCE_START: i32 = 1;
    /// `MipMaps.MipMapFadeDistanceEnd`.
    pub const MIPS_FADE_DISTANCE_END: i32 = 3;
    /// `MipMaps.AlphaTestReferenceValue`.
    pub const MIPS_ALPHA_TEST_REFERENCE_VALUE: f32 = 0.5;
    /// `BumpMap.HeightScale`.
    pub const BUMP_HEIGHT_SCALE: f32 = 0.25;
    /// `StreamingMipmapsPriority` is copied, but `CompressionQuality` is always 50.
    pub const COMPRESSION_QUALITY: i32 = 50;
    /// `SpriteExtrude` for the default and multi-sprite paths.
    pub const SPRITE_EXTRUDE: i32 = 1;
    /// `SpritePixelsToUnits` for the default and multi-sprite paths.
    pub const SPRITE_PIXELS_TO_UNITS: f32 = 100.0;
    /// `SpriteTessellationDetail`.
    pub const SPRITE_TESSELLATION_DETAIL: i32 = -1;
    /// `SpriteGenerateFallbackPhysicsShape`.
    pub const SPRITE_GENERATE_FALLBACK_PHYSICS_SHAPE: i32 = 1;
    /// `AlphaIsTransparency`.
    pub const ALPHA_IS_TRANSPARENCY: i32 = 1;
    /// The single platform-settings block target.
    pub const PLATFORM_BUILD_TARGET: &str = "DefaultTexturePlatform";
}

/// Platform settings from `ScriptExportCollection.OnAssemblyExported`.
pub mod plugin_importer {
    /// The always-enabled `Any` platform entry and its empty CPU value.
    pub const ANY_PLATFORM_NAME: &str = "Any";
    /// The disabled `Editor` platform entry and its CPU value.
    pub const EDITOR_PLATFORM_NAME: &str = "Editor";
    pub const EDITOR_CPU: &str = "Editor";
    /// `Settings["DefaultValueInitialized"]` on the editor entry.
    pub const DEFAULT_VALUE_INITIALIZED: (&str, &str) = ("DefaultValueInitialized", "true");
}

/// Which importer object a collection's `CreateImporter` override constructs.
///
/// Variant names are Rust descriptions; the `importer` strings in the table below carry the
/// exact baseline importer type names.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CreateImporterKind {
    /// `AssetExportCollection<T>`: a fresh `NativeFormatImporter` carrying the main export ID.
    NativeFormat,
    /// `AudioExportCollection`: `AudioImporter` with asset-copied fields.
    Audio,
    /// `FontAssetExportCollection`: `TrueTypeFontImporter` with font-copied fields.
    TrueTypeFont,
    /// `TextAssetExportCollection`: a bare `TextScriptImporter` plus bundle propagation.
    TextScript,
    /// `VideoClipExportCollection`: `VideoClipImporter` with frame fields and `ImportAudio`.
    VideoClip,
    /// `PrefabExportCollection`: base native importer or a fresh `PrefabImporter`.
    Prefab,
    /// `ShaderExportCollection`: `ShaderImporter` with a gated texture-map copy.
    Shader,
    /// `ImporterFactory.GenerateTextureImporter` result (`TextureImporter`).
    GeneratedTexture,
    /// `ImporterFactory.GenerateIHVImporter` result (`IHVImageFormatImporter`).
    GeneratedIhv,
    /// `SceneExportCollection.ExportScene`: `DefaultImporter` with bundle propagation.
    Default,
    /// `ScriptExportCollectionBase.OnScriptExported`: `MonoImporter` with truncated order.
    Mono,
    /// `ScriptExportCollection.OnAssemblyExported`: `PluginImporter` with platform entries.
    Plugin,
}

/// One `CreateImporter` override in the frozen baseline.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CreateImporterRegistration {
    /// The exporting collection type name.
    pub collection: &'static str,
    /// The importer type name the override constructs.
    pub importer: &'static str,
    /// Which factory shape builds it.
    pub kind: CreateImporterKind,
    /// True when the override delegates to `ImporterFactory` instead of constructing directly.
    pub via_importer_factory: bool,
}

/// Every `CreateImporter` override found by source search, in file-discovery order.
///
/// Subclasses without their own override (for example the concrete audio/texture collection
/// subclasses) inherit the nearest row above; the table records only methods the snapshot
/// actually declares.
pub const CREATE_IMPORTER_TABLE: &[CreateImporterRegistration] = &[
    CreateImporterRegistration {
        collection: "AssetExportCollection<T>",
        importer: "NativeFormatImporter",
        kind: CreateImporterKind::NativeFormat,
        via_importer_factory: false,
    },
    CreateImporterRegistration {
        collection: "AudioExportCollection",
        importer: "AudioImporter",
        kind: CreateImporterKind::Audio,
        via_importer_factory: false,
    },
    CreateImporterRegistration {
        collection: "FontAssetExportCollection",
        importer: "TrueTypeFontImporter",
        kind: CreateImporterKind::TrueTypeFont,
        via_importer_factory: false,
    },
    CreateImporterRegistration {
        collection: "TextAssetExportCollection",
        importer: "TextScriptImporter",
        kind: CreateImporterKind::TextScript,
        via_importer_factory: false,
    },
    CreateImporterRegistration {
        collection: "VideoClipExportCollection",
        importer: "VideoClipImporter",
        kind: CreateImporterKind::VideoClip,
        via_importer_factory: false,
    },
    CreateImporterRegistration {
        collection: "PrefabExportCollection",
        importer: "PrefabImporter",
        kind: CreateImporterKind::Prefab,
        via_importer_factory: false,
    },
    CreateImporterRegistration {
        collection: "ShaderExportCollection",
        importer: "ShaderImporter",
        kind: CreateImporterKind::Shader,
        via_importer_factory: false,
    },
    CreateImporterRegistration {
        collection: "LightmapExportCollection",
        importer: "TextureImporter",
        kind: CreateImporterKind::GeneratedTexture,
        via_importer_factory: true,
    },
    CreateImporterRegistration {
        collection: "TextureArrayAssetExportCollection",
        importer: "TextureImporter",
        kind: CreateImporterKind::GeneratedTexture,
        via_importer_factory: true,
    },
    CreateImporterRegistration {
        collection: "TextureExportCollection",
        importer: "TextureImporter",
        kind: CreateImporterKind::GeneratedTexture,
        via_importer_factory: true,
    },
];

/// The `TextureExportCollection` convert flag selects between the two `ImporterFactory`
/// entry points: a converting export uses `GenerateTextureImporter`, a non-converting one
/// uses `GenerateIHVImporter` (which warns below Unity 5.6 and calls `SetToDefault`).
pub fn select_texture_collection_importer(convert: bool) -> CreateImporterKind {
    if convert {
        CreateImporterKind::GeneratedTexture
    } else {
        CreateImporterKind::GeneratedIhv
    }
}

/// Unconditional literals inside the font, video, and IHV importer paths.
///
/// Asset-copied values (font sizes, audio settings, video dimensions, texture data) and the
/// numeric values of generated enums stay open; only the literal names and fixed booleans the
/// snapshot spells out are pinned here.
pub mod fixed_importer_literals {
    /// `TrueTypeFontImporter.IncludeFontData` is always set to true.
    pub const FONT_INCLUDE_FONT_DATA: bool = true;
    /// `TrueTypeFontImporter.AscentCalculationModeE` literal name.
    pub const FONT_ASCENT_CALCULATION_MODE: &str = "FaceAscender";
    /// `TrueTypeFontImporter.ForceTextureCaseE` literal name.
    pub const FONT_FORCE_TEXTURE_CASE: &str = "Dynamic";
    /// `VideoClipImporter.ImportAudio` is always set to true.
    pub const VIDEO_IMPORT_AUDIO: bool = true;
    /// `VideoClipImporter.FrameCount` always equals the just-written `EndFrame`.
    pub const VIDEO_FRAME_COUNT_EQUALS_END_FRAME: bool = true;
    /// `GenerateIHVImporter` warns when the export version is below this Unity version.
    pub const IHV_MINIMUM_VERSION: (u32, u32) = (5, 6);
    /// `GenerateTextureImporter` platform-settings literal names.
    pub const TEXTURE_RESIZE_ALGORITHM: &str = "Mitchell";
    pub const TEXTURE_PLATFORM_FORMAT: &str = "Automatic";
    pub const TEXTURE_PLATFORM_COMPRESSION: &str = "CompressedHQ";
    /// `GenerateTextureImporter` swizzle order literal names.
    pub const TEXTURE_SWIZZLE_ORDER: [&str; 4] = ["R", "G", "B", "A"];
}

#[cfg(test)]
mod tests {
    use super::{
        apply_sprite_importer_decision, apply_sprite_sheet_metadata_to_importer,
        decide_sprite_importer, importer_node_has_field, propagate_asset_bundle_name,
        read_sprite_bone_geometry, read_sprite_bones, read_sprite_physics_shape,
        read_sprite_texture_link, select_prefab_importer, sprite_export_id_for_ordinal,
        truncate_execution_order, PrefabImporterBranch, SpriteAlignment, SpriteFacts,
        SpriteImportMode, SpriteImporterDecision, SpriteOutlineTransform,
        SPRITE_MESH_TYPE_FULL_RECT, TEXTURE_IMPORTER_TYPE_SPRITE,
    };
    use super::{plugin_importer, texture_importer};
    use crate::export::unity_yaml::{UnityYamlNode, UnityYamlNodeKind, UnityYamlScalar};
    use crate::resources::vertex_data_blob::{
        ChannelInfo, EndianType, StreamInfo, UnityVersion, VertexDataBlob,
    };
    use crate::serialized_file::object::{SerializedFieldValue, SerializedValue};

    #[test]
    fn bundle_name_propagates_only_with_field_and_non_null_source() {
        assert_eq!(
            propagate_asset_bundle_name(true, Some("bundle-a")),
            Some("bundle-a".to_owned())
        );
        // The source checks null only, so an empty name propagates verbatim.
        assert_eq!(
            propagate_asset_bundle_name(true, Some("")),
            Some(String::new())
        );
        assert_eq!(propagate_asset_bundle_name(true, None), None);
        assert_eq!(propagate_asset_bundle_name(false, Some("bundle-a")), None);
        assert_eq!(propagate_asset_bundle_name(false, None), None);
    }

    #[test]
    fn execution_order_truncation_wraps_like_the_source_cast() {
        assert_eq!(truncate_execution_order(0), 0);
        assert_eq!(truncate_execution_order(300), 300);
        assert_eq!(truncate_execution_order(-5), -5);
        assert_eq!(truncate_execution_order(0x1_8001), -32767);
        assert_eq!(truncate_execution_order(i32::MAX), -1);
        assert_eq!(truncate_execution_order(i32::MIN), 0);
    }

    #[test]
    fn prefab_branch_follows_the_legacy_marker() {
        assert_eq!(
            select_prefab_importer(true),
            PrefabImporterBranch::LegacyPrefabAsset
        );
        assert_eq!(
            select_prefab_importer(false),
            PrefabImporterBranch::ModernPrefabImporter
        );
    }

    #[test]
    fn sprite_importer_follows_add_sprites_branches() {
        fn sprite(matching: bool) -> SpriteFacts {
            SpriteFacts {
                name: if matching { "texture" } else { "sprite" }.to_owned(),
                export_id: None,
                rect: [1.0, 2.0, 32.0, 16.0],
                source_rect_size: [32.0, 16.0],
                texture_rect: if matching {
                    [1.0, 2.0, 32.0, 16.0]
                } else {
                    [0.0, 0.0, 64.0, 64.0]
                },
                name_matches_texture: matching,
                extrude: 3,
                mesh_type: 7,
                has_pivot: true,
                pivot: [0.25, 0.75],
                has_border: true,
                border: [1.0, 2.0, 3.0, 4.0],
                pixels_to_units: 50.0,
                outline: Vec::new(),
                physics_shape: Vec::new(),
                bones: None,
                sprite_id: None,
                bone_geometry: super::SpriteBoneGeometry::default(),
            }
        }

        // No sprites: Single mode, factory extrude, FullRect, center alignment,
        // a centered pivot, 100 pixels per unit, texture type untouched, no sheet.
        let empty = decide_sprite_importer(&[], true, true, true);
        assert_eq!(empty.mode, SpriteImportMode::Single);
        assert_eq!(empty.mode as i32, 1);
        assert!(!empty.texture_type_is_sprite);
        assert_eq!(empty.extrude, 1);
        assert_eq!(empty.mesh_type, SPRITE_MESH_TYPE_FULL_RECT);
        assert_eq!(empty.mesh_type, 0);
        assert_eq!(empty.alignment, SpriteAlignment::Center);
        assert_eq!(empty.alignment as i32, 0);
        assert_eq!(empty.pivot, Some([0.5, 0.5]));
        assert_eq!(empty.border, None);
        assert_eq!(empty.pixels_to_units, 100.0);
        assert!(!empty.build_sprite_sheet);
        // Without an importer pivot field there is no pivot either.
        assert_eq!(decide_sprite_importer(&[], true, false, true).pivot, None);

        // One matching sprite: Single mode with the sprite's own geometry.
        let single = decide_sprite_importer(&[sprite(true)], true, true, true);
        assert_eq!(single.mode, SpriteImportMode::Single);
        assert!(single.texture_type_is_sprite);
        assert_eq!(TEXTURE_IMPORTER_TYPE_SPRITE, 8);
        assert_eq!(single.extrude, 3);
        assert_eq!(single.mesh_type, 7);
        assert_eq!(single.alignment, SpriteAlignment::Custom);
        assert_eq!(single.alignment as i32, 9);
        assert_eq!(single.pivot, Some([0.25, 0.75]));
        assert_eq!(single.border, Some([1.0, 2.0, 3.0, 4.0]));
        assert_eq!(single.pixels_to_units, 50.0);
        assert!(single.build_sprite_sheet);
        // The sheet builds only when sprite export is enabled.
        assert!(!decide_sprite_importer(&[sprite(true)], false, true, true).build_sprite_sheet);

        // One sprite whose rect or name differs: Multiple mode, same sprite values.
        for other in [sprite(false), sprite(true)] {
            let mut other = other;
            if other.rect == other.texture_rect {
                other.name_matches_texture = false;
            }
            let multiple = decide_sprite_importer(&[other], true, true, true);
            assert_eq!(multiple.mode, SpriteImportMode::Multiple);
            assert_eq!(multiple.mode as i32, 2);
            assert_eq!(multiple.alignment, SpriteAlignment::Custom);
            assert_eq!(multiple.extrude, 3);
        }

        // Missing sprite pivot or border suppresses that copy only.
        let mut no_pivot = sprite(true);
        no_pivot.has_pivot = false;
        let gated = decide_sprite_importer(&[no_pivot], true, true, true);
        assert_eq!(gated.pivot, None);
        assert_eq!(gated.border, Some([1.0, 2.0, 3.0, 4.0]));
        assert_eq!(
            decide_sprite_importer(&[sprite(true)], true, true, false).border,
            None
        );

        // Several sprites: Multiple mode with the first sprite's values.
        let many = decide_sprite_importer(&[sprite(false), sprite(true)], true, true, true);
        assert_eq!(many.mode, SpriteImportMode::Multiple);
        assert!(many.texture_type_is_sprite);
        assert_eq!(many.extrude, 3);
        assert_eq!(many.mesh_type, 7);
        assert_eq!(many.alignment, SpriteAlignment::Center);
        assert_eq!(many.pivot, Some([0.5, 0.5]));
        assert_eq!(many.border, None);
        assert_eq!(many.pixels_to_units, 50.0);
        assert!(many.build_sprite_sheet);
        assert!(
            !decide_sprite_importer(&[sprite(false), sprite(true)], false, true, true)
                .build_sprite_sheet
        );
    }

    /// Synthetic Sprite payload: a single full-rect sprite (name match, settingsRaw bit 6 set).
    fn menu_sprite_payload() -> SerializedValue {
        use crate::serialized_file::object::SerializedFieldValue;
        fn field(name: &str, value: SerializedValue) -> SerializedFieldValue {
            SerializedFieldValue {
                name: name.to_owned(),
                value,
            }
        }
        fn rect(x: f32, y: f32, width: f32, height: f32) -> SerializedValue {
            SerializedValue::Struct(vec![
                field("x", SerializedValue::F32(x)),
                field("y", SerializedValue::F32(y)),
                field("width", SerializedValue::F32(width)),
                field("height", SerializedValue::F32(height)),
            ])
        }
        SerializedValue::Struct(vec![
            field("m_Name", SerializedValue::StringBytes(b"menu".to_vec())),
            field("m_Rect", rect(0.0, 0.0, 1071.0, 755.0)),
            field("m_Offset", rect_point(0.0, 0.0)),
            field("m_Pivot", rect_point(0.5, 0.5)),
            field("m_Border", rect_border(0.0, 0.0, 0.0, 0.0)),
            field("m_PixelsToUnits", SerializedValue::F32(100.0)),
            field("m_Extrude", SerializedValue::U32(1)),
            field(
                "m_SpriteAtlas",
                SerializedValue::PPtr {
                    file_id: 0,
                    path_id: 0,
                },
            ),
            field(
                "m_RD",
                SerializedValue::Struct(vec![
                    field(
                        "texture",
                        SerializedValue::PPtr {
                            file_id: 0,
                            path_id: 12345,
                        },
                    ),
                    field("textureRect", rect(0.0, 0.0, 1071.0, 755.0)),
                    field("textureRectOffset", rect_point(0.0, 0.0)),
                    field("settingsRaw", SerializedValue::U32(64)),
                ]),
            ),
        ])
    }

    fn rect_point(x: f32, y: f32) -> SerializedValue {
        use crate::serialized_file::object::SerializedFieldValue;
        SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "x".to_owned(),
                value: SerializedValue::F32(x),
            },
            SerializedFieldValue {
                name: "y".to_owned(),
                value: SerializedValue::F32(y),
            },
        ])
    }

    fn rect_border(x: f32, y: f32, z: f32, w: f32) -> SerializedValue {
        use crate::serialized_file::object::SerializedFieldValue;
        SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "x".to_owned(),
                value: SerializedValue::F32(x),
            },
            SerializedFieldValue {
                name: "y".to_owned(),
                value: SerializedValue::F32(y),
            },
            SerializedFieldValue {
                name: "z".to_owned(),
                value: SerializedValue::F32(z),
            },
            SerializedFieldValue {
                name: "w".to_owned(),
                value: SerializedValue::F32(w),
            },
        ])
    }

    #[test]
    fn sprite_link_reads_menu_shaped_payload() {
        let root = menu_sprite_payload();
        let version = UnityVersion::parse("2022.3.0f1").unwrap();
        let link = read_sprite_texture_link(&root, Some(&version), "menu")
            .unwrap()
            .expect("menu sprite links");
        assert_eq!(link.texture_file_id, 0);
        assert_eq!(link.texture_path_id, 12345);
        assert_eq!(
            link.facts,
            SpriteFacts {
                name: "menu".to_owned(),
                export_id: None,
                rect: [0.0, 0.0, 1071.0, 755.0],
                source_rect_size: [1071.0, 755.0],
                texture_rect: [0.0, 0.0, 1071.0, 755.0],
                name_matches_texture: true,
                extrude: 1,
                mesh_type: 1,
                has_pivot: true,
                pivot: [0.5, 0.5],
                has_border: true,
                border: [0.0, 0.0, 0.0, 0.0],
                pixels_to_units: 100.0,
                outline: Vec::new(),
                physics_shape: Vec::new(),
                bones: None,
                sprite_id: None,
                bone_geometry: super::SpriteBoneGeometry::default(),
            }
        );
        // A different texture name only flips the match flag.
        let other = read_sprite_texture_link(&root, Some(&version), "other")
            .unwrap()
            .expect("links regardless");
        assert!(!other.facts.name_matches_texture);
        // A null texture pointer skips the sprite like the source null check.
        let mut null_root = root.clone();
        if let SerializedValue::Struct(fields) = &mut null_root {
            for field in fields.iter_mut() {
                if field.name == "m_RD" {
                    if let SerializedValue::Struct(rd) = &mut field.value {
                        for rd_field in rd.iter_mut() {
                            if rd_field.name == "texture" {
                                rd_field.value = SerializedValue::PPtr {
                                    file_id: 0,
                                    path_id: 0,
                                };
                            }
                        }
                    }
                }
            }
        }
        assert!(read_sprite_texture_link(&null_root, Some(&version), "menu")
            .unwrap()
            .is_none());
    }

    #[test]
    fn sprite_link_reports_ambiguous_and_malformed_required_fields() {
        use crate::serialized_file::object::SerializedFieldValue;

        let version = UnityVersion::parse("2022.3.0f1").unwrap();
        let mut duplicate = menu_sprite_payload();
        if let SerializedValue::Struct(fields) = &mut duplicate {
            fields.push(SerializedFieldValue {
                name: "m_RD".to_owned(),
                value: SerializedValue::Struct(Vec::new()),
            });
        }
        assert!(read_sprite_texture_link(&duplicate, Some(&version), "menu").is_err());

        let mut malformed = menu_sprite_payload();
        if let SerializedValue::Struct(fields) = &mut malformed {
            let rect = fields
                .iter_mut()
                .find(|field| field.name == "m_Rect")
                .unwrap();
            rect.value = SerializedValue::I32(7);
        }
        assert!(read_sprite_texture_link(&malformed, Some(&version), "menu").is_err());

        let mut missing = menu_sprite_payload();
        if let SerializedValue::Struct(fields) = &mut missing {
            fields.retain(|field| field.name != "m_Offset");
        }
        assert!(read_sprite_texture_link(&missing, Some(&version), "menu").is_err());
    }

    #[test]
    fn physics_shape_uses_source_ppu_pivot_and_packing_transform() {
        use crate::serialized_file::object::SerializedFieldValue;

        let root = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "m_PhysicsShape".to_owned(),
            value: SerializedValue::Array(vec![SerializedValue::Array(vec![
                rect_point(1.0, 2.0),
                rect_point(-1.0, 0.0),
            ])]),
        }]);
        let transform = SpriteOutlineTransform {
            rect_size: [8.0, 6.0],
            pivot: [0.75, 0.25],
            pixels_to_units: 2.0,
            is_packed: true,
            packing_rotation: 4,
        };

        assert_eq!(
            read_sprite_physics_shape(&root, transform).unwrap(),
            // Pivot shift is [2, -1.5], then PPU scaling and the 90-degree swap apply.
            vec![vec![[2.5, 4.0], [-1.5, 0.0]]],
        );
        assert_eq!(
            read_sprite_physics_shape(&SerializedValue::Struct(Vec::new()), transform).unwrap(),
            Vec::<Vec<[f32; 2]>>::new(),
        );
        let malformed = SerializedValue::Struct(vec![SerializedFieldValue {
            name: "m_PhysicsShape".to_owned(),
            value: SerializedValue::I32(7),
        }]);
        assert!(read_sprite_physics_shape(&malformed, transform).is_err());

        let ambiguous_wrapper = SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "data".to_owned(),
                value: SerializedValue::Struct(Vec::new()),
            },
            SerializedFieldValue {
                name: "data".to_owned(),
                value: SerializedValue::Struct(Vec::new()),
            },
        ]);
        assert!(read_sprite_physics_shape(&ambiguous_wrapper, transform).is_err());
    }

    #[test]
    fn sprite_decision_applies_to_built_importer() {
        use crate::export::unity_yaml::{
            write_unity_yaml_mapping, UnityYamlMappingStyle, UnityYamlScalar, UnityYamlWriteLimits,
        };
        let mut importer = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        for (name, value) in [
            (
                "spriteMode",
                UnityYamlNode::scalar(UnityYamlScalar::Signed(1)),
            ),
            (
                "spriteExtrude",
                UnityYamlNode::scalar(UnityYamlScalar::Signed(1)),
            ),
            (
                "spriteMeshType",
                UnityYamlNode::scalar(UnityYamlScalar::Signed(0)),
            ),
            (
                "alignment",
                UnityYamlNode::scalar(UnityYamlScalar::Signed(0)),
            ),
            (
                "spritePivot",
                UnityYamlNode::mapping(UnityYamlMappingStyle::Flow),
            ),
            (
                "spriteBorder",
                UnityYamlNode::mapping(UnityYamlMappingStyle::Flow),
            ),
            (
                "spritePixelsToUnits",
                UnityYamlNode::scalar(UnityYamlScalar::Float64(100.0)),
            ),
            (
                "textureType",
                UnityYamlNode::scalar(UnityYamlScalar::Signed(0)),
            ),
        ] {
            importer.insert_field(name, value).unwrap();
        }
        assert!(importer_node_has_field(&importer, "spritePivot"));
        assert!(!importer_node_has_field(&importer, "missing"));
        let root = menu_sprite_payload();
        let version = UnityVersion::parse("2022.3.0f1").unwrap();
        let link = read_sprite_texture_link(&root, Some(&version), "menu")
            .unwrap()
            .expect("menu sprite links");
        let decision = decide_sprite_importer(std::slice::from_ref(&link.facts), false, true, true);
        assert_eq!(decision.mode, SpriteImportMode::Single);
        apply_sprite_importer_decision(&mut importer, &decision).unwrap();
        let rendered =
            write_unity_yaml_mapping(&importer, UnityYamlWriteLimits::default()).unwrap();
        for line in [
            "spriteMode: 1",
            "spriteExtrude: 1",
            "spriteMeshType: 1",
            "alignment: 9",
            "spritePivot: {x: 0.5, y: 0.5}",
            "spriteBorder: {x: 0, y: 0, z: 0, w: 0}",
            "spritePixelsToUnits: 100",
            "textureType: 8",
        ] {
            assert!(rendered.contains(line), "{line}\n{rendered}");
        }
        // Field order is preserved by in-place overwrite.
        let order: Vec<&str> = rendered
            .lines()
            .filter_map(|line| line.split(':').next())
            .collect();
        assert_eq!(
            order,
            [
                "spriteMode",
                "spriteExtrude",
                "spriteMeshType",
                "alignment",
                "spritePivot",
                "spriteBorder",
                "spritePixelsToUnits",
                "textureType"
            ]
        );
    }

    #[test]
    fn sprite_sheet_outline_and_bone_geometry_are_written_for_single_and_multiple_modes() {
        use crate::export::unity_yaml::{UnityYamlMappingStyle, UnityYamlSequenceStyle};

        fn field<'a>(node: &'a UnityYamlNode, name: &str) -> &'a UnityYamlNode {
            let UnityYamlNodeKind::Mapping { entries, .. } = &node.kind else {
                panic!("expected a mapping while reading {name}");
            };
            entries
                .iter()
                .find(|(key, _)| super::yaml_key_is(key, name))
                .map(|(_, value)| value)
                .unwrap_or_else(|| panic!("missing field {name}"))
        }

        let sprite = SpriteFacts {
            name: "texture".to_owned(),
            export_id: Some(21_300_000),
            rect: [0.0, 0.0, 8.0, 8.0],
            source_rect_size: [8.0, 8.0],
            texture_rect: [0.0, 0.0, 8.0, 8.0],
            name_matches_texture: true,
            extrude: 1,
            mesh_type: 0,
            has_pivot: true,
            pivot: [0.5, 0.5],
            has_border: true,
            border: [0.0; 4],
            pixels_to_units: 100.0,
            outline: vec![vec![[1.25, -2.0], [3.0, 4.0]]],
            physics_shape: vec![vec![[0.0, 0.0], [1.0, 2.0]]],
            bones: None,
            sprite_id: None,
            bone_geometry: super::SpriteBoneGeometry::default(),
        };
        let mut sprite_sheet = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        sprite_sheet
            .insert_field(
                "outline",
                UnityYamlNode::sequence(UnityYamlSequenceStyle::Block),
            )
            .unwrap();
        sprite_sheet
            .insert_field(
                "physicsShape",
                UnityYamlNode::sequence(UnityYamlSequenceStyle::Block),
            )
            .unwrap();
        sprite_sheet
            .insert_field(
                "bones",
                UnityYamlNode::sequence(UnityYamlSequenceStyle::Block),
            )
            .unwrap();
        sprite_sheet
            .insert_field("spriteID", UnityYamlNode::string(String::new()))
            .unwrap();
        for name in ["vertices", "indices", "edges", "weights"] {
            sprite_sheet
                .insert_field(name, UnityYamlNode::sequence(UnityYamlSequenceStyle::Block))
                .unwrap();
        }
        sprite_sheet
            .insert_field(
                "sprites",
                UnityYamlNode::sequence(UnityYamlSequenceStyle::Block),
            )
            .unwrap();
        let mut importer = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        importer
            .insert_field(
                "fileIDToRecycleName",
                UnityYamlNode::mapping(UnityYamlMappingStyle::Block),
            )
            .unwrap();
        importer.insert_field("spriteSheet", sprite_sheet).unwrap();

        let single = decide_sprite_importer(std::slice::from_ref(&sprite), true, true, true);
        assert_eq!(single.mode, SpriteImportMode::Single);
        apply_sprite_sheet_metadata_to_importer(
            &mut importer,
            std::slice::from_ref(&sprite),
            &single,
        )
        .unwrap();

        let sheet = super::yaml_mapping_field_mut(&mut importer, "spriteSheet").unwrap();
        let outline = super::yaml_mapping_field_mut(sheet, "outline").unwrap();
        let UnityYamlNodeKind::Sequence {
            items: outlines, ..
        } = &outline.kind
        else {
            panic!("outline must be a sequence");
        };
        assert_eq!(outlines.len(), 1);
        let UnityYamlNodeKind::Sequence { items: points, .. } = &outlines[0].kind else {
            panic!("each outline must be a point sequence");
        };
        assert_eq!(points.len(), 2);
        let UnityYamlNodeKind::Mapping { entries, .. } = &points[0].kind else {
            panic!("outline points must be Vector2 mappings");
        };
        assert_eq!(entries.len(), 2);
        assert!(matches!(
            &entries[0].1.kind,
            UnityYamlNodeKind::Scalar(UnityYamlScalar::Float32(value)) if *value == 1.25
        ));
        assert!(matches!(
            &entries[1].1.kind,
            UnityYamlNodeKind::Scalar(UnityYamlScalar::Float32(value)) if *value == -2.0
        ));
        let physics_shape = super::yaml_mapping_field_mut(sheet, "physicsShape").unwrap();
        let UnityYamlNodeKind::Sequence {
            items: physics_polygons,
            ..
        } = &physics_shape.kind
        else {
            panic!("physics shape must be a polygon sequence");
        };
        assert_eq!(physics_polygons.len(), 1);
        let UnityYamlNodeKind::Sequence {
            items: physics_points,
            ..
        } = &physics_polygons[0].kind
        else {
            panic!("each physics polygon must be a point sequence");
        };
        assert_eq!(physics_points.len(), 2);

        let mut bones_sprite = sprite.clone();
        bones_sprite.bones = Some(vec![super::SpriteBoneData {
            name: "root".to_owned(),
            position: [1.0, 2.0, 3.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            length: 8.0,
            parent_id: -1,
            color: Some([1, 2, 3, 4]),
            guid: Some("00112233445566778899aabbccddeeff".to_owned()),
        }]);
        bones_sprite.sprite_id = Some("00000000000000000000000000000001".to_owned());
        bones_sprite.bone_geometry = super::SpriteBoneGeometry {
            vertices: vec![[6.0, 7.0]],
            indices: vec![0, -1],
            weights: vec![crate::numerics::BoneWeight4 {
                weights: [0.5, 0.25, 0.25, 0.0],
                indices: [0, 1, 2, 0],
            }],
        };
        let bones_decision =
            decide_sprite_importer(std::slice::from_ref(&bones_sprite), true, true, true);
        assert!(apply_sprite_sheet_metadata_to_importer(
            &mut importer,
            std::slice::from_ref(&bones_sprite),
            &bones_decision,
        )
        .is_ok());

        let sheet = field(&importer, "spriteSheet");
        assert!(matches!(
            &field(sheet, "spriteID").kind,
            UnityYamlNodeKind::Scalar(UnityYamlScalar::String(value))
                if value == "00000000000000000000000000000001"
        ));
        let UnityYamlNodeKind::Sequence { items: bones, .. } = &field(sheet, "bones").kind else {
            panic!("SpriteSheet.bones must be a sequence");
        };
        assert_eq!(bones.len(), 1);
        assert!(matches!(
            &field(&bones[0], "name").kind,
            UnityYamlNodeKind::Scalar(UnityYamlScalar::String(value)) if value == "root"
        ));
        assert!(matches!(
            &field(&bones[0], "parentId").kind,
            UnityYamlNodeKind::Scalar(UnityYamlScalar::Signed(-1))
        ));
        assert!(matches!(
            &field(&bones[0], "guid").kind,
            UnityYamlNodeKind::Scalar(UnityYamlScalar::String(value))
                if value == "00112233445566778899aabbccddeeff"
        ));
        let UnityYamlNodeKind::Sequence {
            items: vertices, ..
        } = &field(sheet, "vertices").kind
        else {
            panic!("SpriteSheet.vertices must be a sequence");
        };
        assert!(matches!(
            &field(&vertices[0], "x").kind,
            UnityYamlNodeKind::Scalar(UnityYamlScalar::Float32(value)) if *value == 6.0
        ));
        let UnityYamlNodeKind::Sequence { items: indices, .. } = &field(sheet, "indices").kind
        else {
            panic!("SpriteSheet.indices must be a sequence");
        };
        assert!(matches!(
            &indices[1].kind,
            UnityYamlNodeKind::Scalar(UnityYamlScalar::Signed(-1))
        ));
        let UnityYamlNodeKind::Sequence { items: edges, .. } = &field(sheet, "edges").kind else {
            panic!("SpriteSheet.edges must be a sequence");
        };
        assert!(edges.is_empty());
        let UnityYamlNodeKind::Sequence { items: weights, .. } = &field(sheet, "weights").kind
        else {
            panic!("SpriteSheet.weights must be a sequence");
        };
        assert_eq!(weights.len(), 1);
        assert!(matches!(
            &field(&weights[0], "weight[0]").kind,
            UnityYamlNodeKind::Scalar(UnityYamlScalar::Float32(value)) if *value == 0.5
        ));

        let mut multiple_sprite = bones_sprite;
        multiple_sprite.name_matches_texture = false;
        let multiple = decide_sprite_importer(&[multiple_sprite.clone()], true, true, true);
        assert_eq!(multiple.mode, SpriteImportMode::Multiple);
        apply_sprite_sheet_metadata_to_importer(&mut importer, &[multiple_sprite], &multiple)
            .unwrap();
        let sheet = field(&importer, "spriteSheet");
        let UnityYamlNodeKind::Sequence { items, .. } = &field(sheet, "sprites").kind else {
            panic!("SpriteSheet.sprites must be a sequence");
        };
        assert_eq!(items.len(), 1);
        assert!(matches!(
            &field(&items[0], "spriteID").kind,
            UnityYamlNodeKind::Scalar(UnityYamlScalar::String(value))
                if value == "00000000000000000000000000000001"
        ));
        assert!(matches!(
            &field(&items[0], "indices").kind,
            UnityYamlNodeKind::Sequence { items, .. } if items.len() == 2
        ));
    }

    #[test]
    fn sprite_bone_positions_and_lengths_follow_source_scaling() {
        fn field(name: &str, value: SerializedValue) -> SerializedFieldValue {
            SerializedFieldValue {
                name: name.to_owned(),
                value,
            }
        }

        fn vector(fields: &[(&str, f32)]) -> SerializedValue {
            SerializedValue::Struct(
                fields
                    .iter()
                    .map(|(name, value)| field(name, SerializedValue::F32(*value)))
                    .collect(),
            )
        }

        fn bone(name: &str, parent_id: i32, position: [f32; 3], length: f32) -> SerializedValue {
            SerializedValue::Struct(vec![
                field(
                    "name",
                    SerializedValue::StringBytes(name.as_bytes().to_vec()),
                ),
                field(
                    "position",
                    vector(&[("x", position[0]), ("y", position[1]), ("z", position[2])]),
                ),
                field(
                    "rotation",
                    vector(&[("x", 0.0), ("y", 0.5), ("z", 0.0), ("w", 1.0)]),
                ),
                field("length", SerializedValue::F32(length)),
                field("parentId", SerializedValue::I32(parent_id)),
            ])
        }

        let sprite = SerializedValue::Struct(vec![field(
            "m_Bones",
            SerializedValue::Array(vec![
                {
                    let mut root = match bone("root", -1, [1.0, 2.0, 3.0], 4.0) {
                        SerializedValue::Struct(fields) => fields,
                        _ => unreachable!(),
                    };
                    root.push(field(
                        "color",
                        SerializedValue::Struct(vec![
                            field("r", SerializedValue::U8(10)),
                            field("g", SerializedValue::U8(20)),
                            field("b", SerializedValue::U8(30)),
                            field("a", SerializedValue::U8(40)),
                        ]),
                    ));
                    root.push(field(
                        "guid",
                        SerializedValue::StringBytes(b"00112233445566778899aabbccddeeff".to_vec()),
                    ));
                    SerializedValue::Struct(root)
                },
                bone("child", 0, [1.0, -2.0, 3.0], 5.0),
            ]),
        )]);

        let bones = read_sprite_bones(&sprite, 2.0, [10.0, 6.0])
            .unwrap()
            .expect("m_Bones field is present");
        assert_eq!(bones.len(), 2);
        assert_eq!(bones[0].position, [7.0, 7.0, 6.0]);
        assert_eq!(bones[0].length, 8.0);
        assert_eq!(bones[0].color, Some([10, 20, 30, 40]));
        assert_eq!(
            bones[0].guid.as_deref(),
            Some("00112233445566778899aabbccddeeff")
        );
        assert_eq!(bones[1].position, [2.0, -4.0, 6.0]);
        assert_eq!(bones[1].length, 10.0);

        let malformed = SerializedValue::Struct(vec![field(
            "m_Bones",
            SerializedValue::Array(vec![SerializedValue::Struct(Vec::new())]),
        )]);
        assert!(read_sprite_bones(&malformed, 1.0, [0.0; 2]).is_err());
    }

    #[test]
    fn sprite_bone_geometry_decodes_positions_skin_and_little_endian_indices() {
        fn field(name: &str, value: SerializedValue) -> SerializedFieldValue {
            SerializedFieldValue {
                name: name.to_owned(),
                value,
            }
        }

        fn channel_value(channel: ChannelInfo) -> SerializedValue {
            SerializedValue::Struct(vec![
                field("dimension", SerializedValue::U8(channel.dimension)),
                field("offset", SerializedValue::U8(channel.offset)),
                field("format", SerializedValue::U8(channel.format)),
                field("stream", SerializedValue::U8(channel.stream)),
            ])
        }

        let version = UnityVersion::parse("2019.4.0f1").unwrap();
        let mut channels = vec![ChannelInfo::default(); 14];
        channels[0] = ChannelInfo {
            dimension: 3,
            offset: 0,
            format: 0,
            stream: 0,
        };
        channels[12] = ChannelInfo {
            dimension: 4,
            offset: 12,
            format: 0,
            stream: 0,
        };
        channels[13] = ChannelInfo {
            dimension: 4,
            offset: 28,
            format: 11,
            stream: 0,
        };
        let stream = StreamInfo {
            channel_mask: (1 << 0) | (1 << 12) | (1 << 13),
            offset: 0,
            stride: 44,
            stride_is_u32: false,
            divider_op: 0,
            frequency: 0,
        };
        let mut data = Vec::new();
        for (position, weights, indices) in [
            (
                [1.0f32, 2.0, 0.0],
                [0.5f32, 0.25, 0.25, 0.0],
                [0i32, 1, 2, 0],
            ),
            ([4.0f32, 5.0, 0.0], [1.0f32, 0.0, 0.0, 0.0], [3i32, 0, 0, 0]),
        ] {
            for value in position.into_iter().chain(weights) {
                data.extend_from_slice(&value.to_le_bytes());
            }
            for value in indices {
                data.extend_from_slice(&value.to_le_bytes());
            }
        }
        let blob = VertexDataBlob::from_parts(
            Some(channels.clone()),
            Some(vec![stream]),
            data,
            2,
            version,
            EndianType::Little,
        )
        .unwrap();
        let serialized_channels = channels.into_iter().map(channel_value).collect();
        let serialized_stream = SerializedValue::Struct(vec![
            field("channelMask", SerializedValue::U32(stream.channel_mask)),
            field("offset", SerializedValue::U32(stream.offset)),
            field("stride", SerializedValue::U8(stream.stride as u8)),
            field("dividerOp", SerializedValue::U8(stream.divider_op)),
            field("frequency", SerializedValue::U8(stream.frequency)),
        ]);
        let render_data = SerializedValue::Struct(vec![
            field(
                "m_VertexData",
                SerializedValue::Struct(vec![
                    field("m_VertexCount", SerializedValue::U32(2)),
                    field("m_Channels", SerializedValue::Array(serialized_channels)),
                    field("m_Streams", SerializedValue::Array(vec![serialized_stream])),
                    field("m_DataSize", SerializedValue::Bytes(blob.data)),
                ]),
            ),
            field(
                "m_IndexBuffer",
                SerializedValue::Bytes(vec![0, 0, 1, 0, 0xff, 0xff, 0x7f]),
            ),
        ]);
        let SerializedValue::Struct(render_fields) = &render_data else {
            unreachable!("render data fixture is a struct");
        };
        let vertex_bytes = match &render_fields[0].value {
            SerializedValue::Struct(vertex_fields) => match &vertex_fields[3].value {
                SerializedValue::Bytes(bytes) => bytes.clone(),
                _ => unreachable!("vertex data fixture bytes"),
            },
            _ => unreachable!("vertex data fixture is a struct"),
        };

        let geometry = read_sprite_bone_geometry(
            &render_data,
            version,
            EndianType::Little,
            None,
            2.0,
            [10.0, 6.0],
        )
        .unwrap();
        assert_eq!(geometry.vertices, vec![[7.0, 7.0], [13.0, 13.0]]);
        assert_eq!(geometry.indices, vec![0, 1, -1]);
        assert_eq!(geometry.weights.len(), 2);
        assert_eq!(geometry.weights[0].weights, [0.5, 0.25, 0.25, 0.0]);
        assert_eq!(geometry.weights[0].indices, [0, 1, 2, 0]);
        assert_eq!(geometry.weights[1].indices, [3, 0, 0, 0]);

        let mut streamed_render_data = render_data.clone();
        let SerializedValue::Struct(render_fields) = &mut streamed_render_data else {
            unreachable!("render data fixture is a struct");
        };
        let vertex_data = render_fields
            .iter_mut()
            .find(|field| field.name == "m_VertexData")
            .map(|field| &mut field.value)
            .unwrap();
        let SerializedValue::Struct(vertex_fields) = vertex_data else {
            unreachable!("vertex data fixture is a struct");
        };
        let data_field = vertex_fields
            .iter_mut()
            .find(|field| field.name == "m_DataSize")
            .unwrap();
        data_field.value = SerializedValue::Bytes(Vec::new());
        render_fields.push(field(
            "m_StreamData",
            SerializedValue::Struct(vec![
                field("path", SerializedValue::StringBytes(b"mesh.resS".to_vec())),
                field("offset", SerializedValue::U64(0)),
                field(
                    "size",
                    SerializedValue::U32(u32::try_from(vertex_bytes.len()).unwrap()),
                ),
            ]),
        ));

        let streamed_geometry = read_sprite_bone_geometry(
            &streamed_render_data,
            version,
            EndianType::Little,
            Some(&vertex_bytes),
            2.0,
            [10.0, 6.0],
        )
        .unwrap();
        assert_eq!(streamed_geometry, geometry);
        assert!(read_sprite_bone_geometry(
            &streamed_render_data,
            version,
            EndianType::Little,
            None,
            2.0,
            [10.0, 6.0],
        )
        .is_err());
    }

    #[test]
    fn legacy_multiple_sprite_sheet_writes_ordered_metadata_and_export_ids() {
        use crate::export::unity_yaml::{UnityYamlMappingStyle, UnityYamlSequenceStyle};

        fn field<'a>(node: &'a UnityYamlNode, name: &str) -> &'a UnityYamlNode {
            let UnityYamlNodeKind::Mapping { entries, .. } = &node.kind else {
                panic!("expected mapping while reading {name}");
            };
            entries
                .iter()
                .find(|(key, _)| super::yaml_key_is(key, name))
                .map(|(_, value)| value)
                .unwrap_or_else(|| panic!("missing field {name}"))
        }

        fn sprite(name: &str, export_id: i64, rect: [f32; 4]) -> SpriteFacts {
            SpriteFacts {
                name: name.to_owned(),
                export_id: Some(export_id),
                rect,
                source_rect_size: [rect[2], rect[3]],
                texture_rect: rect,
                name_matches_texture: false,
                extrude: 1,
                mesh_type: 0,
                has_pivot: true,
                pivot: [0.25, 0.75],
                has_border: true,
                border: [1.0, 2.0, 3.0, 4.0],
                pixels_to_units: 100.0,
                outline: vec![vec![[1.0, 2.0], [3.0, 4.0]]],
                physics_shape: vec![vec![[5.0, 6.0], [7.0, 8.0]]],
                bones: None,
                sprite_id: None,
                bone_geometry: super::SpriteBoneGeometry::default(),
            }
        }

        assert_eq!(sprite_export_id_for_ordinal(0).unwrap(), 21_300_000);
        assert_eq!(sprite_export_id_for_ordinal(1).unwrap(), 21_300_002);

        let sprites = [
            sprite("first", 21_300_000, [0.0, 0.0, 16.0, 8.0]),
            sprite("second", 21_300_002, [16.0, 0.0, 8.0, 16.0]),
        ];
        let decision = decide_sprite_importer(&sprites, true, true, true);
        assert_eq!(decision.mode, SpriteImportMode::Multiple);

        let mut sprite_sheet = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        sprite_sheet
            .insert_field(
                "sprites",
                UnityYamlNode::sequence(UnityYamlSequenceStyle::Block),
            )
            .unwrap();
        sprite_sheet
            .insert_field(
                "outline",
                UnityYamlNode::sequence(UnityYamlSequenceStyle::Block),
            )
            .unwrap();
        sprite_sheet
            .insert_field(
                "physicsShape",
                UnityYamlNode::sequence(UnityYamlSequenceStyle::Block),
            )
            .unwrap();
        let mut importer = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        importer
            .insert_field(
                "fileIDToRecycleName",
                UnityYamlNode::mapping(UnityYamlMappingStyle::Block),
            )
            .unwrap();
        importer.insert_field("spriteSheet", sprite_sheet).unwrap();

        apply_sprite_sheet_metadata_to_importer(&mut importer, &sprites, &decision).unwrap();

        let entries = field(field(&importer, "spriteSheet"), "sprites");
        let UnityYamlNodeKind::Sequence { items, .. } = &entries.kind else {
            panic!("SpriteSheet.sprites must be a sequence");
        };
        assert_eq!(items.len(), 2);
        assert!(matches!(
            &field(&items[0], "name").kind,
            crate::export::unity_yaml::UnityYamlNodeKind::Scalar(UnityYamlScalar::String(name))
                if name == "first"
        ));
        assert!(matches!(
            &field(&items[1], "name").kind,
            crate::export::unity_yaml::UnityYamlNodeKind::Scalar(UnityYamlScalar::String(name))
                if name == "second"
        ));
        assert!(matches!(
            &field(field(&items[0], "rect"), "width").kind,
            crate::export::unity_yaml::UnityYamlNodeKind::Scalar(UnityYamlScalar::Float32(width))
                if *width == 16.0
        ));
        assert!(matches!(
            &field(&items[1], "tessellationDetail").kind,
            crate::export::unity_yaml::UnityYamlNodeKind::Scalar(UnityYamlScalar::Signed(0))
        ));

        let UnityYamlNodeKind::Mapping { entries: ids, .. } =
            &field(&importer, "fileIDToRecycleName").kind
        else {
            panic!("fileIDToRecycleName must be a mapping");
        };
        assert_eq!(ids.len(), 2);
        assert!(matches!(
            &ids[0].0.kind,
            crate::export::unity_yaml::UnityYamlNodeKind::Scalar(
                UnityYamlScalar::PlainString(id)
            ) if id == "21300000"
        ));
        assert!(matches!(
            &ids[1].0.kind,
            crate::export::unity_yaml::UnityYamlNodeKind::Scalar(
                UnityYamlScalar::PlainString(id)
            ) if id == "21300002"
        ));
    }

    #[test]
    fn sprite_floats_render_with_single_precision() {
        use crate::export::unity_yaml::{
            write_unity_yaml_mapping, UnityYamlMappingStyle, UnityYamlScalar, UnityYamlWriteLimits,
        };
        let mut importer = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        for (name, value) in [
            (
                "spriteMode",
                UnityYamlNode::scalar(UnityYamlScalar::Signed(1)),
            ),
            (
                "spriteExtrude",
                UnityYamlNode::scalar(UnityYamlScalar::Signed(1)),
            ),
            (
                "spriteMeshType",
                UnityYamlNode::scalar(UnityYamlScalar::Signed(0)),
            ),
            (
                "alignment",
                UnityYamlNode::scalar(UnityYamlScalar::Signed(0)),
            ),
            (
                "spritePivot",
                UnityYamlNode::mapping(UnityYamlMappingStyle::Flow),
            ),
            (
                "spritePixelsToUnits",
                UnityYamlNode::scalar(UnityYamlScalar::Float64(100.0)),
            ),
            (
                "textureType",
                UnityYamlNode::scalar(UnityYamlScalar::Signed(0)),
            ),
        ] {
            importer.insert_field(name, value).unwrap();
        }
        let decision = SpriteImporterDecision {
            mode: SpriteImportMode::Multiple,
            texture_type_is_sprite: true,
            extrude: 1,
            mesh_type: 1,
            alignment: SpriteAlignment::Custom,
            pivot: Some([0.49999997, 0.5]),
            border: None,
            pixels_to_units: 50.0,
            build_sprite_sheet: false,
        };
        apply_sprite_importer_decision(&mut importer, &decision).unwrap();
        let rendered =
            write_unity_yaml_mapping(&importer, UnityYamlWriteLimits::default()).unwrap();
        // The exact `f32` bits render with `f32` precision like the source `float`
        // members; `f64` precision would print `0.4999999701976776`.
        assert!(
            rendered.contains("spritePivot: {x: 0.49999997, y: 0.5}"),
            "{rendered}"
        );
        assert!(rendered.contains("spritePixelsToUnits: 50"), "{rendered}");
    }

    #[test]
    fn texture_literals_match_generate_texture_importer() {
        assert_eq!(texture_importer::MIPS_FADE_DISTANCE_START, 1);
        assert_eq!(texture_importer::MIPS_FADE_DISTANCE_END, 3);
        assert_eq!(texture_importer::MIPS_ALPHA_TEST_REFERENCE_VALUE, 0.5);
        assert_eq!(texture_importer::BUMP_HEIGHT_SCALE, 0.25);
        assert_eq!(texture_importer::COMPRESSION_QUALITY, 50);
        assert_eq!(texture_importer::SPRITE_EXTRUDE, 1);
        assert_eq!(texture_importer::SPRITE_PIXELS_TO_UNITS, 100.0);
        assert_eq!(texture_importer::SPRITE_TESSELLATION_DETAIL, -1);
        assert_eq!(texture_importer::SPRITE_GENERATE_FALLBACK_PHYSICS_SHAPE, 1);
        assert_eq!(texture_importer::ALPHA_IS_TRANSPARENCY, 1);
        assert_eq!(
            texture_importer::PLATFORM_BUILD_TARGET,
            "DefaultTexturePlatform"
        );
    }

    #[test]
    fn plugin_platform_settings_match_on_assembly_exported() {
        assert_eq!(plugin_importer::ANY_PLATFORM_NAME, "Any");
        assert_eq!(plugin_importer::EDITOR_PLATFORM_NAME, "Editor");
        assert_eq!(plugin_importer::EDITOR_CPU, "Editor");
        assert_eq!(
            plugin_importer::DEFAULT_VALUE_INITIALIZED,
            ("DefaultValueInitialized", "true")
        );
    }

    #[test]
    fn create_importer_table_covers_every_snapshot_override() {
        use super::select_texture_collection_importer;
        use super::{fixed_importer_literals, CreateImporterKind, CREATE_IMPORTER_TABLE};
        // The base default plus the nine overrides found by source search.
        assert_eq!(CREATE_IMPORTER_TABLE.len(), 10);
        assert_eq!(
            CREATE_IMPORTER_TABLE[0].collection,
            "AssetExportCollection<T>"
        );
        assert_eq!(CREATE_IMPORTER_TABLE[0].importer, "NativeFormatImporter");
        assert_eq!(
            CREATE_IMPORTER_TABLE[0].kind,
            CreateImporterKind::NativeFormat
        );
        // Only the texture collections delegate to ImporterFactory.
        let factory_collections: Vec<&str> = CREATE_IMPORTER_TABLE
            .iter()
            .filter(|entry| entry.via_importer_factory)
            .map(|entry| entry.collection)
            .collect();
        assert_eq!(
            factory_collections,
            [
                "LightmapExportCollection",
                "TextureArrayAssetExportCollection",
                "TextureExportCollection"
            ]
        );
        // The prefab row records the modern branch; the legacy branch reuses the base
        // native importer through select_prefab_importer.
        let prefab = CREATE_IMPORTER_TABLE
            .iter()
            .find(|entry| entry.collection == "PrefabExportCollection")
            .unwrap();
        assert_eq!(prefab.importer, "PrefabImporter");
        // The texture convert flag selects between the two factory entry points.
        assert_eq!(
            select_texture_collection_importer(true),
            CreateImporterKind::GeneratedTexture
        );
        assert_eq!(
            select_texture_collection_importer(false),
            CreateImporterKind::GeneratedIhv
        );
        // Fixed literals are pinned by name, not by generated numeric value.
        let font_include_font_data = fixed_importer_literals::FONT_INCLUDE_FONT_DATA;
        assert!(font_include_font_data);
        assert_eq!(
            fixed_importer_literals::FONT_ASCENT_CALCULATION_MODE,
            "FaceAscender"
        );
        assert_eq!(fixed_importer_literals::FONT_FORCE_TEXTURE_CASE, "Dynamic");
        let video_import_audio = fixed_importer_literals::VIDEO_IMPORT_AUDIO;
        assert!(video_import_audio);
        let frame_count_follows_end_frame =
            fixed_importer_literals::VIDEO_FRAME_COUNT_EQUALS_END_FRAME;
        assert!(frame_count_follows_end_frame);
        assert_eq!(fixed_importer_literals::IHV_MINIMUM_VERSION, (5, 6));
        assert_eq!(
            fixed_importer_literals::TEXTURE_SWIZZLE_ORDER,
            ["R", "G", "B", "A"]
        );
    }
}
