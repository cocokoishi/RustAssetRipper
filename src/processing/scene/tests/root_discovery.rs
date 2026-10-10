use super::*;

#[test]
fn discovers_transform_roots_and_game_objects_without_transforms() {
    let metadata = metadata(&[(10, 1), (20, 4), (30, 1), (40, 4), (50, 1)]);
    let mut child_transform_fields = transform_fields_with_links(false, pptr(0, 20), Vec::new());
    child_transform_fields
        .iter_mut()
        .find(|field| field.name == "m_GameObject")
        .unwrap()
        .value = pptr(0, 30);
    let roots = vec![
        (
            0,
            Value::Struct(game_object_fields("Root", vec![pptr(0, 20)])),
        ),
        (
            1,
            Value::Struct(transform_fields_with_links(
                false,
                pptr(0, 0),
                vec![pptr(0, 40)],
            )),
        ),
        (
            2,
            Value::Struct(game_object_fields("Child", vec![pptr(0, 40)])),
        ),
        (3, Value::Struct(child_transform_fields)),
        (
            4,
            Value::Struct(game_object_fields("NoTransform", Vec::new())),
        ),
    ];
    let graph =
        SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()).unwrap();

    let discovery = graph
        .discover_local_roots(SceneRootDiscoveryLimits::default())
        .unwrap();
    assert_eq!(
        discovery.roots(),
        [
            super::SceneRootCandidate {
                game_object_index: 0,
                transform_index: Some(1),
                reason: SceneRootReason::NullFather,
            },
            super::SceneRootCandidate {
                game_object_index: 4,
                transform_index: None,
                reason: SceneRootReason::NoTransformComponent,
            },
        ]
    );
    assert!(discovery.issues().is_empty());
    assert!(discovery.is_complete());
    assert!(matches!(
        graph.discover_local_roots(SceneRootDiscoveryLimits {
            max_roots: 1,
            ..SceneRootDiscoveryLimits::default()
        }),
        Err(super::SceneRootDiscoveryError::RootCountLimitExceeded {
            requested: 2,
            limit: 1,
        })
    ));
}

#[test]
fn root_discovery_keeps_unresolved_component_and_parent_states_visible() {
    let component_metadata = metadata(&[(10, 1), (20, 4)]);
    let component_roots = vec![
        (
            0,
            Value::Struct(game_object_fields(
                "Unknown",
                vec![pptr(0, 99), pptr(0, 20)],
            )),
        ),
        (1, Value::Struct(transform_fields(false))),
    ];
    let component_graph = SceneGraph::from_decoded_roots(
        &component_metadata,
        &component_roots,
        SceneGraphLimits::default(),
    )
    .unwrap();
    let component_discovery = component_graph
        .discover_local_roots(SceneRootDiscoveryLimits::default())
        .unwrap();
    assert!(component_discovery.roots().is_empty());
    assert!(matches!(
        component_discovery.issues()[0].kind,
        SceneRootDiscoveryIssueKind::UnresolvedComponent {
            target: SceneRelationshipTarget::MissingLocalObject { path_id: 99 }
        }
    ));
    assert!(matches!(
        component_graph.discover_local_roots(SceneRootDiscoveryLimits {
            max_issues: 0,
            ..SceneRootDiscoveryLimits::default()
        }),
        Err(super::SceneRootDiscoveryError::IssueCountLimitExceeded {
            requested: 1,
            limit: 0,
        })
    ));

    let parent_metadata = metadata(&[(10, 1), (20, 4)]);
    let mut transform = transform_fields(false);
    transform
        .iter_mut()
        .find(|field| field.name == "m_Father")
        .unwrap()
        .value = pptr(0, 99);
    let parent_roots = vec![
        (
            0,
            Value::Struct(game_object_fields("UnknownParent", vec![pptr(0, 20)])),
        ),
        (1, Value::Struct(transform)),
    ];
    let parent_graph = SceneGraph::from_decoded_roots(
        &parent_metadata,
        &parent_roots,
        SceneGraphLimits::default(),
    )
    .unwrap();
    let parent_discovery = parent_graph
        .discover_local_roots(SceneRootDiscoveryLimits::default())
        .unwrap();
    assert!(parent_discovery.roots().is_empty());
    assert!(parent_discovery.issues().iter().any(|issue| matches!(
        issue.kind,
        SceneRootDiscoveryIssueKind::UnresolvedTransformParent {
            target: SceneRelationshipTarget::MissingLocalObject { path_id: 99 }
        }
    )));
}

#[test]
fn discovers_workspace_roots_when_transform_component_is_external() {
    let source_bytes = make_game_object_file("Source", 42, Some((1, 99)), Some("shared.assets"));
    let target_bytes = make_transform_file_with_links(99, 0, 0);
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

    let roots = scene
        .discover_workspace_roots(
            0,
            "main.assets",
            &reference_graph,
            SceneRootDiscoveryLimits::default(),
        )
        .unwrap();
    assert_eq!(
        roots.roots(),
        &[super::WorkspaceSceneRootCandidate {
            source_file_index: 0,
            game_object_index: 0,
            transform_file_index: Some(1),
            transform_index: Some(0),
            reason: SceneRootReason::NullFather,
        }]
    );
    assert!(roots.issues().is_empty());
    assert!(roots.is_complete());
}

#[test]
fn workspace_root_discovery_does_not_hide_missing_external_transform_parent() {
    let source_bytes = make_game_object_file("Source", 42, Some((1, 99)), Some("shared.assets"));
    let target_bytes = make_transform_file_with_links(99, 0, 777);
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

    let roots = scene
        .discover_workspace_roots(
            0,
            "main.assets",
            &reference_graph,
            SceneRootDiscoveryLimits::default(),
        )
        .unwrap();
    assert!(roots.roots().is_empty());
    assert_eq!(
        roots.issues(),
        &[super::WorkspaceSceneRootDiscoveryIssue {
            source_file_index: 0,
            game_object_index: 0,
            transform_file_index: Some(1),
            transform_index: Some(0),
            kind: super::WorkspaceSceneRootDiscoveryIssueKind::UnresolvedTransformParent {
                target: SceneRelationshipTarget::MissingLocalObject { path_id: 777 },
            },
        }]
    );
    assert!(!roots.is_complete());
}

#[test]
fn scene_definition_owns_compacted_cross_file_hierarchy_data() {
    use crate::serialized_file::reference_graph::{
        MultiFileReferenceGraph, NamedObjectReferenceGraph, ObjectReferenceGraph,
    };

    let source_bytes = make_game_object_file_with_components(
        "SceneObject",
        42,
        &[(1, 99), (0, 0)],
        Some("shared.assets"),
    );
    let target_bytes = make_transform_file_with_hierarchy(99, 0, 0, &[], &["main.assets"], false);
    let source_file = SerializedFile::parse(&source_bytes).unwrap();
    let target_file = SerializedFile::parse(&target_bytes).unwrap();
    let source_references = ObjectReferenceGraph::build(&source_file).unwrap();
    let target_references = ObjectReferenceGraph::build(&target_file).unwrap();
    let named_files = [
        NamedObjectReferenceGraph {
            file_name: "main.assets",
            metadata: &source_file.metadata,
            graph: &source_references,
        },
        NamedObjectReferenceGraph {
            file_name: "shared.assets",
            metadata: &target_file.metadata,
            graph: &target_references,
        },
    ];
    let multi_file_graph = MultiFileReferenceGraph::build(&named_files).unwrap();
    let mut scene_graph = SceneGraph::build(&source_file, SceneGraphLimits::default()).unwrap();
    let roots = scene_graph
        .discover_workspace_roots(
            0,
            "main.assets",
            &multi_file_graph,
            SceneRootDiscoveryLimits::default(),
        )
        .unwrap();
    let audit = scene_graph
        .audit_workspace_transform_links(
            0,
            "main.assets",
            &multi_file_graph,
            super::SceneHierarchyAuditLimits::default(),
        )
        .unwrap();
    let plan = crate::processing::scene_collections::plan_scene_definition(
        0,
        "main.assets",
        &crate::processing::scene_collections::SceneCollectionPathStatus::BuildSettingsMissing,
        None,
        None,
        crate::processing::scene_collections::SceneDefinitionPlanLimits::default(),
    )
    .unwrap();
    let definitions = crate::processing::scene_collections::WorkspaceSceneDefinitions::from_plans(
        vec![plan],
        2,
        crate::processing::scene_collections::WorkspaceSceneDefinitionLimits::default(),
    )
    .unwrap();

    let limited = crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits {
        max_scene_objects: 0,
        ..crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits::default()
    };
    let mut bounded =
        crate::processing::scene_hierarchy::WorkspaceSceneHierarchyBuilder::new(2, limited)
            .unwrap();
    assert!(matches!(
        bounded.add_collection_graph(
            0,
            &scene_graph,
            roots.clone(),
            audit.clone(),
            &multi_file_graph,
        ),
        Err(crate::processing::scene_hierarchy::WorkspaceSceneHierarchyError::ResourceLimitExceeded {
            resource: "scene objects",
            requested: 1,
            limit: 0,
        })
    ));
    assert!(!bounded.contains_collection_graph(0));

    let graph_limited = crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits {
        max_workspace_collection_graphs: 0,
        ..crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits::default()
    };
    let mut bounded_graphs =
        crate::processing::scene_hierarchy::WorkspaceSceneHierarchyBuilder::new(2, graph_limited)
            .unwrap();
    assert!(matches!(
        bounded_graphs.add_collection_graph(
            0,
            &scene_graph,
            roots.clone(),
            audit.clone(),
            &multi_file_graph,
        ),
        Err(crate::processing::scene_hierarchy::WorkspaceSceneHierarchyError::ResourceLimitExceeded {
            resource: "workspace hierarchy collection graphs",
            requested: 1,
            limit: 0,
        })
    ));
    assert!(!bounded_graphs.contains_collection_graph(0));

    let asset_limited = crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits {
        max_scene_asset_records: 1,
        ..crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits::default()
    };
    let mut bounded_assets =
        crate::processing::scene_hierarchy::WorkspaceSceneHierarchyBuilder::new(2, asset_limited)
            .unwrap();
    assert!(matches!(
        bounded_assets.add_collection_graph(
            0,
            &scene_graph,
            roots.clone(),
            audit.clone(),
            &multi_file_graph,
        ),
        Err(crate::processing::scene_hierarchy::WorkspaceSceneHierarchyError::ResourceLimitExceeded {
            resource: "scene asset and generated-transform records",
            requested: 2,
            limit: 1,
        })
    ));
    assert!(!bounded_assets.contains_collection_graph(0));

    let non_scene_definitions =
        crate::processing::scene_collections::WorkspaceSceneDefinitions::from_plans(
            Vec::new(),
            2,
            crate::processing::scene_collections::WorkspaceSceneDefinitionLimits::default(),
        )
        .unwrap();
    let mut non_scene_builder =
        crate::processing::scene_hierarchy::WorkspaceSceneHierarchyBuilder::new(
            2,
            crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits::default(),
        )
        .unwrap();
    non_scene_builder
        .add_collection_graph(
            0,
            &scene_graph,
            roots.clone(),
            audit.clone(),
            &multi_file_graph,
        )
        .unwrap();
    let non_scene_hierarchies = non_scene_builder.finish(&non_scene_definitions).unwrap();
    assert!(non_scene_hierarchies.scenes().is_empty());
    assert_eq!(non_scene_hierarchies.non_scene_collections().len(), 1);
    assert_eq!(
        non_scene_hierarchies.non_scene_collections()[0].collection_file_index(),
        0
    );
    assert_eq!(
        non_scene_hierarchies.non_scene_collections()[0].game_object_count(),
        1
    );

    let mut builder = crate::processing::scene_hierarchy::WorkspaceSceneHierarchyBuilder::new(
        2,
        crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits::default(),
    )
    .unwrap();
    builder
        .add_collection_graph(0, &scene_graph, roots, audit, &multi_file_graph)
        .unwrap();
    let hierarchy = builder.finish(&definitions).unwrap();
    assert_eq!(hierarchy.scenes().len(), 1);
    assert_eq!(hierarchy.scenes()[0].plan.collection_file_index, 0);
    let collections = hierarchy.scenes()[0].collections();
    assert_eq!(collections.len(), 1);
    assert_eq!(collections[0].game_object_count(), 1);
    assert_eq!(collections[0].transform_count(), 0);
    assert_eq!(collections[0].scene_assets().len(), 1);
    assert_eq!(
        collections[0].scene_assets()[0],
        crate::processing::scene_hierarchy::WorkspaceSceneAssetIdentity {
            identity: crate::processing::scene_hierarchy::WorkspaceSceneObjectIdentity {
                file_index: 0,
                object_index: 0,
                path_id: 42,
                class_id: Some(1),
            },
            category: crate::processing::scene_hierarchy::WorkspaceSceneAssetCategory::GameObject,
        }
    );
    assert_eq!(collections[0].component_candidates().len(), 1);
    assert_eq!(
        collections[0].component_candidates()[0],
        crate::processing::scene_hierarchy::WorkspaceSceneComponentCandidate {
            identity: crate::processing::scene_hierarchy::WorkspaceSceneObjectIdentity {
                file_index: 1,
                object_index: 0,
                path_id: 99,
                class_id: Some(4),
            },
            evidence: crate::processing::scene_hierarchy::WorkspaceSceneComponentEvidence::GameObjectComponentReference,
        }
    );
    assert_eq!(collections[0].relationships().len(), 2);
    assert_eq!(collections[0].game_object_hierarchies().len(), 1);
    assert_eq!(
        collections[0].game_object_hierarchies()[0].game_object(),
        crate::processing::scene_hierarchy::WorkspaceSceneObjectIdentity {
            file_index: 0,
            object_index: 0,
            path_id: 42,
            class_id: Some(1),
        }
    );
    assert_eq!(
        collections[0].game_object_hierarchies()[0].component_relationship_count(),
        2
    );
    let ordered_components: Vec<_> = collections[0]
        .game_object_component_relationships(0)
        .unwrap()
        .collect();
    assert_eq!(ordered_components.len(), 2);
    assert_eq!(ordered_components[0].field_path, "m_Component[0]");
    assert_eq!(ordered_components[0].file_id, 1);
    assert_eq!(ordered_components[0].path_id, 99);
    assert_eq!(
        ordered_components[0].target,
        SceneRelationshipTarget::WorkspaceObject {
            file_index: 1,
            object_index: 0,
        }
    );
    assert_eq!(ordered_components[1].field_path, "m_Component[1]");
    assert_eq!(ordered_components[1].target, SceneRelationshipTarget::Null);
    assert!(matches!(
        collections[0].game_object_component_relationships(1),
        Err(crate::processing::scene_hierarchy::WorkspaceSceneHierarchyError::GameObjectHierarchyIndexOutOfBounds {
            hierarchy_index: 1,
            hierarchy_count: 1,
        })
    ));
    assert_eq!(collections[0].roots().roots().len(), 1);
    assert_eq!(
        collections[0].roots().roots()[0].transform_file_index,
        Some(1)
    );
    assert!(collections[0].is_complete());
}

#[test]
fn scene_definition_plans_source_defaults_for_a_missing_game_object_transform() {
    use crate::serialized_file::reference_graph::{
        MultiFileReferenceGraph, NamedObjectReferenceGraph, ObjectReferenceGraph,
    };

    let bytes = make_game_object_file("NoTransform", 42, None, None);
    let file = SerializedFile::parse(&bytes).unwrap();
    let object_references = ObjectReferenceGraph::build(&file).unwrap();
    let named_file = NamedObjectReferenceGraph {
        file_name: "scene.assets",
        metadata: &file.metadata,
        graph: &object_references,
    };
    let references = MultiFileReferenceGraph::build(&[named_file]).unwrap();
    let mut scene_graph = SceneGraph::build(&file, SceneGraphLimits::default()).unwrap();
    let roots = scene_graph
        .discover_workspace_roots(
            0,
            "scene.assets",
            &references,
            SceneRootDiscoveryLimits::default(),
        )
        .unwrap();
    assert_eq!(roots.roots().len(), 1);
    assert_eq!(
        roots.roots()[0].reason,
        SceneRootReason::NoTransformComponent
    );
    let mut bounded_non_scene_builder =
        crate::processing::scene_hierarchy::WorkspaceGeneratedTransformPlanBuilder::new(
            1,
            crate::processing::scene_hierarchy::WorkspaceGeneratedTransformPlanLimits {
                max_missing_transform_plans: 0,
                ..crate::processing::scene_hierarchy::WorkspaceGeneratedTransformPlanLimits::default()
            },
        )
        .unwrap();
    assert!(matches!(
        bounded_non_scene_builder.add_file(0, &roots, &references),
        Err(crate::processing::scene_hierarchy::WorkspaceSceneHierarchyError::ResourceLimitExceeded {
            resource: "workspace missing-Transform plans",
            requested: 1,
            limit: 0,
        })
    ));
    assert!(!bounded_non_scene_builder.contains_file(0));
    let audit = scene_graph
        .audit_workspace_transform_links(
            0,
            "scene.assets",
            &references,
            SceneHierarchyAuditLimits::default(),
        )
        .unwrap();
    let plan = crate::processing::scene_collections::plan_scene_definition(
        0,
        "scene.assets",
        &crate::processing::scene_collections::SceneCollectionPathStatus::BuildSettingsMissing,
        None,
        None,
        crate::processing::scene_collections::SceneDefinitionPlanLimits::default(),
    )
    .unwrap();
    let definitions = crate::processing::scene_collections::WorkspaceSceneDefinitions::from_plans(
        vec![plan],
        1,
        crate::processing::scene_collections::WorkspaceSceneDefinitionLimits::default(),
    )
    .unwrap();
    let mut non_scene_plan_builder =
        crate::processing::scene_hierarchy::WorkspaceGeneratedTransformPlanBuilder::new(
            1,
            crate::processing::scene_hierarchy::WorkspaceGeneratedTransformPlanLimits::default(),
        )
        .unwrap();
    non_scene_plan_builder
        .add_file(0, &roots, &references)
        .unwrap();
    assert!(non_scene_plan_builder.contains_file(0));
    let no_scene_definitions =
        crate::processing::scene_collections::WorkspaceSceneDefinitions::from_plans(
            Vec::new(),
            1,
            crate::processing::scene_collections::WorkspaceSceneDefinitionLimits::default(),
        )
        .unwrap();
    let non_scene_plans = non_scene_plan_builder
        .finish(&no_scene_definitions)
        .unwrap();
    assert_eq!(non_scene_plans.plans().len(), 1);
    assert_eq!(non_scene_plans.plans()[0].game_object.path_id, 42);

    let mut scene_plan_builder =
        crate::processing::scene_hierarchy::WorkspaceGeneratedTransformPlanBuilder::new(
            1,
            crate::processing::scene_hierarchy::WorkspaceGeneratedTransformPlanLimits::default(),
        )
        .unwrap();
    scene_plan_builder.add_file(0, &roots, &references).unwrap();
    assert!(scene_plan_builder
        .finish(&definitions)
        .unwrap()
        .plans()
        .is_empty());

    let mut bounded_builder =
        crate::processing::scene_hierarchy::WorkspaceSceneHierarchyBuilder::new(
            1,
            crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits {
                max_scene_asset_records: 1,
                ..crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits::default()
            },
        )
        .unwrap();
    assert!(matches!(
        bounded_builder.add_collection_graph(
            0,
            &scene_graph,
            roots.clone(),
            audit.clone(),
            &references,
        ),
        Err(crate::processing::scene_hierarchy::WorkspaceSceneHierarchyError::ResourceLimitExceeded {
            resource: "scene asset and generated-transform records",
            requested: 2,
            limit: 1,
        })
    ));
    assert!(!bounded_builder.contains_collection_graph(0));
    let mut builder = crate::processing::scene_hierarchy::WorkspaceSceneHierarchyBuilder::new(
        1,
        crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits::default(),
    )
    .unwrap();
    builder
        .add_collection_graph(0, &scene_graph, roots, audit, &references)
        .unwrap();

    let hierarchy = builder.finish(&definitions).unwrap();
    let plans = hierarchy.scenes()[0].collections()[0].missing_transform_plans();
    assert_eq!(plans.len(), 1);
    assert_eq!(
        plans[0].game_object,
        crate::processing::scene_hierarchy::WorkspaceSceneObjectIdentity {
            file_index: 0,
            object_index: 0,
            path_id: 42,
            class_id: Some(1),
        }
    );
    assert_eq!(
        plans[0].default_values,
        crate::processing::scene_hierarchy::WorkspaceGeneratedTransformDefaults {
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
    );
}
