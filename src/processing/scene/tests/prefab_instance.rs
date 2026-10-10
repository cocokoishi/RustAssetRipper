use super::*;

#[test]
fn reads_prefab_instance_root_game_object_pointer() {
    let metadata = metadata(&[(100, 1001), (200, 1)]);
    let roots = vec![
        (
            0,
            Value::Struct(vec![field("m_RootGameObject", pptr(0, 200))]),
        ),
        (1, Value::Struct(game_object_fields("Root", Vec::new()))),
    ];
    let graph =
        SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()).unwrap();

    assert!(graph.is_complete(), "{:?}", graph.issues());
    assert_eq!(graph.relationships().len(), 1);
    let relationship = &graph.relationships()[0];
    assert_eq!(
        relationship.kind,
        SceneRelationshipKind::PrefabInstanceRootGameObject
    );
    assert_eq!(relationship.field_path, "m_RootGameObject");
    assert_eq!(relationship.file_id, 0);
    assert_eq!(relationship.path_id, 200);
    assert_eq!(
        relationship.target,
        SceneRelationshipTarget::LocalObject { object_index: 1 }
    );
}

#[test]
fn distinguishes_null_and_missing_prefab_root_pointer_fields() {
    let metadata = metadata(&[(100, 1001)]);
    let null_graph = SceneGraph::from_decoded_roots(
        &metadata,
        &[(
            0,
            Value::Struct(vec![field("m_RootGameObject", pptr(0, 0))]),
        )],
        SceneGraphLimits::default(),
    )
    .unwrap();
    assert!(null_graph.is_complete());
    assert_eq!(null_graph.relationships().len(), 1);
    assert_eq!(
        null_graph.relationships()[0].target,
        SceneRelationshipTarget::Null
    );

    let missing_graph = SceneGraph::from_decoded_roots(
        &metadata,
        &[(0, Value::Struct(Vec::new()))],
        SceneGraphLimits::default(),
    )
    .unwrap();
    assert!(!missing_graph.is_complete());
    assert!(missing_graph.relationships().is_empty());
    assert!(missing_graph.issues().iter().any(|issue| matches!(
        &issue.kind,
        SceneGraphIssueKind::MissingField { field_name }
            if field_name == "m_RootGameObject"
    )));
}

#[test]
fn resolves_serialized_prefab_root_pointer_across_loaded_files() {
    let prefab_bytes = make_prefab_instance_file(100, 1, 42, Some("shared.assets"));
    let root_bytes = make_game_object_file("Root", 42, None, None);
    let prefab_file = SerializedFile::parse(&prefab_bytes).unwrap();
    let root_file = SerializedFile::parse(&root_bytes).unwrap();
    let prefab_reference_graph =
        crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&prefab_file).unwrap();
    let root_reference_graph =
        crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&root_file).unwrap();
    let files = [
        crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
            file_name: "main.assets",
            metadata: &prefab_file.metadata,
            graph: &prefab_reference_graph,
        },
        crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
            file_name: "shared.assets",
            metadata: &root_file.metadata,
            graph: &root_reference_graph,
        },
    ];
    let references = MultiFileReferenceGraph::build(&files).unwrap();
    let mut scene = SceneGraph::build(&prefab_file, SceneGraphLimits::default()).unwrap();
    assert_eq!(
        scene.relationships().len(),
        1,
        "class={:?}; issues={:?}; decoded={:?}",
        prefab_file.metadata.objects[0].class_id,
        scene.issues(),
        prefab_file.decode_object_payload(0).unwrap().root
    );
    assert_eq!(
        scene.relationships()[0].target,
        SceneRelationshipTarget::ExternalObject {
            file_id: 1,
            external_index: 0,
            path_id: 42,
        }
    );

    scene
        .resolve_with_multi_file_graph(0, "main.assets", &references)
        .unwrap();
    assert_eq!(
        scene.relationships()[0].target,
        SceneRelationshipTarget::WorkspaceObject {
            file_index: 1,
            object_index: 0,
        }
    );
}
