//! `PPtrExtensions`.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/PPtrExtensions.cs`. A pointer resolves to an
//! asset when the path ids match and the collection matches: directly when the file id is zero, or
//! through the file's dependency table otherwise.

/// A pointer view.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PPtrView {
    /// `PathID`.
    pub path_id: i64,
    /// `FileID`.
    pub file_id: i64,
}

/// An asset identity: its collection and path id.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AssetRef {
    /// The owning collection.
    pub collection: usize,
    /// The asset's path id.
    pub path_id: i64,
}

/// `PPtrExtensions.IsAsset`.
pub fn is_asset(pptr: PPtrView, file: usize, dependencies: &[usize], asset: AssetRef) -> bool {
    if asset.path_id != pptr.path_id {
        false
    } else if pptr.file_id == 0 {
        file == asset.collection
    } else {
        usize::try_from(pptr.file_id - 1)
            .ok()
            .and_then(|index| dependencies.get(index))
            .is_some_and(|dependency| *dependency == asset.collection)
    }
}

/// `PPtrExtensions.IsNull`.
pub fn is_null(pptr: PPtrView) -> bool {
    pptr.path_id == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_asset_matches_the_reference_semantics() {
        let asset = AssetRef {
            collection: 1,
            path_id: 5,
        };
        // Zero file id compares the collection directly.
        assert!(is_asset(
            PPtrView {
                path_id: 5,
                file_id: 0
            },
            1,
            &[],
            asset
        ));
        assert!(!is_asset(
            PPtrView {
                path_id: 5,
                file_id: 0
            },
            2,
            &[],
            asset
        ));
        // A mismatched path id fails regardless of collection.
        assert!(!is_asset(
            PPtrView {
                path_id: 6,
                file_id: 0
            },
            1,
            &[],
            asset
        ));
        // A non-zero file id indexes the dependency table.
        assert!(is_asset(
            PPtrView {
                path_id: 5,
                file_id: 1
            },
            2,
            &[1],
            asset
        ));
        assert!(!is_asset(
            PPtrView {
                path_id: 5,
                file_id: 1
            },
            2,
            &[3],
            asset
        ));
        assert!(!is_asset(
            PPtrView {
                path_id: 5,
                file_id: 2
            },
            2,
            &[1],
            asset
        ));
        // Nullness is path id zero.
        assert!(is_null(PPtrView {
            path_id: 0,
            file_id: 0
        }));
        assert!(!is_null(PPtrView {
            path_id: 5,
            file_id: 0
        }));
    }
}
