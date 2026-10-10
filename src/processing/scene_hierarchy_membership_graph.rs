#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceSceneHierarchyMembershipIssue {
    IncompleteCollectionGraph {
        file_index: usize,
    },
    IncompleteReferenceScans {
        file_index: usize,
    },
    InvalidSceneAssetIdentity {
        identity: WorkspaceSceneObjectIdentity,
        collection_file_index: usize,
        expected_class_id: Option<i32>,
    },
    DuplicateSceneAsset {
        identity: WorkspaceSceneObjectIdentity,
    },
    ConflictingSceneAssetIdentity {
        existing: WorkspaceSceneObjectIdentity,
        conflicting: WorkspaceSceneObjectIdentity,
    },
    ConflictingComponentIdentity {
        existing: WorkspaceSceneObjectIdentity,
        conflicting: WorkspaceSceneObjectIdentity,
    },
    ConflictingComponentGameObjectEvidence {
        identity: WorkspaceSceneObjectIdentity,
        existing: WorkspaceSceneComponentEvidence,
        conflicting: WorkspaceSceneComponentEvidence,
    },
    ComponentOutsideSceneCollections {
        identity: WorkspaceSceneObjectIdentity,
        evidence: WorkspaceSceneComponentEvidence,
    },
    ComponentClassIdUnavailable {
        identity: WorkspaceSceneObjectIdentity,
    },
    MonoBehaviourGameObjectReferenceUnavailable {
        identity: WorkspaceSceneObjectIdentity,
    },
    UnclassifiedComponentCandidate {
        identity: WorkspaceSceneObjectIdentity,
        evidence: WorkspaceSceneComponentEvidence,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceSceneHierarchyMembership {
    game_objects: Vec<WorkspaceSceneObjectIdentity>,
    components: Vec<WorkspaceSceneObjectIdentity>,
    prefab_instances: Vec<WorkspaceSceneObjectIdentity>,
    managers: Vec<WorkspaceSceneObjectIdentity>,
    scene_roots: Option<WorkspaceSceneObjectIdentity>,
    issues: Vec<WorkspaceSceneHierarchyMembershipIssue>,
}

impl WorkspaceSceneHierarchyMembership {
    pub fn game_objects(&self) -> &[WorkspaceSceneObjectIdentity] {
        &self.game_objects
    }

    pub fn components(&self) -> &[WorkspaceSceneObjectIdentity] {
        &self.components
    }

    pub fn prefab_instances(&self) -> &[WorkspaceSceneObjectIdentity] {
        &self.prefab_instances
    }

    pub fn managers(&self) -> &[WorkspaceSceneObjectIdentity] {
        &self.managers
    }

    pub fn scene_roots(&self) -> Option<WorkspaceSceneObjectIdentity> {
        self.scene_roots
    }

    pub fn issues(&self) -> &[WorkspaceSceneHierarchyMembershipIssue] {
        &self.issues
    }

    pub fn is_complete(&self) -> bool {
        self.issues.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceSceneHierarchyMembershipError {
    AssetLimitExceeded {
        requested: usize,
        limit: usize,
    },
    IssueLimitExceeded {
        requested: usize,
        limit: usize,
    },
    CountOverflow {
        resource: &'static str,
    },
    DuplicateCollectionFileIndex {
        file_index: usize,
    },
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
}

impl Display for WorkspaceSceneHierarchyMembershipError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AssetLimitExceeded { requested, limit } => write!(
                f,
                "Scene hierarchy membership needs {requested} assets/candidates, exceeding limit {limit}"
            ),
            Self::IssueLimitExceeded { requested, limit } => write!(
                f,
                "Scene hierarchy membership has {requested} issues, exceeding limit {limit}"
            ),
            Self::CountOverflow { resource } => {
                write!(f, "Scene hierarchy membership count overflowed for {resource}")
            }
            Self::DuplicateCollectionFileIndex { file_index } => write!(
                f,
                "SceneDefinition repeats workspace collection file index {file_index}"
            ),
            Self::AllocationFailed { resource, requested } => {
                write!(f, "could not allocate {requested} entries for {resource}")
            }
        }
    }
}

impl Error for WorkspaceSceneHierarchyMembershipError {}

fn charge_membership_asset(
    count: &mut usize,
    limit: usize,
) -> Result<(), WorkspaceSceneHierarchyMembershipError> {
    let requested =
        count
            .checked_add(1)
            .ok_or(WorkspaceSceneHierarchyMembershipError::CountOverflow {
                resource: "ordered asset membership",
            })?;
    if requested > limit {
        return Err(WorkspaceSceneHierarchyMembershipError::AssetLimitExceeded {
            requested,
            limit,
        });
    }
    *count = requested;
    Ok(())
}

fn push_membership_asset(
    count: &mut usize,
    assets: &mut Vec<WorkspaceSceneObjectIdentity>,
    identity: WorkspaceSceneObjectIdentity,
    limit: usize,
) -> Result<(), WorkspaceSceneHierarchyMembershipError> {
    let requested =
        count
            .checked_add(1)
            .ok_or(WorkspaceSceneHierarchyMembershipError::CountOverflow {
                resource: "ordered asset membership",
            })?;
    if requested > limit {
        return Err(WorkspaceSceneHierarchyMembershipError::AssetLimitExceeded {
            requested,
            limit,
        });
    }
    assets.try_reserve(1).map_err(|_| {
        WorkspaceSceneHierarchyMembershipError::AllocationFailed {
            resource: "ordered Scene hierarchy category",
            requested,
        }
    })?;
    assets.push(identity);
    *count = requested;
    Ok(())
}

fn push_membership_issue(
    issues: &mut Vec<WorkspaceSceneHierarchyMembershipIssue>,
    issue: WorkspaceSceneHierarchyMembershipIssue,
    limit: usize,
) -> Result<(), WorkspaceSceneHierarchyMembershipError> {
    let requested = issues.len().checked_add(1).ok_or(
        WorkspaceSceneHierarchyMembershipError::CountOverflow {
            resource: "membership diagnostics",
        },
    )?;
    if requested > limit {
        return Err(WorkspaceSceneHierarchyMembershipError::IssueLimitExceeded {
            requested,
            limit,
        });
    }
    issues.try_reserve(1).map_err(|_| {
        WorkspaceSceneHierarchyMembershipError::AllocationFailed {
            resource: "Scene hierarchy membership diagnostics",
            requested,
        }
    })?;
    issues.push(issue);
    Ok(())
}

/// Scene graphs associated with source collection ownership, in SceneDefinition order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorkspaceSceneHierarchies {
    scenes: Vec<WorkspaceSceneHierarchyScene>,
    non_scene_collections: Vec<WorkspaceSceneCollectionHierarchy>,
    collection_graph_index: Vec<WorkspaceSceneCollectionGraphIndexEntry>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WorkspaceSceneCollectionGraphIndexEntry {
    file_index: usize,
    scene_index: Option<usize>,
    collection_index: usize,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct WorkspaceObjectKey {
    file_index: usize,
    object_index: usize,
}

impl From<WorkspaceSceneObjectIdentity> for WorkspaceObjectKey {
    fn from(identity: WorkspaceSceneObjectIdentity) -> Self {
        Self {
            file_index: identity.file_index,
            object_index: identity.object_index,
        }
    }
}

enum WorkspaceHierarchyWalkAction {
    Enter {
        game_object: WorkspaceSceneObjectIdentity,
        depth: usize,
    },
    Exit(WorkspaceObjectKey),
}

impl WorkspaceSceneHierarchies {
    pub fn scenes(&self) -> &[WorkspaceSceneHierarchyScene] {
        &self.scenes
    }

    pub fn non_scene_collections(&self) -> &[WorkspaceSceneCollectionHierarchy] {
        &self.non_scene_collections
    }

    fn collection_graph_for_file(
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

    /// Walks the source `GameObjectExtensions.FetchHierarchy` order across loaded collection files.
    pub fn fetch_game_object_hierarchy(
        &self,
        root: WorkspaceSceneObjectIdentity,
        limits: WorkspaceGameObjectHierarchyWalkLimits,
    ) -> Result<WorkspaceGameObjectHierarchyWalk, WorkspaceGameObjectHierarchyWalkError> {
        if root.class_id != Some(GAME_OBJECT_CLASS_ID) {
            return Err(WorkspaceGameObjectHierarchyWalkError::RootIsNotGameObject {
                identity: root,
            });
        }

        let mut walk = WorkspaceGameObjectHierarchyWalk {
            root,
            entries: Vec::new(),
            issues: Vec::new(),
        };
        let mut actions = Vec::new();
        if limits.max_pending_actions == 0 {
            return Err(
                WorkspaceGameObjectHierarchyWalkError::ResourceLimitExceeded {
                    resource: "pending hierarchy actions",
                    requested: 1,
                    limit: limits.max_pending_actions,
                },
            );
        }
        actions.try_reserve_exact(1).map_err(|_| {
            WorkspaceGameObjectHierarchyWalkError::AllocationFailed {
                resource: "GameObject hierarchy walk actions",
                requested: 1,
            }
        })?;
        actions.push(WorkspaceHierarchyWalkAction::Enter {
            game_object: root,
            depth: 0,
        });
        let mut active = HashSet::<WorkspaceObjectKey>::new();
        let mut visited_graphs = HashSet::<usize>::new();

        while let Some(action) = actions.pop() {
            match action {
                WorkspaceHierarchyWalkAction::Exit(key) => {
                    if !active.remove(&key) {
                        return Err(
                            WorkspaceGameObjectHierarchyWalkError::GraphInvariantFailed {
                                message: format!(
                                    "GameObject {}:{} was exited without an active entry",
                                    key.file_index, key.object_index
                                ),
                            },
                        );
                    }
                }
                WorkspaceHierarchyWalkAction::Enter { game_object, depth } => {
                    if depth > limits.max_depth {
                        return Err(
                            WorkspaceGameObjectHierarchyWalkError::ResourceLimitExceeded {
                                resource: "GameObject hierarchy depth",
                                requested: depth,
                                limit: limits.max_depth,
                            },
                        );
                    }
                    let key = WorkspaceObjectKey::from(game_object);
                    if active.contains(&key) {
                        push_hierarchy_walk_issue(
                            &mut walk,
                            WorkspaceGameObjectHierarchyIssue::CycleDetected { game_object },
                            limits,
                        )?;
                        continue;
                    }
                    let Some(graph) = self.collection_graph_for_file(game_object.file_index) else {
                        push_hierarchy_walk_issue(
                            &mut walk,
                            WorkspaceGameObjectHierarchyIssue::MissingCollectionGraph {
                                identity: game_object,
                            },
                            limits,
                        )?;
                        continue;
                    };
                    note_incomplete_collection_graph(
                        graph,
                        &mut visited_graphs,
                        &mut walk,
                        limits,
                    )?;
                    let Ok(hierarchy_index) = graph
                        .game_object_hierarchies
                        .binary_search_by_key(&game_object.object_index, |hierarchy| {
                            hierarchy.game_object.object_index
                        })
                    else {
                        push_hierarchy_walk_issue(
                            &mut walk,
                            WorkspaceGameObjectHierarchyIssue::MissingGameObjectHierarchy {
                                identity: game_object,
                            },
                            limits,
                        )?;
                        continue;
                    };
                    if graph.game_object_hierarchies[hierarchy_index].game_object != game_object {
                        push_hierarchy_walk_issue(
                            &mut walk,
                            WorkspaceGameObjectHierarchyIssue::MissingGameObjectHierarchy {
                                identity: game_object,
                            },
                            limits,
                        )?;
                        continue;
                    }
                    let requested_active = active.len().checked_add(1).ok_or(
                        WorkspaceGameObjectHierarchyWalkError::CountOverflow {
                            resource: "active GameObject hierarchy path",
                        },
                    )?;
                    active.try_reserve(1).map_err(|_| {
                        WorkspaceGameObjectHierarchyWalkError::AllocationFailed {
                            resource: "active GameObject hierarchy path",
                            requested: requested_active,
                        }
                    })?;
                    active.insert(key);
                    push_hierarchy_walk_entry(
                        &mut walk,
                        WorkspaceGameObjectHierarchyEntry {
                            identity: game_object,
                            kind: WorkspaceGameObjectHierarchyEntryKind::GameObject,
                        },
                        limits,
                    )?;
                    let selected_transform = expand_game_object_components(
                        self,
                        graph,
                        game_object,
                        hierarchy_index,
                        &mut walk,
                        limits,
                    )?;
                    let children = if let Some(transform) = selected_transform {
                        collect_hierarchy_children(
                            self,
                            transform,
                            &mut walk,
                            &mut visited_graphs,
                            limits,
                        )?
                    } else {
                        Vec::new()
                    };

                    let next_action_count = actions
                        .len()
                        .checked_add(children.len())
                        .and_then(|count| count.checked_add(1))
                        .ok_or(WorkspaceGameObjectHierarchyWalkError::CountOverflow {
                            resource: "pending hierarchy actions",
                        })?;
                    if next_action_count > limits.max_pending_actions {
                        return Err(
                            WorkspaceGameObjectHierarchyWalkError::ResourceLimitExceeded {
                                resource: "pending hierarchy actions",
                                requested: next_action_count,
                                limit: limits.max_pending_actions,
                            },
                        );
                    }
                    let additional_actions = children.len().checked_add(1).ok_or(
                        WorkspaceGameObjectHierarchyWalkError::CountOverflow {
                            resource: "pending hierarchy actions",
                        },
                    )?;
                    actions.try_reserve(additional_actions).map_err(|_| {
                        WorkspaceGameObjectHierarchyWalkError::AllocationFailed {
                            resource: "GameObject hierarchy walk actions",
                            requested: next_action_count,
                        }
                    })?;
                    actions.push(WorkspaceHierarchyWalkAction::Exit(key));
                    let child_depth = depth.checked_add(1).ok_or(
                        WorkspaceGameObjectHierarchyWalkError::CountOverflow {
                            resource: "GameObject hierarchy depth",
                        },
                    )?;
                    for child in children.into_iter().rev() {
                        actions.push(WorkspaceHierarchyWalkAction::Enter {
                            game_object: child,
                            depth: child_depth,
                        });
                    }
                }
            }
        }
        Ok(walk)
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
    /// `AddMissingTransforms` pass has supplied its generated Transform.
    pub fn get_root_game_object(
        &self,
        game_object: WorkspaceSceneObjectIdentity,
        limits: WorkspaceGetRootGameObjectLimits,
    ) -> Result<WorkspaceGetRootGameObjectWalk, WorkspaceGetRootGameObjectError> {
        let mut walk = WorkspaceGetRootGameObjectWalk {
            source_game_object: game_object,
            first_transform: None,
            transform_chain: Vec::new(),
            root_game_object: None,
            termination: None,
            issue: None,
        };
        if game_object.class_id != Some(GAME_OBJECT_CLASS_ID) {
            return Err(WorkspaceGetRootGameObjectError::GraphInvariantFailed {
                message: format!(
                    "source identity {}:{} is not a GameObject",
                    game_object.file_index, game_object.object_index
                ),
            });
        }
        let Some(graph) = self.collection_graph_for_file(game_object.file_index) else {
            walk.issue = Some(WorkspaceGetRootGameObjectIssue::MissingCollectionGraph);
            return Ok(walk);
        };
        if !graph.source_graph_complete() || !graph.source_reference_scans_complete() {
            walk.issue = Some(WorkspaceGetRootGameObjectIssue::IncompleteCollectionGraph {
                file_index: game_object.file_index,
            });
            return Ok(walk);
        }
        let Ok(game_object_index) = graph
            .game_object_hierarchies
            .binary_search_by_key(&game_object.object_index, |hierarchy| {
                hierarchy.game_object.object_index
            })
        else {
            walk.issue = Some(WorkspaceGetRootGameObjectIssue::MissingGameObjectHierarchy);
            return Ok(walk);
        };
        if graph.game_object_hierarchies[game_object_index].game_object != game_object {
            walk.issue = Some(WorkspaceGetRootGameObjectIssue::MissingGameObjectHierarchy);
            return Ok(walk);
        }

        let component_relationships = graph
            .game_object_component_relationships(game_object_index)
            .map_err(
                |error| WorkspaceGetRootGameObjectError::GraphInvariantFailed {
                    message: error.to_string(),
                },
            )?;
        let mut selected_transform = None;
        for (relationship_index, relationship) in component_relationships.enumerate() {
            let relationship_index = graph
                .game_object_component_relationship_indices
                .get(
                    graph.game_object_hierarchies[game_object_index].component_relationship_start
                        + relationship_index,
                )
                .copied()
                .ok_or_else(|| WorkspaceGetRootGameObjectError::GraphInvariantFailed {
                    message: "GameObject component index range changed during iteration".to_owned(),
                })?;
            if relationship.kind != SceneRelationshipKind::GameObjectComponent
                || relationship.source_object_index != game_object.object_index
                || relationship.source_path_id != game_object.path_id
            {
                return Err(WorkspaceGetRootGameObjectError::GraphInvariantFailed {
                    message: format!(
                        "GameObject {}:{} component index points to an unrelated relationship",
                        game_object.file_index, game_object.object_index
                    ),
                });
            }
            match resolve_relationship_identity(self, graph, relationship) {
                RelationshipIdentityResolution::Null => {}
                RelationshipIdentityResolution::Resolved(component)
                    if matches!(
                        component.class_id,
                        Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)
                    ) =>
                {
                    selected_transform = Some(component);
                    break;
                }
                RelationshipIdentityResolution::Resolved(component)
                    if component.class_id.is_some() => {}
                RelationshipIdentityResolution::Resolved(component) => {
                    walk.issue = Some(WorkspaceGetRootGameObjectIssue::UnknownComponentClass {
                        relationship_index,
                        component,
                    });
                    return Ok(walk);
                }
                RelationshipIdentityResolution::Unresolved(target) => {
                    walk.issue = Some(
                        WorkspaceGetRootGameObjectIssue::UnresolvedComponentReference {
                            relationship_index,
                            target,
                        },
                    );
                    return Ok(walk);
                }
                RelationshipIdentityResolution::Unavailable {
                    target_file_index,
                    target_object_index,
                    path_id,
                } => {
                    walk.issue = Some(
                        WorkspaceGetRootGameObjectIssue::ComponentIdentityUnavailable {
                            relationship_index,
                            target_file_index,
                            target_object_index,
                            path_id,
                        },
                    );
                    return Ok(walk);
                }
                RelationshipIdentityResolution::PathMismatch {
                    identity,
                    expected_path_id,
                } => {
                    walk.issue = Some(WorkspaceGetRootGameObjectIssue::ComponentPathIdMismatch {
                        relationship_index,
                        expected_path_id,
                        actual_path_id: identity.path_id,
                    });
                    return Ok(walk);
                }
            }
        }

        let Some(first_transform) = selected_transform else {
            walk.root_game_object = Some(game_object);
            walk.termination = Some(WorkspaceGetRootGameObjectTermination::NoTransformComponent);
            return Ok(walk);
        };
        walk.first_transform = Some(first_transform);
        walk.transform_chain.try_reserve(1).map_err(|_| {
            WorkspaceGetRootGameObjectError::AllocationFailed {
                resource: "GetRoot Transform chain",
                requested: 1,
            }
        })?;
        walk.transform_chain.push(first_transform);
        let mut visited = HashSet::new();
        visited
            .try_reserve(1)
            .map_err(|_| WorkspaceGetRootGameObjectError::AllocationFailed {
                resource: "GetRoot Transform cycle set",
                requested: 1,
            })?;
        visited.insert(WorkspaceObjectKey::from(first_transform));
        let mut current_transform = first_transform;

        loop {
            if walk.transform_chain.len() > limits.max_parent_depth {
                return Err(WorkspaceGetRootGameObjectError::ParentDepthLimitExceeded {
                    requested: walk.transform_chain.len(),
                    limit: limits.max_parent_depth,
                });
            }
            let Some(current_graph) = self.collection_graph_for_file(current_transform.file_index)
            else {
                walk.issue = Some(WorkspaceGetRootGameObjectIssue::MissingCollectionGraph);
                return Ok(walk);
            };
            if !current_graph.source_graph_complete()
                || !current_graph.source_reference_scans_complete()
            {
                walk.issue = Some(WorkspaceGetRootGameObjectIssue::IncompleteCollectionGraph {
                    file_index: current_transform.file_index,
                });
                return Ok(walk);
            }
            let Ok(transform_index) = current_graph
                .transform_hierarchies
                .binary_search_by_key(&current_transform.object_index, |hierarchy| {
                    hierarchy.transform.object_index
                })
            else {
                walk.issue = Some(WorkspaceGetRootGameObjectIssue::MissingTransformHierarchy {
                    transform: current_transform,
                });
                return Ok(walk);
            };
            if current_graph.transform_hierarchies[transform_index].transform != current_transform {
                walk.issue = Some(WorkspaceGetRootGameObjectIssue::MissingTransformHierarchy {
                    transform: current_transform,
                });
                return Ok(walk);
            }
            let parent_relationships = current_graph
                .transform_parent_relationships(transform_index)
                .map_err(
                    |error| WorkspaceGetRootGameObjectError::GraphInvariantFailed {
                        message: error.to_string(),
                    },
                )?;
            let mut parent_relationships = parent_relationships.peekable();
            let Some(parent_relationship) = parent_relationships.next() else {
                walk.issue = Some(
                    WorkspaceGetRootGameObjectIssue::MissingTransformParentField {
                        transform: current_transform,
                    },
                );
                return Ok(walk);
            };
            let extra_parent_count = parent_relationships.count();
            if extra_parent_count > 0 {
                walk.issue = Some(
                    WorkspaceGetRootGameObjectIssue::DuplicateTransformParentFields {
                        transform: current_transform,
                        match_count: extra_parent_count + 1,
                    },
                );
                return Ok(walk);
            }
            if parent_relationship.kind != SceneRelationshipKind::TransformParent
                || parent_relationship.source_object_index != current_transform.object_index
                || parent_relationship.source_path_id != current_transform.path_id
            {
                return Err(WorkspaceGetRootGameObjectError::GraphInvariantFailed {
                    message: format!(
                        "Transform {}:{} parent index points to an unrelated relationship",
                        current_transform.file_index, current_transform.object_index
                    ),
                });
            }
            let parent_transform =
                match resolve_relationship_identity(self, current_graph, parent_relationship) {
                    RelationshipIdentityResolution::Null => {
                        walk.termination = Some(WorkspaceGetRootGameObjectTermination::NullFather);
                        None
                    }
                    RelationshipIdentityResolution::Resolved(parent)
                        if matches!(
                            parent.class_id,
                            Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)
                        ) =>
                    {
                        match resolve_transform_game_object(self, parent, &mut walk)? {
                            Some(_) => Some(parent),
                            None if walk.issue.is_none() => {
                                walk.termination = Some(
                                    WorkspaceGetRootGameObjectTermination::ParentHasNullGameObject,
                                );
                                None
                            }
                            None => return Ok(walk),
                        }
                    }
                    RelationshipIdentityResolution::Resolved(target) => {
                        walk.issue = Some(
                            WorkspaceGetRootGameObjectIssue::TransformParentClassMismatch {
                                target,
                            },
                        );
                        return Ok(walk);
                    }
                    RelationshipIdentityResolution::Unresolved(target) => {
                        walk.issue =
                            Some(WorkspaceGetRootGameObjectIssue::UnresolvedTransformParent {
                                transform: current_transform,
                                target,
                            });
                        return Ok(walk);
                    }
                    RelationshipIdentityResolution::Unavailable {
                        target_file_index,
                        target_object_index,
                        path_id,
                    } => {
                        walk.issue = Some(
                            WorkspaceGetRootGameObjectIssue::TransformParentIdentityUnavailable {
                                transform: current_transform,
                                target_file_index,
                                target_object_index,
                                path_id,
                            },
                        );
                        return Ok(walk);
                    }
                    RelationshipIdentityResolution::PathMismatch {
                        identity,
                        expected_path_id,
                    } => {
                        walk.issue = Some(
                            WorkspaceGetRootGameObjectIssue::TransformParentPathIdMismatch {
                                transform: current_transform,
                                expected_path_id,
                                actual_path_id: identity.path_id,
                            },
                        );
                        return Ok(walk);
                    }
                };

            let Some(parent_transform) = parent_transform else {
                break;
            };
            let key = WorkspaceObjectKey::from(parent_transform);
            if visited.contains(&key) {
                walk.issue = Some(WorkspaceGetRootGameObjectIssue::CycleDetected {
                    transform: parent_transform,
                });
                return Ok(walk);
            }
            let requested_depth = walk.transform_chain.len().checked_add(1).ok_or(
                WorkspaceGetRootGameObjectError::CountOverflow {
                    resource: "GetRoot Transform chain",
                },
            )?;
            if requested_depth > limits.max_parent_depth {
                return Err(WorkspaceGetRootGameObjectError::ParentDepthLimitExceeded {
                    requested: requested_depth,
                    limit: limits.max_parent_depth,
                });
            }
            walk.transform_chain.try_reserve(1).map_err(|_| {
                WorkspaceGetRootGameObjectError::AllocationFailed {
                    resource: "GetRoot Transform chain",
                    requested: requested_depth,
                }
            })?;
            visited.try_reserve(1).map_err(|_| {
                WorkspaceGetRootGameObjectError::AllocationFailed {
                    resource: "GetRoot Transform cycle set",
                    requested: visited.len().saturating_add(1),
                }
            })?;
            visited.insert(key);
            walk.transform_chain.push(parent_transform);
            current_transform = parent_transform;
        }

        if walk.issue.is_none() {
            if let Some(terminal_transform) = walk.transform_chain.last().copied() {
                walk.root_game_object =
                    resolve_transform_game_object(self, terminal_transform, &mut walk)?;
                if walk.root_game_object.is_none() && walk.issue.is_none() {
                    walk.issue = Some(WorkspaceGetRootGameObjectIssue::TransformGameObjectNull {
                        transform: terminal_transform,
                    });
                }
            }
        }
        Ok(walk)
    }

    /// Resolves one serialized `PrefabInstance.m_RootGameObject` edge against retained graphs.
    /// A missing or malformed field remains `FieldUnavailable`; it is never converted to null.
    pub fn resolve_prefab_instance_root(
        &self,
        prefab_instance: WorkspaceSceneObjectIdentity,
    ) -> Result<WorkspacePrefabInstanceRootResolution, WorkspacePrefabInstanceRootError> {
        if prefab_instance.class_id != Some(PREFAB_INSTANCE_CLASS_ID) {
            return Err(WorkspacePrefabInstanceRootError::NotPrefabInstance {
                identity: prefab_instance,
            });
        }
        let Some(graph) = self.collection_graph_for_file(prefab_instance.file_index) else {
            return Err(WorkspacePrefabInstanceRootError::MissingCollectionGraph {
                file_index: prefab_instance.file_index,
            });
        };
        let key = (prefab_instance.file_index, prefab_instance.object_index);
        let has_prefab_instance = graph
            .scene_assets
            .binary_search_by_key(&key, |asset| {
                (asset.identity.file_index, asset.identity.object_index)
            })
            .ok()
            .and_then(|index| graph.scene_assets.get(index))
            .filter(|asset| {
                asset.category == WorkspaceSceneAssetCategory::PrefabInstance
                    && asset.identity == prefab_instance
            })
            .is_some();
        if !has_prefab_instance {
            return Err(WorkspacePrefabInstanceRootError::MissingPrefabInstance {
                identity: prefab_instance,
            });
        }
        let relationships =
            graph.prefab_root_relationships_for_object(prefab_instance.object_index);
        if relationships.is_empty() {
            return Ok(WorkspacePrefabInstanceRootResolution::FieldUnavailable);
        }
        if relationships.len() > 1 {
            return Ok(
                WorkspacePrefabInstanceRootResolution::DuplicateRootReferences {
                    match_count: relationships.len(),
                },
            );
        }
        let relationship_index = relationships[0].relationship_index;
        let Some(relationship) = graph.relationships.get(relationship_index) else {
            return Err(WorkspacePrefabInstanceRootError::InvalidRelationshipIndex {
                relationship_index,
                relationship_count: graph.relationships.len(),
            });
        };
        if relationship.source_object_index != prefab_instance.object_index
            || relationship.kind != SceneRelationshipKind::PrefabInstanceRootGameObject
            || relationship.source_path_id != prefab_instance.path_id
        {
            return Err(
                WorkspacePrefabInstanceRootError::RelationshipIndexMismatch {
                    expected_object_index: prefab_instance.object_index,
                    relationship_index,
                    actual_object_index: relationship.source_object_index,
                    kind: relationship.kind,
                },
            );
        }
        let resolution = match resolve_relationship_identity(self, graph, relationship) {
            RelationshipIdentityResolution::Null => WorkspacePrefabInstanceRootResolution::Null,
            RelationshipIdentityResolution::Resolved(identity)
                if identity.class_id == Some(GAME_OBJECT_CLASS_ID) =>
            {
                WorkspacePrefabInstanceRootResolution::Resolved(identity)
            }
            RelationshipIdentityResolution::Resolved(identity) => {
                WorkspacePrefabInstanceRootResolution::TargetClassMismatch {
                    target: identity,
                    actual_class_id: identity.class_id,
                }
            }
            RelationshipIdentityResolution::Unresolved(target) => {
                WorkspacePrefabInstanceRootResolution::UnresolvedTarget(target)
            }
            RelationshipIdentityResolution::Unavailable {
                target_file_index,
                target_object_index,
                path_id,
            } => WorkspacePrefabInstanceRootResolution::TargetIdentityUnavailable {
                target_file_index,
                target_object_index,
                path_id,
            },
            RelationshipIdentityResolution::PathMismatch {
                identity,
                expected_path_id,
            } => WorkspacePrefabInstanceRootResolution::TargetIdentityPathMismatch {
                expected_path_id,
                actual_path_id: identity.path_id,
            },
        };
        Ok(resolution)
    }

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

enum RelationshipIdentityResolution {
    Null,
    Resolved(WorkspaceSceneObjectIdentity),
    Unresolved(SceneRelationshipTarget),
    Unavailable {
        target_file_index: usize,
        target_object_index: usize,
        path_id: i64,
    },
    PathMismatch {
        identity: WorkspaceSceneObjectIdentity,
        expected_path_id: i64,
    },
}

fn note_incomplete_collection_graph(
    graph: &WorkspaceSceneCollectionHierarchy,
    visited_graphs: &mut HashSet<usize>,
    walk: &mut WorkspaceGameObjectHierarchyWalk,
    limits: WorkspaceGameObjectHierarchyWalkLimits,
) -> Result<(), WorkspaceGameObjectHierarchyWalkError> {
    let file_index = graph.collection_file_index();
    if visited_graphs.contains(&file_index) {
        return Ok(());
    }
    let requested_graphs = visited_graphs.len().checked_add(1).ok_or(
        WorkspaceGameObjectHierarchyWalkError::CountOverflow {
            resource: "hierarchy graph visitation index",
        },
    )?;
    visited_graphs.try_reserve(1).map_err(|_| {
        WorkspaceGameObjectHierarchyWalkError::AllocationFailed {
            resource: "hierarchy graph visitation index",
            requested: requested_graphs,
        }
    })?;
    visited_graphs.insert(file_index);
    let root_discovery_complete = graph.roots().is_complete();
    let transform_audit_complete = graph.hierarchy_audit().is_complete();
    if graph.source_graph_complete()
        && graph.source_reference_scans_complete()
        && root_discovery_complete
        && transform_audit_complete
    {
        return Ok(());
    }
    push_hierarchy_walk_issue(
        walk,
        WorkspaceGameObjectHierarchyIssue::IncompleteCollectionGraph {
            file_index,
            source_graph_complete: graph.source_graph_complete(),
            reference_scans_complete: graph.source_reference_scans_complete(),
            root_discovery_complete,
            transform_audit_complete,
            graph_issue_count: graph.graph_issues().len(),
            root_issue_count: graph.roots().issues().len(),
            transform_audit_issue_count: graph.hierarchy_audit().issues().len(),
        },
        limits,
    )
}

fn push_hierarchy_walk_entry(
    walk: &mut WorkspaceGameObjectHierarchyWalk,
    entry: WorkspaceGameObjectHierarchyEntry,
    limits: WorkspaceGameObjectHierarchyWalkLimits,
) -> Result<(), WorkspaceGameObjectHierarchyWalkError> {
    let requested = walk.entries.len().checked_add(1).ok_or(
        WorkspaceGameObjectHierarchyWalkError::CountOverflow {
            resource: "GameObject hierarchy entries",
        },
    )?;
    if requested > limits.max_entries {
        return Err(
            WorkspaceGameObjectHierarchyWalkError::ResourceLimitExceeded {
                resource: "GameObject hierarchy entries",
                requested,
                limit: limits.max_entries,
            },
        );
    }
    walk.entries.try_reserve(1).map_err(|_| {
        WorkspaceGameObjectHierarchyWalkError::AllocationFailed {
            resource: "GameObject hierarchy entries",
            requested,
        }
    })?;
    walk.entries.push(entry);
    Ok(())
}

fn push_hierarchy_walk_issue(
    walk: &mut WorkspaceGameObjectHierarchyWalk,
    issue: WorkspaceGameObjectHierarchyIssue,
    limits: WorkspaceGameObjectHierarchyWalkLimits,
) -> Result<(), WorkspaceGameObjectHierarchyWalkError> {
    let requested = walk.issues.len().checked_add(1).ok_or(
        WorkspaceGameObjectHierarchyWalkError::CountOverflow {
            resource: "GameObject hierarchy issues",
        },
    )?;
    if requested > limits.max_issues {
        return Err(
            WorkspaceGameObjectHierarchyWalkError::ResourceLimitExceeded {
                resource: "GameObject hierarchy issues",
                requested,
                limit: limits.max_issues,
            },
        );
    }
    walk.issues.try_reserve(1).map_err(|_| {
        WorkspaceGameObjectHierarchyWalkError::AllocationFailed {
            resource: "GameObject hierarchy issues",
            requested,
        }
    })?;
    walk.issues.push(issue);
    Ok(())
}

fn workspace_object_identity(
    graph: &WorkspaceSceneCollectionHierarchy,
    file_index: usize,
    object_index: usize,
) -> Option<WorkspaceSceneObjectIdentity> {
    let key = (file_index, object_index);
    graph
        .scene_assets()
        .binary_search_by_key(&key, |asset| {
            (asset.identity.file_index, asset.identity.object_index)
        })
        .ok()
        .and_then(|index| graph.scene_assets().get(index))
        .map(|asset| asset.identity)
        .or_else(|| {
            graph
                .component_candidates()
                .binary_search_by_key(&key, |candidate| {
                    (
                        candidate.identity.file_index,
                        candidate.identity.object_index,
                    )
                })
                .ok()
                .and_then(|index| graph.component_candidates().get(index))
                .map(|candidate| candidate.identity)
        })
        .or_else(|| {
            if file_index != graph.collection_file_index() {
                return None;
            }
            graph
                .scene_objects()
                .binary_search_by_key(&object_index, |object| object.object_index)
                .ok()
                .and_then(|index| graph.scene_objects().get(index))
                .map(|object| WorkspaceSceneObjectIdentity {
                    file_index,
                    object_index,
                    path_id: object.path_id,
                    class_id: object.class_id,
                })
        })
}

fn resolve_relationship_identity(
    workspace: &WorkspaceSceneHierarchies,
    source_graph: &WorkspaceSceneCollectionHierarchy,
    relationship: &SceneRelationship,
) -> RelationshipIdentityResolution {
    let (target_file_index, target_object_index) = match &relationship.target {
        SceneRelationshipTarget::Null => return RelationshipIdentityResolution::Null,
        SceneRelationshipTarget::LocalObject { object_index } => {
            (source_graph.collection_file_index(), *object_index)
        }
        SceneRelationshipTarget::WorkspaceObject {
            file_index,
            object_index,
        } => (*file_index, *object_index),
        target => return RelationshipIdentityResolution::Unresolved(target.clone()),
    };
    let identity = workspace_object_identity(source_graph, target_file_index, target_object_index)
        .or_else(|| {
            workspace
                .collection_graph_for_file(target_file_index)
                .and_then(|graph| {
                    workspace_object_identity(graph, target_file_index, target_object_index)
                })
        });
    let Some(identity) = identity else {
        return RelationshipIdentityResolution::Unavailable {
            target_file_index,
            target_object_index,
            path_id: relationship.path_id,
        };
    };
    if identity.path_id != relationship.path_id {
        return RelationshipIdentityResolution::PathMismatch {
            identity,
            expected_path_id: relationship.path_id,
        };
    }
    RelationshipIdentityResolution::Resolved(identity)
}

fn graph_index_walk_error(
    error: WorkspaceSceneHierarchyError,
) -> WorkspaceGameObjectHierarchyWalkError {
    WorkspaceGameObjectHierarchyWalkError::GraphInvariantFailed {
        message: error.to_string(),
    }
}

fn expand_game_object_components(
    workspace: &WorkspaceSceneHierarchies,
    graph: &WorkspaceSceneCollectionHierarchy,
    game_object: WorkspaceSceneObjectIdentity,
    hierarchy_index: usize,
    walk: &mut WorkspaceGameObjectHierarchyWalk,
    limits: WorkspaceGameObjectHierarchyWalkLimits,
) -> Result<Option<WorkspaceSceneObjectIdentity>, WorkspaceGameObjectHierarchyWalkError> {
    let hierarchy = graph
        .game_object_hierarchies
        .get(hierarchy_index)
        .ok_or_else(|| {
            graph_index_walk_error(
                WorkspaceSceneHierarchyError::GameObjectHierarchyIndexOutOfBounds {
                    hierarchy_index,
                    hierarchy_count: graph.game_object_hierarchies.len(),
                },
            )
        })?;
    let indexes = graph
        .game_object_component_relationship_indices
        .get(hierarchy.component_relationship_start..hierarchy.component_relationship_end)
        .ok_or_else(|| {
            graph_index_walk_error(
                WorkspaceSceneHierarchyError::InvalidGameObjectComponentRelationshipRange {
                    hierarchy_index,
                    start: hierarchy.component_relationship_start,
                    end: hierarchy.component_relationship_end,
                    index_count: graph.game_object_component_relationship_indices.len(),
                },
            )
        })?;

    let mut selected_transform = None;
    for &relationship_index in indexes {
        let relationship = graph
            .relationships()
            .get(relationship_index)
            .ok_or_else(|| {
                graph_index_walk_error(
                    WorkspaceSceneHierarchyError::InvalidGameObjectComponentRelationshipIndex {
                        hierarchy_index,
                        relationship_index,
                        relationship_count: graph.relationships().len(),
                    },
                )
            })?;
        if relationship.kind != SceneRelationshipKind::GameObjectComponent {
            return Err(
                WorkspaceGameObjectHierarchyWalkError::GraphInvariantFailed {
                    message: format!(
                        "GameObject {}:{} index points at {:?}",
                        game_object.file_index, game_object.object_index, relationship.kind
                    ),
                },
            );
        }
        match resolve_relationship_identity(workspace, graph, relationship) {
            RelationshipIdentityResolution::Null => {}
            RelationshipIdentityResolution::Resolved(component) => {
                if component.class_id.is_none() {
                    push_hierarchy_walk_issue(
                        walk,
                        WorkspaceGameObjectHierarchyIssue::UnknownComponentClass {
                            game_object,
                            relationship_index,
                            component,
                        },
                        limits,
                    )?;
                }
                push_hierarchy_walk_entry(
                    walk,
                    WorkspaceGameObjectHierarchyEntry {
                        identity: component,
                        kind: WorkspaceGameObjectHierarchyEntryKind::Component,
                    },
                    limits,
                )?;
                if matches!(
                    component.class_id,
                    Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)
                ) {
                    selected_transform = Some(component);
                }
            }
            RelationshipIdentityResolution::Unresolved(target) => {
                push_hierarchy_walk_issue(
                    walk,
                    WorkspaceGameObjectHierarchyIssue::UnresolvedComponentReference {
                        game_object,
                        relationship_index,
                        target,
                    },
                    limits,
                )?;
            }
            RelationshipIdentityResolution::Unavailable {
                target_file_index,
                target_object_index,
                path_id,
            } => {
                push_hierarchy_walk_issue(
                    walk,
                    WorkspaceGameObjectHierarchyIssue::ComponentIdentityUnavailable {
                        game_object,
                        relationship_index,
                        target_file_index,
                        target_object_index,
                        path_id,
                    },
                    limits,
                )?;
            }
            RelationshipIdentityResolution::PathMismatch {
                identity,
                expected_path_id,
            } => {
                push_hierarchy_walk_issue(
                    walk,
                    WorkspaceGameObjectHierarchyIssue::TargetIdentityPathMismatch {
                        source: game_object,
                        relationship_index,
                        reference_kind: WorkspaceGameObjectHierarchyReferenceKind::Component,
                        expected_path_id,
                        actual_path_id: identity.path_id,
                    },
                    limits,
                )?;
            }
        }
    }
    Ok(selected_transform)
}

fn collect_hierarchy_children(
    workspace: &WorkspaceSceneHierarchies,
    transform: WorkspaceSceneObjectIdentity,
    walk: &mut WorkspaceGameObjectHierarchyWalk,
    visited_graphs: &mut HashSet<usize>,
    limits: WorkspaceGameObjectHierarchyWalkLimits,
) -> Result<Vec<WorkspaceSceneObjectIdentity>, WorkspaceGameObjectHierarchyWalkError> {
    let mut children = Vec::new();
    let Some(graph) = workspace.collection_graph_for_file(transform.file_index) else {
        push_hierarchy_walk_issue(
            walk,
            WorkspaceGameObjectHierarchyIssue::MissingCollectionGraph {
                identity: transform,
            },
            limits,
        )?;
        return Ok(children);
    };
    note_incomplete_collection_graph(graph, visited_graphs, walk, limits)?;
    let Ok(transform_index) = graph
        .transform_hierarchies
        .binary_search_by_key(&transform.object_index, |hierarchy| {
            hierarchy.transform.object_index
        })
    else {
        push_hierarchy_walk_issue(
            walk,
            WorkspaceGameObjectHierarchyIssue::MissingTransformHierarchy { transform },
            limits,
        )?;
        return Ok(children);
    };
    if graph.transform_hierarchies[transform_index].transform != transform {
        push_hierarchy_walk_issue(
            walk,
            WorkspaceGameObjectHierarchyIssue::MissingTransformHierarchy { transform },
            limits,
        )?;
        return Ok(children);
    }
    let transform_hierarchy = &graph.transform_hierarchies[transform_index];
    let child_indexes = graph
        .transform_child_relationship_indices
        .get(
            transform_hierarchy.child_relationship_start
                ..transform_hierarchy.child_relationship_end,
        )
        .ok_or_else(|| {
            graph_index_walk_error(
                WorkspaceSceneHierarchyError::InvalidTransformChildRelationshipRange {
                    hierarchy_index: transform_index,
                    start: transform_hierarchy.child_relationship_start,
                    end: transform_hierarchy.child_relationship_end,
                    index_count: graph.transform_child_relationship_indices.len(),
                },
            )
        })?;
    children
        .try_reserve_exact(child_indexes.len())
        .map_err(
            |_| WorkspaceGameObjectHierarchyWalkError::AllocationFailed {
                resource: "ordered GameObject hierarchy children",
                requested: child_indexes.len(),
            },
        )?;

    for &child_relationship_index in child_indexes {
        let child_relationship = graph
            .relationships()
            .get(child_relationship_index)
            .ok_or_else(|| {
                graph_index_walk_error(
                    WorkspaceSceneHierarchyError::InvalidTransformRelationshipIndex {
                        hierarchy_index: transform_index,
                        relationship_index: child_relationship_index,
                        relationship_count: graph.relationships().len(),
                    },
                )
            })?;
        if child_relationship.kind != SceneRelationshipKind::TransformChild {
            return Err(
                WorkspaceGameObjectHierarchyWalkError::GraphInvariantFailed {
                    message: format!(
                        "Transform {}:{} child index points at {:?}",
                        transform.file_index, transform.object_index, child_relationship.kind
                    ),
                },
            );
        }
        let child_transform =
            match resolve_relationship_identity(workspace, graph, child_relationship) {
                RelationshipIdentityResolution::Null => continue,
                RelationshipIdentityResolution::Resolved(identity) => identity,
                RelationshipIdentityResolution::Unresolved(target) => {
                    push_hierarchy_walk_issue(
                        walk,
                        WorkspaceGameObjectHierarchyIssue::UnresolvedChildTransformReference {
                            transform,
                            relationship_index: child_relationship_index,
                            target,
                        },
                        limits,
                    )?;
                    continue;
                }
                RelationshipIdentityResolution::Unavailable {
                    target_file_index,
                    target_object_index,
                    path_id,
                } => {
                    push_hierarchy_walk_issue(
                        walk,
                        WorkspaceGameObjectHierarchyIssue::ChildTransformIdentityUnavailable {
                            transform,
                            relationship_index: child_relationship_index,
                            target_file_index,
                            target_object_index,
                            path_id,
                        },
                        limits,
                    )?;
                    continue;
                }
                RelationshipIdentityResolution::PathMismatch {
                    identity,
                    expected_path_id,
                } => {
                    push_hierarchy_walk_issue(
                        walk,
                        WorkspaceGameObjectHierarchyIssue::TargetIdentityPathMismatch {
                            source: transform,
                            relationship_index: child_relationship_index,
                            reference_kind:
                                WorkspaceGameObjectHierarchyReferenceKind::ChildTransform,
                            expected_path_id,
                            actual_path_id: identity.path_id,
                        },
                        limits,
                    )?;
                    continue;
                }
            };
        if !matches!(
            child_transform.class_id,
            Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)
        ) {
            push_hierarchy_walk_issue(
                walk,
                WorkspaceGameObjectHierarchyIssue::ChildTargetIsNotTransform {
                    transform,
                    relationship_index: child_relationship_index,
                    child: child_transform,
                },
                limits,
            )?;
            continue;
        }
        let Some(child_graph) = workspace.collection_graph_for_file(child_transform.file_index)
        else {
            push_hierarchy_walk_issue(
                walk,
                WorkspaceGameObjectHierarchyIssue::MissingCollectionGraph {
                    identity: child_transform,
                },
                limits,
            )?;
            continue;
        };
        note_incomplete_collection_graph(child_graph, visited_graphs, walk, limits)?;
        let Ok(child_transform_index) = child_graph
            .transform_hierarchies
            .binary_search_by_key(&child_transform.object_index, |hierarchy| {
                hierarchy.transform.object_index
            })
        else {
            push_hierarchy_walk_issue(
                walk,
                WorkspaceGameObjectHierarchyIssue::MissingTransformHierarchy {
                    transform: child_transform,
                },
                limits,
            )?;
            continue;
        };
        if child_graph.transform_hierarchies[child_transform_index].transform != child_transform {
            push_hierarchy_walk_issue(
                walk,
                WorkspaceGameObjectHierarchyIssue::MissingTransformHierarchy {
                    transform: child_transform,
                },
                limits,
            )?;
            continue;
        }
        let child_transform_hierarchy = &child_graph.transform_hierarchies[child_transform_index];
        let owner_indexes = child_graph
            .transform_game_object_relationship_indices
            .get(
                child_transform_hierarchy.game_object_relationship_start
                    ..child_transform_hierarchy.game_object_relationship_end,
            )
            .ok_or_else(|| {
                graph_index_walk_error(
                    WorkspaceSceneHierarchyError::InvalidTransformGameObjectRelationshipRange {
                        hierarchy_index: child_transform_index,
                        start: child_transform_hierarchy.game_object_relationship_start,
                        end: child_transform_hierarchy.game_object_relationship_end,
                        index_count: child_graph.transform_game_object_relationship_indices.len(),
                    },
                )
            })?;
        match owner_indexes {
            [] => {
                push_hierarchy_walk_issue(
                    walk,
                    WorkspaceGameObjectHierarchyIssue::MissingTransformGameObjectReference {
                        transform: child_transform,
                    },
                    limits,
                )?;
            }
            [owner_relationship_index] => {
                let owner_relationship = child_graph
                    .relationships()
                    .get(*owner_relationship_index)
                    .ok_or_else(|| {
                        graph_index_walk_error(
                            WorkspaceSceneHierarchyError::InvalidTransformRelationshipIndex {
                                hierarchy_index: child_transform_index,
                                relationship_index: *owner_relationship_index,
                                relationship_count: child_graph.relationships().len(),
                            },
                        )
                    })?;
                if owner_relationship.kind != SceneRelationshipKind::ComponentGameObject {
                    return Err(
                        WorkspaceGameObjectHierarchyWalkError::GraphInvariantFailed {
                            message: format!(
                                "Transform {}:{} owner index points at {:?}",
                                child_transform.file_index,
                                child_transform.object_index,
                                owner_relationship.kind
                            ),
                        },
                    );
                }
                match resolve_relationship_identity(workspace, child_graph, owner_relationship) {
                    RelationshipIdentityResolution::Null => {}
                    RelationshipIdentityResolution::Resolved(owner) => {
                        if owner.class_id != Some(GAME_OBJECT_CLASS_ID) {
                            push_hierarchy_walk_issue(
                                walk,
                                WorkspaceGameObjectHierarchyIssue::TransformOwnerIsNotGameObject {
                                    transform: child_transform,
                                    relationship_index: *owner_relationship_index,
                                    target: owner,
                                },
                                limits,
                            )?;
                        } else {
                            children.push(owner);
                        }
                    }
                    RelationshipIdentityResolution::Unresolved(target) => {
                        push_hierarchy_walk_issue(
                            walk,
                            WorkspaceGameObjectHierarchyIssue::UnresolvedTransformGameObjectReference {
                                transform: child_transform,
                                relationship_index: *owner_relationship_index,
                                target,
                            },
                            limits,
                        )?;
                    }
                    RelationshipIdentityResolution::Unavailable {
                        target_file_index,
                        target_object_index,
                        path_id,
                    } => {
                        push_hierarchy_walk_issue(
                            walk,
                            WorkspaceGameObjectHierarchyIssue::GameObjectOwnerIdentityUnavailable {
                                transform: child_transform,
                                relationship_index: *owner_relationship_index,
                                target_file_index,
                                target_object_index,
                                path_id,
                            },
                            limits,
                        )?;
                    }
                    RelationshipIdentityResolution::PathMismatch {
                        identity,
                        expected_path_id,
                    } => {
                        push_hierarchy_walk_issue(
                            walk,
                            WorkspaceGameObjectHierarchyIssue::TargetIdentityPathMismatch {
                                source: child_transform,
                                relationship_index: *owner_relationship_index,
                                reference_kind:
                                    WorkspaceGameObjectHierarchyReferenceKind::TransformGameObject,
                                expected_path_id,
                                actual_path_id: identity.path_id,
                            },
                            limits,
                        )?;
                    }
                }
            }
            relationships => {
                push_hierarchy_walk_issue(
                    walk,
                    WorkspaceGameObjectHierarchyIssue::DuplicateTransformGameObjectReferences {
                        transform: child_transform,
                        match_count: relationships.len(),
                    },
                    limits,
                )?;
            }
        }
    }
    Ok(children)
}
