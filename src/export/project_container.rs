//! Project export container mappings ported from `ProjectAssetContainer`.
//!
//! The container sits between the exporter stack and the collections during
//! `ProjectExporter.Export`: it remembers which collection owns each asset, tracks the
//! collection currently being exported, and resolves IDs, pointers, and scene GUIDs:
//!
//! - Construction records every collection's assets in order and rejects an asset claimed
//!   by two collections. The source performs that duplicate check only in `DEBUG` builds;
//!   this port always checks, which is strictly stricter and can only turn silent
//!   misrouting into an explicit error.
//! - `GetExportID` answers the owning collection's ID for known assets and the main export
//!   ID (`ids.rs`) for strangers.
//! - `CreateExportPointer` delegates to the owning collection with locality set exactly when
//!   it is the current collection; assets without a collection become missing references
//!   with an explicit `Meta` type (see `export_type.rs`).
//! - `ScenePathToGUID` returns the first scene whose path matches, or the zero GUID when no
//!   scene matches. Duplicate-index checks reuse `scene_helpers::is_scene_duplicate` with the
//!   container's optional settings list.
//!
//! Assets, collections, and scenes are caller-supplied keys; GUIDs flow as words matching
//! the `format_unity_guid_words` convention. Versions, YAML, and file writes stay outside.

/// A zero scene GUID: no scene matched the requested path.
pub const ZERO_SCENE_GUID: [u32; 4] = [0, 0, 0, 0];

/// An asset claimed by two collections during container construction.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct DuplicateAssetError {
    /// The twice-claimed asset key.
    pub asset: usize,
}

impl core::fmt::Display for DuplicateAssetError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            formatter,
            "asset {} is already added by another collection",
            self.asset
        )
    }
}

impl std::error::Error for DuplicateAssetError {}

/// The asset-to-collection map plus scene GUIDs behind one project export.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProjectContainerMappings {
    asset_collections: Vec<(usize, usize)>,
    scene_guids: Vec<(String, [u32; 4])>,
    current_collection: Option<usize>,
}

impl ProjectContainerMappings {
    /// An empty container with no current collection.
    pub const fn new() -> Self {
        Self {
            asset_collections: Vec::new(),
            scene_guids: Vec::new(),
            current_collection: None,
        }
    }

    /// Record one collection's ownership of an asset, refusing double claims.
    pub fn add_asset_mapping(
        &mut self,
        asset: usize,
        collection: usize,
    ) -> Result<(), DuplicateAssetError> {
        if self
            .asset_collections
            .iter()
            .any(|(known, _)| *known == asset)
        {
            return Err(DuplicateAssetError { asset });
        }
        self.asset_collections.push((asset, collection));
        Ok(())
    }

    /// Record one scene path/GUID pair in scene-list order.
    pub fn add_scene_guid(&mut self, path: String, guid: [u32; 4]) {
        self.scene_guids.push((path, guid));
    }

    /// Select the collection currently being exported.
    pub fn set_current_collection(&mut self, collection: usize) {
        self.current_collection = Some(collection);
    }

    /// The owning collection of an asset, if construction recorded one.
    pub fn collection_for(&self, asset: usize) -> Option<usize> {
        self.asset_collections
            .iter()
            .find(|(known, _)| *known == asset)
            .map(|(_, collection)| *collection)
    }

    /// Mirror `CreateExportPointer` locality: true exactly for the current collection.
    pub fn is_current_collection(&self, collection: usize) -> bool {
        self.current_collection == Some(collection)
    }

    /// Mirror `ScenePathToGUID`: the first matching scene's GUID, else the zero GUID.
    pub fn scene_path_to_guid(&self, path: &str) -> [u32; 4] {
        self.scene_guids
            .iter()
            .find(|(known, _)| known == path)
            .map(|(_, guid)| *guid)
            .unwrap_or(ZERO_SCENE_GUID)
    }
}

/// Mirror `GetExportID`: the owning collection's ID when known, else the main export ID
/// fallback for strangers.
pub const fn resolve_container_export_id(
    collection_export_id: Option<i64>,
    main_export_id: i64,
) -> i64 {
    match collection_export_id {
        Some(id) => id,
        None => main_export_id,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        resolve_container_export_id, DuplicateAssetError, ProjectContainerMappings, ZERO_SCENE_GUID,
    };

    #[test]
    fn construction_refuses_double_claims() {
        let mut mappings = ProjectContainerMappings::new();
        assert_eq!(mappings.add_asset_mapping(1, 10), Ok(()));
        assert_eq!(mappings.add_asset_mapping(2, 10), Ok(()));
        assert_eq!(
            mappings.add_asset_mapping(1, 11),
            Err(DuplicateAssetError { asset: 1 })
        );
        assert_eq!(mappings.collection_for(1), Some(10));
        assert_eq!(mappings.collection_for(2), Some(10));
        assert_eq!(mappings.collection_for(9), None);
    }

    #[test]
    fn export_ids_prefer_the_collection_then_fall_back() {
        assert_eq!(
            resolve_container_export_id(Some(2_100_000), 100_000),
            2_100_000
        );
        assert_eq!(resolve_container_export_id(None, 100_000), 100_000);
    }

    #[test]
    fn locality_follows_the_current_collection() {
        let mut mappings = ProjectContainerMappings::new();
        assert!(!mappings.is_current_collection(10));
        mappings.set_current_collection(10);
        assert!(mappings.is_current_collection(10));
        assert!(!mappings.is_current_collection(11));
        mappings.set_current_collection(11);
        assert!(!mappings.is_current_collection(10));
        assert!(mappings.is_current_collection(11));
    }

    #[test]
    fn scene_paths_resolve_first_match_or_zero() {
        let mut mappings = ProjectContainerMappings::new();
        assert_eq!(mappings.scene_path_to_guid("Assets/A"), ZERO_SCENE_GUID);
        mappings.add_scene_guid("Assets/A".to_owned(), [1, 2, 3, 4]);
        mappings.add_scene_guid("Assets/A".to_owned(), [5, 6, 7, 8]);
        mappings.add_scene_guid("Assets/B".to_owned(), [9, 9, 9, 9]);
        assert_eq!(mappings.scene_path_to_guid("Assets/A"), [1, 2, 3, 4]);
        assert_eq!(mappings.scene_path_to_guid("Assets/B"), [9, 9, 9, 9]);
        assert_eq!(
            mappings.scene_path_to_guid("Assets/Missing"),
            ZERO_SCENE_GUID
        );
    }
}
