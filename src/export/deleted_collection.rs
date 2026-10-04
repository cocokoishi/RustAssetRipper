//! Deleted-asset collection mechanics ported from `DeletedAssetsExportCollection`.
//!
//! Each deleted-assets collection wraps one `DeletedAssetsInformation` record: it never
//! exports (`Exportable` is false, `Export` and `GetExportID` throw) and exists so references
//! to stripped assets resolve explicitly. Its rules differ deliberately from the other
//! diagnostic families in `diagnostic_pointers.rs`:
//!
//! - Membership covers the information record itself plus its deleted assets in that order:
//!   `Assets` yields every deleted asset first, then the record.
//! - The display name is always the fixed `DeletedAssetsExportCollection` name.
//! - A reference logs a warning naming the deleted asset's best name and the referencing
//!   container file, then resolves to a missing reference built from the asset's class-ID
//!   main export ID (`ids.rs`) and the pointer type from the container's type-stack walk
//!   (`ProjectExporter.ToExportType`), not from any single exporter. Locality plays no
//!   role: deleted pointers never throw for local references.
//!
//! Members are caller-supplied keys; the missing-reference GUID itself lives in the external
//! primitives package and stays outside.

use super::export_type::ExportAssetType;
use super::ids::{get_main_export_id, ExportIdError};

/// The fixed display name of deleted-asset collections.
pub const DELETED_COLLECTION_NAME: &str = "DeletedAssetsExportCollection";

/// One deleted-asset collection: its information record plus deleted members in order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeletedCollection {
    info: usize,
    deleted: Vec<usize>,
}

/// Why a deleted-collection operation fails.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum DeletedCollectionError {
    /// `Export` and `GetExportID` always throw `NotSupportedException`.
    NotSupported,
}

impl core::fmt::Display for DeletedCollectionError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotSupported => write!(formatter, "deleted collections are never exported"),
        }
    }
}

impl std::error::Error for DeletedCollectionError {}

impl DeletedCollection {
    /// Wrap one information record with its deleted members in source order.
    pub fn new(info: usize, deleted: Vec<usize>) -> Self {
        Self { info, deleted }
    }

    /// Mirror `Contains`: the record itself or any deleted member, by identity.
    pub fn contains(&self, asset: usize) -> bool {
        asset == self.info || self.deleted.contains(&asset)
    }

    /// Mirror `Assets`: deleted members first, then the record itself.
    pub fn members(&self) -> impl Iterator<Item = usize> + '_ {
        self.deleted
            .iter()
            .copied()
            .chain(std::iter::once(self.info))
    }

    /// Mirror `Export` and `GetExportID`: always throwing.
    pub fn export_or_id(&self) -> Result<core::convert::Infallible, DeletedCollectionError> {
        Err(DeletedCollectionError::NotSupported)
    }
}

/// Mirror the reference warning: `Deleted asset '{best}' was referenced from '{container}'`.
pub fn deleted_reference_warning(asset_best_name: &str, container_file_name: &str) -> String {
    format!("Deleted asset '{asset_best_name}' was referenced from '{container_file_name}'")
}

/// One resolved deleted pointer: the class main ID plus the stack-walk pointer type.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DeletedPointer {
    pub main_export_id: i64,
    pub asset_type: ExportAssetType,
}

/// Mirror the pointer body after the warning: the class-ID main export ID with the
/// type-stack pointer type.
pub fn deleted_pointer(
    class_id: i32,
    stack_pointer_type: ExportAssetType,
) -> Result<DeletedPointer, ExportIdError> {
    Ok(DeletedPointer {
        main_export_id: get_main_export_id(class_id, 0)?,
        asset_type: stack_pointer_type,
    })
}

#[cfg(test)]
mod tests {
    use super::super::export_type::ExportAssetType;
    use super::{
        deleted_pointer, deleted_reference_warning, DeletedCollection, DeletedCollectionError,
        DeletedPointer, DELETED_COLLECTION_NAME,
    };

    #[test]
    fn membership_covers_record_and_deleted_members_in_order() {
        let collection = DeletedCollection::new(1, [2, 3].into_iter().collect());
        assert!(collection.contains(1));
        assert!(collection.contains(2));
        assert!(collection.contains(3));
        assert!(!collection.contains(4));
        assert_eq!(collection.members().collect::<Vec<_>>(), [2, 3, 1]);
        assert_eq!(DELETED_COLLECTION_NAME, "DeletedAssetsExportCollection");
    }

    #[test]
    fn export_and_ids_always_throw() {
        let collection = DeletedCollection::new(1, Vec::new());
        assert_eq!(
            collection.export_or_id(),
            Err(DeletedCollectionError::NotSupported)
        );
        assert_eq!(collection.members().collect::<Vec<_>>(), [1]);
    }

    #[test]
    fn references_warn_then_resolve_by_class_and_stack_type() {
        assert_eq!(
            deleted_reference_warning("MyMaterial", "level0"),
            "Deleted asset 'MyMaterial' was referenced from 'level0'"
        );
        // The pointer keeps the class main ID with the stack-walk type; locality is
        // irrelevant, so unlike Skip/Fail there is no local-reference error here.
        assert_eq!(
            deleted_pointer(21, ExportAssetType::Serialized),
            Ok(DeletedPointer {
                main_export_id: 2_100_000,
                asset_type: ExportAssetType::Serialized,
            })
        );
        assert_eq!(
            deleted_pointer(28, ExportAssetType::Meta),
            Ok(DeletedPointer {
                main_export_id: 2_800_000,
                asset_type: ExportAssetType::Meta,
            })
        );
    }
}
