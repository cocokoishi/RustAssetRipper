use super::*;

#[test]
fn resolves_scene_component_links_across_loaded_serialized_files() {
    let source_bytes = make_game_object_file("Source", 42, Some((1, 99)), Some("shared.assets"));
    let target_bytes = make_game_object_file("Target", 99, None, None);
    let source_file = SerializedFile::parse(&source_bytes).unwrap();
    let target_file = SerializedFile::parse(&target_bytes).unwrap();
    let source_per_file_graph =
        crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&source_file).unwrap();
    let target_per_file_graph =
        crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&target_file).unwrap();
    let named_files = [
        crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
            file_name: "main.assets",
            metadata: &source_file.metadata,
            graph: &source_per_file_graph,
        },
        crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
            file_name: "shared.assets",
            metadata: &target_file.metadata,
            graph: &target_per_file_graph,
        },
    ];
    let reference_graph = MultiFileReferenceGraph::build(&named_files).unwrap();
    let mut scene = SceneGraph::build(&source_file, SceneGraphLimits::default()).unwrap();
    assert!(matches!(
        scene.relationships()[0].target,
        SceneRelationshipTarget::ExternalObject {
            file_id: 1,
            external_index: 0,
            path_id: 99,
        }
    ));

    scene
        .resolve_with_multi_file_graph(0, "main.assets", &reference_graph)
        .unwrap();
    assert_eq!(
        scene.relationships()[0].target,
        SceneRelationshipTarget::WorkspaceObject {
            file_index: 1,
            object_index: 0,
        }
    );
    assert!(scene.is_complete());

    let source_only = [
        crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
            file_name: "main.assets",
            metadata: &source_file.metadata,
            graph: &source_per_file_graph,
        },
    ];
    let missing_reference_graph = MultiFileReferenceGraph::build(&source_only).unwrap();
    let mut missing_scene = SceneGraph::build(&source_file, SceneGraphLimits::default()).unwrap();
    missing_scene
        .resolve_with_multi_file_graph(0, "main.assets", &missing_reference_graph)
        .unwrap();
    assert_eq!(
        missing_scene.relationships()[0].target,
        SceneRelationshipTarget::MissingDependency {
            external_index: 0,
            path_id: 99,
        }
    );
    assert!(!missing_scene.is_complete());

    let no_edge_bytes = make_game_object_file("Source", 42, None, Some("shared.assets"));
    let no_edge_file = SerializedFile::parse(&no_edge_bytes).unwrap();
    let no_edge_per_file_graph =
        crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&no_edge_file)
            .unwrap();
    let no_edge_named = [
        crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
            file_name: "main.assets",
            metadata: &no_edge_file.metadata,
            graph: &no_edge_per_file_graph,
        },
    ];
    let no_edge_graph = MultiFileReferenceGraph::build(&no_edge_named).unwrap();
    let mut unmatched_scene = SceneGraph::build(&source_file, SceneGraphLimits::default()).unwrap();
    let original_target = unmatched_scene.relationships()[0].target.clone();
    assert!(matches!(
        unmatched_scene.resolve_with_multi_file_graph(0, "main.assets", &no_edge_graph),
        Err(super::SceneGraphResolveError::ReferenceFieldNotFound { .. })
    ));
    assert_eq!(unmatched_scene.relationships()[0].target, original_target);
}

#[test]
fn refuses_scene_reference_join_when_same_identity_has_a_different_pptr_value() {
    let scene_bytes = make_game_object_file("Source", 42, Some((1, 99)), Some("shared.assets"));
    let graph_bytes = make_game_object_file("Source", 42, Some((1, 98)), Some("shared.assets"));
    let scene_file = SerializedFile::parse(&scene_bytes).unwrap();
    let graph_file = SerializedFile::parse(&graph_bytes).unwrap();
    let per_file_graph =
        crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&graph_file).unwrap();
    let named_files = [
        crate::serialized_file::reference_graph::NamedObjectReferenceGraph {
            file_name: "main.assets",
            metadata: &graph_file.metadata,
            graph: &per_file_graph,
        },
    ];
    let reference_graph = MultiFileReferenceGraph::build(&named_files).unwrap();
    let mut scene = SceneGraph::build(&scene_file, SceneGraphLimits::default()).unwrap();
    let original_target = scene.relationships()[0].target.clone();

    assert!(matches!(
        scene.resolve_with_multi_file_graph(0, "main.assets", &reference_graph),
        Err(super::SceneGraphResolveError::ReferenceValueMismatch {
            scene_file_id: 1,
            scene_path_id: 99,
            graph_file_id: 1,
            graph_path_id: 98,
            ..
        })
    ));
    assert_eq!(scene.relationships()[0].target, original_target);
}

#[test]
fn matches_inherited_scene_reference_field_paths_without_matching_neighbors() {
    let reference = crate::serialized_file::reference_graph::MultiFileObjectReference {
        source_file_index: 0,
        source_object_index: 4,
        source_path_id: 42,
        field_path: "Base.m_Component[0].component",
        file_id: 0,
        path_id: 0,
        resolution: crate::serialized_file::reference_graph::MultiFilePPtrResolution::Null,
    };
    assert!(reference_field_path_matches(
        &reference,
        "m_Component[0].component"
    ));
    assert!(!reference_field_path_matches(
        &reference,
        "m_Component[1].component"
    ));
    assert!(!reference_field_path_matches(&reference, "component"));

    for (field_path, expected) in [
        ("Base.Base.m_Children[12]", true),
        ("m_Children[0]", true),
        ("m_Children[]", false),
        ("m_Children[not-an-index]", false),
        ("m_Children[0].unexpected", false),
        ("m_Children[0]suffix", false),
    ] {
        let array_reference = crate::serialized_file::reference_graph::MultiFileObjectReference {
            source_file_index: 0,
            source_object_index: 4,
            source_path_id: 42,
            field_path,
            file_id: 0,
            path_id: 99,
            resolution: crate::serialized_file::reference_graph::MultiFilePPtrResolution::Object {
                target_file_index: 0,
                target_object_index: 5,
            },
        };
        assert_eq!(
            reference_field_path_matches_array_element(&array_reference, "m_Children"),
            expected,
            "unexpected array-field match for {field_path}"
        );
    }
}

#[test]
fn preserves_all_cross_file_reference_failure_categories() {
    use crate::serialized_file::reference_graph::MultiFilePPtrResolution as Resolution;

    let cases = [
        (
            Resolution::AmbiguousDependency {
                external_index: 2,
                path_id: 70,
                candidate_count: 3,
            },
            SceneRelationshipTarget::AmbiguousDependency {
                external_index: 2,
                path_id: 70,
                candidate_count: 3,
            },
        ),
        (
            Resolution::MissingExternalObject {
                target_file_index: 4,
                path_id: 71,
            },
            SceneRelationshipTarget::MissingExternalObject {
                file_index: 4,
                path_id: 71,
            },
        ),
        (
            Resolution::AmbiguousExternalObject {
                target_file_index: 5,
                path_id: 72,
                match_count: 2,
            },
            SceneRelationshipTarget::AmbiguousExternalObject {
                file_index: 5,
                path_id: 72,
                match_count: 2,
            },
        ),
        (
            Resolution::InvalidFileId {
                file_id: -1,
                external_count: 2,
            },
            SceneRelationshipTarget::InvalidFileId {
                file_id: -1,
                external_count: 2,
            },
        ),
    ];
    for (resolution, expected) in cases {
        assert_eq!(
            scene_target_from_multi_file_resolution(&resolution, 0),
            expected
        );
    }
}
