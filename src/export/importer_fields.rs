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

use crate::export::unity_yaml::{UnityYamlNode, UnityYamlNodeKind, UnityYamlScalar};
use crate::resources::sprite::{
    generate_sprite_render_data_outlines_from_value, sprite_settings_raw_is_packed,
    sprite_settings_raw_packing_rotation, transform_sprite_outline_points, SpriteOutlineTransform,
};
use crate::resources::vertex_data_blob::{EndianType, UnityVersion};
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
    /// The sprite `Rect` (`[x, y, width, height]`).
    pub rect: [f32; 4],
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
    /// Whether the source Sprite layout exposes `m_Bones`.
    pub source_has_bones: bool,
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
/// importer path only; asset grouping and multiple-sprite sheets stay open. Facts follow
/// workspace encounter order. Source file version and endianness select the render-data
/// geometry decoder. When sprite-sheet export is enabled, malformed outline or physics-shape
/// geometry is an error rather than a silently omitted field.
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
        }
        facts
            .try_reserve(1)
            .map_err(|_| invalid_sprite_data("could not allocate sprite facts"))?;
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

/// Copies the source outline and physics-shape fields for a Single-mode sheet.
///
/// Multiple-mode sheets require per-sprite metadata records and planned export IDs. Until those
/// are connected, an enabled Multiple-mode export fails instead of emitting an empty sheet.
/// Bone metadata is also rejected when both source and target layouts expose it because the
/// source derives additional vertices, indices, and weights for that branch.
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
        return Err(invalid_sprite_data(
            "multiple SpriteSheet metadata entries and export IDs are not yet planned",
        ));
    }
    let [sprite] = sprites else {
        return Err(invalid_sprite_data(
            "Single SpriteSheet mode requires exactly one sprite fact",
        ));
    };
    if sprite.source_has_bones
        && importer_node_has_field(sprite_sheet, "bones")
        && importer_node_has_field(sprite_sheet, "spriteID")
    {
        return Err(invalid_sprite_data(
            "Sprite bone metadata and geometry are not yet planned",
        ));
    }
    let Some(outline_field) = yaml_mapping_field_mut(sprite_sheet, "outline") else {
        return write_sprite_physics_shape(sprite_sheet, &sprite.physics_shape);
    };
    *outline_field = sprite_outline_to_yaml(&sprite.outline)?;
    write_sprite_physics_shape(sprite_sheet, &sprite.physics_shape)
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
    let root =
        data_or_self(root).ok_or_else(|| invalid_sprite_data("Sprite payload is not a struct"))?;
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
            let point_value = data_or_self(source_point).ok_or_else(|| {
                invalid_sprite_data(format!(
                    "m_PhysicsShape polygon {shape_index} point {point_index} is not a Vector2 struct"
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
/// the queried texture. Returns `None` when the sprite is atlas-packed (recovery stays open),
/// when the render-data texture pointer is null, or when a required field is absent or mistyped.
///
/// Crucially, the facts carry the *processed* rect/pivot/border: `SpriteProcessor.ProcessSprite`
/// recalculates them from the render data (`GetSpriteCoordinatesInAtlas` plus copies), and
/// `AddSprites` reads those processed values — not the stale bundle `m_Rect`/`m_Pivot`.
fn read_sprite_texture_link<'a>(
    root: &'a SerializedValue,
    source_version: Option<&UnityVersion>,
    texture_name: &str,
) -> Option<SpriteTextureLink<'a>> {
    use crate::resources::sprite::get_sprite_coordinates_in_atlas;
    // Atlas-packed sprites need render-data recovery from the atlas map first, which
    // stays open; only atlas-null sprites take the direct render-data path.
    match sprite_field(root, "m_SpriteAtlas") {
        None => {}
        Some(SerializedValue::PPtr {
            file_id: 0,
            path_id: 0,
        }) => {}
        Some(_) => return None,
    }
    let rd = data_or_self(sprite_field(root, "m_RD")?)?;
    let (texture_file_id, texture_path_id) = match sprite_field(rd, "texture")? {
        SerializedValue::PPtr { file_id, path_id } => (*file_id, *path_id),
        _ => return None,
    };
    if texture_path_id == 0 {
        return None;
    }
    let sprite_rect = sprite_rect4(sprite_field(root, "m_Rect")?)?;
    let texture_rect = sprite_rect4(sprite_field(rd, "textureRect")?)?;
    let texture_rect_offset = sprite_point2(sprite_field(rd, "textureRectOffset")?)?;
    let offset = sprite_point2(sprite_field(root, "m_Offset")?)?;
    let mesh_type = sprite_mesh_type(sprite_field(rd, "settingsRaw")?)?;
    let stored_pivot = sprite_field(root, "m_Pivot").and_then(sprite_point2);
    let stored_border = sprite_field(root, "m_Border").and_then(sprite_point4);
    // `Has_Pivot`/`Has_Border` are layout gates, not value presence: every supported
    // layout carries the fields, so a present bundle value always counts; missing bundle
    // values on ancient layouts fall back to the derived/zero coordinates below.
    let layout_has_pivot = sprite_field(root, "m_Pivot").is_some()
        || source_version.is_some_and(|version| (version.major, version.minor) >= (5, 0));
    let layout_has_border = sprite_field(root, "m_Border").is_some()
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
    let name = sprite_name(root)?;
    let settings_raw = sprite_u32(sprite_field(rd, "settingsRaw")?)?;
    Some(SpriteTextureLink {
        facts: SpriteFacts {
            name: name.clone(),
            rect: coords.rect,
            texture_rect,
            name_matches_texture: name == texture_name,
            extrude: sprite_u32(sprite_field(root, "m_Extrude")?)?,
            mesh_type,
            has_pivot: layout_has_pivot,
            pivot: coords.pivot,
            has_border: layout_has_border,
            border: coords.border,
            pixels_to_units: sprite_f32(sprite_field(root, "m_PixelsToUnits")?)?,
            outline: Vec::new(),
            physics_shape: Vec::new(),
            source_has_bones: sprite_field(root, "m_Bones").is_some(),
        },
        texture_file_id,
        texture_path_id,
        render_data: rd,
        outline_transform: SpriteOutlineTransform {
            rect_size: [coords.rect[2], coords.rect[3]],
            pivot: coords.pivot,
            pixels_to_units: sprite_f32(sprite_field(root, "m_PixelsToUnits")?)?,
            is_packed: sprite_settings_raw_is_packed(settings_raw),
            packing_rotation: sprite_settings_raw_packing_rotation(settings_raw),
        },
    })
}

/// Sprite render-data mesh type: bit 6 of the render-data `settingsRaw`, matching the
/// witnessed imports (bit set reads back as the source `spriteMeshType` value).
fn sprite_mesh_type(value: &SerializedValue) -> Option<i32> {
    sprite_u32(value).map(|raw| ((raw >> 6) & 1) as i32)
}

fn sprite_name(root: &SerializedValue) -> Option<String> {
    match sprite_field(root, "m_Name")? {
        SerializedValue::StringBytes(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
        _ => None,
    }
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

fn sprite_rect4(value: &SerializedValue) -> Option<[f32; 4]> {
    let value = data_or_self(value)?;
    Some([
        sprite_f32(sprite_field(value, "x")?)?,
        sprite_f32(sprite_field(value, "y")?)?,
        sprite_f32(sprite_field(value, "width")?)?,
        sprite_f32(sprite_field(value, "height")?)?,
    ])
}

fn sprite_point2(value: &SerializedValue) -> Option<[f32; 2]> {
    let value = data_or_self(value)?;
    Some([
        sprite_f32(sprite_field(value, "x")?)?,
        sprite_f32(sprite_field(value, "y")?)?,
    ])
}

fn sprite_point4(value: &SerializedValue) -> Option<[f32; 4]> {
    let value = data_or_self(value)?;
    Some([
        sprite_f32(sprite_field(value, "x")?)?,
        sprite_f32(sprite_field(value, "y")?)?,
        sprite_f32(sprite_field(value, "z")?)?,
        sprite_f32(sprite_field(value, "w")?)?,
    ])
}

/// Find a struct field by name, following `data` wrappers like the payload readers do.
/// Lookup failures (missing field, duplicate field, non-struct scope) read as absent so
/// structurally unfamiliar sprites fall back to the default importer fields.
fn sprite_field<'a>(value: &'a SerializedValue, name: &str) -> Option<&'a SerializedValue> {
    data_or_self(value)?.unique_field(name).ok()?
}

/// Unwrap a `data` child when present, else read the struct itself.
fn data_or_self(value: &SerializedValue) -> Option<&SerializedValue> {
    let SerializedValue::Struct(_) = value else {
        return None;
    };
    if let Ok(Some(data)) = value.unique_field("data") {
        return Some(data);
    }
    Some(value)
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
        read_sprite_physics_shape, read_sprite_texture_link, select_prefab_importer,
        truncate_execution_order, PrefabImporterBranch, SpriteAlignment, SpriteFacts,
        SpriteImportMode, SpriteImporterDecision, SpriteOutlineTransform,
        SPRITE_MESH_TYPE_FULL_RECT, TEXTURE_IMPORTER_TYPE_SPRITE,
    };
    use super::{plugin_importer, texture_importer};
    use crate::export::unity_yaml::{UnityYamlNode, UnityYamlNodeKind, UnityYamlScalar};
    use crate::resources::vertex_data_blob::UnityVersion;
    use crate::serialized_file::object::SerializedValue;

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
                rect: [1.0, 2.0, 32.0, 16.0],
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
                source_has_bones: false,
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

    /// Menu-shaped bundle Sprite payload: the single full-rect sprite the frozen
    /// `menu.png` importer was built from (name match, settingsRaw bit 6 set).
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
        let link =
            read_sprite_texture_link(&root, Some(&version), "menu").expect("menu sprite links");
        assert_eq!(link.texture_file_id, 0);
        assert_eq!(link.texture_path_id, 12345);
        assert_eq!(
            link.facts,
            SpriteFacts {
                name: "menu".to_owned(),
                rect: [0.0, 0.0, 1071.0, 755.0],
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
                source_has_bones: false,
            }
        );
        // A different texture name only flips the match flag.
        let other =
            read_sprite_texture_link(&root, Some(&version), "other").expect("links regardless");
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
        assert!(read_sprite_texture_link(&null_root, Some(&version), "menu").is_none());
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
            vec![vec![[3.0, 4.0], [-1.0, 0.0]]],
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
        let link =
            read_sprite_texture_link(&root, Some(&version), "menu").expect("menu sprite links");
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
    fn sprite_sheet_outline_is_written_for_single_and_rejects_unplanned_multiple() {
        use crate::export::unity_yaml::{UnityYamlMappingStyle, UnityYamlSequenceStyle};

        let sprite = SpriteFacts {
            name: "texture".to_owned(),
            rect: [0.0, 0.0, 8.0, 8.0],
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
            source_has_bones: false,
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
        let mut importer = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
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
        bones_sprite.source_has_bones = true;
        let bones_decision =
            decide_sprite_importer(std::slice::from_ref(&bones_sprite), true, true, true);
        assert!(apply_sprite_sheet_metadata_to_importer(
            &mut importer,
            &[bones_sprite],
            &bones_decision,
        )
        .is_err());

        let mut multiple_sprite = sprite.clone();
        multiple_sprite.name_matches_texture = false;
        let multiple = decide_sprite_importer(&[multiple_sprite], true, true, true);
        assert_eq!(multiple.mode, SpriteImportMode::Multiple);
        assert!(
            apply_sprite_sheet_metadata_to_importer(&mut importer, &[sprite], &multiple,).is_err()
        );
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
