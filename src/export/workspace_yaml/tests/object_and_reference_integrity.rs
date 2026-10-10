use super::*;

#[test]
fn exports_workspace_gameobject_component_and_transform_back_reference() {
    let (filesystem, workspace) = load_workspaces(TRANSFORM_PATH_ID);
    let objects =
        load_objects_with_test_layouts(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .unwrap();
    let reference_graph = workspace
        .reference_graph(MultiFileReferenceGraphLimits::default())
        .unwrap();
    assert!(reference_graph.is_complete());
    assert_eq!(
        reference_graph.references()[0].field_path,
        "m_Component[0].component"
    );
    assert_eq!(
        reference_graph.references()[0].resolution,
        MultiFilePPtrResolution::Object {
            target_file_index: 0,
            target_object_index: 1,
        }
    );

    let pointer_map = pointer_map();
    let game_object = workspace_object_to_unity_yaml_document(
        &workspace,
        &reference_graph,
        &pointer_map,
        &objects.objects()[0],
        asset(0, GAME_OBJECT_PATH_ID, 1),
        GAME_OBJECT_PATH_ID,
        COLLECTION_INDEX,
        None,
        UnityYamlObjectConversionLimits::default(),
    )
    .unwrap();
    let transform = workspace_object_to_unity_yaml_document(
        &workspace,
        &reference_graph,
        &pointer_map,
        &objects.objects()[1],
        asset(1, TRANSFORM_PATH_ID, 4),
        TRANSFORM_PATH_ID,
        COLLECTION_INDEX,
        None,
        UnityYamlObjectConversionLimits::default(),
    )
    .unwrap();
    let output = crate::export::unity_yaml::write_unity_yaml_documents(
        &[game_object, transform],
        crate::export::unity_yaml::UnityYamlWriteLimits::default(),
    )
    .unwrap();
    assert!(output.contains("--- !u!1 &42"));
    assert!(output.contains("serializedVersion: 6"));
    assert!(output.contains("component: {fileID: 43}"));
    assert!(output.contains("m_GameObject: {fileID: 42}"));
}

#[test]
fn scene_component_membership_ids_flow_through_workspace_yaml_export() {
    const COMPONENT_CLASS_ID: i32 = 23;
    const COMPONENT_PATH_ID: i64 = 9001;

    let bytes = make_mesh_renderer_component_file();
    let (filesystem, workspace) = load_workspace_bytes(&bytes);
    let objects =
        load_objects_with_test_layouts(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .unwrap();
    assert!(objects.report().is_complete());
    let reference_graph = workspace
        .reference_graph(MultiFileReferenceGraphLimits::default())
        .unwrap();
    assert!(reference_graph.is_complete());
    let assignments = plan_single_file_scene_assignments(&bytes, &reference_graph);
    assert_eq!(assignments.len(), 1);
    assert_eq!(assignments[0].file_id, COMPONENT_PATH_ID);
    assert_eq!(assignments[0].asset.class_id, COMPONENT_CLASS_ID);

    let mut pointer_map_builder =
        ProjectExportPointerMapBuilder::new(ExportPointerMapLimits::default());
    pointer_map_builder
        .register_scene_collection(COLLECTION_INDEX, [0x71; 16], &assignments)
        .unwrap();
    let pointer_map = pointer_map_builder.build().unwrap();
    let output = workspace_scene_assignments_to_unity_yaml(
        &workspace,
        &objects,
        &reference_graph,
        &pointer_map,
        &assignments,
        COLLECTION_INDEX,
        None,
        UnityYamlWorkspaceCollectionLimits::default(),
    )
    .unwrap();

    assert!(output.contains("--- !u!23 &9001"));
    assert!(output.contains("MeshRenderer:"));
    assert!(output.contains("m_GameObject: {fileID: 0}"));
}

#[test]
fn scene_gameobject_component_nonnull_pptrs_follow_ids_into_yaml() {
    let bytes = make_game_object_mesh_renderer_file();
    let (filesystem, workspace) = load_workspace_bytes(&bytes);
    let objects =
        load_objects_with_test_layouts(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .unwrap();
    assert!(objects.report().is_complete());
    let reference_graph = workspace
        .reference_graph(MultiFileReferenceGraphLimits::default())
        .unwrap();
    assert!(reference_graph.is_complete());
    assert_eq!(reference_graph.file_object_count(0), Some(2));
    assert_eq!(
        reference_graph.references()[0].resolution,
        crate::serialized_file::reference_graph::MultiFilePPtrResolution::Object {
            target_file_index: 0,
            target_object_index: 1,
        }
    );
    assert_eq!(
        reference_graph.references()[1].resolution,
        crate::serialized_file::reference_graph::MultiFilePPtrResolution::Object {
            target_file_index: 0,
            target_object_index: 0,
        }
    );

    let assignments = plan_single_file_scene_assignments(&bytes, &reference_graph);
    assert_eq!(
        assignments
            .iter()
            .map(|assignment| (assignment.asset.object_index, assignment.file_id))
            .collect::<Vec<_>>(),
        [(0, GAME_OBJECT_PATH_ID), (1, 9001)]
    );
    let mut pointer_map_builder =
        ProjectExportPointerMapBuilder::new(ExportPointerMapLimits::default());
    pointer_map_builder
        .register_scene_collection(COLLECTION_INDEX, [0x71; 16], &assignments)
        .unwrap();
    let pointer_map = pointer_map_builder.build().unwrap();
    let output = workspace_scene_assignments_to_unity_yaml(
        &workspace,
        &objects,
        &reference_graph,
        &pointer_map,
        &assignments,
        COLLECTION_INDEX,
        None,
        UnityYamlWorkspaceCollectionLimits::default(),
    )
    .unwrap();

    assert!(output.find("--- !u!1 &42").unwrap() < output.find("--- !u!23 &9001").unwrap());
    assert!(output.contains("component: {fileID: 9001}"));
    assert!(output.contains("m_GameObject: {fileID: 42}"));
}

#[test]
fn scene_component_pointers_to_other_collections_emit_guid_and_type() {
    let bytes = make_game_object_mesh_renderer_file();
    let (filesystem, workspace) = load_workspace_bytes(&bytes);
    let objects =
        load_objects_with_test_layouts(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .unwrap();
    let reference_graph = workspace
        .reference_graph(MultiFileReferenceGraphLimits::default())
        .unwrap();
    let assignments = plan_single_file_scene_assignments(&bytes, &reference_graph);
    assert_eq!(assignments.len(), 2);

    let mut pointer_map_builder =
        ProjectExportPointerMapBuilder::new(ExportPointerMapLimits::default());
    pointer_map_builder
        .register_scene_collection(COLLECTION_INDEX, [0x71; 16], &assignments[..1])
        .unwrap();
    pointer_map_builder
        .register_scene_collection(COLLECTION_INDEX + 1, [0x82; 16], &assignments[1..])
        .unwrap();
    let pointer_map = pointer_map_builder.build().unwrap();
    let game_object_yaml = workspace_assignments_to_unity_yaml(
        &workspace,
        &objects,
        &reference_graph,
        &pointer_map,
        &assignments[..1],
        COLLECTION_INDEX,
        None,
        UnityYamlWorkspaceCollectionLimits::default(),
    )
    .unwrap();
    let mesh_renderer_yaml = workspace_assignments_to_unity_yaml(
        &workspace,
        &objects,
        &reference_graph,
        &pointer_map,
        &assignments[1..],
        COLLECTION_INDEX + 1,
        None,
        UnityYamlWorkspaceCollectionLimits::default(),
    )
    .unwrap();

    assert!(
        game_object_yaml
            .contains("component: {fileID: 9001, guid: 28282828282828282828282828282828, type: 2}"),
        "{game_object_yaml}"
    );
    assert!(
        mesh_renderer_yaml.contains(
            "m_GameObject: {fileID: 42, guid: 17171717171717171717171717171717, type: 2}"
        ),
        "{mesh_renderer_yaml}"
    );
}

#[test]
fn cross_file_component_pptrs_reach_separate_scene_yaml_collections() {
    let game_object_bytes = make_game_object_with_external_component();
    let mesh_renderer_bytes = make_mesh_renderer_with_external_game_object();
    let source_files = [
        ("main.assets", game_object_bytes.as_slice()),
        ("shared.assets", mesh_renderer_bytes.as_slice()),
    ];
    let (filesystem, workspace) = load_workspace_files(&source_files);
    let objects =
        load_objects_with_test_layouts(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .unwrap();
    assert!(objects.report().is_complete());
    let reference_graph = workspace
        .reference_graph(MultiFileReferenceGraphLimits::default())
        .unwrap();
    assert!(reference_graph.is_complete());
    assert_eq!(reference_graph.file_object_count(0), Some(1));
    assert_eq!(reference_graph.file_object_count(1), Some(1));
    assert_eq!(reference_graph.references().len(), 2);
    assert_eq!(
        reference_graph.references()[0].resolution,
        crate::serialized_file::reference_graph::MultiFilePPtrResolution::Object {
            target_file_index: 1,
            target_object_index: 0,
        }
    );
    assert_eq!(
        reference_graph.references()[1].resolution,
        crate::serialized_file::reference_graph::MultiFilePPtrResolution::Object {
            target_file_index: 0,
            target_object_index: 0,
        }
    );

    let assignments = plan_scene_assignments_for_workspace(&source_files, &reference_graph);
    assert_eq!(assignments.len(), 2);
    assert_eq!(assignments[0].asset.path_id, 42);
    assert_eq!(assignments[1].asset.path_id, 9001);
    assert_eq!(
        assignments
            .iter()
            .map(|assignment| assignment.file_id)
            .collect::<Vec<_>>(),
        [42, 9001]
    );

    let mut pointer_map_builder =
        ProjectExportPointerMapBuilder::new(ExportPointerMapLimits::default());
    pointer_map_builder
        .register_scene_collection(COLLECTION_INDEX, [0x71; 16], &assignments[..1])
        .unwrap();
    pointer_map_builder
        .register_scene_collection(COLLECTION_INDEX + 1, [0x82; 16], &assignments[1..])
        .unwrap();
    let pointer_map = pointer_map_builder.build().unwrap();
    let game_object_yaml = workspace_assignments_to_unity_yaml(
        &workspace,
        &objects,
        &reference_graph,
        &pointer_map,
        &assignments[..1],
        COLLECTION_INDEX,
        None,
        UnityYamlWorkspaceCollectionLimits::default(),
    )
    .unwrap();
    let mesh_renderer_yaml = workspace_assignments_to_unity_yaml(
        &workspace,
        &objects,
        &reference_graph,
        &pointer_map,
        &assignments[1..],
        COLLECTION_INDEX + 1,
        None,
        UnityYamlWorkspaceCollectionLimits::default(),
    )
    .unwrap();

    assert!(game_object_yaml.contains("--- !u!1 &42"));
    assert!(mesh_renderer_yaml.contains("--- !u!23 &9001"));
    assert!(game_object_yaml
        .contains("component: {fileID: 9001, guid: 28282828282828282828282828282828, type: 2}"));
    assert!(mesh_renderer_yaml
        .contains("m_GameObject: {fileID: 42, guid: 17171717171717171717171717171717, type: 2}"));
}

#[test]
fn pptr_dictionary_values_resolve_and_emit_exported_collection_pointer() {
    const SOURCE_PATH_ID: i64 = 51;
    const TARGET_COLLECTION_INDEX: usize = COLLECTION_INDEX + 1;
    const TARGET_FILE_ID: i64 = 142;

    let dictionary_bytes = make_game_object_with_external_pptr_dictionary();
    let target_bytes = make_game_object_and_transform_file(TRANSFORM_PATH_ID);
    let source_files = [
        ("main.assets", dictionary_bytes.as_slice()),
        ("target.assets", target_bytes.as_slice()),
    ];
    let (filesystem, workspace) = load_workspace_files(&source_files);
    let objects =
        load_objects_with_test_layouts(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .unwrap();
    assert!(objects.report().is_complete());

    let reference_graph = workspace
        .reference_graph(MultiFileReferenceGraphLimits::default())
        .unwrap();
    assert!(reference_graph.is_complete());
    let dictionary_edges: Vec<_> = reference_graph.references_from_object(0, 0).collect();
    assert_eq!(dictionary_edges.len(), 1);
    assert_eq!(dictionary_edges[0].field_path, "m_Targets[0].second");
    assert_eq!(dictionary_edges[0].file_id, 1);
    assert_eq!(dictionary_edges[0].path_id, GAME_OBJECT_PATH_ID);
    assert_eq!(
        dictionary_edges[0].resolution,
        MultiFilePPtrResolution::Object {
            target_file_index: 1,
            target_object_index: 0,
        }
    );

    let target_asset = ExportAssetIdentity {
        collection: ExportAssetCollectionIdentity::SerializedFile { file_index: 1 },
        object_index: 0,
        path_id: GAME_OBJECT_PATH_ID,
        class_id: 1,
    };
    let pointer_map = ProjectExportPointerMap::new(
        &[ExportedAssetPointerEntry {
            asset: target_asset,
            file_id: TARGET_FILE_ID,
            collection_index: TARGET_COLLECTION_INDEX,
        }],
        &[ExportCollectionPointerMetadata {
            collection_index: TARGET_COLLECTION_INDEX,
            guid: [0x82; 16],
            asset_type: UnityProjectAssetType::Serialized,
        }],
        ExportPointerMapLimits::default(),
    )
    .unwrap();
    let document = workspace_object_to_unity_yaml_document(
        &workspace,
        &reference_graph,
        &pointer_map,
        &objects.objects()[0],
        asset(0, SOURCE_PATH_ID, 1),
        SOURCE_PATH_ID,
        COLLECTION_INDEX,
        None,
        UnityYamlObjectConversionLimits::default(),
    )
    .unwrap();
    let yaml = crate::export::unity_yaml::write_unity_yaml_documents(
        &[document],
        crate::export::unity_yaml::UnityYamlWriteLimits::default(),
    )
    .unwrap();

    assert!(yaml.contains("--- !u!1 &51"), "{yaml}");
    assert!(
        yaml.contains("target: {fileID: 142, guid: 28282828282828282828282828282828, type: 2}"),
        "{yaml}"
    );
}

#[test]
fn refuses_workspace_payload_bytes_or_reference_edges_that_do_not_match() {
    let (filesystem, workspace) = load_workspaces(TRANSFORM_PATH_ID);
    let objects =
        load_objects_with_test_layouts(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .unwrap();
    let reference_graph = workspace
        .reference_graph(MultiFileReferenceGraphLimits::default())
        .unwrap();
    let pointer_map = pointer_map();

    let mut with_trailing_bytes = objects.objects()[0].clone();
    with_trailing_bytes
        .payload
        .as_mut()
        .unwrap()
        .trailing_bytes
        .push(0xaa);
    assert!(matches!(
        workspace_object_to_unity_yaml_document(
            &workspace,
            &reference_graph,
            &pointer_map,
            &with_trailing_bytes,
            asset(0, GAME_OBJECT_PATH_ID, 1),
            GAME_OBJECT_PATH_ID,
            COLLECTION_INDEX,
            None,
            UnityYamlObjectConversionLimits::default(),
        ),
        Err(UnityYamlWorkspaceExportError::UnparsedTrailingPayload { byte_count: 1, .. })
    ));

    let mut with_wrong_pointer = objects.objects()[0].clone();
    let payload = with_wrong_pointer.payload.as_mut().unwrap();
    let crate::serialized_file::object::SerializedValue::Struct(fields) = &mut payload.root else {
        panic!("fixture root is a struct")
    };
    let crate::serialized_file::object::SerializedValue::Array(components) = &mut fields[0].value
    else {
        panic!("fixture component field is an array")
    };
    let crate::serialized_file::object::SerializedValue::Struct(pair_fields) = &mut components[0]
    else {
        panic!("fixture component value is a struct")
    };
    pair_fields[0].value = crate::serialized_file::object::SerializedValue::PPtr {
        file_id: 0,
        path_id: 999,
    };
    assert!(matches!(
        workspace_object_to_unity_yaml_document(
            &workspace,
            &reference_graph,
            &pointer_map,
            &with_wrong_pointer,
            asset(0, GAME_OBJECT_PATH_ID, 1),
            GAME_OBJECT_PATH_ID,
            COLLECTION_INDEX,
            None,
            UnityYamlObjectConversionLimits::default(),
        ),
        Err(UnityYamlWorkspaceExportError::ReferenceEdgeMissing { path_id: 999, .. })
    ));
}

#[test]
fn missing_local_component_reference_remains_an_export_error() {
    let (filesystem, workspace) = load_workspaces(999);
    let objects =
        load_objects_with_test_layouts(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .unwrap();
    let reference_graph = workspace
        .reference_graph(MultiFileReferenceGraphLimits::default())
        .unwrap();
    let pointer_map = pointer_map();
    assert!(matches!(
        workspace_object_to_unity_yaml_document(
            &workspace,
            &reference_graph,
            &pointer_map,
            &objects.objects()[0],
            asset(0, GAME_OBJECT_PATH_ID, 1),
            GAME_OBJECT_PATH_ID,
            COLLECTION_INDEX,
            None,
            UnityYamlObjectConversionLimits::default(),
        ),
        Err(UnityYamlWorkspaceExportError::Conversion(
            UnityYamlObjectConversionError::PointerConversion { .. }
        ))
    ));
}

#[test]
fn stripped_marker_needs_hierarchy_and_set_membership() {
    // `StrippedAssetExtensions.IsStripped`: both facts must hold.
    assert!(is_stripped_document(true, true));
    assert!(!is_stripped_document(true, false));
    assert!(!is_stripped_document(false, true));
    assert!(!is_stripped_document(false, false));
}
