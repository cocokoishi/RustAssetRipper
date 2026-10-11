use super::*;

#[test]
fn audits_missing_reciprocal_transform_child_links() {
    let metadata = metadata(&[(10, 4), (20, 4)]);
    let mut child_fields = transform_fields(false);
    child_fields
        .iter_mut()
        .find(|field| field.name == "m_Father")
        .unwrap()
        .value = pptr(0, 20);
    let roots = vec![
        (0, Value::Struct(child_fields)),
        (
            1,
            Value::Struct(transform_fields_with_links(false, pptr(0, 0), Vec::new())),
        ),
    ];
    let graph =
        SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()).unwrap();

    let issues = graph.audit_local_transform_links().unwrap();
    assert_eq!(issues.len(), 1);
    assert_eq!(
        issues[0].kind,
        SceneHierarchyIssueKind::MissingReciprocal {
            counterpart_kind: SceneRelationshipKind::TransformChild,
        }
    );
    assert_eq!(issues[0].source_object_index, 0);
    assert_eq!(issues[0].target_object_index, 1);
}

#[test]
fn audits_mismatched_and_duplicate_reciprocal_transform_links() {
    let metadata = metadata(&[(10, 4), (20, 4)]);
    let mismatched_roots = vec![
        (
            0,
            Value::Struct(transform_fields_with_links(
                false,
                pptr(0, 0),
                vec![pptr(0, 20)],
            )),
        ),
        (1, Value::Struct(transform_fields(false))),
    ];
    let mismatched =
        SceneGraph::from_decoded_roots(&metadata, &mismatched_roots, SceneGraphLimits::default())
            .unwrap();
    let mismatch_issues = mismatched.audit_local_transform_links().unwrap();
    assert_eq!(mismatch_issues.len(), 1);
    assert_eq!(
        mismatch_issues[0].kind,
        SceneHierarchyIssueKind::MismatchedReciprocalTarget {
            counterpart_kind: SceneRelationshipKind::TransformParent,
            actual_target: SceneRelationshipTarget::Null,
        }
    );

    let duplicate_roots = vec![
        (
            0,
            Value::Struct(transform_fields_with_links(
                false,
                pptr(0, 0),
                vec![pptr(0, 20), pptr(0, 20)],
            )),
        ),
        (
            1,
            Value::Struct(transform_fields_with_links(false, pptr(0, 10), Vec::new())),
        ),
    ];
    let duplicate =
        SceneGraph::from_decoded_roots(&metadata, &duplicate_roots, SceneGraphLimits::default())
            .unwrap();
    let duplicate_issues = duplicate.audit_local_transform_links().unwrap();
    assert_eq!(duplicate_issues.len(), 1);
    assert_eq!(
        duplicate_issues[0].kind,
        SceneHierarchyIssueKind::DuplicateReciprocal {
            counterpart_kind: SceneRelationshipKind::TransformChild,
            match_count: 2,
        }
    );
}

#[test]
fn audits_local_parent_pointers_that_target_non_transforms() {
    let metadata = metadata(&[(10, 4), (20, 23)]);
    let mut fields = transform_fields(false);
    fields
        .iter_mut()
        .find(|field| field.name == "m_Father")
        .unwrap()
        .value = pptr(0, 20);
    let roots = vec![(0, Value::Struct(fields))];
    let graph =
        SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()).unwrap();

    let issues = graph.audit_local_transform_links().unwrap();
    assert_eq!(issues.len(), 1);
    assert_eq!(
        issues[0].kind,
        SceneHierarchyIssueKind::TargetIsNotTransform { class_id: 23 }
    );
}

#[test]
fn audits_reciprocal_transform_edges_across_serialized_files() {
    let child_bytes = make_transform_file_with_hierarchy(42, 1, 99, &[], &["parent.assets"], false);
    let sibling_bytes =
        make_transform_file_with_hierarchy(43, 1, 99, &[], &["parent.assets"], false);
    let parent_bytes = make_transform_file_with_hierarchy(
        99,
        0,
        0,
        &[(1, 42), (2, 43)],
        &["child.assets", "sibling.assets"],
        false,
    );
    let child_file = SerializedFile::parse(&child_bytes).unwrap();
    let sibling_file = SerializedFile::parse(&sibling_bytes).unwrap();
    let parent_file = SerializedFile::parse(&parent_bytes).unwrap();
    let child_references =
        crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&child_file).unwrap();
    let sibling_references =
        crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&sibling_file)
            .unwrap();
    let parent_references =
        crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&parent_file).unwrap();
    let named_files = [
        crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
            file_name: "child.assets",
            metadata: &child_file.metadata,
            graph: &child_references,
        },
        crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
            file_name: "sibling.assets",
            metadata: &sibling_file.metadata,
            graph: &sibling_references,
        },
        crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
            file_name: "parent.assets",
            metadata: &parent_file.metadata,
            graph: &parent_references,
        },
    ];
    let references = MultiFileReferenceGraph::build(&named_files).unwrap();

    let mut child_scene = SceneGraph::build(&child_file, SceneGraphLimits::default()).unwrap();
    let child_audit = child_scene
        .audit_workspace_transform_links(
            0,
            "child.assets",
            &references,
            super::SceneHierarchyAuditLimits::default(),
        )
        .unwrap();
    assert_eq!(child_audit.relationships_checked(), 1);
    assert!(child_audit.issues().is_empty());
    assert!(child_audit.is_complete());

    let mut parent_scene = SceneGraph::build(&parent_file, SceneGraphLimits::default()).unwrap();
    let parent_audit = parent_scene
        .audit_workspace_transform_links(
            2,
            "parent.assets",
            &references,
            super::SceneHierarchyAuditLimits::default(),
        )
        .unwrap();
    assert_eq!(parent_audit.relationships_checked(), 3);
    assert!(parent_audit.issues().is_empty());
    assert!(parent_audit.is_complete());

    let mut sibling_scene = SceneGraph::build(&sibling_file, SceneGraphLimits::default()).unwrap();
    let sibling_audit = sibling_scene
        .audit_workspace_transform_links(
            1,
            "sibling.assets",
            &references,
            super::SceneHierarchyAuditLimits::default(),
        )
        .unwrap();
    assert_eq!(sibling_audit.relationships_checked(), 1);
    assert!(sibling_audit.issues().is_empty());
    assert!(sibling_audit.is_complete());
}

#[test]
fn reports_missing_cross_file_transform_reciprocal_without_inventing_a_match() {
    let child_bytes = make_transform_file_with_hierarchy(42, 1, 99, &[], &["parent.assets"], false);
    let parent_bytes = make_transform_file_with_hierarchy(99, 0, 0, &[], &[], false);
    let child_file = SerializedFile::parse(&child_bytes).unwrap();
    let parent_file = SerializedFile::parse(&parent_bytes).unwrap();
    let child_references =
        crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&child_file).unwrap();
    let parent_references =
        crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&parent_file).unwrap();
    let named_files = [
        crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
            file_name: "child.assets",
            metadata: &child_file.metadata,
            graph: &child_references,
        },
        crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
            file_name: "parent.assets",
            metadata: &parent_file.metadata,
            graph: &parent_references,
        },
    ];
    let references = MultiFileReferenceGraph::build(&named_files).unwrap();
    let mut child_scene = SceneGraph::build(&child_file, SceneGraphLimits::default()).unwrap();

    let audit = child_scene
        .audit_workspace_transform_links(
            0,
            "child.assets",
            &references,
            super::SceneHierarchyAuditLimits::default(),
        )
        .unwrap();
    assert_eq!(audit.relationships_checked(), 1);
    assert_eq!(
        audit.issues(),
        &[super::WorkspaceSceneHierarchyIssue {
            source_file_index: 0,
            relationship_index: 1,
            source_object_index: 0,
            source_path_id: 42,
            target_file_index: Some(1),
            target_object_index: Some(0),
            field_path: "m_Father".to_owned(),
            kind: super::SceneHierarchyIssueKind::MissingReciprocal {
                counterpart_kind: SceneRelationshipKind::TransformChild,
            },
        }]
    );
    assert!(!audit.is_complete());
}

#[test]
fn workspace_transform_audit_enforces_relationship_and_issue_limits() {
    let child_bytes = make_transform_file_with_hierarchy(42, 1, 99, &[], &["parent.assets"], false);
    let parent_bytes = make_transform_file_with_hierarchy(99, 0, 0, &[], &[], false);
    let child_file = SerializedFile::parse(&child_bytes).unwrap();
    let parent_file = SerializedFile::parse(&parent_bytes).unwrap();
    let child_references =
        crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&child_file).unwrap();
    let parent_references =
        crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&parent_file).unwrap();
    let named_files = [
        crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
            file_name: "child.assets",
            metadata: &child_file.metadata,
            graph: &child_references,
        },
        crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
            file_name: "parent.assets",
            metadata: &parent_file.metadata,
            graph: &parent_references,
        },
    ];
    let references = MultiFileReferenceGraph::build(&named_files).unwrap();

    let mut relationship_limited_scene =
        SceneGraph::build(&child_file, SceneGraphLimits::default()).unwrap();
    assert!(matches!(
        relationship_limited_scene.audit_workspace_transform_links(
            0,
            "child.assets",
            &references,
            super::SceneHierarchyAuditLimits {
                max_relationships: 0,
                max_issues: 1,
            },
        ),
        Err(super::SceneGraphAuditError::RelationshipLimitExceeded { count: 1, limit: 0 })
    ));

    let mut issue_limited_scene =
        SceneGraph::build(&child_file, SceneGraphLimits::default()).unwrap();
    assert!(matches!(
        issue_limited_scene.audit_workspace_transform_links(
            0,
            "child.assets",
            &references,
            super::SceneHierarchyAuditLimits {
                max_relationships: 1,
                max_issues: 0,
            },
        ),
        Err(super::SceneGraphAuditError::IssueLimitExceeded { count: 1, limit: 0 })
    ));
}

#[test]
fn does_not_call_cross_file_reciprocal_missing_when_target_scan_is_incomplete() {
    let child_bytes = make_transform_file_with_hierarchy(42, 1, 99, &[], &["parent.assets"], false);
    let parent_bytes = make_transform_file_with_hierarchy(99, 0, 0, &[], &[], true);
    let child_file = SerializedFile::parse(&child_bytes).unwrap();
    let parent_file = SerializedFile::parse(&parent_bytes).unwrap();
    let child_references =
        crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&child_file).unwrap();
    let parent_references =
        crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&parent_file).unwrap();
    assert!(!parent_references.object_scan_is_complete(0));
    let named_files = [
        crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
            file_name: "child.assets",
            metadata: &child_file.metadata,
            graph: &child_references,
        },
        crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
            file_name: "parent.assets",
            metadata: &parent_file.metadata,
            graph: &parent_references,
        },
    ];
    let references = MultiFileReferenceGraph::build(&named_files).unwrap();
    let mut child_scene = SceneGraph::build(&child_file, SceneGraphLimits::default()).unwrap();

    let audit = child_scene
        .audit_workspace_transform_links(
            0,
            "child.assets",
            &references,
            super::SceneHierarchyAuditLimits::default(),
        )
        .unwrap();
    assert_eq!(audit.relationships_checked(), 1);
    assert_eq!(
        audit.issues(),
        &[super::WorkspaceSceneHierarchyIssue {
            source_file_index: 0,
            relationship_index: 1,
            source_object_index: 0,
            source_path_id: 42,
            target_file_index: Some(1),
            target_object_index: Some(0),
            field_path: "m_Father".to_owned(),
            kind: super::SceneHierarchyIssueKind::CounterpartScanIncomplete,
        }]
    );
    assert!(!audit.is_complete());
}

#[test]
fn does_not_report_transform_audit_complete_when_source_scan_is_incomplete() {
    let child_bytes = make_transform_file_with_hierarchy(42, 1, 99, &[], &["parent.assets"], true);
    let parent_bytes =
        make_transform_file_with_hierarchy(99, 0, 0, &[(1, 42)], &["child.assets"], false);
    let child_file = SerializedFile::parse(&child_bytes).unwrap();
    let parent_file = SerializedFile::parse(&parent_bytes).unwrap();
    let child_references =
        crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&child_file).unwrap();
    let parent_references =
        crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&parent_file).unwrap();
    assert!(!child_references.object_scan_is_complete(0));
    let named_files = [
        crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
            file_name: "child.assets",
            metadata: &child_file.metadata,
            graph: &child_references,
        },
        crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
            file_name: "parent.assets",
            metadata: &parent_file.metadata,
            graph: &parent_references,
        },
    ];
    let references = MultiFileReferenceGraph::build(&named_files).unwrap();
    let mut child_scene = SceneGraph::build(&child_file, SceneGraphLimits::default()).unwrap();

    let audit = child_scene
        .audit_workspace_transform_links(
            0,
            "child.assets",
            &references,
            super::SceneHierarchyAuditLimits::default(),
        )
        .unwrap();
    assert_eq!(audit.relationships_checked(), 1);
    assert_eq!(
        audit.issues(),
        &[super::WorkspaceSceneHierarchyIssue {
            source_file_index: 0,
            relationship_index: 1,
            source_object_index: 0,
            source_path_id: 42,
            target_file_index: None,
            target_object_index: None,
            field_path: "m_Father".to_owned(),
            kind: super::SceneHierarchyIssueKind::SourceScanIncomplete,
        }]
    );
    assert!(!audit.is_complete());
}
