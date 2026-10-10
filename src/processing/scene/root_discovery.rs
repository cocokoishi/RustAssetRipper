use super::reference_resolution::{
    reference_field_path_matches, scene_target_from_multi_file_resolution,
};
use super::*;

impl SceneGraph {
    /// Find local GameObject root candidates using the source SceneHierarchyObject rule.
    ///
    /// A GameObject is a root when it has no resolved Transform component or its first resolved
    /// Transform component has a null father. Missing, ambiguous, or cross-file component and
    /// father pointers are reported as issues and do not become roots by assumption. This returns
    /// file-local candidates; assigning them to a SceneDefinition still requires collection and
    /// scene metadata that this graph does not hold.
    pub fn discover_local_roots(
        &self,
        limits: SceneRootDiscoveryLimits,
    ) -> Result<SceneRootDiscovery, SceneRootDiscoveryError> {
        let indexed_relationship_count = self
            .relationships
            .iter()
            .filter(|relationship| {
                matches!(
                    relationship.kind,
                    SceneRelationshipKind::GameObjectComponent
                        | SceneRelationshipKind::TransformParent
                )
            })
            .count();
        let mut relation_index = Vec::new();
        relation_index
            .try_reserve_exact(indexed_relationship_count)
            .map_err(|_| SceneRootDiscoveryError::AllocationFailed {
                resource: "relationship lookup index",
                requested: indexed_relationship_count,
            })?;
        for (relationship_index, relationship) in self.relationships.iter().enumerate() {
            if matches!(
                relationship.kind,
                SceneRelationshipKind::GameObjectComponent | SceneRelationshipKind::TransformParent
            ) {
                relation_index.push(IndexedSceneRelationship {
                    source_object_index: relationship.source_object_index,
                    kind: relationship.kind,
                    relationship_index,
                });
            }
        }
        relation_index.sort_unstable_by_key(|entry| {
            (
                entry.source_object_index,
                entry.kind.sort_rank(),
                entry.relationship_index,
            )
        });

        let mut component_issue_objects = Vec::new();
        component_issue_objects
            .try_reserve_exact(self.issues.len())
            .map_err(|_| SceneRootDiscoveryError::AllocationFailed {
                resource: "GameObject component issue index",
                requested: self.issues.len(),
            })?;
        for issue in &self.issues {
            if issue.field_path.as_deref().is_some_and(|field_path| {
                field_path == "m_Component" || field_path.starts_with("m_Component[")
            }) {
                component_issue_objects.push(issue.object_index);
            }
        }
        component_issue_objects.sort_unstable();
        component_issue_objects.dedup();

        let mut discovery = SceneRootDiscovery {
            roots: Vec::new(),
            issues: Vec::new(),
        };
        for (game_object_index, object) in self.objects.iter().enumerate() {
            if object.class_id != Some(GAME_OBJECT_CLASS_ID) {
                continue;
            }
            let mut component_list_incomplete = component_issue_objects
                .binary_search(&game_object_index)
                .is_ok();
            if component_list_incomplete {
                push_scene_root_issue(
                    &mut discovery.issues,
                    limits.max_issues,
                    SceneRootDiscoveryIssue {
                        game_object_index,
                        transform_index: None,
                        kind: SceneRootDiscoveryIssueKind::ComponentListIncomplete,
                    },
                )?;
            }

            let mut selected_transform: Option<usize> = None;
            let mut transform_match_count = 0usize;
            let component_range = scene_relationship_range(
                &relation_index,
                game_object_index,
                SceneRelationshipKind::GameObjectComponent,
            );
            for entry in &relation_index[component_range] {
                let relationship = self.relationships.get(entry.relationship_index).ok_or(
                    SceneRootDiscoveryError::InvalidRelationshipIndex {
                        relationship_index: entry.relationship_index,
                        relationship_count: self.relationships.len(),
                    },
                )?;
                match &relationship.target {
                    SceneRelationshipTarget::Null => {}
                    SceneRelationshipTarget::LocalObject { object_index } => {
                        let Some(component) = self.objects.get(*object_index) else {
                            component_list_incomplete = true;
                            push_scene_root_issue(
                                &mut discovery.issues,
                                limits.max_issues,
                                SceneRootDiscoveryIssue {
                                    game_object_index,
                                    transform_index: selected_transform,
                                    kind: SceneRootDiscoveryIssueKind::UnresolvedComponent {
                                        target: relationship.target.clone(),
                                    },
                                },
                            )?;
                            continue;
                        };
                        match component.class_id {
                            Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID) => {
                                transform_match_count = transform_match_count
                                    .checked_add(1)
                                    .ok_or(SceneRootDiscoveryError::CountOverflow {
                                        resource: "Transform components",
                                    })?;
                                if selected_transform.is_none() {
                                    selected_transform = Some(*object_index);
                                }
                            }
                            Some(_) => {}
                            None => {
                                component_list_incomplete = true;
                                push_scene_root_issue(
                                    &mut discovery.issues,
                                    limits.max_issues,
                                    SceneRootDiscoveryIssue {
                                        game_object_index,
                                        transform_index: selected_transform,
                                        kind: SceneRootDiscoveryIssueKind::UnresolvedComponent {
                                            target: relationship.target.clone(),
                                        },
                                    },
                                )?;
                            }
                        }
                    }
                    target => {
                        component_list_incomplete = true;
                        push_scene_root_issue(
                            &mut discovery.issues,
                            limits.max_issues,
                            SceneRootDiscoveryIssue {
                                game_object_index,
                                transform_index: selected_transform,
                                kind: SceneRootDiscoveryIssueKind::UnresolvedComponent {
                                    target: target.clone(),
                                },
                            },
                        )?;
                    }
                }
            }
            if transform_match_count > 1 {
                if let Some(selected_transform_index) = selected_transform {
                    push_scene_root_issue(
                        &mut discovery.issues,
                        limits.max_issues,
                        SceneRootDiscoveryIssue {
                            game_object_index,
                            transform_index: Some(selected_transform_index),
                            kind: SceneRootDiscoveryIssueKind::MultipleTransformComponents {
                                selected_transform_index,
                                match_count: transform_match_count,
                            },
                        },
                    )?;
                }
            }

            if component_list_incomplete {
                continue;
            }

            let Some(transform_index) = selected_transform else {
                push_scene_root_candidate(
                    &mut discovery.roots,
                    limits.max_roots,
                    SceneRootCandidate {
                        game_object_index,
                        transform_index: None,
                        reason: SceneRootReason::NoTransformComponent,
                    },
                )?;
                continue;
            };

            let father_range = scene_relationship_range(
                &relation_index,
                transform_index,
                SceneRelationshipKind::TransformParent,
            );
            let father_entries = &relation_index[father_range];
            match father_entries {
                [] => push_scene_root_issue(
                    &mut discovery.issues,
                    limits.max_issues,
                    SceneRootDiscoveryIssue {
                        game_object_index,
                        transform_index: Some(transform_index),
                        kind: SceneRootDiscoveryIssueKind::MissingTransformParent,
                    },
                )?,
                [entry] => {
                    let father = self.relationships.get(entry.relationship_index).ok_or(
                        SceneRootDiscoveryError::InvalidRelationshipIndex {
                            relationship_index: entry.relationship_index,
                            relationship_count: self.relationships.len(),
                        },
                    )?;
                    match &father.target {
                        SceneRelationshipTarget::Null => push_scene_root_candidate(
                            &mut discovery.roots,
                            limits.max_roots,
                            SceneRootCandidate {
                                game_object_index,
                                transform_index: Some(transform_index),
                                reason: SceneRootReason::NullFather,
                            },
                        )?,
                        SceneRelationshipTarget::LocalObject { object_index } => {
                            let parent_is_transform =
                                self.objects.get(*object_index).is_some_and(|parent| {
                                    matches!(
                                        parent.class_id,
                                        Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)
                                    )
                                });
                            if !parent_is_transform {
                                push_scene_root_issue(
                                    &mut discovery.issues,
                                    limits.max_issues,
                                    SceneRootDiscoveryIssue {
                                        game_object_index,
                                        transform_index: Some(transform_index),
                                        kind:
                                            SceneRootDiscoveryIssueKind::UnresolvedTransformParent {
                                                target: father.target.clone(),
                                            },
                                    },
                                )?;
                            }
                        }
                        target => push_scene_root_issue(
                            &mut discovery.issues,
                            limits.max_issues,
                            SceneRootDiscoveryIssue {
                                game_object_index,
                                transform_index: Some(transform_index),
                                kind: SceneRootDiscoveryIssueKind::UnresolvedTransformParent {
                                    target: target.clone(),
                                },
                            },
                        )?,
                    }
                }
                entries => push_scene_root_issue(
                    &mut discovery.issues,
                    limits.max_issues,
                    SceneRootDiscoveryIssue {
                        game_object_index,
                        transform_index: Some(transform_index),
                        kind: SceneRootDiscoveryIssueKind::DuplicateTransformParent {
                            match_count: entries.len(),
                        },
                    },
                )?,
            }
        }
        Ok(discovery)
    }

    /// Discover roots while resolving component and father targets across loaded files.
    ///
    /// The scene graph is resolved against the supplied graph before discovery; source file and
    /// object identities are checked by `resolve_with_multi_file_graph`. The graph supplies target
    /// class IDs and the `m_Father` edge for external Transforms. Missing or undecodable targets
    /// remain issues, never roots.
    pub fn discover_workspace_roots(
        &mut self,
        source_file_index: usize,
        source_file_name: &str,
        graph: &MultiFileReferenceGraph<'_>,
        limits: SceneRootDiscoveryLimits,
    ) -> Result<WorkspaceSceneRootDiscovery, SceneRootDiscoveryError> {
        self.resolve_with_multi_file_graph(source_file_index, source_file_name, graph)
            .map_err(|source| SceneRootDiscoveryError::ReferenceGraphMismatch {
                message: source.to_string(),
            })?;
        let indexed_relationship_count = self
            .relationships
            .iter()
            .filter(|relationship| {
                matches!(
                    relationship.kind,
                    SceneRelationshipKind::GameObjectComponent
                        | SceneRelationshipKind::TransformParent
                )
            })
            .count();
        let mut relation_index = Vec::new();
        relation_index
            .try_reserve_exact(indexed_relationship_count)
            .map_err(|_| SceneRootDiscoveryError::AllocationFailed {
                resource: "workspace root relationship lookup index",
                requested: indexed_relationship_count,
            })?;
        for (relationship_index, relationship) in self.relationships.iter().enumerate() {
            if matches!(
                relationship.kind,
                SceneRelationshipKind::GameObjectComponent | SceneRelationshipKind::TransformParent
            ) {
                relation_index.push(IndexedSceneRelationship {
                    source_object_index: relationship.source_object_index,
                    kind: relationship.kind,
                    relationship_index,
                });
            }
        }
        relation_index.sort_unstable_by_key(|entry| {
            (
                entry.source_object_index,
                entry.kind.sort_rank(),
                entry.relationship_index,
            )
        });

        let mut component_issue_objects = Vec::new();
        component_issue_objects
            .try_reserve_exact(self.issues.len())
            .map_err(|_| SceneRootDiscoveryError::AllocationFailed {
                resource: "workspace GameObject component issue index",
                requested: self.issues.len(),
            })?;
        for issue in &self.issues {
            if issue.field_path.as_deref().is_some_and(|field_path| {
                field_path == "m_Component" || field_path.starts_with("m_Component[")
            }) {
                component_issue_objects.push(issue.object_index);
            }
        }
        component_issue_objects.sort_unstable();
        component_issue_objects.dedup();

        let mut discovery = WorkspaceSceneRootDiscovery {
            roots: Vec::new(),
            issues: Vec::new(),
        };
        for (game_object_index, game_object) in self.objects.iter().enumerate() {
            if game_object.class_id != Some(GAME_OBJECT_CLASS_ID) {
                continue;
            }
            let mut component_list_incomplete = component_issue_objects
                .binary_search(&game_object_index)
                .is_ok();
            if component_list_incomplete {
                push_workspace_scene_root_issue(
                    &mut discovery,
                    limits.max_issues,
                    WorkspaceSceneRootDiscoveryIssue {
                        source_file_index,
                        game_object_index,
                        transform_file_index: None,
                        transform_index: None,
                        kind: WorkspaceSceneRootDiscoveryIssueKind::ComponentListIncomplete,
                    },
                )?;
            }

            let mut selected_transform: Option<(usize, usize)> = None;
            let mut transform_match_count = 0usize;
            let component_range = scene_relationship_range(
                &relation_index,
                game_object_index,
                SceneRelationshipKind::GameObjectComponent,
            );
            for entry in &relation_index[component_range] {
                let relationship = self.relationships.get(entry.relationship_index).ok_or(
                    SceneRootDiscoveryError::InvalidRelationshipIndex {
                        relationship_index: entry.relationship_index,
                        relationship_count: self.relationships.len(),
                    },
                )?;
                let target_identity = match &relationship.target {
                    SceneRelationshipTarget::Null => continue,
                    SceneRelationshipTarget::LocalObject { object_index } => {
                        Some((source_file_index, *object_index))
                    }
                    SceneRelationshipTarget::WorkspaceObject {
                        file_index,
                        object_index,
                    } => Some((*file_index, *object_index)),
                    target => {
                        component_list_incomplete = true;
                        push_workspace_scene_root_issue(
                            &mut discovery,
                            limits.max_issues,
                            WorkspaceSceneRootDiscoveryIssue {
                                source_file_index,
                                game_object_index,
                                transform_file_index: selected_transform.map(|identity| identity.0),
                                transform_index: selected_transform.map(|identity| identity.1),
                                kind: WorkspaceSceneRootDiscoveryIssueKind::UnresolvedComponent {
                                    target: target.clone(),
                                },
                            },
                        )?;
                        None
                    }
                };
                let Some((target_file_index, target_object_index)) = target_identity else {
                    continue;
                };
                match graph.object_class_id(target_file_index, target_object_index) {
                    Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID) => {
                        transform_match_count = transform_match_count.checked_add(1).ok_or(
                            SceneRootDiscoveryError::CountOverflow {
                                resource: "workspace Transform components",
                            },
                        )?;
                        if selected_transform.is_none() {
                            selected_transform = Some((target_file_index, target_object_index));
                        }
                    }
                    Some(_) => {}
                    None => {
                        component_list_incomplete = true;
                        push_workspace_scene_root_issue(
                            &mut discovery,
                            limits.max_issues,
                            WorkspaceSceneRootDiscoveryIssue {
                                source_file_index,
                                game_object_index,
                                transform_file_index: selected_transform.map(|identity| identity.0),
                                transform_index: selected_transform.map(|identity| identity.1),
                                kind: WorkspaceSceneRootDiscoveryIssueKind::UnresolvedComponent {
                                    target: relationship.target.clone(),
                                },
                            },
                        )?;
                    }
                }
            }
            if transform_match_count > 1 {
                if let Some((transform_file_index, selected_transform_index)) = selected_transform {
                    push_workspace_scene_root_issue(
                        &mut discovery,
                        limits.max_issues,
                        WorkspaceSceneRootDiscoveryIssue {
                            source_file_index,
                            game_object_index,
                            transform_file_index: Some(transform_file_index),
                            transform_index: Some(selected_transform_index),
                            kind:
                                WorkspaceSceneRootDiscoveryIssueKind::MultipleTransformComponents {
                                    selected_transform_file_index: transform_file_index,
                                    selected_transform_index,
                                    match_count: transform_match_count,
                                },
                        },
                    )?;
                }
            }
            if component_list_incomplete {
                continue;
            }

            let Some((transform_file_index, transform_index)) = selected_transform else {
                push_workspace_scene_root_candidate(
                    &mut discovery,
                    limits.max_roots,
                    WorkspaceSceneRootCandidate {
                        source_file_index,
                        game_object_index,
                        transform_file_index: None,
                        transform_index: None,
                        reason: SceneRootReason::NoTransformComponent,
                    },
                )?;
                continue;
            };
            let Some(transform_identity) =
                graph.object_identity(transform_file_index, transform_index)
            else {
                push_workspace_scene_root_issue(
                    &mut discovery,
                    limits.max_issues,
                    WorkspaceSceneRootDiscoveryIssue {
                        source_file_index,
                        game_object_index,
                        transform_file_index: Some(transform_file_index),
                        transform_index: Some(transform_index),
                        kind: WorkspaceSceneRootDiscoveryIssueKind::UnresolvedComponent {
                            target: SceneRelationshipTarget::WorkspaceObject {
                                file_index: transform_file_index,
                                object_index: transform_index,
                            },
                        },
                    },
                )?;
                continue;
            };
            let mut father_references = graph
                .references_from_object(transform_file_index, transform_index)
                .filter(|reference| {
                    reference.source_path_id == transform_identity.path_id
                        && reference_field_path_matches(reference, "m_Father")
                });
            let Some(father_reference) = father_references.next() else {
                push_workspace_scene_root_issue(
                    &mut discovery,
                    limits.max_issues,
                    WorkspaceSceneRootDiscoveryIssue {
                        source_file_index,
                        game_object_index,
                        transform_file_index: Some(transform_file_index),
                        transform_index: Some(transform_index),
                        kind: WorkspaceSceneRootDiscoveryIssueKind::MissingTransformParent,
                    },
                )?;
                continue;
            };
            if father_references.next().is_some() {
                let match_count = graph
                    .references_from_object(transform_file_index, transform_index)
                    .filter(|reference| {
                        reference.source_path_id == transform_identity.path_id
                            && reference_field_path_matches(reference, "m_Father")
                    })
                    .count();
                push_workspace_scene_root_issue(
                    &mut discovery,
                    limits.max_issues,
                    WorkspaceSceneRootDiscoveryIssue {
                        source_file_index,
                        game_object_index,
                        transform_file_index: Some(transform_file_index),
                        transform_index: Some(transform_index),
                        kind: WorkspaceSceneRootDiscoveryIssueKind::DuplicateTransformParent {
                            match_count,
                        },
                    },
                )?;
                continue;
            }
            match &father_reference.resolution {
                MultiFilePPtrResolution::Null => push_workspace_scene_root_candidate(
                    &mut discovery,
                    limits.max_roots,
                    WorkspaceSceneRootCandidate {
                        source_file_index,
                        game_object_index,
                        transform_file_index: Some(transform_file_index),
                        transform_index: Some(transform_index),
                        reason: SceneRootReason::NullFather,
                    },
                )?,
                MultiFilePPtrResolution::Object {
                    target_file_index,
                    target_object_index,
                } if matches!(
                    graph.object_class_id(*target_file_index, *target_object_index),
                    Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)
                ) => {}
                resolution => push_workspace_scene_root_issue(
                    &mut discovery,
                    limits.max_issues,
                    WorkspaceSceneRootDiscoveryIssue {
                        source_file_index,
                        game_object_index,
                        transform_file_index: Some(transform_file_index),
                        transform_index: Some(transform_index),
                        kind: WorkspaceSceneRootDiscoveryIssueKind::UnresolvedTransformParent {
                            target: scene_target_from_multi_file_resolution(
                                resolution,
                                transform_file_index,
                            ),
                        },
                    },
                )?,
            }
        }
        Ok(discovery)
    }
}
fn scene_relationship_range(
    index: &[IndexedSceneRelationship],
    source_object_index: usize,
    kind: SceneRelationshipKind,
) -> std::ops::Range<usize> {
    let key = (source_object_index, kind.sort_rank());
    let start =
        index.partition_point(|entry| (entry.source_object_index, entry.kind.sort_rank()) < key);
    let end =
        index.partition_point(|entry| (entry.source_object_index, entry.kind.sort_rank()) <= key);
    start..end
}

fn push_scene_root_issue(
    issues: &mut Vec<SceneRootDiscoveryIssue>,
    limit: usize,
    issue: SceneRootDiscoveryIssue,
) -> Result<(), SceneRootDiscoveryError> {
    let requested = issues
        .len()
        .checked_add(1)
        .ok_or(SceneRootDiscoveryError::CountOverflow { resource: "issues" })?;
    if requested > limit {
        return Err(SceneRootDiscoveryError::IssueCountLimitExceeded { requested, limit });
    }
    issues
        .try_reserve(1)
        .map_err(|_| SceneRootDiscoveryError::AllocationFailed {
            resource: "issues",
            requested,
        })?;
    issues.push(issue);
    Ok(())
}

fn push_scene_root_candidate(
    roots: &mut Vec<SceneRootCandidate>,
    limit: usize,
    root: SceneRootCandidate,
) -> Result<(), SceneRootDiscoveryError> {
    let requested = roots
        .len()
        .checked_add(1)
        .ok_or(SceneRootDiscoveryError::CountOverflow { resource: "roots" })?;
    if requested > limit {
        return Err(SceneRootDiscoveryError::RootCountLimitExceeded { requested, limit });
    }
    roots
        .try_reserve(1)
        .map_err(|_| SceneRootDiscoveryError::AllocationFailed {
            resource: "roots",
            requested,
        })?;
    roots.push(root);
    Ok(())
}

fn push_workspace_scene_root_issue(
    discovery: &mut WorkspaceSceneRootDiscovery,
    limit: usize,
    issue: WorkspaceSceneRootDiscoveryIssue,
) -> Result<(), SceneRootDiscoveryError> {
    let requested = discovery
        .issues
        .len()
        .checked_add(1)
        .ok_or(SceneRootDiscoveryError::CountOverflow { resource: "issues" })?;
    if requested > limit {
        return Err(SceneRootDiscoveryError::IssueCountLimitExceeded { requested, limit });
    }
    discovery
        .issues
        .try_reserve(1)
        .map_err(|_| SceneRootDiscoveryError::AllocationFailed {
            resource: "workspace root issues",
            requested,
        })?;
    discovery.issues.push(issue);
    Ok(())
}

fn push_workspace_scene_root_candidate(
    discovery: &mut WorkspaceSceneRootDiscovery,
    limit: usize,
    root: WorkspaceSceneRootCandidate,
) -> Result<(), SceneRootDiscoveryError> {
    let requested = discovery
        .roots
        .len()
        .checked_add(1)
        .ok_or(SceneRootDiscoveryError::CountOverflow { resource: "roots" })?;
    if requested > limit {
        return Err(SceneRootDiscoveryError::RootCountLimitExceeded { requested, limit });
    }
    discovery
        .roots
        .try_reserve(1)
        .map_err(|_| SceneRootDiscoveryError::AllocationFailed {
            resource: "workspace root candidates",
            requested,
        })?;
    discovery.roots.push(root);
    Ok(())
}
