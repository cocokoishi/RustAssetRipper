use super::collection_preparation::{CollectionGraphInput, PreparedCollectionGraph};
use super::resource_limits::checked_charge;
use super::workspace_graph::build_collection_graph_index;
use super::*;
use crate::serialized_file::reference_graph::MultiFileReferenceGraph;
#[derive(Clone, Debug)]
pub struct WorkspaceSceneHierarchyBuilder {
    collection_graphs: Vec<Option<WorkspaceSceneCollectionHierarchy>>,
    limits: WorkspaceSceneHierarchyLimits,
    collection_graph_count: usize,
    scene_object_count: usize,
    scene_asset_record_count: usize,
    relationship_count: usize,
    issue_count: usize,
    name_bytes: usize,
    diagnostic_bytes: usize,
}

impl WorkspaceSceneHierarchyBuilder {
    pub fn new(
        workspace_file_count: usize,
        limits: WorkspaceSceneHierarchyLimits,
    ) -> Result<Self, WorkspaceSceneHierarchyError> {
        if workspace_file_count > limits.max_workspace_files {
            return Err(WorkspaceSceneHierarchyError::WorkspaceFileCountMismatch {
                count: workspace_file_count,
                limit: limits.max_workspace_files,
            });
        }
        let mut collection_graphs = Vec::new();
        collection_graphs
            .try_reserve_exact(workspace_file_count)
            .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
                resource: "workspace scene collection graph index",
                requested: workspace_file_count,
            })?;
        collection_graphs.resize_with(workspace_file_count, || None);
        Ok(Self {
            collection_graphs,
            limits,
            collection_graph_count: 0,
            scene_object_count: 0,
            scene_asset_record_count: 0,
            relationship_count: 0,
            issue_count: 0,
            name_bytes: 0,
            diagnostic_bytes: 0,
        })
    }

    pub fn contains_collection_graph(&self, file_index: usize) -> bool {
        self.collection_graphs
            .get(file_index)
            .is_some_and(Option::is_some)
    }

    pub fn add_collection_graph(
        &mut self,
        file_index: usize,
        scene_graph: &SceneGraph,
        roots: WorkspaceSceneRootDiscovery,
        hierarchy_audit: WorkspaceSceneHierarchyAudit,
        references: &MultiFileReferenceGraph<'_>,
    ) -> Result<(), WorkspaceSceneHierarchyError> {
        self.add_collection_graph_with_lighting_data_asset_support(
            file_index,
            scene_graph,
            roots,
            hierarchy_audit,
            None,
            references,
        )
    }

    /// Add a collection graph with the source generated class's versioned
    /// `Has_LightingDataAsset` fact. `None` retains the serialized-reference fallback for callers
    /// without a usable Unity version.
    pub fn add_collection_graph_with_lighting_data_asset_support(
        &mut self,
        file_index: usize,
        scene_graph: &SceneGraph,
        roots: WorkspaceSceneRootDiscovery,
        hierarchy_audit: WorkspaceSceneHierarchyAudit,
        has_lighting_data_asset: Option<bool>,
        references: &MultiFileReferenceGraph<'_>,
    ) -> Result<(), WorkspaceSceneHierarchyError> {
        let Some(slot) = self.collection_graphs.get(file_index) else {
            return Err(
                WorkspaceSceneHierarchyError::CollectionFileIndexOutOfBounds {
                    file_index,
                    file_count: self.collection_graphs.len(),
                },
            );
        };
        if slot.is_some() {
            return Err(WorkspaceSceneHierarchyError::DuplicateCollectionGraph { file_index });
        }

        let prepared = PreparedCollectionGraph::prepare(CollectionGraphInput {
            file_index,
            scene_graph,
            roots,
            hierarchy_audit,
            has_lighting_data_asset,
            references,
        })?;
        let usage = prepared.usage();
        let next_collection_graph_count = checked_charge(
            self.collection_graph_count,
            1,
            self.limits.max_workspace_collection_graphs,
            "workspace hierarchy collection graphs",
        )?;
        let next_scene_object_count = checked_charge(
            self.scene_object_count,
            usage.scene_objects,
            self.limits.max_scene_objects,
            "scene objects",
        )?;
        let scene_asset_record_count = prepared.scene_asset_record_count()?;
        let next_scene_asset_record_count = checked_charge(
            self.scene_asset_record_count,
            scene_asset_record_count,
            self.limits.max_scene_asset_records,
            "scene asset and generated-transform records",
        )?;
        let next_relationship_count = checked_charge(
            self.relationship_count,
            scene_graph.relationships().len(),
            self.limits.max_relationships,
            "scene relationships",
        )?;
        let next_issue_count = checked_charge(
            self.issue_count,
            usage.issue_count,
            self.limits.max_issues,
            "scene graph issues",
        )?;
        let next_name_bytes = checked_charge(
            self.name_bytes,
            usage.name_bytes,
            self.limits.max_total_name_bytes,
            "scene object name bytes",
        )?;
        let next_diagnostic_bytes = checked_charge(
            self.diagnostic_bytes,
            usage.diagnostic_bytes,
            self.limits.max_total_diagnostic_bytes,
            "scene graph diagnostic bytes",
        )?;

        let snapshot = prepared.build_snapshot()?;
        self.collection_graphs[file_index] = Some(snapshot);
        self.collection_graph_count = next_collection_graph_count;
        self.scene_object_count = next_scene_object_count;
        self.scene_asset_record_count = next_scene_asset_record_count;
        self.relationship_count = next_relationship_count;
        self.issue_count = next_issue_count;
        self.name_bytes = next_name_bytes;
        self.diagnostic_bytes = next_diagnostic_bytes;
        Ok(())
    }
    pub fn finish(
        mut self,
        definitions: &WorkspaceSceneDefinitions,
    ) -> Result<WorkspaceSceneHierarchies, WorkspaceSceneHierarchyError> {
        if definitions.workspace_collection_count() != self.collection_graphs.len() {
            return Err(
                WorkspaceSceneHierarchyError::WorkspaceDefinitionCollectionCountMismatch {
                    expected: self.collection_graphs.len(),
                    actual: definitions.workspace_collection_count(),
                },
            );
        }
        if definitions.definitions().len() > self.limits.max_scene_definitions {
            return Err(WorkspaceSceneHierarchyError::SceneCountLimitExceeded {
                count: definitions.definitions().len(),
                limit: self.limits.max_scene_definitions,
            });
        }

        let mut scenes = Vec::new();
        scenes
            .try_reserve_exact(definitions.definitions().len())
            .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
                resource: "SceneDefinition hierarchy records",
                requested: definitions.definitions().len(),
            })?;
        for definition in definitions.definitions() {
            let mut collections = Vec::new();
            collections
                .try_reserve_exact(definition.collection_file_indices().len())
                .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
                    resource: "SceneDefinition collection hierarchy records",
                    requested: definition.collection_file_indices().len(),
                })?;
            for &file_index in definition.collection_file_indices() {
                let Some(slot) = self.collection_graphs.get_mut(file_index) else {
                    return Err(
                        WorkspaceSceneHierarchyError::CollectionFileIndexOutOfBounds {
                            file_index,
                            file_count: self.collection_graphs.len(),
                        },
                    );
                };
                let Some(collection_graph) = slot.take() else {
                    return Err(WorkspaceSceneHierarchyError::MissingCollectionGraph {
                        scene_index: scenes.len(),
                        file_index,
                    });
                };
                collections.push(collection_graph);
            }
            scenes.push(WorkspaceSceneHierarchyScene {
                plan: definition.plan.clone(),
                collections,
            });
        }
        let non_scene_collection_count = self
            .collection_graphs
            .iter()
            .filter(|graph| graph.is_some())
            .count();
        let mut non_scene_collections = Vec::new();
        non_scene_collections
            .try_reserve_exact(non_scene_collection_count)
            .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
                resource: "non-scene hierarchy collection graphs",
                requested: non_scene_collection_count,
            })?;
        for graph in self.collection_graphs.into_iter().flatten() {
            non_scene_collections.push(graph);
        }
        let collection_graph_index = build_collection_graph_index(
            &scenes,
            &non_scene_collections,
            self.collection_graph_count,
        )?;
        Ok(WorkspaceSceneHierarchies {
            scenes,
            non_scene_collections,
            collection_graph_index,
        })
    }
}
