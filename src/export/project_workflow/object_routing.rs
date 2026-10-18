use super::*;

use crate::processing::asset_factory::FactoryUnityVersion;

/// Ordered `EngineAssetsTpk` blob band keys, one per line, in source blob order.
const ENGINE_ASSET_BAND_KEYS: &str = include_str!("engine_asset_bands.tsv");
const PREDEFINED_ENGINE_MATERIALS_BAND_2022_2_15F1: &str =
    include_str!("predefined_materials_band_2022_2_15f1.tsv");
const PREDEFINED_ENGINE_MATERIALS_BAND_2022_3_11F1: &str =
    include_str!("predefined_materials_band_2022_3_11f1.tsv");
const PREDEFINED_ENGINE_SHADERS_BAND_2022_2_15F1: &str =
    include_str!("predefined_shaders_band_2022_2_15f1.tsv");
const PREDEFINED_ENGINE_SHADERS_BAND_2022_3_11F1: &str =
    include_str!("predefined_shaders_band_2022_3_11f1.tsv");
const PREDEFINED_ENGINE_SPRITES_BAND_2022_2_15F1: &str =
    include_str!("predefined_sprites_band_2022_2_15f1.tsv");
const PREDEFINED_ENGINE_SPRITES_BAND_2022_3_11F1: &str =
    include_str!("predefined_sprites_band_2022_3_11f1.tsv");
const PREDEFINED_ENGINE_TEXTURES_BAND_2022_2_15F1: &str =
    include_str!("predefined_textures_band_2022_2_15f1.tsv");
const PREDEFINED_ENGINE_TEXTURES_BAND_2022_3_11F1: &str =
    include_str!("predefined_textures_band_2022_3_11f1.tsv");
const PREDEFINED_ENGINE_MATERIALS_BAND_6000_0_29F1: &str =
    include_str!("predefined_materials_band_6000_0_29f1.tsv");
const PREDEFINED_ENGINE_SHADERS_BAND_6000_0_29F1: &str =
    include_str!("predefined_shaders_band_6000_0_29f1.tsv");
const PREDEFINED_ENGINE_SPRITES_BAND_6000_0_29F1: &str =
    include_str!("predefined_sprites_band_6000_0_29f1.tsv");
const PREDEFINED_ENGINE_TEXTURES_BAND_6000_0_29F1: &str =
    include_str!("predefined_textures_band_6000_0_29f1.tsv");
const PREDEFINED_ENGINE_MATERIALS_BAND_2018_3_0B6: &str =
    include_str!("predefined_materials_band_2018_3_0b6.tsv");
const PREDEFINED_ENGINE_SHADERS_BAND_2018_3_0B6: &str =
    include_str!("predefined_shaders_band_2018_3_0b6.tsv");
const PREDEFINED_ENGINE_SPRITES_BAND_2018_3_0B6: &str =
    include_str!("predefined_sprites_band_2018_3_0b6.tsv");
const PREDEFINED_ENGINE_TEXTURES_BAND_2018_3_0B6: &str =
    include_str!("predefined_textures_band_2018_3_0b6.tsv");
const PREDEFINED_ENGINE_MESHES_BAND_2022_2_15F1: &str =
    include_str!("predefined_meshes_band_2022_2_15f1.tsv");
const PREDEFINED_ENGINE_MESHES_BAND_2022_3_11F1: &str =
    include_str!("predefined_meshes_band_2022_3_11f1.tsv");
const PREDEFINED_ENGINE_MESHES_BAND_6000_0_29F1: &str =
    include_str!("predefined_meshes_band_6000_0_29f1.tsv");
const PREDEFINED_ENGINE_MESHES_BAND_2018_3_0B6: &str =
    include_str!("predefined_meshes_band_2018_3_0b6.tsv");

/// Selects the `EngineAssetsTpk` band key for an export Unity version.
///
/// This mirrors the frozen `EngineAssetsExporter.CreateFromTpkBlob` selection over the
/// embedded blob band keys: the first key stays selected while the requested version does
/// not exceed it, otherwise the last key at or below the requested version wins.
/// Versions that cannot be ordered, and empty band lists, select nothing.
fn select_engine_assets_band(unity_version: Option<&str>) -> Option<&'static str> {
    let version = FactoryUnityVersion::parse(unity_version?.trim())?;
    let bits = version.source_order_bits()?;
    let mut entries = ENGINE_ASSET_BAND_KEYS.lines().filter_map(|line| {
        let key = line.trim();
        if key.is_empty() || key.starts_with('#') {
            return None;
        }
        let parsed = FactoryUnityVersion::parse(key)?;
        Some((parsed.source_order_bits()?, key))
    });
    let (mut previous_bits, mut previous_key) = entries.next()?;
    if bits <= previous_bits {
        return Some(previous_key);
    }
    let mut selected_key = previous_key;
    for (key_bits, key) in entries {
        selected_key = key;
        if bits <= key_bits {
            selected_key = previous_key;
            break;
        }
        previous_bits = key_bits;
        previous_key = key;
    }
    let _ = previous_bits;
    Some(selected_key)
}

/// Material identity rows for an `EngineAssetsTpk` band key.
///
/// Only probed bands have embedded rows; anything else selects nothing. Each band keeps
/// its own rows file so a future band refresh stays independent; the regression suite
/// pins the probe-verified fact that the two 2022.3 bands are currently byte-identical.
fn predefined_engine_material_rows(band: &str) -> Option<&'static str> {
    match band {
        "2018.3.0b6" => Some(PREDEFINED_ENGINE_MATERIALS_BAND_2018_3_0B6),
        "2022.2.15f1" => Some(PREDEFINED_ENGINE_MATERIALS_BAND_2022_2_15F1),
        "2022.3.11f1" => Some(PREDEFINED_ENGINE_MATERIALS_BAND_2022_3_11F1),
        "6000.0.29f1" => Some(PREDEFINED_ENGINE_MATERIALS_BAND_6000_0_29F1),
        _ => None,
    }
}

/// Shader identity rows for an `EngineAssetsTpk` band key.
///
/// Only probed bands have embedded rows; anything else selects nothing.
fn predefined_engine_shader_rows(band: &str) -> Option<&'static str> {
    match band {
        "2018.3.0b6" => Some(PREDEFINED_ENGINE_SHADERS_BAND_2018_3_0B6),
        "2022.2.15f1" => Some(PREDEFINED_ENGINE_SHADERS_BAND_2022_2_15F1),
        "2022.3.11f1" => Some(PREDEFINED_ENGINE_SHADERS_BAND_2022_3_11F1),
        "6000.0.29f1" => Some(PREDEFINED_ENGINE_SHADERS_BAND_6000_0_29F1),
        _ => None,
    }
}

fn predefined_engine_shader_target(
    shader_view: &ShaderView,
    unity_version: Option<&str>,
) -> Result<Option<PredefinedEngineAssetTarget>, Box<dyn Error>> {
    let Some(parsed) = shader_view.parsed.as_ref() else {
        return Ok(None);
    };
    let Some(band) = select_engine_assets_band(unity_version) else {
        return Ok(None);
    };
    let Some(table) = predefined_engine_shader_rows(band) else {
        return Ok(None);
    };
    for (line_index, line) in table.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.splitn(4, '|');
        let Some(file_id_text) = fields.next() else {
            return Err(invalid_project(format!(
                "predefined Shader table row {} has no file ID",
                line_index + 1
            )));
        };
        let Some(resource_kind) = fields.next() else {
            return Err(invalid_project(format!(
                "predefined Shader table row {} has no resource kind",
                line_index + 1
            )));
        };
        let Some(name) = fields.next() else {
            return Err(invalid_project(format!(
                "predefined Shader table row {} has no name",
                line_index + 1
            )));
        };
        let Some(property_names_json) = fields.next() else {
            return Err(invalid_project(format!(
                "predefined Shader table row {} has no property names",
                line_index + 1
            )));
        };
        if name != parsed.name {
            continue;
        }
        let properties_match = if property_names_json.is_empty() {
            parsed.properties.is_empty()
        } else {
            parsed
                .properties
                .iter()
                .map(|property| property.name.as_str())
                .eq(property_names_json.split_whitespace())
        };
        if !properties_match {
            continue;
        }
        let file_id = file_id_text.parse::<i64>().map_err(|error| {
            invalid_project(format!(
                "predefined Shader table row {} has an invalid file ID: {error}",
                line_index + 1
            ))
        })?;
        let words = match resource_kind {
            "D" => crate::export::guid_policy::ENGINE_DEFAULT_RESOURCES_GUID_WORDS,
            "F" => crate::export::guid_policy::ENGINE_BUILTIN_EXTRA_GUID_WORDS,
            other => {
                return Err(invalid_project(format!(
                    "predefined Shader table row {} has unknown resource kind {other:?}",
                    line_index + 1
                )));
            }
        };
        let mut guid = [0; 16];
        for (index, word) in words.iter().enumerate() {
            guid[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        return Ok(Some(PredefinedEngineAssetTarget { file_id, guid }));
    }
    Ok(None)
}

fn predefined_engine_material_target(
    name: &str,
    shader_name: Option<&str>,
    unity_version: Option<&str>,
) -> Result<Option<PredefinedEngineAssetTarget>, Box<dyn Error>> {
    let Some(band) = select_engine_assets_band(unity_version) else {
        return Ok(None);
    };
    let Some(table) = predefined_engine_material_rows(band) else {
        return Ok(None);
    };

    for (line_index, line) in table.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.splitn(4, '|');
        let Some(file_id_text) = fields.next() else {
            return Err(invalid_project(format!(
                "predefined Material table row {} has no file ID",
                line_index + 1
            )));
        };
        let Some(resource_kind) = fields.next() else {
            return Err(invalid_project(format!(
                "predefined Material table row {} has no resource kind",
                line_index + 1
            )));
        };
        let Some(candidate_name) = fields.next() else {
            return Err(invalid_project(format!(
                "predefined Material table row {} has no name",
                line_index + 1
            )));
        };
        let Some(candidate_shader) = fields.next() else {
            return Err(invalid_project(format!(
                "predefined Material table row {} has no Shader name",
                line_index + 1
            )));
        };
        if candidate_name != name || candidate_shader != shader_name.unwrap_or_default() {
            continue;
        }
        let file_id = file_id_text.parse::<i64>().map_err(|error| {
            invalid_project(format!(
                "predefined Material table row {} has an invalid file ID: {error}",
                line_index + 1
            ))
        })?;
        let words = match resource_kind {
            "D" => crate::export::guid_policy::ENGINE_DEFAULT_RESOURCES_GUID_WORDS,
            "F" => crate::export::guid_policy::ENGINE_BUILTIN_EXTRA_GUID_WORDS,
            other => {
                return Err(invalid_project(format!(
                    "predefined Material table row {} has unknown resource kind {other:?}",
                    line_index + 1
                )));
            }
        };
        let mut guid = [0; 16];
        for (index, word) in words.iter().enumerate() {
            guid[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        return Ok(Some(PredefinedEngineAssetTarget { file_id, guid }));
    }
    Ok(None)
}

/// Sprite identity rows for an `EngineAssetsTpk` band key.
///
/// Only probed bands have embedded rows; anything else selects nothing. Each
/// row is `fileID|resourceKind|spriteName|textureName`, mirroring the frozen
/// `SpriteKey(Name, TextureName)` match.
fn predefined_engine_sprite_rows(band: &str) -> Option<&'static str> {
    match band {
        "2018.3.0b6" => Some(PREDEFINED_ENGINE_SPRITES_BAND_2018_3_0B6),
        "2022.2.15f1" => Some(PREDEFINED_ENGINE_SPRITES_BAND_2022_2_15F1),
        "2022.3.11f1" => Some(PREDEFINED_ENGINE_SPRITES_BAND_2022_3_11F1),
        "6000.0.29f1" => Some(PREDEFINED_ENGINE_SPRITES_BAND_6000_0_29F1),
        _ => None,
    }
}

/// Texture2D identity rows for an `EngineAssetsTpk` band key.
///
/// Only probed bands have embedded rows; anything else selects nothing. Each
/// row is `fileID|resourceKind|textureName|width|height`, mirroring the frozen
/// `Texture2DKey(Name, Width, Height)` match.
fn predefined_engine_texture_rows(band: &str) -> Option<&'static str> {
    match band {
        "2018.3.0b6" => Some(PREDEFINED_ENGINE_TEXTURES_BAND_2018_3_0B6),
        "2022.2.15f1" => Some(PREDEFINED_ENGINE_TEXTURES_BAND_2022_2_15F1),
        "2022.3.11f1" => Some(PREDEFINED_ENGINE_TEXTURES_BAND_2022_3_11F1),
        "6000.0.29f1" => Some(PREDEFINED_ENGINE_TEXTURES_BAND_6000_0_29F1),
        _ => None,
    }
}

/// Mesh identity rows for an `EngineAssetsTpk` band key.
///
/// Only probed bands have embedded rows; anything else selects nothing. Each row is
/// `fileID|resourceKind|meshName|vertexCount|subMeshCount|cx|cy|cz|ex|ey|ez`, mirroring the
/// frozen `MeshKey(Name, VertexCount, SubMeshCount, LocalAABB)` match.
fn predefined_engine_mesh_rows(band: &str) -> Option<&'static str> {
    match band {
        "2018.3.0b6" => Some(PREDEFINED_ENGINE_MESHES_BAND_2018_3_0B6),
        "2022.2.15f1" => Some(PREDEFINED_ENGINE_MESHES_BAND_2022_2_15F1),
        "2022.3.11f1" => Some(PREDEFINED_ENGINE_MESHES_BAND_2022_3_11F1),
        "6000.0.29f1" => Some(PREDEFINED_ENGINE_MESHES_BAND_6000_0_29F1),
        _ => None,
    }
}

fn predefined_engine_guid_words(
    table_kind: &str,
    line_index: usize,
    resource_kind: &str,
) -> Result<[u8; 16], Box<dyn Error>> {
    let words = match resource_kind {
        "D" => crate::export::guid_policy::ENGINE_DEFAULT_RESOURCES_GUID_WORDS,
        "F" => crate::export::guid_policy::ENGINE_BUILTIN_EXTRA_GUID_WORDS,
        other => {
            return Err(invalid_project(format!(
                "predefined {table_kind} table row {} has unknown resource kind {other:?}",
                line_index + 1
            )));
        }
    };
    let mut guid = [0; 16];
    for (index, word) in words.iter().enumerate() {
        guid[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    Ok(guid)
}

fn predefined_engine_sprite_target(
    name: &str,
    texture_name: Option<&str>,
    unity_version: Option<&str>,
) -> Result<Option<PredefinedEngineAssetTarget>, Box<dyn Error>> {
    let Some(band) = select_engine_assets_band(unity_version) else {
        return Ok(None);
    };
    let Some(table) = predefined_engine_sprite_rows(band) else {
        return Ok(None);
    };
    let texture_name = texture_name.unwrap_or_default();
    for (line_index, line) in table.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.splitn(4, '|');
        let Some(file_id_text) = fields.next() else {
            return Err(invalid_project(format!(
                "predefined Sprite table row {} has no file ID",
                line_index + 1
            )));
        };
        let Some(resource_kind) = fields.next() else {
            return Err(invalid_project(format!(
                "predefined Sprite table row {} has no resource kind",
                line_index + 1
            )));
        };
        let Some(candidate_name) = fields.next() else {
            return Err(invalid_project(format!(
                "predefined Sprite table row {} has no name",
                line_index + 1
            )));
        };
        let Some(candidate_texture) = fields.next() else {
            return Err(invalid_project(format!(
                "predefined Sprite table row {} has no Texture name",
                line_index + 1
            )));
        };
        if candidate_name != name || candidate_texture != texture_name {
            continue;
        }
        let file_id = file_id_text.parse::<i64>().map_err(|error| {
            invalid_project(format!(
                "predefined Sprite table row {} has an invalid file ID: {error}",
                line_index + 1
            ))
        })?;
        let guid = predefined_engine_guid_words("Sprite", line_index, resource_kind)?;
        return Ok(Some(PredefinedEngineAssetTarget { file_id, guid }));
    }
    Ok(None)
}

fn predefined_engine_texture_target(
    name: &str,
    width: i32,
    height: i32,
    unity_version: Option<&str>,
) -> Result<Option<PredefinedEngineAssetTarget>, Box<dyn Error>> {
    let Some(band) = select_engine_assets_band(unity_version) else {
        return Ok(None);
    };
    let Some(table) = predefined_engine_texture_rows(band) else {
        return Ok(None);
    };
    for (line_index, line) in table.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.splitn(5, '|');
        let Some(file_id_text) = fields.next() else {
            return Err(invalid_project(format!(
                "predefined Texture table row {} has no file ID",
                line_index + 1
            )));
        };
        let Some(resource_kind) = fields.next() else {
            return Err(invalid_project(format!(
                "predefined Texture table row {} has no resource kind",
                line_index + 1
            )));
        };
        let Some(candidate_name) = fields.next() else {
            return Err(invalid_project(format!(
                "predefined Texture table row {} has no name",
                line_index + 1
            )));
        };
        let row_width = fields.next().and_then(|text| text.parse::<i32>().ok());
        let row_height = fields.next().and_then(|text| text.parse::<i32>().ok());
        if candidate_name != name || row_width != Some(width) || row_height != Some(height) {
            continue;
        }
        let file_id = file_id_text.parse::<i64>().map_err(|error| {
            invalid_project(format!(
                "predefined Texture table row {} has an invalid file ID: {error}",
                line_index + 1
            ))
        })?;
        let guid = predefined_engine_guid_words("Texture", line_index, resource_kind)?;
        return Ok(Some(PredefinedEngineAssetTarget { file_id, guid }));
    }
    Ok(None)
}

/// The frozen `PredefinedAssetCache.MeshKey` components read from one decoded class-43 Mesh:
/// name, vertex count, sub-mesh count and the local bounding box.
struct PredefinedEngineMeshKey {
    name: String,
    vertex_count: i32,
    sub_mesh_count: usize,
    center: [f32; 3],
    extent: [f32; 3],
}

/// The frozen `PredefinedAssetCache.MeshKey` decided from a decoded class-43 Mesh.
///
/// Every component must be readable; a mesh whose identity cannot be established keeps
/// its ordinary route instead of risking a wrong redirect.
fn predefined_engine_mesh_key(root: &SerializedValue) -> Option<PredefinedEngineMeshKey> {
    let name = predefined_engine_object_name(root)?;
    let vertex_data = root
        .unique_field_in_base_chain("m_VertexData")
        .ok()
        .flatten()?;
    let vertex_count = match vertex_data.unique_field("m_VertexCount").ok().flatten()? {
        SerializedValue::I32(count) => *count,
        SerializedValue::U32(count) => i32::try_from(*count).ok()?,
        _ => return None,
    };
    let sub_mesh_count = match root
        .unique_field_in_base_chain("m_SubMeshes")
        .ok()
        .flatten()?
    {
        SerializedValue::Array(sub_meshes) => sub_meshes.len(),
        SerializedValue::Bytes(bytes) => bytes.len(),
        _ => return None,
    };
    let aabb = root
        .unique_field_in_base_chain("m_LocalAABB")
        .ok()
        .flatten()?;
    let center = predefined_engine_vector3(aabb, "m_Center")?;
    let extent = predefined_engine_vector3(aabb, "m_Extent")?;
    Some(PredefinedEngineMeshKey {
        name,
        vertex_count,
        sub_mesh_count,
        center,
        extent,
    })
}

fn predefined_engine_vector3(value: &SerializedValue, field_name: &str) -> Option<[f32; 3]> {
    let vector = value.unique_field(field_name).ok().flatten()?;
    let mut components = [0.0f32; 3];
    for (index, name) in ["x", "y", "z"].into_iter().enumerate() {
        let component = vector.unique_field(name).ok().flatten()?;
        let SerializedValue::F32(component) = component else {
            return None;
        };
        components[index] = *component;
    }
    Some(components)
}

fn predefined_engine_mesh_target(
    name: &str,
    vertex_count: i32,
    sub_mesh_count: usize,
    center: [f32; 3],
    extent: [f32; 3],
    unity_version: Option<&str>,
) -> Result<Option<PredefinedEngineAssetTarget>, Box<dyn Error>> {
    let Some(band) = select_engine_assets_band(unity_version) else {
        return Ok(None);
    };
    let Some(table) = predefined_engine_mesh_rows(band) else {
        return Ok(None);
    };
    // The counts are compared as parsed numbers because the table stores one numeric
    // column kind; every value is a small integer or an exact float, so f64 is lossless.
    let expected: [f64; 8] = [
        f64::from(vertex_count),
        sub_mesh_count as f64,
        f64::from(center[0]),
        f64::from(center[1]),
        f64::from(center[2]),
        f64::from(extent[0]),
        f64::from(extent[1]),
        f64::from(extent[2]),
    ];
    for (line_index, line) in table.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = line.splitn(11, '|');
        let Some(file_id_text) = fields.next() else {
            return Err(invalid_project(format!(
                "predefined Mesh table row {} has no file ID",
                line_index + 1
            )));
        };
        let Some(resource_kind) = fields.next() else {
            return Err(invalid_project(format!(
                "predefined Mesh table row {} has no resource kind",
                line_index + 1
            )));
        };
        let Some(candidate_name) = fields.next() else {
            return Err(invalid_project(format!(
                "predefined Mesh table row {} has no name",
                line_index + 1
            )));
        };
        if candidate_name != name {
            continue;
        }
        // The frozen `MeshKey` compares the vertex count, the sub-mesh count and all six
        // bounding-box components exactly, so every numeric field must parse.
        let mut numbers = [0f64; 8];
        let mut malformed = false;
        for slot in numbers.iter_mut() {
            match fields.next().and_then(|text| text.parse::<f64>().ok()) {
                Some(value) => *slot = value,
                None => {
                    malformed = true;
                    break;
                }
            }
        }
        if malformed || fields.next().is_some() {
            return Err(invalid_project(format!(
                "predefined Mesh table row {} has malformed numeric fields",
                line_index + 1
            )));
        }
        if numbers != expected {
            continue;
        }
        let file_id = file_id_text.parse::<i64>().map_err(|error| {
            invalid_project(format!(
                "predefined Mesh table row {} has an invalid file ID: {error}",
                line_index + 1
            ))
        })?;
        let guid = predefined_engine_guid_words("Mesh", line_index, resource_kind)?;
        return Ok(Some(PredefinedEngineAssetTarget { file_id, guid }));
    }
    Ok(None)
}

/// Lenient `m_Name` read for predefined engine-asset matching.
/// Missing fields, wrong types, and non-UTF-8 bytes select no redirect rather
/// than failing the export: the redirect is opportunistic and the asset keeps
/// its ordinary route when identity cannot be established.
fn predefined_engine_object_name(root: &SerializedValue) -> Option<String> {
    let value = root.unique_field_in_base_chain("m_Name").ok()??;
    match value {
        SerializedValue::StringBytes(bytes) | SerializedValue::Bytes(bytes) => {
            std::str::from_utf8(bytes).ok().map(str::to_owned)
        }
        _ => None,
    }
}

fn predefined_engine_texture_dimensions(root: &SerializedValue) -> Option<(i32, i32)> {
    let width = root.unique_field_in_base_chain("m_Width").ok()??;
    let height = root.unique_field_in_base_chain("m_Height").ok()??;
    let SerializedValue::I32(width) = width else {
        return None;
    };
    let SerializedValue::I32(height) = height else {
        return None;
    };
    Some((*width, *height))
}

/// Resolves a Sprite render-data texture pointer to the name used for the
/// group's predefined-engine lookup, mirroring the frozen `Cache.Contains(sio.Texture)`
/// gate.
///
/// The outer `None` means "no engine-group redirect applies", which keeps the
/// sprite's ordinary export: the pointer is unresolvable, the target is not a
/// Texture2D, or the target texture is not a predefined engine texture once its
/// name *and* dimensions are considered. `Some(name)` means the whole
/// `SpriteInformationObject` group redirects, and the sprite's own table entry
/// is then consulted with that texture name (a null or unreadable name stays
/// `Some(None)`, preserving the name-only lookup the frozen cache also allows).
fn resolve_engine_group_texture_name(
    field_path: &str,
    file_id: i32,
    path_id: i64,
    source: &WorkspaceObject,
    objects: &ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
    unity_version: Option<&str>,
) -> Result<Option<Option<String>>, Box<dyn Error>> {
    let mut matching_edges = reference_graph
        .references_from_object(source.file_index, source.object_index)
        .filter(|edge| {
            edge.field_path == field_path && edge.file_id == file_id && edge.path_id == path_id
        });
    let Some(edge) = matching_edges.next() else {
        return Ok(None);
    };
    let MultiFilePPtrResolution::Object {
        target_file_index,
        target_object_index,
    } = &edge.resolution
    else {
        return Ok(None);
    };
    if reference_graph.object_class_id(*target_file_index, *target_object_index)
        != Some(crate::export::res_wiring::TEXTURE2D_CLASS_ID)
    {
        return Ok(None);
    }
    let Some(target) = objects.get(*target_file_index, *target_object_index) else {
        return Ok(None);
    };
    let Ok(payload) = &target.payload else {
        return Ok(None);
    };
    let Some(name) = predefined_engine_object_name(&payload.root) else {
        return Ok(None);
    };
    let Some((width, height)) = predefined_engine_texture_dimensions(&payload.root) else {
        return Ok(None);
    };
    match predefined_engine_texture_target(&name, width, height, unity_version)? {
        Some(_) => Ok(Some(Some(name))),
        None => Ok(None),
    }
}

fn resolved_material_shader_name(
    material: &crate::resources::material::MaterialView,
    source: &WorkspaceObject,
    objects: &ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
) -> Result<Option<Option<String>>, Box<dyn Error>> {
    let Some((file_id, path_id)) = material.shader else {
        return Ok(Some(None));
    };
    let mut matching_edges = reference_graph
        .references_from_object(source.file_index, source.object_index)
        .filter(|edge| {
            edge.field_path == "m_Shader" && edge.file_id == file_id && edge.path_id == path_id
        });
    let Some(edge) = matching_edges.next() else {
        return Err(invalid_project(format!(
            "Material {}:{} has non-null m_Shader PPtr ({file_id}, {path_id}) without a matching reference-graph edge",
            source.file_index, source.path_id
        )));
    };
    if matching_edges.next().is_some() {
        return Err(invalid_project(format!(
            "Material {}:{} has duplicate reference-graph edges for m_Shader PPtr ({file_id}, {path_id})",
            source.file_index, source.path_id
        )));
    }
    match &edge.resolution {
        MultiFilePPtrResolution::Object {
            target_file_index,
            target_object_index,
        } if reference_graph.object_class_id(*target_file_index, *target_object_index)
            == Some(SHADER_CLASS_ID) =>
        {
            let Some(target) = objects.get(*target_file_index, *target_object_index) else {
                return Err(invalid_project(format!(
                    "Material {}:{} m_Shader resolves to missing workspace object {target_file_index}:{target_object_index}",
                    source.file_index, source.path_id
                )));
            };
            let Ok(payload) = &target.payload else {
                return Ok(None);
            };
            let view = ShaderView::from_value(&payload.root).map_err(|error| {
                invalid_project(format!(
                    "Material {}:{} resolved Shader {}:{} could not be read: {error}",
                    source.file_index, source.path_id, target.file_index, target.path_id
                ))
            })?;
            Ok(view.parsed.map(|form| Some(form.name)))
        }
        MultiFilePPtrResolution::PredefinedEngineAsset {
            resource,
            path_id,
            class_id,
            ..
        } if *class_id == SHADER_CLASS_ID => Ok(
            predefined_engine_shader_name(*resource, *path_id).map(|name| Some(name.to_owned()))
        ),
        _ => Ok(None),
    }
}

fn predefined_engine_shader_name(
    resource: crate::serialized_file::reference_graph::PredefinedEngineResourceKind,
    path_id: i64,
) -> Option<&'static str> {
    let resource_kind = match resource {
        crate::serialized_file::reference_graph::PredefinedEngineResourceKind::DefaultResources => {
            "D"
        }
    };
    // Default-resource shader rows are probe-verified identical across the embedded bands,
    // so the reverse name lookup is band-independent.
    [
        PREDEFINED_ENGINE_SHADERS_BAND_2022_2_15F1,
        PREDEFINED_ENGINE_SHADERS_BAND_2022_3_11F1,
    ]
    .into_iter()
    .flat_map(str::lines)
    .find_map(|line| {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            return None;
        }
        let mut fields = line.splitn(4, '|');
        let file_id = fields.next()?.parse::<i64>().ok()?;
        let candidate_resource = fields.next()?;
        let name = fields.next()?;
        (file_id == path_id && candidate_resource == resource_kind).then_some(name)
    })
}

fn project_object_name(
    class_id: i32,
    payload: Option<&ParsedObjectPayload>,
    shader_view: Option<&ShaderView>,
) -> Option<String> {
    if class_id == SHADER_CLASS_ID {
        if let Some(name) = shader_view
            .and_then(|view| view.parsed.as_ref())
            .map(|form| form.name.as_str())
            .filter(|name| !name.is_empty())
        {
            // In the current Shader layout, the generated IShader.Name property reads
            // m_ParsedForm.Name. The root m_Name field is not the export identity.
            return Some(name.to_owned());
        }
    }
    payload.and_then(object_asset_name)
}

fn texture_has_exportable_image_payload(payload: &ParsedObjectPayload) -> bool {
    let Ok(view) = crate::resources::texture::Texture2DView::from_value(&payload.root) else {
        return false;
    };
    view.width > 0
        && view.height > 0
        && (!view.image_data.is_empty()
            || view
                .stream_data
                .as_ref()
                .is_some_and(|stream| stream.is_set() && stream.size > 0))
}

/// Predicts the materialization skip for a streamed generic-asset `Texture2D`:
/// no inline image bytes and no viable stream range. Dimensions are
/// irrelevant (degenerate native textures are still written); a set stream
/// with a positive size is optimistically treated as writable, with missing
/// or empty resolved bytes left to the loud materialization paths.
fn texture_lacks_usable_image_data(payload: Option<&ParsedObjectPayload>) -> bool {
    let Some(payload) = payload else {
        return false;
    };
    let Ok(view) = crate::resources::texture::Texture2DView::from_value(&payload.root) else {
        return false;
    };
    view.image_data.is_empty()
        && !view
            .stream_data
            .as_ref()
            .is_some_and(|stream| stream.is_set() && stream.size > 0)
}

fn texture3d_has_exportable_image_payload(payload: &ParsedObjectPayload) -> bool {
    let Ok(view) = crate::resources::texture_array::Texture3DView::from_value(&payload.root) else {
        return false;
    };
    view.width > 0
        && view.height > 0
        && view.depth > 0
        && (!view.image_data.is_empty()
            || view
                .stream_data
                .as_ref()
                .is_some_and(|stream| stream.is_set() && stream.size > 0))
}

fn texture_array_has_exportable_image_payload(
    payload: &ParsedObjectPayload,
    class_id: i32,
) -> bool {
    let (width, height, depth, image_data_is_empty, stream_data) = match class_id {
        crate::resources::texture_array::TEXTURE2DARRAY_CLASS_ID => {
            let Ok(view) =
                crate::resources::texture_array::Texture2DArrayView::from_value(&payload.root)
            else {
                return false;
            };
            (
                view.width,
                view.height,
                view.depth,
                view.image_data.is_empty(),
                view.stream_data,
            )
        }
        crate::resources::texture_array::CUBEMAPARRAY_CLASS_ID => {
            let Ok(view) =
                crate::resources::texture_array::CubemapArrayView::from_value(&payload.root)
            else {
                return false;
            };
            let Ok(depth) =
                crate::resources::texture_array::cubemap_array_depth(view.cubemap_count)
            else {
                return false;
            };
            (
                view.width,
                view.width,
                usize::try_from(depth).unwrap_or(0),
                view.image_data.is_empty(),
                view.stream_data,
            )
        }
        _ => return false,
    };
    width > 0
        && height > 0
        && depth > 0
        && (!image_data_is_empty
            || stream_data.is_some_and(|stream| stream.is_set() && stream.size > 0))
}

fn source_has_texture_array_exporter(project_version: SceneUnityVersion) -> bool {
    (project_version.major, project_version.minor) >= (2020, 2)
}

fn texture_array_project_route_supported(
    class_id: i32,
    project_version: SceneUnityVersion,
    image_payload_exportable: bool,
) -> bool {
    image_payload_exportable
        && matches!(
            class_id,
            crate::resources::texture_array::TEXTURE2DARRAY_CLASS_ID
                | crate::resources::texture_array::CUBEMAPARRAY_CLASS_ID
        )
        && source_has_texture_array_exporter(project_version)
        && TextureImporterLayout::for_project_version(project_version)
            == Some(TextureImporterLayout::Unity2022_3)
}

fn should_report_unported_texture_route(
    image_payload_exportable: bool,
    texture_route_supported: bool,
    yaml_lightmap_route: bool,
) -> bool {
    image_payload_exportable && !texture_route_supported && !yaml_lightmap_route
}

fn read_audio_data_for_project_export(
    context: &ProjectRoutingContext<'_, '_, '_>,
    file_index: usize,
    path_id: i64,
    view: &crate::resources::audio::AudioClipView,
) -> Result<Vec<u8>, Box<dyn Error>> {
    use crate::resources::audio::AudioDataSource;

    match view.audio_data_source() {
        AudioDataSource::Inline(bytes) => Ok(bytes.to_vec()),
        AudioDataSource::Empty => Ok(Vec::new()),
        AudioDataSource::Resource(resource) if resource.size == 0 => Ok(Vec::new()),
        AudioDataSource::Resource(resource) => {
            if resource.path.is_empty() {
                return Err(invalid_project(format!(
                    "file {file_index} AudioClip PathID {path_id} declares {} streamed bytes with an empty resource path",
                    resource.size
                )));
            }
            let source_bundle_index = context
                .workspace
                .files()
                .get(file_index)
                .ok_or_else(|| {
                    invalid_project(format!(
                        "AudioClip references absent SerializedFile {file_index}"
                    ))
                })?
                .container_bundle_index;
            match resolve_and_read_streamed_resource_range(
                context.filesystem,
                context.workspace,
                source_bundle_index,
                StreamedResourceRange {
                    path: &resource.path,
                    offset: resource.offset,
                    size: resource.size,
                },
                context.root_catalog,
                context.resource_provider,
                context.limits.streamed_content,
            )
            .map_err(|error| {
                invalid_project(format!(
                    "file {file_index} AudioClip PathID {path_id} stream {:?} could not be read: {error}",
                    resource.path
                ))
            })? {
                StreamedResourceContentResolution::Resolved { bytes, .. } => Ok(bytes),
                StreamedResourceContentResolution::Missing {
                    unavailable_data_paths,
                } => Err(invalid_project(format!(
                    "file {file_index} AudioClip PathID {path_id} stream {:?} is missing; unavailable data paths: [{}]",
                    resource.path,
                    unavailable_data_paths.join(", ")
                ))),
                StreamedResourceContentResolution::Unset => Err(invalid_project(format!(
                    "file {file_index} AudioClip PathID {path_id} stream {:?} became unset during resolution",
                    resource.path
                ))),
            }
        }
    }
}

fn read_sprite_vertex_stream_for_project_export(
    context: &ProjectRoutingContext<'_, '_, '_>,
    file_index: usize,
    path_id: i64,
    render_data: &SerializedValue,
    version: crate::resources::vertex_data_blob::UnityVersion,
    endian: crate::resources::vertex_data_blob::EndianType,
) -> Result<Option<Vec<u8>>, Box<dyn Error>> {
    let mesh = crate::resources::mesh::MeshView::from_render_data_value(
        render_data,
        version,
        endian,
    )
    .map_err(|error| {
        invalid_project(format!(
            "file {file_index} Sprite PathID {path_id} render data could not be read for outline stream resolution: {error}"
        ))
    })?;
    if mesh.vertex_data_bytes_source() != crate::resources::mesh::MeshVertexBytesSource::Streamed {
        return Ok(None);
    }
    let Some(resource) = mesh.stream_data.filter(|resource| resource.is_set()) else {
        return Ok(None);
    };
    if resource.size == 0 {
        return Ok(Some(Vec::new()));
    }
    let source_bundle_index = context
        .workspace
        .files()
        .get(file_index)
        .ok_or_else(|| {
            invalid_project(format!(
                "Sprite references absent SerializedFile {file_index}"
            ))
        })?
        .container_bundle_index;
    match resolve_and_read_streamed_resource_range(
        context.filesystem,
        context.workspace,
        source_bundle_index,
        StreamedResourceRange {
            path: &resource.path,
            offset: resource.offset,
            size: u64::from(resource.size),
        },
        context.root_catalog,
        context.resource_provider,
        context.limits.streamed_content,
    )
    .map_err(|error| {
        invalid_project(format!(
            "file {file_index} Sprite PathID {path_id} streamed vertex bytes could not be read: {error}"
        ))
    })? {
        StreamedResourceContentResolution::Resolved { bytes, .. } => Ok(Some(bytes)),
        StreamedResourceContentResolution::Missing {
            unavailable_data_paths,
        } => Err(invalid_project(format!(
            "file {file_index} Sprite PathID {path_id} streamed vertex bytes are missing; unavailable data paths: [{}]",
            unavailable_data_paths.join(", ")
        ))),
        StreamedResourceContentResolution::Unset => Err(invalid_project(format!(
            "file {file_index} Sprite PathID {path_id} vertex stream became unset during resolution"
        ))),
    }
}

pub(super) fn route_project_objects<'context, 'provider, 'graph>(
    context: &ProjectRoutingContext<'context, 'provider, 'graph>,
) -> Result<RoutedProjectObjects<'context>, Box<dyn Error>> {
    let workspace = context.workspace;
    let objects = context.objects;
    let reference_graph = context.reference_graph;
    let settings = context.settings;
    let project_version = context.project_version;
    let editor_version_text = context.editor_version_text;
    let settings_facts = context.settings_facts;
    let use_embedded_engine_data = context.engine_asset_source == EngineAssetSource::EmbeddedData;
    let assembly_manager_is_set = context.assembly_manager_is_set;
    let managed_assemblies = context.managed_assemblies;
    let reference_assembly_maps = context.reference_assembly_maps;
    let opaque_objects = context.opaque_objects;
    let scene_member_keys = context.scene_member_keys;
    let prefab_member_keys = context.prefab_member_keys;
    let scriptable_object_groups = context.scriptable_object_groups;
    let controller_collections = context.controller_collections;
    let terrain_collections = context.terrain_collections;
    let lightmap_texture_directories = context.lightmap_texture_directories;
    let scene_lighting_subasset_directories = context.scene_lighting_subasset_directories;
    let bundle_container_original_directories = context.bundle_container_original_directories;
    let mut used_guids = context.existing_guids.clone();
    let sprite_texture_associations =
        crate::export::importer_fields::SpriteTextureAssociationIndex::build(
            workspace,
            objects,
            reference_graph,
        )?;
    let mut next_guid_sequence = 0u64;
    let mut next_collection_index = 0usize;
    let mut routed = Vec::new();
    routed
        .try_reserve_exact(objects.report().object_count)
        .map_err(|_| invalid_project("could not allocate routed project objects"))?;
    let mut notices: Vec<ProjectExportNotice> = Vec::new();
    let mut unresolved_script_content: Vec<ProjectExportNotice> = Vec::new();
    let mut opaque_fallback_notices: Vec<ProjectExportNotice> = Vec::new();
    for (file_index, file) in workspace.files().iter().enumerate() {
        for (object_index, object_info) in file.metadata.objects.iter().enumerate() {
            let source_key = source_object_key(file_index, object_index);
            if scene_member_keys.contains(&source_key)
                || prefab_member_keys.contains(&source_key)
                || controller_collections
                    .member_keys
                    .contains(&(file_index, object_index))
                || terrain_collections
                    .member_keys
                    .contains(&(file_index, object_index))
                || scriptable_object_groups
                    .member_keys
                    .contains(&(file_index, object_index))
            {
                continue;
            }
            let scriptable_group_plan = scriptable_object_groups
                .roots
                .get(&(file_index, object_index));
            let object = objects.get(file_index, object_index).ok_or_else(|| {
                invalid_project(format!(
                    "workspace object is missing at file {file_index}, object {object_index}"
                ))
            })?;
            if object_info.path_id == 0 {
                return Err(invalid_project(format!(
                    "file {file_index} object {object_index} has PathID 0, which cannot identify a Unity YAML document"
                )));
            }
            // Newer SerializedFile generations omit the legacy object-table class ID;
            // the source factory resolves it from the serialized type table.
            let class_id = object.factory.class_id;
            let source_engine_guid = engine_resource_collection_guid(&file.file_name);
            let payload = object.payload.as_ref().ok();
            let text_asset_script = if source_engine_guid.is_none() && class_id == 49 {
                payload.map(text_asset_script_bytes).transpose()?.flatten()
            } else {
                None
            };
            if payload.is_some() && !opaque_objects.contains(&(file_index, object_index)) {
                validate_decoded_object(object, object_info.path_id, reference_graph)?;
            }
            // The mapping, including the Texture3D special case, lives in one place.
            let facts = object_class_facts(&object.factory.kind, class_id, payload.is_some());
            let shader_view = if source_engine_guid.is_none() && class_id == SHADER_CLASS_ID {
                payload
                    .map(|payload| ShaderView::from_value(&payload.root))
                    .transpose()?
            } else {
                None
            };
            let predefined_engine_shader = if use_embedded_engine_data
                && source_engine_guid.is_none()
                && class_id == SHADER_CLASS_ID
            {
                shader_view
                    .as_ref()
                    .map(|view| {
                        predefined_engine_shader_target(
                            view,
                            file.metadata.prefix.unity_version_text.as_deref(),
                        )
                    })
                    .transpose()?
                    .flatten()
            } else {
                None
            };
            let material_view = if source_engine_guid.is_none() && class_id == 21 {
                payload
                    .map(|payload| crate::resources::material::MaterialView::from_value(&payload.root))
                    .transpose()
                    .map_err(|error| {
                        invalid_project(format!(
                            "Material {}:{} could not be read for predefined engine-asset matching: {error}",
                            file_index, object_info.path_id
                        ))
                    })?
            } else {
                None
            };
            let predefined_engine_material = if use_embedded_engine_data {
                if let Some(material) = material_view.as_ref() {
                    let resolved_shader_name =
                        resolved_material_shader_name(material, object, objects, reference_graph)?;
                    match (material.name.as_deref(), resolved_shader_name) {
                        (Some(name), Some(shader_name)) => predefined_engine_material_target(
                            name,
                            shader_name.as_deref(),
                            file.metadata.prefix.unity_version_text.as_deref(),
                        )?,
                        _ => None,
                    }
                } else {
                    None
                }
            } else {
                None
            };
            let predefined_engine_sprite = if use_embedded_engine_data
                && source_engine_guid.is_none()
                && class_id == crate::resources::sprite::SPRITE_CLASS_ID
            {
                if let Some(payload) = payload {
                    let name = predefined_engine_object_name(&payload.root);
                    // The frozen `EngineAssetsExporter` redirects a Sprite only through its
                    // `SpriteInformationObject`: `Cache.Contains(sio.Texture)` decides the whole
                    // texture group first (its table is keyed by name *and* dimensions), and only
                    // then is the sprite's own table entry consulted for the redirected identity.
                    // A sprite whose texture is an ordinary authored texture keeps its ordinary
                    // export even when its own name and texture name happen to match an engine
                    // row; only a null texture pointer falls back to a name-only sprite lookup.
                    // Unreadable render data keeps the ordinary sprite route, so strict sprite
                    // reads still fail loudly in their own path.
                    let texture_pointer =
                        crate::export::importer_fields::sprite_render_data_texture_pointer(
                            &payload.root,
                        );
                    let group_texture_name = match texture_pointer {
                        Ok(None) => Some(None),
                        Ok(Some((field_path, file_id, path_id))) => {
                            resolve_engine_group_texture_name(
                                &field_path,
                                file_id,
                                path_id,
                                object,
                                objects,
                                reference_graph,
                                file.metadata.prefix.unity_version_text.as_deref(),
                            )?
                        }
                        Err(_) => None,
                    };
                    match (name, group_texture_name) {
                        (Some(name), Some(texture_name)) => predefined_engine_sprite_target(
                            &name,
                            texture_name.as_deref(),
                            file.metadata.prefix.unity_version_text.as_deref(),
                        )?,
                        _ => None,
                    }
                } else {
                    None
                }
            } else {
                None
            };
            let predefined_engine_texture = if use_embedded_engine_data
                && source_engine_guid.is_none()
                && class_id == crate::export::res_wiring::TEXTURE2D_CLASS_ID
            {
                if let Some(payload) = payload {
                    let name = predefined_engine_object_name(&payload.root);
                    let dimensions = predefined_engine_texture_dimensions(&payload.root);
                    match (name, dimensions) {
                        (Some(name), Some((width, height))) => predefined_engine_texture_target(
                            &name,
                            width,
                            height,
                            file.metadata.prefix.unity_version_text.as_deref(),
                        )?,
                        _ => None,
                    }
                } else {
                    None
                }
            } else {
                None
            };
            let predefined_engine_mesh = if use_embedded_engine_data
                && source_engine_guid.is_none()
                && class_id == crate::export::res_wiring::MESH_CLASS_ID
            {
                if let Some(payload) = payload {
                    // The frozen cache keys a Mesh by name, vertex count, sub-mesh count
                    // and local bounding box; a mesh that reproduces the builtin identity
                    // is redirected and consumes no export file name, which keeps the
                    // shared-name suffix sequence aligned with the baseline.
                    match predefined_engine_mesh_key(&payload.root) {
                        Some(key) => predefined_engine_mesh_target(
                            &key.name,
                            key.vertex_count,
                            key.sub_mesh_count,
                            key.center,
                            key.extent,
                            file.metadata.prefix.unity_version_text.as_deref(),
                        )?,
                        None => None,
                    }
                } else {
                    None
                }
            } else {
                None
            };
            let predefined_engine_text_asset = if context.engine_asset_source
                == EngineAssetSource::StoredResourceData
                && source_engine_guid.is_none()
                && class_id == 49
            {
                let name = payload.and_then(|payload| predefined_engine_object_name(&payload.root));
                match (name.as_deref(), text_asset_script.as_deref()) {
                    (Some(name), Some(bytes)) => context
                        .engine_resource_data
                        .and_then(|data| data.text_asset_target(name, bytes)),
                    _ => None,
                }
            } else {
                None
            };
            let predefined_engine_target = predefined_engine_shader
                .or(predefined_engine_material)
                .or(predefined_engine_sprite)
                .or(predefined_engine_texture)
                .or(predefined_engine_mesh)
                .or(predefined_engine_text_asset);
            let engine_guid =
                source_engine_guid.or_else(|| predefined_engine_target.map(|target| target.guid));
            let sprite_subasset = if class_id == crate::resources::sprite::SPRITE_CLASS_ID
                && !settings_facts.sprite_export_mode_yaml
                && engine_guid.is_none()
            {
                sprite_texture_associations.subasset_for_sprite(file_index, object_index)
            } else {
                None
            };
            let audio_view = if source_engine_guid.is_none()
                && class_id == crate::resources::audio::AUDIO_CLIP_CLASS_ID
            {
                payload
                    .map(|payload| {
                        crate::resources::audio::AudioClipView::from_value(&payload.root)
                    })
                    .transpose()?
            } else {
                None
            };
            let mut audio_binary_export = None;
            let audio_claim_decode = if let Some(view) = audio_view.as_ref().filter(|_| {
                (project_version.major == 2022 && project_version.minor == 3
                    || project_version.major == 2018 && project_version.minor == 4
                    || project_version.major == 2019 && project_version.minor == 4)
                    && matches!(
                        settings.audio_export_format,
                        crate::export::settings::AudioExportFormat::Default
                            | crate::export::settings::AudioExportFormat::PreferWav
                    )
            }) {
                let data = read_audio_data_for_project_export(
                    context,
                    file_index,
                    object_info.path_id,
                    view,
                )?;
                let name = view
                    .name
                    .as_deref()
                    .map(String::from_utf8_lossy)
                    .unwrap_or_default();
                let audio_output_limit = crate::export::audio_export::MAX_DECODED_AUDIO_BYTES
                    .min(context.limits.max_total_asset_payload_bytes);
                match crate::export::audio_export::try_decode_audio_with_limit(
                    &name,
                    view.sound_type,
                    &data,
                    audio_output_limit,
                ) {
                    Ok((decoded_bytes, extension)) if !decoded_bytes.is_empty() => {
                        let collection_extension = crate::export::audio_export::plan_audio_clip_collection(
                            crate::export::audio_export::AudioDecodeFact {
                                is_audio_clip: true,
                                decode_succeeded: true,
                                decoded_is_empty: false,
                                decoded_extension: extension,
                                format: settings.audio_export_format,
                            },
                        )
                        .map_err(|refusal| {
                            invalid_project(format!(
                                "AudioClip {}:{} decoded successfully but collection planning refused it: {refusal:?}",
                                file_index, object_info.path_id
                            ))
                        })?;
                        let bytes = if crate::export::audio_export::converts_ogg_to_wav(
                            extension,
                            &collection_extension,
                        ) {
                            crate::export::audio_export::convert_ogg_to_wav_with_limit(
                                &decoded_bytes,
                                audio_output_limit,
                            )
                            .map_err(|error| {
                                invalid_project(format!(
                                    "file {file_index} AudioClip PathID {} Ogg-to-WAV conversion failed: {error}",
                                    object_info.path_id
                                ))
                            })?
                        } else {
                            decoded_bytes
                        };
                        let audio_importer = if project_version.major == 2018
                            && project_version.minor == 4
                            || project_version.major == 2019 && project_version.minor == 4
                        {
                            crate::export::project_workflow::audio_asset::audio_importer_yaml_2018_4(
                                view,
                                payload.and_then(object_asset_bundle_name).as_deref(),
                            )?
                        } else {
                            audio_importer_yaml_2022_3(
                                view,
                                payload.and_then(object_asset_bundle_name).as_deref(),
                            )?
                        };
                        audio_binary_export = Some(AudioBinaryExportDetails {
                            bytes,
                            collection_extension,
                            importer: audio_importer,
                        });
                        crate::export::collection_claims::AudioClaimDecode::Decoded {
                            is_ogg: extension == "ogg",
                        }
                    }
                    Ok(_) => crate::export::collection_claims::AudioClaimDecode::Empty,
                    Err(
                        error @ crate::export::audio_export::AudioDecodeError::DecodedDataLimitExceeded { .. }
                        | error @ crate::export::audio_export::AudioDecodeError::OutputAllocationFailed,
                    ) => {
                        return Err(invalid_project(format!(
                            "file {file_index} AudioClip PathID {} could not be safely decoded: {error}",
                            object_info.path_id
                        )));
                    }
                    Err(_) => crate::export::collection_claims::AudioClaimDecode::Undecodable,
                }
            } else {
                crate::export::collection_claims::AudioClaimDecode::Undecodable
            };
            let shader_text = match shader_view.as_ref().and_then(|view| view.script.as_deref()) {
                None => crate::export::collection_claims::ShaderTextState::NoScript,
                Some(script) if script_has_decompiled_text(script) => {
                    crate::export::collection_claims::ShaderTextState::DecompiledSource
                }
                Some(_) => crate::export::collection_claims::ShaderTextState::CompiledOrEmpty,
            };
            let font_data = if source_engine_guid.is_none() && class_id == 128 {
                payload
                    .map(|payload| font_asset_data(&payload.root))
                    .transpose()?
                    .flatten()
            } else {
                None
            };
            let texture_image_exportable = engine_guid.is_none()
                && matches!(class_id, 28 | crate::resources::texture::CUBEMAP_CLASS_ID)
                && matches!(&object.factory.kind, FactoryAssetKind::Generated(_))
                && payload.is_some_and(texture_has_exportable_image_payload);
            let texture3d_image_exportable = engine_guid.is_none()
                && class_id == crate::resources::texture_array::TEXTURE3D_CLASS_ID
                && payload.is_some_and(texture3d_has_exportable_image_payload);
            let texture_array_image_exportable = engine_guid.is_none()
                && matches!(
                    class_id,
                    crate::resources::texture_array::TEXTURE2DARRAY_CLASS_ID
                        | crate::resources::texture_array::CUBEMAPARRAY_CLASS_ID
                )
                && payload.is_some_and(|payload| {
                    texture_array_has_exportable_image_payload(payload, class_id)
                });
            let lightmap_scene_directory = lightmap_texture_directories
                .get(&(file_index, object_index))
                .cloned();
            let yaml_lightmap_route = lightmap_scene_directory.is_some()
                && matches!(
                    settings.lightmap_texture_export_format,
                    crate::export::settings::LightmapTextureExportFormat::Yaml
                );
            let lightmap_image_route = lightmap_scene_directory.is_some() && !yaml_lightmap_route;
            let texture_importer_supported =
                TextureImporterLayout::for_project_version(project_version).is_some();
            let cubemap_importer_supported = class_id
                != crate::resources::texture::CUBEMAP_CLASS_ID
                || matches!(
                    TextureImporterLayout::for_project_version(project_version),
                    Some(TextureImporterLayout::Unity2022_3 | TextureImporterLayout::Unity2018_4)
                );
            let texture_route_supported = texture_importer_supported
                && cubemap_importer_supported
                && ((texture_image_exportable && lightmap_scene_directory.is_none())
                    || lightmap_image_route
                    || (texture3d_image_exportable && lightmap_scene_directory.is_none())
                    || texture_array_project_route_supported(
                        class_id,
                        project_version,
                        texture_array_image_exportable,
                    ));
            let mut claim_facts = claim_facts_for_shader_text(facts, settings_facts, shader_text);
            claim_facts.audio_decode = audio_claim_decode;
            claim_facts.engine_file = engine_guid.is_some();
            claim_facts.data_valid = match class_id {
                49 => text_asset_script
                    .as_ref()
                    .is_some_and(|script| !script.is_empty()),
                128 => font_data.is_some_and(|data| !data.is_empty()),
                crate::resources::texture_array::TEXTURE3D_CLASS_ID => texture3d_image_exportable,
                crate::resources::texture_array::TEXTURE2DARRAY_CLASS_ID
                | crate::resources::texture_array::CUBEMAPARRAY_CLASS_ID => {
                    texture_array_image_exportable
                }
                _ => false,
            };
            if class_id == 128 {
                claim_facts.main = ClaimMainKind::Font;
            } else if lightmap_scene_directory.is_some() {
                // LightingDataProcessor assigns the generated LightingDataAsset as the
                // main asset for every texture referenced by LightmapSettings.Lightmaps.
                claim_facts.main = ClaimMainKind::LightingData;
            } else if sprite_subasset.is_some() {
                // In Native/Texture2D mode, a Sprite whose render data resolves to a Texture2D
                // is a member of that texture's SpriteInformationObject/TextureExportCollection.
                claim_facts.main = ClaimMainKind::SpriteInformation;
            } else if texture_route_supported
                && !matches!(
                    class_id,
                    crate::resources::texture_array::TEXTURE3D_CLASS_ID
                        | crate::resources::texture_array::TEXTURE2DARRAY_CLASS_ID
                        | crate::resources::texture_array::CUBEMAPARRAY_CLASS_ID
                )
            {
                // SpriteProcessor assigns exportable textures a generated main asset, including
                // textures referenced by Sprite objects. Keep image routing in the ordinary
                // registration walk with the processed fact.
                claim_facts.main = ClaimMainKind::SpriteInformation;
            } else if scriptable_group_plan.is_some() {
                claim_facts.main = ClaimMainKind::ScriptableObjectGroup;
            }
            let selection = select_collection_with_claim_facts(facts, settings_facts, claim_facts)?;
            if scriptable_group_plan.is_some()
                && selection.exporter != "ScriptableObjectGroupExporter"
            {
                return Err(invalid_project(format!(
                    "MonoBehaviour {}:{} was identified as a generated ScriptableObjectGroup root, but registration selected {}",
                    file_index, object_info.path_id, selection.exporter
                )));
            }
            if selection.exporter == "LightmapTextureAssetExporter" && !texture_importer_supported {
                return Err(invalid_project(format!(
                    "file {file_index} object {object_index} PathID {} selected LightmapTextureAssetExporter, but Unity {}.{} has no source-verified TextureImporter layout",
                    object.path_id, project_version.major, project_version.minor
                )));
            }
            let audio_yaml_route = if selection.exporter == "YamlAudioExporter" {
                audio_view.as_ref().ok_or_else(|| {
                    invalid_project(
                        "YamlAudioExporter was selected without decoded AudioClip fields",
                    )
                })?;
                if settings.audio_export_format
                    == crate::export::settings::AudioExportFormat::Native
                {
                    return Err(invalid_project(format!(
                        "file {file_index} object {object_index} PathID {} selected YamlAudioExporter under Native, whose binary AudioClip project route is not integrated",
                        object.path_id
                    )));
                }
                // The source registers this YAML collection as the fallback after
                // AudioClipExporter declines undecodable payloads. It preserves inline
                // m_AudioData and stages a resolved m_Resource as a .resS sidecar even
                // when Default or PreferWav is configured.
                true
            } else {
                false
            };
            if class_id == crate::resources::audio::AUDIO_CLIP_CLASS_ID
                && selection.exporter == "NativeAudioExporter"
            {
                return Err(invalid_project(format!(
                    "file {file_index} object {object_index} PathID {} selected NativeAudioExporter, whose binary native AudioClip route is not integrated",
                    object.path_id
                )));
            }
            let audio_binary_route = selection.exporter == "AudioClipExporter";
            if audio_binary_route && audio_binary_export.is_none() {
                return Err(invalid_project(format!(
                    "file {file_index} object {object_index} PathID {} selected AudioClipExporter without retained decoded audio bytes",
                    object.path_id
                )));
            }
            let audio_yaml_resource = if audio_yaml_route {
                let view = audio_view.as_ref().ok_or_else(|| {
                    invalid_project(
                        "YamlAudioExporter was selected without decoded AudioClip fields",
                    )
                })?;
                match view.resource.as_ref() {
                    None => None,
                    Some(resource) if resource.size > 0 && resource.path.is_empty() => {
                        return Err(invalid_project(format!(
                            "file {file_index} object {object_index} PathID {} AudioClip declares {} streamed bytes with an empty resource path",
                            object.path_id, resource.size
                        )));
                    }
                    Some(resource) => Some(resource.clone()),
                }
            } else {
                None
            };
            let text_asset_export = if selection.exporter == "TextAssetExporter" {
                let script = text_asset_script.ok_or_else(|| {
                    invalid_project("TextAssetExporter was selected without decoded m_Script bytes")
                })?;
                let text = String::from_utf8_lossy(&script);
                let extension = text_export_extension(None, settings.text_export_mode, &text);
                Some(TextAssetExportDetails { script, extension })
            } else {
                None
            };
            let texture_export = if texture_route_supported {
                let expected_exporter = match class_id {
                    crate::resources::texture_array::TEXTURE3D_CLASS_ID
                    | crate::resources::texture_array::TEXTURE2DARRAY_CLASS_ID
                    | crate::resources::texture_array::CUBEMAPARRAY_CLASS_ID => {
                        "TextureArrayAssetExporter"
                    }
                    _ if lightmap_image_route => "LightmapTextureAssetExporter",
                    _ => "TextureAssetExporter",
                };
                if selection.exporter != expected_exporter {
                    return Err(invalid_project(format!(
                        "generated texture class {class_id} selected {}, expected the registered {expected_exporter}",
                        selection.exporter,
                    )));
                }
                let payload = payload.ok_or_else(|| {
                    invalid_project(
                        "texture image exporter was selected without decoded texture fields",
                    )
                })?;
                let asset_bundle_name = object_asset_bundle_name(payload);
                let source_unity_version = file
                    .metadata
                    .prefix
                    .unity_version_text
                    .as_deref()
                    .and_then(crate::resources::vertex_data_blob::UnityVersion::parse);
                let plan = if class_id == crate::resources::texture_array::TEXTURE3D_CLASS_ID {
                    plan_texture3d_project_export_for_source_version(
                        &payload.root,
                        project_version,
                        source_unity_version,
                        settings,
                        asset_bundle_name.as_deref(),
                    )
                } else if class_id == crate::resources::texture_array::TEXTURE2DARRAY_CLASS_ID {
                    plan_texture2d_array_project_export_for_source_version(
                        &payload.root,
                        project_version,
                        source_unity_version,
                        settings,
                        asset_bundle_name.as_deref(),
                    )
                } else if class_id == crate::resources::texture_array::CUBEMAPARRAY_CLASS_ID {
                    plan_cubemap_array_project_export_for_source_version(
                        &payload.root,
                        project_version,
                        source_unity_version,
                        settings,
                        asset_bundle_name.as_deref(),
                    )
                } else if class_id == crate::resources::texture::CUBEMAP_CLASS_ID {
                    plan_cubemap_project_export_for_source_version(
                        &payload.root,
                        project_version,
                        source_unity_version,
                        settings,
                        asset_bundle_name.as_deref(),
                    )
                } else if lightmap_image_route {
                    plan_lightmap_texture_project_export_for_source_version(
                        &payload.root,
                        project_version,
                        source_unity_version,
                        settings,
                        asset_bundle_name.as_deref(),
                    )
                } else {
                    let mut plan = plan_texture_project_export_for_source_version(
                        &payload.root,
                        project_version,
                        source_unity_version,
                        settings,
                        asset_bundle_name.as_deref(),
                    )?;
                    // Mirror `TextureExportCollection.AddSprites` for plain Texture2D
                    // collections: sprites bound to this texture through their render-data
                    // texture pointer overwrite the default sprite importer fields.
                    // Cubemap, texture-array, Texture3D, and lightmap routes keep their existing fields.
                    let sprite_facts =
                        crate::export::importer_fields::collect_sprite_facts_for_texture_with_associations(
                            workspace,
                            objects,
                            reference_graph,
                            &sprite_texture_associations,
                            file_index,
                            object_index,
                            &object.factory.name,
                            !settings_facts.sprite_export_mode_yaml,
                            |sprite_file_index, sprite_path_id, render_data, version, endian| {
                                read_sprite_vertex_stream_for_project_export(
                                    context,
                                    sprite_file_index,
                                    sprite_path_id,
                                    render_data,
                                    version,
                                    endian,
                                )
                            },
                        )?;
                    if !sprite_facts.is_empty() {
                        let decision = crate::export::importer_fields::decide_sprite_importer(
                            &sprite_facts,
                            !settings_facts.sprite_export_mode_yaml,
                            crate::export::importer_fields::importer_node_has_field(
                                &plan.importer,
                                "spritePivot",
                            ),
                            crate::export::importer_fields::importer_node_has_field(
                                &plan.importer,
                                "spriteBorder",
                            ),
                        );
                        crate::export::importer_fields::apply_sprite_importer_decision(
                            &mut plan.importer,
                            &decision,
                        )?;
                        crate::export::importer_fields::apply_sprite_sheet_metadata_to_importer(
                            &mut plan.importer,
                            &sprite_facts,
                            &decision,
                        )?;
                    }
                    Ok(plan)
                };
                Some(plan.map_err(|error| {
                    invalid_project(format!(
                        "file {file_index} object {object_index} PathID {} Texture2D project planning failed: {error}",
                        object.path_id
                    ))
                })?)
            } else {
                None
            };
            let kind = selection.kind();
            let script_export = if class_id == 115 && source_engine_guid.is_none() {
                plan_script_export_details(
                    object,
                    settings,
                    assembly_manager_is_set,
                    managed_assemblies,
                    editor_version_text,
                    reference_assembly_maps,
                )?
            } else {
                None
            };
            if let Some(script) = script_export.as_ref().filter(|script| {
                script.missing_assembly || script.source_script_present == Some(false)
            }) {
                let reason = match (script.missing_assembly, script.source_script_present) {
                    (true, Some(false)) => {
                        "the assembly and script type are absent; its source decompile pointer is retained and no .cs file is emitted"
                    }
                    (false, Some(false)) => {
                        "the imported manager has no matching script type; its source pointer is retained and no .cs file is emitted"
                    }
                    (true, _) => {
                        "the assembly is absent from the imported catalog; its source GUID/fileID pointer is retained and the missing script DLL is not fabricated"
                    }
                    (false, _) => {
                        "the source script content is unavailable; its pointer is retained and no .cs file is emitted"
                    }
                };
                unresolved_script_content.push(ProjectExportNotice {
                    subject: format!(
                        "MonoScript assembly {:?} ({}.{}, ScriptExportMode::{:?})",
                        script.identity.assembly,
                        script.identity.namespace,
                        script.identity.class,
                        settings.script_export_mode
                    ),
                    reason,
                });
            }
            let is_opaque_fallback = opaque_objects.contains(&(file_index, object_index));
            let raw_fallback_kind_matches = match kind {
                ExportCollectionKind::Unknown => {
                    matches!(&object.factory.kind, FactoryAssetKind::Unknown)
                }
                ExportCollectionKind::Unreadable => {
                    matches!(&object.factory.kind, FactoryAssetKind::Unreadable)
                }
                _ => true,
            };
            if matches!(
                kind,
                ExportCollectionKind::Unknown | ExportCollectionKind::Unreadable
            ) && (!is_opaque_fallback || !raw_fallback_kind_matches)
            {
                return Err(invalid_project(format!(
                    "{} selected a raw fallback exporter, but the source factory did not retain an opaque UnknownObject or UnreadableObject payload",
                    selection.collection_name
                )));
            }
            // Predict a skipped generic-asset Texture2D file for pointer
            // typing: a streamed texture without usable image data is
            // skipped at materialization, while the frozen texture
            // collection still reports `Meta`. Native lightmap textures
            // carry inline bytes, and raw-housed textures are written
            // Predict a skipped generic-asset Texture2D file for pointer
            // typing: a streamed texture without usable image data is
            // skipped at materialization, while the frozen texture
            // collection still reports `Meta`. Written textures (inline
            // bytes or a viable stream) and other housings stay `Serialized`.
            let texture_file_skipped = class_id == 28
                && selection.collection_name == "YamlStreamedAssetExportCollection"
                && texture_lacks_usable_image_data(payload);
            let (mut route, skip_reason, fallback_reason) = if engine_guid.is_some() {
                if selection.exporter != "EngineAssetsExporter"
                    || kind != ExportCollectionKind::Redirect
                {
                    return Err(invalid_project(format!(
                        "engine resource collection {} selected {}, expected EngineAssetsExporter redirect",
                        file.file_name, selection.exporter
                    )));
                }
                (ObjectRoute::EngineRedirect, "", None)
            } else {
                match kind {
                    ExportCollectionKind::Manager => (ObjectRoute::Manager, "", None),
                    ExportCollectionKind::Unknown => (ObjectRoute::Unknown, "", None),
                    ExportCollectionKind::Unreadable => (ObjectRoute::Unreadable, "", None),
                    ExportCollectionKind::Asset if selection.exporter == "TextAssetExporter" => {
                        (ObjectRoute::TextAsset, "", None)
                    }
                    ExportCollectionKind::Asset if selection.exporter == "FontAssetExporter" => {
                        if class_id != 128 || font_data.is_none_or(|data| data.is_empty()) {
                            return Err(invalid_project(format!(
                                "FontAssetExporter selected class {class_id} at {file_index}:{} without non-empty Font.m_FontData",
                                object_info.path_id
                            )));
                        }
                        (ObjectRoute::FontBinary, "", None)
                    }
                    ExportCollectionKind::Asset if audio_binary_route => {
                        (ObjectRoute::AudioBinary, "", None)
                    }
                    ExportCollectionKind::Asset if audio_yaml_route => {
                        (ObjectRoute::AudioYaml, "", None)
                    }
                    ExportCollectionKind::Asset if selection.exporter == "YamlShaderExporter" => {
                        (ObjectRoute::ShaderYaml, "", None)
                    }
                    ExportCollectionKind::Asset
                        if selection.exporter == "DummyShaderTextExporter"
                            || selection.exporter == "SimpleShaderExporter" =>
                    {
                        (ObjectRoute::ShaderText, "", None)
                    }
                    ExportCollectionKind::Asset if texture_export.is_some() => {
                        (ObjectRoute::TextureImage, "", None)
                    }
                    ExportCollectionKind::Asset => (
                        ObjectRoute::Asset,
                        "",
                        asset_family_fallback(selection.collection_name),
                    ),
                    ExportCollectionKind::Script | ExportCollectionKind::EmptyScript
                        if let Some(script) = script_export.as_ref() =>
                    {
                        (
                            if script.export_type == AssemblyExportType::Save
                                || (script.export_type == AssemblyExportType::Skip
                                    && assembly_manager_is_set)
                                || (script.export_type == AssemblyExportType::Decompile
                                    && assembly_manager_is_set
                                    && script.source_script_present == Some(false))
                            {
                                ObjectRoute::ScriptPointerOnly
                            } else {
                                ObjectRoute::Script
                            },
                            "",
                            None,
                        )
                    }
                    ExportCollectionKind::Script | ExportCollectionKind::EmptyScript
                        if class_id == 115 =>
                    {
                        (
                            ObjectRoute::Skipped,
                            "injected MonoScripts have no standalone script file in the source exporter",
                            None,
                        )
                    }
                    ExportCollectionKind::Scene | ExportCollectionKind::Prefab => (
                        ObjectRoute::Asset,
                        "",
                        Some(
                            "the specialized collection renderer is not ported; the object was written \
                         through the generic asset family",
                        ),
                    ),
                    ExportCollectionKind::Script | ExportCollectionKind::EmptyScript => (
                        ObjectRoute::Skipped,
                        "a script export collection was selected without a MonoScript object",
                        None,
                    ),
                    ExportCollectionKind::Fail => (
                        ObjectRoute::Skipped,
                        "FailExportCollection writes nothing and reports a failed export",
                        None,
                    ),
                    ExportCollectionKind::Empty => (
                        ObjectRoute::Skipped,
                        "EmptyExportCollection is not exportable; the baseline writes nothing",
                        None,
                    ),
                    ExportCollectionKind::Skip => (
                        ObjectRoute::Skipped,
                        "SkipExportCollection is not exportable; references become missing references",
                        None,
                    ),
                    ExportCollectionKind::Deleted => (
                        ObjectRoute::Skipped,
                        "DeletedAssetsExportCollection is not exportable",
                        None,
                    ),
                    ExportCollectionKind::Redirect => (
                        ObjectRoute::Skipped,
                        "RedirectExportCollection is not exportable",
                        None,
                    ),
                }
            };
            if sprite_subasset.is_some() {
                if selection.exporter != "TextureAssetExporter"
                    || kind != ExportCollectionKind::Asset
                {
                    return Err(invalid_project(format!(
                        "Sprite {}:{} is associated with a Texture2D in non-YAML mode, but registration selected {}",
                        file_index, object_info.path_id, selection.exporter
                    )));
                }
                route = ObjectRoute::TextureSpriteSubAsset;
            }
            let fallback_reason = if should_report_unported_texture_route(
                texture_image_exportable
                    || texture3d_image_exportable
                    || (texture_array_image_exportable
                        && source_has_texture_array_exporter(project_version)),
                texture_route_supported,
                yaml_lightmap_route,
            ) {
                Some(
                    "the source TextureExportCollection image/importer route is not yet ported for this Unity importer version or Sprite group",
                )
            } else {
                fallback_reason
            };
            let label = if route == ObjectRoute::Skipped {
                format!("{} (class {class_id})", selection.collection_name)
            } else {
                selection.collection_name.to_owned()
            };
            if route == ObjectRoute::Skipped {
                notices.push(ProjectExportNotice {
                    subject: label.clone(),
                    reason: skip_reason,
                });
            }
            if matches!(route, ObjectRoute::Unknown | ObjectRoute::Unreadable) {
                opaque_fallback_notices.push(ProjectExportNotice {
                    subject: label,
                    reason: "raw bytes are preserved, but this object has no decoded fields or reference scan",
                });
            }
            let collection_index = if matches!(
                route,
                ObjectRoute::Skipped | ObjectRoute::TextureSpriteSubAsset
            ) {
                usize::MAX
            } else {
                let index = next_collection_index;
                next_collection_index = next_collection_index
                    .checked_add(1)
                    .ok_or_else(|| invalid_project("project collection count overflows"))?;
                index
            };
            let (class_name, named_name) = if opaque_objects.contains(&(file_index, object_index)) {
                (
                    object.factory.class_name.to_string(),
                    Some(object.factory.name.clone()),
                )
            } else {
                (
                    object_class_name(class_id, payload.map(|payload| payload.type_name.as_str())),
                    project_object_name(class_id, payload, shader_view.as_ref()),
                )
            };
            let file_id = match route {
                ObjectRoute::Asset
                | ObjectRoute::AudioYaml
                | ObjectRoute::AudioBinary
                | ObjectRoute::FontBinary
                | ObjectRoute::TextureImage
                | ObjectRoute::TextAsset
                | ObjectRoute::ShaderText
                | ObjectRoute::ShaderYaml => crate::export::ids::get_main_export_id(class_id, 0)
                    .map_err(|error| {
                        invalid_project(format!(
                            "class {class_id} export file ID could not be planned: {error}"
                        ))
                    })?,
                ObjectRoute::Script | ObjectRoute::ScriptPointerOnly => {
                    script_export
                        .as_ref()
                        .ok_or_else(|| invalid_project("MonoScript route has no export details"))?
                        .pointer_file_id
                }
                ObjectRoute::TextureSpriteSubAsset => {
                    sprite_subasset
                        .ok_or_else(|| {
                            invalid_project("Sprite subasset route has no associated Texture2D")
                        })?
                        .1
                }
                ObjectRoute::EngineRedirect => {
                    predefined_engine_target.map_or(object_info.path_id, |target| target.file_id)
                }
                _ => object_info.path_id,
            };
            let scriptable_object_group = scriptable_group_plan
                .map(|plan| {
                    let mut children = Vec::new();
                    children
                        .try_reserve_exact(plan.children.len())
                        .map_err(|_| {
                            invalid_project(format!(
                                "could not allocate ScriptableObjectGroup children for {}:{}",
                                file_index, object_info.path_id
                            ))
                        })?;
                    for member in &plan.children {
                        let child = objects
                            .get(member.file_index, member.object_index)
                            .ok_or_else(|| {
                                invalid_project(format!(
                                    "ScriptableObjectGroup {}:{} member {}:{} is absent from ObjectWorkspace",
                                    file_index,
                                    object_info.path_id,
                                    member.file_index,
                                    member.object_index
                                ))
                            })?;
                        if child.factory.class_id != 114 {
                            return Err(invalid_project(format!(
                                "ScriptableObjectGroup {}:{} member {}:{} has class ID {}, expected MonoBehaviour (114)",
                                file_index,
                                object_info.path_id,
                                member.file_index,
                                child.path_id,
                                child.factory.class_id
                            )));
                        }
                        children.push(RoutedScriptableObjectGroupChild {
                            object: child,
                            file_id: member.file_id,
                        });
                    }
                    Ok::<_, Box<dyn Error>>(RoutedScriptableObjectGroup {
                        extension: plan.extension,
                        children,
                    })
                })
                .transpose()?;
            let controller_collection = controller_collections
                .roots
                .get(&(file_index, object_index))
                .map(|plan| {
                    let mut children = Vec::new();
                    children
                        .try_reserve_exact(plan.children.len())
                        .map_err(|_| {
                            invalid_project("could not allocate routed AnimatorController members")
                        })?;
                    for member in &plan.children {
                        let object = objects.get(member.key.0, member.key.1).ok_or_else(|| {
                            invalid_project(
                                "routed AnimatorController member is absent from ObjectWorkspace",
                            )
                        })?;
                        children.push(RoutedControllerMember {
                            object,
                            file_id: member.file_id,
                            register_global: member.register_global,
                        });
                    }
                    Ok::<_, Box<dyn Error>>(RoutedControllerCollection {
                        is_release: plan.is_release,
                        children,
                    })
                })
                .transpose()?;
            let terrain_collection = terrain_collections
                .roots
                .get(&(file_index, object_index))
                .map(|plan| {
                    let mut children = Vec::new();
                    children
                        .try_reserve_exact(plan.children.len())
                        .map_err(|_| {
                            invalid_project(
                                "could not allocate routed terrain splat-alpha members",
                            )
                        })?;
                    for member in &plan.children {
                        let object = objects.get(member.key.0, member.key.1).ok_or_else(|| {
                            invalid_project(
                                "routed TerrainData splat-alpha member is absent from ObjectWorkspace",
                            )
                        })?;
                        children.push(RoutedTerrainMember {
                            object,
                            file_id: member.file_id,
                        });
                    }
                    Ok::<_, Box<dyn Error>>(RoutedTerrainCollection { children })
                })
                .transpose()?;
            let guid = if route == ObjectRoute::TextureSpriteSubAsset {
                [0; 16]
            } else if let Some(engine_guid) = engine_guid {
                used_guids.insert(engine_guid);
                engine_guid
            } else if let Some(script) = script_export.as_ref() {
                script.guid
            } else {
                mint_unique_collection_guid(&mut next_guid_sequence, &mut used_guids)?
            };
            let pointer_guid = script_export
                .as_ref()
                .map_or(guid, |script| script.pointer_guid);
            routed.push(RoutedObject {
                object,
                class_id,
                class_name: class_name.clone(),
                named_name,
                // Bundle-container `OriginalDirectory` assignments run before the
                // lighting-subasset rule, matching the source processor order
                // (`OriginalPathProcessor` yields before `LightingDataProcessor`,
                // whose `??=` only fills unset directories). Lightmap textures never
                // share collection membership with container-mapped classes, so the
                // texture-only flags stay untouched.
                original_directory: bundle_container_original_directories
                    .get(&(file_index, object_index))
                    .cloned()
                    .or_else(|| {
                        lightmap_scene_directory.or_else(|| {
                            scene_lighting_subasset_directories
                                .get(&(file_index, object_index))
                                .cloned()
                        })
                    }),
                facts,
                shader_view,
                audio_yaml_resource,
                audio_binary_export,
                text_asset_export,
                font_collection: None,
                texture_export,
                script_export,
                scriptable_object_group,
                controller_collection,
                terrain_collection,
                selection,
                route,
                texture_file_skipped,
                fallback_reason,
                collection_index,
                guid,
                pointer_guid,
                file_id,
                texture_subasset_target: sprite_subasset.map(|(texture, _)| texture),
            });
        }
    }

    attach_texture_sprite_subasset_pointers(&mut routed)?;
    attach_font_collection_subassets(&mut routed, objects, reference_graph)?;
    let fallback_notices = routed
        .iter()
        .filter_map(|entry| {
            entry
                .fallback_reason
                .map(|reason| (entry.selection.collection_name, reason))
        })
        .map(|(collection_name, reason)| ProjectExportNotice {
            subject: collection_name.to_owned(),
            reason,
        })
        .collect();

    Ok(RoutedProjectObjects {
        routed,
        notices,
        unresolved_script_content,
        fallback_notices,
        opaque_fallback_notices,
        next_collection_index,
        next_guid_sequence,
        used_guids,
    })
}

fn attach_texture_sprite_subasset_pointers(
    routed: &mut [RoutedObject<'_>],
) -> Result<(), Box<dyn Error>> {
    let texture_count = routed
        .iter()
        .filter(|entry| entry.route == ObjectRoute::TextureImage)
        .count();
    let mut texture_targets = HashMap::new();
    texture_targets
        .try_reserve(texture_count)
        .map_err(|_| invalid_project("could not allocate Texture2D Sprite subasset targets"))?;
    for entry in routed
        .iter()
        .filter(|entry| entry.route == ObjectRoute::TextureImage)
    {
        let identity = (entry.object.file_index, entry.object.object_index);
        if texture_targets
            .insert(
                identity,
                (entry.collection_index, entry.guid, entry.pointer_guid),
            )
            .is_some()
        {
            return Err(invalid_project(format!(
                "Texture2D object {}:{} has multiple project image collections",
                identity.0, entry.object.path_id
            )));
        }
    }
    for entry in routed
        .iter_mut()
        .filter(|entry| entry.route == ObjectRoute::TextureSpriteSubAsset)
    {
        let texture = entry.texture_subasset_target.ok_or_else(|| {
            invalid_project("Texture Sprite subasset has no owning Texture2D identity")
        })?;
        let (collection_index, guid, pointer_guid) =
            texture_targets.get(&texture).copied().ok_or_else(|| {
                invalid_project(format!(
                    "Sprite object {}:{} belongs to Texture2D {}:{}, which has no routed image collection",
                    entry.object.file_index,
                    entry.object.path_id,
                    texture.0,
                    texture.1
                ))
            })?;
        entry.collection_index = collection_index;
        entry.guid = guid;
        entry.pointer_guid = pointer_guid;
    }
    Ok(())
}

fn attach_font_collection_subassets<'object>(
    routed: &mut [RoutedObject<'object>],
    objects: &'object ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
) -> Result<(), Box<dyn Error>> {
    let mut prepared = Vec::new();
    let mut owners = HashMap::<(usize, usize), usize>::new();
    for (root_index, root) in routed.iter().enumerate() {
        if root.route != ObjectRoute::FontBinary {
            continue;
        }
        let mut children = Vec::new();
        for (field, class_id, expected_name) in [
            ("m_DefaultMaterial", 21, "Font Material"),
            ("m_Texture", 28, "Font Texture"),
        ] {
            let Some(child) = font_collection_subasset(
                root,
                field,
                class_id,
                expected_name,
                objects,
                reference_graph,
            )?
            else {
                continue;
            };
            let key = (child.object.file_index, child.object.object_index);
            if let Some(previous_root) = owners.insert(key, root_index) {
                return Err(invalid_project(format!(
                    "Font subasset {}:{} is claimed by Font collections at routed indexes {previous_root} and {root_index}",
                    key.0, key.1
                )));
            }
            children.push(child);
        }
        prepared.push((root_index, RoutedFontCollection { children }));
    }

    for (root_index, font_collection) in prepared {
        routed[root_index].font_collection = Some(font_collection);
    }
    for (entry_index, entry) in routed.iter_mut().enumerate() {
        let key = (entry.object.file_index, entry.object.object_index);
        if let Some(root_index) = owners.get(&key).copied() {
            if root_index == entry_index {
                return Err(invalid_project(format!(
                    "Font collection at routed index {root_index} claims itself as a generated subasset"
                )));
            }
            entry.route = ObjectRoute::Skipped;
            entry.collection_index = usize::MAX;
            entry.fallback_reason = None;
        }
    }
    Ok(())
}

fn font_collection_subasset<'object>(
    root: &RoutedObject<'object>,
    field_name: &str,
    expected_class_id: i32,
    expected_name: &str,
    objects: &'object ObjectWorkspace,
    reference_graph: &MultiFileReferenceGraph<'_>,
) -> Result<Option<RoutedFontCollectionChild<'object>>, Box<dyn Error>> {
    let payload = root.object.payload.as_ref().map_err(|error| {
        invalid_project(format!(
            "Font {}:{} has no decoded payload while planning generated subassets: {error}",
            root.object.file_index, root.object.path_id
        ))
    })?;
    let value = payload
        .root
        .unique_field_in_base_chain(field_name)
        .map_err(|error| invalid_project(format!("Font {field_name} lookup failed: {error}")))?;
    let Some(value) = value else {
        return Ok(None);
    };
    let SerializedValue::PPtr { file_id, path_id } = value else {
        return Err(invalid_project(format!(
            "Font {}:{} field {field_name} is not a PPtr",
            root.object.file_index, root.object.path_id
        )));
    };
    if *path_id == 0 {
        return Ok(None);
    }
    let mut edges = reference_graph
        .references_from_object(root.object.file_index, root.object.object_index)
        .filter(|edge| {
            edge.field_path == field_name && edge.file_id == *file_id && edge.path_id == *path_id
        });
    let edge = edges.next().ok_or_else(|| {
        invalid_project(format!(
            "Font {}:{} has non-null {field_name} PPtr ({file_id}, {path_id}) without a matching reference-graph edge",
            root.object.file_index, root.object.path_id
        ))
    })?;
    if edges.next().is_some() {
        return Err(invalid_project(format!(
            "Font {}:{} has duplicate reference-graph edges for {field_name} PPtr ({file_id}, {path_id})",
            root.object.file_index, root.object.path_id
        )));
    }
    let (target_file_index, target_object_index) = match &edge.resolution {
        MultiFilePPtrResolution::Object {
            target_file_index,
            target_object_index,
        } => (*target_file_index, *target_object_index),
        MultiFilePPtrResolution::PredefinedEngineAsset { .. } => return Ok(None),
        MultiFilePPtrResolution::Null => {
            return Err(invalid_project(format!(
                "Font {}:{} non-null {field_name} PPtr was classified as null",
                root.object.file_index, root.object.path_id
            )));
        }
        unresolved => {
            return Err(invalid_project(format!(
                "Font {}:{} {field_name} PPtr ({file_id}, {path_id}) is unresolved: {unresolved:?}",
                root.object.file_index, root.object.path_id
            )));
        }
    };
    let target = objects
        .get(target_file_index, target_object_index)
        .ok_or_else(|| {
            invalid_project(format!(
                "Font {}:{} {field_name} resolves to absent workspace object {target_file_index}:{target_object_index}",
                root.object.file_index, root.object.path_id
            ))
        })?;
    if target.factory.class_id != expected_class_id {
        return Err(invalid_project(format!(
            "Font {}:{} {field_name} resolves to class {}, expected {expected_class_id}",
            root.object.file_index, root.object.path_id, target.factory.class_id
        )));
    }
    if target.factory.name != expected_name {
        return Ok(None);
    }
    let file_id =
        crate::export::ids::get_main_export_id(expected_class_id, 0).map_err(|error| {
            invalid_project(format!(
                "Font generated {expected_name:?} file ID could not be planned: {error}"
            ))
        })?;
    Ok(Some(RoutedFontCollectionChild {
        object: target,
        file_id,
    }))
}

#[cfg(test)]
mod tests {
    use super::{
        predefined_engine_material_rows, predefined_engine_material_target,
        predefined_engine_mesh_rows, predefined_engine_mesh_target,
        predefined_engine_shader_target, predefined_engine_sprite_rows,
        predefined_engine_sprite_target, predefined_engine_texture_rows,
        predefined_engine_texture_target, project_object_name, select_engine_assets_band,
        should_report_unported_texture_route, texture_array_project_route_supported,
    };
    use crate::export::project_workflow::SHADER_CLASS_ID;
    use crate::processing::scene_naming::SceneUnityVersion;
    use crate::resources::shader::{ParsedShaderForm, ParsedShaderProperty, ShaderView};

    #[test]
    fn default_yaml_lightmaps_do_not_report_an_unported_image_route() {
        assert!(!should_report_unported_texture_route(true, false, true));
        assert!(should_report_unported_texture_route(true, false, false));
        assert!(!should_report_unported_texture_route(true, true, false));
        assert!(!should_report_unported_texture_route(false, false, false));
    }

    #[test]
    fn texture_array_project_routes_require_the_registered_and_pinned_target_version() {
        let supported = SceneUnityVersion::parse("2022.3.22f1").unwrap();
        let registration_boundary = SceneUnityVersion::parse("2020.2.0f1").unwrap();
        let older = SceneUnityVersion::parse("2019.4.40f1").unwrap();
        for class_id in [
            crate::resources::texture_array::TEXTURE2DARRAY_CLASS_ID,
            crate::resources::texture_array::CUBEMAPARRAY_CLASS_ID,
        ] {
            assert!(texture_array_project_route_supported(
                class_id, supported, true
            ));
            assert!(!texture_array_project_route_supported(
                class_id, supported, false
            ));
            assert!(!texture_array_project_route_supported(
                class_id,
                registration_boundary,
                true
            ));
            assert!(!texture_array_project_route_supported(
                class_id, older, true
            ));
        }
        assert!(!texture_array_project_route_supported(117, supported, true));
    }

    #[test]
    fn engine_assets_band_selection_matches_the_source_blob_order() {
        assert_eq!(select_engine_assets_band(None), None);
        assert_eq!(select_engine_assets_band(Some("not a version")), None);
        // First band key wins while the requested version does not exceed it.
        assert_eq!(select_engine_assets_band(Some("2.0.0f1")), Some("3.2.0f1"));
        assert_eq!(select_engine_assets_band(Some("3.2.0f1")), Some("3.2.0f1"));
        // Otherwise the last band key at or below the requested version wins.
        assert_eq!(
            select_engine_assets_band(Some("2022.3.6f1")),
            Some("2022.2.15f1")
        );
        assert_eq!(
            select_engine_assets_band(Some("2022.3.10f1")),
            Some("2022.2.15f1")
        );
        assert_eq!(
            select_engine_assets_band(Some("2022.3.11f1")),
            Some("2022.2.15f1")
        );
        assert_eq!(
            select_engine_assets_band(Some("2022.3.22f1")),
            Some("2022.3.11f1")
        );
        assert_eq!(
            select_engine_assets_band(Some("2021.3.0f1")),
            Some("2021.2.0b14")
        );
        // Past the final band key, the final band stays selected.
        assert_eq!(
            select_engine_assets_band(Some("6100.0.0f1")),
            Some("6000.5.0a3")
        );
    }

    #[test]
    fn predefined_material_band_rows_stay_byte_identical() {
        // Probe-verified fact (isolated engine-assets TPK dump): the two embedded 2022.3
        // bands carry identical Material rows. If a future probe refresh diverges, the
        // band mapping above must gain independent tables instead of sharing.
        fn data_rows(table: &str) -> Vec<&str> {
            table
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .collect()
        }
        assert_eq!(
            data_rows(predefined_engine_material_rows("2022.2.15f1").unwrap()),
            data_rows(predefined_engine_material_rows("2022.3.11f1").unwrap())
        );
    }

    #[test]
    fn unity_2022_3_predefined_shader_matches_name_and_ordered_properties() {
        let shader = ShaderView {
            parsed: Some(ParsedShaderForm {
                name: "Unlit/Color".to_owned(),
                properties: vec![ParsedShaderProperty {
                    name: "_Color".to_owned(),
                    ..ParsedShaderProperty::default()
                }],
                ..ParsedShaderForm::default()
            }),
            ..ShaderView::default()
        };

        let target = predefined_engine_shader_target(&shader, Some("2022.3.6f1"))
            .unwrap()
            .expect("the 2022.2.15f1 engine-assets band contains Unlit/Color");
        let mut expected_guid = [0; 16];
        for (index, word) in crate::export::guid_policy::ENGINE_BUILTIN_EXTRA_GUID_WORDS
            .iter()
            .enumerate()
        {
            expected_guid[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        assert_eq!(target.file_id, 10755);
        assert_eq!(target.guid, expected_guid);

        let mut different_properties = shader.clone();
        different_properties.parsed.as_mut().unwrap().properties[0].name = "_MainTex".to_owned();
        assert_eq!(
            predefined_engine_shader_target(&different_properties, Some("2022.3.6f1")).unwrap(),
            None
        );
        // Versions selecting an unprobed band resolve nothing instead of borrowing a
        // newer band's rows.
        assert_eq!(
            predefined_engine_shader_target(&shader, Some("2019.4.40f1")).unwrap(),
            None,
            "2019.4 selects an unprobed engine-assets band"
        );
        assert_eq!(
            predefined_engine_shader_target(&shader, None).unwrap(),
            None,
            "a missing version selects no engine-assets band"
        );
    }

    #[test]
    fn unity_2022_3_predefined_material_matches_name_and_resolved_shader() {
        let target = predefined_engine_material_target(
            "Default-Material",
            Some("Standard"),
            Some("2022.3.22f1"),
        )
        .unwrap()
        .expect("2022.3 TPK maps Default-Material to builtin extra");
        let mut expected_guid = [0; 16];
        for (index, word) in crate::export::guid_policy::ENGINE_BUILTIN_EXTRA_GUID_WORDS
            .iter()
            .enumerate()
        {
            expected_guid[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        assert_eq!(target.file_id, 10303);
        assert_eq!(target.guid, expected_guid);
        // Unity 2022.3.10f1 selects the 2022.2.15f1 band, which carries the same
        // Default-Material row: the old patch-11 gate was the bug.
        let earlier_patch = predefined_engine_material_target(
            "Default-Material",
            Some("Standard"),
            Some("2022.3.10f1"),
        )
        .unwrap()
        .expect("2022.3.10f1 selects the 2022.2.15f1 engine-assets band");
        assert_eq!(earlier_patch.file_id, 10303);
        assert_eq!(earlier_patch.guid, expected_guid);
        assert_eq!(
            predefined_engine_material_target(
                "Default-Material",
                Some("Unlit/Color"),
                Some("2022.3.22f1")
            )
            .unwrap(),
            None,
            "the resolved Shader name is part of the baseline cache identity"
        );
        assert_eq!(
            predefined_engine_material_target(
                "Default-Material",
                Some("Standard"),
                Some("2021.3.0f1")
            )
            .unwrap(),
            None,
            "a version selecting an unprobed band resolves nothing"
        );
    }

    #[test]
    fn unity_2022_3_predefined_standard_shader_matches_all_ordered_properties() {
        let property_names = [
            "_Color",
            "_MainTex",
            "_Cutoff",
            "_Glossiness",
            "_GlossMapScale",
            "_SmoothnessTextureChannel",
            "_Metallic",
            "_MetallicGlossMap",
            "_SpecularHighlights",
            "_GlossyReflections",
            "_BumpScale",
            "_BumpMap",
            "_Parallax",
            "_ParallaxMap",
            "_OcclusionStrength",
            "_OcclusionMap",
            "_EmissionColor",
            "_EmissionMap",
            "_DetailMask",
            "_DetailAlbedoMap",
            "_DetailNormalMapScale",
            "_DetailNormalMap",
            "_UVSec",
            "_Mode",
            "_SrcBlend",
            "_DstBlend",
            "_ZWrite",
        ];
        let shader = ShaderView {
            parsed: Some(ParsedShaderForm {
                name: "Standard".to_owned(),
                properties: property_names
                    .iter()
                    .map(|name| ParsedShaderProperty {
                        name: (*name).to_owned(),
                        ..ParsedShaderProperty::default()
                    })
                    .collect(),
                ..ParsedShaderForm::default()
            }),
            ..ShaderView::default()
        };

        let target = predefined_engine_shader_target(&shader, Some("2022.3.22f1"))
            .unwrap()
            .expect("the 2022.3.11f1 engine-assets band contains Standard");
        assert_eq!(target.file_id, 46);
        let mut expected_guid = [0; 16];
        for (index, word) in crate::export::guid_policy::ENGINE_BUILTIN_EXTRA_GUID_WORDS
            .iter()
            .enumerate()
        {
            expected_guid[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        assert_eq!(target.guid, expected_guid);

        let mut reordered = shader;
        reordered.parsed.as_mut().unwrap().properties.swap(0, 1);
        assert_eq!(
            predefined_engine_shader_target(&reordered, Some("2022.3.22f1")).unwrap(),
            None,
            "property ordering is part of the predefined shader identity"
        );
    }

    #[test]
    fn shader_export_name_comes_from_parsed_form() {
        let shader = ShaderView {
            parsed: Some(ParsedShaderForm {
                name: "Hidden/lilToonOutline".to_owned(),
                ..ParsedShaderForm::default()
            }),
            ..ShaderView::default()
        };

        assert_eq!(
            project_object_name(SHADER_CLASS_ID, None, Some(&shader)).as_deref(),
            Some("Hidden/lilToonOutline")
        );
    }

    #[test]
    fn predefined_sprite_and_texture_bands_stay_byte_identical() {
        // Probe-verified fact (isolated engine-assets TPK dump): the two embedded
        // 2022.3 bands carry identical Sprite and Texture2D rows. If a future probe
        // refresh diverges, the band mapping above must gain independent tables.
        fn data_rows(table: &str) -> Vec<&str> {
            table
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .collect()
        }
        assert_eq!(
            data_rows(predefined_engine_sprite_rows("2022.2.15f1").unwrap()),
            data_rows(predefined_engine_sprite_rows("2022.3.11f1").unwrap())
        );
        assert_eq!(
            data_rows(predefined_engine_texture_rows("2022.2.15f1").unwrap()),
            data_rows(predefined_engine_texture_rows("2022.3.11f1").unwrap())
        );
    }

    #[test]
    fn unity_2022_3_predefined_mesh_matches_name_counts_and_bounds() {
        // 2022.3.22f1 selects the 2022.3.11f1 band. The builtin cube is path ID 10202
        // with 24 vertices, one sub-mesh and a (0,0,0)/(0.5,0.5,0.5) bounding box.
        let target = predefined_engine_mesh_target(
            "Cube",
            24,
            1,
            [0.0, 0.0, 0.0],
            [0.5, 0.5, 0.5],
            Some("2022.3.22f1"),
        )
        .unwrap()
        .expect("the 2022.3.11f1 engine-assets band contains the builtin Cube mesh");
        assert_eq!(target.file_id, 10202);
        // The builtin primitives live in Unity's default resources, so the row's resource
        // kind is `D` and the redirect identity is the default-resources GUID.
        assert_eq!(target.guid, default_resources_guid());
        // Every MeshKey component participates in the match.
        for (label, vertex_count, sub_mesh_count, center, extent) in [
            ("vertex count", 23, 1, [0.0, 0.0, 0.0], [0.5, 0.5, 0.5]),
            ("sub-mesh count", 24, 2, [0.0, 0.0, 0.0], [0.5, 0.5, 0.5]),
            ("center", 24, 1, [0.0, 0.0, 0.25], [0.5, 0.5, 0.5]),
            ("extent", 24, 1, [0.0, 0.0, 0.0], [0.5, 0.5, 0.25]),
        ] {
            assert_eq!(
                predefined_engine_mesh_target(
                    "Cube",
                    vertex_count,
                    sub_mesh_count,
                    center,
                    extent,
                    Some("2022.3.22f1")
                )
                .unwrap(),
                None,
                "a mismatched {label} must not redirect"
            );
        }
        assert_eq!(
            predefined_engine_mesh_target(
                "NoSuchMesh",
                24,
                1,
                [0.0, 0.0, 0.0],
                [0.5, 0.5, 0.5],
                Some("2022.3.22f1")
            )
            .unwrap(),
            None
        );
    }

    #[test]
    fn predefined_mesh_bands_are_per_band_and_include_the_builtin_cube() {
        for band in ["2018.3.0b6", "2022.2.15f1", "2022.3.11f1", "6000.0.29f1"] {
            let table = predefined_engine_mesh_rows(band).expect("probed band has a mesh table");
            assert!(
                table.lines().any(|line| line.contains("|Cube|24|1|")),
                "band {band} must carry the builtin Cube row"
            );
        }
        let rows = |table: &'static str| -> Vec<&'static str> {
            table
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .collect()
        };
        // Comparing 2022.2.15f1 with 2022.3.11f1 pins that these two probed bands carry
        // the same mesh identity rows; the 2018 band is allowed to differ.
        assert_eq!(
            rows(predefined_engine_mesh_rows("2022.2.15f1").unwrap()),
            rows(predefined_engine_mesh_rows("2022.3.11f1").unwrap())
        );
        assert!(predefined_engine_mesh_rows("2030.1.0f1").is_none());
    }

    fn default_resources_guid() -> [u8; 16] {
        let mut guid = [0; 16];
        for (index, word) in crate::export::guid_policy::ENGINE_DEFAULT_RESOURCES_GUID_WORDS
            .iter()
            .enumerate()
        {
            guid[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        guid
    }

    fn builtin_extra_guid() -> [u8; 16] {
        let mut guid = [0; 16];
        for (index, word) in crate::export::guid_policy::ENGINE_BUILTIN_EXTRA_GUID_WORDS
            .iter()
            .enumerate()
        {
            guid[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
        }
        guid
    }

    #[test]
    fn unity_2022_3_predefined_sprite_matches_name_and_texture_name() {
        // 2022.3.22f1 selects the 2022.3.11f1 band, whose builtin-extra rows carry
        // the default UI sprites keyed by sprite and texture name.
        let target = predefined_engine_sprite_target(
            "InputFieldBackground",
            Some("InputFieldBackground"),
            Some("2022.3.22f1"),
        )
        .unwrap()
        .expect("the 2022.3.11f1 engine-assets band contains the UI input sprite");
        assert_eq!(target.file_id, 10911);
        assert_eq!(target.guid, builtin_extra_guid());
        // The texture name is part of the baseline cache identity.
        assert_eq!(
            predefined_engine_sprite_target(
                "InputFieldBackground",
                Some("Checkmark"),
                Some("2022.3.22f1")
            )
            .unwrap(),
            None,
            "a mismatched texture name must not redirect"
        );
        assert_eq!(
            predefined_engine_sprite_target("InputFieldBackground", None, Some("2022.3.22f1"))
                .unwrap(),
            None,
            "a null texture must not match rows carrying a texture name"
        );
        assert_eq!(
            predefined_engine_sprite_target(
                "NoSuchSprite",
                Some("NoSuchSprite"),
                Some("2022.3.22f1")
            )
            .unwrap(),
            None
        );
        assert_eq!(
            predefined_engine_sprite_target(
                "InputFieldBackground",
                Some("InputFieldBackground"),
                Some("2021.3.0f1")
            )
            .unwrap(),
            None,
            "a version selecting an unprobed band resolves nothing"
        );
        assert_eq!(
            predefined_engine_sprite_target(
                "InputFieldBackground",
                Some("InputFieldBackground"),
                None
            )
            .unwrap(),
            None,
            "a missing version selects no engine-assets band"
        );
    }

    /// A sprite named like an engine sprite only redirects when its texture
    /// group is an engine texture. The engine "Background" texture is 32x32,
    /// so an authored 20x20 "Background" texture must not match: the sprite
    /// table entry alone is not enough to redirect the sprite.
    #[test]
    fn unity_2022_3_authored_sprite_texture_dimensions_block_the_engine_group() {
        let version = Some("2022.3.22f1");
        // The sprite row exists and matches on name plus texture name.
        assert!(
            predefined_engine_sprite_target("Background", Some("Background"), version)
                .unwrap()
                .is_some(),
            "the 2022.3.11f1 band contains the builtin Background sprite"
        );
        // The engine texture identity additionally requires the dimensions.
        let engine_texture =
            predefined_engine_texture_target("Background", 32, 32, version).unwrap();
        assert!(
            engine_texture.is_some(),
            "the engine Background texture is registered at 32x32"
        );
        assert_eq!(
            predefined_engine_texture_target("Background", 20, 20, version).unwrap(),
            None,
            "an authored 20x20 texture named Background is not the engine texture"
        );
    }

    #[test]
    fn unity_2022_3_predefined_texture_matches_name_and_dimensions() {
        let target =
            predefined_engine_texture_target("Default-Particle", 64, 64, Some("2022.3.22f1"))
                .unwrap()
                .expect("the 2022.3.11f1 engine-assets band contains Default-Particle");
        assert_eq!(target.file_id, 10300);
        assert_eq!(target.guid, builtin_extra_guid());
        // Width and height are part of the baseline cache identity.
        assert_eq!(
            predefined_engine_texture_target("Default-Particle", 32, 64, Some("2022.3.22f1"))
                .unwrap(),
            None,
            "mismatched dimensions must not redirect"
        );
        assert_eq!(
            predefined_engine_texture_target("NoSuchTexture", 64, 64, Some("2022.3.22f1")).unwrap(),
            None
        );
        assert_eq!(
            predefined_engine_texture_target("Default-Particle", 64, 64, Some("2019.4.40f1"))
                .unwrap(),
            None,
            "a version selecting an unprobed band resolves nothing"
        );
    }

    #[test]
    fn unity_6000_predefined_bands_resolve_and_stay_separate() {
        // 6000.0.60f1 selects the 6000.0.29f1 band. Shared rows resolve there;
        // rows added or removed between the 2022.3.11f1 and 6000.0.29f1 bands
        // prove the selections stay independent.
        let sprite = predefined_engine_sprite_target(
            "InputFieldBackground",
            Some("InputFieldBackground"),
            Some("6000.0.60f1"),
        )
        .unwrap()
        .expect("the 6000.0.29f1 engine-assets band keeps the UI input sprite");
        assert_eq!(sprite.file_id, 10911);
        assert_eq!(sprite.guid, builtin_extra_guid());
        let texture =
            predefined_engine_texture_target("Default-Particle", 64, 64, Some("6000.0.60f1"))
                .unwrap()
                .expect("the 6000.0.29f1 engine-assets band keeps Default-Particle");
        assert_eq!(texture.file_id, 10300);
        assert_eq!(texture.guid, builtin_extra_guid());
        let property_names = [
            "_MainTex",
            "_ColorTint",
            "_HueVariationKwToggle",
            "_HueVariationColor",
            "_NormalMapKwToggle",
            "_NormalMap",
            "_ExtraTex",
            "_Glossiness",
            "_Metallic",
            "_SubsurfaceKwToggle",
            "_SubsurfaceTex",
            "_SubsurfaceColor",
            "_SubsurfaceIndirect",
            "_BillboardKwToggle",
            "_BillboardShadowFade",
            "_TwoSided",
            "_LeafFacingKwToggle",
            "_WIND_SHARED",
            "_WIND_BRANCH1",
            "_WIND_BRANCH2",
            "_WIND_RIPPLE",
            "_WIND_SHIMMER",
        ];
        let shader = ShaderView {
            parsed: Some(ParsedShaderForm {
                name: "Nature/SpeedTree9".to_owned(),
                properties: property_names
                    .iter()
                    .map(|name| ParsedShaderProperty {
                        name: (*name).to_owned(),
                        ..ParsedShaderProperty::default()
                    })
                    .collect(),
                ..ParsedShaderForm::default()
            }),
            ..ShaderView::default()
        };
        let target = predefined_engine_shader_target(&shader, Some("6000.0.60f1"))
            .unwrap()
            .expect("the 6000.0.29f1 engine-assets band adds Nature/SpeedTree9");
        assert_eq!(target.file_id, 14003);
        assert_eq!(target.guid, builtin_extra_guid());
        // The 2022-only GraphView UI shader row is gone from the 6000 band.
        let removed = ShaderView {
            parsed: Some(ParsedShaderForm {
                name: "Hidden/GraphView/GraphViewUIE".to_owned(),
                properties: ["_MainTex", "_FontTex", "_CustomTex", "_Color"]
                    .iter()
                    .map(|name| ParsedShaderProperty {
                        name: (*name).to_owned(),
                        ..ParsedShaderProperty::default()
                    })
                    .collect(),
                ..ParsedShaderForm::default()
            }),
            ..ShaderView::default()
        };
        assert_eq!(
            predefined_engine_shader_target(&removed, Some("6000.0.60f1")).unwrap(),
            None,
            "removed 2022 rows must not resolve under the 6000 band"
        );
    }

    #[test]
    fn unity_2018_predefined_bands_resolve() {
        // 2018.4.20f1 selects the 2018.3.0b6 band. Shared UI-sprite and
        // particle-material rows resolve there with builtin-extra identity.
        let sprite = predefined_engine_sprite_target(
            "InputFieldBackground",
            Some("InputFieldBackground"),
            Some("2018.4.20f1"),
        )
        .unwrap()
        .expect("the 2018.3.0b6 engine-assets band contains the UI input sprite");
        assert_eq!(sprite.file_id, 10911);
        assert_eq!(sprite.guid, builtin_extra_guid());
        let material = predefined_engine_material_target(
            "Default-Particle",
            Some("Legacy Shaders/Particles/Alpha Blended Premultiply"),
            Some("2018.4.20f1"),
        )
        .unwrap()
        .expect("the 2018.3.0b6 engine-assets band contains Default-Particle");
        assert_eq!(material.file_id, 10301);
        assert_eq!(material.guid, builtin_extra_guid());
    }
}
