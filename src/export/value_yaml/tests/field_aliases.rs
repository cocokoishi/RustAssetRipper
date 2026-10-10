use super::*;

#[test]
fn decoded_managed_reference_registry_converts_with_cycles_and_pptr_mapping() {
    let fixture =
        crate::serialized_file::managed_reference_test_support::managed_reference_fixture();
    let decoded = fixture
        .metadata
        .decode_object_payload(&fixture.file, &fixture.header, 0)
        .unwrap();
    let mut observed = Vec::new();
    let document = convert(&decoded.root, &HashMap::new(), |request| {
        observed.push((
            request.field_path.to_owned(),
            request.file_id,
            request.path_id,
        ));
        ProjectPointerResolution::Mapped(UnityMetaPointer {
            file_id: 2345,
            guid: ExportPointerGuid::None,
            asset_type: UnityProjectAssetType::Serialized,
        })
    })
    .unwrap();

    let first = mapping_field(&document.root, "GameObject");
    let first = mapping_field(first, "m_First");
    let UnityYamlNodeKind::Mapping { .. } = &first.kind else {
        panic!("managed identifier should serialize as a mapping");
    };
    assert!(matches!(
        mapping_field(first, "rid").kind,
        UnityYamlNodeKind::Scalar(UnityYamlScalar::Signed(7))
    ));

    let registry = object_field(&document, "references");
    assert!(matches!(
        mapping_field(registry, "version").kind,
        UnityYamlNodeKind::Scalar(UnityYamlScalar::Signed(2))
    ));
    let UnityYamlNodeKind::Sequence { items, .. } = &mapping_field(registry, "RefIds").kind else {
        panic!("registry RefIds should serialize as a sequence");
    };
    assert_eq!(items.len(), 2);
    assert!(matches!(
        mapping_field(&items[0], "rid").kind,
        UnityYamlNodeKind::Scalar(UnityYamlScalar::Signed(7))
    ));
    let type_a = mapping_field(&items[0], "type");
    assert!(matches!(
        mapping_field(type_a, "class").kind,
        UnityYamlNodeKind::Scalar(UnityYamlScalar::String(ref name)) if name == "NodeA"
    ));
    let data_a = mapping_field(&items[0], "data");
    let link_a = mapping_field(data_a, "m_Link");
    assert!(matches!(
        mapping_field(link_a, "rid").kind,
        UnityYamlNodeKind::Scalar(UnityYamlScalar::Signed(9))
    ));
    assert!(matches!(
        mapping_field(data_a, "m_Asset").kind,
        UnityYamlNodeKind::Mapping { .. }
    ));
    assert_eq!(
        observed,
        [
            ("references.RefIds[0].data.m_Asset".to_owned(), 1, 42),
            ("references.RefIds[1].data.m_Asset".to_owned(), 0, 300),
        ]
    );
    let text = crate::export::unity_yaml::write_unity_yaml_documents(
        &[document],
        crate::export::unity_yaml::UnityYamlWriteLimits::default(),
    )
    .unwrap();
    assert!(text.contains("    RefIds:\n"));
    assert!(text.contains("      type: {class: NodeA, ns: Example, asm: Assembly-CSharp}"));
    assert!(text.contains("        m_Link:\n          rid: 9\n"));
}

#[test]
fn importer_yaml_omits_baseline_and_type_tree_ignored_fields() {
    let type_tree = TypeTree {
        nodes: vec![
            type_tree_node(0, "GameObject", "Base", 0),
            type_tree_node(1, "UInt32", "m_ObjectHideFlags", 0),
            type_tree_node(1, "SInt32", "m_EditorOnly", TRANSFER_IGNORE_IN_META_FILES),
            type_tree_node(1, "SInt32", "m_Keep", 0),
            type_tree_node(1, "vector", "m_Items", 0),
            type_tree_node(2, "Array", "Array", 0),
            type_tree_node(3, "SInt32", "size", 0),
            type_tree_node(3, "Item", "data", 0),
            type_tree_node(
                4,
                "SInt32",
                "m_ItemEditorOnly",
                TRANSFER_IGNORE_IN_META_FILES,
            ),
            type_tree_node(4, "SInt32", "m_ItemKeep", 0),
        ],
        string_buffer: Vec::new(),
    };
    let hints = UnityYamlObjectConversionHints::from_type_tree(
        &type_tree,
        version(),
        UnityYamlObjectConversionLimits::default(),
    )
    .unwrap();
    assert!(hints.importer_excluded_field_paths.contains("m_EditorOnly"));
    assert!(hints
        .importer_excluded_field_paths
        .contains("m_Items[*].m_ItemEditorOnly"));

    let value = SerializedValue::Struct(vec![
        field("m_ObjectHideFlags", SerializedValue::U32(1)),
        field("m_EditorOnly", SerializedValue::I32(2)),
        field("m_Keep", SerializedValue::I32(3)),
        field(
            "m_Items",
            SerializedValue::Array(vec![SerializedValue::Struct(vec![
                field("m_ItemEditorOnly", SerializedValue::I32(4)),
                field("m_ItemKeep", SerializedValue::I32(5)),
            ])]),
        ),
    ]);

    let full_document = convert_with_object_hints(&value, &hints, |_| {
        unreachable!("the generated fixture has no PPtrs")
    })
    .unwrap();
    let full_text = crate::export::unity_yaml::write_unity_yaml_documents(
        &[full_document],
        crate::export::unity_yaml::UnityYamlWriteLimits::default(),
    )
    .unwrap();
    assert!(full_text.contains("m_HideFlags: 1"));
    assert!(!full_text.contains("m_ObjectHideFlags"));
    assert!(full_text.contains("m_EditorOnly: 2"));
    assert!(full_text.contains("m_ItemEditorOnly: 4"));

    let importer_document = convert_with_object_field_mode(
        &value,
        &hints,
        UnityYamlObjectFieldMode::AssetImporter,
        |_| unreachable!("the generated fixture has no PPtrs"),
    )
    .unwrap();
    let importer_text = crate::export::unity_yaml::write_unity_yaml_documents(
        &[importer_document],
        crate::export::unity_yaml::UnityYamlWriteLimits::default(),
    )
    .unwrap();
    assert!(!importer_text.contains("m_ObjectHideFlags"));
    assert!(!importer_text.contains("m_EditorOnly"));
    assert!(!importer_text.contains("m_ItemEditorOnly"));
    assert!(importer_text.contains("m_Keep: 3"));
    assert!(importer_text.contains("m_ItemKeep: 5"));
}

#[test]
fn baseline_global_and_prefix_field_names_are_emitted_as_yaml_aliases() {
    let root = SerializedValue::Struct(vec![
        field("m_Image_data", SerializedValue::U8(7)),
        field("m_TextureFormat", SerializedValue::I32(10)),
        field("m_TextureDimension", SerializedValue::I32(2)),
        field("m_ObjectHideFlags", SerializedValue::U32(1)),
        field("m_Val", SerializedValue::I32(-4)),
        field("m_DstPosition", SerializedValue::I32(11)),
        field("m_DestRotation", SerializedValue::I32(12)),
        field("m_SrcScale", SerializedValue::I32(13)),
        field("m_DstÉtage", SerializedValue::I32(17)),
        field("m_Dst𝐀", SerializedValue::I32(19)),
        field("m_Dstposition", SerializedValue::I32(14)),
        field("m_Dst", SerializedValue::I32(15)),
        field("m_Dest", SerializedValue::I32(16)),
        field("m_Destination", SerializedValue::I32(18)),
    ]);
    let document =
        convert_with_object_hints(&root, &UnityYamlObjectConversionHints::default(), |_| {
            unreachable!("the generated fixture has no PPtrs")
        })
        .unwrap();
    let output = crate::export::unity_yaml::write_unity_yaml_documents(
        &[document],
        crate::export::unity_yaml::UnityYamlWriteLimits::default(),
    )
    .unwrap();

    assert!(output.contains(concat!(
        "m_ImageData: 7\n  m_Format: 10\n  m_Dimension: 2\n",
        "  m_HideFlags: 1\n  m_Value: -4\n",
        "  m_DestinationPosition: 11\n  m_DestinationRotation: 12\n",
        "  m_SourceScale: 13\n  m_DestinationÉtage: 17\n  m_Dst𝐀: 19\n",
        "  m_Dstposition: 14\n  m_Dst: 15\n  m_Dest: 16\n",
        "  m_Destination: 18"
    )));
    for serialized_name in [
        "m_Image_data",
        "m_TextureFormat",
        "m_TextureDimension",
        "m_ObjectHideFlags",
        "m_Val",
        "m_DstPosition",
        "m_DestRotation",
        "m_SrcScale",
        "m_DstÉtage",
    ] {
        assert!(
            !output.contains(&format!("  {serialized_name}:")),
            "found {serialized_name}"
        );
    }
}

#[test]
fn animation_clip_settings_field_uses_type_tree_scoped_alias() {
    let animation_clip_type_tree = TypeTree {
        nodes: vec![
            type_tree_node(0, "AnimationClip", "Base", 0),
            type_tree_node(1, "AnimationClipSettings", "m_AnimationClipSettings", 0),
            type_tree_node(2, "bool", "m_LoopTime", 0),
        ],
        string_buffer: Vec::new(),
    };
    let animation_clip_hints = UnityYamlObjectConversionHints::from_type_tree(
        &animation_clip_type_tree,
        version(),
        UnityYamlObjectConversionLimits::default(),
    )
    .unwrap();
    assert_eq!(
        animation_clip_hints
            .field_name_aliases
            .get("m_AnimationClipSettings")
            .map(String::as_str),
        Some("m_MuscleClipInfo")
    );
    let animation_clip_value = SerializedValue::Struct(vec![field(
        "m_AnimationClipSettings",
        SerializedValue::Struct(vec![field("m_LoopTime", SerializedValue::Bool(true))]),
    )]);
    let animation_clip_document = convert_object_with_options(
        source_with_class_id(74),
        &animation_clip_value,
        &animation_clip_hints,
        UnityYamlObjectFieldMode::ProjectAsset,
        false,
        |_| unreachable!("AnimationClip settings contain no PPtrs"),
    )
    .unwrap();
    let animation_clip_output = crate::export::unity_yaml::write_unity_yaml_documents(
        &[animation_clip_document],
        crate::export::unity_yaml::UnityYamlWriteLimits::default(),
    )
    .unwrap();
    assert!(animation_clip_output.contains("m_MuscleClipInfo:"));
    assert!(!animation_clip_output.contains("m_AnimationClipSettings:"));

    let array_type_tree = TypeTree {
        nodes: vec![
            type_tree_node(0, "GameObject", "Base", 0),
            type_tree_node(1, "vector", "m_Clips", 0),
            type_tree_node(2, "Array", "Array", 0),
            type_tree_node(3, "SInt32", "size", 0),
            type_tree_node(3, "AnimationClip", "data", 0),
            type_tree_node(4, "AnimationClipSettings", "m_AnimationClipSettings", 0),
            type_tree_node(5, "bool", "m_LoopTime", 0),
        ],
        string_buffer: Vec::new(),
    };
    let array_hints = UnityYamlObjectConversionHints::from_type_tree(
        &array_type_tree,
        version(),
        UnityYamlObjectConversionLimits::default(),
    )
    .unwrap();
    assert_eq!(
        array_hints
            .field_name_aliases
            .get("m_Clips[*].m_AnimationClipSettings")
            .map(String::as_str),
        Some("m_MuscleClipInfo")
    );
    let array_value = SerializedValue::Struct(vec![field(
        "m_Clips",
        SerializedValue::Array(vec![SerializedValue::Struct(vec![field(
            "m_AnimationClipSettings",
            SerializedValue::Struct(vec![field("m_LoopTime", SerializedValue::Bool(false))]),
        )])]),
    )]);
    let array_document = convert_with_object_hints(&array_value, &array_hints, |_| {
        unreachable!("AnimationClip settings contain no PPtrs")
    })
    .unwrap();
    let array_output = crate::export::unity_yaml::write_unity_yaml_documents(
        &[array_document],
        crate::export::unity_yaml::UnityYamlWriteLimits::default(),
    )
    .unwrap();
    assert!(array_output.contains("m_MuscleClipInfo:"));

    let other_type_tree = TypeTree {
        nodes: vec![
            type_tree_node(0, "GameObject", "Base", 0),
            type_tree_node(1, "AnimationClipSettings", "m_AnimationClipSettings", 0),
            type_tree_node(2, "bool", "m_LoopTime", 0),
        ],
        string_buffer: Vec::new(),
    };
    let other_hints = UnityYamlObjectConversionHints::from_type_tree(
        &other_type_tree,
        version(),
        UnityYamlObjectConversionLimits::default(),
    )
    .unwrap();
    assert!(other_hints.field_name_aliases.is_empty());
    let other_document = convert_with_object_hints(&animation_clip_value, &other_hints, |_| {
        unreachable!("the generated fixture has no PPtrs")
    })
    .unwrap();
    let other_output = crate::export::unity_yaml::write_unity_yaml_documents(
        &[other_document],
        crate::export::unity_yaml::UnityYamlWriteLimits::default(),
    )
    .unwrap();
    assert!(other_output.contains("m_AnimationClipSettings:"));
    assert!(!other_output.contains("m_MuscleClipInfo:"));
}

#[test]
fn generic_binding_type_id_uses_type_tree_scoped_alias() {
    let type_tree = TypeTree {
        nodes: vec![
            type_tree_node(0, "GameObject", "Base", 0),
            type_tree_node(1, "vector", "m_Bindings", 0),
            type_tree_node(2, "Array", "Array", 0),
            type_tree_node(3, "SInt32", "size", 0),
            type_tree_node(3, "GenericBinding", "data", 0),
            type_tree_node(4, "SInt32", "m_TypeID", 0),
        ],
        string_buffer: Vec::new(),
    };
    let hints = UnityYamlObjectConversionHints::from_type_tree(
        &type_tree,
        version(),
        UnityYamlObjectConversionLimits::default(),
    )
    .unwrap();
    assert_eq!(
        hints
            .field_name_aliases
            .get("m_Bindings[*].m_TypeID")
            .map(String::as_str),
        Some("m_ClassID")
    );

    let value = SerializedValue::Struct(vec![field(
        "m_Bindings",
        SerializedValue::Array(vec![SerializedValue::Struct(vec![field(
            "m_TypeID",
            SerializedValue::I32(17),
        )])]),
    )]);
    let document = convert_with_object_hints(&value, &hints, |_| {
        unreachable!("GenericBinding contains no PPtrs")
    })
    .unwrap();
    let output = crate::export::unity_yaml::write_unity_yaml_documents(
        &[document],
        crate::export::unity_yaml::UnityYamlWriteLimits::default(),
    )
    .unwrap();
    assert!(output.contains("m_ClassID: 17"));
    assert!(!output.contains("m_TypeID:"));

    let unrelated_type_tree = TypeTree {
        nodes: vec![
            type_tree_node(0, "GameObject", "Base", 0),
            type_tree_node(1, "SInt32", "m_TypeID", 0),
        ],
        string_buffer: Vec::new(),
    };
    let unrelated_hints = UnityYamlObjectConversionHints::from_type_tree(
        &unrelated_type_tree,
        version(),
        UnityYamlObjectConversionLimits::default(),
    )
    .unwrap();
    assert!(unrelated_hints.field_name_aliases.is_empty());
    let unrelated_value =
        SerializedValue::Struct(vec![field("m_TypeID", SerializedValue::I32(17))]);
    let unrelated_document = convert_with_object_hints(&unrelated_value, &unrelated_hints, |_| {
        unreachable!("the generated fixture has no PPtrs")
    })
    .unwrap();
    let unrelated_output = crate::export::unity_yaml::write_unity_yaml_documents(
        &[unrelated_document],
        crate::export::unity_yaml::UnityYamlWriteLimits::default(),
    )
    .unwrap();
    assert!(unrelated_output.contains("m_TypeID: 17"));
    assert!(!unrelated_output.contains("m_ClassID: 17"));
}

#[test]
fn animator_controller_layer_and_blend_tree_aliases_are_type_scoped() {
    let controller_type_tree = TypeTree {
        nodes: vec![
            type_tree_node(0, "AnimatorController", "Base", 0),
            type_tree_node(1, "vector", "m_AnimatorEvents", 0),
            type_tree_node(2, "Array", "Array", 0),
            type_tree_node(3, "SInt32", "size", 0),
            type_tree_node(3, "AnimatorEvent", "data", 0),
            type_tree_node(1, "vector", "m_Layers", 0),
            type_tree_node(2, "Array", "Array", 0),
            type_tree_node(3, "SInt32", "size", 0),
            type_tree_node(3, "AnimatorControllerLayer", "data", 0),
            type_tree_node(4, "PPtr<AvatarMask>", "m_HumanMask", 0),
        ],
        string_buffer: Vec::new(),
    };
    let controller_hints = UnityYamlObjectConversionHints::from_type_tree(
        &controller_type_tree,
        version(),
        UnityYamlObjectConversionLimits::default(),
    )
    .unwrap();
    assert_eq!(
        controller_hints
            .field_name_aliases
            .get("m_AnimatorEvents")
            .map(String::as_str),
        Some("m_AnimatorParameters")
    );
    assert_eq!(
        controller_hints
            .field_name_aliases
            .get("m_Layers")
            .map(String::as_str),
        Some("m_AnimatorLayers")
    );
    assert_eq!(
        controller_hints
            .field_name_aliases
            .get("m_Layers[*].m_HumanMask")
            .map(String::as_str),
        Some("m_Mask")
    );
    let controller_value = SerializedValue::Struct(vec![
        field(
            "m_AnimatorEvents",
            SerializedValue::Array(vec![SerializedValue::Struct(Vec::new())]),
        ),
        field(
            "m_Layers",
            SerializedValue::Array(vec![SerializedValue::Struct(vec![field(
                "m_HumanMask",
                null_pptr(),
            )])]),
        ),
    ]);
    let controller_document =
        convert_with_object_hints(&controller_value, &controller_hints, |_| {
            unreachable!("the generated null-mask fixture has no non-null PPtrs")
        })
        .unwrap();
    let controller_output = crate::export::unity_yaml::write_unity_yaml_documents(
        &[controller_document],
        crate::export::unity_yaml::UnityYamlWriteLimits::default(),
    )
    .unwrap();
    assert!(controller_output.contains("m_AnimatorParameters:"));
    assert!(controller_output.contains("m_AnimatorLayers:"));
    assert!(controller_output.contains("m_Mask: {fileID: 0}"));
    assert!(!controller_output.contains("m_AnimatorEvents:"));
    assert!(!controller_output.contains("m_Layers:"));
    assert!(!controller_output.contains("m_HumanMask:"));

    let blend_tree_type_tree = TypeTree {
        nodes: vec![
            type_tree_node(0, "BlendTree", "Base", 0),
            type_tree_node(1, "string", "m_BlendEvent", 0),
            type_tree_node(2, "Array", "Array", 0),
            type_tree_node(3, "SInt32", "size", 0),
            type_tree_node(3, "char", "data", 0),
            type_tree_node(1, "string", "m_BlendEventY", 0),
            type_tree_node(2, "Array", "Array", 0),
            type_tree_node(3, "SInt32", "size", 0),
            type_tree_node(3, "char", "data", 0),
        ],
        string_buffer: Vec::new(),
    };
    let blend_tree_hints = UnityYamlObjectConversionHints::from_type_tree(
        &blend_tree_type_tree,
        version(),
        UnityYamlObjectConversionLimits::default(),
    )
    .unwrap();
    let blend_tree_value = SerializedValue::Struct(vec![
        field(
            "m_BlendEvent",
            SerializedValue::StringBytes(b"Horizontal".to_vec()),
        ),
        field(
            "m_BlendEventY",
            SerializedValue::StringBytes(b"Vertical".to_vec()),
        ),
    ]);
    let blend_tree_document =
        convert_with_object_hints(&blend_tree_value, &blend_tree_hints, |_| {
            unreachable!("BlendTree parameters contain no PPtrs")
        })
        .unwrap();
    let blend_tree_output = crate::export::unity_yaml::write_unity_yaml_documents(
        &[blend_tree_document],
        crate::export::unity_yaml::UnityYamlWriteLimits::default(),
    )
    .unwrap();
    assert!(blend_tree_output.contains("m_BlendParameter: Horizontal"));
    assert!(blend_tree_output.contains("m_BlendParameterY: Vertical"));
    assert!(!blend_tree_output.contains("m_BlendEvent:"));
    assert!(!blend_tree_output.contains("m_BlendEventY:"));
}

#[test]
fn lightmap_texture3d_mesh_renderer_and_prefab_aliases_are_type_scoped() {
    let type_tree = TypeTree {
        nodes: vec![
            type_tree_node(0, "GameObject", "Base", 0),
            type_tree_node(1, "SInt32", "m_DataSize", 0),
            type_tree_node(1, "LightmapSettings", "m_Lighting", 0),
            type_tree_node(2, "PPtr<LightingDataAsset>", "m_LightmapSnapshot", 0),
            type_tree_node(1, "Texture3D", "m_Texture", 0),
            type_tree_node(2, "SInt32", "m_DataSize", 0),
            type_tree_node(2, "SInt32", "m_ImageCount", 0),
            type_tree_node(1, "MeshRenderer", "m_Renderer", 0),
            type_tree_node(2, "bool", "m_StitchSeams", 0),
            type_tree_node(1, "Prefab", "m_Prefab", 0),
            type_tree_node(2, "bool", "m_IsPrefabParent", 0),
            type_tree_node(2, "PPtr<Prefab>", "m_ParentPrefab", 0),
        ],
        string_buffer: Vec::new(),
    };
    let hints = UnityYamlObjectConversionHints::from_type_tree(
        &type_tree,
        version(),
        UnityYamlObjectConversionLimits::default(),
    )
    .unwrap();
    for (path, alias) in [
        ("m_Lighting.m_LightmapSnapshot", "m_LightingDataAsset"),
        ("m_Texture.m_DataSize", "m_CompleteImageSize"),
        ("m_Texture.m_ImageCount", "m_Depth"),
        ("m_Renderer.m_StitchSeams", "m_StitchLightmapSeams"),
        ("m_Prefab.m_IsPrefabParent", "m_IsPrefabAsset"),
        ("m_Prefab.m_ParentPrefab", "m_SourcePrefab"),
    ] {
        assert_eq!(
            hints.field_name_aliases.get(path).map(String::as_str),
            Some(alias)
        );
    }

    let value = SerializedValue::Struct(vec![
        field("m_DataSize", SerializedValue::I32(777)),
        field(
            "m_Lighting",
            SerializedValue::Struct(vec![field("m_LightmapSnapshot", null_pptr())]),
        ),
        field(
            "m_Texture",
            SerializedValue::Struct(vec![
                field("m_DataSize", SerializedValue::I32(2048)),
                field("m_ImageCount", SerializedValue::I32(8)),
            ]),
        ),
        field(
            "m_Renderer",
            SerializedValue::Struct(vec![field("m_StitchSeams", SerializedValue::Bool(true))]),
        ),
        field(
            "m_Prefab",
            SerializedValue::Struct(vec![
                field("m_IsPrefabParent", SerializedValue::Bool(true)),
                field("m_ParentPrefab", null_pptr()),
            ]),
        ),
    ]);
    let document = convert_with_object_hints(&value, &hints, |_| {
        unreachable!("the generated pointer fields are null")
    })
    .unwrap();
    let output = crate::export::unity_yaml::write_unity_yaml_documents(
        &[document],
        crate::export::unity_yaml::UnityYamlWriteLimits::default(),
    )
    .unwrap();
    for expected in [
        "m_DataSize: 777",
        "m_LightingDataAsset: {fileID: 0}",
        "m_CompleteImageSize: 2048",
        "m_Depth: 8",
        "m_StitchLightmapSeams: 1",
        "m_IsPrefabAsset: 1",
        "m_SourcePrefab: {fileID: 0}",
    ] {
        assert!(output.contains(expected), "missing {expected}");
    }
    for serialized_name in [
        "m_LightmapSnapshot",
        "m_ImageCount",
        "m_StitchSeams",
        "m_IsPrefabParent",
        "m_ParentPrefab",
    ] {
        assert!(
            !output.contains(&format!("{serialized_name}:")),
            "found old key {serialized_name}"
        );
    }
    assert!(!output.contains("m_DataSize: 2048"));
}

#[test]
fn animation_constant_field_aliases_follow_type_paths() {
    let type_tree = TypeTree {
        nodes: vec![
            type_tree_node(0, "GameObject", "Base", 0),
            type_tree_node(1, "SInt32", "m_MotionSetCount", 0),
            type_tree_node(1, "HumanLayerConstant", "m_HumanLayer", 0),
            type_tree_node(2, "SInt32", "m_StateMachineMotionSetIndex", 0),
            type_tree_node(1, "LayerConstant", "m_Layer", 0),
            type_tree_node(2, "SInt32", "m_StateMachineMotionSetIndex", 0),
            type_tree_node(1, "ControllerConstant", "m_Controller", 0),
            type_tree_node(2, "vector", "m_HumanLayerArray", 0),
            type_tree_node(3, "Array", "Array", 0),
            type_tree_node(4, "SInt32", "size", 0),
            type_tree_node(4, "SInt32", "data", 0),
            type_tree_node(1, "StateMachineConstant", "m_StateMachine", 0),
            type_tree_node(2, "SInt32", "m_MotionSetCount", 0),
        ],
        string_buffer: Vec::new(),
    };
    let hints = UnityYamlObjectConversionHints::from_type_tree(
        &type_tree,
        version(),
        UnityYamlObjectConversionLimits::default(),
    )
    .unwrap();
    for (path, alias) in [
        (
            "m_HumanLayer.m_StateMachineMotionSetIndex",
            "m_StateMachineSynchronizedLayerIndex",
        ),
        (
            "m_Layer.m_StateMachineMotionSetIndex",
            "m_StateMachineSynchronizedLayerIndex",
        ),
        ("m_Controller.m_HumanLayerArray", "m_LayerArray"),
        (
            "m_StateMachine.m_MotionSetCount",
            "m_SynchronizedLayerCount",
        ),
    ] {
        assert_eq!(
            hints.field_name_aliases.get(path).map(String::as_str),
            Some(alias)
        );
    }

    let value = SerializedValue::Struct(vec![
        field("m_MotionSetCount", SerializedValue::I32(99)),
        field(
            "m_HumanLayer",
            SerializedValue::Struct(vec![field(
                "m_StateMachineMotionSetIndex",
                SerializedValue::I32(3),
            )]),
        ),
        field(
            "m_Layer",
            SerializedValue::Struct(vec![field(
                "m_StateMachineMotionSetIndex",
                SerializedValue::I32(4),
            )]),
        ),
        field(
            "m_Controller",
            SerializedValue::Struct(vec![field(
                "m_HumanLayerArray",
                SerializedValue::Array(vec![SerializedValue::I32(5)]),
            )]),
        ),
        field(
            "m_StateMachine",
            SerializedValue::Struct(vec![field("m_MotionSetCount", SerializedValue::I32(6))]),
        ),
    ]);
    let document = convert_with_object_hints(&value, &hints, |_| {
        unreachable!("animation constant fixture has no PPtrs")
    })
    .unwrap();
    let output = crate::export::unity_yaml::write_unity_yaml_documents(
        &[document],
        crate::export::unity_yaml::UnityYamlWriteLimits::default(),
    )
    .unwrap();
    for expected in [
        "m_MotionSetCount: 99",
        "m_StateMachineSynchronizedLayerIndex: 3",
        "m_StateMachineSynchronizedLayerIndex: 4",
        "m_LayerArray:",
        "m_SynchronizedLayerCount: 6",
    ] {
        assert!(output.contains(expected), "missing {expected}");
    }
    assert!(!output.contains("m_HumanLayerArray:"));
    assert!(!output.contains("m_MotionSetCount: 6"));
    assert!(!output.contains("m_StateMachineMotionSetIndex:"));
}

#[test]
fn build_settings_and_heightmap_aliases_follow_type_paths() {
    let type_tree = TypeTree {
        nodes: vec![
            type_tree_node(0, "GameObject", "Base", 0),
            type_tree_node(1, "SInt32", "m_Levels", 0),
            type_tree_node(1, "BuildSettings", "m_BuildSettings", 0),
            type_tree_node(2, "vector", "m_Levels", 0),
            type_tree_node(3, "Array", "Array", 0),
            type_tree_node(4, "SInt32", "size", 0),
            type_tree_node(4, "string", "data", 0),
            type_tree_node(1, "Heightmap", "m_Heightmap", 0),
            type_tree_node(2, "bool", "m_EnableSurfaceMaskTextureCompression", 0),
            type_tree_node(2, "PPtr<Texture2D>", "m_SurfaceMask", 0),
            type_tree_node(2, "SInt32", "m_SurfaceMaskLOD", 0),
        ],
        string_buffer: Vec::new(),
    };
    let hints = UnityYamlObjectConversionHints::from_type_tree(
        &type_tree,
        version(),
        UnityYamlObjectConversionLimits::default(),
    )
    .unwrap();
    for (path, alias) in [
        ("m_BuildSettings.m_Levels", "m_Scenes"),
        (
            "m_Heightmap.m_EnableSurfaceMaskTextureCompression",
            "m_EnableHolesTextureCompression",
        ),
        ("m_Heightmap.m_SurfaceMask", "m_Holes"),
        ("m_Heightmap.m_SurfaceMaskLOD", "m_HolesLOD"),
    ] {
        assert_eq!(
            hints.field_name_aliases.get(path).map(String::as_str),
            Some(alias)
        );
    }

    let value = SerializedValue::Struct(vec![
        field("m_Levels", SerializedValue::I32(123)),
        field(
            "m_BuildSettings",
            SerializedValue::Struct(vec![field(
                "m_Levels",
                SerializedValue::Array(vec![SerializedValue::StringBytes(
                    b"Assets/Scenes/Main.unity".to_vec(),
                )]),
            )]),
        ),
        field(
            "m_Heightmap",
            SerializedValue::Struct(vec![
                field(
                    "m_EnableSurfaceMaskTextureCompression",
                    SerializedValue::Bool(true),
                ),
                field("m_SurfaceMask", null_pptr()),
                field("m_SurfaceMaskLOD", SerializedValue::I32(2)),
            ]),
        ),
    ]);
    let document = convert_with_object_hints(&value, &hints, |_| {
        unreachable!("the generated texture pointer is null")
    })
    .unwrap();
    let output = crate::export::unity_yaml::write_unity_yaml_documents(
        &[document],
        crate::export::unity_yaml::UnityYamlWriteLimits::default(),
    )
    .unwrap();
    for expected in [
        "m_Levels: 123",
        "m_Scenes:",
        "Assets/Scenes/Main.unity",
        "m_EnableHolesTextureCompression: 1",
        "m_Holes: {fileID: 0}",
        "m_HolesLOD: 2",
    ] {
        assert!(output.contains(expected), "missing {expected}");
    }
    assert!(!output.contains("m_EnableSurfaceMaskTextureCompression:"));
    assert!(!output.contains("m_SurfaceMask:"));
    assert!(!output.contains("m_SurfaceMaskLOD:"));
    assert!(!output.contains("m_Levels:\n"));
}

#[test]
fn graphics_and_importer_setting_aliases_are_type_scoped() {
    let type_tree = TypeTree {
        nodes: vec![
            type_tree_node(0, "GameObject", "Base", 0),
            type_tree_node(1, "SInt32", "m_BuildTargetSettings", 0),
            type_tree_node(1, "SInt32", "m_ShaderSettings", 0),
            type_tree_node(1, "GraphicsSettings", "m_Graphics", 0),
            type_tree_node(2, "SInt32", "m_BuildTargetShaderSettings", 0),
            type_tree_node(2, "SInt32", "m_AlbedoSwatchInfos", 0),
            type_tree_node(1, "TextureImporter", "m_Importer", 0),
            type_tree_node(2, "SInt32", "m_BuildTargetSettings", 0),
            type_tree_node(1, "BuildTargetShaderSettings", "m_LegacySettings", 0),
            type_tree_node(2, "SInt32", "m_ShaderSettings", 0),
        ],
        string_buffer: Vec::new(),
    };
    let hints = UnityYamlObjectConversionHints::from_type_tree(
        &type_tree,
        version(),
        UnityYamlObjectConversionLimits::default(),
    )
    .unwrap();
    for (path, alias) in [
        ("m_Graphics.m_BuildTargetShaderSettings", "m_TierSettings"),
        ("m_Graphics.m_AlbedoSwatchInfos", "m_AlbedoSwitchInfos"),
        ("m_Importer.m_BuildTargetSettings", "m_PlatformSettings"),
        ("m_LegacySettings.m_ShaderSettings", "m_Settings"),
    ] {
        assert_eq!(
            hints.field_name_aliases.get(path).map(String::as_str),
            Some(alias)
        );
    }

    let value = SerializedValue::Struct(vec![
        field("m_BuildTargetSettings", SerializedValue::I32(77)),
        field("m_ShaderSettings", SerializedValue::I32(88)),
        field(
            "m_Graphics",
            SerializedValue::Struct(vec![
                field("m_BuildTargetShaderSettings", SerializedValue::I32(1)),
                field("m_AlbedoSwatchInfos", SerializedValue::I32(2)),
            ]),
        ),
        field(
            "m_Importer",
            SerializedValue::Struct(vec![field(
                "m_BuildTargetSettings",
                SerializedValue::I32(3),
            )]),
        ),
        field(
            "m_LegacySettings",
            SerializedValue::Struct(vec![field("m_ShaderSettings", SerializedValue::I32(4))]),
        ),
    ]);
    let document = convert_with_object_hints(&value, &hints, |_| {
        unreachable!("the generated settings fixture has no PPtrs")
    })
    .unwrap();
    let output = crate::export::unity_yaml::write_unity_yaml_documents(
        &[document],
        crate::export::unity_yaml::UnityYamlWriteLimits::default(),
    )
    .unwrap();
    for expected in [
        "m_BuildTargetSettings: 77",
        "m_ShaderSettings: 88",
        "m_TierSettings: 1",
        "m_AlbedoSwitchInfos: 2",
        "m_PlatformSettings: 3",
        "m_Settings: 4",
    ] {
        assert!(output.contains(expected), "missing {expected}");
    }
    assert!(!output.contains("m_BuildTargetShaderSettings:"));
    assert!(!output.contains("m_AlbedoSwatchInfos:"));
    assert!(!output.contains("m_PlatformSettings: 77"));
}

#[test]
fn buffer_vertex_compute_yaml_aliases_are_type_scoped() {
    let type_tree = TypeTree {
        nodes: vec![
            type_tree_node(0, "GameObject", "Base", 0),
            type_tree_node(1, "SInt32", "m_Index", 0),
            type_tree_node(1, "Binding", "m_Binding", 0),
            type_tree_node(2, "SInt32", "m_Set", 0),
            type_tree_node(1, "MatrixParameter", "m_Matrix", 0),
            type_tree_node(2, "SInt32", "m_Index", 0),
            type_tree_node(1, "StructParameter", "m_Struct", 0),
            type_tree_node(2, "SInt32", "m_Index", 0),
            type_tree_node(1, "VectorParameter", "m_Vector", 0),
            type_tree_node(2, "SInt32", "m_Index", 0),
            type_tree_node(1, "VertexData", "m_Vertex", 0),
            type_tree_node(2, "SInt32", "m_DataSize", 0),
            type_tree_node(1, "ComputeShaderPlatformVariant", "m_Compute", 0),
            type_tree_node(2, "SInt32", "m_Kernels", 0),
        ],
        string_buffer: Vec::new(),
    };
    let hints = UnityYamlObjectConversionHints::from_type_tree(
        &type_tree,
        version(),
        UnityYamlObjectConversionLimits::default(),
    )
    .unwrap();
    for (path, alias) in [
        ("m_Binding.m_Set", "m_EncodedData"),
        ("m_Matrix.m_Index", "m_OffsetInConstantBuffer"),
        ("m_Struct.m_Index", "m_OffsetInConstantBuffer"),
        ("m_Vector.m_Index", "m_OffsetInConstantBuffer"),
        ("m_Vertex.m_DataSize", "m_Data"),
        ("m_Compute.m_Kernels", "m_KernelParents"),
    ] {
        assert_eq!(
            hints.field_name_aliases.get(path).map(String::as_str),
            Some(alias)
        );
    }

    let value = SerializedValue::Struct(vec![
        field("m_Index", SerializedValue::I32(99)),
        field(
            "m_Binding",
            SerializedValue::Struct(vec![field("m_Set", SerializedValue::I32(1))]),
        ),
        field(
            "m_Matrix",
            SerializedValue::Struct(vec![field("m_Index", SerializedValue::I32(2))]),
        ),
        field(
            "m_Struct",
            SerializedValue::Struct(vec![field("m_Index", SerializedValue::I32(3))]),
        ),
        field(
            "m_Vector",
            SerializedValue::Struct(vec![field("m_Index", SerializedValue::I32(4))]),
        ),
        field(
            "m_Vertex",
            SerializedValue::Struct(vec![field("m_DataSize", SerializedValue::I32(5))]),
        ),
        field(
            "m_Compute",
            SerializedValue::Struct(vec![field("m_Kernels", SerializedValue::I32(6))]),
        ),
    ]);
    let document = convert_with_object_hints(&value, &hints, |_| {
        unreachable!("the generated shader settings fixture has no PPtrs")
    })
    .unwrap();
    let output = crate::export::unity_yaml::write_unity_yaml_documents(
        &[document],
        crate::export::unity_yaml::UnityYamlWriteLimits::default(),
    )
    .unwrap();
    for expected in [
        "m_Index: 99",
        "m_EncodedData: 1",
        "m_OffsetInConstantBuffer: 2",
        "m_OffsetInConstantBuffer: 3",
        "m_OffsetInConstantBuffer: 4",
        "m_Data: 5",
        "m_KernelParents: 6",
    ] {
        assert!(output.contains(expected), "missing {expected}");
    }
    assert!(!output.contains("m_Set:"));
    assert!(!output.contains("m_DataSize:"));
    assert!(!output.contains("m_Kernels:"));
    assert!(!output.contains("m_Index: 2"));
}

#[test]
fn particle_and_shape_module_aliases_follow_child_type() {
    let type_tree = TypeTree {
        nodes: vec![
            type_tree_node(0, "GameObject", "Base", 0),
            type_tree_node(1, "SInt32", "m_StartDelay", 0),
            type_tree_node(1, "SInt32", "m_Arc", 0),
            type_tree_node(1, "ParticleSystem", "m_ParticleSingle", 0),
            type_tree_node(2, "float", "m_StartDelay", 0),
            type_tree_node(1, "ParticleSystem", "m_ParticleCurve", 0),
            type_tree_node(2, "MinMaxCurve", "m_StartDelay", 0),
            type_tree_node(1, "ShapeModule", "m_ShapeArcCurve", 0),
            type_tree_node(2, "MinMaxCurve", "m_Arc", 0),
            type_tree_node(2, "float", "m_Radius", 0),
            type_tree_node(1, "ShapeModule", "m_ShapeRadiusCurve", 0),
            type_tree_node(2, "float", "m_Arc", 0),
            type_tree_node(2, "MinMaxCurve", "m_Radius", 0),
        ],
        string_buffer: Vec::new(),
    };
    let hints = UnityYamlObjectConversionHints::from_type_tree(
        &type_tree,
        version(),
        UnityYamlObjectConversionLimits::default(),
    )
    .unwrap();
    for (path, alias) in [
        ("m_ParticleSingle.m_StartDelay", "m_StartDelay_Single"),
        ("m_ParticleCurve.m_StartDelay", "m_StartDelay_MinMaxCurve"),
        ("m_ShapeArcCurve.m_Arc", "m_ArcParameter"),
        ("m_ShapeRadiusCurve.m_Radius", "m_RadiusParameter"),
    ] {
        assert_eq!(
            hints.field_name_aliases.get(path).map(String::as_str),
            Some(alias)
        );
    }
    assert!(!hints
        .field_name_aliases
        .contains_key("m_ShapeArcCurve.m_Radius"));
    assert!(!hints
        .field_name_aliases
        .contains_key("m_ShapeRadiusCurve.m_Arc"));
    assert!(!hints.field_name_aliases.contains_key("m_StartDelay"));
    assert!(!hints.field_name_aliases.contains_key("m_Arc"));

    let value = SerializedValue::Struct(vec![
        field("m_StartDelay", SerializedValue::I32(9)),
        field("m_Arc", SerializedValue::I32(8)),
        field(
            "m_ParticleSingle",
            SerializedValue::Struct(vec![field("m_StartDelay", SerializedValue::F32(0.5))]),
        ),
        field(
            "m_ParticleCurve",
            SerializedValue::Struct(vec![field(
                "m_StartDelay",
                SerializedValue::Struct(Vec::new()),
            )]),
        ),
        field(
            "m_ShapeArcCurve",
            SerializedValue::Struct(vec![
                field("m_Arc", SerializedValue::Struct(Vec::new())),
                field("m_Radius", SerializedValue::F32(2.0)),
            ]),
        ),
        field(
            "m_ShapeRadiusCurve",
            SerializedValue::Struct(vec![
                field("m_Arc", SerializedValue::F32(3.0)),
                field("m_Radius", SerializedValue::Struct(Vec::new())),
            ]),
        ),
    ]);
    let document = convert_with_object_hints(&value, &hints, |_| {
        unreachable!("ParticleSystem and ShapeModule fixtures contain no PPtrs")
    })
    .unwrap();
    let output = crate::export::unity_yaml::write_unity_yaml_documents(
        &[document],
        crate::export::unity_yaml::UnityYamlWriteLimits::default(),
    )
    .unwrap();
    for expected in [
        "m_StartDelay: 9",
        "m_Arc: 8",
        "m_StartDelay_Single: 0.5",
        "m_StartDelay_MinMaxCurve: {}",
        "m_ArcParameter: {}",
        "m_Radius: 2",
        "m_RadiusParameter: {}",
        "m_Arc: 3",
    ] {
        assert!(output.contains(expected), "missing {expected}");
    }
}

#[test]
fn clip_muscle_and_xform_aliases_follow_vector_child_type() {
    let mut nodes = vec![
        type_tree_node(0, "GameObject", "Base", 0),
        type_tree_node(1, "SInt32", "m_T", 0),
        type_tree_node(1, "ClipMuscleConstant", "m_Clip", 0),
        type_tree_node(2, "Vector3Float", "m_AverageSpeed", 0),
        type_tree_node(3, "float", "x", 0),
        type_tree_node(3, "float", "y", 0),
        type_tree_node(3, "float", "z", 0),
        type_tree_node(1, "Xform", "m_Xform4_3", 0),
        type_tree_node(2, "Vector4Float", "m_T", 0),
        type_tree_node(3, "float", "x", 0),
        type_tree_node(3, "float", "y", 0),
        type_tree_node(3, "float", "z", 0),
        type_tree_node(3, "float", "w", 0),
        type_tree_node(2, "Vector3Float", "m_S", 0),
        type_tree_node(3, "float", "x", 0),
        type_tree_node(3, "float", "y", 0),
        type_tree_node(3, "float", "z", 0),
        type_tree_node(1, "Xform", "m_Xform3_4", 0),
        type_tree_node(2, "Vector3Float", "m_T", 0),
        type_tree_node(3, "float", "x", 0),
        type_tree_node(3, "float", "y", 0),
        type_tree_node(3, "float", "z", 0),
        type_tree_node(2, "Vector4Float", "m_S", 0),
        type_tree_node(3, "float", "x", 0),
        type_tree_node(3, "float", "y", 0),
        type_tree_node(3, "float", "z", 0),
        type_tree_node(3, "float", "w", 0),
    ];
    nodes.extend([
        type_tree_node(1, "ClipMuscleConstant", "m_ClipPrimitive", 0),
        type_tree_node(2, "float3", "m_AverageSpeed", 0),
        type_tree_node(3, "float", "x", 0),
        type_tree_node(3, "float", "y", 0),
        type_tree_node(3, "float", "z", 0),
        type_tree_node(1, "Xform", "m_XformPrimitive", 0),
        type_tree_node(2, "float4", "m_T", 0),
        type_tree_node(3, "float", "x", 0),
        type_tree_node(3, "float", "y", 0),
        type_tree_node(3, "float", "z", 0),
        type_tree_node(3, "float", "w", 0),
        type_tree_node(2, "float3", "m_S", 0),
        type_tree_node(3, "float", "x", 0),
        type_tree_node(3, "float", "y", 0),
        type_tree_node(3, "float", "z", 0),
    ]);
    let type_tree = TypeTree {
        nodes,
        string_buffer: Vec::new(),
    };
    let hints = UnityYamlObjectConversionHints::from_type_tree(
        &type_tree,
        version(),
        UnityYamlObjectConversionLimits::default(),
    )
    .unwrap();
    for (path, alias) in [
        ("m_Clip.m_AverageSpeed", "m_AverageSpeed3"),
        ("m_Xform4_3.m_T", "m_T4"),
        ("m_Xform4_3.m_S", "m_S3"),
        ("m_Xform3_4.m_T", "m_T3"),
        ("m_Xform3_4.m_S", "m_S4"),
        ("m_ClipPrimitive.m_AverageSpeed", "m_AverageSpeed3"),
        ("m_XformPrimitive.m_T", "m_T4"),
        ("m_XformPrimitive.m_S", "m_S3"),
    ] {
        assert_eq!(
            hints.field_name_aliases.get(path).map(String::as_str),
            Some(alias)
        );
    }
    assert!(!hints.field_name_aliases.contains_key("m_T"));

    let vector3 = || {
        SerializedValue::Struct(vec![
            field("x", SerializedValue::F32(1.0)),
            field("y", SerializedValue::F32(2.0)),
            field("z", SerializedValue::F32(3.0)),
        ])
    };
    let vector4 = || {
        SerializedValue::Struct(vec![
            field("x", SerializedValue::F32(1.0)),
            field("y", SerializedValue::F32(2.0)),
            field("z", SerializedValue::F32(3.0)),
            field("w", SerializedValue::F32(4.0)),
        ])
    };
    let value = SerializedValue::Struct(vec![
        field("m_T", SerializedValue::I32(77)),
        field(
            "m_Clip",
            SerializedValue::Struct(vec![field("m_AverageSpeed", vector3())]),
        ),
        field(
            "m_Xform4_3",
            SerializedValue::Struct(vec![field("m_T", vector4()), field("m_S", vector3())]),
        ),
        field(
            "m_Xform3_4",
            SerializedValue::Struct(vec![field("m_T", vector3()), field("m_S", vector4())]),
        ),
    ]);
    let document = convert_with_object_hints(&value, &hints, |_| {
        unreachable!("the generated vector fixture has no PPtrs")
    })
    .unwrap();
    let output = crate::export::unity_yaml::write_unity_yaml_documents(
        &[document],
        crate::export::unity_yaml::UnityYamlWriteLimits::default(),
    )
    .unwrap();
    for expected in [
        "m_T: 77",
        "m_AverageSpeed3:",
        "m_T4:",
        "m_S3:",
        "m_T3:",
        "m_S4:",
    ] {
        assert!(output.contains(expected), "missing {expected}");
    }
    for serialized_name in ["m_AverageSpeed:", "m_T:", "m_S:"] {
        assert!(
            !output.contains(&format!("    {serialized_name}")),
            "found old nested key {serialized_name}"
        );
    }

    let unsupported = TypeTree {
        nodes: vec![
            type_tree_node(0, "Xform", "Base", 0),
            type_tree_node(1, "SInt32", "m_T", 0),
        ],
        string_buffer: Vec::new(),
    };
    assert!(matches!(
        UnityYamlObjectConversionHints::from_type_tree(
            &unsupported,
            version(),
            UnityYamlObjectConversionLimits::default()
        ),
        Err(UnityYamlObjectConversionError::InvalidTypeTree {
            reason: "unsupported child TypeTree type for baseline field alias",
            ..
        })
    ));
}

#[test]
fn value_array_aliases_follow_nested_vector_element_type() {
    fn append_vector_array_field(
        nodes: &mut Vec<TypeTreeNode>,
        field_name: &str,
        element_type: &str,
        array_name: &str,
        data_name: &str,
    ) {
        nodes.push(type_tree_node(2, "vector", field_name, 0));
        nodes.push(type_tree_node(3, "Array", array_name, 0));
        nodes.push(type_tree_node(4, "SInt32", "size", 0));
        nodes.push(type_tree_node(4, element_type, data_name, 0));
        for component in ["x", "y", "z"] {
            nodes.push(type_tree_node(5, "float", component, 0));
        }
        if element_type == "Vector4Float" {
            nodes.push(type_tree_node(5, "float", "w", 0));
        }
    }

    let mut nodes = vec![
        type_tree_node(0, "GameObject", "Base", 0),
        type_tree_node(1, "SInt32", "m_PositionValues", 0),
        type_tree_node(1, "ValueArray", "m_ThreeFour", 0),
    ];
    append_vector_array_field(
        &mut nodes,
        "m_PositionValues",
        "Vector3Float",
        "m_Array",
        "m_Data",
    );
    append_vector_array_field(
        &mut nodes,
        "m_ScaleValues",
        "Vector4Float",
        "m_Array",
        "m_Data",
    );
    nodes.push(type_tree_node(1, "ValueArray", "m_FourThree", 0));
    append_vector_array_field(
        &mut nodes,
        "m_PositionValues",
        "Vector4Float",
        "m_Array",
        "m_Data",
    );
    append_vector_array_field(
        &mut nodes,
        "m_ScaleValues",
        "Vector3Float",
        "m_Array",
        "m_Data",
    );
    nodes.push(type_tree_node(1, "ValueArray", "m_SourceTypes", 0));
    append_vector_array_field(&mut nodes, "m_PositionValues", "float3", "Array", "data");
    append_vector_array_field(&mut nodes, "m_ScaleValues", "float4", "Array", "data");
    nodes.push(type_tree_node(1, "ValueArray", "m_SourceNames", 0));
    append_vector_array_field(&mut nodes, "m_PositionValues", "float4", "Array", "data");
    append_vector_array_field(&mut nodes, "m_ScaleValues", "float3", "Array", "data");
    let type_tree = TypeTree {
        nodes,
        string_buffer: Vec::new(),
    };
    let hints = UnityYamlObjectConversionHints::from_type_tree(
        &type_tree,
        version(),
        UnityYamlObjectConversionLimits::default(),
    )
    .unwrap();
    for (path, alias) in [
        ("m_ThreeFour.m_PositionValues", "m_PositionValues3"),
        ("m_ThreeFour.m_ScaleValues", "m_ScaleValues4"),
        ("m_FourThree.m_PositionValues", "m_PositionValues4"),
        ("m_FourThree.m_ScaleValues", "m_ScaleValues3"),
        ("m_SourceTypes.m_PositionValues", "m_PositionValues3"),
        ("m_SourceTypes.m_ScaleValues", "m_ScaleValues4"),
        ("m_SourceNames.m_PositionValues", "m_PositionValues4"),
        ("m_SourceNames.m_ScaleValues", "m_ScaleValues3"),
    ] {
        assert_eq!(
            hints.field_name_aliases.get(path).map(String::as_str),
            Some(alias)
        );
    }
    assert!(!hints.field_name_aliases.contains_key("m_PositionValues"));

    let vector3 = || {
        SerializedValue::Struct(vec![
            field("x", SerializedValue::F32(1.0)),
            field("y", SerializedValue::F32(2.0)),
            field("z", SerializedValue::F32(3.0)),
        ])
    };
    let vector4 = || {
        SerializedValue::Struct(vec![
            field("x", SerializedValue::F32(1.0)),
            field("y", SerializedValue::F32(2.0)),
            field("z", SerializedValue::F32(3.0)),
            field("w", SerializedValue::F32(4.0)),
        ])
    };
    let value = SerializedValue::Struct(vec![
        field("m_PositionValues", SerializedValue::I32(77)),
        field(
            "m_ThreeFour",
            SerializedValue::Struct(vec![
                field("m_PositionValues", SerializedValue::Array(vec![vector3()])),
                field("m_ScaleValues", SerializedValue::Array(vec![vector4()])),
            ]),
        ),
        field(
            "m_FourThree",
            SerializedValue::Struct(vec![
                field("m_PositionValues", SerializedValue::Array(vec![vector4()])),
                field("m_ScaleValues", SerializedValue::Array(vec![vector3()])),
            ]),
        ),
    ]);
    let document = convert_with_object_hints(&value, &hints, |_| {
        unreachable!("the generated ValueArray fixture has no PPtrs")
    })
    .unwrap();
    let output = crate::export::unity_yaml::write_unity_yaml_documents(
        &[document],
        crate::export::unity_yaml::UnityYamlWriteLimits::default(),
    )
    .unwrap();
    for expected in [
        "m_PositionValues: 77",
        "m_PositionValues3:",
        "m_PositionValues4:",
        "m_ScaleValues3:",
        "m_ScaleValues4:",
        "x: 1",
        "w: 4",
    ] {
        assert!(output.contains(expected), "missing {expected}");
    }

    let unsupported = TypeTree {
        nodes: vec![
            type_tree_node(0, "ValueArray", "Base", 0),
            type_tree_node(1, "vector", "m_PositionValues", 0),
            type_tree_node(2, "Array", "Array", 0),
            type_tree_node(3, "SInt32", "size", 0),
            type_tree_node(3, "SInt32", "data", 0),
        ],
        string_buffer: Vec::new(),
    };
    assert!(matches!(
        UnityYamlObjectConversionHints::from_type_tree(
            &unsupported,
            version(),
            UnityYamlObjectConversionLimits::default()
        ),
        Err(UnityYamlObjectConversionError::InvalidTypeTree {
            reason: "unsupported ValueArray element TypeTree type",
            ..
        })
    ));
}

#[test]
fn mesh_shape_and_shader_blob_aliases_follow_baseline_rules() {
    let type_tree = TypeTree {
        nodes: vec![
            type_tree_node(0, "GameObject", "Base", 0),
            type_tree_node(1, "vector", "m_Shapes", 0),
            type_tree_node(1, "Mesh", "m_Mesh", 0),
            type_tree_node(2, "vector", "m_Shapes", 0),
            type_tree_node(1, "Mesh", "m_NonVectorMesh", 0),
            type_tree_node(2, "SInt32", "m_Shapes", 0),
            type_tree_node(1, "Shader", "m_Shader", 0),
            type_tree_node(2, "SInt32", "m_SubProgramBlob", 0),
        ],
        string_buffer: Vec::new(),
    };
    let hints = UnityYamlObjectConversionHints::from_type_tree(
        &type_tree,
        version(),
        UnityYamlObjectConversionLimits::default(),
    )
    .unwrap();
    assert_eq!(
        hints
            .field_name_aliases
            .get("m_Mesh.m_Shapes")
            .map(String::as_str),
        Some("m_ShapesList")
    );
    assert!(!hints
        .field_name_aliases
        .contains_key("m_NonVectorMesh.m_Shapes"));
    assert!(!hints.field_name_aliases.contains_key("m_Shapes"));
    assert_eq!(
        hints
            .field_name_aliases
            .get("m_Shader.m_SubProgramBlob")
            .map(String::as_str),
        Some("m_CompressedBlob")
    );

    let value = SerializedValue::Struct(vec![
        field("m_Shapes", SerializedValue::I32(77)),
        field(
            "m_Mesh",
            SerializedValue::Struct(vec![field(
                "m_Shapes",
                SerializedValue::Array(vec![SerializedValue::I32(1)]),
            )]),
        ),
        field(
            "m_NonVectorMesh",
            SerializedValue::Struct(vec![field("m_Shapes", SerializedValue::I32(8))]),
        ),
        field(
            "m_Shader",
            SerializedValue::Struct(vec![field("m_SubProgramBlob", SerializedValue::I32(9))]),
        ),
    ]);
    let document = convert_with_object_hints(&value, &hints, |_| {
        unreachable!("the generated Mesh/Shader fixture has no PPtrs")
    })
    .unwrap();
    let output = crate::export::unity_yaml::write_unity_yaml_documents(
        &[document],
        crate::export::unity_yaml::UnityYamlWriteLimits::default(),
    )
    .unwrap();
    for expected in [
        "m_Shapes: 77",
        "m_ShapesList:",
        "m_Shapes: 8",
        "m_CompressedBlob: 9",
    ] {
        assert!(output.contains(expected), "missing {expected}");
    }
    assert!(!output.contains("m_SubProgramBlob:"));
}
