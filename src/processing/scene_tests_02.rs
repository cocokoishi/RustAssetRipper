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
        let definitions =
            crate::processing::scene_collections::WorkspaceSceneDefinitions::from_plans(
                vec![plan],
                1,
                crate::processing::scene_collections::WorkspaceSceneDefinitionLimits::default(),
            )
            .unwrap();
        let mut non_scene_plan_builder =
            crate::processing::scene_hierarchy::WorkspaceGeneratedTransformPlanBuilder::new(
                1,
                crate::processing::scene_hierarchy::WorkspaceGeneratedTransformPlanLimits::default(
                ),
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
                crate::processing::scene_hierarchy::WorkspaceGeneratedTransformPlanLimits::default(
                ),
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

    #[test]
    fn audits_reciprocal_transform_edges_across_serialized_files() {
        let child_bytes =
            make_transform_file_with_hierarchy(42, 1, 99, &[], &["parent.assets"], false);
        let parent_bytes =
            make_transform_file_with_hierarchy(99, 0, 0, &[(1, 42)], &["child.assets"], false);
        let child_file = SerializedFile::parse(&child_bytes).unwrap();
        let parent_file = SerializedFile::parse(&parent_bytes).unwrap();
        let child_references =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&child_file)
                .unwrap();
        let parent_references =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&parent_file)
                .unwrap();
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

        let mut parent_scene =
            SceneGraph::build(&parent_file, SceneGraphLimits::default()).unwrap();
        let parent_audit = parent_scene
            .audit_workspace_transform_links(
                1,
                "parent.assets",
                &references,
                super::SceneHierarchyAuditLimits::default(),
            )
            .unwrap();
        assert_eq!(parent_audit.relationships_checked(), 2);
        assert!(parent_audit.issues().is_empty());
        assert!(parent_audit.is_complete());
    }

    #[test]
    fn reports_missing_cross_file_transform_reciprocal_without_inventing_a_match() {
        let child_bytes =
            make_transform_file_with_hierarchy(42, 1, 99, &[], &["parent.assets"], false);
        let parent_bytes = make_transform_file_with_hierarchy(99, 0, 0, &[], &[], false);
        let child_file = SerializedFile::parse(&child_bytes).unwrap();
        let parent_file = SerializedFile::parse(&parent_bytes).unwrap();
        let child_references =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&child_file)
                .unwrap();
        let parent_references =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&parent_file)
                .unwrap();
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
        let child_bytes =
            make_transform_file_with_hierarchy(42, 1, 99, &[], &["parent.assets"], false);
        let parent_bytes = make_transform_file_with_hierarchy(99, 0, 0, &[], &[], false);
        let child_file = SerializedFile::parse(&child_bytes).unwrap();
        let parent_file = SerializedFile::parse(&parent_bytes).unwrap();
        let child_references =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&child_file)
                .unwrap();
        let parent_references =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&parent_file)
                .unwrap();
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
        let child_bytes =
            make_transform_file_with_hierarchy(42, 1, 99, &[], &["parent.assets"], false);
        let parent_bytes = make_transform_file_with_hierarchy(99, 0, 0, &[], &[], true);
        let child_file = SerializedFile::parse(&child_bytes).unwrap();
        let parent_file = SerializedFile::parse(&parent_bytes).unwrap();
        let child_references =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&child_file)
                .unwrap();
        let parent_references =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&parent_file)
                .unwrap();
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
        let child_bytes =
            make_transform_file_with_hierarchy(42, 1, 99, &[], &["parent.assets"], true);
        let parent_bytes =
            make_transform_file_with_hierarchy(99, 0, 0, &[(1, 42)], &["child.assets"], false);
        let child_file = SerializedFile::parse(&child_bytes).unwrap();
        let parent_file = SerializedFile::parse(&parent_bytes).unwrap();
        let child_references =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&child_file)
                .unwrap();
        let parent_references =
            crate::serialized_file::reference_graph::ObjectReferenceGraph::build(&parent_file)
                .unwrap();
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
            let array_reference =
                crate::serialized_file::reference_graph::MultiFileObjectReference {
                    source_file_index: 0,
                    source_object_index: 4,
                    source_path_id: 42,
                    field_path,
                    file_id: 0,
                    path_id: 99,
                    resolution:
                        crate::serialized_file::reference_graph::MultiFilePPtrResolution::Object {
                            target_file_index: 0,
                            target_object_index: 5,
                        },
                };
            assert_eq!(
                super::reference_field_path_matches_array_element(&array_reference, "m_Children"),
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
                super::scene_target_from_multi_file_resolution(&resolution, 0),
                expected
            );
        }
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
        assert!(
            SceneGraph::from_decoded_roots(&metadata, &[], SceneGraphLimits::default()).is_err()
        );

        let root = Value::Struct(Vec::new());
        let roots = vec![(0, root.clone()), (0, root)];
        assert!(matches!(
            SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()),
            Err(super::SceneGraphBuildError::DuplicateDecodedRoot { object_index: 0 })
        ));
    }

    #[test]
    fn hierarchy_create_sorts_every_arm_into_its_list() {
        assert_eq!(
            classify_scene_hierarchy_asset(SceneHierarchyAssetKind::GameObject),
            SceneHierarchySlot::GameObjects
        );
        // Only MonoBehaviours on a GameObject join Components; detached ones are ignored.
        assert_eq!(
            classify_scene_hierarchy_asset(SceneHierarchyAssetKind::AttachedMonoBehaviour),
            SceneHierarchySlot::Components
        );
        assert_eq!(
            classify_scene_hierarchy_asset(SceneHierarchyAssetKind::Component),
            SceneHierarchySlot::Components
        );
        assert_eq!(
            classify_scene_hierarchy_asset(SceneHierarchyAssetKind::DetachedMonoBehaviour),
            SceneHierarchySlot::Ignored
        );
        assert_eq!(
            classify_scene_hierarchy_asset(SceneHierarchyAssetKind::LevelGameManager),
            SceneHierarchySlot::Managers
        );
        assert_eq!(
            classify_scene_hierarchy_asset(SceneHierarchyAssetKind::PrefabInstance),
            SceneHierarchySlot::PrefabInstances
        );
        assert_eq!(
            classify_scene_hierarchy_asset(SceneHierarchyAssetKind::SceneRoots),
            SceneHierarchySlot::SceneRoots
        );
        assert_eq!(
            classify_scene_hierarchy_asset(SceneHierarchyAssetKind::Other),
            SceneHierarchySlot::Ignored
        );
    }

    #[test]
    fn hierarchy_dependency_fields_follow_the_fetch_order() {
        assert_eq!(
            scene_hierarchy_dependency_fields(2, 1, 1, 1, true),
            vec![
                "GameObjects[]",
                "GameObjects[]",
                "Components[]",
                "PrefabInstances[]",
                "Managers[]",
                "SceneRoots",
            ]
        );
        // The SceneRoots edge exists only when a root block applies.
        assert_eq!(
            scene_hierarchy_dependency_fields(1, 0, 0, 0, false),
            vec!["GameObjects[]"]
        );
        assert!(scene_hierarchy_dependency_fields(0, 0, 0, 0, false).is_empty());
    }

    #[test]
    fn hierarchy_root_predicate_matches_the_is_root_rule() {
        use super::SceneHierarchyRootFact;
        // No Transform: the source `?.` yields null, so the GameObject is a root.
        assert!(super::is_scene_hierarchy_root(SceneHierarchyRootFact {
            has_transform: false,
            father_is_null: false,
        }));
        // A Transform with a null father is a root; a set father is not.
        assert!(super::is_scene_hierarchy_root(SceneHierarchyRootFact {
            has_transform: true,
            father_is_null: true,
        }));
        assert!(!super::is_scene_hierarchy_root(SceneHierarchyRootFact {
            has_transform: true,
            father_is_null: false,
        }));
    }

    #[test]
    fn get_roots_keeps_hierarchy_order_and_reports_no_roots_explicitly() {
        use super::SceneHierarchyRootFact;
        let facts = [
            SceneHierarchyRootFact {
                has_transform: true,
                father_is_null: false,
            },
            SceneHierarchyRootFact {
                has_transform: true,
                father_is_null: true,
            },
            SceneHierarchyRootFact {
                has_transform: false,
                father_is_null: false,
            },
            SceneHierarchyRootFact {
                has_transform: true,
                father_is_null: false,
            },
        ];
        assert_eq!(super::scene_hierarchy_root_indexes(&facts), vec![1, 2]);
        assert!(super::scene_hierarchy_root_indexes(&facts[0..1]).is_empty());
        assert!(super::scene_hierarchy_root_indexes(&[]).is_empty());
    }
