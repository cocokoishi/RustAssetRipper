//! Export-file IDs used by AssetRipper's Unity project collections.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::processing::scene_naming::SceneUnityVersion;

const TEN_TO_THE_FIFTEENTH: i64 = 1_000_000_000_000_000;
const TEN_TO_THE_FIFTH: i64 = 100_000;
const MAX_PREFIXED_CLASS_ID_64: i32 = (i64::MAX / TEN_TO_THE_FIFTEENTH) as i32;
const MAX_PREFIXED_CLASS_ID_32: i32 = i32::MAX / TEN_TO_THE_FIFTH as i32;

/// Error returned when an export ID modifier cannot be represented by the source rules.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExportIdError {
    ModifierOutOfRange { modifier: u32, limit: u32 },
    ModifierForUnprefixedClass { class_id: i32, modifier: u32 },
    ArithmeticOverflow { class_id: i32, modifier: u32 },
}

impl Display for ExportIdError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ModifierOutOfRange { modifier, limit } => write!(
                f,
                "export ID modifier {modifier} is outside the 0..{limit} range"
            ),
            Self::ModifierForUnprefixedClass { class_id, modifier } => write!(
                f,
                "class ID {class_id} uses its class ID as export ID and cannot take modifier {modifier}"
            ),
            Self::ArithmeticOverflow { class_id, modifier } => write!(
                f,
                "export ID arithmetic overflows for class ID {class_id} and modifier {modifier}"
            ),
        }
    }
}

impl Error for ExportIdError {}

/// Return the source `ExportIdHandler.GetMainExportID(classID, value)` result.
pub fn get_main_export_id(class_id: i32, modifier: u32) -> Result<i64, ExportIdError> {
    if class_id > MAX_PREFIXED_CLASS_ID_32 {
        if modifier != 0 {
            return Err(ExportIdError::ModifierForUnprefixedClass { class_id, modifier });
        }
        return Ok(i64::from(class_id));
    }
    if i64::from(modifier) >= TEN_TO_THE_FIFTH {
        return Err(ExportIdError::ModifierOutOfRange {
            modifier,
            limit: TEN_TO_THE_FIFTH as u32,
        });
    }
    i64::from(class_id)
        .checked_mul(TEN_TO_THE_FIFTH)
        .and_then(|prefix| prefix.checked_add(i64::from(modifier)))
        .ok_or(ExportIdError::ArithmeticOverflow { class_id, modifier })
}

/// Return the signed seed-zero-input xxHash32 value used by the baseline.
pub fn get_pseudo_random_value32(seed: i32) -> i32 {
    xxh32_empty(seed as u32) as i32
}

/// Return the signed seed-zero-input xxHash64 value used by the baseline.
pub fn get_pseudo_random_value64(seed: i64) -> i64 {
    xxh64_empty(seed as u64) as i64
}

/// Assign the pseudo-random export ID used by scene/prefab collections.
pub fn get_pseudo_random_export_id(
    class_id: i32,
    seed: i32,
    unity_version: SceneUnityVersion,
) -> i64 {
    if uses_64_bit_export_ids(unity_version) {
        if class_id > MAX_PREFIXED_CLASS_ID_64 {
            return get_pseudo_random_value64(i64::from(seed));
        }
        let prefix = i64::from(class_id).wrapping_mul(TEN_TO_THE_FIFTEENTH);
        let value =
            (get_pseudo_random_value64(i64::from(seed)) as u64) % TEN_TO_THE_FIFTEENTH as u64;
        prefix.wrapping_add(value as i64)
    } else {
        if class_id > MAX_PREFIXED_CLASS_ID_32 {
            return i64::from(get_pseudo_random_value32(seed));
        }
        let prefix = i64::from(class_id).wrapping_mul(TEN_TO_THE_FIFTH);
        let value = (get_pseudo_random_value32(seed) as u32) % TEN_TO_THE_FIFTH as u32;
        prefix.wrapping_add(i64::from(value))
    }
}

fn uses_64_bit_export_ids(version: SceneUnityVersion) -> bool {
    version.major > 5 || (version.major == 5 && version.minor >= 5)
}

fn xxh32_empty(seed: u32) -> u32 {
    let mut hash = seed.wrapping_add(0x1656_67b1);
    hash ^= hash >> 15;
    hash = hash.wrapping_mul(0x85eb_ca77);
    hash ^= hash >> 13;
    hash = hash.wrapping_mul(0xc2b2_ae3d);
    hash ^ (hash >> 16)
}

fn xxh64_empty(seed: u64) -> u64 {
    let mut hash = seed.wrapping_add(0x27d4_eb2f_1656_67c5);
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xc2b2_ae3d_27d4_eb4f);
    hash ^= hash >> 29;
    hash = hash.wrapping_mul(0x1656_67b1_9e37_79f9);
    hash ^ (hash >> 32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn version(value: &str) -> SceneUnityVersion {
        SceneUnityVersion::parse(value).unwrap()
    }

    #[test]
    fn empty_xxhash_values_match_the_published_seed_zero_vectors() {
        assert_eq!(get_pseudo_random_value32(0) as u32, 0x02cc_5d05);
        assert_eq!(get_pseudo_random_value64(0) as u64, 0xef46_db37_51d8_e999);
    }

    #[test]
    fn main_export_ids_use_class_prefixes_and_reject_invalid_modifiers() {
        assert_eq!(get_main_export_id(42, 12_345), Ok(4_212_345));
        assert_eq!(get_main_export_id(100_101, 0), Ok(100_101));
        assert_eq!(
            get_main_export_id(100_101, 1),
            Err(ExportIdError::ModifierForUnprefixedClass {
                class_id: 100_101,
                modifier: 1,
            })
        );
        assert_eq!(
            get_main_export_id(42, 100_000),
            Err(ExportIdError::ModifierOutOfRange {
                modifier: 100_000,
                limit: 100_000,
            })
        );
    }

    #[test]
    fn pseudo_random_export_ids_switch_width_at_unity_55_and_class_thresholds() {
        let modern = version("2022.3.1f1");
        let legacy = version("5.4.7f1");
        let hash64 = get_pseudo_random_value64(0);
        let hash32 = get_pseudo_random_value32(0);

        assert_eq!(get_pseudo_random_export_id(100_101, 0, modern), hash64);
        assert_eq!(
            get_pseudo_random_export_id(100_101, 0, legacy),
            i64::from(hash32)
        );
        assert_eq!(get_pseudo_random_export_id(9_224, 0, modern), hash64);
        assert_eq!(
            get_pseudo_random_export_id(9_223, 0, modern),
            9_223i64
                .wrapping_mul(TEN_TO_THE_FIFTEENTH)
                .wrapping_add((hash64 as u64 % TEN_TO_THE_FIFTEENTH as u64) as i64)
        );
        assert_eq!(
            get_pseudo_random_export_id(21_475, 0, legacy),
            i64::from(hash32)
        );
        assert_eq!(
            get_pseudo_random_export_id(21_474, 0, legacy),
            21_474i64
                .wrapping_mul(TEN_TO_THE_FIFTH)
                .wrapping_add(i64::from((hash32 as u32) % TEN_TO_THE_FIFTH as u32))
        );
        assert_eq!(
            get_pseudo_random_export_id(42, 0, modern),
            42i64
                .wrapping_mul(TEN_TO_THE_FIFTEENTH)
                .wrapping_add((hash64 as u64 % TEN_TO_THE_FIFTEENTH as u64) as i64)
        );
    }

    #[test]
    fn pseudo_random_export_ids_match_frozen_timeline_group_children() {
        // Frozen source `Lost Kitten.playable` child export IDs in document order
        // (after the `11400000` root) from `青岚山河卷` (Unity 2022.3.6f1): the
        // source `AssetsExportCollection.GenerateExportID` assigns
        // `GetPseudoRandomExportId(asset, m_exportIDs.Count)`, i.e. zero-based
        // insertion seeds, not sequential `11400001`-style modifiers.
        let version_2022_3_6 = version("2022.3.6f1");
        for (seed, expected) in [
            (0, 114709254077376921i64),
            (1, 114730242686860875i64),
            (2, 114724804207875695i64),
            (3, 114618682377417806i64),
        ] {
            assert_eq!(
                get_pseudo_random_export_id(114, seed, version_2022_3_6),
                expected,
                "timeline group child seed {seed}"
            );
        }
    }

    #[test]
    fn pseudo_random_values_remain_unique_across_the_baseline_million_seed_probe() {
        let mut values = HashSet::new();
        values.try_reserve(1_000_000).unwrap();
        for seed in 0..1_000_000i32 {
            assert!(values.insert(get_pseudo_random_value64(i64::from(seed))));
        }

        let mut export_ids = HashSet::new();
        export_ids.try_reserve(1_000_000).unwrap();
        for seed in 0..1_000_000i32 {
            assert!(export_ids.insert(get_pseudo_random_export_id(
                42,
                seed,
                version("2022.3.1f1")
            )));
        }
    }
}
