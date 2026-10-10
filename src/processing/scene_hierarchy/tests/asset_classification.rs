use super::*;

#[test]
fn component_candidate_normalization_keeps_distinct_evidence_for_one_object() {
    let mono_behaviour = identity(0, 1, 11, MONO_BEHAVIOUR_CLASS_ID);
    let direct = WorkspaceSceneComponentCandidate {
        identity: mono_behaviour,
        evidence: WorkspaceSceneComponentEvidence::GameObjectComponentReference,
    };
    let back_reference = WorkspaceSceneComponentCandidate {
        identity: mono_behaviour,
        evidence: WorkspaceSceneComponentEvidence::GameObjectPointerField,
    };
    let null_back_reference = WorkspaceSceneComponentCandidate {
        identity: mono_behaviour,
        evidence: WorkspaceSceneComponentEvidence::GameObjectPointerFieldNull,
    };
    let mut candidates = vec![back_reference, direct, null_back_reference, direct];

    normalize_component_candidates(&mut candidates);

    assert_eq!(
        candidates,
        vec![direct, null_back_reference, back_reference]
    );
}

#[test]
fn type_tree_base_chain_classifies_component_inheritance_without_guessing() {
    let tree = |nodes: &[(u8, &str, Option<&str>)]| TypeTree {
        nodes: nodes
            .iter()
            .map(|(level, name, type_name)| TypeTreeNode {
                level: *level,
                version: 1,
                type_flags: 0,
                type_string_offset: None,
                name_string_offset: None,
                type_name: type_name.map(str::to_owned),
                name: Some((*name).to_owned()),
                byte_size: -1,
                index: 0,
                meta_flags: 0,
                reference_type_hash: None,
            })
            .collect(),
        string_buffer: Vec::new(),
    };
    let mesh_renderer = tree(&[
        (0, "", Some("MeshRenderer")),
        (1, "Base", Some("Renderer")),
        (2, "Base", Some("Component")),
        (3, "m_GameObject", Some("PPtr<GameObject>")),
    ]);
    let scriptable_object = tree(&[
        (0, "", Some("ExampleAsset")),
        (1, "Base", Some("ScriptableObject")),
        (2, "Base", Some("Object")),
    ]);
    let duplicate_base = tree(&[
        (0, "", Some("MalformedAsset")),
        (1, "Base", Some("Component")),
        (1, "Base", Some("Object")),
    ]);
    let missing_base_type = tree(&[(0, "", Some("MalformedAsset")), (1, "Base", None)]);
    let empty_tree = TypeTree {
        nodes: Vec::new(),
        string_buffer: Vec::new(),
    };

    assert_eq!(
        classify_type_tree_component_base(Some(&mesh_renderer)),
        TypeTreeComponentBaseClassification::Component
    );
    assert_eq!(
        classify_type_tree_component_base(Some(&scriptable_object)),
        TypeTreeComponentBaseClassification::NotComponent
    );
    assert_eq!(
        classify_type_tree_component_base(Some(&duplicate_base)),
        TypeTreeComponentBaseClassification::Ambiguous
    );
    assert_eq!(
        classify_type_tree_component_base(Some(&missing_base_type)),
        TypeTreeComponentBaseClassification::Ambiguous
    );
    assert_eq!(
        classify_type_tree_component_base(Some(&empty_tree)),
        TypeTreeComponentBaseClassification::Unavailable
    );
    assert_eq!(
        classify_type_tree_component_base(None),
        TypeTreeComponentBaseClassification::Unavailable
    );
}

#[test]
fn generated_serialized_file_type_tree_component_flows_into_scene_ids() {
    const MESH_RENDERER_CLASS_ID: i32 = 23;

    let bytes = generated_mesh_renderer_serialized_file();
    let file = SerializedFile::parse(&bytes).unwrap();
    assert_eq!(
        file.metadata.objects[0].class_id,
        Some(MESH_RENDERER_CLASS_ID as i16)
    );
    let object_graph = ObjectReferenceGraph::build(&file).unwrap();
    let named_files = [NamedObjectReferenceGraph {
        file_name: "scene.assets",
        metadata: &file.metadata,
        graph: &object_graph,
    }];
    let references = MultiFileReferenceGraph::build(&named_files).unwrap();
    assert!(
        references.is_complete(),
        "object scan: {:?}; issues: {:?}",
        references.object_scan_is_complete(0, 0),
        object_graph.issues()
    );
    assert_eq!(references.object_type_tree(0, 0).unwrap().nodes.len(), 6);

    let mut scene_graph =
        SceneGraph::build(&file, crate::processing::scene::SceneGraphLimits::default()).unwrap();
    let roots = scene_graph
        .discover_workspace_roots(
            0,
            "scene.assets",
            &references,
            crate::processing::scene::SceneRootDiscoveryLimits::default(),
        )
        .unwrap();
    let audit = scene_graph
        .audit_workspace_transform_links(
            0,
            "scene.assets",
            &references,
            crate::processing::scene::SceneHierarchyAuditLimits::default(),
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
    let mut builder =
        WorkspaceSceneHierarchyBuilder::new(1, WorkspaceSceneHierarchyLimits::default()).unwrap();
    builder
        .add_collection_graph(0, &scene_graph, roots, audit, &references)
        .unwrap();
    let hierarchies = builder.finish(&definitions).unwrap();
    let scene = &hierarchies.scenes()[0];
    let candidates = scene.collections()[0].component_candidates();
    assert_eq!(
        candidates,
        &[
            WorkspaceSceneComponentCandidate {
                identity: identity(0, 0, 9001, MESH_RENDERER_CLASS_ID),
                evidence: WorkspaceSceneComponentEvidence::TypeTreeComponentBase,
            },
            WorkspaceSceneComponentCandidate {
                identity: identity(0, 0, 9001, MESH_RENDERER_CLASS_ID),
                evidence: WorkspaceSceneComponentEvidence::GameObjectPointerFieldNull,
            },
        ]
    );

    let membership = scene
        .asset_membership(WorkspaceSceneHierarchyMembershipLimits::default())
        .unwrap();
    assert!(
        membership.is_complete(),
        "issues: {:?}",
        membership.issues()
    );
    assert_eq!(
        membership.components(),
        &[identity(0, 0, 9001, MESH_RENDERER_CLASS_ID)]
    );
    let assignments = crate::export::scene_collection::plan_workspace_scene_export_ids(
        scene,
        crate::processing::scene_naming::SceneUnityVersion {
            major: 2019,
            minor: 4,
            patch: 0,
        },
        WorkspaceSceneHierarchyMembershipLimits::default(),
        crate::export::scene_collection::SceneHierarchyAssetOrderLimits::default(),
        crate::export::scene_collection::SceneExportIdPlanLimits::default(),
    )
    .unwrap();
    assert_eq!(assignments.len(), 1);
    assert_eq!(
        assignments[0].asset,
        crate::export::scene_collection::ExportAssetIdentity {
            collection:
                crate::export::scene_collection::ExportAssetCollectionIdentity::SerializedFile {
                    file_index: 0
                },
            object_index: 0,
            path_id: 9001,
            class_id: MESH_RENDERER_CLASS_ID,
        }
    );
    assert_eq!(assignments[0].file_id, 9001);
}

#[test]
fn lightmap_settings_lighting_data_asset_capability_matches_source_version_fixture() {
    let mut row_count = 0usize;
    for line in include_str!("../../lightmap_settings_capability_fixture.txt").lines() {
        let fields = line.split('|').collect::<Vec<_>>();
        assert_eq!(fields.len(), 4, "invalid source fixture row {line:?}");
        assert_eq!(fields[0], "VERSION", "invalid source fixture row {line:?}");
        let version_text = fields[1];
        assert!(
            fields[2].starts_with("LightmapSettings_"),
            "missing generated class in source fixture row {line:?}"
        );
        let expected = match fields[3] {
            "True" => true,
            "False" => false,
            value => panic!("invalid Has_LightingDataAsset value {value:?}"),
        };
        let version = SceneUnityVersion::parse(version_text)
            .unwrap_or_else(|| panic!("invalid Unity version in source fixture row {line:?}"));
        assert_eq!(
            lightmap_settings_has_lighting_data_asset(version),
            expected,
            "Unity {version_text} Has_LightingDataAsset"
        );
        row_count += 1;
    }
    assert_eq!(row_count, 32, "source version fixture row count changed");
}

#[test]
fn lighting_data_asset_field_match_uses_the_terminal_typed_field_name() {
    assert!(is_lighting_data_asset_field_path("m_LightingDataAsset"));
    assert!(is_lighting_data_asset_field_path(
        "LightmapSettings.m_LightingDataAsset"
    ));
    assert!(!is_lighting_data_asset_field_path(
        "m_LightingDataAssetBackup"
    ));
    assert!(!is_lighting_data_asset_field_path("m_LightmapData"));
}

#[test]
fn recognizes_source_scene_hierarchy_asset_class_ids() {
    assert_eq!(
        scene_asset_category(Some(1)),
        Some(WorkspaceSceneAssetCategory::GameObject)
    );
    assert_eq!(
        scene_asset_category(Some(29)),
        Some(WorkspaceSceneAssetCategory::LevelGameManager(
            SceneManagerKind::OcclusionCullingSettings
        ))
    );
    assert_eq!(
        scene_asset_category(Some(104)),
        Some(WorkspaceSceneAssetCategory::LevelGameManager(
            SceneManagerKind::RenderSettings
        ))
    );
    assert_eq!(
        scene_asset_category(Some(157)),
        Some(WorkspaceSceneAssetCategory::LevelGameManager(
            SceneManagerKind::LightmapSettings
        ))
    );
    assert_eq!(
        scene_asset_category(Some(196)),
        Some(WorkspaceSceneAssetCategory::LevelGameManager(
            SceneManagerKind::NavMeshSettings
        ))
    );
    assert_eq!(
        scene_asset_category(Some(1001)),
        Some(WorkspaceSceneAssetCategory::PrefabInstance)
    );
    assert_eq!(
        scene_asset_category(Some(1_660_057_539)),
        Some(WorkspaceSceneAssetCategory::SceneRoots)
    );
    assert_eq!(scene_asset_category(Some(114)), None);
}

#[test]
fn component_pointer_field_match_is_limited_to_inherited_game_object_field() {
    assert!(is_game_object_pointer_path("m_GameObject"));
    assert!(is_game_object_pointer_path("Base.m_GameObject"));
    assert!(is_game_object_pointer_path("Base.Base.m_GameObject"));
    assert!(!is_game_object_pointer_path("Other.m_GameObject"));
    assert!(!is_game_object_pointer_path("m_GameObject.child"));
    assert!(!is_game_object_pointer_path("m_GameObject[0]"));
}
