use super::*;

#[test]
fn reconstructs_game_object_component_and_transform_edges_without_merging_order() {
    let metadata = metadata(&[(10, 1), (20, 4), (30, 4), (40, 23)]);
    let roots = vec![
        (
            0,
            Value::Struct(vec![
                field(
                    "Base",
                    Value::Struct(vec![
                        field("m_Name", Value::StringBytes(b"Avatar".to_vec())),
                        field("m_IsActive", Value::Bool(true)),
                    ]),
                ),
                field(
                    "m_Component",
                    Value::Array(vec![
                        Value::Struct(vec![field("component", pptr(0, 20))]),
                        Value::Struct(vec![field("component", pptr(0, 40))]),
                    ]),
                ),
            ]),
        ),
        (
            1,
            Value::Struct(vec![field(
                "Base",
                Value::Struct(transform_fields_with_links(
                    true,
                    pptr(0, 0),
                    vec![pptr(0, 30)],
                )),
            )]),
        ),
        (
            2,
            Value::Struct(vec![field(
                "Base",
                Value::Struct(transform_fields_with_links(true, pptr(0, 20), Vec::new())),
            )]),
        ),
    ];

    let graph =
        SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()).unwrap();
    assert_eq!(graph.objects().len(), 4);
    assert_eq!(
        graph.objects()[0].name_bytes.as_deref(),
        Some(&b"Avatar"[..])
    );
    assert_eq!(graph.objects()[0].is_active, Some(true));
    assert_eq!(graph.relationships().len(), 7);
    assert_eq!(
        graph
            .relationships()
            .iter()
            .map(|relationship| relationship.kind)
            .collect::<Vec<_>>(),
        [
            SceneRelationshipKind::GameObjectComponent,
            SceneRelationshipKind::GameObjectComponent,
            SceneRelationshipKind::ComponentGameObject,
            SceneRelationshipKind::TransformParent,
            SceneRelationshipKind::TransformChild,
            SceneRelationshipKind::ComponentGameObject,
            SceneRelationshipKind::TransformParent,
        ]
    );
    assert_eq!(
        graph.relationships()[0].target,
        SceneRelationshipTarget::LocalObject { object_index: 1 }
    );
    assert_eq!(
        graph.relationships()[3].target,
        SceneRelationshipTarget::Null
    );
    assert_eq!(
        graph.relationships()[6].target,
        SceneRelationshipTarget::LocalObject { object_index: 1 }
    );
    assert!(graph.issues().is_empty());
    assert!(graph.audit_local_transform_links().unwrap().is_empty());
    assert!(graph.is_complete());
}

#[test]
fn decodes_transform_and_rect_transform_values_from_type_tree_fields() {
    let metadata = metadata(&[(10, 4), (20, 224)]);
    let mut rect_fields = vec![field("Base", Value::Struct(transform_fields(true)))];
    rect_fields.extend([
        field("m_AnchorMin", vector2(0.1, 0.2)),
        field("m_AnchorMax", vector2(0.8, 0.9)),
        field("m_AnchoredPosition", vector2(12.0, -6.0)),
        field("m_SizeDelta", vector2(320.0, 180.0)),
        field("m_Pivot", vector2(0.5, 1.0)),
    ]);
    let roots = vec![
        (0, Value::Struct(transform_fields(false))),
        (1, Value::Struct(rect_fields)),
    ];

    let graph =
        SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()).unwrap();

    assert_eq!(
        graph.objects()[0].transform,
        Some(SceneTransformValues {
            local_position: Some(SceneVector3 {
                x: 1.25,
                y: -2.0,
                z: 3.5,
            }),
            local_rotation: Some(SceneQuaternion {
                x: 0.1,
                y: 0.2,
                z: 0.3,
                w: 0.9,
            }),
            local_scale: Some(SceneVector3 {
                x: 2.0,
                y: 3.0,
                z: 4.0,
            }),
            root_order: None,
            local_euler_angles_hint: None,
            rect_transform: None,
        })
    );
    assert_eq!(
        graph.objects()[1]
            .transform
            .as_ref()
            .unwrap()
            .rect_transform,
        Some(SceneRectTransformValues {
            anchor_min: Some(SceneVector2 { x: 0.1, y: 0.2 }),
            anchor_max: Some(SceneVector2 { x: 0.8, y: 0.9 }),
            anchored_position: Some(SceneVector2 { x: 12.0, y: -6.0 }),
            size_delta: Some(SceneVector2 { x: 320.0, y: 180.0 }),
            pivot: Some(SceneVector2 { x: 0.5, y: 1.0 }),
        })
    );
    assert_eq!(
        graph.objects()[1].transform.as_ref().unwrap().root_order,
        Some(7)
    );
    assert_eq!(
        graph.objects()[1]
            .transform
            .as_ref()
            .unwrap()
            .local_euler_angles_hint,
        Some(SceneVector3 {
            x: 10.0,
            y: 20.0,
            z: 30.0,
        })
    );
    assert!(graph.issues().is_empty());
}

#[test]
fn reports_malformed_transform_values_without_filling_defaults() {
    let metadata = metadata(&[(10, 4)]);
    let mut fields = transform_fields(false);
    fields
        .iter_mut()
        .find(|field| field.name == "m_LocalPosition")
        .unwrap()
        .value = Value::Bool(false);
    let roots = vec![(0, Value::Struct(fields))];

    let graph =
        SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()).unwrap();

    assert_eq!(
        graph.objects()[0]
            .transform
            .as_ref()
            .unwrap()
            .local_position,
        None
    );
    assert!(graph.issues().iter().any(|issue| matches!(
        issue.kind,
        SceneGraphIssueKind::UnexpectedValueType {
            expected: "Vector3",
            actual: "bool",
        }
    )));
    assert!(!graph.is_complete());
}

#[test]
fn builds_a_scene_graph_from_a_complete_serialized_file() {
    let bytes = make_game_object_file("Root", 42, Some((0, 42)), None);
    let file = SerializedFile::parse(&bytes).unwrap();
    let mut graph = SceneGraph::build(&file, SceneGraphLimits::default()).unwrap();

    assert_eq!(graph.objects().len(), 1);
    assert_eq!(graph.objects()[0].path_id, 42);
    assert_eq!(graph.objects()[0].class_id, Some(1));
    assert_eq!(graph.objects()[0].name_bytes.as_deref(), Some(&b"Root"[..]));
    assert_eq!(graph.objects()[0].is_active, Some(true));
    assert_eq!(graph.relationships().len(), 1);
    assert_eq!(graph.relationships()[0].field_path, "m_Component[0]");
    assert_eq!(
        graph.relationships()[0].target,
        SceneRelationshipTarget::LocalObject { object_index: 0 }
    );
    assert!(graph.issues().is_empty());
    assert!(graph.is_complete());

    let per_file_graph =
        crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&file).unwrap();
    let named_file = crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
        file_name: "GameObject.assets",
        metadata: &file.metadata,
        graph: &per_file_graph,
    };
    let multi_file_graph = MultiFileReferenceGraph::build(&[named_file]).unwrap();
    let target_before_failed_resolution = graph.relationships()[0].target.clone();
    assert!(matches!(
        graph.resolve_with_multi_file_graph(0, "wrong.assets", &multi_file_graph),
        Err(super::SceneGraphResolveError::SourceFileNameMismatch { .. })
    ));
    assert_eq!(
        graph.relationships()[0].target,
        target_before_failed_resolution
    );
    graph
        .resolve_with_multi_file_graph(0, "GAMEOBJECT.ASSETS", &multi_file_graph)
        .unwrap();
    assert_eq!(
        graph.relationships()[0].target,
        SceneRelationshipTarget::LocalObject { object_index: 0 }
    );
}

#[test]
fn builds_transform_values_from_a_complete_2019_serialized_file() {
    let bytes = make_transform_file();
    let file = SerializedFile::parse(&bytes).unwrap();
    let graph = SceneGraph::build(&file, SceneGraphLimits::default()).unwrap();

    assert_eq!(graph.objects().len(), 1);
    assert_eq!(graph.objects()[0].class_id, Some(4));
    assert_eq!(
        graph.objects()[0].transform,
        Some(SceneTransformValues {
            local_position: Some(SceneVector3 {
                x: 1.25,
                y: -2.0,
                z: 3.5,
            }),
            local_rotation: Some(SceneQuaternion {
                x: 0.1,
                y: 0.2,
                z: 0.3,
                w: 0.9,
            }),
            local_scale: Some(SceneVector3 {
                x: 2.0,
                y: 3.0,
                z: 4.0,
            }),
            root_order: Some(7),
            local_euler_angles_hint: Some(SceneVector3 {
                x: 10.0,
                y: 20.0,
                z: 30.0,
            }),
            rect_transform: None,
        })
    );
    assert_eq!(graph.relationships().len(), 2);
    assert!(graph.issues().is_empty());
}

#[test]
fn uses_resolved_serialized_types_for_modern_object_class_ids() {
    let mut metadata = metadata(&[(42, 1)]);
    metadata.objects[0].class_id = None;
    metadata.objects[0].resolved_type_index = Some(0);
    let roots = vec![(
        0,
        Value::Struct(vec![
            field("m_Name", Value::StringBytes(b"Modern".to_vec())),
            field("m_IsActive", Value::Bool(true)),
            field("m_Component", Value::Array(Vec::new())),
        ]),
    )];

    let graph =
        SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()).unwrap();
    assert_eq!(graph.objects()[0].class_id, Some(1));
    assert!(graph.issues().is_empty());
}

#[test]
fn keeps_external_missing_duplicate_and_malformed_relationships_explicit() {
    let metadata = metadata(&[(10, 1), (20, 4), (20, 23)]);
    let roots = vec![
        (
            0,
            Value::Struct(vec![
                field("m_Name", Value::StringBytes(vec![0xff])),
                field("m_IsActive", Value::Bool(false)),
                field(
                    "m_Component",
                    Value::Array(vec![
                        Value::Struct(vec![field("component", pptr(1, 99))]),
                        Value::Struct(vec![field("component", pptr(0, 20))]),
                        Value::Struct(vec![field("component", pptr(0, 88))]),
                    ]),
                ),
            ]),
        ),
        (
            1,
            Value::Struct(vec![
                field(
                    "Base",
                    Value::Struct(vec![
                        field("m_GameObject", pptr(-1, 10)),
                        field("m_LocalPosition", vector3(0.0, 0.0, 0.0)),
                        field("m_LocalRotation", quaternion(0.0, 0.0, 0.0, 1.0)),
                        field("m_LocalScale", vector3(1.0, 1.0, 1.0)),
                    ]),
                ),
                field("m_Father", pptr(0, 0)),
                field("m_Father", pptr(0, 20)),
                field("m_Children", Value::Bool(true)),
            ]),
        ),
    ];

    let graph =
        SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()).unwrap();
    assert_eq!(graph.objects()[0].name_bytes.as_deref(), Some(&[0xff][..]));
    assert!(matches!(
        graph.relationships()[0].target,
        SceneRelationshipTarget::ExternalObject {
            external_index: 0,
            path_id: 99,
            ..
        }
    ));
    assert_eq!(
        graph.relationships()[1].target,
        SceneRelationshipTarget::AmbiguousLocalObject {
            path_id: 20,
            match_count: 2,
        }
    );
    assert_eq!(
        graph.relationships()[2].target,
        SceneRelationshipTarget::MissingLocalObject { path_id: 88 }
    );
    assert_eq!(
        graph.relationships()[3].target,
        SceneRelationshipTarget::InvalidFileId {
            file_id: -1,
            external_count: 1,
        }
    );
    assert!(graph.issues().iter().any(|issue| matches!(
        issue.kind,
        SceneGraphIssueKind::DuplicateField {
            field_name: ref name,
            match_count: 2,
        } if name == "m_Father"
    )));
    assert!(graph.issues().iter().any(|issue| matches!(
        issue.kind,
        SceneGraphIssueKind::UnexpectedValueType {
            expected: "array",
            actual: "bool",
        }
    )));
    assert!(!graph.is_complete());
}

#[test]
fn enforces_object_and_relationship_limits() {
    let metadata = metadata(&[(10, 1)]);
    let roots = vec![(
        0,
        Value::Struct(vec![
            field("m_Name", Value::StringBytes(Vec::new())),
            field("m_IsActive", Value::Bool(true)),
            field("m_Component", Value::Array(vec![pptr(0, 0)])),
        ]),
    )];
    let limits = SceneGraphLimits {
        max_objects: 0,
        ..SceneGraphLimits::default()
    };
    assert!(SceneGraph::from_decoded_roots(&metadata, &roots, limits).is_err());

    let limits = SceneGraphLimits {
        max_relationships: 0,
        ..SceneGraphLimits::default()
    };
    assert!(SceneGraph::from_decoded_roots(&metadata, &roots, limits).is_err());
}

#[test]
fn rejects_incomplete_or_duplicate_decoded_root_sets() {
    let metadata = metadata(&[(10, 1), (20, 4)]);
    assert!(SceneGraph::from_decoded_roots(&metadata, &[], SceneGraphLimits::default()).is_err());

    let root = Value::Struct(Vec::new());
    let roots = vec![(0, root.clone()), (0, root)];
    assert!(matches!(
        SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()),
        Err(super::SceneGraphBuildError::DuplicateDecodedRoot { object_index: 0 })
    ));
}
