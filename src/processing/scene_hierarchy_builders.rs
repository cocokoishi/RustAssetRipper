fn build_collection_graph_index(
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceGameObjectHierarchyWalkError {
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    CountOverflow {
        resource: &'static str,
    },
    ResourceLimitExceeded {
        resource: &'static str,
        requested: usize,
        limit: usize,
    },
    RootIsNotGameObject {
        identity: WorkspaceSceneObjectIdentity,
    },
    GraphInvariantFailed {
        message: String,
    },
}

impl Display for WorkspaceGameObjectHierarchyWalkError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AllocationFailed {
                resource,
                requested,
            } => {
                write!(f, "could not allocate {requested} entries for {resource}")
            }
            Self::CountOverflow { resource } => write!(f, "{resource} count overflows usize"),
            Self::ResourceLimitExceeded {
                resource,
                requested,
                limit,
            } => write!(f, "{resource} count {requested} exceeds limit {limit}"),
            Self::RootIsNotGameObject { identity } => write!(
                f,
                "hierarchy root file {} object {} PathID {} is not a GameObject",
                identity.file_index, identity.object_index, identity.path_id
            ),
            Self::GraphInvariantFailed { message } => {
                write!(f, "hierarchy graph index is inconsistent: {message}")
            }
        }
    }
}

impl Error for WorkspaceGameObjectHierarchyWalkError {}

/// Incrementally compacts hierarchy graphs so inspect does not retain full per-file indexes.
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceSceneHierarchyError {
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    CountOverflow {
        resource: &'static str,
    },
    ResourceLimitExceeded {
        resource: &'static str,
        requested: usize,
        limit: usize,
    },
    WorkspaceFileCountMismatch {
        count: usize,
        limit: usize,
    },
    WorkspaceDefinitionCollectionCountMismatch {
        expected: usize,
        actual: usize,
    },
    WorkspaceCollectionGraphIndexCountMismatch {
        expected: usize,
        actual: usize,
    },
    SceneCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    CollectionFileIndexOutOfBounds {
        file_index: usize,
        file_count: usize,
    },
    DuplicateCollectionGraph {
        file_index: usize,
    },
    DuplicateGeneratedTransformSource {
        file_index: usize,
    },
    MissingCollectionGraph {
        scene_index: usize,
        file_index: usize,
    },
    InvalidSourceIdentity {
        file_index: usize,
        object_index: usize,
        path_id: i64,
    },
    InvalidRelationshipSource {
        file_index: usize,
        relationship_index: usize,
        object_index: usize,
    },
    PrefabRootRelationshipSourceMissing {
        file_index: usize,
        relationship_index: usize,
        object_index: usize,
    },
    DuplicateGameObjectHierarchy {
        file_index: usize,
        object_index: usize,
    },
    GameObjectComponentRelationshipSourceMissing {
        file_index: usize,
        relationship_index: usize,
        object_index: usize,
    },
    GameObjectHierarchyIndexOutOfBounds {
        hierarchy_index: usize,
        hierarchy_count: usize,
    },
    MissingGameObjectHierarchy {
        file_index: usize,
        object_index: usize,
    },
    InvalidGameObjectComponentRelationshipRange {
        hierarchy_index: usize,
        start: usize,
        end: usize,
        index_count: usize,
    },
    InvalidGameObjectComponentRelationshipIndex {
        hierarchy_index: usize,
        relationship_index: usize,
        relationship_count: usize,
    },
    DuplicateTransformHierarchy {
        file_index: usize,
        object_index: usize,
    },
    InvalidTransformHierarchySource {
        file_index: usize,
        relationship_index: usize,
        object_index: usize,
    },
    TransformHierarchyIndexOutOfBounds {
        hierarchy_index: usize,
        hierarchy_count: usize,
    },
    InvalidTransformParentRelationshipRange {
        hierarchy_index: usize,
        start: usize,
        end: usize,
        index_count: usize,
    },
    InvalidTransformChildRelationshipRange {
        hierarchy_index: usize,
        start: usize,
        end: usize,
        index_count: usize,
    },
    InvalidTransformGameObjectRelationshipRange {
        hierarchy_index: usize,
        start: usize,
        end: usize,
        index_count: usize,
    },
    InvalidTransformRelationshipIndex {
        hierarchy_index: usize,
        relationship_index: usize,
        relationship_count: usize,
    },
    InvalidComponentTarget {
        source_file_index: usize,
        target_file_index: usize,
        object_index: usize,
    },
    InvalidRootIdentity {
        file_index: usize,
        object_index: usize,
    },
    InvalidRootTransformIdentity {
        file_index: usize,
        transform_file_index: usize,
        transform_index: usize,
    },
    RootSourceFileMismatch {
        expected: usize,
        actual: usize,
    },
    AuditSourceFileMismatch {
        expected: usize,
        actual: usize,
    },
}

impl Display for WorkspaceSceneHierarchyError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AllocationFailed { resource, requested } => {
                write!(f, "could not allocate {requested} entries for {resource}")
            }
            Self::CountOverflow { resource } => write!(f, "{resource} count overflows usize"),
            Self::ResourceLimitExceeded {
                resource,
                requested,
                limit,
            } => write!(f, "{resource} count {requested} exceeds limit {limit}"),
            Self::WorkspaceFileCountMismatch { count, limit } => {
                write!(f, "workspace file count {count} exceeds limit {limit}")
            }
            Self::WorkspaceDefinitionCollectionCountMismatch { expected, actual } => write!(
                f,
                "generated-transform planning expected {expected} workspace collections, definitions contain {actual}"
            ),
            Self::WorkspaceCollectionGraphIndexCountMismatch { expected, actual } => write!(
                f,
                "workspace collection graph index expected {expected} entries, contains {actual}"
            ),
            Self::SceneCountLimitExceeded { count, limit } => {
                write!(f, "SceneDefinition count {count} exceeds limit {limit}")
            }
            Self::CollectionFileIndexOutOfBounds {
                file_index,
                file_count,
            } => write!(f, "scene collection file index {file_index} is outside {file_count} files"),
            Self::DuplicateCollectionGraph { file_index } => {
                write!(f, "scene collection file {file_index} has more than one graph")
            }
            Self::DuplicateGeneratedTransformSource { file_index } => write!(
                f,
                "workspace file {file_index} supplied generated-transform candidates more than once"
            ),
            Self::MissingCollectionGraph {
                scene_index,
                file_index,
            } => write!(
                f,
                "SceneDefinition {scene_index} has no recovered graph for collection file {file_index}"
            ),
            Self::InvalidSourceIdentity {
                file_index,
                object_index,
                path_id,
            } => write!(
                f,
                "scene graph object {file_index}:{object_index} PathID {path_id} does not match the joined workspace"
            ),
            Self::InvalidRelationshipSource {
                file_index,
                relationship_index,
                object_index,
            } => write!(
                f,
                "scene relationship {relationship_index} source object {file_index}:{object_index} is invalid"
            ),
            Self::PrefabRootRelationshipSourceMissing {
                file_index,
                relationship_index,
                object_index,
            } => write!(
                f,
                "PrefabInstance root relationship {relationship_index} in collection {file_index} has no matching PrefabInstance at object {object_index}"
            ),
            Self::DuplicateGameObjectHierarchy {
                file_index,
                object_index,
            } => write!(
                f,
                "scene collection {file_index} contains GameObject {object_index} more than once"
            ),
            Self::GameObjectComponentRelationshipSourceMissing {
                file_index,
                relationship_index,
                object_index,
            } => write!(
                f,
                "GameObject component relationship {relationship_index} in collection {file_index} has no indexed GameObject source at object {object_index}"
            ),
            Self::GameObjectHierarchyIndexOutOfBounds {
                hierarchy_index,
                hierarchy_count,
            } => write!(
                f,
                "GameObject hierarchy index {hierarchy_index} is outside {hierarchy_count} entries"
            ),
            Self::MissingGameObjectHierarchy {
                file_index,
                object_index,
            } => write!(
                f,
                "collection {file_index} has no component list for GameObject {object_index}"
            ),
            Self::InvalidGameObjectComponentRelationshipRange {
                hierarchy_index,
                start,
                end,
                index_count,
            } => write!(
                f,
                "GameObject hierarchy {hierarchy_index} component relationship range {start}..{end} is outside {index_count} indexes"
            ),
            Self::InvalidGameObjectComponentRelationshipIndex {
                hierarchy_index,
                relationship_index,
                relationship_count,
            } => write!(
                f,
                "GameObject hierarchy {hierarchy_index} references relationship {relationship_index}, but only {relationship_count} relationships exist"
            ),
            Self::DuplicateTransformHierarchy {
                file_index,
                object_index,
            } => write!(
                f,
                "scene collection {file_index} contains Transform {object_index} more than once"
            ),
            Self::InvalidTransformHierarchySource {
                file_index,
                relationship_index,
                object_index,
            } => write!(
                f,
                "Transform hierarchy relationship {relationship_index} in collection {file_index} has no indexed Transform source at object {object_index}"
            ),
            Self::TransformHierarchyIndexOutOfBounds {
                hierarchy_index,
                hierarchy_count,
            } => write!(
                f,
                "Transform hierarchy index {hierarchy_index} is outside {hierarchy_count} entries"
            ),
            Self::InvalidTransformParentRelationshipRange {
                hierarchy_index,
                start,
                end,
                index_count,
            } => write!(
                f,
                "Transform hierarchy {hierarchy_index} parent relationship range {start}..{end} is outside {index_count} indexes"
            ),
            Self::InvalidTransformChildRelationshipRange {
                hierarchy_index,
                start,
                end,
                index_count,
            } => write!(
                f,
                "Transform hierarchy {hierarchy_index} child relationship range {start}..{end} is outside {index_count} indexes"
            ),
            Self::InvalidTransformGameObjectRelationshipRange {
                hierarchy_index,
                start,
                end,
                index_count,
            } => write!(
                f,
                "Transform hierarchy {hierarchy_index} GameObject relationship range {start}..{end} is outside {index_count} indexes"
            ),
            Self::InvalidTransformRelationshipIndex {
                hierarchy_index,
                relationship_index,
                relationship_count,
            } => write!(
                f,
                "Transform hierarchy {hierarchy_index} references relationship {relationship_index}, but only {relationship_count} relationships exist"
            ),
            Self::InvalidComponentTarget {
                source_file_index,
                target_file_index,
                object_index,
            } => write!(
                f,
                "scene collection {source_file_index} component edge targets invalid object {target_file_index}:{object_index}"
            ),
            Self::InvalidRootIdentity {
                file_index,
                object_index,
            } => write!(
                f,
                "scene root candidate GameObject {file_index}:{object_index} is invalid"
            ),
            Self::InvalidRootTransformIdentity {
                file_index,
                transform_file_index,
                transform_index,
            } => write!(
                f,
                "scene root candidate Transform {transform_file_index}:{transform_index} from file {file_index} is invalid"
            ),
            Self::RootSourceFileMismatch { expected, actual } => write!(
                f,
                "root discovery belongs to source file {actual}, expected {expected}"
            ),
            Self::AuditSourceFileMismatch { expected, actual } => write!(
                f,
                "hierarchy audit contains source file {actual}, expected {expected}"
            ),
        }
    }
}

impl Error for WorkspaceSceneHierarchyError {}

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

        validate_collection_graph(
            file_index,
            scene_graph,
            &roots,
            &hierarchy_audit,
            references,
        )?;
        let source_reference_scans_complete =
            (0..scene_graph.objects().len()).all(|object_index| {
                references.object_scan_is_complete(file_index, object_index) == Some(true)
            });
        let classification = classify_scene_collection_objects(
            file_index,
            scene_graph,
            has_lighting_data_asset,
            references,
        )?;
        let ClassifiedSceneCollectionObjects {
            scene_assets,
            lighting_data_asset_field_object_indices,
            component_candidates,
        } = classification;
        let missing_transform_plans =
            build_missing_transform_plans(file_index, &roots, references)?;
        let scene_objects = scene_graph
            .objects()
            .iter()
            .filter(|object| {
                matches!(
                    object.class_id,
                    Some(GAME_OBJECT_CLASS_ID | TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)
                )
            })
            .count();
        let issue_count = scene_graph
            .issues()
            .len()
            .checked_add(roots.issues().len())
            .and_then(|count| count.checked_add(hierarchy_audit.issues().len()))
            .ok_or(WorkspaceSceneHierarchyError::CountOverflow {
                resource: "scene graph diagnostics",
            })?;
        let name_bytes = scene_graph
            .objects()
            .iter()
            .filter(|object| {
                matches!(
                    object.class_id,
                    Some(GAME_OBJECT_CLASS_ID | TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)
                )
            })
            .try_fold(0usize, |total, object| {
                total.checked_add(object.name_bytes.as_ref().map_or(0, Vec::len))
            })
            .ok_or(WorkspaceSceneHierarchyError::CountOverflow {
                resource: "scene object names",
            })?;
        let diagnostic_bytes = scene_graph
            .relationships()
            .iter()
            .try_fold(0usize, |total, relationship| {
                total.checked_add(relationship.field_path.len())
            })
            .and_then(|total| {
                scene_graph.issues().iter().try_fold(total, |total, issue| {
                    let field_path_bytes = issue.field_path.as_ref().map_or(0, String::len);
                    let kind_bytes = match &issue.kind {
                        super::scene::SceneGraphIssueKind::ObjectDecodeFailed { message } => {
                            message.len()
                        }
                        super::scene::SceneGraphIssueKind::MissingField { field_name } => {
                            field_name.len()
                        }
                        super::scene::SceneGraphIssueKind::DuplicateField {
                            field_name, ..
                        } => field_name.len(),
                        super::scene::SceneGraphIssueKind::UnexpectedValueType { .. } => 0,
                    };
                    total.checked_add(field_path_bytes)?.checked_add(kind_bytes)
                })
            })
            .and_then(|total| {
                hierarchy_audit
                    .issues()
                    .iter()
                    .try_fold(total, |total, issue| {
                        total.checked_add(issue.field_path.len())
                    })
            })
            .ok_or(WorkspaceSceneHierarchyError::CountOverflow {
                resource: "scene diagnostics",
            })?;

        let next_collection_graph_count = checked_charge(
            self.collection_graph_count,
            1,
            self.limits.max_workspace_collection_graphs,
            "workspace hierarchy collection graphs",
        )?;
        let next_scene_object_count = checked_charge(
            self.scene_object_count,
            scene_objects,
            self.limits.max_scene_objects,
            "scene objects",
        )?;
        let scene_asset_record_count = scene_assets
            .len()
            .checked_add(component_candidates.len())
            .and_then(|count| count.checked_add(missing_transform_plans.len()))
            .ok_or(WorkspaceSceneHierarchyError::CountOverflow {
                resource: "scene asset and generated-transform records",
            })?;
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
            issue_count,
            self.limits.max_issues,
            "scene graph issues",
        )?;
        let next_name_bytes = checked_charge(
            self.name_bytes,
            name_bytes,
            self.limits.max_total_name_bytes,
            "scene object name bytes",
        )?;
        let next_diagnostic_bytes = checked_charge(
            self.diagnostic_bytes,
            diagnostic_bytes,
            self.limits.max_total_diagnostic_bytes,
            "scene graph diagnostic bytes",
        )?;

        let mut objects = Vec::new();
        objects.try_reserve_exact(scene_objects).map_err(|_| {
            WorkspaceSceneHierarchyError::AllocationFailed {
                resource: "SceneDefinition scene objects",
                requested: scene_objects,
            }
        })?;
        for object in scene_graph.objects().iter().filter(|object| {
            matches!(
                object.class_id,
                Some(GAME_OBJECT_CLASS_ID | TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)
            )
        }) {
            objects.push(copy_scene_object(object)?);
        }
        objects.sort_unstable_by_key(|object| object.object_index);

        let mut relationships = Vec::new();
        relationships
            .try_reserve_exact(scene_graph.relationships().len())
            .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
                resource: "SceneDefinition scene relationships",
                requested: scene_graph.relationships().len(),
            })?;
        for relationship in scene_graph.relationships() {
            relationships.push(copy_scene_relationship(relationship)?);
        }

        let (game_object_hierarchies, game_object_component_relationship_indices) =
            build_game_object_component_relationship_index(
                file_index,
                &scene_assets,
                &relationships,
            )?;
        let transform_index =
            build_transform_hierarchy_index(file_index, &component_candidates, &relationships)?;
        let prefab_root_relationship_index =
            build_prefab_root_relationship_index(file_index, &scene_assets, &relationships)?;

        let mut graph_issues = Vec::new();
        graph_issues
            .try_reserve_exact(scene_graph.issues().len())
            .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
                resource: "SceneDefinition scene graph diagnostics",
                requested: scene_graph.issues().len(),
            })?;
        for issue in scene_graph.issues() {
            graph_issues.push(copy_scene_graph_issue(issue)?);
        }

        let snapshot = WorkspaceSceneCollectionHierarchy {
            collection_file_index: file_index,
            scene_objects: objects,
            relationships,
            game_object_hierarchies,
            game_object_component_relationship_indices,
            prefab_root_relationship_index,
            transform_hierarchies: transform_index.transforms,
            transform_parent_relationship_indices: transform_index.parent_relationship_indices,
            transform_child_relationship_indices: transform_index.child_relationship_indices,
            transform_game_object_relationship_indices: transform_index
                .game_object_relationship_indices,
            graph_issues,
            scene_assets,
            lighting_data_asset_field_object_indices,
            component_candidates,
            missing_transform_plans,
            source_graph_complete: scene_graph.is_complete(),
            source_reference_scans_complete,
            roots,
            hierarchy_audit,
        };
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

fn build_game_object_component_relationship_index(
    collection_file_index: usize,
    scene_assets: &[WorkspaceSceneAssetIdentity],
    relationships: &[SceneRelationship],
) -> Result<(Vec<WorkspaceSceneGameObjectHierarchy>, Vec<usize>), WorkspaceSceneHierarchyError> {
    let game_object_count = scene_assets
        .iter()
        .filter(|asset| asset.category == WorkspaceSceneAssetCategory::GameObject)
        .count();
    let mut game_objects = Vec::new();
    game_objects
        .try_reserve_exact(game_object_count)
        .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
            resource: "SceneDefinition GameObject hierarchy indexes",
            requested: game_object_count,
        })?;
    for asset in scene_assets
        .iter()
        .filter(|asset| asset.category == WorkspaceSceneAssetCategory::GameObject)
    {
        game_objects.push(WorkspaceSceneGameObjectHierarchy {
            game_object: asset.identity,
            component_relationship_start: 0,
            component_relationship_end: 0,
        });
    }
    game_objects.sort_unstable_by_key(|hierarchy| hierarchy.game_object.object_index);
    for pair in game_objects.windows(2) {
        if pair[0].game_object.object_index == pair[1].game_object.object_index {
            return Err(WorkspaceSceneHierarchyError::DuplicateGameObjectHierarchy {
                file_index: collection_file_index,
                object_index: pair[0].game_object.object_index,
            });
        }
    }

    let component_relationship_count = relationships
        .iter()
        .filter(|relationship| relationship.kind == SceneRelationshipKind::GameObjectComponent)
        .count();
    let mut component_relationship_indices = Vec::new();
    component_relationship_indices
        .try_reserve_exact(component_relationship_count)
        .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
            resource: "GameObject component relationship indexes",
            requested: component_relationship_count,
        })?;
    component_relationship_indices.extend(relationships.iter().enumerate().filter_map(
        |(relationship_index, relationship)| {
            (relationship.kind == SceneRelationshipKind::GameObjectComponent)
                .then_some(relationship_index)
        },
    ));
    component_relationship_indices.sort_unstable_by_key(|&relationship_index| {
        (
            relationships[relationship_index].source_object_index,
            relationship_index,
        )
    });

    let mut relationship_cursor = 0usize;
    for hierarchy in &mut game_objects {
        let object_index = hierarchy.game_object.object_index;
        if let Some(&relationship_index) = component_relationship_indices.get(relationship_cursor) {
            let relationship = &relationships[relationship_index];
            if relationship.source_object_index < object_index {
                return Err(
                    WorkspaceSceneHierarchyError::GameObjectComponentRelationshipSourceMissing {
                        file_index: collection_file_index,
                        relationship_index,
                        object_index: relationship.source_object_index,
                    },
                );
            }
        }

        hierarchy.component_relationship_start = relationship_cursor;
        while let Some(&relationship_index) =
            component_relationship_indices.get(relationship_cursor)
        {
            if relationships[relationship_index].source_object_index != object_index {
                break;
            }
            relationship_cursor += 1;
        }
        hierarchy.component_relationship_end = relationship_cursor;
    }
    if let Some(&relationship_index) = component_relationship_indices.get(relationship_cursor) {
        return Err(
            WorkspaceSceneHierarchyError::GameObjectComponentRelationshipSourceMissing {
                file_index: collection_file_index,
                relationship_index,
                object_index: relationships[relationship_index].source_object_index,
            },
        );
    }

    Ok((game_objects, component_relationship_indices))
}

fn build_prefab_root_relationship_index(
    collection_file_index: usize,
    scene_assets: &[WorkspaceSceneAssetIdentity],
    relationships: &[SceneRelationship],
) -> Result<Vec<WorkspacePrefabRootRelationshipIndexEntry>, WorkspaceSceneHierarchyError> {
    let relationship_count = relationships
        .iter()
        .filter(|relationship| {
            relationship.kind == SceneRelationshipKind::PrefabInstanceRootGameObject
        })
        .count();
    let mut index = Vec::new();
    index.try_reserve_exact(relationship_count).map_err(|_| {
        WorkspaceSceneHierarchyError::AllocationFailed {
            resource: "PrefabInstance root relationship index",
            requested: relationship_count,
        }
    })?;

    for (relationship_index, relationship) in relationships.iter().enumerate() {
        if relationship.kind != SceneRelationshipKind::PrefabInstanceRootGameObject {
            continue;
        }
        let key = (collection_file_index, relationship.source_object_index);
        let source = scene_assets
            .binary_search_by_key(&key, |asset| {
                (asset.identity.file_index, asset.identity.object_index)
            })
            .ok()
            .and_then(|source_index| scene_assets.get(source_index));
        if !source.is_some_and(|asset| {
            asset.category == WorkspaceSceneAssetCategory::PrefabInstance
                && asset.identity.path_id == relationship.source_path_id
        }) {
            return Err(
                WorkspaceSceneHierarchyError::PrefabRootRelationshipSourceMissing {
                    file_index: collection_file_index,
                    relationship_index,
                    object_index: relationship.source_object_index,
                },
            );
        }
        index.push(WorkspacePrefabRootRelationshipIndexEntry {
            source_object_index: relationship.source_object_index,
            relationship_index,
        });
    }
    index.sort_unstable_by_key(|entry| (entry.source_object_index, entry.relationship_index));
    Ok(index)
}

fn build_transform_hierarchy_index(
    collection_file_index: usize,
    component_candidates: &[WorkspaceSceneComponentCandidate],
    relationships: &[SceneRelationship],
) -> Result<WorkspaceSceneTransformHierarchyIndex, WorkspaceSceneHierarchyError> {
    let transform_count = component_candidates
        .iter()
        .filter(|candidate| candidate.evidence == WorkspaceSceneComponentEvidence::TransformClassId)
        .count();
    let mut transforms = Vec::new();
    transforms.try_reserve_exact(transform_count).map_err(|_| {
        WorkspaceSceneHierarchyError::AllocationFailed {
            resource: "SceneDefinition Transform hierarchy indexes",
            requested: transform_count,
        }
    })?;
    for candidate in component_candidates
        .iter()
        .filter(|candidate| candidate.evidence == WorkspaceSceneComponentEvidence::TransformClassId)
    {
        if candidate.identity.file_index != collection_file_index
            || !matches!(
                candidate.identity.class_id,
                Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)
            )
        {
            return Err(WorkspaceSceneHierarchyError::InvalidSourceIdentity {
                file_index: candidate.identity.file_index,
                object_index: candidate.identity.object_index,
                path_id: candidate.identity.path_id,
            });
        }
        transforms.push(WorkspaceSceneTransformHierarchy {
            transform: candidate.identity,
            parent_relationship_start: 0,
            parent_relationship_end: 0,
            child_relationship_start: 0,
            child_relationship_end: 0,
            game_object_relationship_start: 0,
            game_object_relationship_end: 0,
        });
    }
    transforms.sort_unstable_by_key(|hierarchy| hierarchy.transform.object_index);
    for pair in transforms.windows(2) {
        if pair[0].transform.object_index == pair[1].transform.object_index {
            return Err(WorkspaceSceneHierarchyError::DuplicateTransformHierarchy {
                file_index: collection_file_index,
                object_index: pair[0].transform.object_index,
            });
        }
    }

    let parent_relationship_count = relationships
        .iter()
        .filter(|relationship| relationship.kind == SceneRelationshipKind::TransformParent)
        .count();
    let child_relationship_count = relationships
        .iter()
        .filter(|relationship| relationship.kind == SceneRelationshipKind::TransformChild)
        .count();
    let game_object_relationship_count = relationships
        .iter()
        .filter(|relationship| {
            relationship.kind == SceneRelationshipKind::ComponentGameObject
                && transforms
                    .binary_search_by_key(&relationship.source_object_index, |hierarchy| {
                        hierarchy.transform.object_index
                    })
                    .is_ok()
        })
        .count();

    let mut parent_relationship_indices = Vec::new();
    parent_relationship_indices
        .try_reserve_exact(parent_relationship_count)
        .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
            resource: "Transform parent relationship indexes",
            requested: parent_relationship_count,
        })?;
    let mut child_relationship_indices = Vec::new();
    child_relationship_indices
        .try_reserve_exact(child_relationship_count)
        .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
            resource: "Transform child relationship indexes",
            requested: child_relationship_count,
        })?;
    let mut game_object_relationship_indices = Vec::new();
    game_object_relationship_indices
        .try_reserve_exact(game_object_relationship_count)
        .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
            resource: "Transform GameObject relationship indexes",
            requested: game_object_relationship_count,
        })?;

    for (relationship_index, relationship) in relationships.iter().enumerate() {
        match relationship.kind {
            SceneRelationshipKind::TransformParent => {
                if transforms
                    .binary_search_by_key(&relationship.source_object_index, |hierarchy| {
                        hierarchy.transform.object_index
                    })
                    .is_err()
                {
                    return Err(
                        WorkspaceSceneHierarchyError::InvalidTransformHierarchySource {
                            file_index: collection_file_index,
                            relationship_index,
                            object_index: relationship.source_object_index,
                        },
                    );
                }
                parent_relationship_indices.push(relationship_index);
            }
            SceneRelationshipKind::TransformChild => {
                if transforms
                    .binary_search_by_key(&relationship.source_object_index, |hierarchy| {
                        hierarchy.transform.object_index
                    })
                    .is_err()
                {
                    return Err(
                        WorkspaceSceneHierarchyError::InvalidTransformHierarchySource {
                            file_index: collection_file_index,
                            relationship_index,
                            object_index: relationship.source_object_index,
                        },
                    );
                }
                child_relationship_indices.push(relationship_index);
            }
            SceneRelationshipKind::ComponentGameObject
                if transforms
                    .binary_search_by_key(&relationship.source_object_index, |hierarchy| {
                        hierarchy.transform.object_index
                    })
                    .is_ok() =>
            {
                game_object_relationship_indices.push(relationship_index)
            }
            _ => {}
        }
    }

    parent_relationship_indices.sort_unstable_by_key(|&relationship_index| {
        (
            relationships[relationship_index].source_object_index,
            relationship_index,
        )
    });
    child_relationship_indices.sort_unstable_by_key(|&relationship_index| {
        (
            relationships[relationship_index].source_object_index,
            relationship_index,
        )
    });
    game_object_relationship_indices.sort_unstable_by_key(|&relationship_index| {
        (
            relationships[relationship_index].source_object_index,
            relationship_index,
        )
    });
    assign_transform_relationship_ranges(
        collection_file_index,
        &mut transforms,
        &parent_relationship_indices,
        relationships,
        TransformRelationshipRange::Parent,
    )?;
    assign_transform_relationship_ranges(
        collection_file_index,
        &mut transforms,
        &child_relationship_indices,
        relationships,
        TransformRelationshipRange::Children,
    )?;
    assign_transform_relationship_ranges(
        collection_file_index,
        &mut transforms,
        &game_object_relationship_indices,
        relationships,
        TransformRelationshipRange::GameObject,
    )?;

    Ok(WorkspaceSceneTransformHierarchyIndex {
        transforms,
        parent_relationship_indices,
        child_relationship_indices,
        game_object_relationship_indices,
    })
}

fn assign_transform_relationship_ranges(
    collection_file_index: usize,
    transforms: &mut [WorkspaceSceneTransformHierarchy],
    relationship_indices: &[usize],
    relationships: &[SceneRelationship],
    range_kind: TransformRelationshipRange,
) -> Result<(), WorkspaceSceneHierarchyError> {
    let mut relationship_cursor = 0usize;
    for hierarchy in transforms {
        let object_index = hierarchy.transform.object_index;
        if let Some(&relationship_index) = relationship_indices.get(relationship_cursor) {
            let source_object_index = relationships[relationship_index].source_object_index;
            if source_object_index < object_index {
                return Err(
                    WorkspaceSceneHierarchyError::InvalidTransformHierarchySource {
                        file_index: collection_file_index,
                        relationship_index,
                        object_index: source_object_index,
                    },
                );
            }
        }
        let start = relationship_cursor;
        while let Some(&relationship_index) = relationship_indices.get(relationship_cursor) {
            if relationships[relationship_index].source_object_index != object_index {
                break;
            }
            relationship_cursor += 1;
        }
        match range_kind {
            TransformRelationshipRange::Parent => {
                hierarchy.parent_relationship_start = start;
                hierarchy.parent_relationship_end = relationship_cursor;
            }
            TransformRelationshipRange::Children => {
                hierarchy.child_relationship_start = start;
                hierarchy.child_relationship_end = relationship_cursor;
            }
            TransformRelationshipRange::GameObject => {
                hierarchy.game_object_relationship_start = start;
                hierarchy.game_object_relationship_end = relationship_cursor;
            }
        }
    }
    if let Some(&relationship_index) = relationship_indices.get(relationship_cursor) {
        return Err(
            WorkspaceSceneHierarchyError::InvalidTransformHierarchySource {
                file_index: collection_file_index,
                relationship_index,
                object_index: relationships[relationship_index].source_object_index,
            },
        );
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum TransformRelationshipRange {
    Parent,
    Children,
    GameObject,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TypeTreeComponentBaseClassification {
    Component,
    NotComponent,
    Unavailable,
    Ambiguous,
}

fn classify_type_tree_component_base(
    type_tree: Option<&TypeTree>,
) -> TypeTreeComponentBaseClassification {
    let Some(type_tree) = type_tree else {
        return TypeTreeComponentBaseClassification::Unavailable;
    };
    let Some(root) = type_tree.nodes.first() else {
        // SerializedTypes can retain an empty node list when the file has no TypeTree. That
        // absence cannot support a Component-vs-asset classification and must not become a
        // fabricated component candidate (especially for explicit raw-fallback objects).
        return TypeTreeComponentBaseClassification::Unavailable;
    };
    if root.level != 0 {
        return TypeTreeComponentBaseClassification::Ambiguous;
    }
    if root.type_name.as_deref() == Some("Component") {
        return TypeTreeComponentBaseClassification::Component;
    }

    let mut parent_index = 0usize;
    for _ in 0..type_tree.nodes.len() {
        let parent = &type_tree.nodes[parent_index];
        let Some(child_level) = parent.level.checked_add(1) else {
            return TypeTreeComponentBaseClassification::Ambiguous;
        };
        let mut base_index = None;
        for child_index in (parent_index + 1)..type_tree.nodes.len() {
            let child = &type_tree.nodes[child_index];
            if child.level <= parent.level {
                break;
            }
            if child.level == child_level
                && child.name.as_deref() == Some("Base")
                && base_index.replace(child_index).is_some()
            {
                return TypeTreeComponentBaseClassification::Ambiguous;
            }
        }
        let Some(base_index) = base_index else {
            return TypeTreeComponentBaseClassification::NotComponent;
        };
        let Some(base_type_name) = type_tree.nodes[base_index].type_name.as_deref() else {
            return TypeTreeComponentBaseClassification::Ambiguous;
        };
        if base_type_name == "Component" {
            return TypeTreeComponentBaseClassification::Component;
        }
        parent_index = base_index;
    }
    TypeTreeComponentBaseClassification::Ambiguous
}

struct ClassifiedSceneCollectionObjects {
    scene_assets: Vec<WorkspaceSceneAssetIdentity>,
    lighting_data_asset_field_object_indices: Vec<usize>,
    component_candidates: Vec<WorkspaceSceneComponentCandidate>,
}

fn classify_scene_collection_objects(
    file_index: usize,
    scene_graph: &SceneGraph,
    has_lighting_data_asset: Option<bool>,
    references: &MultiFileReferenceGraph<'_>,
) -> Result<ClassifiedSceneCollectionObjects, WorkspaceSceneHierarchyError> {
    let scene_asset_count = scene_graph
        .objects()
        .iter()
        .filter(|object| {
            scene_asset_category(
                references
                    .object_class_id(file_index, object.object_index)
                    .or(object.class_id),
            )
            .is_some()
        })
        .count();
    let mut scene_assets = Vec::new();
    scene_assets
        .try_reserve_exact(scene_asset_count)
        .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
            resource: "SceneHierarchyObject asset identities",
            requested: scene_asset_count,
        })?;

    let lighting_data_asset_field_object_count = scene_graph
        .objects()
        .iter()
        .filter(|object| {
            let class_id = references
                .object_class_id(file_index, object.object_index)
                .or(object.class_id);
            class_id == Some(LIGHTMAP_SETTINGS_CLASS_ID)
                && has_lighting_data_asset.unwrap_or_else(|| {
                    references
                        .references_from_object(file_index, object.object_index)
                        .any(|reference| is_lighting_data_asset_field_path(reference.field_path))
                })
        })
        .count();
    let mut lighting_data_asset_field_object_indices = Vec::new();
    lighting_data_asset_field_object_indices
        .try_reserve_exact(lighting_data_asset_field_object_count)
        .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
            resource: "LightmapSettings LightingDataAsset field index",
            requested: lighting_data_asset_field_object_count,
        })?;

    let mut component_candidates = Vec::new();
    let mut candidate_upper_bound = scene_graph
        .objects()
        .iter()
        .filter(|object| {
            matches!(
                references
                    .object_class_id(file_index, object.object_index)
                    .or(object.class_id),
                Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)
            )
        })
        .count();
    candidate_upper_bound = scene_graph
        .relationships()
        .iter()
        .filter(|relationship| relationship.kind == SceneRelationshipKind::GameObjectComponent)
        .try_fold(candidate_upper_bound, |count, _| count.checked_add(1))
        .ok_or(WorkspaceSceneHierarchyError::CountOverflow {
            resource: "scene component candidate",
        })?;
    candidate_upper_bound = scene_graph
        .objects()
        .iter()
        .try_fold(candidate_upper_bound, |count, object| {
            let class_id = references
                .object_class_id(file_index, object.object_index)
                .or(object.class_id);
            if scene_asset_category(class_id).is_some()
                || matches!(class_id, Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID))
            {
                return Some(count);
            }
            let pointer_field_count = references
                .references_from_object(file_index, object.object_index)
                .filter(|reference| is_game_object_pointer_path(reference.field_path))
                .count();
            let type_tree_candidate_count = usize::from(matches!(
                classify_type_tree_component_base(
                    references.object_type_tree(file_index, object.object_index,)
                ),
                TypeTreeComponentBaseClassification::Component
                    | TypeTreeComponentBaseClassification::Ambiguous
            ));
            pointer_field_count
                .checked_add(type_tree_candidate_count)
                .and_then(|additional| count.checked_add(additional))
        })
        .ok_or(WorkspaceSceneHierarchyError::CountOverflow {
            resource: "scene component candidate",
        })?;
    component_candidates
        .try_reserve_exact(candidate_upper_bound)
        .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
            resource: "scene component candidate identities",
            requested: candidate_upper_bound,
        })?;

    for object in scene_graph.objects() {
        let class_id = references
            .object_class_id(file_index, object.object_index)
            .or(object.class_id);
        if class_id == Some(LIGHTMAP_SETTINGS_CLASS_ID)
            && has_lighting_data_asset.unwrap_or_else(|| {
                references
                    .references_from_object(file_index, object.object_index)
                    .any(|reference| is_lighting_data_asset_field_path(reference.field_path))
            })
        {
            lighting_data_asset_field_object_indices.push(object.object_index);
        }
        if let Some(category) = scene_asset_category(class_id) {
            let identity = scene_object_identity(
                file_index,
                object.object_index,
                object.path_id,
                class_id,
                references,
            )?;
            scene_assets.push(WorkspaceSceneAssetIdentity { identity, category });
        }
        if matches!(class_id, Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)) {
            let identity = scene_object_identity(
                file_index,
                object.object_index,
                object.path_id,
                class_id,
                references,
            )?;
            component_candidates.push(WorkspaceSceneComponentCandidate {
                identity,
                evidence: WorkspaceSceneComponentEvidence::TransformClassId,
            });
        }

        if scene_asset_category(class_id).is_none()
            && !matches!(class_id, Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID))
        {
            let type_tree_evidence = match classify_type_tree_component_base(
                references.object_type_tree(file_index, object.object_index),
            ) {
                TypeTreeComponentBaseClassification::Component => {
                    Some(WorkspaceSceneComponentEvidence::TypeTreeComponentBase)
                }
                TypeTreeComponentBaseClassification::Ambiguous => {
                    Some(WorkspaceSceneComponentEvidence::AmbiguousTypeTreeBase)
                }
                TypeTreeComponentBaseClassification::NotComponent
                | TypeTreeComponentBaseClassification::Unavailable => None,
            };
            if let Some(evidence) = type_tree_evidence {
                let identity = scene_object_identity(
                    file_index,
                    object.object_index,
                    object.path_id,
                    class_id,
                    references,
                )?;
                component_candidates.push(WorkspaceSceneComponentCandidate { identity, evidence });
            }

            for reference in references.references_from_object(file_index, object.object_index) {
                if !is_game_object_pointer_path(reference.field_path) {
                    continue;
                }
                let identity = scene_object_identity(
                    file_index,
                    object.object_index,
                    object.path_id,
                    class_id,
                    references,
                )?;
                let evidence = if reference.path_id == 0 {
                    WorkspaceSceneComponentEvidence::GameObjectPointerFieldNull
                } else {
                    WorkspaceSceneComponentEvidence::GameObjectPointerField
                };
                component_candidates.push(WorkspaceSceneComponentCandidate { identity, evidence });
            }
        }
    }

    for relationship in scene_graph
        .relationships()
        .iter()
        .filter(|relationship| relationship.kind == SceneRelationshipKind::GameObjectComponent)
    {
        let (target_file_index, target_object_index) = match &relationship.target {
            SceneRelationshipTarget::LocalObject { object_index } => (file_index, *object_index),
            SceneRelationshipTarget::WorkspaceObject {
                file_index,
                object_index,
            } => (*file_index, *object_index),
            _ => continue,
        };
        let class_id = references.object_class_id(target_file_index, target_object_index);
        if target_file_index == file_index
            && matches!(class_id, Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID))
        {
            continue;
        }
        let identity = scene_object_identity(
            target_file_index,
            target_object_index,
            relationship.path_id,
            class_id,
            references,
        )
        .map_err(|error| match error {
            WorkspaceSceneHierarchyError::InvalidSourceIdentity { .. } => {
                WorkspaceSceneHierarchyError::InvalidComponentTarget {
                    source_file_index: file_index,
                    target_file_index,
                    object_index: target_object_index,
                }
            }
            other => other,
        })?;
        if identity.path_id != relationship.path_id {
            return Err(WorkspaceSceneHierarchyError::InvalidComponentTarget {
                source_file_index: file_index,
                target_file_index,
                object_index: target_object_index,
            });
        }
        component_candidates.push(WorkspaceSceneComponentCandidate {
            identity,
            evidence: WorkspaceSceneComponentEvidence::GameObjectComponentReference,
        });
    }

    scene_assets
        .sort_unstable_by_key(|asset| (asset.identity.file_index, asset.identity.object_index));
    lighting_data_asset_field_object_indices.sort_unstable();
    lighting_data_asset_field_object_indices.dedup();
    normalize_component_candidates(&mut component_candidates);
    Ok(ClassifiedSceneCollectionObjects {
        scene_assets,
        lighting_data_asset_field_object_indices,
        component_candidates,
    })
}

fn is_lighting_data_asset_field_path(field_path: &str) -> bool {
    field_path
        .rsplit('.')
        .next()
        .is_some_and(|field_name| field_name == "m_LightingDataAsset")
}

fn scene_asset_category(class_id: Option<i32>) -> Option<WorkspaceSceneAssetCategory> {
    match class_id {
        Some(GAME_OBJECT_CLASS_ID) => Some(WorkspaceSceneAssetCategory::GameObject),
        Some(PREFAB_INSTANCE_CLASS_ID) => Some(WorkspaceSceneAssetCategory::PrefabInstance),
        Some(SCENE_ROOTS_CLASS_ID) => Some(WorkspaceSceneAssetCategory::SceneRoots),
        manager => scene_manager_kind(manager).map(WorkspaceSceneAssetCategory::LevelGameManager),
    }
}

fn scene_object_identity(
    file_index: usize,
    object_index: usize,
    path_id: i64,
    class_id: Option<i32>,
    references: &MultiFileReferenceGraph<'_>,
) -> Result<WorkspaceSceneObjectIdentity, WorkspaceSceneHierarchyError> {
    let Some(identity) = references.object_identity(file_index, object_index) else {
        return Err(WorkspaceSceneHierarchyError::InvalidSourceIdentity {
            file_index,
            object_index,
            path_id,
        });
    };
    if identity.path_id != path_id {
        return Err(WorkspaceSceneHierarchyError::InvalidSourceIdentity {
            file_index,
            object_index,
            path_id,
        });
    }
    Ok(WorkspaceSceneObjectIdentity {
        file_index,
        object_index,
        path_id: identity.path_id,
        class_id,
    })
}

fn build_missing_transform_plans(
    file_index: usize,
    roots: &WorkspaceSceneRootDiscovery,
    references: &MultiFileReferenceGraph<'_>,
) -> Result<Vec<WorkspaceMissingTransformPlan>, WorkspaceSceneHierarchyError> {
    let game_objects = missing_transform_game_objects(file_index, roots, references)?;
    let mut plans = Vec::new();
    plans.try_reserve_exact(game_objects.len()).map_err(|_| {
        WorkspaceSceneHierarchyError::AllocationFailed {
            resource: "missing scene Transform plans",
            requested: game_objects.len(),
        }
    })?;
    for game_object in game_objects {
        plans.push(WorkspaceMissingTransformPlan {
            game_object,
            default_values: default_injected_transform_values(),
        });
    }
    Ok(plans)
}

fn missing_transform_game_objects(
    file_index: usize,
    roots: &WorkspaceSceneRootDiscovery,
    references: &MultiFileReferenceGraph<'_>,
) -> Result<Vec<WorkspaceSceneObjectIdentity>, WorkspaceSceneHierarchyError> {
    let game_object_count = roots
        .roots()
        .iter()
        .filter(|root| root.reason == SceneRootReason::NoTransformComponent)
        .count();
    let mut game_objects = Vec::new();
    game_objects
        .try_reserve_exact(game_object_count)
        .map_err(|_| WorkspaceSceneHierarchyError::AllocationFailed {
            resource: "missing-Transform GameObject identities",
            requested: game_object_count,
        })?;
    for root in roots
        .roots()
        .iter()
        .filter(|root| root.reason == SceneRootReason::NoTransformComponent)
    {
        let is_game_object = references
            .object_class_id(file_index, root.game_object_index)
            .is_some_and(|class_id| class_id == GAME_OBJECT_CLASS_ID);
        if root.source_file_index != file_index
            || !is_game_object
            || root.transform_file_index.is_some()
            || root.transform_index.is_some()
        {
            return Err(WorkspaceSceneHierarchyError::InvalidRootIdentity {
                file_index,
                object_index: root.game_object_index,
            });
        }
        let Some(source_identity) = references.object_identity(file_index, root.game_object_index)
        else {
            return Err(WorkspaceSceneHierarchyError::InvalidRootIdentity {
                file_index,
                object_index: root.game_object_index,
            });
        };
        game_objects.push(scene_object_identity(
            file_index,
            root.game_object_index,
            source_identity.path_id,
            Some(GAME_OBJECT_CLASS_ID),
            references,
        )?);
    }
    Ok(game_objects)
}
