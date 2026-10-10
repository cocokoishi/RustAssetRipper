use super::*;

#[test]
fn workspace_yaml_export_resolves_and_embeds_streamed_image_texture_bytes() {
    let source_bytes = make_streamed_asset_file(189, false);
    let (mut filesystem, workspace) = load_workspace_bytes(&source_bytes);
    filesystem
        .write_all_bytes("/game/texture.resS", b"0123456789")
        .unwrap();
    let catalog = explicit_root_resource_catalog(&filesystem, "texture.resS", "/game/texture.resS");
    let objects =
        load_objects_with_test_layouts(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .unwrap();
    let reference_graph = workspace
        .reference_graph(MultiFileReferenceGraphLimits::default())
        .unwrap();
    assert!(reference_graph.is_complete());
    let identity = asset(0, 77, 189);
    let pointer_map = pointer_map_for_asset(identity);
    let provider = PlatformResourceProvider::new(None, None);
    let context = WorkspaceStreamedYamlResourceContext {
        filesystem: &filesystem,
        root_catalog: &catalog,
        provider: &provider,
        content_limits: StreamedResourceContentLimits::default(),
        staging_limits: StreamedYamlAssetLimits::default(),
    };

    let document = workspace_object_to_unity_yaml_document_with_streamed_resources(
        &workspace,
        &reference_graph,
        &pointer_map,
        &objects.objects()[0],
        identity,
        77,
        COLLECTION_INDEX,
        None,
        UnityYamlObjectConversionLimits::default(),
        &context,
    )
    .unwrap()
    .expect("streamed image texture should be exported");
    let yaml = write_unity_yaml_documents(&[document], UnityYamlWriteLimits::default()).unwrap();

    assert!(yaml.contains("m_ImageData_C189: 323334"));
    assert!(yaml.contains("offset: 0"));
    assert!(yaml.contains("size: 0"));
    assert!(!yaml.contains("texture.resS"));

    let assignment = SceneExportIdAssignment {
        asset: identity,
        file_id: 77,
    };
    let collection_yaml = workspace_assignments_to_unity_yaml_with_streamed_resources(
        &workspace,
        &objects,
        &reference_graph,
        &pointer_map,
        &[assignment],
        COLLECTION_INDEX,
        None,
        UnityYamlWorkspaceCollectionLimits::default(),
        &context,
    )
    .unwrap();
    assert!(collection_yaml.contains("m_ImageData_C189: 323334"));
    let scene_yaml = workspace_scene_assignments_to_unity_yaml_with_streamed_resources(
        &workspace,
        &objects,
        &reference_graph,
        &pointer_map,
        &[assignment],
        COLLECTION_INDEX,
        None,
        UnityYamlWorkspaceCollectionLimits::default(),
        &context,
    )
    .unwrap();
    assert!(scene_yaml.contains("m_ImageData_C189: 323334"));

    let payload = objects.objects()[0].payload.as_ref().unwrap();
    assert_eq!(
        crate::resource::extract_streamed_resource_info(
            payload
                .root
                .unique_field_in_base_chain("m_StreamData_C189")
                .unwrap()
                .unwrap(),
        )
        .unwrap()
        .path,
        "texture.resS"
    );
    assert_eq!(
        payload
            .root
            .unique_field_in_base_chain("m_ImageData_C189")
            .unwrap()
            .unwrap(),
        &crate::serialized_file::object::SerializedValue::Bytes(Vec::new())
    );
}

#[test]
fn workspace_yaml_export_handles_every_generated_image_texture_interface_class() {
    for class_id in [28, 89, 117, 187, 188, 189] {
        let source_bytes = make_streamed_asset_file(class_id, false);
        let (mut filesystem, workspace) = load_workspace_bytes(&source_bytes);
        filesystem
            .write_all_bytes("/game/texture.resS", b"0123456789")
            .unwrap();
        let catalog =
            explicit_root_resource_catalog(&filesystem, "texture.resS", "/game/texture.resS");
        let objects = load_objects_with_test_layouts(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert!(reference_graph.is_complete(), "class ID {class_id}");
        assert!(
            crate::export::project_exporter::class_interfaces(class_id).contains(&"IImageTexture"),
            "class ID {class_id} must come from the frozen generated interface table"
        );
        let identity = asset(0, 77, class_id);
        let pointer_map = pointer_map_for_asset(identity);
        let provider = PlatformResourceProvider::new(None, None);
        let context = WorkspaceStreamedYamlResourceContext {
            filesystem: &filesystem,
            root_catalog: &catalog,
            provider: &provider,
            content_limits: StreamedResourceContentLimits::default(),
            staging_limits: StreamedYamlAssetLimits::default(),
        };

        let document = workspace_object_to_unity_yaml_document_with_streamed_resources(
            &workspace,
            &reference_graph,
            &pointer_map,
            &objects.objects()[0],
            identity,
            77,
            COLLECTION_INDEX,
            None,
            UnityYamlObjectConversionLimits::default(),
            &context,
        )
        .unwrap()
        .unwrap_or_else(|| panic!("class {class_id} unexpectedly skipped"));
        let yaml =
            write_unity_yaml_documents(&[document], UnityYamlWriteLimits::default()).unwrap();
        assert!(yaml.contains("323334"), "class ID {class_id}: {yaml}");
        assert!(yaml.contains("offset: 0"), "class ID {class_id}: {yaml}");
        assert!(yaml.contains("size: 0"), "class ID {class_id}: {yaml}");
        assert!(
            !yaml.contains("texture.resS"),
            "class ID {class_id}: {yaml}"
        );

        let payload = objects.objects()[0].payload.as_ref().unwrap();
        let stream_field = if class_id == 189 {
            "m_StreamData_C189"
        } else {
            "m_StreamData"
        };
        assert_eq!(
            crate::resource::extract_streamed_resource_info(
                payload
                    .root
                    .unique_field_in_base_chain(stream_field)
                    .unwrap()
                    .unwrap(),
            )
            .unwrap()
            .path,
            "texture.resS",
            "source stream remains intact for class ID {class_id}"
        );
    }
}

#[test]
fn workspace_yaml_collection_skips_image_texture_with_unset_or_zero_length_stream() {
    for (stream_path, stream_size) in [("", 3), ("texture.resS", 0)] {
        let source_bytes =
            make_streamed_asset_file_with_stream_info(189, false, stream_path, stream_size);
        let (filesystem, workspace) = load_workspace_bytes(&source_bytes);
        let empty_inputs: [ExplicitRootResourceInput<'_, '_>; 0] = [];
        let catalog = RootResourceFileCatalog::from_explicit_resource_inputs(
            &filesystem,
            &empty_inputs,
            RootResourceCatalogLimits::default(),
        )
        .unwrap();
        let objects = load_objects_with_test_layouts(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let reference_graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert!(reference_graph.is_complete());
        let identity = asset(0, 77, 189);
        let pointer_map = pointer_map_for_asset(identity);
        let provider = PlatformResourceProvider::new(None, None);
        let context = WorkspaceStreamedYamlResourceContext {
            filesystem: &filesystem,
            root_catalog: &catalog,
            provider: &provider,
            content_limits: StreamedResourceContentLimits::default(),
            staging_limits: StreamedYamlAssetLimits::default(),
        };

        let document = workspace_object_to_unity_yaml_document_with_streamed_resources(
            &workspace,
            &reference_graph,
            &pointer_map,
            &objects.objects()[0],
            identity,
            77,
            COLLECTION_INDEX,
            None,
            UnityYamlObjectConversionLimits::default(),
            &context,
        )
        .unwrap();
        assert!(document.is_none());

        let assignment = SceneExportIdAssignment {
            asset: identity,
            file_id: 77,
        };
        let yaml = workspace_assignments_to_unity_yaml_with_streamed_resources(
            &workspace,
            &objects,
            &reference_graph,
            &pointer_map,
            &[assignment],
            COLLECTION_INDEX,
            None,
            UnityYamlWorkspaceCollectionLimits::default(),
            &context,
        )
        .unwrap();
        assert!(!yaml.contains("--- !u!189"));
        assert!(!yaml.contains("ImageTexture:"));
    }
}

#[test]
fn workspace_yaml_collection_preserves_multi_asset_order_and_streamed_bytes() {
    let first_bytes =
        make_streamed_asset_file_with_inline_data(189, false, "first.resS", 3, b"ORIG");
    let second_bytes = make_streamed_asset_file_with_stream_info(189, false, "second.resS", 3);
    let source_files = [
        ("first.assets", first_bytes.as_slice()),
        ("second.assets", second_bytes.as_slice()),
    ];
    let (mut filesystem, workspace) = load_workspace_files(&source_files);
    filesystem
        .write_all_bytes("/game/second.resS", b"yyXYZ!")
        .unwrap();
    let catalog = explicit_root_resource_catalog(&filesystem, "second.resS", "/game/second.resS");
    let objects =
        load_objects_with_test_layouts(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .unwrap();
    let reference_graph = workspace
        .reference_graph(MultiFileReferenceGraphLimits::default())
        .unwrap();
    assert!(reference_graph.is_complete());

    let first = asset(0, 77, 189);
    let second = ExportAssetIdentity {
        collection: ExportAssetCollectionIdentity::SerializedFile { file_index: 1 },
        object_index: 0,
        path_id: 77,
        class_id: 189,
    };
    let pointer_map = pointer_map_for_assets(&[first, second]);
    let provider = PlatformResourceProvider::new(None, None);
    let context = WorkspaceStreamedYamlResourceContext {
        filesystem: &filesystem,
        root_catalog: &catalog,
        provider: &provider,
        content_limits: StreamedResourceContentLimits::default(),
        staging_limits: StreamedYamlAssetLimits::default(),
    };
    let assignments = [
        SceneExportIdAssignment {
            asset: second,
            file_id: 202,
        },
        SceneExportIdAssignment {
            asset: first,
            file_id: 101,
        },
    ];

    let collection_yaml = workspace_assignments_to_unity_yaml_with_streamed_resources(
        &workspace,
        &objects,
        &reference_graph,
        &pointer_map,
        &assignments,
        COLLECTION_INDEX,
        None,
        UnityYamlWorkspaceCollectionLimits::default(),
        &context,
    )
    .unwrap();
    assert_yaml_asset_order_and_streamed_bytes(&collection_yaml);

    let scene_yaml = workspace_scene_assignments_to_unity_yaml_with_streamed_resources(
        &workspace,
        &objects,
        &reference_graph,
        &pointer_map,
        &assignments,
        COLLECTION_INDEX,
        None,
        UnityYamlWorkspaceCollectionLimits::default(),
        &context,
    )
    .unwrap();
    assert_yaml_asset_order_and_streamed_bytes(&scene_yaml);

    let first_payload = objects.objects()[0].payload.as_ref().unwrap();
    assert_eq!(
        crate::resource::extract_streamed_resource_info(
            first_payload
                .root
                .unique_field_in_base_chain("m_StreamData_C189")
                .unwrap()
                .unwrap(),
        )
        .unwrap()
        .path,
        "first.resS"
    );
    assert_eq!(
        first_payload
            .root
            .unique_field_in_base_chain("m_ImageData_C189")
            .unwrap()
            .unwrap(),
        &crate::serialized_file::object::SerializedValue::Bytes(b"ORIG".to_vec())
    );

    let second_payload = objects.objects()[1].payload.as_ref().unwrap();
    assert_eq!(
        crate::resource::extract_streamed_resource_info(
            second_payload
                .root
                .unique_field_in_base_chain("m_StreamData_C189")
                .unwrap()
                .unwrap(),
        )
        .unwrap()
        .path,
        "second.resS"
    );
    assert_eq!(
        second_payload
            .root
            .unique_field_in_base_chain("m_ImageData_C189")
            .unwrap()
            .unwrap(),
        &crate::serialized_file::object::SerializedValue::Bytes(Vec::new())
    );
}

#[test]
fn workspace_yaml_export_resolves_and_embeds_streamed_mesh_bytes() {
    let source_bytes = make_streamed_asset_file(43, true);
    let (mut filesystem, workspace) = load_workspace_bytes(&source_bytes);
    filesystem
        .write_all_bytes("/game/texture.resS", b"0123456789")
        .unwrap();
    let catalog = explicit_root_resource_catalog(&filesystem, "texture.resS", "/game/texture.resS");
    let objects =
        load_objects_with_test_layouts(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .unwrap();
    let reference_graph = workspace
        .reference_graph(MultiFileReferenceGraphLimits::default())
        .unwrap();
    assert!(reference_graph.is_complete());
    let identity = asset(0, 77, 43);
    let pointer_map = pointer_map_for_asset(identity);
    let provider = PlatformResourceProvider::new(None, None);
    let context = WorkspaceStreamedYamlResourceContext {
        filesystem: &filesystem,
        root_catalog: &catalog,
        provider: &provider,
        content_limits: StreamedResourceContentLimits::default(),
        staging_limits: StreamedYamlAssetLimits::default(),
    };

    let document = workspace_object_to_unity_yaml_document_with_streamed_resources(
        &workspace,
        &reference_graph,
        &pointer_map,
        &objects.objects()[0],
        identity,
        77,
        COLLECTION_INDEX,
        None,
        UnityYamlObjectConversionLimits::default(),
        &context,
    )
    .unwrap()
    .expect("streamed Mesh should be exported");
    let yaml = write_unity_yaml_documents(&[document], UnityYamlWriteLimits::default()).unwrap();

    assert!(yaml.contains("m_Data: 323334"));
    assert!(yaml.contains("offset: 0"));
    assert!(yaml.contains("size: 0"));
    assert!(!yaml.contains("texture.resS"));
    let payload = objects.objects()[0].payload.as_ref().unwrap();
    assert_eq!(
        crate::resource::extract_streamed_resource_info(
            payload
                .root
                .unique_field_in_base_chain("m_StreamData")
                .unwrap()
                .unwrap(),
        )
        .unwrap()
        .path,
        "texture.resS"
    );
    assert_eq!(
        payload
            .root
            .unique_field_in_base_chain("m_VertexData")
            .unwrap()
            .unwrap()
            .unique_field_in_base_chain("m_DataSize")
            .unwrap()
            .unwrap(),
        &crate::serialized_file::object::SerializedValue::Bytes(Vec::new())
    );
}

#[test]
fn workspace_yaml_export_reports_missing_streamed_resource_without_null_or_empty_data() {
    let source_bytes = make_streamed_asset_file(189, true);
    let (filesystem, workspace) = load_workspace_bytes(&source_bytes);
    let empty_inputs: [ExplicitRootResourceInput<'_, '_>; 0] = [];
    let catalog = RootResourceFileCatalog::from_explicit_resource_inputs(
        &filesystem,
        &empty_inputs,
        RootResourceCatalogLimits::default(),
    )
    .unwrap();
    let objects =
        load_objects_with_test_layouts(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .unwrap();
    let reference_graph = workspace
        .reference_graph(MultiFileReferenceGraphLimits::default())
        .unwrap();
    let identity = asset(0, 77, 189);
    let pointer_map = pointer_map_for_asset(identity);
    let provider = PlatformResourceProvider::new(None, None);
    let context = WorkspaceStreamedYamlResourceContext {
        filesystem: &filesystem,
        root_catalog: &catalog,
        provider: &provider,
        content_limits: StreamedResourceContentLimits::default(),
        staging_limits: StreamedYamlAssetLimits::default(),
    };

    let no_context_error = workspace_object_to_unity_yaml_document(
        &workspace,
        &reference_graph,
        &pointer_map,
        &objects.objects()[0],
        identity,
        77,
        COLLECTION_INDEX,
        None,
        UnityYamlObjectConversionLimits::default(),
    )
    .unwrap_err();
    match no_context_error {
        UnityYamlWorkspaceExportError::StreamedAssetPreparation { source, .. } => {
            assert!(matches!(
                *source,
                StreamedYamlAssetError::StreamContentRequired { resource }
                    if resource.path == "texture.resS" && resource.offset == 2 && resource.size == 3
            ))
        }
        other => panic!("expected missing resource context error, got {other}"),
    }

    let copy_limited_context = WorkspaceStreamedYamlResourceContext {
        staging_limits: StreamedYamlAssetLimits {
            max_source_value_copy_bytes: 1,
            ..StreamedYamlAssetLimits::default()
        },
        ..context
    };
    let copy_limit_error = workspace_object_to_unity_yaml_document_with_streamed_resources(
        &workspace,
        &reference_graph,
        &pointer_map,
        &objects.objects()[0],
        identity,
        77,
        COLLECTION_INDEX,
        None,
        UnityYamlObjectConversionLimits::default(),
        &copy_limited_context,
    )
    .unwrap_err();
    match copy_limit_error {
        UnityYamlWorkspaceExportError::StreamedAssetPreparation { source, .. } => {
            assert!(matches!(
                *source,
                StreamedYamlAssetError::SourceValueCopyLimitExceeded {
                    size,
                    limit: 1
                } if size > 1
            ))
        }
        other => panic!("expected source-copy bound, got {other}"),
    }

    assert!(matches!(
        workspace_object_to_unity_yaml_document_with_streamed_resources(
            &workspace,
            &reference_graph,
            &pointer_map,
            &objects.objects()[0],
            identity,
            77,
            COLLECTION_INDEX,
            None,
            UnityYamlObjectConversionLimits::default(),
            &context,
        ),
        Err(UnityYamlWorkspaceExportError::StreamedResourceMissing {
            path,
            unavailable_data_paths,
            ..
        }) if path == "texture.resS" && unavailable_data_paths.is_empty()
    ));
    assert_eq!(
        objects.objects()[0]
            .payload
            .as_ref()
            .unwrap()
            .root
            .unique_field_in_base_chain("m_ImageData_C189")
            .unwrap()
            .unwrap(),
        &crate::serialized_file::object::SerializedValue::Bytes(Vec::new())
    );
}
