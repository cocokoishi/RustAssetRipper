    #[test]
    fn object_workspace_recovers_source_generated_textasset_without_usable_file_typetree() {
        let fixture = crate::processing::asset_factory::PROBE_FIXTURE;
        let hex = fixture
            .lines()
            .find_map(|line| line.strip_prefix("SF_HEX|textasset_notree|"))
            .expect("the source-factory fixture includes the no-TypeTree TextAsset");
        assert!(hex.len().is_multiple_of(2), "fixture hex has even length");
        let bytes = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let digits = std::str::from_utf8(pair).expect("fixture hex is ASCII");
                u8::from_str_radix(digits, 16).expect("fixture hex is valid")
            })
            .collect::<Vec<_>>();
        let parsed = SerializedFile::parse(&bytes).expect("source fixture parses");
        assert_eq!(
            parsed.metadata.prefix.unity_version_text.as_deref(),
            Some("2019.4.40f1")
        );
        assert_eq!(parsed.metadata.prefix.target_platform, Some(19));
        let serialized_type_index = parsed.metadata.objects[0]
            .resolved_type_index
            .expect("TextAsset resolves its serialized type");
        assert!(
            parsed.metadata.types[serialized_type_index]
                .type_tree
                .as_ref()
                .is_none_or(|tree| tree.nodes.is_empty()),
            "the object's serialized type must have no usable TypeTree"
        );
        assert!(matches!(
            parsed.decode_object_payload(0).unwrap_err().kind(),
            crate::serialized_file::file::SerializedFileErrorKind::Object(object_error)
                if matches!(
                    object_error.kind(),
                    crate::serialized_file::object::ObjectDecodeErrorKind::MissingTypeTree { .. }
                        | crate::serialized_file::object::ObjectDecodeErrorKind::EmptyTypeTree { .. }
                )
        ));

        let filesystem = virtual_file_system("/game/textasset_notree.assets", &bytes);
        let workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &[input("/game/textasset_notree.assets")],
            SerializedFileWorkspaceLimits::default(),
        )
        .expect("workspace loads the reference fixture");
        assert!(workspace.files()[0].reference_graph.is_complete());
        assert!(workspace.files()[0]
            .reference_graph
            .object_scan_is_complete(0));
        assert!(workspace.files()[0].reference_graph.references().is_empty());
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .expect("the factory fallback materializes the generated class");

        assert!(objects.report().is_complete(), "{:?}", objects.report());
        assert_eq!(objects.report().decoded_objects, 1);
        let payload = &objects.objects()[0]
            .payload
            .as_ref()
            .expect("decoded payload");
        assert_eq!(payload.path_id, 42);
        assert_eq!(payload.type_name, "TextAsset");
        assert_eq!(
            payload
                .root
                .unique_field_in_base_chain("m_Name")
                .expect("unique name field")
                .expect("name field exists"),
            &SerializedValue::StringBytes(b"Hero".to_vec())
        );
        assert_eq!(
            payload
                .root
                .unique_field_in_base_chain("m_Script")
                .expect("unique script field")
                .expect("script field exists"),
            &SerializedValue::StringBytes(b"print(1)".to_vec())
        );
        assert_eq!(
            filesystem
                .read_all_bytes("/game/textasset_notree.assets", usize::MAX)
                .unwrap(),
            bytes,
            "recovery must leave source bytes unchanged"
        );
    }

    #[test]
    fn object_workspace_uses_generated_class_at_the_alpha_tpk_boundary() {
        let fixture = crate::processing::asset_factory::PROBE_FIXTURE;
        let hex = fixture
            .lines()
            .find_map(|line| line.strip_prefix("SF_HEX|gameobject_4_alpha_notree|"))
            .expect("the source fixture includes the 4.0.0a0 GameObject");
        assert!(hex.len().is_multiple_of(2), "fixture hex has even length");
        let bytes = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let digits = std::str::from_utf8(pair).expect("fixture hex is ASCII");
                u8::from_str_radix(digits, 16).expect("fixture hex is valid")
            })
            .collect::<Vec<_>>();

        let parsed = SerializedFile::parse(&bytes).expect("source fixture parses");
        assert_eq!(
            parsed.metadata.prefix.unity_version_text.as_deref(),
            Some("4.0.0a0")
        );
        assert!(parsed.metadata.types[0]
            .type_tree
            .as_ref()
            .is_none_or(|tree| tree.nodes.is_empty()));

        let filesystem = virtual_file_system("/game/gameobject_4_alpha_notree.assets", &bytes);
        let workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &[input("/game/gameobject_4_alpha_notree.assets")],
            SerializedFileWorkspaceLimits::default(),
        )
        .expect("workspace resolves the source-generated object");
        assert!(workspace.files()[0].reference_graph.is_complete());
        assert!(workspace.files()[0]
            .reference_graph
            .object_scan_is_complete(0));

        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .expect("ObjectWorkspace uses the per-class factory boundary");
        assert!(objects.report().is_complete(), "{:?}", objects.report());
        assert_eq!(objects.report().decoded_objects, 1);
        assert_eq!(
            objects.objects()[0].factory.kind,
            crate::processing::asset_factory::FactoryAssetKind::Generated(
                "GameObject_4".to_owned()
            )
        );
        let payload = objects.objects()[0]
            .payload
            .as_ref()
            .expect("generated GameObject is decoded");
        assert_eq!(payload.type_name, "GameObject");
        assert_eq!(payload.consumed_bytes, 15);
        assert!(payload.trailing_bytes.is_empty());
    }

    #[test]
    fn object_workspace_recovers_editor_tpk_tree_object_without_file_typetree() {
        let fixture = crate::processing::asset_factory::PROBE_FIXTURE;
        let hex = fixture
            .lines()
            .find_map(|line| line.strip_prefix("SF_HEX|tpk_navmesh_editor|"))
            .expect("the source factory fixture includes the editor TPK object");
        assert!(hex.len().is_multiple_of(2), "fixture hex has even length");
        let bytes = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let digits = std::str::from_utf8(pair).expect("fixture hex is ASCII");
                u8::from_str_radix(digits, 16).expect("fixture hex is valid")
            })
            .collect::<Vec<_>>();
        let parsed = SerializedFile::parse(&bytes).expect("source fixture parses");
        assert_eq!(
            parsed.metadata.prefix.unity_version_text.as_deref(),
            Some("2019.4.40f1")
        );
        assert_eq!(parsed.metadata.prefix.target_platform, Some(0xFFFF_FFFEu32));
        assert!(
            !parsed
                .object_read_flags("/game/navmesh.unity", "navmesh.unity")
                .is_release(),
            "NoTarget .unity files use editor serialization flags"
        );
        let serialized_type_index = parsed.metadata.objects[0]
            .resolved_type_index
            .expect("the TPK class resolves its serialized type");
        assert!(
            parsed.metadata.types[serialized_type_index]
                .type_tree
                .as_ref()
                .is_none_or(|tree| tree.nodes.is_empty()),
            "the object's serialized type must have no usable TypeTree"
        );

        let filesystem = virtual_file_system("/game/navmesh.unity", &bytes);
        let workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &[input("/game/navmesh.unity")],
            SerializedFileWorkspaceLimits::default(),
        )
        .expect("workspace loads the reference fixture");
        assert!(workspace.files()[0].reference_graph.is_complete());
        assert!(workspace.files()[0]
            .reference_graph
            .object_scan_is_complete(0));

        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .expect("the source TPK fallback materializes the TypeTreeObject");
        assert!(objects.report().is_complete(), "{:?}", objects.report());
        assert_eq!(objects.report().decoded_objects, 1);
        let object = &objects.objects()[0];
        let tree = object
            .recovered_type_tree
            .as_ref()
            .expect("the source TPK editor tree is retained");
        assert!(tree
            .nodes
            .iter()
            .any(|node| { node.name.as_deref() == Some("m_ObjectHideFlags") }));
        let payload = object.payload.as_ref().expect("decoded payload");
        assert_eq!(payload.path_id, 42);
        assert_eq!(payload.type_name, "NavMeshObsolete");
        assert_eq!(
            payload.root.unique_field("m_ObjectHideFlags").unwrap(),
            Some(&SerializedValue::U32(3))
        );
        assert_eq!(
            payload.root.unique_field("m_PrefabParentObject").unwrap(),
            Some(&SerializedValue::PPtr {
                file_id: 0,
                path_id: 0,
            })
        );
        assert_eq!(
            payload.root.unique_field("m_PrefabInternal").unwrap(),
            Some(&SerializedValue::PPtr {
                file_id: 0,
                path_id: 0,
            })
        );
        assert_eq!(
            payload.root.unique_field("m_Name").unwrap(),
            Some(&SerializedValue::StringBytes(b"SceneNavMesh".to_vec()))
        );
        assert!(payload.trailing_bytes.is_empty());
        assert_eq!(
            filesystem
                .read_all_bytes("/game/navmesh.unity", usize::MAX)
                .unwrap(),
            bytes,
            "recovery must leave source bytes unchanged"
        );
    }

    #[test]
    fn object_workspace_recovers_source_generated_monoscript_without_usable_file_typetree() {
        let fixture = crate::processing::asset_factory::PROBE_FIXTURE;
        let hex = fixture
            .lines()
            .find_map(|line| line.strip_prefix("SF_HEX|monoscript_notree|"))
            .expect("the source-factory fixture includes the no-usable-TypeTree MonoScript");
        assert!(hex.len().is_multiple_of(2), "fixture hex has even length");
        let bytes = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let digits = std::str::from_utf8(pair).expect("fixture hex is ASCII");
                u8::from_str_radix(digits, 16).expect("fixture hex is valid")
            })
            .collect::<Vec<_>>();
        let parsed = SerializedFile::parse(&bytes).expect("source fixture parses");
        assert_eq!(
            parsed.metadata.prefix.unity_version_text.as_deref(),
            Some("2019.4.40f1")
        );
        assert_eq!(parsed.metadata.prefix.target_platform, Some(19));
        let serialized_type_index = parsed.metadata.objects[0]
            .resolved_type_index
            .expect("MonoScript resolves its serialized type");
        assert!(
            parsed.metadata.types[serialized_type_index]
                .type_tree
                .as_ref()
                .is_none_or(|tree| tree.nodes.is_empty()),
            "the object's serialized type must have no usable TypeTree"
        );

        let filesystem = virtual_file_system("/game/monoscript_notree.assets", &bytes);
        let workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &[input("/game/monoscript_notree.assets")],
            SerializedFileWorkspaceLimits::default(),
        )
        .expect("workspace loads the reference fixture");
        assert!(workspace.files()[0].reference_graph.is_complete());
        assert!(workspace.files()[0]
            .reference_graph
            .object_scan_is_complete(0));
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .expect("the factory fallback materializes the generated class");

        assert!(objects.report().is_complete(), "{:?}", objects.report());
        assert_eq!(objects.report().decoded_objects, 1);
        let payload = &objects.objects()[0]
            .payload
            .as_ref()
            .expect("decoded payload");
        assert_eq!(payload.path_id, 42);
        assert_eq!(payload.type_name, "MonoScript");
        for (field_name, expected) in [
            ("m_Name", &b"HeroController"[..]),
            ("m_ClassName", &b"HeroController"[..]),
            ("m_Namespace", &b"VRC.SDK"[..]),
            ("m_AssemblyName", &b"Assembly-CSharp"[..]),
        ] {
            assert_eq!(
                payload
                    .root
                    .unique_field_in_base_chain(field_name)
                    .expect("unique field")
                    .expect("field exists"),
                &SerializedValue::StringBytes(expected.to_vec()),
                "{field_name}"
            );
        }
        assert_eq!(
            payload
                .root
                .unique_field_in_base_chain("m_ExecutionOrder")
                .expect("unique execution order")
                .expect("execution order exists"),
            &SerializedValue::I32(200)
        );
        assert_eq!(
            filesystem
                .read_all_bytes("/game/monoscript_notree.assets", usize::MAX)
                .unwrap(),
            bytes,
            "recovery must leave source bytes unchanged"
        );
    }

    #[test]
    fn object_workspace_resolves_unloaded_monobehaviour_script_to_local_monoscript() {
        let fixture = crate::processing::asset_factory::PROBE_FIXTURE;
        let hex = fixture
            .lines()
            .find_map(|line| line.strip_prefix("SF_HEX|mono_unloaded|"))
            .expect("the source fixture includes a MonoBehaviour with a local MonoScript");
        assert!(hex.len().is_multiple_of(2), "fixture hex has even length");
        let bytes = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let digits = std::str::from_utf8(pair).expect("fixture hex is ASCII");
                u8::from_str_radix(digits, 16).expect("fixture hex is valid")
            })
            .collect::<Vec<_>>();

        let filesystem = virtual_file_system("/game/mono_unloaded.assets", &bytes);
        let workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &[input("/game/mono_unloaded.assets")],
            SerializedFileWorkspaceLimits::default(),
        )
        .expect("workspace loads the source-authored two-object file");
        assert_eq!(workspace.files().len(), 1);
        assert_eq!(workspace.files()[0].metadata.objects.len(), 2);
        assert_eq!(workspace.files()[0].metadata.objects[0].path_id, 42);
        assert_eq!(workspace.files()[0].metadata.objects[1].path_id, 7);
        assert!(
            !workspace.files()[0]
                .reference_graph
                .object_scan_is_complete(0),
            "the script-specific remainder remains unloaded"
        );
        assert!(workspace.files()[0]
            .reference_graph
            .object_scan_is_complete(1));

        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .expect("factory materializes both the MonoBehaviour header and MonoScript");
        assert_eq!(objects.objects().len(), 2);
        let behaviour = &objects.objects()[0];
        assert_eq!(behaviour.path_id, 42);
        assert_eq!(behaviour.factory.class_id, 114);
        assert_eq!(behaviour.factory.name, "Behaviour");
        assert!(matches!(
            behaviour.factory.structure.as_ref().map(|structure| structure.outcome),
            Some(crate::processing::asset_factory::MonoBehaviourStructureOutcome::UseUnloadedRemainder)
        ));
        assert!(!behaviour.is_fully_materialized());
        let behaviour_payload = behaviour
            .payload
            .as_ref()
            .expect("the generated MonoBehaviour header remains available");
        assert_eq!(
            behaviour_payload
                .root
                .unique_field("m_Script")
                .expect("unique script pointer field")
                .expect("script pointer field exists"),
            &SerializedValue::PPtr {
                file_id: 0,
                path_id: 7,
            }
        );

        let mono_script = &objects.objects()[1];
        assert_eq!(mono_script.path_id, 7);
        assert_eq!(mono_script.factory.class_id, 115);
        assert_eq!(mono_script.factory.name, "HeroController");
        let mono_script_payload = &mono_script
            .payload
            .as_ref()
            .expect("the generated MonoScript fields remain available")
            .root;
        assert_eq!(
            mono_script_payload
                .unique_field("m_ClassName")
                .expect("unique class-name field")
                .expect("class-name field exists"),
            &SerializedValue::StringBytes(b"HeroController".to_vec())
        );
        assert_eq!(
            mono_script_payload
                .unique_field("m_Namespace")
                .expect("unique namespace field")
                .expect("namespace field exists"),
            &SerializedValue::StringBytes(b"VRC.SDK".to_vec())
        );
        assert_eq!(
            mono_script_payload
                .unique_field("m_AssemblyName")
                .expect("unique assembly-name field")
                .expect("assembly-name field exists"),
            &SerializedValue::StringBytes(b"Assembly-CSharp".to_vec())
        );
        assert_eq!(
            mono_script_payload
                .unique_field("m_ExecutionOrder")
                .expect("unique execution-order field")
                .expect("execution-order field exists"),
            &SerializedValue::I32(200)
        );
        assert!(matches!(
            &mono_script.factory.kind,
            crate::processing::asset_factory::FactoryAssetKind::Generated(_)
        ));

        let graph = workspace
            .reference_graph(
                crate::serialized_file::reference_graph::MultiFileReferenceGraphLimits::default(),
            )
            .expect("the cross-file reference graph joins the local objects");
        let references: Vec<_> = graph.references_from_object(0, 0).collect();
        assert_eq!(references.len(), 2, "{references:?}");
        let script_reference = references
            .iter()
            .find(|reference| reference.field_path == "m_Script")
            .expect("the generated MonoBehaviour header exposes m_Script");
        assert_eq!(script_reference.file_id, 0);
        assert_eq!(script_reference.path_id, 7);
        assert_eq!(
            script_reference.resolution,
            crate::serialized_file::reference_graph::MultiFilePPtrResolution::Object {
                target_file_index: 0,
                target_object_index: 1,
            }
        );
        let game_object_reference = references
            .iter()
            .find(|reference| reference.field_path == "m_GameObject")
            .expect("the generated MonoBehaviour header exposes m_GameObject");
        assert_eq!(
            game_object_reference.resolution,
            crate::serialized_file::reference_graph::MultiFilePPtrResolution::Null
        );
        assert_eq!(graph.object_class_id(0, 1), Some(115));
        assert!(
            !graph.is_complete(),
            "the unloaded script remainder stays explicit"
        );
        assert!(workspace.files()[0]
            .reference_graph
            .issues()
            .iter()
            .any(|issue| {
                matches!(
                    issue,
                    crate::serialized_file::reference_graph::ReferenceGraphIssue::UnparsedTrailingPayload {
                        object_index: 0,
                        path_id: 42,
                        byte_count: 4,
                    }
                )
            }));
        assert_eq!(
            filesystem
                .read_all_bytes("/game/mono_unloaded.assets", usize::MAX)
                .expect("source fixture remains mounted"),
            bytes,
            "factory and graph integration preserve input bytes"
        );
    }

    #[test]
    fn object_workspace_recovers_assembly_backed_unloaded_monobehaviour_structure() {
        let fixture = crate::processing::asset_factory::PROBE_FIXTURE;
        let hex = fixture
            .lines()
            .find_map(|line| line.strip_prefix("SF_HEX|mono_unloaded_valid|"))
            .expect("the source fixture includes a valid assembly-backed structure remainder");
        assert!(hex.len().is_multiple_of(2), "fixture hex has even length");
        let bytes = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let digits = std::str::from_utf8(pair).expect("fixture hex is ASCII");
                u8::from_str_radix(digits, 16).expect("fixture hex is valid")
            })
            .collect::<Vec<_>>();

        let mut filesystem = virtual_file_system("/game/mono_unloaded_valid.assets", &bytes);
        insert_file(
            &mut filesystem,
            "/game/Assembly-CSharp.dll",
            include_bytes!("../processing/managed_assembly_fixture.dll"),
        );
        let mut workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &[input("/game/mono_unloaded_valid.assets")],
            SerializedFileWorkspaceLimits::default(),
        )
        .expect("workspace loads the source-authored two-object file");
        let mut objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .expect("factory materializes the generated MonoBehaviour header and MonoScript");
        let mut catalog = ManagedAssemblyCatalog::default();
        catalog
            .insert(
                "Assembly-CSharp.dll",
                include_bytes!("../processing/managed_assembly_fixture.dll"),
                ManagedAssemblyLimits::default(),
            )
            .expect("Rust reads the synthetic managed assembly metadata");

        let recovery = objects
            .recover_mono_behaviour_structures(
                &filesystem,
                &mut workspace,
                &catalog,
                ObjectWorkspaceLimits::default(),
                ManagedSchemaLimits::default(),
            )
            .expect("assembly-backed field bytes decode through ObjectWorkspace");
        assert_eq!(recovery.behaviours.len(), 1);
        assert!(
            matches!(
                &recovery.behaviours[0].outcome,
                ManagedBehaviourRecoveryOutcome::Recovered {
                    script_version,
                    decoded_bytes: 16,
                    skipped_fields,
                } if script_version == "2019.4.40f1"
                    && skipped_fields.iter().any(|field| field.contains("ignored"))
                    && skipped_fields.iter().any(|field| field.contains("staticValue"))
                    && skipped_fields.iter().any(|field| field.contains("locked"))
            ),
            "unexpected assembly recovery outcome: {:?}",
            recovery.behaviours
        );
        assert_eq!(recovery.behaviours[0].path_id, 42);
        assert_eq!(
            recovery.behaviours[0]
                .script
                .as_ref()
                .map(|script| script.unique_name()),
            Some("[Assembly-CSharp]Game.HeroController".to_owned())
        );

        let behaviour = objects
            .get(0, 0)
            .expect("recovered MonoBehaviour remains in its stable workspace slot");
        assert!(behaviour.is_fully_materialized());
        assert_eq!(
            behaviour
                .factory
                .structure
                .as_ref()
                .map(|structure| structure.outcome),
            Some(
                crate::processing::asset_factory::MonoBehaviourStructureOutcome::UseParsedStructure
            )
        );
        let payload = behaviour
            .payload
            .as_ref()
            .expect("recovered payload remains available");
        let structure = payload
            .root
            .unique_field("m_Structure")
            .expect("MonoBehaviour root lookup succeeds")
            .expect("recovered script fields are attached to m_Structure");
        let SerializedValue::Struct(fields) = structure else {
            panic!("m_Structure is represented as a structure");
        };
        let field_value = |name: &str| {
            fields
                .iter()
                .find(|field| field.name == name)
                .map(|field| &field.value)
                .unwrap_or_else(|| panic!("recovered structure field {name:?} exists"))
        };
        assert_eq!(
            field_value("inherited"),
            &SerializedValue::I64(0x0102_0304_0506_0708)
        );
        assert_eq!(field_value("health"), &SerializedValue::I32(1234));
        assert_eq!(field_value("speed"), &SerializedValue::F32(1.5));
        assert!(!fields
            .iter()
            .any(|field| { matches!(field.name.as_str(), "ignored" | "staticValue" | "locked") }));
        assert!(objects.report().is_complete());
        assert!(workspace.files()[0].reference_graph.is_complete());
        assert!(workspace.files()[0]
            .reference_graph
            .object_scan_is_complete(0));
        assert_eq!(
            filesystem
                .read_all_bytes("/game/mono_unloaded_valid.assets", usize::MAX)
                .expect("source-authored input remains mounted"),
            bytes,
            "assembly recovery must leave the original SerializedFile unchanged"
        );
    }

    #[test]
    fn object_workspace_replays_source_opaque_object_raw_fallbacks() {
        let fixture = crate::processing::asset_factory::PROBE_FIXTURE;
        for (case, expected_raw_len) in [
            ("unknown_class", 8),
            ("unknown_no_tree", 8),
            ("unreadable_version", 20),
            ("unreadable_no_tree", 8),
            ("textasset_bad_tail", 25),
            ("patch_retry_light_failed", 151),
        ] {
            let hex = fixture
                .lines()
                .find_map(|line| line.strip_prefix(&format!("SF_HEX|{case}|")))
                .unwrap_or_else(|| panic!("the source-factory fixture includes {case}"));
            let bytes = hex
                .as_bytes()
                .chunks_exact(2)
                .map(|pair| {
                    let digits = std::str::from_utf8(pair).expect("fixture hex is ASCII");
                    u8::from_str_radix(digits, 16).expect("fixture hex is valid")
                })
                .collect::<Vec<_>>();
            let source_asset = fixture
                .lines()
                .find(|line| line.starts_with(&format!("SF_READ|{case}|ASSET|")))
                .unwrap_or_else(|| panic!("the source-factory fixture records {case}"));
            let source_fields: Vec<_> = source_asset.split('|').collect();
            let source_field = |name: &str| {
                source_fields
                    .iter()
                    .find_map(|field| field.strip_prefix(&format!("{name}=")))
                    .unwrap_or_else(|| panic!("{case} source row has {name}"))
            };
            let (expected_kind, expected_outcome) = match source_field("type") {
                "UnknownObject" => (
                    crate::processing::asset_factory::FactoryAssetKind::Unknown,
                    "UnknownObject raw fallback",
                ),
                "UnreadableObject" => (
                    crate::processing::asset_factory::FactoryAssetKind::Unreadable,
                    "UnreadableObject raw fallback",
                ),
                source_type => panic!("{case}: unexpected source fallback type {source_type}"),
            };
            let expected_class_id = source_field("class")
                .parse::<i32>()
                .expect("source class ID is an integer");
            let expected_class_name = source_field("classname");
            let expected_name = source_field("name");
            let virtual_path = format!("/game/{case}.assets");
            let filesystem = virtual_file_system(&virtual_path, &bytes);
            let workspace = SerializedFileWorkspace::load_from_vfs(
                &filesystem,
                &[input(&virtual_path)],
                SerializedFileWorkspaceLimits::default(),
            )
            .unwrap_or_else(|error| panic!("{case}: workspace load failed: {error}"));

            if !matches!(case, "textasset_bad_tail" | "patch_retry_light_failed") {
                assert!(!workspace.files()[0].reference_graph.is_complete());
                assert!(!workspace.files()[0]
                    .reference_graph
                    .object_scan_is_complete(0));
            }
            let objects = ObjectWorkspace::load_from_vfs(
                &filesystem,
                &workspace,
                ObjectWorkspaceLimits::default(),
            )
            .unwrap_or_else(|error| panic!("{case}: ObjectWorkspace load failed: {error}"));
            assert!(!objects.report().is_complete());
            assert_eq!(objects.report().object_count, 1);
            assert_eq!(objects.report().decoded_objects, 0);
            assert_eq!(objects.report().undecoded_objects, 1);
            assert_eq!(objects.report().total_undecoded_raw_bytes, expected_raw_len);

            let object = &objects.objects()[0];
            assert_eq!(object.factory.kind, expected_kind);
            assert_eq!(object.factory.class_id, expected_class_id);
            assert_eq!(object.factory.class_name, expected_class_name);
            assert_eq!(object.factory.name, expected_name);
            let source_log = fixture
                .lines()
                .find(|line| line.starts_with(&format!("SF_READ|{case}|LOG|")));
            if let Some(source_log) = source_log {
                let fields: Vec<_> = source_log.splitn(6, '|').collect();
                assert_eq!(fields[3], "Error");
                assert_eq!(fields[4], "Import");
                assert_eq!(
                    object.factory.log_level,
                    crate::processing::asset_factory::FactoryLogLevel::Error
                );
                let source_message = fields[5];
                let (source_prefix, source_path) = source_message
                    .split_once(" Path:")
                    .expect("source log includes its file-path context");
                match case {
                    "textasset_bad_tail" => assert_eq!(
                        source_path, "\u{2420}",
                        "the fixture marks its trailing path space visibly"
                    ),
                    "patch_retry_light_failed" => {
                        assert!(source_path.starts_with("\u{2420}"));
                        assert!(source_path.contains("System.ArgumentOutOfRangeException"));
                        assert!(source_path.contains("Parameter 'length'"));
                    }
                    _ => unreachable!("only malformed source rows carry Error logs"),
                }
                let actual_message = object
                    .factory
                    .log_message
                    .as_deref()
                    .expect("source error log message is retained");
                let (actual_prefix, actual_path) = actual_message
                    .split_once(" Path: ")
                    .expect("Rust error log includes its file-path context");
                assert_eq!(actual_prefix, source_prefix);
                match case {
                    "textasset_bad_tail" => {
                        assert_eq!(actual_path, virtual_path.as_str());
                    }
                    "patch_retry_light_failed" => {
                        let (actual_file_path, actual_error) = actual_path
                            .split_once('\n')
                            .expect("Rust retry failure retains the path and read error");
                        assert_eq!(actual_file_path, virtual_path.as_str());
                        assert!(actual_error.contains("truncated input"));
                        assert!(!actual_message.contains("2017.1.1f0"));
                    }
                    _ => unreachable!("only malformed source rows carry Error logs"),
                }
            } else {
                assert_eq!(
                    object.factory.log_level,
                    crate::processing::asset_factory::FactoryLogLevel::Silent
                );
                assert!(object.factory.log_message.is_none());
            }
            assert!(!object.is_fully_materialized());

            let error = object.payload.as_ref().unwrap_err();
            assert!(matches!(
                error.kind(),
                crate::serialized_file::file::SerializedFileErrorKind::Object(object_error)
                    if matches!(
                        object_error.kind(),
                        crate::serialized_file::object::ObjectDecodeErrorKind::FactoryObjectOutcome {
                            class_id,
                            outcome: actual_outcome,
                            diagnostic,
                            ..
                        } if *class_id == expected_class_id
                            && actual_outcome == expected_outcome
                            && diagnostic.as_deref() == object.factory.log_message.as_deref()
                    )
            ));
            let file_bytes = filesystem
                .read_all_bytes(&virtual_path, usize::MAX)
                .expect("source fixture remains mounted");
            let payload_start =
                usize::try_from(object.raw_payload_range.start).expect("payload start fits usize");
            let payload_end =
                usize::try_from(object.raw_payload_range.end).expect("payload end fits usize");
            assert_eq!(
                file_bytes.get(payload_start..payload_end),
                bytes.get(payload_start..payload_end),
                "{case}: exact raw payload survives ObjectWorkspace"
            );
            assert_eq!(payload_end - payload_start, expected_raw_len);
            match case {
                "unknown_class" | "unknown_no_tree" | "unreadable_no_tree" => assert_eq!(
                    file_bytes.get(payload_start..payload_end),
                    Some([1, 2, 3, 4, 5, 6, 7, 8].as_slice()),
                    "{case}: fixture payload is retained byte-for-byte"
                ),
                _ => {}
            }
        }
    }

    #[test]
    fn decoded_object_workspace_enforces_object_count_limit() {
        let filesystem = virtual_file_system("/game/main.assets", &make_generation_10_file());
        let workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &[input("/game/main.assets")],
            SerializedFileWorkspaceLimits::default(),
        )
        .unwrap();
        let error = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits {
                max_objects: 0,
                ..ObjectWorkspaceLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            error.kind,
            ObjectWorkspaceErrorKind::ObjectCountLimitExceeded { count: 1, limit: 0 }
        ));
    }

    #[test]
    fn workspace_collection_models_follow_source_order_and_link_object_ranges() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(
            &mut filesystem,
            "/game/A.assets",
            &make_generation_10_file(),
        );
        insert_file(
            &mut filesystem,
            "/game/Z.assets",
            &make_generation_10_file(),
        );
        let workspace = SerializedFileWorkspace::load_from_vfs(
            &filesystem,
            &[input("/game/A.assets"), input("/game/Z.assets")],
            SerializedFileWorkspaceLimits::default(),
        )
        .unwrap();
        let objects = ObjectWorkspace::load_from_vfs(
            &filesystem,
            &workspace,
            ObjectWorkspaceLimits::default(),
        )
        .unwrap();
        let collections = WorkspaceAssetCollections::build(
            &workspace,
            &objects,
            WorkspaceCollectionLimits::default(),
        )
        .unwrap();

        assert_eq!(collections.report().collection_count, 2);
        assert_eq!(collections.report().object_count, 2);
        assert_eq!(collections.collections()[0].source_file_index, 1);
        assert_eq!(collections.collections()[0].fixed_name, "z.assets");
        assert_eq!(collections.collections()[0].object_range, 1..2);
        assert_eq!(collections.collections()[1].source_file_index, 0);
        assert_eq!(collections.collections()[1].fixed_name, "a.assets");
        assert_eq!(collections.collections()[1].object_range, 0..1);
        assert_eq!(collections.collection_index_for_file(1), Some(0));
        assert_eq!(collections.collection_index_for_file(0), Some(1));
        assert_eq!(collections.collection_index_for_file(2), None);

        let limited = WorkspaceAssetCollections::build(
            &workspace,
            &objects,
            WorkspaceCollectionLimits {
                max_collections: 1,
                ..WorkspaceCollectionLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            limited.kind,
            WorkspaceCollectionErrorKind::CollectionCountLimitExceeded { count: 2, limit: 1 }
        ));

        let object_limited = WorkspaceAssetCollections::build(
            &workspace,
            &objects,
            WorkspaceCollectionLimits {
                max_objects: 1,
                ..WorkspaceCollectionLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            object_limited.kind,
            WorkspaceCollectionErrorKind::ObjectCountLimitExceeded { count: 2, limit: 1 }
        ));

        let names_limited = WorkspaceAssetCollections::build(
            &workspace,
            &objects,
            WorkspaceCollectionLimits {
                max_total_collection_name_bytes: 1,
                ..WorkspaceCollectionLimits::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            names_limited.kind,
            WorkspaceCollectionErrorKind::CollectionNameByteLimitExceeded { limit: 1, .. }
        ));
    }

    #[test]
    fn loads_named_serialized_file_and_builds_cross_file_graph() {
        let bytes = make_generation_10_file();
        let filesystem = virtual_file_system("/game/main.assets", &bytes);
        let inputs = [input("/game/main.assets")];
        let workspace = SerializedFileWorkspace::load_from_vfs_with_layout_source(
            &filesystem,
            &inputs,
            SerializedFileWorkspaceLimits::default(),
            &FileTypeTreeTestLayouts,
        )
        .unwrap();

        assert_eq!(workspace.files().len(), 1);
        assert_eq!(workspace.files()[0].container_scope, None);
        assert_eq!(workspace.files()[0].container_entry_index, None);
        assert_eq!(workspace.files()[0].container_collection_index, None);
        assert_eq!(workspace.files()[0].header.format_version.raw(), 10);
        assert_eq!(workspace.files()[0].metadata.objects[0].path_id, 42);
        assert!(workspace.files()[0].reference_graph.is_complete());
        assert_eq!(
            workspace.report().completed_input_bytes,
            u64::try_from(bytes.len()).unwrap()
        );
        let graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert_eq!(
            graph.references()[0].resolution,
            MultiFilePPtrResolution::Object {
                target_file_index: 0,
                target_object_index: 0,
            }
        );
        assert!(graph.is_complete());
    }

    #[test]
    fn workspace_reference_graph_uses_root_collection_precedence_over_child_bundles() {
        let make_loaded_file =
            |file_name: &str,
             virtual_path: &str,
             bytes: Vec<u8>,
             container_scope: Option<&str>,
             container_entry_index: Option<usize>,
             container_collection_index: Option<usize>,
             container_bundle_index: Option<usize>| {
                let parsed = SerializedFile::parse(&bytes).unwrap();
                let reference_graph = ObjectReferenceGraph::build(&parsed).unwrap();
                super::LoadedSerializedFile {
                    file_name: file_name.to_owned(),
                    virtual_path: virtual_path.to_owned(),
                    container_scope: container_scope.map(str::to_owned),
                    container_entry_index,
                    container_collection_index,
                    container_bundle_index,
                    header: parsed.header,
                    metadata: parsed.metadata,
                    reference_graph,
                }
            };
        let files = vec![
            make_loaded_file(
                "main.assets",
                "/game/main.assets",
                make_generation_10_file_with(&["shared.assets"], 1, 42, 42),
                None,
                None,
                None,
                None,
            ),
            make_loaded_file(
                "shared.assets",
                "/game/container/shared.assets",
                make_generation_10_file_with(&["shared.assets"], 1, 42, 42),
                Some("/game/container"),
                Some(0),
                Some(0),
                Some(0),
            ),
            make_loaded_file(
                "shared.assets",
                "/game/shared.assets",
                make_generation_10_file(),
                None,
                None,
                None,
                None,
            ),
        ];
        let workspace = SerializedFileWorkspace {
            files,
            container_bundles: vec![WorkspaceSerializedBundle {
                scope_path: Some("/game/container".to_owned()),
                name_fixed: None,
                parent_bundle_index: None,
                parent_entry_index: None,
                direct_collection_file_indices: vec![1],
                direct_resource_file_indices: Vec::new(),
            }],
            resource_files: Vec::new(),
            failed_container_entries: Vec::new(),
            failed_container_metadata_bytes: 0,
            report: SerializedFileWorkspaceReport::default(),
        };

        let graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert_eq!(graph.references().len(), 3);
        assert_eq!(
            graph.references()[0].resolution,
            MultiFilePPtrResolution::Object {
                target_file_index: 2,
                target_object_index: 0,
            }
        );
        assert_eq!(
            graph.references()[1].resolution,
            MultiFilePPtrResolution::Object {
                target_file_index: 1,
                target_object_index: 0,
            }
        );
        assert!(graph.is_complete());
    }

    #[test]
    fn workspace_reference_graph_keeps_first_root_bundle_collection_match() {
        let make_loaded_file =
            |file_name: &str,
             virtual_path: &str,
             bytes: Vec<u8>,
             container_scope: Option<&str>,
             container_bundle_index: Option<usize>| {
                let parsed = SerializedFile::parse(&bytes).unwrap();
                let reference_graph = ObjectReferenceGraph::build(&parsed).unwrap();
                super::LoadedSerializedFile {
                    file_name: file_name.to_owned(),
                    virtual_path: virtual_path.to_owned(),
                    container_scope: container_scope.map(str::to_owned),
                    container_entry_index: container_bundle_index.map(|_| 0),
                    container_collection_index: container_bundle_index.map(|_| 0),
                    container_bundle_index,
                    header: parsed.header,
                    metadata: parsed.metadata,
                    reference_graph,
                }
            };
        let workspace = SerializedFileWorkspace {
            files: vec![
                make_loaded_file(
                    "main.assets",
                    "/game/main.assets",
                    make_generation_10_file_with(&["shared.assets"], 1, 42, 42),
                    None,
                    None,
                ),
                make_loaded_file(
                    "shared.assets",
                    "/game/first/shared.assets",
                    make_generation_10_file(),
                    Some("/game/first"),
                    Some(0),
                ),
                make_loaded_file(
                    "shared.assets",
                    "/game/second/shared.assets",
                    make_generation_10_file(),
                    Some("/game/second"),
                    Some(1),
                ),
            ],
            container_bundles: vec![
                WorkspaceSerializedBundle {
                    scope_path: Some("/game/first".to_owned()),
                    name_fixed: None,
                    parent_bundle_index: None,
                    parent_entry_index: None,
                    direct_collection_file_indices: vec![1],
                    direct_resource_file_indices: Vec::new(),
                },
                WorkspaceSerializedBundle {
                    scope_path: Some("/game/second".to_owned()),
                    name_fixed: None,
                    parent_bundle_index: None,
                    parent_entry_index: None,
                    direct_collection_file_indices: vec![2],
                    direct_resource_file_indices: Vec::new(),
                },
            ],
            resource_files: Vec::new(),
            failed_container_entries: Vec::new(),
            failed_container_metadata_bytes: 0,
            report: SerializedFileWorkspaceReport::default(),
        };

        let graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert_eq!(
            graph.references()[0].resolution,
            MultiFilePPtrResolution::Object {
                target_file_index: 1,
                target_object_index: 0,
            }
        );
    }

    #[test]
    fn workspace_reference_graph_orders_nested_bundle_matches_by_source_entry() {
        let make_loaded_file = |file_name: &str,
                                virtual_path: &str,
                                bytes: Vec<u8>,
                                container_scope: &str,
                                container_entry_index: usize,
                                container_collection_index: usize,
                                container_bundle_index: usize| {
            let parsed = SerializedFile::parse(&bytes).unwrap();
            let reference_graph = ObjectReferenceGraph::build(&parsed).unwrap();
            super::LoadedSerializedFile {
                file_name: file_name.to_owned(),
                virtual_path: virtual_path.to_owned(),
                container_scope: Some(container_scope.to_owned()),
                container_entry_index: Some(container_entry_index),
                container_collection_index: Some(container_collection_index),
                container_bundle_index: Some(container_bundle_index),
                header: parsed.header,
                metadata: parsed.metadata,
                reference_graph,
            }
        };
        let workspace = SerializedFileWorkspace {
            files: vec![
                make_loaded_file(
                    "main.assets",
                    "/game/root/main.assets",
                    make_generation_10_file_with(&["shared.assets"], 1, 42, 42),
                    "/game/root",
                    0,
                    0,
                    0,
                ),
                make_loaded_file(
                    "shared.assets",
                    "/game/root/entry-five/shared.assets",
                    make_generation_10_file(),
                    "/game/root/entry-five",
                    0,
                    0,
                    1,
                ),
                make_loaded_file(
                    "shared.assets",
                    "/game/root/entry-two/shared.assets",
                    make_generation_10_file(),
                    "/game/root/entry-two",
                    0,
                    0,
                    2,
                ),
            ],
            container_bundles: vec![
                WorkspaceSerializedBundle {
                    scope_path: Some("/game/root".to_owned()),
                    name_fixed: None,
                    parent_bundle_index: None,
                    parent_entry_index: None,
                    direct_collection_file_indices: vec![0],
                    direct_resource_file_indices: Vec::new(),
                },
                WorkspaceSerializedBundle {
                    scope_path: Some("/game/root/entry-five".to_owned()),
                    name_fixed: None,
                    parent_bundle_index: Some(0),
                    parent_entry_index: Some(5),
                    direct_collection_file_indices: vec![1],
                    direct_resource_file_indices: Vec::new(),
                },
                WorkspaceSerializedBundle {
                    scope_path: Some("/game/root/entry-two".to_owned()),
                    name_fixed: None,
                    parent_bundle_index: Some(0),
                    parent_entry_index: Some(2),
                    direct_collection_file_indices: vec![2],
                    direct_resource_file_indices: Vec::new(),
                },
            ],
            resource_files: Vec::new(),
            failed_container_entries: Vec::new(),
            failed_container_metadata_bytes: 0,
            report: SerializedFileWorkspaceReport::default(),
        };

        let graph = workspace
            .reference_graph(MultiFileReferenceGraphLimits::default())
            .unwrap();
        assert_eq!(
            graph.references()[0].resolution,
            MultiFilePPtrResolution::Object {
                target_file_index: 2,
                target_object_index: 0,
            }
        );
        assert!(graph.is_complete());
    }

    #[test]
    fn platform_inventory_feeds_primary_files_and_ordered_datapaths_to_dependency_loading() {
        let mut filesystem = VirtualFileSystem::new();
        insert_file(
            &mut filesystem,
            "/game/Game_Data/globalgamemanagers",
            &make_generation_10_file_with(&["shared.assets"], 1, 10, 42),
        );
        insert_file(
            &mut filesystem,
            "/game/Game_Data/shared.assets",
            &make_generation_10_file(),
        );
        let structure = PlatformStructure {
            platform: PlatformKind::Windows,
            selected_path: "/game".to_owned(),
            consumed_paths: vec!["/game".to_owned()],
            root_path: "/game".to_owned(),
            name: Some("Game".to_owned()),
            game_data_path: Some("/game/Game_Data".to_owned()),
            data_paths: vec!["/game/Game_Data".to_owned()],
        };

        let loaded = structure
            .load_primary_serialized_workspace(
                &mut filesystem,
                PlatformWorkspaceLoadOptions::default(),
            )
            .unwrap();
        assert_eq!(loaded.file_inventory.primary_file_inputs.len(), 1);
        assert_eq!(
            loaded.file_inventory.primary_file_inputs[0].file_name,
            "globalgamemanagers"
        );
        assert_eq!(loaded.workspace.files().len(), 2);
        assert_eq!(loaded.dependencies.loaded_serialized_files, 1);
        assert!(matches!(
            loaded.dependencies.requests[0].outcome,
            SerializedFileDependencyOutcome::LoadedSerializedFile { .. }
        ));
    }

    #[test]
    fn platform_inventory_expands_initial_webdata_before_external_dependency_loading() {
        let mut filesystem = VirtualFileSystem::new();
        let serialized_leaf = make_generation_10_file();
        let nested_leaf = make_generation_10_file();
        let nested_webdata = make_webdata_fixture(&[
            ("inner.assets", &nested_leaf),
            ("inner.resS", b"nested stream data"),
        ]);
        let webdata = make_webdata_fixture(&[
            ("z-first.assets", &serialized_leaf),
            ("shared.assets", &serialized_leaf),
            ("nested.bundle", &nested_webdata),
            ("a-last.assets", &serialized_leaf),
            ("z-resource.resS", b"first root stream"),
            ("a-resource.resource", b"second root stream"),
        ]);
        insert_file(
            &mut filesystem,
            "/game/Game_Data/globalgamemanagers",
            &make_generation_10_file_with(&["shared.assets", "data.unity3d"], 1, 10, 42),
        );
        insert_file(&mut filesystem, "/game/Game_Data/data.unity3d", &webdata);
        let structure = PlatformStructure {
            platform: PlatformKind::Windows,
            selected_path: "/game".to_owned(),
            consumed_paths: vec!["/game".to_owned()],
            root_path: "/game".to_owned(),
            name: Some("Game".to_owned()),
            game_data_path: Some("/game/Game_Data".to_owned()),
            data_paths: vec!["/game/Game_Data".to_owned()],
        };

        let loaded = structure
            .load_primary_serialized_workspace(
                &mut filesystem,
                PlatformWorkspaceLoadOptions::default(),
            )
            .unwrap();

        assert_eq!(loaded.workspace.files().len(), 5);
        assert_eq!(loaded.initial_container_expansions.len(), 1);
        assert_eq!(
            loaded.initial_container_expansions[0].kind,
            FileKind::UnityWebData
        );
        assert_eq!(
            loaded.initial_container_expansions[0].serialized_files_found,
            4
        );
        let first_direct_file = loaded
            .workspace
            .files()
            .iter()
            .find(|file| file.virtual_path.ends_with("/z-first.assets"))
            .unwrap();
        assert_eq!(first_direct_file.container_entry_index, Some(0));
        assert_eq!(first_direct_file.container_collection_index, Some(0));
        assert_eq!(first_direct_file.container_bundle_index, Some(0));
        let direct_file = loaded
            .workspace
            .files()
            .iter()
            .find(|file| file.virtual_path.ends_with("/shared.assets"))
            .unwrap();
        assert_eq!(
            direct_file.container_scope.as_deref(),
            Some("/.assetripper/import/initial/0")
        );
        assert_eq!(direct_file.container_entry_index, Some(1));
        assert_eq!(direct_file.container_collection_index, Some(1));
        assert_eq!(direct_file.container_bundle_index, Some(0));
        let last_direct_file = loaded
            .workspace
            .files()
            .iter()
            .find(|file| file.virtual_path.ends_with("/a-last.assets"))
            .unwrap();
        assert_eq!(last_direct_file.container_entry_index, Some(3));
        assert_eq!(last_direct_file.container_collection_index, Some(2));
        assert_eq!(last_direct_file.container_bundle_index, Some(0));
        let direct_collection_indices = loaded
            .workspace
            .ordered_collection_file_indices("/.assetripper/import/initial/0")
            .unwrap();
        let direct_collection_names: Vec<_> = direct_collection_indices
            .iter()
            .map(|index| loaded.workspace.files()[*index].file_name.as_str())
            .collect();
        assert_eq!(
            direct_collection_names,
            ["z-first.assets", "shared.assets", "a-last.assets"]
        );
        let bundle_nodes = loaded.workspace.container_bundles();
        assert_eq!(bundle_nodes.len(), 2);
        assert_eq!(
            bundle_nodes[0].scope_path.as_deref(),
            Some("/.assetripper/import/initial/0")
        );
        assert_eq!(bundle_nodes[0].parent_bundle_index, None);
        let root_bundle_collection_names: Vec<_> = bundle_nodes[0]
            .direct_collection_file_indices
            .iter()
            .map(|index| loaded.workspace.files()[*index].file_name.as_str())
            .collect();
        assert_eq!(
            root_bundle_collection_names,
            ["z-first.assets", "shared.assets", "a-last.assets"]
        );
        let nested_file_index = loaded
            .workspace
            .files()
            .iter()
            .position(|file| file.virtual_path.ends_with("/nested.bundle/inner.assets"))
            .unwrap();
        assert_eq!(
            bundle_nodes[1].scope_path.as_deref(),
            Some("/.assetripper/import/initial/0/nested.bundle")
        );
        assert_eq!(bundle_nodes[1].parent_bundle_index, Some(0));
        assert_eq!(bundle_nodes[1].parent_entry_index, Some(2));
        assert_eq!(
            bundle_nodes[1].direct_collection_file_indices,
            [nested_file_index]
        );
        assert_eq!(bundle_nodes[0].direct_resource_file_indices, [0, 1]);
        assert_eq!(bundle_nodes[1].direct_resource_file_indices, [2]);
        let workspace_resources = loaded.workspace.resource_files();
        assert_eq!(workspace_resources.len(), 3);
        assert_eq!(
            workspace_resources
                .iter()
                .map(|resource| resource.virtual_path.as_str())
                .collect::<Vec<_>>(),
            [
                "/.assetripper/import/initial/0/z-resource.resS",
                "/.assetripper/import/initial/0/a-resource.resource",
                "/.assetripper/import/initial/0/nested.bundle/inner.resS",
            ]
        );
        assert_eq!(
            workspace_resources
                .iter()
                .map(|resource| resource.name.as_str())
                .collect::<Vec<_>>(),
            ["z-resource.resS", "a-resource.resource", "inner.resS"]
        );
        assert_eq!(workspace_resources[0].container_bundle_index, 0);
        assert_eq!(workspace_resources[0].container_entry_index, 4);
        assert_eq!(workspace_resources[0].container_resource_index, 0);
        assert_eq!(workspace_resources[2].container_bundle_index, 1);
        assert_eq!(workspace_resources[2].container_entry_index, 1);
        assert_eq!(workspace_resources[2].container_resource_index, 0);
        let mut invalid_workspace = loaded.workspace.clone();
        let a_last_index = invalid_workspace
            .files
            .iter()
            .position(|file| file.file_name == "a-last.assets")
            .unwrap();
        invalid_workspace.files[a_last_index].container_collection_index = Some(4);
        let error = invalid_workspace
            .ordered_collection_file_indices("/.assetripper/import/initial/0")
            .unwrap_err();
        assert!(matches!(
            error.kind,
            SerializedFileWorkspaceErrorKind::InvalidContainerCollectionSequence {
                expected: 2,
                actual: 4
            }
        ));
        let nested_file = loaded
            .workspace
            .files()
            .iter()
            .find(|file| file.virtual_path.ends_with("/nested.bundle/inner.assets"))
            .unwrap();
        assert_eq!(
            nested_file.container_scope.as_deref(),
            Some("/.assetripper/import/initial/0/nested.bundle")
        );
        assert_eq!(nested_file.container_entry_index, Some(0));
        assert_eq!(nested_file.container_collection_index, Some(0));
        assert_eq!(nested_file.container_bundle_index, Some(1));
        assert!(matches!(
            loaded.dependencies.requests[0].outcome,
            SerializedFileDependencyOutcome::AlreadyLoaded { .. }
        ));
        assert_eq!(
            loaded.dependencies.requests[1].outcome,
            SerializedFileDependencyOutcome::ContainerAlreadyExpanded {
                virtual_path: "/game/Game_Data/data.unity3d".to_owned(),
                serialized_files_loaded: 4,
                matching_serialized_files: 0,
            }
        );
    }
