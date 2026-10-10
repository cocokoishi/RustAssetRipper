use super::*;
pub(super) fn copy_scene_object(
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

pub(super) fn copy_scene_relationship(
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

pub(super) fn copy_scene_graph_issue(
    issue: &SceneGraphIssue,
) -> Result<SceneGraphIssue, WorkspaceSceneHierarchyError> {
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
