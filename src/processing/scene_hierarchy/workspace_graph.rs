use super::*;

impl WorkspaceSceneHierarchies {
    pub fn scenes(&self) -> &[WorkspaceSceneHierarchyScene] {
        &self.scenes
    }

    pub fn non_scene_collections(&self) -> &[WorkspaceSceneCollectionHierarchy] {
        &self.non_scene_collections
    }

    pub(super) fn collection_graph_for_file(
        &self,
        file_index: usize,
    ) -> Option<&WorkspaceSceneCollectionHierarchy> {
        let index = self
            .collection_graph_index
            .binary_search_by_key(&file_index, |entry| entry.file_index)
            .ok()?;
        let entry = self.collection_graph_index.get(index)?;
        match entry.scene_index {
            Some(scene_index) => self
                .scenes
                .get(scene_index)?
                .collections()
                .get(entry.collection_index),
            None => self.non_scene_collections.get(entry.collection_index),
        }
    }

    /// Returns PrefabInstance identities in workspace file-index and serialized object order.
    pub fn prefab_instance_identities(
        &self,
    ) -> impl Iterator<Item = WorkspaceSceneObjectIdentity> + '_ {
        self.collection_graph_index.iter().flat_map(move |entry| {
            self.collection_graph_for_file(entry.file_index)
                .into_iter()
                .flat_map(|graph| graph.scene_assets.iter())
                .filter_map(|asset| {
                    (asset.category == WorkspaceSceneAssetCategory::PrefabInstance)
                        .then_some(asset.identity)
                })
        })
    }

    /// Returns recognized scene asset identities in caller-supplied collection traversal order.
    /// Assets inside each collection retain serialized object-table order.
    pub fn scene_asset_identities_in_collection_order<'a>(
        &'a self,
        collection_file_indices: &'a [usize],
    ) -> impl Iterator<Item = WorkspaceSceneAssetIdentity> + 'a {
        collection_file_indices.iter().flat_map(move |&file_index| {
            self.collection_graph_for_file(file_index)
                .into_iter()
                .flat_map(|graph| graph.scene_assets.iter().copied())
        })
    }

    /// Returns PrefabInstance identities in the supplied source collection traversal order.
    ///
    /// Each collection's assets retain serialized object-table order. The caller supplies the
    /// `GameBundle.FetchAssets()` collection order; workspace file indices alone do not encode
    /// that order for nested bundles.
    pub fn prefab_instance_identities_in_collection_order<'a>(
        &'a self,
        collection_file_indices: &'a [usize],
    ) -> impl Iterator<Item = WorkspaceSceneObjectIdentity> + 'a {
        self.scene_asset_identities_in_collection_order(collection_file_indices)
            .filter_map(|asset| {
                (asset.category == WorkspaceSceneAssetCategory::PrefabInstance)
                    .then_some(asset.identity)
            })
    }

    /// Follows the source `GameObjectExtensions.GetRoot` rule: select the first Transform in
    /// `m_Component`, then follow `m_Father` while its target has a non-null `m_GameObject`.
    /// A GameObject without a Transform resolves to itself, matching the state after the source
    pub fn scene_for_collection(
        &self,
        definitions: &WorkspaceSceneDefinitions,
        collection_file_index: usize,
    ) -> Option<&WorkspaceSceneHierarchyScene> {
        definitions
            .scene_index_for_collection(collection_file_index)
            .and_then(|scene_index| self.scenes.get(scene_index))
    }
}

pub(super) fn build_collection_graph_index(
    scenes: &[WorkspaceSceneHierarchyScene],
    non_scene_collections: &[WorkspaceSceneCollectionHierarchy],
    expected_count: usize,
) -> Result<Vec<WorkspaceSceneCollectionGraphIndexEntry>, WorkspaceSceneHierarchyError> {
    let mut index = Vec::new();
    index.try_reserve_exact(expected_count).map_err(|_| {
        WorkspaceSceneHierarchyError::AllocationFailed {
            resource: "workspace collection graph lookup index",
            requested: expected_count,
        }
    })?;
    for (scene_index, scene) in scenes.iter().enumerate() {
        for (collection_index, collection) in scene.collections.iter().enumerate() {
            index.push(WorkspaceSceneCollectionGraphIndexEntry {
                file_index: collection.collection_file_index(),
                scene_index: Some(scene_index),
                collection_index,
            });
        }
    }
    for (collection_index, collection) in non_scene_collections.iter().enumerate() {
        index.push(WorkspaceSceneCollectionGraphIndexEntry {
            file_index: collection.collection_file_index(),
            scene_index: None,
            collection_index,
        });
    }
    if index.len() != expected_count {
        return Err(
            WorkspaceSceneHierarchyError::WorkspaceCollectionGraphIndexCountMismatch {
                expected: expected_count,
                actual: index.len(),
            },
        );
    }
    index.sort_unstable_by_key(|entry| entry.file_index);
    for pair in index.windows(2) {
        if pair[0].file_index == pair[1].file_index {
            return Err(WorkspaceSceneHierarchyError::DuplicateCollectionGraph {
                file_index: pair[0].file_index,
            });
        }
    }
    Ok(index)
}
