    use super::*;
    use crate::io::dependency::DependencyFilePath;
    use crate::processing::asset_factory::FileTypeTreeTestLayouts;
    use crate::processing::scene_collections::{
        SceneDefinitionGuidPlan, SceneDefinitionPathSource, SceneGuidGenerationReason,
    };
    use crate::serialized_file::object_workspace::ObjectWorkspaceLimits;
    use crate::serialized_file::reference_graph::MultiFileReferenceGraphLimits;
    use crate::serialized_file::workspace::SerializedFileWorkspaceLimits;

    const GAME_OBJECT_PATH_ID: i64 = 100;
    const TRANSFORM_PATH_ID: i64 = 200;
    const ORPHAN_GAME_OBJECT_PATH_ID: i64 = 101;
    const OCCLUSION_PATH_ID: i64 = 300;
    const SECOND_GAME_OBJECT_PATH_ID: i64 = 110;
    const SECOND_TRANSFORM_PATH_ID: i64 = 210;
    const SECOND_OCCLUSION_PATH_ID: i64 = 301;
    const BUILD_SETTINGS_PATH_ID: i64 = 400;
    const ASSET_BUNDLE_PATH_ID: i64 = 500;

    /// The frozen 2019.4.40f1 class-4 TPK layout (release tree): no `m_RootOrder` and no
    /// `m_LocalEulerAnglesHint`.
    type Tree = &'static [(u8, i32, &'static str, &'static str, i32, u32)];

    const GAME_OBJECT_TREE: Tree = &[
        (0, 6, "GameObject", "Base", -1, 32768),
        (1, 1, "vector", "m_Component", -1, 268468289),
        (2, 1, "Array", "Array", -1, 268451905),
        (3, 1, "int", "size", 4, 268435521),
        (3, 1, "ComponentPair", "data", -1, 268435521),
        (4, 1, "PPtr<Component>", "component", 12, 268435521),
        (5, 1, "int", "m_FileID", 4, 268435521),
        (5, 1, "SInt64", "m_PathID", 8, 268435521),
        (1, 1, "unsigned int", "m_Layer", 4, 0),
        (1, 1, "string", "m_Name", -1, 32768),
        (2, 1, "Array", "Array", -1, 16385),
        (3, 1, "int", "size", 4, 1),
        (3, 1, "char", "data", 1, 1),
        (1, 1, "UInt16", "m_Tag", 2, 0),
        (1, 1, "bool", "m_IsActive", 1, 0),
    ];

    const TRANSFORM_TREE: Tree = &[
        (0, 1, "Transform", "Base", -1, 32768),
        (1, 1, "PPtr<GameObject>", "m_GameObject", 12, 65),
        (2, 1, "int", "m_FileID", 4, 65),
        (2, 1, "SInt64", "m_PathID", 8, 65),
        (1, 1, "Quaternionf", "m_LocalRotation", -1, 2097152),
        (2, 1, "float", "x", 4, 2097152),
        (2, 1, "float", "y", 4, 2097152),
        (2, 1, "float", "z", 4, 2097152),
        (2, 1, "float", "w", 4, 2097152),
        (1, 1, "Vector3f", "m_LocalPosition", -1, 2097152),
        (2, 1, "float", "x", 4, 2097152),
        (2, 1, "float", "y", 4, 2097152),
        (2, 1, "float", "z", 4, 2097152),
        (1, 1, "Vector3f", "m_LocalScale", -1, 2097152),
        (2, 1, "float", "x", 4, 2097152),
        (2, 1, "float", "y", 4, 2097152),
        (2, 1, "float", "z", 4, 2097152),
        (1, 1, "vector", "m_Children", -1, 32833),
        (2, 1, "Array", "Array", -1, 16449),
        (3, 1, "int", "size", 4, 65),
        (3, 1, "PPtr<Transform>", "data", 12, 65),
        (4, 1, "int", "m_FileID", 4, 65),
        (4, 1, "SInt64", "m_PathID", 8, 65),
        (1, 1, "PPtr<Transform>", "m_Father", 12, 1),
        (2, 1, "int", "m_FileID", 4, 1),
        (2, 1, "SInt64", "m_PathID", 8, 1),
    ];

    const OCCLUSION_TREE: Tree = &[
        (0, 2, "OcclusionCullingSettings", "Base", -1, 32768),
        (1, 1, "GUID", "m_SceneGUID", -1, 0),
        (2, 1, "unsigned int", "data[0]", 4, 0),
        (2, 1, "unsigned int", "data[1]", 4, 0),
        (2, 1, "unsigned int", "data[2]", 4, 0),
        (2, 1, "unsigned int", "data[3]", 4, 0),
        (
            1,
            1,
            "PPtr<OcclusionCullingData>",
            "m_OcclusionCullingData",
            12,
            0,
        ),
        (2, 1, "int", "m_FileID", 4, 8388609),
        (2, 1, "SInt64", "m_PathID", 8, 8388609),
        (1, 1, "vector", "m_StaticRenderers", -1, 32768),
        (2, 1, "Array", "Array", -1, 16384),
        (3, 1, "int", "size", 4, 0),
        (3, 1, "PPtr<Renderer>", "data", 12, 0),
        (4, 1, "int", "m_FileID", 4, 8388609),
        (4, 1, "SInt64", "m_PathID", 8, 8388609),
        (1, 1, "vector", "m_Portals", -1, 32768),
        (2, 1, "Array", "Array", -1, 16384),
        (3, 1, "int", "size", 4, 0),
        (3, 1, "PPtr<OcclusionPortal>", "data", 12, 0),
        (4, 1, "int", "m_FileID", 4, 8388609),
        (4, 1, "SInt64", "m_PathID", 8, 8388609),
    ];

    const BUILD_SETTINGS_TREE: Tree = &[
        (0, 1, "BuildSettings", "", -1, 0),
        (1, 1, "vector", "m_Scenes", -1, 0),
        (2, 1, "Array", "Array", -1, 0),
        (3, 1, "int", "size", 4, 0),
        (3, 1, "string", "data", -1, 0),
        (4, 1, "Array", "Array", -1, 0),
        (5, 1, "int", "size", 4, 0),
        (5, 1, "char", "data", 1, 0),
    ];

    const ASSET_BUNDLE_TREE: Tree = &[
        (0, 1, "AssetBundle", "", -1, 0),
        (1, 1, "map", "m_Container", -1, 0),
        (2, 1, "Array", "Array", -1, 0),
        (3, 1, "int", "size", 4, 0),
        (3, 1, "pair", "data", -1, 0),
        (4, 1, "string", "first", -1, 0),
        (5, 1, "Array", "Array", -1, 0),
        (6, 1, "int", "size", 4, 0),
        (6, 1, "char", "data", 1, 0),
        (4, 1, "AssetInfo", "second", -1, 0),
        (5, 1, "int", "preloadIndex", 4, 0),
        (5, 1, "int", "preloadSize", 4, 0),
        (5, 1, "PPtr<Object>", "asset", 12, 0),
        (6, 1, "int", "m_FileID", 4, 0),
        (6, 1, "SInt64", "m_PathID", 8, 0),
        (1, 1, "bool", "m_IsStreamedSceneAssetBundle", 1, 0),
        (1, 1, "map", "m_SceneHashes", -1, 0),
        (2, 1, "Array", "Array", -1, 0),
        (3, 1, "int", "size", 4, 0),
        (3, 1, "pair", "data", -1, 0),
        (4, 1, "string", "first", -1, 0),
        (5, 1, "Array", "Array", -1, 0),
        (6, 1, "int", "size", 4, 0),
        (6, 1, "char", "data", 1, 0),
        (4, 1, "string", "second", -1, 0),
        (5, 1, "Array", "Array", -1, 0),
        (6, 1, "int", "size", 4, 0),
        (6, 1, "char", "data", 1, 0),
    ];

    /// Release TypeTree nodes copied from AssetRipper's TPK for class 142 at Unity 2019.4.40f1.
    /// Unlike `ASSET_BUNDLE_TREE`, this includes the complete serialized object layout and its
    /// alignment flags so the frozen source factory can materialize the streamed-scene fixture.
    const ASSET_BUNDLE_FULL_TREE: Tree = &[
        (0, 1, "AssetBundle", "Base", -1, 32768),
        (1, 1, "string", "m_Name", -1, 557057),
        (2, 1, "Array", "Array", -1, 540673),
        (3, 1, "int", "size", 4, 524289),
        (3, 1, "char", "data", 1, 524289),
        (1, 1, "vector", "m_PreloadTable", -1, 32768),
        (2, 1, "Array", "Array", -1, 16384),
        (3, 1, "int", "size", 4, 0),
        (3, 1, "PPtr<Object>", "data", -1, 0),
        (4, 1, "int", "m_FileID", 4, 8388609),
        (4, 1, "SInt64", "m_PathID", 8, 8388609),
        (1, 1, "map", "m_Container", -1, 32768),
        (2, 1, "Array", "Array", -1, 32768),
        (3, 1, "int", "size", 4, 0),
        (3, 1, "pair", "data", -1, 32768),
        (4, 1, "string", "first", -1, 32768),
        (5, 1, "Array", "Array", -1, 16385),
        (6, 1, "int", "size", 4, 1),
        (6, 1, "char", "data", 1, 1),
        (4, 1, "AssetInfo", "second", -1, 0),
        (5, 1, "int", "preloadIndex", 4, 0),
        (5, 1, "int", "preloadSize", 4, 0),
        (5, 1, "PPtr<Object>", "asset", -1, 0),
        (6, 1, "int", "m_FileID", 4, 8388609),
        (6, 1, "SInt64", "m_PathID", 8, 8388609),
        (1, 1, "AssetInfo", "m_MainAsset", -1, 0),
        (2, 1, "int", "preloadIndex", 4, 0),
        (2, 1, "int", "preloadSize", 4, 0),
        (2, 1, "PPtr<Object>", "asset", -1, 0),
        (3, 1, "int", "m_FileID", 4, 8388609),
        (3, 1, "SInt64", "m_PathID", 8, 8388609),
        (1, 1, "unsigned int", "m_RuntimeCompatibility", 4, 0),
        (1, 1, "string", "m_AssetBundleName", -1, 32768),
        (2, 1, "Array", "Array", -1, 16385),
        (3, 1, "int", "size", 4, 1),
        (3, 1, "char", "data", 1, 1),
        (1, 1, "vector", "m_Dependencies", -1, 32768),
        (2, 1, "Array", "Array", -1, 49152),
        (3, 1, "int", "size", 4, 0),
        (3, 1, "string", "data", -1, 32768),
        (4, 1, "Array", "Array", -1, 16385),
        (5, 1, "int", "size", 4, 1),
        (5, 1, "char", "data", 1, 1),
        (1, 1, "bool", "m_IsStreamedSceneAssetBundle", 1, 16384),
        (1, 1, "int", "m_ExplicitDataLayout", 4, 0),
        (1, 1, "int", "m_PathFlags", 4, 0),
        (1, 1, "map", "m_SceneHashes", -1, 32768),
        (2, 1, "Array", "Array", -1, 32768),
        (3, 1, "int", "size", 4, 0),
        (3, 1, "pair", "data", -1, 32768),
        (4, 1, "string", "first", -1, 32768),
        (5, 1, "Array", "Array", -1, 16385),
        (6, 1, "int", "size", 4, 1),
        (6, 1, "char", "data", 1, 1),
        (4, 1, "string", "second", -1, 32768),
        (5, 1, "Array", "Array", -1, 16385),
        (6, 1, "int", "size", 4, 1),
        (6, 1, "char", "data", 1, 1),
    ];

    struct FixtureType {
        class_id: i32,
        tree: Tree,
    }

    struct FixtureObject {
        path_id: i64,
        type_index: usize,
        payload: Vec<u8>,
    }

    fn push_string(output: &mut Vec<u8>, value: &str) {
        output.extend_from_slice(&i32::try_from(value.len()).unwrap().to_le_bytes());
        output.extend_from_slice(value.as_bytes());
        while !output.len().is_multiple_of(4) {
            output.push(0);
        }
    }

    fn push_pptr(output: &mut Vec<u8>, file_id: i32, path_id: i64) {
        output.extend_from_slice(&file_id.to_le_bytes());
        output.extend_from_slice(&path_id.to_le_bytes());
    }

    fn push_vector3(output: &mut Vec<u8>, value: [f32; 3]) {
        for component in value {
            output.extend_from_slice(&component.to_le_bytes());
        }
    }

    fn push_quaternion(output: &mut Vec<u8>, value: [f32; 4]) {
        for component in value {
            output.extend_from_slice(&component.to_le_bytes());
        }
    }

    fn game_object_payload(name: &str, is_active: bool, components: &[(i32, i64)]) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&i32::try_from(components.len()).unwrap().to_le_bytes());
        for (file_id, path_id) in components {
            push_pptr(&mut payload, *file_id, *path_id);
        }
        payload.extend_from_slice(&0u32.to_le_bytes()); // m_Layer
        push_string(&mut payload, name);
        payload.extend_from_slice(&0u16.to_le_bytes()); // m_Tag
        payload.push(u8::from(is_active));
        payload
    }

    #[allow(clippy::too_many_arguments)]
    fn transform_payload(
        game_object_path_id: i64,
        position: [f32; 3],
        rotation: [f32; 4],
        scale: [f32; 3],
        children: &[i64],
        father: Option<i64>,
        father_file_id: i32,
    ) -> Vec<u8> {
        let mut payload = Vec::new();
        push_pptr(&mut payload, 0, game_object_path_id);
        push_quaternion(&mut payload, rotation);
        push_vector3(&mut payload, position);
        push_vector3(&mut payload, scale);
        payload.extend_from_slice(&i32::try_from(children.len()).unwrap().to_le_bytes());
        for child in children {
            push_pptr(&mut payload, 0, *child);
        }
        match father {
            Some(path_id) => push_pptr(&mut payload, father_file_id, path_id),
            None => push_pptr(&mut payload, 0, 0),
        }
        payload
    }

    fn occlusion_payload(guid: [u32; 4]) -> Vec<u8> {
        let mut payload = Vec::new();
        for word in guid {
            payload.extend_from_slice(&word.to_le_bytes());
        }
        push_pptr(&mut payload, 0, 0);
        payload.extend_from_slice(&0i32.to_le_bytes()); // m_StaticRenderers
        payload.extend_from_slice(&0i32.to_le_bytes()); // m_Portals
        payload
    }

    fn build_settings_payload(scenes: &[&str]) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&i32::try_from(scenes.len()).unwrap().to_le_bytes());
        for scene in scenes {
            push_string(&mut payload, scene);
        }
        payload
    }

    fn asset_bundle_payload(
        container: &str,
        streamed: bool,
        scene_hashes: &[(&str, &str)],
    ) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&1i32.to_le_bytes()); // m_Container size
        push_string(&mut payload, container);
        payload.extend_from_slice(&0i32.to_le_bytes()); // preloadIndex
        payload.extend_from_slice(&0i32.to_le_bytes()); // preloadSize
        push_pptr(&mut payload, 0, 0); // asset
        payload.push(u8::from(streamed));
        payload.extend_from_slice(&i32::try_from(scene_hashes.len()).unwrap().to_le_bytes());
        for (path, name) in scene_hashes {
            push_string(&mut payload, path);
            push_string(&mut payload, name);
        }
        payload
    }

    fn full_asset_bundle_payload(bundle_name: &str, scene_hashes: &[(&str, &str)]) -> Vec<u8> {
        let mut payload = Vec::new();
        push_string(&mut payload, "CAB-scene"); // m_Name
        payload.extend_from_slice(&0i32.to_le_bytes()); // m_PreloadTable size
        payload.extend_from_slice(&0i32.to_le_bytes()); // m_Container size
        payload.extend_from_slice(&0i32.to_le_bytes()); // m_MainAsset.preloadIndex
        payload.extend_from_slice(&0i32.to_le_bytes()); // m_MainAsset.preloadSize
        push_pptr(&mut payload, 0, 0); // m_MainAsset.asset
        payload.extend_from_slice(&0u32.to_le_bytes()); // m_RuntimeCompatibility
        push_string(&mut payload, bundle_name); // m_AssetBundleName
        payload.extend_from_slice(&0i32.to_le_bytes()); // m_Dependencies size
        payload.push(1); // m_IsStreamedSceneAssetBundle
        while !payload.len().is_multiple_of(4) {
            payload.push(0); // TypeTree AlignBytes flag after streamed-scene bool
        }
        payload.extend_from_slice(&0i32.to_le_bytes()); // m_ExplicitDataLayout
        payload.extend_from_slice(&0i32.to_le_bytes()); // m_PathFlags
        payload.extend_from_slice(&i32::try_from(scene_hashes.len()).unwrap().to_le_bytes());
        for (scene_path, collection_name) in scene_hashes {
            push_string(&mut payload, scene_path);
            push_string(&mut payload, collection_name);
        }
        payload
    }

    fn full_asset_bundle_file(scene_hashes: &[(&str, &str)]) -> Vec<u8> {
        build_serialized_file(
            "2019.4.40f1",
            &[FixtureType {
                class_id: ASSET_BUNDLE_CLASS_ID,
                tree: ASSET_BUNDLE_FULL_TREE,
            }],
            &[FixtureObject {
                path_id: ASSET_BUNDLE_PATH_ID,
                type_index: 0,
                payload: full_asset_bundle_payload("streamed-scene-bundle", scene_hashes),
            }],
        )
    }

    fn write_type_tree(output: &mut Vec<u8>, class_id: i32, nodes: Tree) {
        let mut string_buffer = Vec::new();
        let mut offsets = Vec::new();
        for (_, _, type_name, name, _, _) in nodes {
            let type_offset = push_tree_string(&mut string_buffer, type_name);
            let name_offset = push_tree_string(&mut string_buffer, name);
            offsets.push((type_offset, name_offset));
        }
        output.extend_from_slice(&class_id.to_le_bytes());
        output.extend_from_slice(&i32::try_from(nodes.len()).unwrap().to_le_bytes());
        output.extend_from_slice(&i32::try_from(string_buffer.len()).unwrap().to_le_bytes());
        for (index, ((level, version, _, _, byte_size, meta), (type_offset, name_offset))) in
            nodes.iter().zip(offsets).enumerate()
        {
            output.extend_from_slice(&u16::try_from(*version).unwrap().to_le_bytes());
            output.push(*level);
            output.push(0);
            output.extend_from_slice(&type_offset.to_le_bytes());
            output.extend_from_slice(&name_offset.to_le_bytes());
            output.extend_from_slice(&byte_size.to_le_bytes());
            output.extend_from_slice(&i32::try_from(index).unwrap().to_le_bytes());
            output.extend_from_slice(&meta.to_le_bytes());
        }
        output.extend_from_slice(&string_buffer);
    }

    fn push_tree_string(buffer: &mut Vec<u8>, value: &str) -> u32 {
        let offset = u32::try_from(buffer.len()).unwrap();
        buffer.extend_from_slice(value.as_bytes());
        buffer.push(0);
        offset
    }

    fn build_serialized_file(
        version: &str,
        types: &[FixtureType],
        objects: &[FixtureObject],
    ) -> Vec<u8> {
        let mut metadata = version.as_bytes().to_vec();
        metadata.push(0);
        metadata.extend_from_slice(&0u32.to_le_bytes());
        metadata.extend_from_slice(&i32::try_from(types.len()).unwrap().to_le_bytes());
        for fixture_type in types {
            write_type_tree(&mut metadata, fixture_type.class_id, fixture_type.tree);
        }
        metadata.extend_from_slice(&0u32.to_le_bytes()); // generation-10 long FileID flag
        metadata.extend_from_slice(&i32::try_from(objects.len()).unwrap().to_le_bytes());
        let mut data_start = 0usize;
        for object in objects {
            let class_id = types[object.type_index].class_id;
            metadata.extend_from_slice(&i32::try_from(object.path_id).unwrap().to_le_bytes());
            metadata.extend_from_slice(&u32::try_from(data_start).unwrap().to_le_bytes());
            metadata.extend_from_slice(&i32::try_from(object.payload.len()).unwrap().to_le_bytes());
            metadata.extend_from_slice(&class_id.to_le_bytes());
            metadata.extend_from_slice(&i16::try_from(class_id).unwrap().to_le_bytes());
            metadata.extend_from_slice(&0u16.to_le_bytes());
            data_start += object.payload.len();
        }
        metadata.extend_from_slice(&0i32.to_le_bytes()); // external files
        metadata.push(0); // user information
        let data_offset = 20 + metadata.len();
        let payload_bytes: usize = objects.iter().map(|object| object.payload.len()).sum();
        let file_size = data_offset + payload_bytes;
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&u32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&10i32.to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.push(0);
        file.extend_from_slice(&[0; 3]);
        file.extend_from_slice(&metadata);
        for object in objects {
            file.extend_from_slice(&object.payload);
        }
        file
    }

    fn scene_types() -> Vec<FixtureType> {
        vec![
            FixtureType {
                class_id: GAME_OBJECT_CLASS_ID,
                tree: GAME_OBJECT_TREE,
            },
            FixtureType {
                class_id: TRANSFORM_CLASS_ID,
                tree: TRANSFORM_TREE,
            },
            FixtureType {
                class_id: OCCLUSION_CULLING_SETTINGS_CLASS_ID,
                tree: OCCLUSION_TREE,
            },
        ]
    }

    fn level0_file() -> Vec<u8> {
        build_serialized_file(
            "2019.4.40f1",
            &scene_types(),
            &[
                FixtureObject {
                    path_id: GAME_OBJECT_PATH_ID,
                    type_index: 0,
                    payload: game_object_payload("Root", true, &[(0, TRANSFORM_PATH_ID)]),
                },
                FixtureObject {
                    path_id: TRANSFORM_PATH_ID,
                    type_index: 1,
                    payload: transform_payload(
                        GAME_OBJECT_PATH_ID,
                        [1.0, 2.0, 3.0],
                        [0.0, 0.0, 0.0, 1.0],
                        [2.0, 3.0, 4.0],
                        &[],
                        None,
                        0,
                    ),
                },
                FixtureObject {
                    path_id: ORPHAN_GAME_OBJECT_PATH_ID,
                    type_index: 0,
                    payload: game_object_payload("Orphan", false, &[]),
                },
                FixtureObject {
                    path_id: OCCLUSION_PATH_ID,
                    type_index: 2,
                    payload: occlusion_payload([
                        0x1122_3344,
                        0x5566_7788,
                        0x99aa_bbcc,
                        0xddee_ff00,
                    ]),
                },
            ],
        )
    }

    fn level1_file() -> Vec<u8> {
        build_serialized_file(
            "2019.4.40f1",
            &scene_types(),
            &[
                FixtureObject {
                    path_id: SECOND_GAME_OBJECT_PATH_ID,
                    type_index: 0,
                    payload: game_object_payload("Second", true, &[(0, SECOND_TRANSFORM_PATH_ID)]),
                },
                FixtureObject {
                    path_id: SECOND_TRANSFORM_PATH_ID,
                    type_index: 1,
                    payload: transform_payload(
                        SECOND_GAME_OBJECT_PATH_ID,
                        [0.0; 3],
                        [0.0, 0.0, 0.0, 1.0],
                        [1.0; 3],
                        &[],
                        None,
                        0,
                    ),
                },
                FixtureObject {
                    path_id: SECOND_OCCLUSION_PATH_ID,
                    type_index: 2,
                    payload: occlusion_payload([0; 4]),
                },
            ],
        )
    }

    fn build_settings_file(scenes: &[&str]) -> Vec<u8> {
        build_serialized_file(
            "2019.4.40f1",
            &[FixtureType {
                class_id: BUILD_SETTINGS_CLASS_ID,
                tree: BUILD_SETTINGS_TREE,
            }],
            &[FixtureObject {
                path_id: BUILD_SETTINGS_PATH_ID,
                type_index: 0,
                payload: build_settings_payload(scenes),
            }],
        )
    }

    fn asset_bundle_file(container: &str, streamed: bool, hashes: &[(&str, &str)]) -> Vec<u8> {
        build_serialized_file(
            "2019.4.40f1",
            &[FixtureType {
                class_id: ASSET_BUNDLE_CLASS_ID,
                tree: ASSET_BUNDLE_TREE,
            }],
            &[FixtureObject {
                path_id: ASSET_BUNDLE_PATH_ID,
                type_index: 0,
                payload: asset_bundle_payload(container, streamed, hashes),
            }],
        )
    }

    /// The fixture workspace used by the regression and probe-replay tests.
    ///
    /// Names carry no extension because the baseline `SceneHelpers` scene-name regex requires an
    /// exact `levelN`/`maindata` collection name. `globalgamemanagers` supplies the BuildSettings
    /// scene list and `CAB-scene` a streamed-scene AssetBundle whose SceneHashes map
    /// `Assets/Scenes/Streamed.unity` onto the `level0` collection.
    fn fixture_files() -> Vec<(&'static str, Vec<u8>)> {
        vec![
            ("level0", level0_file()),
            ("level1", level1_file()),
            (
                "globalgamemanagers",
                build_settings_file(&[
                    "Assets/Scenes/FromBuild.unity",
                    "Assets/Scenes/Dupe.unity",
                    "Assets/Scenes/Dupe.unity",
                ]),
            ),
            (
                "CAB-scene",
                asset_bundle_file(
                    "Assets/Scenes/Streamed.unity",
                    true,
                    &[("Assets/Scenes/Streamed.unity", "level0")],
                ),
            ),
        ]
    }

    fn run_pipeline(
        files: &[(&str, Vec<u8>)],
    ) -> (WorkspaceSceneProcessing, SerializedFileWorkspace) {
        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/game")).unwrap();
        let mut input = Vec::new();
        for (file_name, bytes) in files {
            let virtual_path = format!("/game/{file_name}");
            filesystem.write_all_bytes(&virtual_path, bytes).unwrap();
            input.push(DependencyFilePath {
                file_name: (*file_name).to_owned(),
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
        let objects = ObjectWorkspace::load_from_vfs_with_layout_source(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
            &FileTypeTreeTestLayouts,
        )
        .unwrap();
        let references = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        let processing = process_workspace_scenes_and_prefabs(
            &filesystem,
            &workspace,
            &objects,
            &references,
            WorkspaceSceneProcessingLimits::default(),
        )
        .unwrap();
        (processing, workspace)
    }

    fn file_index(workspace: &SerializedFileWorkspace, name: &str) -> usize {
        workspace
            .files()
            .iter()
            .position(|file| file.file_name == name)
            .unwrap_or_else(|| panic!("workspace has no file {name}"))
    }

    #[test]
    fn materializes_scene_definitions_from_build_settings_guid_and_streamed_hashes() {
        let (processing, workspace) = run_pipeline(&fixture_files());
        let level0 = file_index(&workspace, "level0");
        let level1 = file_index(&workspace, "level1");

        let definition0 = processing
            .definitions
            .definitions()
            .iter()
            .find(|definition| definition.plan.collection_file_index == level0)
            .expect("level0 must produce a scene definition");
        assert_eq!(definition0.plan.collection_name, "level0");
        assert_eq!(definition0.plan.path, "Assets/Scenes/Streamed");
        assert_eq!(definition0.plan.name, "Streamed");
        assert_eq!(
            definition0.plan.path_source,
            SceneDefinitionPathSource::StreamedSceneBundle
        );
        assert_eq!(
            definition0.plan.guid,
            SceneDefinitionGuidPlan::Preserve(SceneGuidWords {
                data: [0x1122_3344, 0x5566_7788, 0x99aa_bbcc, 0xddee_ff00],
            })
        );
        assert_eq!(definition0.collection_file_indices().to_vec(), vec![level0]);

        let definition1 = processing
            .definitions
            .definitions()
            .iter()
            .find(|definition| definition.plan.collection_file_index == level1)
            .expect("level1 must produce a scene definition");
        assert_eq!(definition1.plan.path, "Assets/Scenes/Dupe");
        assert_eq!(definition1.plan.name, "Dupe");
        assert_eq!(
            definition1.plan.path_source,
            SceneDefinitionPathSource::BuildSettings { scene_index: 1 }
        );
        assert_eq!(
            definition1.plan.guid,
            SceneDefinitionGuidPlan::GenerateAtExport(SceneGuidGenerationReason::ZeroInInput)
        );

        let settings = processing
            .editor_build_settings
            .as_ref()
            .expect("BuildSettings must produce EditorBuildSettings");
        let paths: Vec<&str> = settings
            .scenes()
            .iter()
            .map(|scene| scene.path.as_str())
            .collect();
        assert_eq!(
            paths,
            [
                "Assets/Scenes/FromBuild.unity",
                "Assets/Scenes/Dupe.unity",
                "Assets/Scenes/Dupe.unity"
            ]
        );
        assert!(settings.scenes().iter().all(|scene| scene.enabled));
        assert_eq!(processing.report.scene_definitions, 2);
    }

    #[test]
    fn materializes_typed_game_object_transform_graph_with_ordered_components() {
        let (processing, workspace) = run_pipeline(&fixture_files());
        let level0 = file_index(&workspace, "level0");
        let collection = processing
            .graph
            .collection(level0)
            .expect("level0 has a materialized collection");

        let root = collection
            .game_object(WorkspaceSceneObjectIdentity {
                file_index: level0,
                object_index: 0,
                path_id: GAME_OBJECT_PATH_ID,
                class_id: Some(GAME_OBJECT_CLASS_ID),
            })
            .expect("Root must be materialized");
        assert_eq!(root.name.as_deref(), Some("Root"));
        assert_eq!(root.is_active, Some(true));
        assert_eq!(root.components.len(), 1);
        let MaterializedComponentTarget::Source { path_id, .. } = &root.components[0].target else {
            panic!("Root's first component must be the source Transform slot");
        };
        assert_eq!(*path_id, TRANSFORM_PATH_ID);
        assert_eq!(root.components[0].class_id, Some(TRANSFORM_CLASS_ID));

        let orphan = collection
            .game_object(WorkspaceSceneObjectIdentity {
                file_index: level0,
                object_index: 2,
                path_id: ORPHAN_GAME_OBJECT_PATH_ID,
                class_id: Some(GAME_OBJECT_CLASS_ID),
            })
            .expect("Orphan must be materialized");
        assert_eq!(orphan.name.as_deref(), Some("Orphan"));
        assert_eq!(orphan.is_active, Some(false));

        let transform = collection
            .transforms
            .iter()
            .find(|transform| transform.identity.path_id == TRANSFORM_PATH_ID)
            .expect("the Root Transform must be materialized");
        let values = transform.values.as_ref().expect("Transform values");
        assert_eq!(
            values
                .local_position
                .map(|value| (value.x, value.y, value.z)),
            Some((1.0, 2.0, 3.0))
        );
        assert_eq!(
            values.local_scale.map(|value| (value.x, value.y, value.z)),
            Some((2.0, 3.0, 4.0))
        );
        assert_eq!(
            values
                .local_rotation
                .map(|value| (value.x, value.y, value.z, value.w)),
            Some((0.0, 0.0, 0.0, 1.0))
        );
        assert!(matches!(
            transform.father.as_ref().map(|father| &father.target),
            Some(SceneRelationshipTarget::Null)
        ));
        assert_eq!(collection.game_objects.len(), 2);
        assert_eq!(collection.transforms.len(), 1);
    }

    #[test]
    fn prefab_processor_generates_and_appends_a_missing_transform() {
        let (processing, workspace) = run_pipeline(&fixture_files());
        let level0 = file_index(&workspace, "level0");
        assert_eq!(processing.report.generated_transforms_requested, 1);
        assert_eq!(processing.report.generated_transforms_materialized, 1);
        assert_eq!(processing.generated_transforms.len(), 1);

        let generated = &processing.generated_transforms[0];
        assert_eq!(generated.game_object.path_id, ORPHAN_GAME_OBJECT_PATH_ID);
        assert_eq!(generated.payload.asset.class_id, TRANSFORM_CLASS_ID);
        assert!(!generated.payload.bytes.is_empty());

        // `TransformExtensions.InitializeDefault` against a real 2019.4 class-4 layout, which has
        // no `m_RootOrder`/`m_LocalEulerAnglesHint`.
        let root = &generated.payload.root;
        let position = root
            .unique_field_in_base_chain("m_LocalPosition")
            .unwrap()
            .unwrap();
        assert_eq!(vector3_of(position), [0.0, 0.0, 0.0]);
        let scale = root
            .unique_field_in_base_chain("m_LocalScale")
            .unwrap()
            .unwrap();
        assert_eq!(vector3_of(scale), [1.0, 1.0, 1.0]);
        let rotation = root
            .unique_field_in_base_chain("m_LocalRotation")
            .unwrap()
            .unwrap();
        assert_eq!(quaternion_of(rotation), [0.0, 0.0, 0.0, 1.0]);
        assert!(root
            .unique_field_in_base_chain("m_RootOrder")
            .unwrap()
            .is_none());
        match root
            .unique_field_in_base_chain("m_GameObject")
            .unwrap()
            .unwrap()
        {
            crate::serialized_file::object::SerializedValue::PPtr { path_id, .. } => {
                assert_eq!(*path_id, ORPHAN_GAME_OBJECT_PATH_ID)
            }
            other => panic!("m_GameObject is {other:?}"),
        }

        // The processor mutation appended the generated Transform to Orphan's component list.
        let collection = processing.graph.collection(level0).unwrap();
        let orphan = collection
            .game_object(WorkspaceSceneObjectIdentity {
                file_index: level0,
                object_index: 2,
                path_id: ORPHAN_GAME_OBJECT_PATH_ID,
                class_id: Some(GAME_OBJECT_CLASS_ID),
            })
            .unwrap();
        assert_eq!(orphan.generated_transform, Some(generated.payload.asset));
        assert_eq!(orphan.components.len(), 1);
        match &orphan.components[0].target {
            MaterializedComponentTarget::Generated { asset } => {
                assert_eq!(*asset, generated.payload.asset)
            }
            other => panic!("expected a generated component slot, got {other:?}"),
        }
        assert!(processing.diagnostics.iter().any(|diagnostic| matches!(
            diagnostic,
            WorkspaceSceneProcessingDiagnostic::MissingTransformGenerated { .. }
        )));
        assert!(processing.report.complete, "{:?}", processing.diagnostics);
    }

    #[test]
    fn materializes_scene_hierarchy_assets_and_roots() {
        let (processing, _workspace) = run_pipeline(&fixture_files());
        let hierarchy = processing
            .scene_hierarchies
            .iter()
            .find(|hierarchy| hierarchy.name == "Streamed")
            .expect("the streamed scene must produce a hierarchy asset");
        assert_eq!(
            hierarchy.generated_collection_name,
            "Streamed (Generated Assets)"
        );
        assert_eq!(
            hierarchy
                .game_objects
                .iter()
                .map(|game_object| game_object.path_id)
                .collect::<Vec<_>>(),
            [GAME_OBJECT_PATH_ID, ORPHAN_GAME_OBJECT_PATH_ID]
        );
        assert_eq!(
            hierarchy
                .roots
                .iter()
                .map(|game_object| game_object.path_id)
                .collect::<Vec<_>>(),
            [GAME_OBJECT_PATH_ID, ORPHAN_GAME_OBJECT_PATH_ID]
        );
        assert_eq!(hierarchy.managers.len(), 1);
        assert_eq!(hierarchy.managers[0].path_id, OCCLUSION_PATH_ID);
        assert_eq!(hierarchy.generated_components.len(), 1);
        assert!(!hierarchy.incomplete);
        assert!(processing.prefab_processing.is_complete());
    }

    #[test]
    fn export_project_writes_processed_scene_collections_and_skips_duplicate_build_paths() {
        // Use complete 2019.4 TPK layouts so the frozen generated factory materializes both
        // BuildSettings and the streamed-scene AssetBundle instead of classifying the shortened
        // unit-test shapes as unreadable. `level1` and `level2` map to duplicate paths; the
        // first export wins. The streamed bundle maps `level3` through SceneHashes.
        let files = vec![
            ("level0", level0_file()),
            ("level1", level1_file()),
            ("level2", level1_file()),
            ("level3", level1_file()),
            (
                "globalgamemanagers",
                crate::processing::scene_metadata::tests::make_reference_build_settings_file(&[
                    "Assets/Scenes/FromBuild.unity",
                    "Assets/Scenes/Dupe.unity",
                    "Assets/Scenes/Dupe.unity",
                ]),
            ),
            (
                "CAB-scene",
                full_asset_bundle_file(&[("Assets/Scenes/Streamed.unity", "level3")]),
            ),
        ];

        let mut filesystem = VirtualFileSystem::new();
        filesystem.create_directory(Some("/game")).unwrap();
        let mut inputs = Vec::new();
        for (file_name, bytes) in &files {
            let virtual_path = format!("/game/{file_name}");
            filesystem.write_all_bytes(&virtual_path, bytes).unwrap();
            inputs.push(DependencyFilePath {
                file_name: (*file_name).to_owned(),
                virtual_path,
            });
        }
        let workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &inputs,
            SerializedFileWorkspaceLimits::default(),
        )
        .unwrap();
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let references = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        let scene_processing = process_workspace_scenes_and_prefabs(
            &filesystem,
            &workspace,
            &objects,
            &references,
            WorkspaceSceneProcessingLimits::default(),
        )
        .unwrap();
        assert!(
            scene_processing.report.is_complete(),
            "{:?}",
            scene_processing.diagnostics
        );
        assert!(scene_processing
            .scene_hierarchies
            .iter()
            .all(|hierarchy| hierarchy.asset_bundle_name.is_none()));
        let reference_rows: Vec<_> = include_str!("scene_processor_fixture.txt")
            .lines()
            .filter(|line| {
                matches!(
                    line.split_once('|').map(|(record, _)| record),
                    Some(
                        "HIER"
                            | "HIER-GUID"
                            | "HIER-GO"
                            | "HIER-COMPONENTS"
                            | "HIER-MANAGERS"
                            | "HIER-PREFABS"
                            | "HIER-SCENEROOTS"
                            | "HIER-ROOTS"
                    )
                )
            })
            .collect();
        assert_eq!(
            reference_rows.len(),
            scene_processing.scene_hierarchies.len() * 8
        );

        fn reference_field<'a>(row: &'a str, name: &str) -> &'a str {
            row.strip_prefix(name)
                .or_else(|| row.split('|').find_map(|part| part.strip_prefix(name)))
                .unwrap_or_else(|| panic!("reference row {row:?} has no {name:?} field"))
        }

        let mut reference_scenes: Vec<_> = reference_rows.chunks_exact(8).collect();
        reference_scenes.sort_by_key(|rows| reference_field(rows[0], "path="));
        let mut rust_scenes: Vec<_> = scene_processing
            .hierarchies
            .scenes()
            .iter()
            .zip(&scene_processing.scene_hierarchies)
            .collect();
        rust_scenes.sort_by_key(|(scene, _)| scene.plan.path.as_str());
        for (rows, (scene, hierarchy)) in reference_scenes.into_iter().zip(rust_scenes) {
            assert_eq!(reference_field(rows[0], "scene="), scene.plan.name);
            assert_eq!(reference_field(rows[0], "path="), scene.plan.path);
            assert_eq!(reference_field(rows[0], "name="), scene.plan.name);
            assert_eq!(
                reference_field(rows[0], "bundle="),
                hierarchy.asset_bundle_name.as_deref().unwrap_or("null")
            );
            match scene.plan.guid {
                SceneDefinitionGuidPlan::Preserve(words) => {
                    let mut guid_bytes = [0; 16];
                    for (index, word) in words.data.iter().enumerate() {
                        guid_bytes[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
                    }
                    assert_eq!(
                        reference_field(rows[1], "HIER-GUID|"),
                        crate::export::unity_yaml::format_unity_guid_words(guid_bytes)
                    );
                }
                SceneDefinitionGuidPlan::GenerateAtExport(_) => {
                    // AssetRipper generates these GUIDs randomly in its in-memory scene
                    // definitions, so compare the rest of the scene semantics and validate the
                    // Rust-emitted GUID in the project export below instead of freezing a random
                    // source-runtime value in this fixture.
                }
            }

            let source_ids = |row: &str| {
                row.split_once('|')
                    .unwrap()
                    .1
                    .split(',')
                    .filter_map(|asset| {
                        asset
                            .split_once('#')
                            .and_then(|(_, tail)| tail.split_once(':'))
                            .map(|(path_id, _)| path_id.parse::<i64>().unwrap())
                    })
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                hierarchy
                    .game_objects
                    .iter()
                    .map(|identity| identity.path_id)
                    .collect::<Vec<_>>(),
                source_ids(rows[2])
            );
            assert_eq!(
                hierarchy.components.len() + hierarchy.generated_components.len(),
                reference_field(rows[3], "HIER-COMPONENTS|")
                    .parse::<usize>()
                    .unwrap()
            );
            assert_eq!(
                hierarchy
                    .managers
                    .iter()
                    .map(|identity| identity.path_id)
                    .collect::<Vec<_>>(),
                source_ids(rows[4])
            );
            assert_eq!(
                hierarchy.prefab_instances.len(),
                reference_field(rows[5], "HIER-PREFABS|")
                    .parse::<usize>()
                    .unwrap()
            );
            assert_eq!(
                hierarchy.scene_roots.is_some(),
                reference_field(rows[6], "HIER-SCENEROOTS|") != "null"
            );
            assert_eq!(
                hierarchy
                    .roots
                    .iter()
                    .map(|identity| identity.path_id)
                    .collect::<Vec<_>>(),
                source_ids(rows[7])
            );
        }
        let output_root = crate::test_support::unique_test_directory("scene-project-export");
        let root_catalog = crate::resource::RootResourceFileCatalog::default();
        let provider = crate::io::platform_structure::PlatformResourceProvider::new(None, None);

        let report = crate::export::project_workflow::export_workspace_yaml_project(
            &output_root,
            false,
            &filesystem,
            &root_catalog,
            &provider,
            &workspace,
            &objects,
            &references,
            crate::export::project_workflow::WorkspaceProjectExportLimits::default(),
        )
        .unwrap();

        let first_scene =
            std::fs::read_to_string(output_root.join("Assets/Scenes/FromBuild.unity")).unwrap();
        assert!(first_scene.contains("--- !u!1 &100"));
        assert!(first_scene.contains("--- !u!4 &"));
        assert!(first_scene.contains("m_Component:"));
        assert!(!first_scene.contains("m_Components:"));
        assert!(first_scene.contains("m_GameObject:"));

        let first_meta =
            std::fs::read_to_string(output_root.join("Assets/Scenes/FromBuild.unity.meta"))
                .unwrap();
        let importer_reference = include_str!("default_importer_probe_fixture.txt");
        assert!(importer_reference.contains("VALUE|ExternalObjects|Count = 0"));
        for property in ["AssetBundleName_R", "AssetBundleVariant", "UserData"] {
            assert!(importer_reference
                .lines()
                .any(|line| line == format!("VALUE|{property}|")));
        }
        assert!(first_meta.contains("DefaultImporter:"));
        assert!(first_meta.contains("externalObjects: {}"));
        assert!(first_meta.contains("assetBundleName:"));
        assert!(first_meta
            .lines()
            .any(|line| line.trim() == "assetBundleName:"));
        assert!(first_meta
            .lines()
            .any(|line| line.trim() == "assetBundleVariant:"));
        assert!(!first_meta.contains("mainObjectFileID:"));
        let first_guid = first_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .unwrap();
        let from_build_rows = reference_rows
            .chunks_exact(8)
            .find(|rows| reference_field(rows[0], "path=") == "Assets/Scenes/FromBuild")
            .unwrap();
        assert_eq!(
            first_guid,
            reference_field(from_build_rows[1], "HIER-GUID|")
        );

        let duplicate_scene = output_root.join("Assets/Scenes/Dupe.unity");
        assert!(duplicate_scene.is_file());
        let duplicate_meta =
            std::fs::read_to_string(duplicate_scene.with_extension("unity.meta")).unwrap();
        let duplicate_guid = duplicate_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .unwrap();
        assert_eq!(duplicate_guid.len(), 32);
        assert!(duplicate_guid
            .chars()
            .all(|character| character.is_ascii_hexdigit()));
        assert_ne!(duplicate_guid, "00000000000000000000000000000000");
        assert_ne!(duplicate_guid, first_guid);
        let streamed_scene = output_root.join("Assets/Scenes/Streamed.unity");
        assert!(streamed_scene.is_file());
        let streamed_meta =
            std::fs::read_to_string(streamed_scene.with_extension("unity.meta")).unwrap();
        assert!(streamed_meta.contains("assetBundleName:"));
        let streamed_guid = streamed_meta
            .lines()
            .find_map(|line| line.strip_prefix("guid: "))
            .unwrap();
        assert_eq!(streamed_guid.len(), 32);
        assert!(streamed_guid
            .chars()
            .all(|character| character.is_ascii_hexdigit()));
        assert_ne!(streamed_guid, "00000000000000000000000000000000");
        assert_ne!(streamed_guid, first_guid);
        assert_ne!(streamed_guid, duplicate_guid);
        assert!(report.skipped_objects.iter().any(|notice| {
            notice.reason == "a previous duplicate BuildSettings scene already wrote this path"
        }));
        assert!(!output_root.join("Assets/GameObject/RootA.asset").exists());
        assert!(report.exported_object_count >= 5);

        std::fs::remove_dir_all(output_root).unwrap();
    }

    fn vector3_of(value: &crate::serialized_file::object::SerializedValue) -> [f32; 3] {
        let fields = value.struct_fields().expect("Vector3f struct");
        let mut result = [0.0; 3];
        for (index, name) in ["x", "y", "z"].iter().enumerate() {
            let field = fields
                .iter()
                .find(|field| field.name == *name)
                .unwrap_or_else(|| panic!("Vector3f has no {name}"));
            match &field.value {
                crate::serialized_file::object::SerializedValue::F32(component) => {
                    result[index] = *component
                }
                other => panic!("Vector3f.{name} is {other:?}"),
            }
        }
        result
    }

    fn quaternion_of(value: &crate::serialized_file::object::SerializedValue) -> [f32; 4] {
        let fields = value.struct_fields().expect("Quaternionf struct");
        let mut result = [0.0; 4];
        for (index, name) in ["x", "y", "z", "w"].iter().enumerate() {
            let field = fields
                .iter()
                .find(|field| field.name == *name)
                .unwrap_or_else(|| panic!("Quaternionf has no {name}"));
            match &field.value {
                crate::serialized_file::object::SerializedValue::F32(component) => {
                    result[index] = *component
                }
                other => panic!("Quaternionf.{name} is {other:?}"),
            }
        }
        result
    }

    /// Emit the fixture inputs consumed by `tools/probes/scene-processor` (the isolated reference
    /// runtime probe). Run with:
    /// `cargo test --lib scene_wiring::tests::emit_scene_processor_probe_inputs -- --ignored`.
    #[test]
    #[ignore = "writes probe inputs under target/scene-scratch"]
    fn emit_scene_processor_probe_inputs() {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("scene-scratch");
        std::fs::create_dir_all(&directory).unwrap();
        let files = [
            ("level0", level0_file()),
            ("level1", level1_file()),
            ("level2", level1_file()),
            ("level3", level1_file()),
            (
                "globalgamemanagers",
                crate::processing::scene_metadata::tests::make_reference_build_settings_file(&[
                    "Assets/Scenes/FromBuild.unity",
                    "Assets/Scenes/Dupe.unity",
                    "Assets/Scenes/Dupe.unity",
                ]),
            ),
            (
                "CAB-scene",
                full_asset_bundle_file(&[("Assets/Scenes/Streamed.unity", "level3")]),
            ),
        ];
        for (name, bytes) in files {
            std::fs::write(directory.join(name), bytes).unwrap();
        }
    }

    #[test]
    fn fixture_bytes_are_hand_built_images_not_rust_encoder_output() {
        // The fixtures are hand-assembled SerializedFile images (header + metadata + TypeTree +
        // payload) with literal offsets. This guards the probe inputs against silently becoming
        // self-generated encoder round trips: the production loader must replay them unchanged.
        let level0 = level0_file();
        let parsed = SerializedFile::parse(&level0).unwrap();
        assert_eq!(parsed.metadata.objects.len(), 4);
        assert_eq!(
            parsed
                .metadata
                .objects
                .iter()
                .map(|object| object.class_id)
                .collect::<Vec<_>>(),
            [Some(1), Some(4), Some(1), Some(29)]
        );
        assert_eq!(
            parsed
                .metadata
                .objects
                .iter()
                .map(|object| object.path_id)
                .collect::<Vec<_>>(),
            [
                GAME_OBJECT_PATH_ID,
                TRANSFORM_PATH_ID,
                ORPHAN_GAME_OBJECT_PATH_ID,
                OCCLUSION_PATH_ID
            ]
        );
        let payload = parsed.decode_object_payload(0).unwrap();
        assert_eq!(payload.type_name, "GameObject");
    }
