//! Multi-asset collection mechanics ported from `AssetsExportCollection<T>`.
//!
//! Single-asset collections export one asset; multi-asset collections gather extra members
//! around a primary asset and export the whole `ExportableAssets` enumerable through one file:
//!
//! - `AddAsset` ignores null members and the primary asset itself, assigns the newcomer an ID
//!   with `GenerateExportID`, and keeps the first registration on duplicates (`TryAdd`).
//!   `AddAssets` applies the same rule element-wise, so null entries are skipped safely.
//! - The default `GenerateExportID` is `GetPseudoRandomExportId(asset, m_exportIDs.Count)`:
//!   the insertion count (extra members only) seeds the ID, so the first added member uses
//!   seed 0. `TextureExportCollection` instead uses `GetMainExportID(asset, m_nextExportID)`
//!   with a modifier starting at 0 and stepping by 2 per added member; font members use the
//!   plain main export ID.
//! - `Contains` matches the primary asset or any registered member; `GetExportID` answers the
//!   primary ID for the primary asset and the stored ID otherwise, failing for strangers.
//! - `Assets` yields the primary asset first, then registered members in insertion order, and
//!   `ExportInner` hands the `ExportableAssets` enumerable (not a single asset) to the
//!   exporter. The `File` property tracks the most recently enumerated member's collection as
//!   a side effect of enumeration; that cursor behavior is recorded here but not modeled.
//!
//! Members are caller-supplied keys; ID arithmetic reuses `ids.rs` exactly.

use super::ids::{get_main_export_id, get_pseudo_random_export_id, ExportIdError};
use crate::processing::scene_naming::SceneUnityVersion;

/// How a multi-asset collection assigns IDs to added members.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemberIdScheme {
    /// `GetPseudoRandomExportId(asset, insertion count)`.
    PseudoRandom {
        class_id: i32,
        unity_version: SceneUnityVersion,
    },
    /// `GetMainExportID(asset, running modifier)` starting at 0 and stepping by `stride`
    /// per added member (the texture collection uses stride 2).
    MainStride { class_id: i32, stride: u32 },
    /// The plain main export ID with modifier 0 (the font collection rule).
    MainPlain { class_id: i32 },
}

/// Why a member ID cannot be assigned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MultiAssetError {
    ExportId(ExportIdError),
    MemberCountOverflow,
}

impl core::fmt::Display for MultiAssetError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ExportId(source) => write!(formatter, "cannot assign member export ID: {source}"),
            Self::MemberCountOverflow => write!(formatter, "member count overflows the ID seed"),
        }
    }
}

impl std::error::Error for MultiAssetError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::ExportId(source) => Some(source),
            Self::MemberCountOverflow => None,
        }
    }
}

impl From<ExportIdError> for MultiAssetError {
    fn from(source: ExportIdError) -> Self {
        Self::ExportId(source)
    }
}

/// An insertion-ordered multi-asset collection around one primary member.
#[derive(Clone, Debug, PartialEq)]
pub struct MultiAssetCollection {
    primary: usize,
    primary_export_id: i64,
    members: Vec<(usize, i64)>,
    next_modifier: u32,
}

impl MultiAssetCollection {
    /// Start a collection whose primary member already owns `primary_export_id`.
    pub fn new(primary: usize, primary_export_id: i64) -> Self {
        Self {
            primary,
            primary_export_id,
            members: Vec::new(),
            next_modifier: 0,
        }
    }

    /// Mirror `AddAsset`: the primary member and duplicates are refused without an ID.
    ///
    /// Returns `false` (not an error) exactly when the source returns false.
    pub fn add(&mut self, member: usize, scheme: MemberIdScheme) -> Result<bool, MultiAssetError> {
        if member == self.primary || self.members.iter().any(|(key, _)| *key == member) {
            return Ok(false);
        }
        let export_id = match scheme {
            MemberIdScheme::PseudoRandom {
                class_id,
                unity_version,
            } => {
                let seed = i32::try_from(self.members.len())
                    .map_err(|_| MultiAssetError::MemberCountOverflow)?;
                get_pseudo_random_export_id(class_id, seed, unity_version)
            }
            MemberIdScheme::MainStride { class_id, stride } => {
                let export_id = get_main_export_id(class_id, self.next_modifier)?;
                self.next_modifier = self
                    .next_modifier
                    .checked_add(stride)
                    .ok_or(MultiAssetError::MemberCountOverflow)?;
                export_id
            }
            MemberIdScheme::MainPlain { class_id } => get_main_export_id(class_id, 0)?,
        };
        self.members.push((member, export_id));
        Ok(true)
    }

    /// Mirror `Contains`: the primary member or any registered member.
    pub fn contains(&self, member: usize) -> bool {
        member == self.primary || self.members.iter().any(|(key, _)| *key == member)
    }

    /// Mirror `GetExportID`: the primary ID, a stored member ID, or `None` for strangers
    /// (the source `KeyNotFoundException` path).
    pub fn export_id(&self, member: usize) -> Option<i64> {
        if member == self.primary {
            return Some(self.primary_export_id);
        }
        self.members
            .iter()
            .find(|(key, _)| *key == member)
            .map(|(_, id)| *id)
    }

    /// Registered members in insertion order (the tail of the `Assets` enumeration).
    pub fn extra_members(&self) -> impl Iterator<Item = usize> + '_ {
        self.members.iter().map(|(key, _)| *key)
    }
}

#[cfg(test)]
mod tests {
    use super::{MemberIdScheme, MultiAssetCollection};
    use crate::processing::scene_naming::SceneUnityVersion;

    fn modern() -> SceneUnityVersion {
        SceneUnityVersion::parse("2022.3.1f1").unwrap()
    }

    #[test]
    fn add_refuses_primary_and_duplicates_with_count_seeded_ids() {
        let mut collection = MultiAssetCollection::new(7, 700_000);
        let scheme = MemberIdScheme::PseudoRandom {
            class_id: 28,
            unity_version: modern(),
        };
        // The primary asset itself is refused.
        assert_eq!(collection.add(7, scheme), Ok(false));
        // The first added member uses seed 0, the second seed 1.
        assert_eq!(collection.add(11, scheme), Ok(true));
        assert_eq!(collection.add(13, scheme), Ok(true));
        assert_eq!(
            collection.export_id(11),
            Some(super::super::ids::get_pseudo_random_export_id(
                28,
                0,
                modern()
            ))
        );
        assert_eq!(
            collection.export_id(13),
            Some(super::super::ids::get_pseudo_random_export_id(
                28,
                1,
                modern()
            ))
        );
        // Duplicates keep the first registration.
        assert_eq!(collection.add(11, scheme), Ok(false));
        assert_eq!(collection.extra_members().collect::<Vec<_>>(), [11, 13]);
    }

    #[test]
    fn contains_and_export_ids_cover_primary_members_and_strangers() {
        let mut collection = MultiAssetCollection::new(7, 700_000);
        let scheme = MemberIdScheme::PseudoRandom {
            class_id: 28,
            unity_version: modern(),
        };
        assert!(collection.contains(7));
        assert!(!collection.contains(11));
        assert_eq!(collection.export_id(7), Some(700_000));
        assert_eq!(collection.export_id(11), None);
        collection.add(11, scheme).unwrap();
        assert!(collection.contains(11));
        assert_eq!(
            collection.export_id(11),
            Some(super::super::ids::get_pseudo_random_export_id(
                28,
                0,
                modern()
            ))
        );
        assert_eq!(collection.export_id(99), None);
    }

    #[test]
    fn texture_stride_scheme_steps_the_modifier_by_two() {
        let mut collection = MultiAssetCollection::new(7, 2_800_000);
        let scheme = MemberIdScheme::MainStride {
            class_id: 213,
            stride: 2,
        };
        assert_eq!(collection.add(11, scheme), Ok(true));
        assert_eq!(collection.add(13, scheme), Ok(true));
        assert_eq!(collection.export_id(11), Some(21_300_000));
        assert_eq!(collection.export_id(13), Some(21_300_002));
        assert_eq!(collection.extra_members().collect::<Vec<_>>(), [11, 13]);
    }

    #[test]
    fn font_plain_scheme_uses_unmodified_main_ids() {
        let mut collection = MultiAssetCollection::new(7, 2_800_000);
        let scheme = MemberIdScheme::MainPlain { class_id: 128 };
        assert_eq!(collection.add(11, scheme), Ok(true));
        assert_eq!(collection.add(13, scheme), Ok(true));
        assert_eq!(collection.export_id(11), Some(12_800_000));
        assert_eq!(collection.export_id(13), Some(12_800_000));
    }
}
