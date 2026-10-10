use super::reference_resolution::{
    reference_field_path_matches, reference_field_path_matches_array_element,
    scene_target_from_multi_file_resolution,
};
use super::*;

impl SceneGraph {
    /// Audit reciprocal parent/child pointers whose targets resolve inside this scene graph.
    ///
    /// Cross-file targets are not checked here because this graph contains scene values for only
    /// one file. The source editor conversion requires each non-root Transform to appear in its
    /// father's child array when it calculates root order; this method reports that and related
    /// contradictions without panicking or repairing the serialized values.
    pub fn audit_local_transform_links(
        &self,
    ) -> Result<Vec<SceneHierarchyIssue>, SceneGraphAuditError> {
        let relationship_count = self
            .relationships
            .iter()
            .filter(|relationship| {
                matches!(
                    relationship.kind,
                    SceneRelationshipKind::TransformParent | SceneRelationshipKind::TransformChild
                )
            })
            .count();
        let mut index = Vec::new();
        index.try_reserve_exact(relationship_count).map_err(|_| {
            SceneGraphAuditError::AllocationFailed {
                resource: "local Transform relationship index",
                requested: relationship_count,
            }
        })?;
        for (relationship_index, relationship) in self.relationships.iter().enumerate() {
            if !matches!(
                relationship.kind,
                SceneRelationshipKind::TransformParent | SceneRelationshipKind::TransformChild
            ) {
                continue;
            }
            index.push(IndexedHierarchyRelationship {
                source_object_index: relationship.source_object_index,
                kind: relationship.kind,
                target_local_object_index: match relationship.target {
                    SceneRelationshipTarget::LocalObject { object_index } => Some(object_index),
                    _ => None,
                },
                relationship_index,
            });
        }
        index.sort_unstable_by_key(|entry| {
            (
                entry.source_object_index,
                entry.kind.sort_rank(),
                entry.target_local_object_index.unwrap_or(usize::MAX),
                entry.relationship_index,
            )
        });

        let mut issues = Vec::new();
        issues.try_reserve_exact(relationship_count).map_err(|_| {
            SceneGraphAuditError::AllocationFailed {
                resource: "local Transform consistency issues",
                requested: relationship_count,
            }
        })?;
        for (relationship_index, relationship) in self.relationships.iter().enumerate() {
            let counterpart_kind = match relationship.kind {
                SceneRelationshipKind::TransformParent => SceneRelationshipKind::TransformChild,
                SceneRelationshipKind::TransformChild => SceneRelationshipKind::TransformParent,
                _ => continue,
            };
            let SceneRelationshipTarget::LocalObject {
                object_index: target_index,
            } = &relationship.target
            else {
                continue;
            };
            let target_index = *target_index;
            let Some(target_object) = self.objects.get(target_index) else {
                continue;
            };
            match target_object.class_id {
                Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID) => {}
                Some(class_id) => {
                    issues.push(SceneHierarchyIssue {
                        relationship_index,
                        source_object_index: relationship.source_object_index,
                        target_object_index: target_index,
                        field_path: relationship.field_path.clone(),
                        kind: SceneHierarchyIssueKind::TargetIsNotTransform { class_id },
                    });
                    continue;
                }
                None => {
                    issues.push(SceneHierarchyIssue {
                        relationship_index,
                        source_object_index: relationship.source_object_index,
                        target_object_index: target_index,
                        field_path: relationship.field_path.clone(),
                        kind: SceneHierarchyIssueKind::TargetClassUnknown,
                    });
                    continue;
                }
            }

            let counterpart_source_index = target_index;
            let expected_target_index = relationship.source_object_index;
            let range =
                hierarchy_relationship_range(&index, counterpart_source_index, counterpart_kind);
            let entries = &index[range];
            match relationship.kind {
                SceneRelationshipKind::TransformParent => {
                    let match_count = entries
                        .iter()
                        .filter(|entry| {
                            entry.target_local_object_index == Some(expected_target_index)
                        })
                        .count();
                    let kind = match match_count {
                        0 => Some(SceneHierarchyIssueKind::MissingReciprocal { counterpart_kind }),
                        1 => None,
                        match_count => Some(SceneHierarchyIssueKind::DuplicateReciprocal {
                            counterpart_kind,
                            match_count,
                        }),
                    };
                    if let Some(kind) = kind {
                        issues.push(SceneHierarchyIssue {
                            relationship_index,
                            source_object_index: relationship.source_object_index,
                            target_object_index: target_index,
                            field_path: relationship.field_path.clone(),
                            kind,
                        });
                    }
                }
                SceneRelationshipKind::TransformChild => match entries {
                    [] => issues.push(SceneHierarchyIssue {
                        relationship_index,
                        source_object_index: relationship.source_object_index,
                        target_object_index: target_index,
                        field_path: relationship.field_path.clone(),
                        kind: SceneHierarchyIssueKind::MissingReciprocal { counterpart_kind },
                    }),
                    [entry] if entry.target_local_object_index == Some(expected_target_index) => {}
                    [entry] => {
                        let counterpart = self.relationships.get(entry.relationship_index).ok_or(
                            SceneGraphAuditError::InvalidRelationshipIndex {
                                relationship_index: entry.relationship_index,
                                relationship_count: self.relationships.len(),
                            },
                        )?;
                        issues.push(SceneHierarchyIssue {
                            relationship_index,
                            source_object_index: relationship.source_object_index,
                            target_object_index: target_index,
                            field_path: relationship.field_path.clone(),
                            kind: SceneHierarchyIssueKind::MismatchedReciprocalTarget {
                                counterpart_kind,
                                actual_target: counterpart.target.clone(),
                            },
                        });
                    }
                    entries => issues.push(SceneHierarchyIssue {
                        relationship_index,
                        source_object_index: relationship.source_object_index,
                        target_object_index: target_index,
                        field_path: relationship.field_path.clone(),
                        kind: SceneHierarchyIssueKind::DuplicateReciprocal {
                            counterpart_kind,
                            match_count: entries.len(),
                        },
                    }),
                },
                _ => {}
            }
        }
        Ok(issues)
    }

    /// Audit reciprocal Transform parent/child pointers across every loaded SerializedFile.
    ///
    /// A missing reciprocal edge is reported only when the target object's reference scan is
    /// complete. If its TypeTree payload or one of its fields could not be scanned, the result is
    /// marked incomplete instead of claiming that the edge is absent.
    pub fn audit_workspace_transform_links(
        &mut self,
        source_file_index: usize,
        source_file_name: &str,
        graph: &MultiFileReferenceGraph<'_>,
        limits: SceneHierarchyAuditLimits,
    ) -> Result<WorkspaceSceneHierarchyAudit, SceneGraphAuditError> {
        self.resolve_with_multi_file_graph(source_file_index, source_file_name, graph)
            .map_err(|source| SceneGraphAuditError::ReferenceGraphMismatch {
                message: source.to_string(),
            })?;

        let relationship_count = self
            .relationships
            .iter()
            .filter(|relationship| {
                matches!(
                    relationship.kind,
                    SceneRelationshipKind::TransformParent | SceneRelationshipKind::TransformChild
                )
            })
            .count();
        if relationship_count > limits.max_relationships {
            return Err(SceneGraphAuditError::RelationshipLimitExceeded {
                count: relationship_count,
                limit: limits.max_relationships,
            });
        }

        let mut audit = WorkspaceSceneHierarchyAudit {
            relationships_checked: 0,
            issues: Vec::new(),
        };
        for (relationship_index, relationship) in self.relationships.iter().enumerate() {
            let counterpart_kind = match relationship.kind {
                SceneRelationshipKind::TransformParent => SceneRelationshipKind::TransformChild,
                SceneRelationshipKind::TransformChild => SceneRelationshipKind::TransformParent,
                _ => continue,
            };
            audit.relationships_checked = audit.relationships_checked.checked_add(1).ok_or(
                SceneGraphAuditError::CountOverflow {
                    resource: "workspace Transform relationships",
                },
            )?;

            if graph.object_scan_is_complete(source_file_index, relationship.source_object_index)
                != Some(true)
            {
                push_workspace_hierarchy_issue(
                    &mut audit,
                    limits.max_issues,
                    WorkspaceSceneHierarchyIssue {
                        source_file_index,
                        relationship_index,
                        source_object_index: relationship.source_object_index,
                        source_path_id: relationship.source_path_id,
                        target_file_index: None,
                        target_object_index: None,
                        field_path: relationship.field_path.clone(),
                        kind: SceneHierarchyIssueKind::SourceScanIncomplete,
                    },
                )?;
                continue;
            }

            if relationship.kind == SceneRelationshipKind::TransformParent
                && relationship.target == SceneRelationshipTarget::Null
            {
                continue;
            }

            let (target_file_index, target_object_index) = match &relationship.target {
                SceneRelationshipTarget::LocalObject { object_index } => {
                    (source_file_index, *object_index)
                }
                SceneRelationshipTarget::WorkspaceObject {
                    file_index,
                    object_index,
                } => (*file_index, *object_index),
                target => {
                    push_workspace_hierarchy_issue(
                        &mut audit,
                        limits.max_issues,
                        WorkspaceSceneHierarchyIssue {
                            source_file_index,
                            relationship_index,
                            source_object_index: relationship.source_object_index,
                            source_path_id: relationship.source_path_id,
                            target_file_index: None,
                            target_object_index: None,
                            field_path: relationship.field_path.clone(),
                            kind: SceneHierarchyIssueKind::UnresolvedTarget {
                                target: target.clone(),
                            },
                        },
                    )?;
                    continue;
                }
            };

            let Some(target_class_id) =
                graph.object_class_id(target_file_index, target_object_index)
            else {
                push_workspace_hierarchy_issue(
                    &mut audit,
                    limits.max_issues,
                    WorkspaceSceneHierarchyIssue {
                        source_file_index,
                        relationship_index,
                        source_object_index: relationship.source_object_index,
                        source_path_id: relationship.source_path_id,
                        target_file_index: Some(target_file_index),
                        target_object_index: Some(target_object_index),
                        field_path: relationship.field_path.clone(),
                        kind: SceneHierarchyIssueKind::TargetClassUnknown,
                    },
                )?;
                continue;
            };
            if !matches!(
                target_class_id,
                TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID
            ) {
                push_workspace_hierarchy_issue(
                    &mut audit,
                    limits.max_issues,
                    WorkspaceSceneHierarchyIssue {
                        source_file_index,
                        relationship_index,
                        source_object_index: relationship.source_object_index,
                        source_path_id: relationship.source_path_id,
                        target_file_index: Some(target_file_index),
                        target_object_index: Some(target_object_index),
                        field_path: relationship.field_path.clone(),
                        kind: SceneHierarchyIssueKind::TargetIsNotTransform {
                            class_id: target_class_id,
                        },
                    },
                )?;
                continue;
            }

            if graph.object_scan_is_complete(target_file_index, target_object_index) != Some(true) {
                push_workspace_hierarchy_issue(
                    &mut audit,
                    limits.max_issues,
                    WorkspaceSceneHierarchyIssue {
                        source_file_index,
                        relationship_index,
                        source_object_index: relationship.source_object_index,
                        source_path_id: relationship.source_path_id,
                        target_file_index: Some(target_file_index),
                        target_object_index: Some(target_object_index),
                        field_path: relationship.field_path.clone(),
                        kind: SceneHierarchyIssueKind::CounterpartScanIncomplete,
                    },
                )?;
                continue;
            }

            let Some(target_identity) =
                graph.object_identity(target_file_index, target_object_index)
            else {
                push_workspace_hierarchy_issue(
                    &mut audit,
                    limits.max_issues,
                    WorkspaceSceneHierarchyIssue {
                        source_file_index,
                        relationship_index,
                        source_object_index: relationship.source_object_index,
                        source_path_id: relationship.source_path_id,
                        target_file_index: Some(target_file_index),
                        target_object_index: Some(target_object_index),
                        field_path: relationship.field_path.clone(),
                        kind: SceneHierarchyIssueKind::TargetClassUnknown,
                    },
                )?;
                continue;
            };
            let mut counterpart_edges = graph
                .references_from_object(target_file_index, target_object_index)
                .filter(|reference| {
                    if reference.source_path_id != target_identity.path_id {
                        return false;
                    }
                    match counterpart_kind {
                        SceneRelationshipKind::TransformParent => {
                            reference_field_path_matches(reference, "m_Father")
                        }
                        SceneRelationshipKind::TransformChild => {
                            reference_field_path_matches_array_element(reference, "m_Children")
                        }
                        _ => false,
                    }
                });
            let Some(counterpart_edge) = counterpart_edges.next() else {
                push_workspace_hierarchy_issue(
                    &mut audit,
                    limits.max_issues,
                    WorkspaceSceneHierarchyIssue {
                        source_file_index,
                        relationship_index,
                        source_object_index: relationship.source_object_index,
                        source_path_id: relationship.source_path_id,
                        target_file_index: Some(target_file_index),
                        target_object_index: Some(target_object_index),
                        field_path: relationship.field_path.clone(),
                        kind: SceneHierarchyIssueKind::MissingReciprocal { counterpart_kind },
                    },
                )?;
                continue;
            };

            let Some(_second_edge) = counterpart_edges.next() else {
                if !matches!(
                    counterpart_edge.resolution,
                    MultiFilePPtrResolution::Object {
                        target_file_index: actual_file_index,
                        target_object_index: actual_object_index,
                    } if actual_file_index == source_file_index
                        && actual_object_index == relationship.source_object_index
                ) {
                    push_workspace_hierarchy_issue(
                        &mut audit,
                        limits.max_issues,
                        WorkspaceSceneHierarchyIssue {
                            source_file_index,
                            relationship_index,
                            source_object_index: relationship.source_object_index,
                            source_path_id: relationship.source_path_id,
                            target_file_index: Some(target_file_index),
                            target_object_index: Some(target_object_index),
                            field_path: relationship.field_path.clone(),
                            kind: SceneHierarchyIssueKind::MismatchedReciprocalTarget {
                                counterpart_kind,
                                actual_target: scene_target_from_multi_file_resolution(
                                    &counterpart_edge.resolution,
                                    target_file_index,
                                ),
                            },
                        },
                    )?;
                }
                continue;
            };
            let match_count = counterpart_edges.count().checked_add(2).ok_or(
                SceneGraphAuditError::CountOverflow {
                    resource: "workspace reciprocal edge matches",
                },
            )?;
            push_workspace_hierarchy_issue(
                &mut audit,
                limits.max_issues,
                WorkspaceSceneHierarchyIssue {
                    source_file_index,
                    relationship_index,
                    source_object_index: relationship.source_object_index,
                    source_path_id: relationship.source_path_id,
                    target_file_index: Some(target_file_index),
                    target_object_index: Some(target_object_index),
                    field_path: relationship.field_path.clone(),
                    kind: SceneHierarchyIssueKind::DuplicateReciprocal {
                        counterpart_kind,
                        match_count,
                    },
                },
            )?;
        }
        Ok(audit)
    }
}
fn hierarchy_relationship_range(
    index: &[IndexedHierarchyRelationship],
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
fn push_workspace_hierarchy_issue(
    audit: &mut WorkspaceSceneHierarchyAudit,
    limit: usize,
    issue: WorkspaceSceneHierarchyIssue,
) -> Result<(), SceneGraphAuditError> {
    let requested =
        audit
            .issues
            .len()
            .checked_add(1)
            .ok_or(SceneGraphAuditError::CountOverflow {
                resource: "workspace hierarchy issues",
            })?;
    if requested > limit {
        return Err(SceneGraphAuditError::IssueLimitExceeded {
            count: requested,
            limit,
        });
    }
    audit
        .issues
        .try_reserve(1)
        .map_err(|_| SceneGraphAuditError::AllocationFailed {
            resource: "workspace hierarchy issues",
            requested,
        })?;
    audit.issues.push(issue);
    Ok(())
}
