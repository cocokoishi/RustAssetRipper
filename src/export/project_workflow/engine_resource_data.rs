use super::PredefinedEngineAssetTarget;
use serde::de::{Error as DeError, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct EngineResourceData {
    text_assets: Vec<TextAssetEntry>,
    texture_2d_assets: Vec<Texture2DEntry>,
    cubemap_assets: Vec<CubemapEntry>,
    audio_clip_assets: Vec<AudioClipEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TextAssetEntry {
    name: String,
    length: i32,
    hash: u128,
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
    /// Parses the supported TextAsset, Texture2D, Cubemap, and AudioClip subsets of the Mining
    /// EngineResourceData model.
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
        let mut texture_2d_assets = Vec::new();
        let mut cubemap_assets = Vec::new();
        let mut audio_clip_assets = Vec::new();
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
                if kind != "TextAsset" {
                    if matches!(
                        kind,
                        "Behaviour"
                            | "Component"
                            | "GameObject"
                            | "NamedObject"
                            | "Material"
                            | "Mesh"
                            | "MonoBehaviour"
                            | "MonoScript"
                            | "Shader"
                            | "Sprite"
                    ) {
                        return Err(super::invalid_project(format!(
                            "stored EngineResourceData contains unsupported {kind} cache entries; only TextAsset, Texture2D, Cubemap, and AudioClip matching is currently wired"
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
            texture_2d_assets,
            cubemap_assets,
            audio_clip_assets,
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
            r#"{"DefaultResources":{"900":{"$type":"Shader","Name":"Custom"}},"ExtraResources":{}}"#,
        ))
        .expect_err("unsupported typed resources must not be silently discarded");
        assert!(error.to_string().contains(
            "only TextAsset, Texture2D, Cubemap, and AudioClip matching is currently wired"
        ));
    }
}
