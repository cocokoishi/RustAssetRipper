//! Frozen source reference-assembly GUID tables used by MonoScript export.
//!
//! The embedded rows were emitted by `ReferenceAssemblies.GetReferenceAssemblies` for every
//! version boundary in the frozen AssetRipper snapshot with both values of `HasMscorlib2`.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::processing::asset_factory::FactoryUnityVersion;

use super::script_hashing::UnityGuid;

const SOURCE_FIXTURE: &str = include_str!("../processing/reference_assembly_map_fixture.txt");
const MSCORLIB2_SOURCE_FIXTURE: &str =
    include_str!("../processing/reference_assembly_map_mscorlib2_fixture.txt");

/// Frozen source maps keyed by the source Unity version boundaries.
#[derive(Clone, Debug)]
pub struct ReferenceAssemblyMaps {
    versions: Vec<(u64, usize)>,
    maps: BTreeMap<usize, BTreeMap<String, UnityGuid>>,
}

impl ReferenceAssemblyMaps {
    /// Parse the checked-in source-runtime capture.
    pub fn from_source_fixture() -> Result<Self, ReferenceAssemblyMapError> {
        Self::from_source_fixture_with_mscorlib2(false)
    }

    /// Parse the source-runtime capture selected by `IAssemblyManager.HasMscorlib2`.
    pub fn from_source_fixture_with_mscorlib2(
        has_mscorlib2: bool,
    ) -> Result<Self, ReferenceAssemblyMapError> {
        let fixture = if has_mscorlib2 {
            MSCORLIB2_SOURCE_FIXTURE
        } else {
            SOURCE_FIXTURE
        };
        Self::parse_source_fixture(fixture)
    }

    fn parse_source_fixture(fixture: &str) -> Result<Self, ReferenceAssemblyMapError> {
        let mut versions = Vec::new();
        let mut maps = BTreeMap::<usize, BTreeMap<String, UnityGuid>>::new();

        for (line_number, line) in fixture.lines().enumerate() {
            let line = line.trim_end_matches('\r');
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields = line.split('|').collect::<Vec<_>>();
            match fields.as_slice() {
                ["VERSION", version_text, map_id_text] => {
                    let version = FactoryUnityVersion::parse(version_text).ok_or_else(|| {
                        ReferenceAssemblyMapError::new(format!(
                            "reference assembly fixture line {} has an invalid Unity version {version_text:?}",
                            line_number + 1
                        ))
                    })?;
                    let order = version.source_order_bits().ok_or_else(|| {
                        ReferenceAssemblyMapError::new(format!(
                            "reference assembly fixture line {} has an unordered Unity version {version_text:?}",
                            line_number + 1
                        ))
                    })?;
                    let map_id = parse_usize(map_id_text, line_number + 1, "map ID")?;
                    if versions
                        .last()
                        .is_some_and(|(previous, _)| *previous >= order)
                    {
                        return Err(ReferenceAssemblyMapError::new(format!(
                            "reference assembly fixture version rows are not strictly ordered at line {}",
                            line_number + 1
                        )));
                    }
                    versions.push((order, map_id));
                }
                ["ENTRY", map_id_text, assembly, guid_text] => {
                    if assembly.is_empty() {
                        return Err(ReferenceAssemblyMapError::new(format!(
                            "reference assembly fixture line {} has an empty assembly name",
                            line_number + 1
                        )));
                    }
                    let map_id = parse_usize(map_id_text, line_number + 1, "map ID")?;
                    let guid = parse_unity_guid(guid_text).ok_or_else(|| {
                        ReferenceAssemblyMapError::new(format!(
                            "reference assembly fixture line {} has an invalid Unity GUID {guid_text:?}",
                            line_number + 1
                        ))
                    })?;
                    let map = maps.entry(map_id).or_default();
                    if map.insert((*assembly).to_owned(), guid).is_some() {
                        return Err(ReferenceAssemblyMapError::new(format!(
                            "reference assembly fixture repeats assembly {assembly:?} in map {map_id}"
                        )));
                    }
                }
                _ => {
                    return Err(ReferenceAssemblyMapError::new(format!(
                        "reference assembly fixture line {} has an invalid row shape",
                        line_number + 1
                    )))
                }
            }
        }

        if versions.is_empty() || maps.is_empty() {
            return Err(ReferenceAssemblyMapError::new(
                "reference assembly fixture contains no version rows or maps",
            ));
        }
        for (_, map_id) in &versions {
            if !maps.contains_key(map_id) {
                return Err(ReferenceAssemblyMapError::new(format!(
                    "reference assembly version references missing map {map_id}"
                )));
            }
        }
        if maps.keys().any(|map_id| {
            !versions
                .iter()
                .any(|(_, version_map_id)| version_map_id == map_id)
        }) {
            return Err(ReferenceAssemblyMapError::new(
                "reference assembly fixture contains an unreferenced map",
            ));
        }

        Ok(Self { versions, maps })
    }

    /// Resolve the source reference GUID for an assembly at a Unity version.
    ///
    /// AssetRipper clamps versions older than the first data row to the first map and versions
    /// newer than the last row to the last map, then selects the last boundary at or below the
    /// requested version.
    pub fn guid_for(
        &self,
        version: &FactoryUnityVersion,
        assembly: &str,
    ) -> Result<Option<UnityGuid>, ReferenceAssemblyMapError> {
        let requested = version.source_order_bits().ok_or_else(|| {
            ReferenceAssemblyMapError::new(format!(
                "Unity version {:?} cannot be ordered for reference assembly lookup",
                version.text
            ))
        })?;
        let selected = self
            .versions
            .partition_point(|(boundary, _)| *boundary <= requested)
            .saturating_sub(1);
        let map_id = self.versions[selected].1;
        Ok(self.maps[&map_id].get(assembly).copied())
    }

    /// Number of source version boundaries represented by this table.
    pub fn version_count(&self) -> usize {
        self.versions.len()
    }

    /// Number of deduplicated source maps represented by this table.
    pub fn map_count(&self) -> usize {
        self.maps.len()
    }
}

fn parse_usize(
    value: &str,
    line_number: usize,
    label: &str,
) -> Result<usize, ReferenceAssemblyMapError> {
    value.parse::<usize>().map_err(|_| {
        ReferenceAssemblyMapError::new(format!(
            "reference assembly fixture line {line_number} has an invalid {label} {value:?}"
        ))
    })
}

/// Parse the source `UnityGuid.ToString` representation, whose nibbles are low-to-high per
/// 32-bit word, into the same words used by `UnityGuid::to_byte_array`.
fn parse_unity_guid(value: &str) -> Option<UnityGuid> {
    if value.len() != 32 || !value.is_ascii() {
        return None;
    }
    let mut words = [0u32; 4];
    for (index, byte) in value.bytes().enumerate() {
        let digit = match byte {
            b'0'..=b'9' => u32::from(byte - b'0'),
            b'a'..=b'f' => u32::from(byte - b'a' + 10),
            b'A'..=b'F' => u32::from(byte - b'A' + 10),
            _ => return None,
        };
        let word_index = index / 8;
        let nibble_shift = (index % 8) * 4;
        words[word_index] |= digit << nibble_shift;
    }
    Some(UnityGuid::new(words[0], words[1], words[2], words[3]))
}

/// Malformed or incomplete frozen source reference-assembly data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferenceAssemblyMapError(String);

impl ReferenceAssemblyMapError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl Display for ReferenceAssemblyMapError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for ReferenceAssemblyMapError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_reference_map_selects_versioned_guids_and_clamps_boundaries() {
        let maps = ReferenceAssemblyMaps::from_source_fixture().unwrap();
        assert_eq!(maps.version_count(), 111);
        assert_eq!(maps.map_count(), 83);

        let engine_guid = UnityGuid::new(0x1F55_507F, 0xA194_8D44, 0x4080_F528, 0xC176_C90E);
        for version_text in ["2019.4.0f1", "2019.4.40f1", "2022.3.22f1"] {
            let version = FactoryUnityVersion::parse(version_text).unwrap();
            assert_eq!(
                maps.guid_for(&version, "UnityEngine").unwrap(),
                Some(engine_guid)
            );
        }

        let before_core_module = FactoryUnityVersion::parse("2017.1.0b1").unwrap();
        let after_core_module = FactoryUnityVersion::parse("2017.2.0b2").unwrap();
        assert_eq!(
            maps.guid_for(&before_core_module, "UnityEngine.CoreModule")
                .unwrap(),
            None
        );
        assert_eq!(
            maps.guid_for(&after_core_module, "UnityEngine.CoreModule")
                .unwrap(),
            Some(engine_guid)
        );
        assert_eq!(
            maps.guid_for(&before_core_module, "UnityEngine.UI")
                .unwrap()
                .unwrap()
                .to_unity_string(),
            "f5f67c52d1564df4a8936ccd202a3bd8"
        );

        let before = FactoryUnityVersion::parse("0.0.0f0").unwrap();
        assert!(maps.guid_for(&before, "UnityEngine").unwrap().is_some());
        assert_eq!(maps.guid_for(&before, "NoSuchAssembly").unwrap(), None);
        let unordered = FactoryUnityVersion::parse("2019.4.40z0").unwrap();
        assert!(maps.guid_for(&unordered, "UnityEngine").is_err());
    }

    #[test]
    fn source_reference_map_uses_the_mscorlib_two_assembly_list() {
        let mscorlib_four =
            ReferenceAssemblyMaps::from_source_fixture_with_mscorlib2(false).unwrap();
        let mscorlib_two = ReferenceAssemblyMaps::from_source_fixture_with_mscorlib2(true).unwrap();
        assert_eq!(mscorlib_two.version_count(), 111);
        assert_eq!(mscorlib_two.map_count(), 69);

        let version = FactoryUnityVersion::parse("2019.1.0f1").unwrap();
        assert_eq!(
            mscorlib_four.guid_for(&version, "Mono.Cecil").unwrap(),
            None
        );
        assert_eq!(
            mscorlib_two
                .guid_for(&version, "Mono.Cecil")
                .unwrap()
                .unwrap()
                .to_unity_string(),
            "f70555f144d8491a825f0804e09c671c"
        );

        let engine_guid = UnityGuid::new(0x1F55_507F, 0xA194_8D44, 0x4080_F528, 0xC176_C90E);
        assert_eq!(
            mscorlib_two.guid_for(&version, "UnityEngine").unwrap(),
            Some(engine_guid)
        );
    }

    #[test]
    fn mscorlib_two_reference_map_replays_every_source_version_boundary() {
        let maps = ReferenceAssemblyMaps::from_source_fixture_with_mscorlib2(true).unwrap();
        let all_assemblies = maps
            .maps
            .values()
            .flat_map(|map| map.keys().cloned())
            .collect::<std::collections::BTreeSet<_>>();
        let mut checked_versions = 0;

        for line in MSCORLIB2_SOURCE_FIXTURE.lines() {
            let Some(row) = line.strip_prefix("VERSION|") else {
                continue;
            };
            let (version_text, map_id_text) = row.split_once('|').unwrap();
            let version = FactoryUnityVersion::parse(version_text).unwrap();
            let map_id = map_id_text.parse::<usize>().unwrap();
            let expected = maps.maps.get(&map_id).unwrap();
            for assembly in &all_assemblies {
                assert_eq!(
                    maps.guid_for(&version, assembly).unwrap(),
                    expected.get(assembly).copied(),
                    "source map mismatch for Unity {version_text} assembly {assembly}"
                );
            }
            checked_versions += 1;
        }

        assert_eq!(checked_versions, 111);
    }

    #[test]
    fn source_unity_guid_parser_preserves_word_nibble_order() {
        let guid = parse_unity_guid("f70555f144d8491a825f0804e09c671c").unwrap();
        assert_eq!(
            guid.words(),
            (0x1F55_507F, 0xA194_8D44, 0x4080_F528, 0xC176_C90E)
        );
        assert_eq!(guid.to_unity_string(), "f70555f144d8491a825f0804e09c671c");
        assert!(parse_unity_guid("f70555f144d8491a825f0804e09c671z").is_none());
    }
}
