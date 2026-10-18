use super::*;

impl WorkspaceSceneCollectionHierarchy {
    pub fn collection_file_index(&self) -> usize {
        self.collection_file_index
    }

    pub fn scene_objects(&self) -> &[SceneObjectRecord] {
        &self.scene_objects
    }

    pub fn game_object_count(&self) -> usize {
        self.scene_objects
            .iter()
            .filter(|object| object.class_id == Some(GAME_OBJECT_CLASS_ID))
            .count()
    }

    pub fn transform_count(&self) -> usize {
        self.scene_objects
            .iter()
            .filter(|object| {
                matches!(
                    object.class_id,
                    Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)
                )
            })
            .count()
    }

    pub fn relationships(&self) -> &[SceneRelationship] {
        &self.relationships
    }

    pub fn game_object_hierarchies(&self) -> &[WorkspaceSceneGameObjectHierarchy] {
        &self.game_object_hierarchies
    }

    pub(super) fn prefab_root_relationships_for_object(
        &self,
        object_index: usize,
    ) -> &[WorkspacePrefabRootRelationshipIndexEntry] {
        let start = self
            .prefab_root_relationship_index
            .partition_point(|entry| entry.source_object_index < object_index);
        let end = self
            .prefab_root_relationship_index
            .partition_point(|entry| entry.source_object_index <= object_index);
        &self.prefab_root_relationship_index[start..end]
    }

    /// Return a GameObject's `m_Component` edges in serialized array order.
    pub fn game_object_component_relationships<'a>(
        &'a self,
        hierarchy_index: usize,
    ) -> Result<impl Iterator<Item = &'a SceneRelationship> + 'a, WorkspaceSceneHierarchyError>
    {
        let hierarchy = self.game_object_hierarchies.get(hierarchy_index).ok_or(
            WorkspaceSceneHierarchyError::GameObjectHierarchyIndexOutOfBounds {
                hierarchy_index,
                hierarchy_count: self.game_object_hierarchies.len(),
            },
        )?;
        let range = self
            .game_object_component_relationship_indices
            .get(hierarchy.component_relationship_start..hierarchy.component_relationship_end)
            .ok_or(
                WorkspaceSceneHierarchyError::InvalidGameObjectComponentRelationshipRange {
                    hierarchy_index,
                    start: hierarchy.component_relationship_start,
                    end: hierarchy.component_relationship_end,
                    index_count: self.game_object_component_relationship_indices.len(),
                },
            )?;
        for &relationship_index in range {
            if self.relationships.get(relationship_index).is_none() {
                return Err(
                    WorkspaceSceneHierarchyError::InvalidGameObjectComponentRelationshipIndex {
                        hierarchy_index,
                        relationship_index,
                        relationship_count: self.relationships.len(),
                    },
                );
            }
        }
        Ok(range
            .iter()
            .map(|&relationship_index| &self.relationships[relationship_index]))
    }

    /// Returns one source GameObject's ordered component-array relationships after identity checks.
    pub fn game_object_component_relationships_for_identity<'a>(
        &'a self,
        game_object: WorkspaceSceneObjectIdentity,
    ) -> Result<impl Iterator<Item = &'a SceneRelationship> + 'a, WorkspaceSceneHierarchyError>
    {
        let hierarchy_index = self.game_object_hierarchy_index(game_object)?;
        self.game_object_component_relationships(hierarchy_index)
    }

    /// Returns the serialized GameObject component-array length for a verified source identity.
    pub fn game_object_component_slot_count(
        &self,
        game_object: WorkspaceSceneObjectIdentity,
    ) -> Result<usize, WorkspaceSceneHierarchyError> {
        Ok(self
            .game_object_component_relationships_for_identity(game_object)?
            .count())
    }

    fn game_object_hierarchy_index(
        &self,
        game_object: WorkspaceSceneObjectIdentity,
    ) -> Result<usize, WorkspaceSceneHierarchyError> {
        if game_object.file_index != self.collection_file_index
            || game_object.class_id != Some(GAME_OBJECT_CLASS_ID)
        {
            return Err(WorkspaceSceneHierarchyError::InvalidSourceIdentity {
                file_index: game_object.file_index,
                object_index: game_object.object_index,
                path_id: game_object.path_id,
            });
        }
        let hierarchy_index = self
            .game_object_hierarchies
            .binary_search_by_key(&game_object.object_index, |hierarchy| {
                hierarchy.game_object.object_index
            })
            .map_err(
                |_| WorkspaceSceneHierarchyError::MissingGameObjectHierarchy {
                    file_index: game_object.file_index,
                    object_index: game_object.object_index,
                },
            )?;
        let hierarchy = &self.game_object_hierarchies[hierarchy_index];
        if hierarchy.game_object != game_object {
            return Err(WorkspaceSceneHierarchyError::InvalidSourceIdentity {
                file_index: game_object.file_index,
                object_index: game_object.object_index,
                path_id: game_object.path_id,
            });
        }
        Ok(hierarchy_index)
    }

    pub fn transform_hierarchies(&self) -> &[WorkspaceSceneTransformHierarchy] {
        &self.transform_hierarchies
    }

    pub fn transform_parent_relationships<'a>(
        &'a self,
        hierarchy_index: usize,
    ) -> Result<impl Iterator<Item = &'a SceneRelationship> + 'a, WorkspaceSceneHierarchyError>
    {
        let hierarchy = self.transform_hierarchies.get(hierarchy_index).ok_or(
            WorkspaceSceneHierarchyError::TransformHierarchyIndexOutOfBounds {
                hierarchy_index,
                hierarchy_count: self.transform_hierarchies.len(),
            },
        )?;
        let range = self
            .transform_parent_relationship_indices
            .get(hierarchy.parent_relationship_start..hierarchy.parent_relationship_end)
            .ok_or(
                WorkspaceSceneHierarchyError::InvalidTransformParentRelationshipRange {
                    hierarchy_index,
                    start: hierarchy.parent_relationship_start,
                    end: hierarchy.parent_relationship_end,
                    index_count: self.transform_parent_relationship_indices.len(),
                },
            )?;
        self.validate_transform_relationship_indexes(hierarchy_index, range)?;
        Ok(range
            .iter()
            .map(|&relationship_index| &self.relationships[relationship_index]))
    }

    pub fn transform_child_relationships<'a>(
        &'a self,
        hierarchy_index: usize,
    ) -> Result<impl Iterator<Item = &'a SceneRelationship> + 'a, WorkspaceSceneHierarchyError>
    {
        let hierarchy = self.transform_hierarchies.get(hierarchy_index).ok_or(
            WorkspaceSceneHierarchyError::TransformHierarchyIndexOutOfBounds {
                hierarchy_index,
                hierarchy_count: self.transform_hierarchies.len(),
            },
        )?;
        let range = self
            .transform_child_relationship_indices
            .get(hierarchy.child_relationship_start..hierarchy.child_relationship_end)
            .ok_or(
                WorkspaceSceneHierarchyError::InvalidTransformChildRelationshipRange {
                    hierarchy_index,
                    start: hierarchy.child_relationship_start,
                    end: hierarchy.child_relationship_end,
                    index_count: self.transform_child_relationship_indices.len(),
                },
            )?;
        self.validate_transform_relationship_indexes(hierarchy_index, range)?;
        Ok(range
            .iter()
            .map(|&relationship_index| &self.relationships[relationship_index]))
    }

    pub fn transform_game_object_relationships<'a>(
        &'a self,
        hierarchy_index: usize,
    ) -> Result<impl Iterator<Item = &'a SceneRelationship> + 'a, WorkspaceSceneHierarchyError>
    {
        let hierarchy = self.transform_hierarchies.get(hierarchy_index).ok_or(
            WorkspaceSceneHierarchyError::TransformHierarchyIndexOutOfBounds {
                hierarchy_index,
                hierarchy_count: self.transform_hierarchies.len(),
            },
        )?;
        let range = self
            .transform_game_object_relationship_indices
            .get(hierarchy.game_object_relationship_start..hierarchy.game_object_relationship_end)
            .ok_or(
                WorkspaceSceneHierarchyError::InvalidTransformGameObjectRelationshipRange {
                    hierarchy_index,
                    start: hierarchy.game_object_relationship_start,
                    end: hierarchy.game_object_relationship_end,
                    index_count: self.transform_game_object_relationship_indices.len(),
                },
            )?;
        self.validate_transform_relationship_indexes(hierarchy_index, range)?;
        Ok(range
            .iter()
            .map(|&relationship_index| &self.relationships[relationship_index]))
    }

    fn validate_transform_relationship_indexes(
        &self,
        hierarchy_index: usize,
        indexes: &[usize],
    ) -> Result<(), WorkspaceSceneHierarchyError> {
        for &relationship_index in indexes {
            if self.relationships.get(relationship_index).is_none() {
                return Err(
                    WorkspaceSceneHierarchyError::InvalidTransformRelationshipIndex {
                        hierarchy_index,
                        relationship_index,
                        relationship_count: self.relationships.len(),
                    },
                );
            }
        }
        Ok(())
    }

    pub fn graph_issues(&self) -> &[SceneGraphIssue] {
        &self.graph_issues
    }

    pub fn scene_assets(&self) -> &[WorkspaceSceneAssetIdentity] {
        &self.scene_assets
    }

    pub fn has_lighting_data_asset_field(&self, object_index: usize) -> bool {
        self.lighting_data_asset_field_object_indices
            .binary_search(&object_index)
            .is_ok()
    }

    pub fn component_candidates(&self) -> &[WorkspaceSceneComponentCandidate] {
        &self.component_candidates
    }

    pub fn missing_transform_plans(&self) -> &[WorkspaceMissingTransformPlan] {
        &self.missing_transform_plans
    }

    pub fn source_graph_complete(&self) -> bool {
        self.source_graph_complete
    }

    pub fn source_reference_scans_complete(&self) -> bool {
        self.source_reference_scans_complete
    }

    /// True when objects consumed by `PrefabProcessor` have complete source reference scans.
    /// Unrelated asset scans remain visible through `source_reference_scans_complete` and the
    /// workspace reference-graph report, but do not affect prefab hierarchy allocation.
    pub fn source_prefab_processor_reference_scans_complete(&self) -> bool {
        self.source_prefab_processor_reference_scans_complete
    }

    /// True when every local source object consumed by SceneDefinition membership has a complete
    /// reference scan. Workspace-wide unrelated asset scans remain available separately.
    pub fn source_scene_membership_reference_scans_complete(&self) -> bool {
        self.source_scene_membership_reference_scans_complete
    }

    pub fn roots(&self) -> &WorkspaceSceneRootDiscovery {
        &self.roots
    }

    pub fn hierarchy_audit(&self) -> &WorkspaceSceneHierarchyAudit {
        &self.hierarchy_audit
    }

    pub fn is_complete(&self) -> bool {
        self.source_graph_complete
            && self.source_reference_scans_complete
            && self.graph_issues.is_empty()
            && self.roots.is_complete()
            && self.hierarchy_audit.is_complete()
    }

    pub fn is_prefab_processor_complete(&self) -> bool {
        // PrefabProcessor queries typed PPtr accessors. Failed target lookup is
        // presented as null there, while the raw reference graph remains
        // incomplete and separately observable.
        self.source_prefab_processor_reference_scans_complete
            && self.graph_issues.is_empty()
            && self.roots.is_complete()
            && self.hierarchy_audit.is_complete()
    }
}
