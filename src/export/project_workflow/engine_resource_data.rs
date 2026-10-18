use super::PredefinedEngineAssetTarget;
use serde::de::{Error as DeError, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct EngineResourceData {
    text_assets: Vec<TextAssetEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TextAssetEntry {
    name: String,
    hash: u128,
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
    /// Parses the supported TextAsset subset of the Mining EngineResourceData model.
    ///
    /// A missing, null, malformed, or otherwise non-deserializable nullable value means the
    /// frozen serializer produced `null` and the exporter falls back to embedded TPK data. A
    /// valid empty object remains `Some`, because it selects a custom empty cache and suppresses
    /// that TPK fallback. Known non-TextAsset object kinds fail loudly until their cache keys are
    /// implemented; they are never silently dropped.
    pub(super) fn parse(text: Option<&str>) -> Result<Option<Self>, Box<dyn std::error::Error>> {
        let Some(text) = text.filter(|text| !text.is_empty()) else {
            return Ok(None);
        };
        let Ok(raw) = serde_json::from_str::<RawEngineResourceData>(text) else {
            return Ok(None);
        };

        let mut text_assets = Vec::new();
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
                if kind != "TextAsset" {
                    if matches!(
                        kind,
                        "AudioClip"
                            | "Behaviour"
                            | "Component"
                            | "Cubemap"
                            | "GameObject"
                            | "NamedObject"
                            | "Material"
                            | "Mesh"
                            | "MonoBehaviour"
                            | "MonoScript"
                            | "Shader"
                            | "Sprite"
                            | "Texture2D"
                    ) {
                        return Err(super::invalid_project(format!(
                            "stored EngineResourceData contains unsupported {kind} cache entries; only TextAsset matching is currently wired"
                        )));
                    }
                    return Ok(None);
                }
                let Some(name) = object.get("Name").and_then(Value::as_str) else {
                    return Ok(None);
                };
                let Some(_source_length) = object
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
                    hash,
                    target: PredefinedEngineAssetTarget { file_id, guid },
                });
            }
        }
        Ok(Some(Self { text_assets }))
    }

    /// The frozen cache matches TextAssets by name and MD5; its serialized `Length` field does
    /// not participate in the lookup key. The source-run regression deliberately stores a
    /// seven-byte length for an eight-byte payload and still observes the redirect.
    pub(super) fn text_asset_target(
        &self,
        name: &str,
        bytes: &[u8],
    ) -> Option<PredefinedEngineAssetTarget> {
        let hash = u128::from_le_bytes(crate::export::script_hashing::md5(bytes));
        self.text_assets
            .iter()
            .find(|entry| entry.name == name && entry.hash == hash)
            .map(|entry| entry.target)
    }
}

#[cfg(test)]
mod tests {
    use super::EngineResourceData;

    #[test]
    fn engine_resource_data_textasset_matches_name_hash_and_resource_identity() {
        let data = EngineResourceData::parse(Some(
            r#"{"DefaultResources":{"777":{"$type":"TextAsset","Name":"Hero","Length":7,"Hash":130469715505528193672212848297838865176}},"ExtraResources":{}}"#,
        ))
        .expect("TextAsset resource parses")
        .expect("valid object selects a stored cache");
        let target = data
            .text_asset_target("Hero", b"print(1)")
            .expect("name and UInt128 MD5 identify the source TextAsset");
        assert_eq!(target.file_id, 777);
        assert_eq!(target.guid[8..12], 14u32.to_le_bytes());
        assert!(data.text_asset_target("Hero", b"print(2)").is_none());
        assert!(data.text_asset_target("Other", b"print(1)").is_none());
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
            r#"{"DefaultResources":{"900":{"$type":"TextAsset","Name":"Hero","Length":7,"Hash":130469715505528193672212848297838865176}},"ExtraResources":{"700":{"$type":"TextAsset","Name":"Hero","Length":7,"Hash":130469715505528193672212848297838865176}}}"#,
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
            r#"{"DefaultResources":{},"ExtraResources":{"700":{"$type":"TextAsset","Name":"Hero","Length":7,"Hash":130469715505528193672212848297838865176}}}"#,
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
        assert!(error
            .to_string()
            .contains("only TextAsset matching is currently wired"));
    }
}
