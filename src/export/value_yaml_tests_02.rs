    #[test]
    fn game_object_component_array_uses_plural_baseline_field_name() {
        let type_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "vector", "m_Component", 0),
                type_tree_node(1, "SomeStruct", "m_Unrelated", 0),
                type_tree_node(2, "SInt32", "m_Component", 0),
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
                .get("m_Component")
                .map(String::as_str),
            Some("m_Components")
        );
        assert!(!hints
            .field_name_aliases
            .contains_key("m_Unrelated.m_Component"));

        let value = SerializedValue::Struct(vec![
            field(
                "m_Component",
                SerializedValue::Array(vec![SerializedValue::I32(123)]),
            ),
            field(
                "m_Unrelated",
                SerializedValue::Struct(vec![field("m_Component", SerializedValue::I32(456))]),
            ),
        ]);
        let document = convert_with_object_hints(&value, &hints, |_| {
            unreachable!("the generated GameObject fixture has no PPtrs")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(output.contains("m_Components:"));
        assert!(output.contains("m_Component: 456"));
        assert!(!output.contains("m_Component:\n"));
    }

    #[test]
    fn stripped_asset_yaml_keeps_only_baseline_prefab_fields() {
        let root = SerializedValue::Struct(vec![field(
            "Base",
            SerializedValue::Struct(vec![
                field("m_CorrespondingSourceObject", null_pptr()),
                field("m_PrefabAsset", null_pptr()),
                field("m_PrefabInstance", null_pptr()),
                field("m_PrefabInternal", null_pptr()),
                field("m_PrefabParentObject", null_pptr()),
                field("m_ObjectHideFlags", SerializedValue::U32(0)),
                field(
                    "m_Name",
                    SerializedValue::StringBytes(b"not retained".to_vec()),
                ),
            ]),
        )]);
        let document = convert_object_with_options(
            source(),
            &root,
            &UnityYamlObjectConversionHints::default(),
            UnityYamlObjectFieldMode::ProjectAsset,
            true,
            |_| ProjectPointerResolution::Null,
        )
        .unwrap();
        assert!(document.root.stripped);
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(output.contains("--- !u!1 &1000001 stripped"));
        assert!(output.contains("m_CorrespondingSourceObject: {fileID: 0}"));
        assert!(output.contains("m_PrefabAsset: {fileID: 0}"));
        assert!(output.contains("m_PrefabInstance: {fileID: 0}"));
        assert!(output.contains("m_PrefabInternal: {fileID: 0}"));
        assert!(output.contains("m_PrefabParentObject: {fileID: 0}"));
        assert!(!output.contains("serializedVersion"));
        assert!(!output.contains("m_ObjectHideFlags"));
        assert!(!output.contains("m_Name"));
    }

    #[test]
    fn stripped_monobehaviour_yaml_keeps_baseline_identity_fields() {
        let root = SerializedValue::Struct(vec![
            field("m_CorrespondingSourceObject", null_pptr()),
            field(
                "m_EditorClassIdentifier",
                SerializedValue::StringBytes(Vec::new()),
            ),
            field("m_EditorHideFlags", SerializedValue::I32(0)),
            field("m_Enabled", SerializedValue::Bool(true)),
            field("m_GameObject", null_pptr()),
            field("m_Name", SerializedValue::StringBytes(Vec::new())),
            field("m_PrefabAsset", null_pptr()),
            field("m_PrefabInstance", null_pptr()),
            field("m_PrefabInternal", null_pptr()),
            field("m_PrefabParentObject", null_pptr()),
            field("m_Script", null_pptr()),
            field("m_ObjectHideFlags", SerializedValue::U32(0)),
            field("m_CustomData", SerializedValue::I32(123)),
        ]);
        let document = convert_object_with_options(
            source_with_class_id(114),
            &root,
            &UnityYamlObjectConversionHints::default(),
            UnityYamlObjectFieldMode::ProjectAsset,
            true,
            |_| ProjectPointerResolution::Null,
        )
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(output.contains("--- !u!114 &1000001 stripped"));
        for field_name in STRIPPED_MONO_BEHAVIOUR_ALLOWED_FIELDS {
            assert!(output.contains(field_name), "missing {field_name}");
        }
        assert!(!output.contains("serializedVersion"));
        assert!(!output.contains("m_ObjectHideFlags"));
        assert!(!output.contains("m_CustomData"));
    }

    #[test]
    fn monobehaviour_structure_field_is_written_last() {
        let value = SerializedValue::Struct(vec![
            field("m_Before", SerializedValue::I32(1)),
            field(
                "m_Structure",
                SerializedValue::Struct(vec![field("m_Value", SerializedValue::I32(2))]),
            ),
            field("m_After", SerializedValue::I32(3)),
        ]);
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[convert_object_with_options(
                source_with_class_id(114),
                &value,
                &UnityYamlObjectConversionHints::default(),
                UnityYamlObjectFieldMode::ProjectAsset,
                false,
                |_| unreachable!("the generated fixture has no PPtrs"),
            )
            .unwrap()],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        let before = output.find("  m_Before: 1").unwrap();
        let after = output.find("  m_After: 3").unwrap();
        let structure = output.find("  m_Structure:").unwrap();
        assert!(before < after && after < structure);

        let non_mono_output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[convert_object_with_options(
                source(),
                &value,
                &UnityYamlObjectConversionHints::default(),
                UnityYamlObjectFieldMode::ProjectAsset,
                false,
                |_| unreachable!("the generated fixture has no PPtrs"),
            )
            .unwrap()],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        let non_mono_structure = non_mono_output.find("  m_Structure:").unwrap();
        let non_mono_after = non_mono_output.find("  m_After: 3").unwrap();
        assert!(non_mono_structure < non_mono_after);
    }

    #[test]
    fn recovered_monobehaviour_structure_matches_source_yaml_shape() {
        let root = SerializedValue::Struct(vec![
            field("m_GameObject", null_pptr()),
            field("m_Enabled", SerializedValue::Bool(true)),
            field("m_Script", null_pptr()),
            field(
                "m_Name",
                SerializedValue::StringBytes(b"Behaviour".to_vec()),
            ),
            field(
                "m_Structure",
                SerializedValue::Struct(vec![
                    field("inherited", SerializedValue::I64(0x0102_0304_0506_0708)),
                    field("health", SerializedValue::I32(1234)),
                    field("speed", SerializedValue::F32(1.5)),
                ]),
            ),
        ]);
        let document = convert_object_with_options(
            source_with_class_id(114),
            &root,
            &UnityYamlObjectConversionHints::default(),
            UnityYamlObjectFieldMode::ProjectAsset,
            false,
            |_| ProjectPointerResolution::Null,
        )
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        for line in [
            "  m_ObjectHideFlags: 0",
            "  m_CorrespondingSourceObject: {fileID: 0}",
            "  m_PrefabInstance: {fileID: 0}",
            "  m_PrefabAsset: {fileID: 0}",
            "  m_EditorHideFlags: 0",
            "  m_EditorClassIdentifier:",
            "  inherited: 72623859790382856",
            "  health: 1234",
            "  speed: 1.5",
        ] {
            assert!(output.contains(line), "missing {line:?} in:\n{output}");
        }
        assert!(!output.contains("m_Structure"), "{output}");
        let name = output.find("  m_Name: Behaviour").unwrap();
        let class_identifier = output.find("  m_EditorClassIdentifier:").unwrap();
        let inherited = output.find("  inherited: 72623859790382856").unwrap();
        assert!(
            name < class_identifier && class_identifier < inherited,
            "{output}"
        );
    }

    #[test]
    fn dictionary_yaml_uses_type_tree_keys_and_unity_version_shape() {
        let string_map_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "map", "m_StringMap", 0),
                type_tree_node(2, "Array", "Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "pair", "data", 0),
                type_tree_node(4, "string", "first", 0),
                type_tree_node(4, "SInt32", "second", 0),
            ],
            string_buffer: Vec::new(),
        };
        let string_hints = UnityYamlObjectConversionHints::from_type_tree(
            &string_map_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert_eq!(
            string_hints.dictionary_representations.get("m_StringMap"),
            Some(&UnityYamlDictionaryRepresentation::KeyedMapping)
        );
        let string_map = SerializedValue::Struct(vec![field(
            "m_StringMap",
            SerializedValue::Array(vec![
                SerializedValue::Struct(vec![
                    field("first", SerializedValue::StringBytes(b"One".to_vec())),
                    field("second", SerializedValue::I32(1)),
                ]),
                SerializedValue::Struct(vec![
                    field("first", SerializedValue::StringBytes(b"Two".to_vec())),
                    field("second", SerializedValue::I32(2)),
                ]),
            ]),
        )]);
        let string_output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[convert_with_object_hints(&string_map, &string_hints, |_| {
                unreachable!("the generated dictionary has no PPtrs")
            })
            .unwrap()],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(string_output.contains("    One: 1"));
        assert!(string_output.contains("    Two: 2"));

        let empty_string_map = SerializedValue::Struct(vec![field(
            "m_StringMap",
            SerializedValue::Array(Vec::new()),
        )]);
        let empty_string_output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[
                convert_with_object_hints(&empty_string_map, &string_hints, |_| {
                    unreachable!("the generated dictionary has no PPtrs")
                })
                .unwrap(),
            ],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(empty_string_output.contains("m_StringMap: {}"));

        let bool_map_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "map", "m_BoolMap", 0),
                type_tree_node(2, "Array", "Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "pair", "data", 0),
                type_tree_node(4, "bool", "first", 0),
                type_tree_node(4, "SInt32", "second", 0),
            ],
            string_buffer: Vec::new(),
        };
        let bool_hints = UnityYamlObjectConversionHints::from_type_tree(
            &bool_map_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        let bool_map = SerializedValue::Struct(vec![field(
            "m_BoolMap",
            SerializedValue::Array(vec![
                SerializedValue::Struct(vec![
                    field("first", SerializedValue::Bool(true)),
                    field("second", SerializedValue::I32(1)),
                ]),
                SerializedValue::Struct(vec![
                    field("first", SerializedValue::Bool(false)),
                    field("second", SerializedValue::I32(0)),
                ]),
            ]),
        )]);
        let bool_output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[convert_with_object_hints(&bool_map, &bool_hints, |_| {
                unreachable!("the generated dictionary has no PPtrs")
            })
            .unwrap()],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(bool_output.contains("True: 1"));
        assert!(bool_output.contains("False: 0"));

        let mut integer_map_tree = bool_map_tree.clone();
        integer_map_tree.nodes[5].type_name = Some("SInt32".to_owned());
        let integer_hints = UnityYamlObjectConversionHints::from_type_tree(
            &integer_map_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        let integer_map = SerializedValue::Struct(vec![field(
            "m_BoolMap",
            SerializedValue::Array(vec![
                SerializedValue::Struct(vec![
                    field("first", SerializedValue::I32(-2)),
                    field("second", SerializedValue::I32(-20)),
                ]),
                SerializedValue::Struct(vec![
                    field("first", SerializedValue::I32(3)),
                    field("second", SerializedValue::I32(30)),
                ]),
            ]),
        )]);
        let integer_output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[
                convert_with_object_hints(&integer_map, &integer_hints, |_| {
                    unreachable!("the generated dictionary has no PPtrs")
                })
                .unwrap(),
            ],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(integer_output.contains("-2: -20"));
        assert!(integer_output.contains("3: 30"));

        let char_map_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "map", "m_CharMap", 0),
                type_tree_node(2, "Array", "Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "pair", "data", 0),
                type_tree_node(4, "char", "first", 0),
                type_tree_node(4, "SInt32", "second", 0),
            ],
            string_buffer: Vec::new(),
        };
        let char_hints = UnityYamlObjectConversionHints::from_type_tree(
            &char_map_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        let char_map = SerializedValue::Struct(vec![field(
            "m_CharMap",
            SerializedValue::Array(vec![SerializedValue::Struct(vec![
                field("first", SerializedValue::Char(u16::from(b'A'))),
                field("second", SerializedValue::I32(1)),
            ])]),
        )]);
        let char_output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[convert_with_object_hints(&char_map, &char_hints, |_| {
                unreachable!("the generated dictionary has no PPtrs")
            })
            .unwrap()],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(char_output.contains("A: 1"));

        let guid_map_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "map", "m_GuidMap", 0),
                type_tree_node(2, "Array", "Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "pair", "data", 0),
                type_tree_node(4, "GUID", "first", 0),
                type_tree_node(5, "UInt32", "m_Data_0_", 0),
                type_tree_node(5, "UInt32", "m_Data_1_", 0),
                type_tree_node(5, "UInt32", "m_Data_2_", 0),
                type_tree_node(5, "UInt32", "m_Data_3_", 0),
                type_tree_node(4, "bool", "second", 0),
            ],
            string_buffer: Vec::new(),
        };
        let guid_hints = UnityYamlObjectConversionHints::from_type_tree(
            &guid_map_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        let guid_map = SerializedValue::Struct(vec![field(
            "m_GuidMap",
            SerializedValue::Array(vec![SerializedValue::Struct(vec![
                field("first", guid_value_struct()),
                field("second", SerializedValue::Bool(true)),
            ])]),
        )]);
        let guid_output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[convert_with_object_hints(&guid_map, &guid_hints, |_| {
                unreachable!("the generated dictionary has no PPtrs")
            })
            .unwrap()],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(guid_output.contains("40302010413121114232221243332313: 1"));

        let vector_map_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "map", "m_VectorMap", 0),
                type_tree_node(2, "Array", "Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "pair", "data", 0),
                type_tree_node(4, "Vector3f", "first", 0),
                type_tree_node(5, "float", "x", 0),
                type_tree_node(5, "float", "y", 0),
                type_tree_node(5, "float", "z", 0),
                type_tree_node(4, "SInt32", "second", 0),
            ],
            string_buffer: Vec::new(),
        };
        let vector_map_value = SerializedValue::Struct(vec![field(
            "m_VectorMap",
            SerializedValue::Array(vec![
                SerializedValue::Struct(vec![
                    field(
                        "first",
                        SerializedValue::Struct(vec![
                            field("x", SerializedValue::F32(1.0)),
                            field("y", SerializedValue::F32(2.0)),
                            field("z", SerializedValue::F32(3.0)),
                        ]),
                    ),
                    field("second", SerializedValue::I32(4)),
                ]),
                SerializedValue::Struct(vec![
                    field(
                        "first",
                        SerializedValue::Struct(vec![
                            field("x", SerializedValue::F32(5.0)),
                            field("y", SerializedValue::F32(6.0)),
                            field("z", SerializedValue::F32(7.0)),
                        ]),
                    ),
                    field("second", SerializedValue::I32(8)),
                ]),
            ]),
        )]);
        let modern_hints = UnityYamlObjectConversionHints::from_type_tree(
            &vector_map_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert_eq!(
            modern_hints.dictionary_representations.get("m_VectorMap"),
            Some(&UnityYamlDictionaryRepresentation::PairSequence)
        );
        let modern_output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[
                convert_with_object_hints(&vector_map_value, &modern_hints, |_| {
                    unreachable!("the generated dictionary has no PPtrs")
                })
                .unwrap(),
            ],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert_eq!(modern_output.matches("  - first:").count(), 2);

        let legacy_version = SceneUnityVersion::parse("5.3.8f1").unwrap();
        let legacy_hints = UnityYamlObjectConversionHints::from_type_tree(
            &vector_map_tree,
            legacy_version,
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert_eq!(
            legacy_hints.dictionary_representations.get("m_VectorMap"),
            Some(&UnityYamlDictionaryRepresentation::LegacyPairMapping)
        );
        let legacy_output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[
                convert_with_object_hints(&vector_map_value, &legacy_hints, |_| {
                    unreachable!("the generated dictionary has no PPtrs")
                })
                .unwrap(),
            ],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert_eq!(legacy_output.matches("    data:").count(), 2);
    }

    #[test]
    fn float_dictionary_keys_use_dotnet_tostring_not_rust_display() {
        // Negative zero keeps its sign, exactly like the source `ToString` (the
        // float fixture pins `-0` for these bits through the same call).
        assert_eq!(format_dictionary_float32_key(-0.0).unwrap(), "-0");
        assert_eq!(format_dictionary_float64_key(-0.0).unwrap(), "-0");
        // Fixed/scientific choices follow the invariant-culture shortest form:
        // fixed while the base-10 exponent is in -4..9 for floats (-4..17 for
        // doubles), matching the probed ToString fixture (`16777216`, `1E+10`).
        assert_eq!(format_dictionary_float32_key(0.5).unwrap(), "0.5");
        assert_eq!(format_dictionary_float32_key(1e-7).unwrap(), "1E-07");
        assert_eq!(format_dictionary_float32_key(1e7).unwrap(), "10000000");
        assert_eq!(format_dictionary_float32_key(1e10).unwrap(), "1E+10");
        assert_eq!(format_dictionary_float64_key(0.5).unwrap(), "0.5");
        assert_eq!(format_dictionary_float64_key(1e-7).unwrap(), "1E-07");
        assert_eq!(format_dictionary_float64_key(1e17).unwrap(), "1E+17");
        // Special values keep their source spellings.
        assert_eq!(format_dictionary_float32_key(f32::NAN).unwrap(), "NaN");
        assert_eq!(
            format_dictionary_float64_key(f64::NEG_INFINITY).unwrap(),
            "-Infinity"
        );
    }

    #[test]
    fn exposed_reference_table_entries_use_block_sequences_instead_of_keyed_maps() {
        let exposed_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "ExposedReferenceTable", "Base", 0),
                type_tree_node(1, "map", "m_References", 0),
                type_tree_node(2, "Array", "Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "pair", "data", 0),
                type_tree_node(4, "string", "first", 0),
                type_tree_node(5, "Array", "Array", 0),
                type_tree_node(6, "SInt32", "size", 0),
                type_tree_node(6, "char", "data", 0),
                type_tree_node(4, "SInt32", "second", 0),
            ],
            string_buffer: Vec::new(),
        };
        let exposed_hints = UnityYamlObjectConversionHints::from_type_tree(
            &exposed_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert!(exposed_hints
            .force_block_sequence_paths
            .contains("m_References"));
        assert!(!exposed_hints
            .dictionary_representations
            .contains_key("m_References"));
        assert_eq!(
            exposed_hints.empty_array_kinds.get("m_References"),
            Some(&UnityYamlEmptyArrayKind::BlockSequence)
        );

        let entries = SerializedValue::Array(vec![
            SerializedValue::Struct(vec![
                field("first", SerializedValue::StringBytes(b"Player".to_vec())),
                field("second", SerializedValue::I32(9)),
            ]),
            SerializedValue::Struct(vec![
                field(
                    "first",
                    SerializedValue::StringBytes(b"Object.Name".to_vec()),
                ),
                field("second", SerializedValue::I32(14)),
            ]),
        ]);
        let exposed_value = SerializedValue::Struct(vec![field("m_References", entries.clone())]);
        let exposed_document = convert_with_object_hints(&exposed_value, &exposed_hints, |_| {
            unreachable!("the synthetic table has no PPtrs")
        })
        .unwrap();
        let exposed_yaml = object_field(&exposed_document, "m_References");
        let UnityYamlNodeKind::Sequence { style, items } = &exposed_yaml.kind else {
            panic!("editor exposed references are a YAML sequence")
        };
        assert_eq!(*style, UnityYamlSequenceStyle::Block);
        assert_eq!(items.len(), 2);
        assert!(matches!(items[0].kind, UnityYamlNodeKind::Mapping { .. }));

        let mut ordinary_tree = exposed_tree.clone();
        ordinary_tree.nodes[0].type_name = Some("GameObject".to_owned());
        let ordinary_hints = UnityYamlObjectConversionHints::from_type_tree(
            &ordinary_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert!(!ordinary_hints
            .force_block_sequence_paths
            .contains("m_References"));
        assert_eq!(
            ordinary_hints
                .dictionary_representations
                .get("m_References"),
            Some(&UnityYamlDictionaryRepresentation::KeyedMapping)
        );
        let ordinary_value = SerializedValue::Struct(vec![field("m_References", entries)]);
        let ordinary_document = convert_with_object_hints(&ordinary_value, &ordinary_hints, |_| {
            unreachable!("the synthetic dictionary has no PPtrs")
        })
        .unwrap();
        let ordinary_yaml = object_field(&ordinary_document, "m_References");
        let UnityYamlNodeKind::Mapping { entries, .. } = &ordinary_yaml.kind else {
            panic!("ordinary string-key dictionaries remain YAML mappings")
        };
        assert_eq!(entries.len(), 2);

        let empty = SerializedValue::Struct(vec![field(
            "m_References",
            SerializedValue::Array(Vec::new()),
        )]);
        let empty_document = convert_with_object_hints(&empty, &exposed_hints, |_| {
            unreachable!("the empty table has no PPtrs")
        })
        .unwrap();
        assert_eq!(
            object_field(&empty_document, "m_References").kind,
            UnityYamlNodeKind::Sequence {
                style: UnityYamlSequenceStyle::Block,
                items: Vec::new(),
            }
        );
    }

    #[test]
    fn malformed_dictionary_pair_shapes_fail_with_context() {
        let type_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "map", "m_Map", 0),
                type_tree_node(2, "Array", "Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "pair", "data", 0),
                type_tree_node(4, "string", "first", 0),
                type_tree_node(4, "SInt32", "second", 0),
            ],
            string_buffer: Vec::new(),
        };
        let hints = UnityYamlObjectConversionHints::from_type_tree(
            &type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        let malformed = SerializedValue::Struct(vec![field(
            "m_Map",
            SerializedValue::Array(vec![SerializedValue::Struct(vec![field(
                "first",
                SerializedValue::StringBytes(b"key".to_vec()),
            )])]),
        )]);
        assert!(matches!(
            convert_with_object_hints(&malformed, &hints, |_| {
                unreachable!("the malformed dictionary has no PPtrs")
            }),
            Err(UnityYamlObjectConversionError::InvalidDictionaryPair {
                field_path,
                element_index: 0,
            }) if field_path == "m_Map"
        ));

        let malformed_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "map", "m_Map", 0),
                type_tree_node(2, "Array", "Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "pair", "data", 0),
                type_tree_node(4, "string", "first", 0),
            ],
            string_buffer: Vec::new(),
        };
        assert!(matches!(
            UnityYamlObjectConversionHints::from_type_tree(
                &malformed_tree,
                version(),
                UnityYamlObjectConversionLimits::default(),
            ),
            Err(UnityYamlObjectConversionError::InvalidTypeTree {
                reason: "map pair must contain ordered first and second fields",
                ..
            })
        ));
    }

    #[test]
    fn decoded_gameobject_fields_flatten_base_and_resolve_component_pptrs() {
        let root = SerializedValue::Struct(vec![
            field(
                "Base",
                SerializedValue::Struct(vec![
                    field("m_ObjectHideFlags", SerializedValue::U32(0)),
                    field(
                        "m_Source",
                        SerializedValue::PPtr {
                            file_id: 0,
                            path_id: 0,
                        },
                    ),
                ]),
            ),
            field("m_Layer", SerializedValue::I32(5)),
            field("m_Name", SerializedValue::StringBytes(b"Avatar".to_vec())),
            field(
                "m_Component",
                SerializedValue::Array(vec![SerializedValue::Struct(vec![field(
                    "component",
                    SerializedValue::PPtr {
                        file_id: 2,
                        path_id: 77,
                    },
                )])]),
            ),
        ]);
        let hints = HashMap::new();
        let mut observed = Vec::new();
        let document = convert(&root, &hints, |request| {
            observed.push((
                request.source.object_index,
                request.field_path.to_owned(),
                request.file_id,
                request.path_id,
            ));
            ProjectPointerResolution::Mapped(UnityMetaPointer {
                file_id: 2_345,
                guid: ExportPointerGuid::None,
                asset_type: UnityProjectAssetType::Serialized,
            })
        })
        .unwrap();
        let text = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();

        assert_eq!(
            observed,
            [(7, "m_Component[0].component".to_owned(), 2, 77)]
        );
        assert!(text.contains("serializedVersion: 6\n  m_HideFlags: 0\n  m_Source: {fileID: 0}\n"));
        assert!(text.contains("  m_Component:\n  - component: {fileID: 2345}\n"));
        assert!(text.contains("  m_Name: Avatar\n"));
    }

    #[test]
    fn serialized_version_hints_match_concrete_array_element_paths() {
        let mut hints = UnityYamlObjectConversionHints::default();
        hints
            .serialized_version_paths
            .insert("m_Lightmaps[*]".to_owned(), 2);
        let value = SerializedValue::Struct(vec![field(
            "m_Lightmaps",
            SerializedValue::Array(vec![
                SerializedValue::Struct(vec![field("m_Value", SerializedValue::I32(1))]),
                SerializedValue::Struct(vec![field("m_Value", SerializedValue::I32(2))]),
            ]),
        )]);
        let document = convert_with_object_hints(&value, &hints, |_| {
            unreachable!("the versioned array contains no PPtrs")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert_eq!(output.matches("serializedVersion: 2").count(), 2);
        assert!(output.contains("m_Value: 1"));
        assert!(output.contains("m_Value: 2"));
    }

    #[test]
    fn integer_arrays_use_baseline_reverse_hex_and_empty_arrays_require_schema_hints() {
        let integer_array = SerializedValue::Struct(vec![field(
            "m_Data",
            SerializedValue::Array(vec![
                SerializedValue::I32(0x0102_0304),
                SerializedValue::I32(2),
            ]),
        )]);
        let hints = HashMap::new();
        let document = convert(&integer_array, &hints, |_| {
            unreachable!("integer arrays contain no PPtrs")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(output.contains("m_Data: 0403020102000000"));

        let empty_array = SerializedValue::Struct(vec![field(
            "m_Children",
            SerializedValue::Array(Vec::new()),
        )]);
        assert!(matches!(
            convert(&empty_array, &HashMap::new(), |_| {
                unreachable!("empty arrays contain no PPtrs")
            }),
            Err(UnityYamlObjectConversionError::MissingEmptyArrayHint { field_path })
                if field_path == "m_Children"
        ));
        let hints = HashMap::from([(
            "m_Children".to_owned(),
            UnityYamlEmptyArrayKind::BlockSequence,
        )]);
        let document = convert(&empty_array, &hints, |_| {
            unreachable!("empty arrays contain no PPtrs")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(output.contains("m_Children: []"));
    }

    #[test]
    fn typed_data_lists_match_the_reference_runtime() {
        // Expected hex strings come from the isolated `AssetRipper.Yaml` probe in
        // `Temp/opencode/typedprobe` (recorded in docs/validation-log.md), which called
        // `YamlScalarNode.CreateHex` for every supported element width.
        let cases: Vec<(SerializedValue, &str)> = vec![
            (
                SerializedValue::Array(vec![
                    SerializedValue::U8(0x00),
                    SerializedValue::U8(0x12),
                    SerializedValue::U8(0xFF),
                ]),
                "0012ff",
            ),
            (
                SerializedValue::Array(vec![
                    SerializedValue::I8(-1),
                    SerializedValue::I8(0x7F),
                    SerializedValue::I8(-128),
                ]),
                "ff7f80",
            ),
            (
                SerializedValue::Array(vec![
                    SerializedValue::I16(0x1234),
                    SerializedValue::I16(-2),
                ]),
                "3412feff",
            ),
            (
                SerializedValue::Array(vec![
                    SerializedValue::U16(0x1234),
                    SerializedValue::U16(0xFFFF),
                ]),
                "3412ffff",
            ),
            (
                SerializedValue::Array(vec![
                    SerializedValue::Char(0x41),
                    SerializedValue::Char(0x56DE),
                ]),
                "4100de56",
            ),
            (
                SerializedValue::Array(vec![
                    SerializedValue::I32(0x0102_0304),
                    SerializedValue::I32(-1),
                ]),
                "04030201ffffffff",
            ),
            (
                SerializedValue::Array(vec![
                    SerializedValue::U32(0x0102_0304),
                    SerializedValue::U32(0xFFFF_FFFF),
                ]),
                "04030201ffffffff",
            ),
            (
                SerializedValue::Array(vec![
                    SerializedValue::I64(0x0102_0304_0506_0708),
                    SerializedValue::I64(-1),
                ]),
                "0807060504030201ffffffffffffffff",
            ),
            (
                SerializedValue::Array(vec![
                    SerializedValue::U64(0x0102_0304_0506_0708),
                    SerializedValue::U64(0xFFFF_FFFF_FFFF_FFFF),
                ]),
                "0807060504030201ffffffffffffffff",
            ),
            (
                SerializedValue::Array(vec![
                    SerializedValue::Bool(true),
                    SerializedValue::Bool(false),
                    SerializedValue::Bool(true),
                ]),
                "010001",
            ),
        ];

        for (value, expected) in cases {
            let structure = SerializedValue::Struct(vec![field("m_Data", value)]);
            let document = convert(&structure, &HashMap::new(), |_| {
                unreachable!("typed data lists contain no PPtrs")
            })
            .unwrap();
            let output = crate::export::unity_yaml::write_unity_yaml_documents(
                &[document],
                crate::export::unity_yaml::UnityYamlWriteLimits::default(),
            )
            .unwrap();
            assert!(
                output.contains(&format!("m_Data: {expected}")),
                "expected {expected} in {output}"
            );
        }
    }

    #[test]
    fn type_tree_hints_drive_flow_structs_and_empty_array_shapes() {
        let type_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "Color", "m_Color", TRANSFER_USING_FLOW_MAPPING_STYLE),
                type_tree_node(2, "float", "r", 0),
                type_tree_node(1, "vector", "m_Colors", 0),
                type_tree_node(2, "Array", "Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "Color", "data", TRANSFER_USING_FLOW_MAPPING_STYLE),
                type_tree_node(4, "float", "r", 0),
                type_tree_node(1, "vector", "m_Bits", 0),
                type_tree_node(2, "Array", "Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "UInt8", "data", 0),
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
            hints.empty_array_kinds.get("m_Colors"),
            Some(&UnityYamlEmptyArrayKind::BlockSequence)
        );
        assert_eq!(
            hints.empty_array_kinds.get("m_Bits"),
            Some(&UnityYamlEmptyArrayKind::IntegerHex)
        );
        assert!(hints.flow_mapped_struct_paths.contains("m_Color"));
        assert!(hints.flow_mapped_struct_paths.contains("m_Colors[*]"));

        let populated = SerializedValue::Struct(vec![
            field(
                "m_Color",
                SerializedValue::Struct(vec![field("r", SerializedValue::F32(0.25))]),
            ),
            field(
                "m_Colors",
                SerializedValue::Array(vec![SerializedValue::Struct(vec![field(
                    "r",
                    SerializedValue::F32(0.5),
                )])]),
            ),
        ]);
        let document = convert_with_object_hints(&populated, &hints, |_| {
            unreachable!("the synthetic color values contain no PPtrs")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(output.contains("m_Color: {r: 0.25}"));
        assert!(output.contains("m_Colors:\n  - {r: 0.5}"));

        let empty =
            SerializedValue::Struct(vec![field("m_Colors", SerializedValue::Array(Vec::new()))]);
        let document = convert_with_object_hints(&empty, &hints, |_| {
            unreachable!("the synthetic empty array contains no PPtrs")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(output.contains("m_Colors: []"));
    }

    #[test]
    fn type_tree_hints_apply_root_flow_mapping_and_reject_invalid_levels() {
        let type_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", TRANSFER_USING_FLOW_MAPPING_STYLE),
                type_tree_node(1, "string", "m_Name", 0),
            ],
            string_buffer: Vec::new(),
        };
        let hints = UnityYamlObjectConversionHints::from_type_tree(
            &type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert!(hints.flow_mapped_struct_paths.contains(""));
        let value = SerializedValue::Struct(vec![field(
            "m_Name",
            SerializedValue::StringBytes(b"Avatar".to_vec()),
        )]);
        let document = convert_with_object_hints(&value, &hints, |_| {
            unreachable!("the synthetic name contains no PPtrs")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(output.contains("GameObject: {serializedVersion: 6, m_Name: Avatar}"));

        let invalid = TypeTree {
            nodes: vec![
                type_tree_node(0, "Root", "Base", 0),
                type_tree_node(2, "int", "m_Value", 0),
            ],
            string_buffer: Vec::new(),
        };
        assert!(matches!(
            UnityYamlObjectConversionHints::from_type_tree(
                &invalid,
                version(),
                UnityYamlObjectConversionLimits::default()
            ),
            Err(UnityYamlObjectConversionError::InvalidTypeTree {
                node_index: 1,
                reason: "node level skips its parent"
            })
        ));
        assert!(matches!(
            UnityYamlObjectConversionHints::from_type_tree(
                &type_tree,
                version(),
                UnityYamlObjectConversionLimits {
                    max_type_tree_nodes: 1,
                    ..UnityYamlObjectConversionLimits::default()
                }
            ),
            Err(UnityYamlObjectConversionError::ValueLimitExceeded {
                requested: 2,
                limit: 1
            })
        ));
    }

    #[test]
    fn hash128_uses_baseline_versioned_yaml_forms_and_requires_all_bytes() {
        let value = hash128_value();
        let modern_tree = hash128_type_tree(1);
        let modern_hints = UnityYamlObjectConversionHints::from_type_tree(
            &modern_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert_eq!(
            modern_hints
                .hash128_schemas
                .get("m_Hash")
                .map(|schema| schema.serialized_version),
            Some(2)
        );
        let unity_5_hints = UnityYamlObjectConversionHints::from_type_tree(
            &modern_tree,
            SceneUnityVersion::parse("5.0.0f1").unwrap(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert_eq!(
            unity_5_hints
                .hash128_schemas
                .get("m_Hash")
                .map(|schema| schema.serialized_version),
            Some(2)
        );
        let document = convert_with_object_hints(&value, &modern_hints, |_| {
            unreachable!("Hash128 contains no PPtrs")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(output.contains("serializedVersion: 2"));
        assert!(output.contains("Hash: 03020100070605040b0a09080f0e0d0c"));
        assert!(!output.contains("m_Bytes__0"));

        let old_tree = hash128_type_tree(1);
        let old_hints = UnityYamlObjectConversionHints::from_type_tree(
            &old_tree,
            SceneUnityVersion::parse("4.7.0f1").unwrap(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert_eq!(
            old_hints
                .hash128_schemas
                .get("m_Hash")
                .map(|schema| schema.serialized_version),
            Some(1)
        );
        let document = convert_with_object_hints(&value, &old_hints, |_| {
            unreachable!("Hash128 contains no PPtrs")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(output.contains("serializedVersion: 1"));
        assert!(output.contains("bytes[0]: 0"));
        assert!(output.contains("bytes[15]: 15"));
        assert!(!output.contains("Hash: 03020100070605040b0a09080f0e0d0c"));

        let invalid = SerializedValue::Struct(vec![field(
            "m_Hash",
            SerializedValue::Struct(vec![field("m_Bytes__0", SerializedValue::I8(0))]),
        )]);
        assert!(matches!(
            convert_with_object_hints(&invalid, &modern_hints, |_| {
                unreachable!("Hash128 contains no PPtrs")
            }),
            Err(UnityYamlObjectConversionError::Hash128FieldCount {
                expected: 16,
                actual: 1,
                ..
            })
        ));
        let invalid_bytes = (0..16)
            .map(|index| {
                let name = if index < 10 {
                    format!("m_Bytes__{index}")
                } else {
                    format!("m_Bytes_{index}")
                };
                let value = if index == 7 {
                    SerializedValue::I8(index as i8)
                } else {
                    SerializedValue::U8(index as u8)
                };
                field(&name, value)
            })
            .collect();
        let invalid = SerializedValue::Struct(vec![field(
            "m_Hash",
            SerializedValue::Struct(invalid_bytes),
        )]);
        assert!(matches!(
            convert_with_object_hints(&invalid, &modern_hints, |_| {
                unreachable!("Hash128 contains no PPtrs")
            }),
            Err(UnityYamlObjectConversionError::InvalidHash128Byte { field_index: 7, .. })
        ));
    }

    #[test]
    fn guid_structs_emit_unity_guid_strings_and_validate_the_four_words() {
        let type_tree = guid_type_tree(false);
        let hints = UnityYamlObjectConversionHints::from_type_tree(
            &type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert_eq!(
            hints
                .guid_schemas
                .get("m_Guid")
                .map(|schema| schema.word_field_names.as_slice()),
            Some(
                [
                    "m_Data_0_".to_owned(),
                    "m_Data_1_".to_owned(),
                    "m_Data_2_".to_owned(),
                    "m_Data_3_".to_owned(),
                ]
                .as_slice()
            )
        );
        let document = convert_with_object_hints(&guid_value(false), &hints, |_| {
            unreachable!("GUID contains no PPtrs")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(output.contains("m_Guid: 40302010413121114232221243332313"));

        let missing_word = SerializedValue::Struct(vec![field(
            "m_Guid",
            SerializedValue::Struct(vec![
                field("m_Data_0_", SerializedValue::U32(1)),
                field("m_Data_1_", SerializedValue::U32(2)),
                field("m_Data_2_", SerializedValue::U32(3)),
            ]),
        )]);
        assert!(matches!(
            convert_with_object_hints(&missing_word, &hints, |_| {
                unreachable!("GUID contains no PPtrs")
            }),
            Err(UnityYamlObjectConversionError::GuidFieldCount {
                expected: 4,
                actual: 3,
                ..
            })
        ));

        let wrong_word = SerializedValue::Struct(vec![field(
            "m_Guid",
            SerializedValue::Struct(vec![
                field("m_Data_0_", SerializedValue::U32(1)),
                field("m_Data_1_", SerializedValue::U32(2)),
                field("m_Data_2_", SerializedValue::U64(3)),
                field("m_Data_3_", SerializedValue::U32(4)),
            ]),
        )]);
        assert!(matches!(
            convert_with_object_hints(&wrong_word, &hints, |_| {
                unreachable!("GUID contains no PPtrs")
            }),
            Err(UnityYamlObjectConversionError::InvalidGuidWord { word_index: 2, .. })
        ));
    }

    #[test]
    fn guid_type_tree_indexed_data_names_emit_unity_guid_strings() {
        let mut nodes = vec![
            type_tree_node(0, "OcclusionCullingSettings", "Base", 0),
            type_tree_node(1, "GUID", "m_SceneGUID", 0),
        ];
        let words = [0x0102_0304, 0x1112_1314, 0x2122_2324, 0x3132_3334];
        let mut fields = Vec::new();
        for (index, word) in words.into_iter().enumerate() {
            let name = format!("data[{index}]");
            nodes.push(type_tree_node(2, "unsigned int", &name, 4));
            fields.push(field(&name, SerializedValue::U32(word)));
        }
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
        assert_eq!(
            hints
                .guid_schemas
                .get("m_SceneGUID")
                .map(|schema| schema.word_field_names.as_slice()),
            Some(
                [
                    "data[0]".to_owned(),
                    "data[1]".to_owned(),
                    "data[2]".to_owned(),
                    "data[3]".to_owned(),
                ]
                .as_slice()
            )
        );
        let value =
            SerializedValue::Struct(vec![field("m_SceneGUID", SerializedValue::Struct(fields))]);
        let document =
            convert_with_object_hints(&value, &hints, |_| unreachable!("GUID contains no PPtrs"))
                .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(output.contains("m_SceneGUID: 40302010413121114232221243332313"));
    }

    #[test]
    fn property_name_types_emit_integer_and_utf8_ids_as_yaml_strings() {
        let type_tree = property_name_type_tree();
        let hints = UnityYamlObjectConversionHints::from_type_tree(
            &type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert_eq!(
            hints.property_name_schemas.get("m_IntegerId"),
            Some(&UnityYamlPropertyNameSchema {
                value_field_name: "Id_Int32".to_owned(),
                value_kind: UnityYamlPropertyNameValueKind::Int32,
            })
        );
        assert_eq!(
            hints.property_name_schemas.get("m_StringId"),
            Some(&UnityYamlPropertyNameSchema {
                value_field_name: "Id_Utf8String".to_owned(),
                value_kind: UnityYamlPropertyNameValueKind::Utf8String,
            })
        );

        let value = SerializedValue::Struct(vec![
            field(
                "m_IntegerId",
                SerializedValue::Struct(vec![field("Id_Int32", SerializedValue::I32(-42))]),
            ),
            field(
                "m_StringId",
                SerializedValue::Struct(vec![field(
                    "Id_Utf8String",
                    SerializedValue::StringBytes(b"Track/Object.Name".to_vec()),
                )]),
            ),
        ]);
        let document = convert_with_object_hints(&value, &hints, |_| {
            unreachable!("PropertyName contains no PPtrs")
        })
        .unwrap();
        assert_eq!(
            &object_field(&document, "m_IntegerId").kind,
            &UnityYamlNodeKind::Scalar(UnityYamlScalar::String("-42".to_owned()))
        );
        assert_eq!(
            &object_field(&document, "m_StringId").kind,
            &UnityYamlNodeKind::Scalar(UnityYamlScalar::String("Track/Object.Name".to_owned()))
        );

        let invalid_value = SerializedValue::Struct(vec![field(
            "m_IntegerId",
            SerializedValue::Struct(vec![field("Id_Int32", SerializedValue::U32(42))]),
        )]);
        assert!(matches!(
            convert_with_object_hints(&invalid_value, &hints, |_| {
                unreachable!("PropertyName contains no PPtrs")
            }),
            Err(UnityYamlObjectConversionError::InvalidPropertyNameValue {
                field_path,
                field_name,
            }) if field_path == "m_IntegerId" && field_name == "Id_Int32"
        ));
    }

    #[test]
    fn property_name_array_hints_match_wildcard_elements() {
        let type_tree = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "vector", "m_Ids", 0),
                type_tree_node(2, "Array", "Array", 0),
                type_tree_node(3, "SInt32", "size", 0),
                type_tree_node(3, "string", "data", 0),
                type_tree_node(4, "SInt32", "Id_Int32", 0),
            ],
            string_buffer: Vec::new(),
        };
        let hints = UnityYamlObjectConversionHints::from_type_tree(
            &type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert!(hints.property_name_schemas.contains_key("m_Ids[*]"));
        let value = SerializedValue::Struct(vec![field(
            "m_Ids",
            SerializedValue::Array(vec![SerializedValue::Struct(vec![field(
                "Id_Int32",
                SerializedValue::I32(73),
            )])]),
        )]);
        let document = convert_with_object_hints(&value, &hints, |_| {
            unreachable!("PropertyName array contains no PPtrs")
        })
        .unwrap();
        let array = object_field(&document, "m_Ids");
        let UnityYamlNodeKind::Sequence { items, .. } = &array.kind else {
            panic!("PropertyName arrays export as YAML sequences")
        };
        assert_eq!(
            items[0].kind,
            UnityYamlNodeKind::Scalar(UnityYamlScalar::String("73".to_owned()))
        );
    }

    #[test]
    fn scene_object_identifier_uses_project_yaml_target_field_names() {
        let type_tree = scene_object_identifier_type_tree(false);
        let hints = UnityYamlObjectConversionHints::from_type_tree(
            &type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert_eq!(
            hints.scene_object_identifier_schemas.get("m_Identifier"),
            Some(&UnityYamlSceneObjectIdentifierSchema {
                target_object_field_name: "TargetObject".to_owned(),
                target_prefab_field_name: "TargetPrefab".to_owned(),
            })
        );

        let document =
            convert_with_object_hints(&scene_object_identifier_value(123, -456), &hints, |_| {
                unreachable!("SceneObjectIdentifier contains no PPtrs")
            })
            .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(
            output.contains("m_Identifier:\n    targetObject: 123\n    targetPrefab: -456"),
            "{output:?}"
        );
        assert!(!output.contains("TargetObject"));
        assert!(!output.contains("TargetPrefab"));
    }

    #[test]
    fn scene_object_identifier_array_hints_match_wildcard_elements() {
        let type_tree = scene_object_identifier_type_tree(true);
        let hints = UnityYamlObjectConversionHints::from_type_tree(
            &type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert!(hints
            .scene_object_identifier_schemas
            .contains_key("m_Identifiers[*]"));

        let value = SerializedValue::Struct(vec![field(
            "m_Identifiers",
            SerializedValue::Array(vec![SerializedValue::Struct(vec![
                field("TargetObject", SerializedValue::I64(73)),
                field("TargetPrefab", SerializedValue::I64(84)),
            ])]),
        )]);
        let document = convert_with_object_hints(&value, &hints, |_| {
            unreachable!("SceneObjectIdentifier contains no PPtrs")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(output.contains("m_Identifiers:\n  - targetObject: 73\n    targetPrefab: 84"));
    }

    #[test]
    fn malformed_scene_object_identifier_schema_and_value_are_rejected() {
        let malformed = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "SceneObjectIdentifier", "m_Identifier", 0),
                type_tree_node(2, "SInt32", "TargetObject", 0),
                type_tree_node(2, "SInt64", "TargetPrefab", 0),
            ],
            string_buffer: Vec::new(),
        };
        assert!(matches!(
            UnityYamlObjectConversionHints::from_type_tree(
                &malformed,
                version(),
                UnityYamlObjectConversionLimits::default()
            ),
            Err(UnityYamlObjectConversionError::InvalidTypeTree {
                reason: "SceneObjectIdentifier TargetObject is not a signed 64-bit field",
                ..
            })
        ));

        let type_tree = scene_object_identifier_type_tree(false);
        let hints = UnityYamlObjectConversionHints::from_type_tree(
            &type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        let invalid_value = SerializedValue::Struct(vec![field(
            "m_Identifier",
            SerializedValue::Struct(vec![
                field("TargetObject", SerializedValue::U64(123)),
                field("TargetPrefab", SerializedValue::I64(456)),
            ]),
        )]);
        assert!(matches!(
            convert_with_object_hints(&invalid_value, &hints, |_| {
                unreachable!("SceneObjectIdentifier contains no PPtrs")
            }),
            Err(UnityYamlObjectConversionError::InvalidSceneObjectIdentifierValue {
                field_path,
                field_name,
            }) if field_path == "m_Identifier" && field_name == "TargetObject"
        ));
    }

    #[test]
    fn malformed_property_name_type_tree_is_rejected() {
        let malformed = TypeTree {
            nodes: vec![
                type_tree_node(0, "GameObject", "Base", 0),
                type_tree_node(1, "PropertyName", "m_Id", 0),
                type_tree_node(2, "float", "value", 0),
            ],
            string_buffer: Vec::new(),
        };
        assert!(matches!(
            UnityYamlObjectConversionHints::from_type_tree(
                &malformed,
                version(),
                UnityYamlObjectConversionLimits::default()
            ),
            Err(UnityYamlObjectConversionError::InvalidTypeTree {
                reason: "PropertyName value must be an Int32 or serialized UTF-8 string",
                ..
            })
        ));
    }

    #[test]
    fn guid_type_tree_hints_resolve_each_array_element_by_schema_path() {
        let type_tree = guid_type_tree(true);
        let hints = UnityYamlObjectConversionHints::from_type_tree(
            &type_tree,
            version(),
            UnityYamlObjectConversionLimits::default(),
        )
        .unwrap();
        assert!(hints.guid_schemas.contains_key("m_Guids[*]"));
        let document = convert_with_object_hints(&guid_value(true), &hints, |_| {
            unreachable!("GUID array contains no PPtrs")
        })
        .unwrap();
        let output = crate::export::unity_yaml::write_unity_yaml_documents(
            &[document],
            crate::export::unity_yaml::UnityYamlWriteLimits::default(),
        )
        .unwrap();
        assert!(output.contains("m_Guids:\n  - 40302010413121114232221243332313"));
    }

    #[test]
    fn unresolved_pptrs_and_raw_or_invalid_utf8_values_are_explicit_errors() {
        let pptr = SerializedValue::Struct(vec![field(
            "m_Target",
            SerializedValue::PPtr {
                file_id: 2,
                path_id: 88,
            },
        )]);
        assert!(matches!(
            convert(&pptr, &HashMap::new(), |request| {
                ProjectPointerResolution::UnresolvedInput(UnresolvedProjectPointer {
                    source_file_index: 2,
                    source_object_index: request.source.object_index,
                    source_path_id: request.source.path_id,
                    field_path: request.field_path.to_owned(),
                    serialized_file_id: request.file_id,
                    serialized_path_id: request.path_id,
                    expected_class_id: Some(21),
                    resolution: crate::serialized_file::reference_graph::MultiFilePPtrResolution::MissingDependency {
                        external_index: 1,
                        path_id: request.path_id,
                    },
                })
            }),
            Err(UnityYamlObjectConversionError::PointerConversion { field_path, .. })
                if field_path == "m_Target"
        ));

        for (value, expected_path) in [
            (
                SerializedValue::Raw {
                    type_name: "UnknownLeaf".to_owned(),
                    bytes: vec![1, 2],
                },
                "m_Raw",
            ),
            (SerializedValue::StringBytes(vec![0xff]), "m_Text"),
        ] {
            let name = if expected_path == "m_Raw" {
                "m_Raw"
            } else {
                "m_Text"
            };
            let root = SerializedValue::Struct(vec![field(name, value)]);
            let error = convert(&root, &HashMap::new(), |_| {
                unreachable!("the value has no PPtrs")
            })
            .unwrap_err();
            match error {
                UnityYamlObjectConversionError::UnsupportedRawValue { field_path, .. }
                | UnityYamlObjectConversionError::InvalidUtf8String { field_path } => {
                    assert_eq!(field_path, expected_path)
                }
                other => panic!("unexpected error: {other}"),
            }
        }
    }
