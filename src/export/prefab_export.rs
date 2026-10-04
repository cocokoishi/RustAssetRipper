//! Prefab collection export rules ported from `PrefabExportCollection`.
//!
//! A prefab collection gathers the hierarchy's assets plus the hierarchy object itself, in
//! that order, through the shared multi-asset mechanics (`multi_asset.rs`): the primary
//! asset is the hierarchy's prefab, and every gathered member takes the default
//! count-seeded pseudo-random ID. The gathered set and the exported set differ on purpose:
//! `ExportableAssets` yields the hierarchy's own exportable list in order (not the
//! collected extras), tracking each member's file as it goes, while the display name is the
//! root GameObject's name.
//!
//! Prefab pointers (`GenerateMetaPtrForPrefab`) combine the `PrefabInstance` main export ID
//! with the collection GUID and the legacy/modern asset type (`export_type.rs`): a legacy
//! prefab marker points at `Serialized`, a modern instance at `Meta`. That class ID lives
//! in the external generated package, so callers supply it. Collection flags OR the
//! prefab-system bit (`transfer_flags.rs`).
//!
//! Which hierarchy members are exportable lives in the processing object model and arrives
//! here as an ordered caller-supplied list.

use super::export_type::{prefab_pointer_type, ExportAssetType};
use super::ids::{get_main_export_id, ExportIdError};

/// One prefab pointer: the class main ID, the collection GUID words, and the asset type.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PrefabInstancePointer {
    pub file_id: i64,
    pub guid: [u32; 4],
    pub asset_type: ExportAssetType,
}

/// Mirror `GenerateMetaPtrForPrefab` for one legacy-marker flag and collection GUID.
pub fn prefab_instance_pointer(
    prefab_instance_class_id: i32,
    collection_guid: [u32; 4],
    emit_prefab_asset: bool,
) -> Result<PrefabInstancePointer, ExportIdError> {
    Ok(PrefabInstancePointer {
        file_id: get_main_export_id(prefab_instance_class_id, 0)?,
        guid: collection_guid,
        asset_type: prefab_pointer_type(emit_prefab_asset),
    })
}

/// Mirror the constructor gathering order: hierarchy assets first, then the hierarchy
/// object itself, each deduplicated by the multi-asset rules.
///
/// Returns the ordered add sequence; callers feed it to `MultiAssetCollection::add` with
/// the pseudo-random scheme so count-seeded IDs match the source.
pub fn prefab_gathering_order(hierarchy_assets: Vec<usize>, hierarchy_object: usize) -> Vec<usize> {
    let mut ordered = Vec::new();
    for member in hierarchy_assets
        .into_iter()
        .chain(std::iter::once(hierarchy_object))
    {
        if !ordered.contains(&member) {
            ordered.push(member);
        }
    }
    ordered
}

/// Mirror `ExportableAssets`: the hierarchy's exportable members in order with their files.
///
/// The collected extras never appear here; the file cursor simply follows each member.
pub fn prefab_exportable_with_files(
    hierarchy_exportable: Vec<(usize, usize)>,
) -> Vec<(usize, usize)> {
    hierarchy_exportable
}

#[cfg(test)]
mod tests {
    use super::super::export_type::ExportAssetType;
    use super::{prefab_exportable_with_files, prefab_gathering_order, prefab_instance_pointer};

    #[test]
    fn pointers_combine_class_ids_guids_and_legacy_branches() {
        let legacy = prefab_instance_pointer(1001, [1, 2, 3, 4], true).unwrap();
        assert_eq!(legacy.file_id, 100_100_000);
        assert_eq!(legacy.guid, [1, 2, 3, 4]);
        assert_eq!(legacy.asset_type, ExportAssetType::Serialized);
        let modern = prefab_instance_pointer(1001, [1, 2, 3, 4], false).unwrap();
        assert_eq!(modern.file_id, 100_100_000);
        assert_eq!(modern.asset_type, ExportAssetType::Meta);
    }

    #[test]
    fn gathering_orders_hierarchy_assets_before_the_hierarchy_itself() {
        assert_eq!(
            prefab_gathering_order([10, 11].into_iter().collect(), 12),
            [10, 11, 12]
        );
        // An already-listed hierarchy object is not appended twice, and member
        // duplicates keep their first position like TryAdd.
        assert_eq!(
            prefab_gathering_order([10, 12].into_iter().collect(), 12),
            [10, 12]
        );
        assert_eq!(
            prefab_gathering_order([10, 11, 10].into_iter().collect(), 12),
            [10, 11, 12]
        );
        assert!(prefab_gathering_order(Vec::new(), 12) == [12]);
    }

    #[test]
    fn exportable_sets_follow_hierarchy_order_with_files() {
        let members = [(10, 100), (11, 100), (12, 101)];
        assert_eq!(
            prefab_exportable_with_files(members.into_iter().collect()),
            [(10, 100), (11, 100), (12, 101)]
        );
        assert!(prefab_exportable_with_files(Vec::new()).is_empty());
    }
}
