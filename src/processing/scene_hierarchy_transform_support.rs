fn default_injected_transform_values() -> WorkspaceGeneratedTransformDefaults {
    WorkspaceGeneratedTransformDefaults {
        local_position: SceneVector3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        local_rotation: SceneQuaternion {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        },
        local_scale: SceneVector3 {
            x: 1.0,
            y: 1.0,
            z: 1.0,
        },
        root_order: 0,
        local_euler_angles_hint_if_present: SceneVector3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
    }
}

/// Returns the baseline `TransformExtensions.InitializeDefault` values for missing-Transform
/// planning. No generated Unity object or PathID is allocated by this function.
pub fn default_workspace_generated_transform_values() -> WorkspaceGeneratedTransformDefaults {
    default_injected_transform_values()
}

fn is_game_object_pointer_path(field_path: &str) -> bool {
    let mut path = field_path;
    loop {
        if path == "m_GameObject" {
            return true;
        }
        let Some(base) = path.strip_prefix("Base.") else {
            return false;
        };
        path = base;
    }
}

const fn component_evidence_order(evidence: WorkspaceSceneComponentEvidence) -> u8 {
    match evidence {
        WorkspaceSceneComponentEvidence::TransformClassId => 0,
        WorkspaceSceneComponentEvidence::TypeTreeComponentBase => 1,
        WorkspaceSceneComponentEvidence::GameObjectComponentReference => 2,
        WorkspaceSceneComponentEvidence::GameObjectPointerFieldNull => 3,
        WorkspaceSceneComponentEvidence::GameObjectPointerField => 4,
        WorkspaceSceneComponentEvidence::AmbiguousTypeTreeBase => 5,
    }
}

fn normalize_component_candidates(candidates: &mut Vec<WorkspaceSceneComponentCandidate>) {
    candidates.sort_unstable_by_key(|candidate| {
        (
            candidate.identity.file_index,
            candidate.identity.object_index,
            component_evidence_order(candidate.evidence),
        )
    });
    // Keep distinct evidence kinds for the same object. In particular, baseline Scene
    // membership requires the non-null m_GameObject evidence for MonoBehaviour assets.
    candidates.dedup_by_key(|candidate| {
        (
            candidate.identity.file_index,
            candidate.identity.object_index,
            candidate.evidence,
        )
    });
}

fn validate_collection_graph(
    file_index: usize,
    scene_graph: &SceneGraph,
    roots: &WorkspaceSceneRootDiscovery,
    hierarchy_audit: &WorkspaceSceneHierarchyAudit,
    references: &MultiFileReferenceGraph<'_>,
) -> Result<(), WorkspaceSceneHierarchyError> {
    for object in scene_graph.objects() {
        let Some(identity) = references.object_identity(file_index, object.object_index) else {
            return Err(WorkspaceSceneHierarchyError::InvalidSourceIdentity {
                file_index,
                object_index: object.object_index,
                path_id: object.path_id,
            });
        };
        if identity.path_id != object.path_id {
            return Err(WorkspaceSceneHierarchyError::InvalidSourceIdentity {
                file_index,
                object_index: object.object_index,
                path_id: object.path_id,
            });
        }
    }
    for (relationship_index, relationship) in scene_graph.relationships().iter().enumerate() {
        let Some(source) = scene_graph.objects().get(relationship.source_object_index) else {
            return Err(WorkspaceSceneHierarchyError::InvalidRelationshipSource {
                file_index,
                relationship_index,
                object_index: relationship.source_object_index,
            });
        };
        if source.path_id != relationship.source_path_id {
            return Err(WorkspaceSceneHierarchyError::InvalidRelationshipSource {
                file_index,
                relationship_index,
                object_index: relationship.source_object_index,
            });
        }
        let Some(identity) =
            references.object_identity(file_index, relationship.source_object_index)
        else {
            return Err(WorkspaceSceneHierarchyError::InvalidSourceIdentity {
                file_index,
                object_index: relationship.source_object_index,
                path_id: relationship.source_path_id,
            });
        };
        if identity.path_id != relationship.source_path_id {
            return Err(WorkspaceSceneHierarchyError::InvalidSourceIdentity {
                file_index,
                object_index: relationship.source_object_index,
                path_id: relationship.source_path_id,
            });
        }
    }

    for root in roots.roots() {
        if root.source_file_index != file_index {
            return Err(WorkspaceSceneHierarchyError::RootSourceFileMismatch {
                expected: file_index,
                actual: root.source_file_index,
            });
        }
        let is_game_object = scene_graph
            .objects()
            .get(root.game_object_index)
            .is_some_and(|object| object.class_id == Some(GAME_OBJECT_CLASS_ID));
        if !is_game_object
            || references
                .object_identity(file_index, root.game_object_index)
                .is_none()
        {
            return Err(WorkspaceSceneHierarchyError::InvalidRootIdentity {
                file_index,
                object_index: root.game_object_index,
            });
        }
        if let (Some(transform_file_index), Some(transform_index)) =
            (root.transform_file_index, root.transform_index)
        {
            let is_transform = references
                .object_class_id(transform_file_index, transform_index)
                .is_some_and(|class_id| {
                    matches!(class_id, TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)
                });
            if !is_transform {
                return Err(WorkspaceSceneHierarchyError::InvalidRootTransformIdentity {
                    file_index,
                    transform_file_index,
                    transform_index,
                });
            }
        } else if root.reason != SceneRootReason::NoTransformComponent {
            return Err(WorkspaceSceneHierarchyError::InvalidRootIdentity {
                file_index,
                object_index: root.game_object_index,
            });
        }
    }
    for issue in roots.issues() {
        if issue.source_file_index != file_index {
            return Err(WorkspaceSceneHierarchyError::RootSourceFileMismatch {
                expected: file_index,
                actual: issue.source_file_index,
            });
        }
    }
    for issue in hierarchy_audit.issues() {
        if issue.source_file_index != file_index {
            return Err(WorkspaceSceneHierarchyError::AuditSourceFileMismatch {
                expected: file_index,
                actual: issue.source_file_index,
            });
        }
    }
    Ok(())
}

fn checked_charge(
    current: usize,
    amount: usize,
    limit: usize,
    resource: &'static str,
) -> Result<usize, WorkspaceSceneHierarchyError> {
    let requested = current
        .checked_add(amount)
        .ok_or(WorkspaceSceneHierarchyError::CountOverflow { resource })?;
    if requested > limit {
        return Err(WorkspaceSceneHierarchyError::ResourceLimitExceeded {
            resource,
            requested,
            limit,
        });
    }
    Ok(requested)
}

fn copy_scene_object(
    object: &SceneObjectRecord,
) -> Result<SceneObjectRecord, WorkspaceSceneHierarchyError> {
    let name_bytes = if let Some(source) = &object.name_bytes {
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(source.len()).map_err(|_| {
            WorkspaceSceneHierarchyError::AllocationFailed {
                resource: "scene object name bytes",
                requested: source.len(),
            }
        })?;
        bytes.extend_from_slice(source);
        Some(bytes)
    } else {
        None
    };
    Ok(SceneObjectRecord {
        object_index: object.object_index,
        path_id: object.path_id,
        class_id: object.class_id,
        name_bytes,
        is_active: object.is_active,
        transform: object.transform.clone(),
    })
}

fn copy_scene_relationship(
    relationship: &SceneRelationship,
) -> Result<SceneRelationship, WorkspaceSceneHierarchyError> {
    Ok(SceneRelationship {
        source_object_index: relationship.source_object_index,
        source_path_id: relationship.source_path_id,
        kind: relationship.kind,
        field_path: try_copy_string(&relationship.field_path, "scene relationship field path")?,
        file_id: relationship.file_id,
        path_id: relationship.path_id,
        target: relationship.target.clone(),
    })
}

fn copy_scene_graph_issue(
    issue: &SceneGraphIssue,
) -> Result<SceneGraphIssue, WorkspaceSceneHierarchyError> {
    use super::scene::SceneGraphIssueKind;

    let kind = match &issue.kind {
        SceneGraphIssueKind::ObjectDecodeFailed { message } => {
            SceneGraphIssueKind::ObjectDecodeFailed {
                message: try_copy_string(message, "scene object decode diagnostic")?,
            }
        }
        SceneGraphIssueKind::MissingField { field_name } => SceneGraphIssueKind::MissingField {
            field_name: try_copy_string(field_name, "scene missing-field diagnostic")?,
        },
        SceneGraphIssueKind::DuplicateField {
            field_name,
            match_count,
        } => SceneGraphIssueKind::DuplicateField {
            field_name: try_copy_string(field_name, "scene duplicate-field diagnostic")?,
            match_count: *match_count,
        },
        SceneGraphIssueKind::UnexpectedValueType { expected, actual } => {
            SceneGraphIssueKind::UnexpectedValueType { expected, actual }
        }
    };
    Ok(SceneGraphIssue {
        object_index: issue.object_index,
        path_id: issue.path_id,
        field_path: issue
            .field_path
            .as_ref()
            .map(|path| try_copy_string(path, "scene diagnostic field path"))
            .transpose()?,
        kind,
    })
}

fn try_copy_string(
    source: &str,
    resource: &'static str,
) -> Result<String, WorkspaceSceneHierarchyError> {
    let mut copy = String::new();
    copy.try_reserve_exact(source.len()).map_err(|_| {
        WorkspaceSceneHierarchyError::AllocationFailed {
            resource,
            requested: source.len(),
        }
    })?;
    copy.push_str(source);
    Ok(copy)
}
