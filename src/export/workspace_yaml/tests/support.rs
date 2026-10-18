use super::*;

pub(super) fn asset(object_index: usize, path_id: i64, class_id: i32) -> ExportAssetIdentity {
    ExportAssetIdentity {
        collection: ExportAssetCollectionIdentity::SerializedFile { file_index: 0 },
        object_index,
        path_id,
        class_id,
    }
}

pub(super) fn pointer_map() -> ProjectExportPointerMap {
    let assets = [
        ExportedAssetPointerEntry {
            asset: asset(0, GAME_OBJECT_PATH_ID, 1),
            file_id: GAME_OBJECT_PATH_ID,
            collection_index: COLLECTION_INDEX,
        },
        ExportedAssetPointerEntry {
            asset: asset(1, TRANSFORM_PATH_ID, 4),
            file_id: TRANSFORM_PATH_ID,
            collection_index: COLLECTION_INDEX,
        },
    ];
    let collections = [ExportCollectionPointerMetadata {
        collection_index: COLLECTION_INDEX,
        guid: [0x71; 16],
        asset_type: UnityProjectAssetType::Serialized,
    }];
    ProjectExportPointerMap::new(&assets, &collections, ExportPointerMapLimits::default()).unwrap()
}

pub(super) fn pointer_map_for_asset(asset: ExportAssetIdentity) -> ProjectExportPointerMap {
    pointer_map_for_assets(&[asset])
}

pub(super) fn pointer_map_for_assets(assets: &[ExportAssetIdentity]) -> ProjectExportPointerMap {
    let entries = assets
        .iter()
        .map(|&asset| ExportedAssetPointerEntry {
            asset,
            file_id: asset.path_id,
            collection_index: COLLECTION_INDEX,
        })
        .collect::<Vec<_>>();
    let collections = [ExportCollectionPointerMetadata {
        collection_index: COLLECTION_INDEX,
        guid: [0x72; 16],
        asset_type: UnityProjectAssetType::Serialized,
    }];
    ProjectExportPointerMap::new(&entries, &collections, ExportPointerMapLimits::default()).unwrap()
}

pub(super) fn load_workspaces(target_path_id: i64) -> (VirtualFileSystem, SerializedFileWorkspace) {
    let bytes = make_game_object_and_transform_file(target_path_id);
    load_workspace_bytes(&bytes)
}

pub(crate) fn load_workspace_bytes(bytes: &[u8]) -> (VirtualFileSystem, SerializedFileWorkspace) {
    load_workspace_files(&[("main.assets", bytes)])
}

pub(crate) fn load_workspace_bytes_with_source_factory(
    bytes: &[u8],
) -> (VirtualFileSystem, SerializedFileWorkspace) {
    load_workspace_named_bytes_with_source_factory("main.assets", bytes)
}

pub(crate) fn load_workspace_named_bytes_with_source_factory(
    file_name: &str,
    bytes: &[u8],
) -> (VirtualFileSystem, SerializedFileWorkspace) {
    let mut filesystem = VirtualFileSystem::new();
    filesystem.create_directory(Some("/game")).unwrap();
    let virtual_path = format!("/game/{file_name}");
    filesystem.write_all_bytes(&virtual_path, bytes).unwrap();
    let input = [DependencyFilePath {
        file_name: file_name.to_owned(),
        virtual_path,
    }];
    let workspace = SerializedFileWorkspace::load_from_vfs(
        &filesystem,
        &input,
        SerializedFileWorkspaceLimits::default(),
    )
    .unwrap();
    (filesystem, workspace)
}

pub(super) fn load_workspace_files(
    files: &[(&str, &[u8])],
) -> (VirtualFileSystem, SerializedFileWorkspace) {
    let mut filesystem = VirtualFileSystem::new();
    filesystem.create_directory(Some("/game")).unwrap();
    let mut input = Vec::new();
    for &(file_name, bytes) in files {
        let virtual_path = format!("/game/{file_name}");
        filesystem.write_all_bytes(&virtual_path, bytes).unwrap();
        input.push(DependencyFilePath {
            file_name: file_name.to_owned(),
            virtual_path,
        });
    }
    let workspace = SerializedFileWorkspace::load_from_vfs_with_layout_source(
        &filesystem,
        &input,
        SerializedFileWorkspaceLimits::default(),
        &FileTypeTreeTestLayouts,
    )
    .unwrap();
    (filesystem, workspace)
}

pub(super) fn load_objects_with_test_layouts(
    filesystem: &VirtualFileSystem,
    workspace: &SerializedFileWorkspace,
    limits: ObjectWorkspaceLimits,
) -> Result<ObjectWorkspace, Box<crate::serialized_file::object_workspace::ObjectWorkspaceError>> {
    ObjectWorkspace::load_from_vfs_with_layout_source(
        filesystem,
        workspace,
        limits,
        &FileTypeTreeTestLayouts,
    )
}

pub(super) fn explicit_root_resource_catalog(
    filesystem: &VirtualFileSystem,
    file_name: &str,
    virtual_path: &str,
) -> RootResourceFileCatalog {
    explicit_root_resource_catalog_for_files(filesystem, &[(file_name, virtual_path)])
}

pub(super) fn explicit_root_resource_catalog_for_files(
    filesystem: &VirtualFileSystem,
    resources: &[(&str, &str)],
) -> RootResourceFileCatalog {
    let dispatched = resources
        .iter()
        .map(|_| {
            SchemeReader::read_with_limits(b"root resource", SchemeReaderLimits::default()).unwrap()
        })
        .collect::<Vec<_>>();
    let inputs = resources
        .iter()
        .zip(&dispatched)
        .map(
            |(&(file_name, virtual_path), dispatched_file)| ExplicitRootResourceInput {
                file_name,
                virtual_path,
                dispatched_file,
            },
        )
        .collect::<Vec<_>>();
    RootResourceFileCatalog::from_explicit_resource_inputs(
        filesystem,
        &inputs,
        RootResourceCatalogLimits::default(),
    )
    .unwrap()
}

pub(super) fn make_streamed_asset_file(class_id: i32, offset_is_u64: bool) -> Vec<u8> {
    make_streamed_asset_file_with_stream_path(class_id, offset_is_u64, "texture.resS")
}

pub(super) fn make_streamed_asset_file_with_stream_path(
    class_id: i32,
    offset_is_u64: bool,
    stream_path: &str,
) -> Vec<u8> {
    make_streamed_asset_file_with_stream_info(class_id, offset_is_u64, stream_path, 3)
}

pub(super) fn make_streamed_asset_file_with_stream_info(
    class_id: i32,
    offset_is_u64: bool,
    stream_path: &str,
    stream_size: u32,
) -> Vec<u8> {
    make_streamed_asset_file_with_inline_data(
        class_id,
        offset_is_u64,
        stream_path,
        stream_size,
        &[],
    )
}

pub(super) fn make_streamed_asset_file_with_inline_data(
    class_id: i32,
    offset_is_u64: bool,
    stream_path: &str,
    stream_size: u32,
    inline_data: &[u8],
) -> Vec<u8> {
    let (root_type, stream_field, inline_parent_type, inline_parent_name, inline_field) =
        match class_id {
            43 => (
                "Mesh",
                "m_StreamData",
                "VertexData",
                "m_VertexData",
                "m_DataSize",
            ),
            28 => (
                "Texture2D",
                "m_StreamData",
                "TypelessData",
                "image data",
                "",
            ),
            89 => ("Cubemap", "m_StreamData", "TypelessData", "image data", ""),
            117 => (
                "Texture3D",
                "m_StreamData",
                "TypelessData",
                "image data",
                "",
            ),
            187 => (
                "Texture2DArray",
                "m_StreamData",
                "TypelessData",
                "image data",
                "",
            ),
            188 => (
                "CubemapArray",
                "m_StreamData",
                "TypelessData",
                "image data",
                "",
            ),
            189 => (
                "ImageTexture",
                "m_StreamData_C189",
                "TypelessData",
                "m_ImageData_C189",
                "",
            ),
            _ => panic!("fixture supports Mesh and ImageTexture only"),
        };
    let mut tree = vec![
        (0, 1, root_type, "", -1),
        (1, 1, "StreamingInfo", stream_field, -1),
        (2, 1, "string", "path", -1),
        (3, 1, "Array", "Array", -1),
        (4, 1, "int", "size", 4),
        (4, 1, "char", "data", 1),
        (
            2,
            1,
            if offset_is_u64 { "UInt64" } else { "UInt32" },
            "offset",
            if offset_is_u64 { 8 } else { 4 },
        ),
        (2, 1, "UInt32", "size", 4),
    ];
    if class_id == 43 {
        tree.extend([
            (1, 1, inline_parent_type, inline_parent_name, -1),
            (2, 1, "TypelessData", inline_field, -1),
            (3, 1, "int", "size", 4),
            (3, 1, "UInt8", "data", 1),
        ]);
    } else {
        tree.extend([
            (1, 1, inline_parent_type, inline_parent_name, -1),
            (2, 1, "int", "size", 4),
            (2, 1, "UInt8", "data", 1),
        ]);
    }

    let mut payload = Vec::new();
    payload.extend_from_slice(&i32::try_from(stream_path.len()).unwrap().to_le_bytes());
    payload.extend_from_slice(stream_path.as_bytes());
    let padding = (4 - payload.len() % 4) % 4;
    payload.resize(payload.len() + padding, 0);
    if offset_is_u64 {
        payload.extend_from_slice(&2u64.to_le_bytes());
    } else {
        payload.extend_from_slice(&2u32.to_le_bytes());
    }
    payload.extend_from_slice(&stream_size.to_le_bytes());
    payload.extend_from_slice(&i32::try_from(inline_data.len()).unwrap().to_le_bytes());
    payload.extend_from_slice(inline_data);

    let mut metadata = b"2019.4.0f1\0".to_vec();
    metadata.extend_from_slice(&0u32.to_le_bytes()); // generation-10 long FileID flag
    metadata.extend_from_slice(&1i32.to_le_bytes());
    write_type_tree(&mut metadata, class_id, &tree);
    metadata.extend_from_slice(&0u32.to_le_bytes()); // long FileID flag
    metadata.extend_from_slice(&1i32.to_le_bytes());
    write_object_record(
        &mut metadata,
        77,
        0,
        payload.len(),
        class_id,
        class_id as i16,
    );
    metadata.extend_from_slice(&0i32.to_le_bytes()); // external files
    metadata.push(0); // user information string

    let data_offset = 20 + metadata.len();
    let file_size = data_offset + payload.len();
    let mut file = Vec::with_capacity(file_size);
    file.extend_from_slice(&(metadata.len() as u32).to_be_bytes());
    file.extend_from_slice(&(file_size as u32).to_be_bytes());
    file.extend_from_slice(&10i32.to_be_bytes());
    file.extend_from_slice(&(data_offset as u32).to_be_bytes());
    file.push(0);
    file.extend_from_slice(&[0; 3]);
    file.extend_from_slice(&metadata);
    file.extend_from_slice(&payload);
    file
}

pub(super) fn plan_single_file_scene_assignments(
    bytes: &[u8],
    reference_graph: &MultiFileReferenceGraph<'_>,
) -> Vec<SceneExportIdAssignment> {
    plan_scene_assignments_for_workspace(&[("main.assets", bytes)], reference_graph)
}

pub(super) fn plan_scene_assignments_for_workspace(
    files: &[(&str, &[u8])],
    reference_graph: &MultiFileReferenceGraph<'_>,
) -> Vec<SceneExportIdAssignment> {
    let hierarchies = scene_hierarchies_for_workspace(files, reference_graph);
    crate::export::scene_collection::plan_workspace_scene_export_ids(
        &hierarchies.scenes()[0],
        SceneUnityVersion {
            major: 2019,
            minor: 4,
            patch: 0,
        },
        crate::processing::scene_hierarchy::WorkspaceSceneHierarchyMembershipLimits::default(),
        crate::export::scene_collection::SceneHierarchyAssetOrderLimits::default(),
        crate::export::scene_collection::SceneExportIdPlanLimits::default(),
    )
    .unwrap()
}

pub(super) fn scene_hierarchies_for_workspace(
    files: &[(&str, &[u8])],
    reference_graph: &MultiFileReferenceGraph<'_>,
) -> crate::processing::scene_hierarchy::WorkspaceSceneHierarchies {
    let (first_file_name, _) = files.first().unwrap();
    let plan = crate::processing::scene_collections::plan_scene_definition(
        0,
        first_file_name,
        &crate::processing::scene_collections::SceneCollectionPathStatus::BuildSettingsMissing,
        None,
        None,
        crate::processing::scene_collections::SceneDefinitionPlanLimits::default(),
    )
    .unwrap();
    let definitions = crate::processing::scene_collections::WorkspaceSceneDefinitions::from_plans(
        vec![plan],
        files.len(),
        crate::processing::scene_collections::WorkspaceSceneDefinitionLimits::default(),
    )
    .unwrap();
    let mut definitions = definitions;
    for file_index in 1..files.len() {
        definitions.add_collection(0, file_index).unwrap();
    }
    let mut hierarchy_builder =
        crate::processing::scene_hierarchy::WorkspaceSceneHierarchyBuilder::new(
            files.len(),
            crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits::default(),
        )
        .unwrap();
    for (file_index, &(file_name, bytes)) in files.iter().enumerate() {
        let serialized_file = SerializedFile::parse(bytes).unwrap();
        let mut scene_graph = crate::processing::scene::SceneGraph::build(
            &serialized_file,
            crate::processing::scene::SceneGraphLimits::default(),
        )
        .unwrap();
        scene_graph
            .resolve_with_multi_file_graph(file_index, file_name, reference_graph)
            .unwrap();
        let roots = scene_graph
            .discover_workspace_roots(
                file_index,
                file_name,
                reference_graph,
                crate::processing::scene::SceneRootDiscoveryLimits::default(),
            )
            .unwrap();
        let audit = scene_graph
            .audit_workspace_transform_links(
                file_index,
                file_name,
                reference_graph,
                crate::processing::scene::SceneHierarchyAuditLimits::default(),
            )
            .unwrap();
        hierarchy_builder
            .add_collection_graph(file_index, &scene_graph, roots, audit, reference_graph)
            .unwrap();
    }
    hierarchy_builder.finish(&definitions).unwrap()
}

pub(super) fn non_scene_hierarchies_for_workspace(
    file_name: &str,
    bytes: &[u8],
    reference_graph: &MultiFileReferenceGraph<'_>,
) -> crate::processing::scene_hierarchy::WorkspaceSceneHierarchies {
    let definitions = crate::processing::scene_collections::WorkspaceSceneDefinitions::from_plans(
        Vec::new(),
        1,
        crate::processing::scene_collections::WorkspaceSceneDefinitionLimits::default(),
    )
    .unwrap();
    let mut hierarchy_builder =
        crate::processing::scene_hierarchy::WorkspaceSceneHierarchyBuilder::new(
            1,
            crate::processing::scene_hierarchy::WorkspaceSceneHierarchyLimits::default(),
        )
        .unwrap();
    let serialized_file = SerializedFile::parse(bytes).unwrap();
    let mut scene_graph = crate::processing::scene::SceneGraph::build(
        &serialized_file,
        crate::processing::scene::SceneGraphLimits::default(),
    )
    .unwrap();
    scene_graph
        .resolve_with_multi_file_graph(0, file_name, reference_graph)
        .unwrap();
    let roots = scene_graph
        .discover_workspace_roots(
            0,
            file_name,
            reference_graph,
            crate::processing::scene::SceneRootDiscoveryLimits::default(),
        )
        .unwrap();
    let audit = scene_graph
        .audit_workspace_transform_links(
            0,
            file_name,
            reference_graph,
            crate::processing::scene::SceneHierarchyAuditLimits::default(),
        )
        .unwrap();
    hierarchy_builder
        .add_collection_graph(0, &scene_graph, roots, audit, reference_graph)
        .unwrap();
    hierarchy_builder.finish(&definitions).unwrap()
}

pub(super) fn make_game_object_and_transform_file(component_path_id: i64) -> Vec<u8> {
    let game_object_tree = [
        (0, 6, "GameObject", "", -1),
        (1, 1, "vector", "m_Component", -1),
        (2, 1, "Array", "Array", -1),
        (3, 1, "int", "size", 4),
        (3, 1, "ComponentPair", "data", -1),
        (4, 1, "PPtr<Transform>", "component", 12),
        (5, 1, "SInt32", "m_FileID", 4),
        (5, 1, "SInt64", "m_PathID", 8),
    ];
    let transform_tree = [
        (0, 2, "Transform", "", -1),
        (1, 1, "PPtr<GameObject>", "m_GameObject", 12),
        (2, 1, "SInt32", "m_FileID", 4),
        (2, 1, "SInt64", "m_PathID", 8),
    ];
    let mut metadata = b"2019.4.0f1\0".to_vec();
    metadata.extend_from_slice(&0u32.to_le_bytes());
    metadata.extend_from_slice(&2i32.to_le_bytes());
    write_type_tree(&mut metadata, 1, &game_object_tree);
    write_type_tree(&mut metadata, 4, &transform_tree);

    metadata.extend_from_slice(&0u32.to_le_bytes()); // generation-10 long FileID flag
    metadata.extend_from_slice(&2i32.to_le_bytes());
    let mut game_object_payload = Vec::new();
    game_object_payload.extend_from_slice(&1i32.to_le_bytes()); // m_Component size
    game_object_payload.extend_from_slice(&0i32.to_le_bytes()); // component FileID
    game_object_payload.extend_from_slice(&component_path_id.to_le_bytes());
    let mut transform_payload = Vec::new();
    transform_payload.extend_from_slice(&0i32.to_le_bytes()); // m_GameObject FileID
    transform_payload.extend_from_slice(&GAME_OBJECT_PATH_ID.to_le_bytes());

    write_object_record(
        &mut metadata,
        GAME_OBJECT_PATH_ID,
        0,
        game_object_payload.len(),
        1,
        1,
    );
    write_object_record(
        &mut metadata,
        TRANSFORM_PATH_ID,
        game_object_payload.len(),
        transform_payload.len(),
        4,
        4,
    );
    metadata.extend_from_slice(&0i32.to_le_bytes()); // external files
    metadata.push(0); // user information string

    let data_offset = 20 + metadata.len();
    let file_size = data_offset + game_object_payload.len() + transform_payload.len();
    let mut file = Vec::with_capacity(file_size);
    file.extend_from_slice(&(metadata.len() as u32).to_be_bytes());
    file.extend_from_slice(&(file_size as u32).to_be_bytes());
    file.extend_from_slice(&10i32.to_be_bytes());
    file.extend_from_slice(&(data_offset as u32).to_be_bytes());
    file.push(0);
    file.extend_from_slice(&[0; 3]);
    file.extend_from_slice(&metadata);
    file.extend_from_slice(&game_object_payload);
    file.extend_from_slice(&transform_payload);
    file
}

pub(crate) fn make_game_object_without_transform_file() -> Vec<u8> {
    let game_object_tree = [
        (0, 6, "GameObject", "", -1),
        (1, 1, "string", "m_Name", -1),
        (2, 1, "Array", "Array", -1),
        (3, 1, "int", "size", 4),
        (3, 1, "char", "data", 1),
        (1, 1, "bool", "m_IsActive", 1),
        (1, 1, "vector", "m_Component", -1),
        (2, 1, "Array", "Array", -1),
        (3, 1, "int", "size", 4),
        (3, 1, "ComponentPair", "data", -1),
        (4, 1, "PPtr<Transform>", "component", 12),
        (5, 1, "SInt32", "m_FileID", 4),
        (5, 1, "SInt64", "m_PathID", 8),
    ];
    let transform_tree = [
        (0, 1, "Transform", "", -1),
        (1, 1, "Component", "Base", -1),
        (2, 1, "PPtr<GameObject>", "m_GameObject", 12),
        (3, 1, "SInt32", "m_FileID", 4),
        (3, 1, "SInt64", "m_PathID", 8),
        (1, 1, "PPtr<Transform>", "m_Father", 12),
        (2, 1, "SInt32", "m_FileID", 4),
        (2, 1, "SInt64", "m_PathID", 8),
        (1, 1, "vector", "m_Children", -1),
        (2, 1, "Array", "Array", -1),
        (3, 1, "int", "size", 4),
        (3, 1, "PPtr<Transform>", "data", 12),
        (4, 1, "SInt32", "m_FileID", 4),
        (4, 1, "SInt64", "m_PathID", 8),
        (1, 1, "int", "m_RootOrder", 4),
        (1, 1, "Vector3f", "m_LocalEulerAnglesHint", -1),
        (2, 1, "float", "x", 4),
        (2, 1, "float", "y", 4),
        (2, 1, "float", "z", 4),
        (1, 1, "Vector3f", "m_LocalPosition", -1),
        (2, 1, "float", "x", 4),
        (2, 1, "float", "y", 4),
        (2, 1, "float", "z", 4),
        (1, 1, "Quaternionf", "m_LocalRotation", -1),
        (2, 1, "float", "x", 4),
        (2, 1, "float", "y", 4),
        (2, 1, "float", "z", 4),
        (2, 1, "float", "w", 4),
        (1, 1, "Vector3f", "m_LocalScale", -1),
        (2, 1, "float", "x", 4),
        (2, 1, "float", "y", 4),
        (2, 1, "float", "z", 4),
    ];
    let mut metadata = b"2019.4.0f1\0".to_vec();
    metadata.extend_from_slice(&0u32.to_le_bytes());
    metadata.extend_from_slice(&2i32.to_le_bytes());
    write_type_tree(&mut metadata, 1, &game_object_tree);
    write_type_tree(&mut metadata, 4, &transform_tree);
    metadata.extend_from_slice(&0u32.to_le_bytes()); // generation-10 long FileID flag
    metadata.extend_from_slice(&1i32.to_le_bytes());
    let mut payload = Vec::new();
    payload.extend_from_slice(&4i32.to_le_bytes());
    payload.extend_from_slice(b"Root");
    payload.push(1); // active GameObject
    payload.extend_from_slice(&0i32.to_le_bytes()); // empty m_Component array
    write_object_record(&mut metadata, GAME_OBJECT_PATH_ID, 0, payload.len(), 1, 1);
    metadata.extend_from_slice(&0i32.to_le_bytes()); // external files
    metadata.push(0); // user information string

    let data_offset = 20 + metadata.len();
    let file_size = data_offset + payload.len();
    let mut file = Vec::with_capacity(file_size);
    file.extend_from_slice(&(metadata.len() as u32).to_be_bytes());
    file.extend_from_slice(&(file_size as u32).to_be_bytes());
    file.extend_from_slice(&10i32.to_be_bytes());
    file.extend_from_slice(&(data_offset as u32).to_be_bytes());
    file.push(0);
    file.extend_from_slice(&[0; 3]);
    file.extend_from_slice(&metadata);
    file.extend_from_slice(&payload);
    file
}

pub(super) fn generated_transform_metadata() -> SerializedFileMetadata {
    let node = |level, name: &str, type_name: &str, byte_size| TypeTreeNode {
        level,
        version: 1,
        type_flags: 0,
        type_string_offset: None,
        name_string_offset: None,
        type_name: Some(type_name.to_owned()),
        name: Some(name.to_owned()),
        byte_size,
        index: 0,
        meta_flags: 0,
        reference_type_hash: None,
    };
    let nodes = vec![
        node(0, "", "Transform", -1),
        node(1, "Base", "Component", -1),
        node(2, "m_GameObject", "PPtr<GameObject>", 12),
        node(3, "m_FileID", "SInt32", 4),
        node(3, "m_PathID", "SInt64", 8),
        node(1, "m_Father", "PPtr<Transform>", 12),
        node(2, "m_FileID", "SInt32", 4),
        node(2, "m_PathID", "SInt64", 8),
        node(1, "m_Children", "vector", -1),
        node(2, "Array", "Array", -1),
        node(3, "size", "int", 4),
        node(3, "data", "PPtr<Transform>", 12),
        node(4, "m_FileID", "SInt32", 4),
        node(4, "m_PathID", "SInt64", 8),
        node(1, "m_RootOrder", "int", 4),
        node(1, "m_LocalEulerAnglesHint", "Vector3f", -1),
        node(2, "x", "float", 4),
        node(2, "y", "float", 4),
        node(2, "z", "float", 4),
        node(1, "m_LocalPosition", "Vector3f", -1),
        node(2, "x", "float", 4),
        node(2, "y", "float", 4),
        node(2, "z", "float", 4),
        node(1, "m_LocalRotation", "Quaternionf", -1),
        node(2, "x", "float", 4),
        node(2, "y", "float", 4),
        node(2, "z", "float", 4),
        node(2, "w", "float", 4),
        node(1, "m_LocalScale", "Vector3f", -1),
        node(2, "x", "float", 4),
        node(2, "y", "float", 4),
        node(2, "z", "float", 4),
    ];
    SerializedFileMetadata {
        prefix: SerializedFileMetadataPrefix {
            metadata_offset: 0,
            metadata_size: 0,
            body_endian: crate::io::binary::Endian::Little,
            unity_version_text: Some("2019.4.31f1".to_owned()),
            target_platform: Some(0),
            has_type_tree: true,
            consumed_bytes: 0,
        },
        types: vec![SerializedType {
            raw_type_id: 4,
            type_id_for_hash: 4,
            is_stripped_type: false,
            script_type_index: -1,
            script_id: None,
            old_type_hash: None,
            type_tree: Some(TypeTree {
                nodes,
                string_buffer: Vec::new(),
            }),
            type_dependencies: Vec::new(),
        }],
        long_file_id: None,
        objects: Vec::new(),
        script_types: Vec::new(),
        externals: Vec::new(),
        reference_types: Vec::new(),
        user_information: None,
        trailing_metadata: Vec::new(),
        consumed_bytes: 0,
    }
}

pub(super) fn make_mesh_renderer_component_file() -> Vec<u8> {
    const COMPONENT_CLASS_ID: i32 = 23;
    const COMPONENT_PATH_ID: i64 = 9001;

    let component_tree = [
        (0, 1, "MeshRenderer", "", -1),
        (1, 1, "Renderer", "Base", -1),
        (2, 1, "Component", "Base", -1),
        (3, 1, "PPtr<GameObject>", "m_GameObject", 8),
        (4, 1, "SInt32", "m_FileID", 4),
        (4, 1, "SInt32", "m_PathID", 4),
    ];
    let payload = [0_u8; 8];
    let mut metadata = b"2019.4.0f1\0".to_vec();
    metadata.extend_from_slice(&0_u32.to_le_bytes());
    metadata.extend_from_slice(&1_i32.to_le_bytes());
    write_type_tree(&mut metadata, COMPONENT_CLASS_ID, &component_tree);
    metadata.extend_from_slice(&0_u32.to_le_bytes()); // generation-10 long FileID flag
    metadata.extend_from_slice(&1_i32.to_le_bytes());
    write_object_record(
        &mut metadata,
        COMPONENT_PATH_ID,
        0,
        payload.len(),
        COMPONENT_CLASS_ID,
        COMPONENT_CLASS_ID as i16,
    );
    metadata.extend_from_slice(&0_i32.to_le_bytes()); // external files
    metadata.push(0); // user information string

    let data_offset = 20 + metadata.len();
    let file_size = data_offset + payload.len();
    let mut file = Vec::with_capacity(file_size);
    file.extend_from_slice(&(metadata.len() as u32).to_be_bytes());
    file.extend_from_slice(&(file_size as u32).to_be_bytes());
    file.extend_from_slice(&10_i32.to_be_bytes());
    file.extend_from_slice(&(data_offset as u32).to_be_bytes());
    file.push(0);
    file.extend_from_slice(&[0; 3]);
    file.extend_from_slice(&metadata);
    file.extend_from_slice(&payload);
    file
}

pub(super) fn make_game_object_mesh_renderer_file() -> Vec<u8> {
    const GAME_OBJECT_CLASS_ID: i32 = 1;
    const MESH_RENDERER_CLASS_ID: i32 = 23;

    let game_object_tree = [
        (0, 1, "GameObject", "", -1),
        (1, 1, "string", "m_Name", -1),
        (2, 1, "Array", "Array", -1),
        (3, 1, "int", "size", 4),
        (3, 1, "char", "data", 1),
        (1, 1, "bool", "m_IsActive", 1),
        (1, 1, "vector", "m_Component", -1),
        (2, 1, "Array", "Array", -1),
        (3, 1, "int", "size", 4),
        (3, 1, "ComponentPair", "data", -1),
        (4, 1, "PPtr<Component>", "component", 8),
        (5, 1, "SInt32", "m_FileID", 4),
        (5, 1, "SInt32", "m_PathID", 4),
    ];
    let mesh_renderer_tree = [
        (0, 1, "MeshRenderer", "", -1),
        (1, 1, "Renderer", "Base", -1),
        (2, 1, "Component", "Base", -1),
        (3, 1, "PPtr<GameObject>", "m_GameObject", 8),
        (4, 1, "SInt32", "m_FileID", 4),
        (4, 1, "SInt32", "m_PathID", 4),
    ];

    let mut metadata = b"2019.4.0f1\0".to_vec();
    metadata.extend_from_slice(&0_u32.to_le_bytes());
    metadata.extend_from_slice(&2_i32.to_le_bytes());
    write_type_tree(&mut metadata, GAME_OBJECT_CLASS_ID, &game_object_tree);
    write_type_tree(&mut metadata, MESH_RENDERER_CLASS_ID, &mesh_renderer_tree);

    let mut game_object_payload = Vec::new();
    game_object_payload.extend_from_slice(&4_i32.to_le_bytes());
    game_object_payload.extend_from_slice(b"Root");
    game_object_payload.push(1);
    game_object_payload.extend_from_slice(&1_i32.to_le_bytes());
    game_object_payload.extend_from_slice(&0_i32.to_le_bytes());
    game_object_payload.extend_from_slice(&9001_i32.to_le_bytes());
    let mut mesh_renderer_payload = Vec::new();
    mesh_renderer_payload.extend_from_slice(&0_i32.to_le_bytes());
    mesh_renderer_payload.extend_from_slice(&42_i32.to_le_bytes());

    metadata.extend_from_slice(&0_u32.to_le_bytes()); // generation-10 long FileID flag
    metadata.extend_from_slice(&2_i32.to_le_bytes());
    write_object_record(
        &mut metadata,
        42,
        0,
        game_object_payload.len(),
        GAME_OBJECT_CLASS_ID,
        GAME_OBJECT_CLASS_ID as i16,
    );
    write_object_record(
        &mut metadata,
        9001,
        game_object_payload.len(),
        mesh_renderer_payload.len(),
        MESH_RENDERER_CLASS_ID,
        MESH_RENDERER_CLASS_ID as i16,
    );
    metadata.extend_from_slice(&0_i32.to_le_bytes()); // external files
    metadata.push(0); // user information string

    let data_offset = 20 + metadata.len();
    let file_size = data_offset + game_object_payload.len() + mesh_renderer_payload.len();
    let mut file = Vec::with_capacity(file_size);
    file.extend_from_slice(&(metadata.len() as u32).to_be_bytes());
    file.extend_from_slice(&(file_size as u32).to_be_bytes());
    file.extend_from_slice(&10_i32.to_be_bytes());
    file.extend_from_slice(&(data_offset as u32).to_be_bytes());
    file.push(0);
    file.extend_from_slice(&[0; 3]);
    file.extend_from_slice(&metadata);
    file.extend_from_slice(&game_object_payload);
    file.extend_from_slice(&mesh_renderer_payload);
    file
}

pub(super) fn make_generation_10_object_file_with_external(
    class_id: i32,
    path_id: i64,
    type_tree: &[(u8, i32, &str, &str, i32)],
    payload: &[u8],
    external_file_name: &str,
) -> Vec<u8> {
    let mut metadata = b"2019.4.0f1\0".to_vec();
    metadata.extend_from_slice(&0_u32.to_le_bytes());
    metadata.extend_from_slice(&1_i32.to_le_bytes());
    write_type_tree(&mut metadata, class_id, type_tree);
    metadata.extend_from_slice(&0_u32.to_le_bytes()); // generation-10 long FileID flag
    metadata.extend_from_slice(&1_i32.to_le_bytes());
    write_object_record(
        &mut metadata,
        path_id,
        0,
        payload.len(),
        class_id,
        class_id as i16,
    );
    metadata.extend_from_slice(&1_i32.to_le_bytes()); // external files
    metadata.push(0); // empty asset path
    metadata.extend_from_slice(&[0; 16]); // GUID
    metadata.extend_from_slice(&0_i32.to_le_bytes()); // asset type
    metadata.extend_from_slice(external_file_name.as_bytes());
    metadata.push(0);
    metadata.push(0); // user information string

    let data_offset = 20 + metadata.len();
    let file_size = data_offset + payload.len();
    let mut file = Vec::with_capacity(file_size);
    file.extend_from_slice(&(metadata.len() as u32).to_be_bytes());
    file.extend_from_slice(&(file_size as u32).to_be_bytes());
    file.extend_from_slice(&10_i32.to_be_bytes());
    file.extend_from_slice(&(data_offset as u32).to_be_bytes());
    file.push(0);
    file.extend_from_slice(&[0; 3]);
    file.extend_from_slice(&metadata);
    file.extend_from_slice(payload);
    file
}

pub(super) fn make_game_object_with_external_component() -> Vec<u8> {
    let tree = [
        (0, 1, "GameObject", "", -1),
        (1, 1, "string", "m_Name", -1),
        (2, 1, "Array", "Array", -1),
        (3, 1, "int", "size", 4),
        (3, 1, "char", "data", 1),
        (1, 1, "bool", "m_IsActive", 1),
        (1, 1, "vector", "m_Component", -1),
        (2, 1, "Array", "Array", -1),
        (3, 1, "int", "size", 4),
        (3, 1, "ComponentPair", "data", -1),
        (4, 1, "PPtr<Component>", "component", 8),
        (5, 1, "SInt32", "m_FileID", 4),
        (5, 1, "SInt32", "m_PathID", 4),
    ];
    let mut payload = Vec::new();
    payload.extend_from_slice(&4_i32.to_le_bytes());
    payload.extend_from_slice(b"Root");
    payload.push(1);
    payload.extend_from_slice(&1_i32.to_le_bytes());
    payload.extend_from_slice(&1_i32.to_le_bytes());
    payload.extend_from_slice(&9001_i32.to_le_bytes());
    make_generation_10_object_file_with_external(1, 42, &tree, &payload, "shared.assets")
}

pub(super) fn make_game_object_with_external_pptr_dictionary() -> Vec<u8> {
    let tree = [
        (0, 1, "GameObject", "", -1),
        (1, 1, "map", "m_Targets", -1),
        (2, 1, "Array", "Array", -1),
        (3, 1, "SInt32", "size", 4),
        (3, 1, "pair", "data", -1),
        (4, 1, "string", "first", -1),
        (5, 1, "Array", "Array", -1),
        (6, 1, "SInt32", "size", 4),
        (6, 1, "char", "data", 2),
        (4, 1, "PPtr<GameObject>", "second", 12),
        (5, 1, "SInt32", "m_FileID", 4),
        (5, 1, "SInt64", "m_PathID", 8),
    ];
    let mut payload = Vec::new();
    payload.extend_from_slice(&1_i32.to_le_bytes()); // one dictionary entry
    payload.extend_from_slice(&6_i32.to_le_bytes()); // string length
    payload.extend_from_slice(b"target");
    payload.extend_from_slice(&[0; 2]); // align string data to four bytes
    payload.extend_from_slice(&1_i32.to_le_bytes()); // first external file
    payload.extend_from_slice(&GAME_OBJECT_PATH_ID.to_le_bytes());
    make_generation_10_object_file_with_external(1, 51, &tree, &payload, "target.assets")
}

pub(super) fn make_mesh_renderer_with_external_game_object() -> Vec<u8> {
    let tree = [
        (0, 1, "MeshRenderer", "", -1),
        (1, 1, "Renderer", "Base", -1),
        (2, 1, "Component", "Base", -1),
        (3, 1, "PPtr<GameObject>", "m_GameObject", 8),
        (4, 1, "SInt32", "m_FileID", 4),
        (4, 1, "SInt32", "m_PathID", 4),
    ];
    let mut payload = Vec::new();
    payload.extend_from_slice(&1_i32.to_le_bytes());
    payload.extend_from_slice(&42_i32.to_le_bytes());
    make_generation_10_object_file_with_external(23, 9001, &tree, &payload, "main.assets")
}

pub(super) fn write_type_tree(
    output: &mut Vec<u8>,
    class_id: i32,
    nodes: &[(u8, i32, &str, &str, i32)],
) {
    let nodes_with_meta_flags: Vec<_> = nodes
        .iter()
        .map(|(level, version, type_name, name, byte_size)| {
            (*level, *version, *type_name, *name, *byte_size, 0)
        })
        .collect();
    write_type_tree_with_meta_flags(output, class_id, &nodes_with_meta_flags);
}

pub(super) fn write_type_tree_with_meta_flags(
    output: &mut Vec<u8>,
    class_id: i32,
    nodes: &[(u8, i32, &str, &str, i32, u32)],
) {
    let mut string_buffer = Vec::new();
    let mut offsets = Vec::new();
    for (_, _, type_name, name, _, _) in nodes {
        let type_offset = push_type_tree_string(&mut string_buffer, type_name);
        let name_offset = push_type_tree_string(&mut string_buffer, name);
        offsets.push((type_offset, name_offset));
    }
    output.extend_from_slice(&class_id.to_le_bytes());
    output.extend_from_slice(&(nodes.len() as i32).to_le_bytes());
    output.extend_from_slice(&(string_buffer.len() as i32).to_le_bytes());
    for ((level, version, _, _, byte_size, meta_flags), (type_offset, name_offset)) in
        nodes.iter().zip(offsets)
    {
        output.extend_from_slice(&(*version as u16).to_le_bytes());
        output.push(*level);
        output.push(0);
        output.extend_from_slice(&type_offset.to_le_bytes());
        output.extend_from_slice(&name_offset.to_le_bytes());
        output.extend_from_slice(&byte_size.to_le_bytes());
        output.extend_from_slice(&0i32.to_le_bytes());
        output.extend_from_slice(&meta_flags.to_le_bytes());
    }
    output.extend_from_slice(&string_buffer);
}

pub(super) fn push_type_tree_string(buffer: &mut Vec<u8>, value: &str) -> u32 {
    let offset = u32::try_from(buffer.len()).unwrap();
    buffer.extend_from_slice(value.as_bytes());
    buffer.push(0);
    offset
}

pub(super) fn write_object_record(
    output: &mut Vec<u8>,
    path_id: i64,
    data_start: usize,
    data_size: usize,
    raw_type_id: i32,
    class_id: i16,
) {
    output.extend_from_slice(&i32::try_from(path_id).unwrap().to_le_bytes());
    output.extend_from_slice(&u32::try_from(data_start).unwrap().to_le_bytes());
    output.extend_from_slice(&i32::try_from(data_size).unwrap().to_le_bytes());
    output.extend_from_slice(&raw_type_id.to_le_bytes());
    output.extend_from_slice(&class_id.to_le_bytes());
    output.extend_from_slice(&0u16.to_le_bytes());
}

pub(super) fn assert_yaml_asset_order_and_streamed_bytes(yaml: &str) {
    let second_start = yaml.find("--- !u!189 &202").unwrap();
    let first_start = yaml.find("--- !u!189 &101").unwrap();
    assert!(second_start < first_start);
    assert_eq!(yaml.matches("--- !u!189").count(), 2);
    assert!(yaml[second_start..first_start].contains("m_ImageData_C189: 3"));
    assert!(yaml[second_start..first_start].contains("_typelessdata: 58595a"));
    assert!(yaml[first_start..].contains("m_ImageData_C189: 4"));
    assert!(yaml[first_start..].contains("_typelessdata: 4f524947"));
    assert!(!yaml.contains("first.resS"));
    assert!(!yaml.contains("second.resS"));
}
