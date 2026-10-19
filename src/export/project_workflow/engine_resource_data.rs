use super::PredefinedEngineAssetTarget;
use serde::de::{Error as DeError, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct EngineResourceData {
    text_assets: Vec<TextAssetEntry>,
    mesh_assets: Vec<MeshEntry>,
    sprite_assets: Vec<SpriteEntry>,
    material_assets: Vec<MaterialEntry>,
    shader_assets: Vec<ShaderEntry>,
    texture_2d_assets: Vec<Texture2DEntry>,
    cubemap_assets: Vec<CubemapEntry>,
    audio_clip_assets: Vec<AudioClipEntry>,
    game_object_assets: Vec<GameObjectEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TextAssetEntry {
    name: String,
    length: i32,
    hash: u128,
    target: PredefinedEngineAssetTarget,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MeshEntry {
    name: String,
    vertex_count: i32,
    sub_mesh_count: i32,
    target: PredefinedEngineAssetTarget,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SpriteEntry {
    name: String,
    texture_name: Option<String>,
    target: PredefinedEngineAssetTarget,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct MaterialEntry {
    name: String,
    shader_name: Option<String>,
    target: PredefinedEngineAssetTarget,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ShaderEntry {
    name: String,
    property_names: Vec<String>,
    target: PredefinedEngineAssetTarget,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Texture2DEntry {
    name: String,
    width: i32,
    height: i32,
    target: PredefinedEngineAssetTarget,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CubemapEntry {
    name: String,
    width: i32,
    height: i32,
    target: PredefinedEngineAssetTarget,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AudioClipEntry {
    name: String,
    channels: i32,
    frequency: i32,
    length_bits: u32,
    target: PredefinedEngineAssetTarget,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct GameObjectEntry {
    name: String,
    layer: u32,
    component_count: usize,
    target: PredefinedEngineAssetTarget,
}

#[derive(Default)]
struct OrderedResourceMap(Vec<(i64, Value)>);

impl<'de> Deserialize<'de> for OrderedResourceMap {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct OrderedResourceMapVisitor;

        impl<'de> Visitor<'de> for OrderedResourceMapVisitor {
            type Value = OrderedResourceMap;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an EngineResourceData resource dictionary")
            }

            fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut entries = Vec::new();
                while let Some((key, value)) = map.next_entry::<String, Value>()? {
                    let path_id = key.parse::<i64>().map_err(M::Error::custom)?;
                    entries.push((path_id, value));
                }
                Ok(OrderedResourceMap(entries))
            }
        }

        deserializer.deserialize_map(OrderedResourceMapVisitor)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawEngineResourceData {
    #[serde(default)]
    default_resources: Option<OrderedResourceMap>,
    #[serde(default)]
    extra_resources: Option<OrderedResourceMap>,
}

impl EngineResourceData {
    /// Parses the supported TextAsset, Mesh, Sprite, Material, Shader, Texture2D, Cubemap,
    /// AudioClip, and GameObject subsets of the Mining EngineResourceData model.
    ///
    /// A missing, null, malformed, or otherwise non-deserializable nullable value means the
    /// frozen serializer produced `null` and the exporter falls back to embedded TPK data. A
    /// valid empty object remains `Some`, because it selects a custom empty cache and suppresses
    /// that TPK fallback. Known object kinds without a stored-resource cache key fail loudly; they
    /// are never silently dropped.
    pub(super) fn parse(text: Option<&str>) -> Result<Option<Self>, Box<dyn std::error::Error>> {
        let Some(text) = text.filter(|text| !text.is_empty()) else {
            return Ok(None);
        };
        let Ok(raw) = serde_json::from_str::<RawEngineResourceData>(text) else {
            return Ok(None);
        };

        let mut text_assets = Vec::new();
        let mut mesh_assets = Vec::new();
        let mut sprite_assets = Vec::new();
        let mut material_assets = Vec::new();
        let mut shader_assets = Vec::new();
        let mut texture_2d_assets = Vec::new();
        let mut cubemap_assets = Vec::new();
        let mut audio_clip_assets = Vec::new();
        let mut game_object_assets = Vec::new();
        for (resources, guid_words) in [
            (
                raw.default_resources.unwrap_or_default(),
                crate::export::guid_policy::ENGINE_DEFAULT_RESOURCES_GUID_WORDS,
            ),
            (
                raw.extra_resources.unwrap_or_default(),
                crate::export::guid_policy::ENGINE_BUILTIN_EXTRA_GUID_WORDS,
            ),
        ] {
            let mut guid = [0; 16];
            for (index, word) in guid_words.iter().enumerate() {
                guid[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
            }
            for (file_id, value) in resources.0 {
                if value.is_null() {
                    continue;
                }
                let Some(object) = value.as_object() else {
                    return Ok(None);
                };
                let Some(kind) = object.get("$type").and_then(Value::as_str) else {
                    return Ok(None);
                };
                if kind == "Material" {
                    let Some(name) = object.get("Name").and_then(Value::as_str) else {
                        return Ok(None);
                    };
                    let shader_name = match object.get("Shader") {
                        None | Some(Value::Null) => None,
                        Some(Value::String(shader_name)) => Some(shader_name.clone()),
                        Some(_) => return Ok(None),
                    };
                    material_assets.push(MaterialEntry {
                        name: name.to_owned(),
                        shader_name,
                        target: PredefinedEngineAssetTarget { file_id, guid },
                    });
                    continue;
                }
                if kind == "Mesh" {
                    let Some(name) = object.get("Name").and_then(Value::as_str) else {
                        return Ok(None);
                    };
                    let Some(vertex_count) = object
                        .get("VertexCount")
                        .and_then(Value::as_i64)
                        .and_then(|count| i32::try_from(count).ok())
                    else {
                        return Ok(None);
                    };
                    let Some(sub_mesh_count) = object
                        .get("SubMeshCount")
                        .and_then(Value::as_i64)
                        .and_then(|count| i32::try_from(count).ok())
                    else {
                        return Ok(None);
                    };
                    let Some(local_aabb) = object.get("LocalAABB").and_then(Value::as_object)
                    else {
                        return Ok(None);
                    };
                    if ["Center", "Extent"].into_iter().any(|field| {
                        local_aabb
                            .get(field)
                            .is_some_and(|value| !value.is_object())
                    }) {
                        return Ok(None);
                    }
                    mesh_assets.push(MeshEntry {
                        name: name.to_owned(),
                        vertex_count,
                        sub_mesh_count,
                        target: PredefinedEngineAssetTarget { file_id, guid },
                    });
                    continue;
                }
                if kind == "Sprite" {
                    let Some(name) = object.get("Name").and_then(Value::as_str) else {
                        return Ok(None);
                    };
                    let texture_name = match object.get("Texture") {
                        None | Some(Value::Null) => None,
                        Some(Value::String(texture_name)) => Some(texture_name.clone()),
                        Some(_) => return Ok(None),
                    };
                    sprite_assets.push(SpriteEntry {
                        name: name.to_owned(),
                        texture_name,
                        target: PredefinedEngineAssetTarget { file_id, guid },
                    });
                    continue;
                }
                if kind == "Shader" {
                    let Some(name) = object.get("Name").and_then(Value::as_str) else {
                        return Ok(None);
                    };
                    let property_names = match object.get("PropertyNames") {
                        None | Some(Value::Null) => Vec::new(),
                        Some(Value::Array(values)) => {
                            let Some(names) =
                                values.iter().map(Value::as_str).collect::<Option<Vec<_>>>()
                            else {
                                return Ok(None);
                            };
                            names.into_iter().map(str::to_owned).collect()
                        }
                        Some(_) => return Ok(None),
                    };
                    shader_assets.push(ShaderEntry {
                        name: name.to_owned(),
                        property_names,
                        target: PredefinedEngineAssetTarget { file_id, guid },
                    });
                    continue;
                }
                if kind == "Texture2D" {
                    let Some(name) = object.get("Name").and_then(Value::as_str) else {
                        return Ok(None);
                    };
                    let Some(width) = object
                        .get("Width")
                        .and_then(Value::as_i64)
                        .and_then(|width| i32::try_from(width).ok())
                    else {
                        return Ok(None);
                    };
                    let Some(height) = object
                        .get("Height")
                        .and_then(Value::as_i64)
                        .and_then(|height| i32::try_from(height).ok())
                    else {
                        return Ok(None);
                    };
                    texture_2d_assets.push(Texture2DEntry {
                        name: name.to_owned(),
                        width,
                        height,
                        target: PredefinedEngineAssetTarget { file_id, guid },
                    });
                    continue;
                }
                if kind == "Cubemap" {
                    let Some(name) = object.get("Name").and_then(Value::as_str) else {
                        return Ok(None);
                    };
                    let Some(width) = object
                        .get("Width")
                        .and_then(Value::as_i64)
                        .and_then(|width| i32::try_from(width).ok())
                    else {
                        return Ok(None);
                    };
                    let Some(height) = object
                        .get("Height")
                        .and_then(Value::as_i64)
                        .and_then(|height| i32::try_from(height).ok())
                    else {
                        return Ok(None);
                    };
                    cubemap_assets.push(CubemapEntry {
                        name: name.to_owned(),
                        width,
                        height,
                        target: PredefinedEngineAssetTarget { file_id, guid },
                    });
                    continue;
                }
                if kind == "AudioClip" {
                    let Some(name) = object.get("Name").and_then(Value::as_str) else {
                        return Ok(None);
                    };
                    let Some(channels) = object
                        .get("Channels")
                        .and_then(Value::as_i64)
                        .and_then(|channels| i32::try_from(channels).ok())
                    else {
                        return Ok(None);
                    };
                    let Some(frequency) = object
                        .get("Frequency")
                        .and_then(Value::as_i64)
                        .and_then(|frequency| i32::try_from(frequency).ok())
                    else {
                        return Ok(None);
                    };
                    let Some(length) = object
                        .get("Length")
                        .and_then(Value::as_f64)
                        .map(|length| length as f32)
                        .filter(|length| length.is_finite())
                    else {
                        return Ok(None);
                    };
                    audio_clip_assets.push(AudioClipEntry {
                        name: name.to_owned(),
                        channels,
                        frequency,
                        length_bits: length.to_bits(),
                        target: PredefinedEngineAssetTarget { file_id, guid },
                    });
                    continue;
                }
                if kind == "GameObject" {
                    let Some(name) = object.get("Name").and_then(Value::as_str) else {
                        return Ok(None);
                    };
                    let Some(layer) = object
                        .get("Layer")
                        .and_then(Value::as_u64)
                        .and_then(|layer| u32::try_from(layer).ok())
                    else {
                        return Ok(None);
                    };
                    let Some(components) = object.get("Components").and_then(Value::as_array)
                    else {
                        return Ok(None);
                    };
                    if components.iter().any(|component| {
                        component
                            .as_i64()
                            .and_then(|component| i32::try_from(component).ok())
                            .is_none()
                    }) {
                        return Ok(None);
                    }
                    game_object_assets.push(GameObjectEntry {
                        name: name.to_owned(),
                        layer,
                        component_count: components.len(),
                        target: PredefinedEngineAssetTarget { file_id, guid },
                    });
                    continue;
                }
                if kind != "TextAsset" {
                    if matches!(
                        kind,
                        "Behaviour" | "Component" | "NamedObject" | "MonoBehaviour" | "MonoScript"
                    ) {
                        return Err(super::invalid_project(format!(
                            "stored EngineResourceData contains unsupported {kind} cache entries; only TextAsset, Mesh, Sprite, Material, Shader, Texture2D, Cubemap, AudioClip, and GameObject matching is currently wired"
                        )));
                    }
                    return Ok(None);
                }
                let Some(name) = object.get("Name").and_then(Value::as_str) else {
                    return Ok(None);
                };
                let Some(length) = object
                    .get("Length")
                    .and_then(Value::as_i64)
                    .and_then(|length| i32::try_from(length).ok())
                else {
                    return Ok(None);
                };
                let Some(hash) = object
                    .get("Hash")
                    .and_then(Value::as_number)
                    .and_then(|hash| hash.to_string().parse::<u128>().ok())
                else {
                    return Ok(None);
                };
                text_assets.push(TextAssetEntry {
                    name: name.to_owned(),
                    length,
                    hash,
                    target: PredefinedEngineAssetTarget { file_id, guid },
                });
            }
        }
        Ok(Some(Self {
            text_assets,
            mesh_assets,
            sprite_assets,
            material_assets,
            shader_assets,
            texture_2d_assets,
            cubemap_assets,
            audio_clip_assets,
            game_object_assets,
        }))
    }

    /// The frozen cache keys TextAssets by their complete value: name, byte length, and MD5.
    /// Source-runtime differential evidence redirects an eight-byte `print(1)` payload at
    /// Length 8, while the same name/hash at Length 7 stays a normal exported TextAsset.
    pub(super) fn text_asset_target(
        &self,
        name: &str,
        bytes: &[u8],
    ) -> Option<PredefinedEngineAssetTarget> {
        let length = i32::try_from(bytes.len()).ok()?;
        let hash = u128::from_le_bytes(crate::export::script_hashing::md5(bytes));
        self.text_assets
            .iter()
            .find(|entry| entry.name == name && entry.length == length && entry.hash == hash)
            .map(|entry| entry.target)
    }

    /// The frozen Web serializer reads System.Numerics.Vector3 public fields as empty objects,
    /// so every stored Mesh LocalAABB reaches the cache as a zero center and extent. Mesh inputs
    /// still compare their actual bounds against that zero-valued key.
    pub(super) fn mesh_target(
        &self,
        name: &str,
        vertex_count: i32,
        sub_mesh_count: usize,
        center: [f32; 3],
        extent: [f32; 3],
    ) -> Option<PredefinedEngineAssetTarget> {
        self.mesh_assets
            .iter()
            .find(|entry| {
                entry.name == name
                    && entry.vertex_count == vertex_count
                    && usize::try_from(entry.sub_mesh_count).ok() == Some(sub_mesh_count)
                    && center == [0.0; 3]
                    && extent == [0.0; 3]
            })
            .map(|entry| entry.target)
    }

    /// The frozen cache keys Sprites by their name and nullable Texture name.
    pub(super) fn sprite_target(
        &self,
        name: &str,
        texture_name: Option<&str>,
    ) -> Option<PredefinedEngineAssetTarget> {
        self.sprite_assets
            .iter()
            .find(|entry| entry.name == name && entry.texture_name.as_deref() == texture_name)
            .map(|entry| entry.target)
    }

    /// The frozen cache keys Materials by name and the resolved Shader name.
    pub(super) fn material_target(
        &self,
        name: &str,
        shader_name: Option<&str>,
    ) -> Option<PredefinedEngineAssetTarget> {
        self.material_assets
            .iter()
            .find(|entry| entry.name == name && entry.shader_name.as_deref() == shader_name)
            .map(|entry| entry.target)
    }

    /// The frozen cache hashes the ordered Shader property names alongside the Shader name.
    /// Comparing the hash inputs directly preserves the identity without depending on .NET's
    /// process-randomized HashCode seed.
    pub(super) fn shader_target(
        &self,
        name: &str,
        property_names: &[&str],
    ) -> Option<PredefinedEngineAssetTarget> {
        self.shader_assets
            .iter()
            .find(|entry| {
                entry.name == name
                    && entry
                        .property_names
                        .iter()
                        .map(String::as_str)
                        .eq(property_names.iter().copied())
            })
            .map(|entry| entry.target)
    }

    /// The frozen cache keys plain Texture2D objects by name and dimensions.
    pub(super) fn texture_2d_target(
        &self,
        name: &str,
        width: i32,
        height: i32,
    ) -> Option<PredefinedEngineAssetTarget> {
        self.texture_2d_assets
            .iter()
            .find(|entry| entry.name == name && entry.width == width && entry.height == height)
            .map(|entry| entry.target)
    }

    /// The frozen cache keys Cubemaps by name and dimensions.
    pub(super) fn cubemap_target(
        &self,
        name: &str,
        width: i32,
        height: i32,
    ) -> Option<PredefinedEngineAssetTarget> {
        self.cubemap_assets
            .iter()
            .find(|entry| entry.name == name && entry.width == width && entry.height == height)
            .map(|entry| entry.target)
    }

    /// The frozen cache keys AudioClips by name, channel count, frequency, and float length.
    pub(super) fn audio_clip_target(
        &self,
        name: &str,
        channels: i32,
        frequency: i32,
        length: f32,
    ) -> Option<PredefinedEngineAssetTarget> {
        self.audio_clip_assets
            .iter()
            .find(|entry| {
                entry.name == name
                    && entry.channels == channels
                    && entry.frequency == frequency
                    && f32::from_bits(entry.length_bits) == length
            })
            .map(|entry| entry.target)
    }

    /// The frozen cache keys GameObjects by name, layer, and component count.
    pub(super) fn game_object_target(
        &self,
        name: &str,
        layer: u32,
        component_count: usize,
    ) -> Option<PredefinedEngineAssetTarget> {
        self.game_object_assets
            .iter()
            .find(|entry| {
                entry.name == name
                    && entry.layer == layer
                    && entry.component_count == component_count
            })
            .map(|entry| entry.target)
    }
}

#[cfg(test)]
mod tests {
    use super::EngineResourceData;

    #[test]
    fn engine_resource_data_textasset_matches_name_length_hash_and_resource_identity() {
        let data = EngineResourceData::parse(Some(
            r#"{"DefaultResources":{"777":{"$type":"TextAsset","Name":"Hero","Length":8,"Hash":130469715505528193672212848297838865176}},"ExtraResources":{}}"#,
        ))
        .expect("TextAsset resource parses")
        .expect("valid object selects a stored cache");
        let target = data
            .text_asset_target("Hero", b"print(1)")
            .expect("name, byte length, and UInt128 MD5 identify the source TextAsset");
        assert_eq!(target.file_id, 777);
        assert_eq!(target.guid[8..12], 14u32.to_le_bytes());
        assert!(data.text_asset_target("Hero", b"print(2)").is_none());
        assert!(data.text_asset_target("Other", b"print(1)").is_none());

        let mismatched_length = EngineResourceData::parse(Some(
            r#"{"DefaultResources":{"777":{"$type":"TextAsset","Name":"Hero","Length":7,"Hash":130469715505528193672212848297838865176}},"ExtraResources":{}}"#,
        ))
        .expect("mismatched Length remains a valid resource entry")
        .expect("valid object selects a stored cache");
        assert!(
            mismatched_length
                .text_asset_target("Hero", b"print(1)")
                .is_none(),
            "the serialized Length participates in the source TextAsset cache key"
        );
    }

    #[test]
    fn engine_resource_data_mesh_matches_name_counts_and_source_zero_bounds() {
        let data = EngineResourceData::parse(Some(
            r#"{"DefaultResources":{"777":{"$type":"Mesh","Name":"LightmappedMesh","VertexCount":0,"SubMeshCount":0,"LocalAABB":{"Center":{"X":1,"Y":2,"Z":3},"Extent":{"X":4,"Y":5,"Z":6}}}},"ExtraResources":{"779":{"$type":"Mesh","Name":"ExtraMesh","VertexCount":2,"SubMeshCount":1,"LocalAABB":{"Center":{},"Extent":{}}}}}"#,
        ))
        .expect("Mesh resource entries parse")
        .expect("valid object selects a stored cache");
        let target = data
            .mesh_target("LightmappedMesh", 0, 0, [0.0; 3], [0.0; 3])
            .expect("the source Web serializer drops Vector3 public fields");
        assert_eq!(target.file_id, 777);
        assert_eq!(target.guid[8..12], 14u32.to_le_bytes());
        let extra_target = data
            .mesh_target("ExtraMesh", 2, 1, [0.0; 3], [0.0; 3])
            .expect("ExtraResources meshes retain their resource identity");
        assert_eq!(extra_target.file_id, 779);
        assert_eq!(extra_target.guid[8..12], 15u32.to_le_bytes());
        assert!(data
            .mesh_target("LightmappedMesh", 1, 0, [0.0; 3], [0.0; 3])
            .is_none());
        assert!(data
            .mesh_target("LightmappedMesh", 0, 1, [0.0; 3], [0.0; 3])
            .is_none());
        assert!(data
            .mesh_target("LightmappedMesh", 0, 0, [1.0, 0.0, 0.0], [0.0; 3])
            .is_none());
        assert!(data
            .mesh_target("LightmappedMesh", 0, 0, [0.0; 3], [0.0, 1.0, 0.0])
            .is_none());
        assert!(data
            .mesh_target("OtherMesh", 0, 0, [0.0; 3], [0.0; 3])
            .is_none());
    }

    #[test]
    fn engine_resource_data_sprite_matches_name_texture_and_resource_identity() {
        let data = EngineResourceData::parse(Some(
            r#"{"DefaultResources":{"777":{"$type":"Sprite","Name":"PackedSpriteFixture","Texture":"AtlasTextureFixture"},"778":{"$type":"Sprite","Name":"NullTextureSprite","Texture":null}},"ExtraResources":{"779":{"$type":"Sprite","Name":"ExtraSprite","Texture":"AtlasTextureFixture"}}}"#,
        ))
        .expect("Sprite resource entries parse")
        .expect("valid object selects a stored cache");
        let target = data
            .sprite_target("PackedSpriteFixture", Some("AtlasTextureFixture"))
            .expect("name and nullable Texture name identify the source Sprite");
        assert_eq!(target.file_id, 777);
        assert_eq!(target.guid[8..12], 14u32.to_le_bytes());
        let null_texture_target = data
            .sprite_target("NullTextureSprite", None)
            .expect("null Sprite Texture remains part of the cache key");
        assert_eq!(null_texture_target.file_id, 778);
        let extra_target = data
            .sprite_target("ExtraSprite", Some("AtlasTextureFixture"))
            .expect("ExtraResources Sprites retain their resource identity");
        assert_eq!(extra_target.file_id, 779);
        assert_eq!(extra_target.guid[8..12], 15u32.to_le_bytes());
        assert!(data
            .sprite_target("PackedSpriteFixture", Some("OtherTexture"))
            .is_none());
        assert!(data.sprite_target("PackedSpriteFixture", None).is_none());
        assert!(data
            .sprite_target("OtherSprite", Some("AtlasTextureFixture"))
            .is_none());
    }

    #[test]
    fn engine_resource_data_material_matches_name_shader_and_resource_identity() {
        let data = EngineResourceData::parse(Some(
            r#"{"DefaultResources":{"777":{"$type":"Material","Name":"MaterialFixture","Shader":"ShaderFixture"},"778":{"$type":"Material","Name":"NullShaderFixture","Shader":null}},"ExtraResources":{"779":{"$type":"Material","Name":"ExtraMaterialFixture","Shader":"ShaderFixture"}}}"#,
        ))
        .expect("Material resource entries parse")
        .expect("valid object selects a stored cache");
        let target = data
            .material_target("MaterialFixture", Some("ShaderFixture"))
            .expect("name and resolved shader name identify the source Material");
        assert_eq!(target.file_id, 777);
        assert_eq!(target.guid[8..12], 14u32.to_le_bytes());
        let null_shader_target = data
            .material_target("NullShaderFixture", None)
            .expect("a null Material shader remains part of the cache key");
        assert_eq!(null_shader_target.file_id, 778);
        let extra_target = data
            .material_target("ExtraMaterialFixture", Some("ShaderFixture"))
            .expect("extra Material entries use their resource identity");
        assert_eq!(extra_target.file_id, 779);
        assert_eq!(extra_target.guid[8..12], 15u32.to_le_bytes());
        assert!(data
            .material_target("MaterialFixture", Some("OtherShader"))
            .is_none());
        assert!(data
            .material_target("OtherMaterial", Some("ShaderFixture"))
            .is_none());
        assert!(data.material_target("MaterialFixture", None).is_none());
    }

    #[test]
    fn engine_resource_data_shader_matches_name_and_ordered_property_names() {
        let data = EngineResourceData::parse(Some(
            r#"{"DefaultResources":{"777":{"$type":"Shader","Name":"ShaderFixture","PropertyNames":["_Color","_MainTex"]},"778":{"$type":"Shader","Name":"EmptyShader","PropertyNames":null}},"ExtraResources":{"779":{"$type":"Shader","Name":"ExtraShader","PropertyNames":["_MainTex","_Color"]}}}"#,
        ))
        .expect("Shader resource entries parse")
        .expect("valid object selects a stored cache");
        let target = data
            .shader_target("ShaderFixture", &["_Color", "_MainTex"])
            .expect("Shader name and ordered properties identify the source Shader");
        assert_eq!(target.file_id, 777);
        assert_eq!(target.guid[8..12], 14u32.to_le_bytes());
        let empty_target = data
            .shader_target("EmptyShader", &[])
            .expect("null property lists become the source empty array");
        assert_eq!(empty_target.file_id, 778);
        let extra_target = data
            .shader_target("ExtraShader", &["_MainTex", "_Color"])
            .expect("extra Shader entries retain their identity");
        assert_eq!(extra_target.file_id, 779);
        assert_eq!(extra_target.guid[8..12], 15u32.to_le_bytes());
        assert!(data
            .shader_target("ShaderFixture", &["_MainTex", "_Color"])
            .is_none());
        assert!(data.shader_target("ShaderFixture", &["_Color"]).is_none());
        assert!(data
            .shader_target("OtherShader", &["_Color", "_MainTex"])
            .is_none());
    }

    #[test]
    fn engine_resource_data_texture2d_matches_name_dimensions_and_resource_identity() {
        let data = EngineResourceData::parse(Some(
            r#"{"DefaultResources":{"777":{"$type":"Texture2D","Name":"TextureFixture","Width":2,"Height":2}},"ExtraResources":{"700":{"$type":"Texture2D","Name":"TextureFixture","Width":2,"Height":2},"778":{"$type":"Texture2D","Name":"ExtraTextureFixture","Width":2,"Height":2}}}"#,
        ))
        .expect("Texture2D resource parses")
        .expect("valid object selects a stored cache");
        let target = data
            .texture_2d_target("TextureFixture", 2, 2)
            .expect("name and dimensions identify the source Texture2D");
        assert_eq!(target.file_id, 777);
        assert_eq!(target.guid[8..12], 14u32.to_le_bytes());
        let extra_target = data
            .texture_2d_target("ExtraTextureFixture", 2, 2)
            .expect("extra resource matches by name and dimensions");
        assert_eq!(extra_target.file_id, 778);
        assert_eq!(extra_target.guid[8..12], 15u32.to_le_bytes());
        assert!(data.texture_2d_target("TextureFixture", 3, 2).is_none());
        assert!(data.texture_2d_target("Other", 2, 2).is_none());
    }

    #[test]
    fn engine_resource_data_audioclip_matches_name_metadata_and_resource_identity() {
        let data = EngineResourceData::parse(Some(
            r#"{"DefaultResources":{"900":{"$type":"AudioClip","Name":"AudioFixture","Channels":0,"Frequency":0,"Length":0.0}},"ExtraResources":{"700":{"$type":"AudioClip","Name":"AudioFixture","Channels":0,"Frequency":0,"Length":0.0},"901":{"$type":"AudioClip","Name":"ExtraAudioFixture","Channels":2,"Frequency":44100,"Length":1.25}}}"#,
        ))
        .expect("AudioClip resources parse")
        .expect("valid object selects a stored cache");
        let target = data
            .audio_clip_target("AudioFixture", 0, 0, 0.0)
            .expect("name and audio metadata identify the source AudioClip");
        assert_eq!(target.file_id, 900);
        assert_eq!(target.guid[8..12], 14u32.to_le_bytes());
        let extra_target = data
            .audio_clip_target("ExtraAudioFixture", 2, 44100, 1.25)
            .expect("extra resource matches by name and audio metadata");
        assert_eq!(extra_target.file_id, 901);
        assert_eq!(extra_target.guid[8..12], 15u32.to_le_bytes());
        assert!(data.audio_clip_target("AudioFixture", 1, 0, 0.0).is_none());
        assert!(data.audio_clip_target("AudioFixture", 0, 1, 0.0).is_none());
        assert!(data.audio_clip_target("AudioFixture", 0, 0, 0.1).is_none());
        assert!(data.audio_clip_target("Other", 0, 0, 0.0).is_none());
    }

    #[test]
    fn engine_resource_data_cubemap_matches_name_dimensions_and_resource_identity() {
        let data = EngineResourceData::parse(Some(
            r#"{"DefaultResources":{"900":{"$type":"Cubemap","Name":"CubeFixture","Width":2,"Height":2}},"ExtraResources":{"700":{"$type":"Cubemap","Name":"CubeFixture","Width":2,"Height":2},"901":{"$type":"Cubemap","Name":"ExtraCubeFixture","Width":4,"Height":4}}}"#,
        ))
        .expect("Cubemap resources parse")
        .expect("valid object selects a stored cache");
        let target = data
            .cubemap_target("CubeFixture", 2, 2)
            .expect("name and dimensions identify the source Cubemap");
        assert_eq!(target.file_id, 900);
        assert_eq!(target.guid[8..12], 14u32.to_le_bytes());
        let extra_target = data
            .cubemap_target("ExtraCubeFixture", 4, 4)
            .expect("extra resource matches by name and dimensions");
        assert_eq!(extra_target.file_id, 901);
        assert_eq!(extra_target.guid[8..12], 15u32.to_le_bytes());
        assert!(data.cubemap_target("CubeFixture", 3, 2).is_none());
        assert!(data.cubemap_target("CubeFixture", 2, 3).is_none());
        assert!(data.cubemap_target("Other", 2, 2).is_none());
    }

    #[test]
    fn engine_resource_data_gameobject_matches_name_layer_component_count_and_identity() {
        // This JSON shape is emitted by the frozen EngineResourceDataContext serializer.
        let data = EngineResourceData::parse(Some(
            r#"{"DefaultResources":{"900":{"$type":"GameObject","Name":"FixtureObject","Components":[4,224],"Layer":4294967295}},"ExtraResources":{"901":{"$type":"GameObject","Name":"FixtureObject","Components":[4,224],"Layer":4294967295},"902":{"$type":"GameObject","Name":"ExtraFixtureObject","Components":[4],"Layer":3}}}"#,
        ))
        .expect("GameObject resource entries parse")
        .expect("valid object selects a stored cache");

        let default_target = data
            .game_object_target("FixtureObject", u32::MAX, 2)
            .expect("name, UInt32 layer, and component count identify the source GameObject");
        assert_eq!(default_target.file_id, 900);
        assert_eq!(default_target.guid[8..12], 14u32.to_le_bytes());
        assert_eq!(
            data.game_object_target("ExtraFixtureObject", 3, 1)
                .expect("extra GameObjects retain their resource identity")
                .file_id,
            902
        );
        let extra_target = data
            .game_object_target("ExtraFixtureObject", 3, 1)
            .expect("matching extra GameObject");
        assert_eq!(extra_target.guid[8..12], 15u32.to_le_bytes());

        assert!(data.game_object_target("Other", u32::MAX, 2).is_none());
        assert!(data
            .game_object_target("FixtureObject", u32::MAX - 1, 2)
            .is_none());
        assert!(data
            .game_object_target("FixtureObject", u32::MAX, 1)
            .is_none());
    }

    #[test]
    fn valid_empty_engine_resource_data_selects_a_custom_empty_cache() {
        assert!(EngineResourceData::parse(Some("{}"))
            .expect("valid empty object parses")
            .is_some());
        assert!(EngineResourceData::parse(Some(
            r#"{"DefaultResources":null,"ExtraResources":{}}"#
        ))
        .expect("null dictionaries use their source defaults")
        .is_some());
        assert!(EngineResourceData::parse(Some("null"))
            .expect("nullable null parses")
            .is_none());
        assert!(EngineResourceData::parse(Some("not-json"))
            .expect("source serializer replaces malformed input with null")
            .is_none());
    }

    #[test]
    fn duplicate_textasset_signatures_keep_source_dictionary_order() {
        let data = EngineResourceData::parse(Some(
            r#"{"DefaultResources":{"900":{"$type":"TextAsset","Name":"Hero","Length":8,"Hash":130469715505528193672212848297838865176}},"ExtraResources":{"700":{"$type":"TextAsset","Name":"Hero","Length":8,"Hash":130469715505528193672212848297838865176}}}"#,
        ))
        .expect("duplicate signatures parse")
        .expect("valid custom resource data");
        assert_eq!(
            data.text_asset_target("Hero", b"print(1)")
                .expect("matching signature")
                .file_id,
            900,
            "the default-resource dictionary is visited before builtin-extra"
        );
    }

    #[test]
    fn extra_textasset_resources_use_the_builtin_extra_guid() {
        let data = EngineResourceData::parse(Some(
            r#"{"DefaultResources":{},"ExtraResources":{"700":{"$type":"TextAsset","Name":"Hero","Length":8,"Hash":130469715505528193672212848297838865176}}}"#,
        ))
        .expect("extra TextAsset resource parses")
        .expect("valid custom resource data");
        let target = data
            .text_asset_target("Hero", b"print(1)")
            .expect("matching signature");
        assert_eq!(target.file_id, 700);
        assert_eq!(target.guid[8..12], 15u32.to_le_bytes());
    }

    #[test]
    fn known_unimplemented_resource_types_fail_before_project_writes() {
        let error = EngineResourceData::parse(Some(
            r#"{"DefaultResources":{"900":{"$type":"MonoBehaviour","Name":"Custom"}},"ExtraResources":{}}"#,
        ))
        .expect_err("unsupported typed resources must not be silently discarded");
        assert!(error.to_string().contains(
            "only TextAsset, Mesh, Sprite, Material, Shader, Texture2D, Cubemap, AudioClip, and GameObject matching is currently wired"
        ));
    }
}
