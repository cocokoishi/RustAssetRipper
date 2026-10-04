//! Scene YAML member ordering ported from `SceneExportCollection.Compare`.
//!
//! The scene collection constructor snapshots its display order up front:
//! `hierarchy.ExportableAssets.Order(this).ToArray()`. LINQ `Order` is a stable sort, so members
//! the comparator calls equal keep their `ExportableAssets` relative order. The comparator
//! itself only looks at two facts per member:
//!
//! - Equal class IDs compare equal, whatever the manager flags say.
//! - Two level-game managers order by class ID.
//! - A manager sorts before any non-manager.
//! - Two non-managers compare equal, preserving their relative order.
//!
//! Null members never reach the snapshot in practice (`ExportableAssets` yields hierarchy
//! members), so this port takes plain class IDs and manager flags; the source null handling
//! (`null` is never a manager, two nulls are equal) falls out of the same arms.
//!
//! The workspace scene planner (`scene_collection.rs`) deliberately assembles members without
//! sorting; feed its output through a stable sort with this comparator to obtain the display
//! order the baseline writes.

use core::cmp::Ordering;

/// Mirror `SceneExportCollection.Compare` for one member pair.
///
/// `class_id` is the member's `ClassID`; `is_manager` is whether it is an
/// `ILevelGameManager`.
pub const fn compare_scene_export_members(
    a_class_id: i32,
    a_is_manager: bool,
    b_class_id: i32,
    b_is_manager: bool,
) -> Ordering {
    if a_class_id == b_class_id {
        return Ordering::Equal;
    }
    match (a_is_manager, b_is_manager) {
        (true, true) => {
            if a_class_id < b_class_id {
                Ordering::Less
            } else {
                Ordering::Greater
            }
        }
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (false, false) => Ordering::Equal,
    }
}

/// Sort scene members into baseline display order.
///
/// The sort must be stable (`slice::sort_by`, never `sort_unstable_by`): the baseline uses
/// LINQ `Order`, and every `Equal` arm above relies on stability to preserve the
/// `ExportableAssets` relative order.
pub fn sort_scene_export_members(members: &mut [(i32, bool)]) {
    members.sort_by(|a, b| compare_scene_export_members(a.0, a.1, b.0, b.1));
}

#[cfg(test)]
mod tests {
    use super::{compare_scene_export_members, sort_scene_export_members};
    use core::cmp::Ordering;

    #[test]
    fn comparator_follows_class_then_manager_rules() {
        use Ordering::{Equal, Greater, Less};
        // Equal class IDs compare equal even across the manager flag.
        assert_eq!(compare_scene_export_members(29, true, 29, false), Equal);
        assert_eq!(
            compare_scene_export_members(1045, false, 1045, false),
            Equal
        );
        // Two managers order by class ID.
        assert_eq!(compare_scene_export_members(29, true, 1045, true), Less);
        assert_eq!(compare_scene_export_members(1045, true, 29, true), Greater);
        // A manager sorts before any non-manager of a different class.
        assert_eq!(compare_scene_export_members(1045, true, 1, false), Less);
        assert_eq!(compare_scene_export_members(1, false, 29, true), Greater);
        // Two non-managers of different classes compare equal.
        assert_eq!(compare_scene_export_members(1, false, 4, false), Equal);
    }

    #[test]
    fn stable_sort_keeps_exportable_order_for_equal_members() {
        // (class ID, is manager): managers must come first by class, everything else must
        // keep its ExportableAssets relative order.
        let mut members = [
            (4, false),
            (1045, true),
            (1, false),
            (29, true),
            (33, false),
            (4, false),
        ];
        sort_scene_export_members(&mut members);
        assert_eq!(
            members,
            [
                (29, true),
                (1045, true),
                (4, false),
                (1, false),
                (33, false),
                (4, false),
            ]
        );
    }
}
