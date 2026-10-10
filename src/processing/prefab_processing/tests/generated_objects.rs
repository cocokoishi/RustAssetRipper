use super::support::*;
use super::*;
use crate::processing::scene::SceneRelationshipTarget;
use crate::processing::scene_hierarchy::WorkspaceSceneObjectIdentity;
use crate::serialized_file::object::{
    ObjectDefaultValueLimits, ObjectEncodeLimits, ParsedObjectPayload, SerializedFieldValue,
    SerializedValue,
};

#[test]
fn materializes_game_object_component_append_without_changing_source_slots() {
    let game_object = WorkspaceSceneObjectIdentity {
        file_index: 3,
        object_index: 2,
        path_id: 10,
        class_id: Some(1),
    };
    let original_elements = vec![
        SerializedValue::PPtr {
            file_id: 0,
            path_id: 0,
        },
        SerializedValue::PPtr {
            file_id: 2,
            path_id: 88,
        },
    ];
    let source_slots = vec![
        WorkspaceSourceGameObjectComponentSlot {
            source_array_index: 0,
            file_id: 0,
            path_id: 0,
            target: SceneRelationshipTarget::Null,
        },
        WorkspaceSourceGameObjectComponentSlot {
            source_array_index: 1,
            file_id: 2,
            path_id: 88,
            target: SceneRelationshipTarget::MissingLocalObject { path_id: 88 },
        },
    ];
    let generated_component = WorkspaceGeneratedComponentAddition {
        game_object,
        component_class_id: 4,
        appended_index: 2,
        component_asset: WorkspaceProcessedAssetIdentity {
            collection_index: 5,
            path_id: 200,
            class_id: 4,
        },
    };
    let overlay = WorkspaceGameObjectComponentListOverlay {
        game_object,
        source_slots,
        generated_component,
    };
    let source_payload = ParsedObjectPayload {
        object_index: game_object.object_index,
        path_id: game_object.path_id,
        type_name: "GameObject".to_owned(),
        root: SerializedValue::Struct(vec![SerializedFieldValue {
            name: "Base".to_owned(),
            value: SerializedValue::Struct(vec![SerializedFieldValue {
                name: "m_Component".to_owned(),
                value: SerializedValue::Array(original_elements.clone()),
            }]),
        }]),
        consumed_bytes: 16,
        trailing_bytes: vec![0xaa],
        allocated_bytes: 64,
    };

    let edited =
        materialize_game_object_component_append(&source_payload, &overlay, 3, None, 3).unwrap();
    let component_array = edited
        .root
        .unique_field_in_base_chain("m_Component")
        .unwrap()
        .unwrap()
        .array_elements()
        .unwrap();
    assert_eq!(&component_array[..2], original_elements);
    assert_eq!(
        component_array[2].pptr(),
        Some((3, generated_component.component_asset.path_id))
    );
    assert_eq!(edited.trailing_bytes, [0xaa]);

    let wrapper_payload = ParsedObjectPayload {
        root: SerializedValue::Struct(vec![SerializedFieldValue {
            name: "m_Component".to_owned(),
            value: SerializedValue::Array(vec![SerializedValue::Struct(vec![
                SerializedFieldValue {
                    name: "component".to_owned(),
                    value: SerializedValue::PPtr {
                        file_id: 0,
                        path_id: 13,
                    },
                },
            ])]),
        }]),
        ..source_payload.clone()
    };
    let wrapper_overlay = WorkspaceGameObjectComponentListOverlay {
        source_slots: vec![WorkspaceSourceGameObjectComponentSlot {
            source_array_index: 0,
            file_id: 0,
            path_id: 13,
            target: SceneRelationshipTarget::LocalObject { object_index: 4 },
        }],
        generated_component: WorkspaceGeneratedComponentAddition {
            appended_index: 1,
            ..generated_component
        },
        ..overlay.clone()
    };
    let wrapper_edited =
        materialize_game_object_component_append(&wrapper_payload, &wrapper_overlay, 3, None, 2)
            .unwrap();
    assert_eq!(
        wrapper_edited
            .root
            .unique_field_in_base_chain("m_Component")
            .unwrap()
            .unwrap()
            .array_elements()
            .unwrap()[1]
            .unique_field("component")
            .unwrap()
            .unwrap()
            .pptr(),
        Some((3, generated_component.component_asset.path_id))
    );
}

#[test]
fn planned_materialization_uses_only_assigned_source_dependency_file_ids() {
    let game_object = WorkspaceSceneObjectIdentity {
        file_index: 3,
        object_index: 1,
        path_id: 12,
        class_id: Some(1),
    };
    let generated_component = WorkspaceGeneratedComponentAddition {
        game_object,
        component_class_id: 4,
        appended_index: 1,
        component_asset: WorkspaceProcessedAssetIdentity {
            collection_index: 5,
            path_id: 201,
            class_id: 4,
        },
    };
    let overlay = WorkspaceGameObjectComponentListOverlay {
        game_object,
        source_slots: vec![WorkspaceSourceGameObjectComponentSlot {
            source_array_index: 0,
            file_id: 0,
            path_id: 0,
            target: SceneRelationshipTarget::Null,
        }],
        generated_component,
    };
    let source_payload = ParsedObjectPayload {
        object_index: game_object.object_index,
        path_id: game_object.path_id,
        type_name: "GameObject".to_owned(),
        root: SerializedValue::Struct(vec![SerializedFieldValue {
            name: "m_Component".to_owned(),
            value: SerializedValue::Array(vec![SerializedValue::PPtr {
                file_id: 0,
                path_id: 0,
            }]),
        }]),
        consumed_bytes: 12,
        trailing_bytes: Vec::new(),
        allocated_bytes: 32,
    };
    let mut plan = empty_plan();
    plan.source_dependency_appends
        .push(WorkspaceSourceCollectionDependencyAppend {
            source_file_index: game_object.file_index,
            target_collection: WorkspaceSourceDependencyAppendTarget::PrefabProcessedCollection {
                collection_index: generated_component.component_asset.collection_index,
            },
            pointer_field: WorkspaceSourceDependencyPointerField::GameObjectComponent,
            first_source_owner: game_object,
            target_asset: Some(generated_component.component_asset),
            target_animator_state_machine: None,
            file_id: Some(7),
        });

    let edited =
        materialize_planned_game_object_component_append(&plan, &source_payload, &overlay, None, 2)
            .unwrap();
    assert_eq!(
        edited
            .root
            .unique_field_in_base_chain("m_Component")
            .unwrap()
            .unwrap()
            .array_elements()
            .unwrap()[1]
            .pptr(),
        Some((7, 201))
    );

    plan.source_dependency_appends[0].file_id = None;
    let deferred =
        materialize_planned_game_object_component_append(&plan, &source_payload, &overlay, None, 2)
            .unwrap_err();
    assert!(deferred
        .to_string()
        .contains("FileID is deferred or unavailable"));
}

#[test]
fn requires_a_typed_template_for_empty_component_arrays_and_checks_limits() {
    let game_object = WorkspaceSceneObjectIdentity {
        file_index: 1,
        object_index: 0,
        path_id: 22,
        class_id: Some(1),
    };
    let overlay = WorkspaceGameObjectComponentListOverlay {
        game_object,
        source_slots: Vec::new(),
        generated_component: WorkspaceGeneratedComponentAddition {
            game_object,
            component_class_id: 4,
            appended_index: 0,
            component_asset: WorkspaceProcessedAssetIdentity {
                collection_index: 2,
                path_id: 1,
                class_id: 4,
            },
        },
    };
    let payload = ParsedObjectPayload {
        object_index: 0,
        path_id: 22,
        type_name: "GameObject".to_owned(),
        root: SerializedValue::Struct(vec![SerializedFieldValue {
            name: "m_Component".to_owned(),
            value: SerializedValue::Array(Vec::new()),
        }]),
        consumed_bytes: 0,
        trailing_bytes: Vec::new(),
        allocated_bytes: 0,
    };
    let missing_template =
        materialize_game_object_component_append(&payload, &overlay, 1, None, 1).unwrap_err();
    assert!(missing_template
        .to_string()
        .contains("TypeTree-derived element template"));

    let too_many = materialize_game_object_component_append(
        &payload,
        &overlay,
        1,
        Some(&SerializedValue::PPtr {
            file_id: 0,
            path_id: 0,
        }),
        0,
    )
    .unwrap_err();
    assert!(too_many.to_string().contains("exceeding the limit of 0"));
    assert_eq!(
        payload
            .root
            .unique_field_in_base_chain("m_Component")
            .unwrap()
            .unwrap()
            .array_elements()
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn materializes_generated_transform_defaults_and_planned_game_object_pointer() {
    let (plan, asset, game_object) = generated_transform_plan();
    let metadata = transform_metadata(true, true);

    let generated = materialize_planned_generated_transform_payload(
        &plan,
        asset,
        &metadata,
        0,
        ObjectDefaultValueLimits::default(),
        ObjectEncodeLimits::default(),
    )
    .unwrap();

    assert_eq!(generated.asset, asset);
    assert_eq!(generated.type_index, 0);
    assert_eq!(
        generated
            .root
            .unique_field_in_base_chain("m_GameObject")
            .unwrap()
            .unwrap()
            .pptr(),
        Some((1, game_object.path_id))
    );
    assert_eq!(
        generated.root.unique_field("m_Father").unwrap(),
        Some(&SerializedValue::PPtr {
            file_id: 0,
            path_id: 0,
        })
    );
    assert_eq!(
        generated.root.unique_field("m_Children").unwrap(),
        Some(&SerializedValue::Array(Vec::new()))
    );
    assert_eq!(
        generated.root.unique_field("m_RootOrder").unwrap(),
        Some(&SerializedValue::I32(0))
    );
    assert_eq!(
        generated
            .root
            .unique_field("m_LocalEulerAnglesHint")
            .unwrap(),
        Some(&vector3_value(0.0, 0.0, 0.0))
    );
    assert_eq!(
        generated.root.unique_field("m_LocalPosition").unwrap(),
        Some(&vector3_value(0.0, 0.0, 0.0))
    );
    assert_eq!(
        generated.root.unique_field("m_LocalRotation").unwrap(),
        Some(&SerializedValue::Struct(vec![
            SerializedFieldValue {
                name: "x".to_owned(),
                value: SerializedValue::F32(0.0),
            },
            SerializedFieldValue {
                name: "y".to_owned(),
                value: SerializedValue::F32(0.0),
            },
            SerializedFieldValue {
                name: "z".to_owned(),
                value: SerializedValue::F32(0.0),
            },
            SerializedFieldValue {
                name: "w".to_owned(),
                value: SerializedValue::F32(1.0),
            },
        ]))
    );
    assert_eq!(
        generated.root.unique_field("m_LocalScale").unwrap(),
        Some(&vector3_value(1.0, 1.0, 1.0))
    );

    let mut expected = Vec::new();
    expected.extend_from_slice(&1i32.to_le_bytes());
    expected.extend_from_slice(&game_object.path_id.to_le_bytes());
    expected.extend_from_slice(&0i32.to_le_bytes());
    expected.extend_from_slice(&0i64.to_le_bytes());
    expected.extend_from_slice(&0i32.to_le_bytes()); // m_Children size
    expected.extend_from_slice(&0i32.to_le_bytes()); // m_RootOrder
    for value in [0.0f32, 0.0, 0.0] {
        // Euler hint
        expected.extend_from_slice(&value.to_le_bytes());
    }
    for value in [0.0f32, 0.0, 0.0] {
        // Local position
        expected.extend_from_slice(&value.to_le_bytes());
    }
    for value in [0.0f32, 0.0, 0.0, 1.0] {
        // Local rotation
        expected.extend_from_slice(&value.to_le_bytes());
    }
    for value in [1.0f32, 1.0, 1.0] {
        // Local scale
        expected.extend_from_slice(&value.to_le_bytes());
    }
    assert_eq!(generated.bytes, expected);
}

#[test]
fn generated_transform_yaml_resolves_its_constructor_pointer_to_a_local_scene_id() {
    let (plan, asset, game_object) = generated_transform_plan();
    let metadata = transform_metadata(true, true);
    let generated = materialize_planned_generated_transform_payload(
        &plan,
        asset,
        &metadata,
        0,
        ObjectDefaultValueLimits::default(),
        ObjectEncodeLimits::default(),
    )
    .unwrap();
    let constructor_pointer = plan.constructor_pointers()[0];
    let target_asset = crate::export::scene_collection::ExportAssetIdentity {
        collection:
            crate::export::scene_collection::ExportAssetCollectionIdentity::SerializedFile {
                file_index: game_object.file_index,
            },
        object_index: game_object.object_index,
        path_id: game_object.path_id,
        class_id: 1,
    };
    let generated_asset = crate::export::scene_collection::ExportAssetIdentity {
        collection:
            crate::export::scene_collection::ExportAssetCollectionIdentity::ProcessedCollection {
                collection_index: asset.collection_index,
            },
        object_index: 0,
        path_id: asset.path_id,
        class_id: asset.class_id,
    };
    let collection_index = 55;
    let pointer_map = crate::export::pointer_map::ProjectExportPointerMap::new(
        &[
            crate::export::pointer_map::ExportedAssetPointerEntry {
                asset: target_asset,
                file_id: game_object.path_id,
                collection_index,
            },
            crate::export::pointer_map::ExportedAssetPointerEntry {
                asset: generated_asset,
                file_id: 123,
                collection_index,
            },
        ],
        &[
            crate::export::pointer_map::ExportCollectionPointerMetadata {
                collection_index,
                guid: [0x55; 16],
                asset_type: crate::export::pointer_map::UnityProjectAssetType::Serialized,
            },
        ],
        crate::export::pointer_map::ExportPointerMapLimits::default(),
    )
    .unwrap();

    let document =
        crate::export::workspace_yaml::generated_transform_payload_to_unity_yaml_document(
            &generated,
            &metadata,
            generated_asset,
            123,
            constructor_pointer,
            &pointer_map,
            collection_index,
            None,
            crate::export::value_yaml::UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
    let yaml = crate::export::unity_yaml::write_unity_yaml_documents(
        &[document],
        crate::export::unity_yaml::UnityYamlWriteLimits::default(),
    )
    .unwrap();

    assert!(yaml.contains("--- !u!4 &123"), "{yaml}");
    assert!(
        yaml.contains(&format!(
            "m_GameObject: {{fileID: {}}}",
            game_object.path_id
        )),
        "{yaml}"
    );
    assert!(yaml.contains("m_LocalPosition:"), "{yaml}");
    assert!(yaml.contains("m_LocalScale:"), "{yaml}");
    assert!(!yaml.contains("guid:"), "{yaml}");

    let mut wrong_class_metadata = metadata.clone();
    wrong_class_metadata.types[0]
        .type_tree
        .as_mut()
        .unwrap()
        .nodes[0]
        .type_name = Some("NotTransform".to_owned());
    assert!(matches!(
            crate::export::workspace_yaml::generated_transform_payload_to_unity_yaml_document(
                &generated,
                &wrong_class_metadata,
                generated_asset,
                123,
                constructor_pointer,
                &pointer_map,
                collection_index,
                None,
                crate::export::value_yaml::UnityYamlObjectConversionLimits::default(),
            ),
            Err(crate::export::workspace_yaml::UnityYamlWorkspaceExportError::
                GeneratedTransformInvariant { .. })
        ));

    let mut wrong_pointer = generated.clone();
    *wrong_pointer
        .root
        .unique_field_in_base_chain_mut("m_Father")
        .unwrap()
        .unwrap() = SerializedValue::PPtr {
        file_id: constructor_pointer.file_id,
        path_id: constructor_pointer.path_id,
    };
    wrong_pointer.bytes = metadata
        .encode_object_payload_for_type(
            metadata.objects.len(),
            wrong_pointer.type_index,
            &wrong_pointer.root,
            &[],
            ObjectEncodeLimits::default(),
        )
        .unwrap();
    assert!(matches!(
        crate::export::workspace_yaml::generated_transform_payload_to_unity_yaml_document(
            &wrong_pointer,
            &metadata,
            generated_asset,
            123,
            constructor_pointer,
            &pointer_map,
            collection_index,
            None,
            crate::export::value_yaml::UnityYamlObjectConversionLimits::default(),
        ),
        Err(crate::export::workspace_yaml::UnityYamlWorkspaceExportError::
            GeneratedTransformInvariant { message, .. })
            if message.contains("unexpected non-null Transform PPtr")
    ));
}

fn vector3_value(x: f32, y: f32, z: f32) -> SerializedValue {
    SerializedValue::Struct(vec![
        SerializedFieldValue {
            name: "x".to_owned(),
            value: SerializedValue::F32(x),
        },
        SerializedFieldValue {
            name: "y".to_owned(),
            value: SerializedValue::F32(y),
        },
        SerializedFieldValue {
            name: "z".to_owned(),
            value: SerializedValue::F32(z),
        },
    ])
}

#[test]
fn generated_transform_accepts_optional_fields_and_requires_transform_vectors() {
    let (plan, asset, _) = generated_transform_plan();
    let without_euler_hint = transform_metadata(false, true);
    let generated = materialize_planned_generated_transform_payload(
        &plan,
        asset,
        &without_euler_hint,
        0,
        ObjectDefaultValueLimits::default(),
        ObjectEncodeLimits::default(),
    )
    .unwrap();
    assert!(generated
        .root
        .unique_field("m_LocalEulerAnglesHint")
        .unwrap()
        .is_none());

    // 2019.4's class-4 TPK omits m_RootOrder; the source setter treats it as optional.
    let without_root_order = transform_metadata(false, false);
    let generated = materialize_planned_generated_transform_payload(
        &plan,
        asset,
        &without_root_order,
        0,
        ObjectDefaultValueLimits::default(),
        ObjectEncodeLimits::default(),
    )
    .unwrap();
    assert!(generated
        .root
        .unique_field("m_RootOrder")
        .unwrap()
        .is_none());

    // The position, rotation, and scale fields are unconditional factory writes.
    let mut without_scale = without_root_order;
    let nodes = &mut without_scale.types[0].type_tree.as_mut().unwrap().nodes;
    let scale_start = nodes
        .iter()
        .position(|node| node.name.as_deref() == Some("m_LocalScale"))
        .expect("the fixture has m_LocalScale");
    let mut scale_end = scale_start + 1;
    while scale_end < nodes.len() && nodes[scale_end].level > nodes[scale_start].level {
        scale_end += 1;
    }
    nodes.drain(scale_start..scale_end);
    let error = materialize_planned_generated_transform_payload(
        &plan,
        asset,
        &without_scale,
        0,
        ObjectDefaultValueLimits::default(),
        ObjectEncodeLimits::default(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("no m_LocalScale field"));
}

#[test]
fn generated_transform_rejects_wrong_type_and_dependency_identity() {
    let (plan, asset, _) = generated_transform_plan();
    let mut wrong_type = transform_metadata(true, true);
    wrong_type.types[0].raw_type_id = 224;
    let wrong_type_error = materialize_planned_generated_transform_payload(
        &plan,
        asset,
        &wrong_type,
        0,
        ObjectDefaultValueLimits::default(),
        ObjectEncodeLimits::default(),
    )
    .unwrap_err();
    assert!(wrong_type_error.to_string().contains("expected 4"));

    let mut wrong_dependency = plan.clone();
    wrong_dependency.collections[asset.collection_index].dependencies[1].target =
        WorkspaceProcessedDependencyTarget::SerializedFileCollection { file_index: 4 };
    let dependency_error = materialize_planned_generated_transform_payload(
        &wrong_dependency,
        asset,
        &transform_metadata(true, true),
        0,
        ObjectDefaultValueLimits::default(),
        ObjectEncodeLimits::default(),
    )
    .unwrap_err();
    assert!(dependency_error
        .to_string()
        .contains("does not uniquely target its source SerializedFile"));
}
