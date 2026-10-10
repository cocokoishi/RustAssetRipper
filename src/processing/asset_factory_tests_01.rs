    use super::*;

    fn decode_fixture_hex(value: &str) -> Vec<u8> {
        assert_eq!(
            value.len() % 2,
            0,
            "fixture hex has an odd number of digits"
        );
        value
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let digits = std::str::from_utf8(pair).expect("hex fixture is ASCII");
                u8::from_str_radix(digits, 16).expect("fixture contains valid hex")
            })
            .collect()
    }

    /// Replays release/editor payloads written by AssetRipper's generated classes at every
    /// non-null source TPK class-version boundary through the production Rust decoder.
    #[test]
    fn source_generated_payloads_decode_against_all_selected_tpk_boundaries() {
        let fixture = include_str!("source_factory_payload_fixture.txt");
        let layouts = reference_class_layouts();
        let mut payload_count = 0usize;
        let mut skipped_count = 0usize;
        let mut payload_bytes = 0usize;
        let mut cases = std::collections::HashSet::new();

        for (line_index, line) in fixture.lines().enumerate() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<&str> = line.split('|').collect();
            match fields.first().copied() {
                Some("SKIP") => {
                    assert_eq!(fields.len(), 4, "fixture line {}", line_index + 1);
                    let class_id: i32 = fields[1].parse().expect("fixture class ID");
                    let version =
                        FactoryUnityVersion::parse(fields[2]).expect("fixture Unity version");
                    match fields[3] {
                        "MISSING" => assert_eq!(
                            layouts.generated_class_name(class_id, &version).unwrap(),
                            None,
                            "source-missing class {class_id} at {}",
                            version.text
                        ),
                        exception if exception.starts_with("THROW:") => {
                            let error = layouts
                                .generated_class_name(class_id, &version)
                                .expect_err("source dispatch exception must remain an error");
                            assert!(
                                error.to_string().contains(&exception["THROW:".len()..]),
                                "class {class_id} at {}: {error} vs {exception}",
                                version.text
                            );
                        }
                        outcome => panic!(
                            "unexpected source skip outcome `{outcome}` on line {}",
                            line_index + 1
                        ),
                    }
                    skipped_count += 1;
                }
                Some("PAYLOAD") => {
                    assert_eq!(fields.len(), 7, "fixture line {}", line_index + 1);
                    let class_id: i32 = fields[1].parse().expect("fixture class ID");
                    let version =
                        FactoryUnityVersion::parse(fields[2]).expect("fixture Unity version");
                    let source_name = layouts
                        .generated_class_name(class_id, &version)
                        .unwrap_or_else(|error| {
                            panic!("class {class_id} at {}: {error}", version.text)
                        })
                        .unwrap_or_else(|| {
                            panic!(
                                "source class missing for payload class {class_id} at {}",
                                version.text
                            )
                        });
                    assert_eq!(
                        source_name,
                        fields[3],
                        "factory type at line {}",
                        line_index + 1
                    );
                    let trees = layouts
                        .tpk_trees(class_id, &version)
                        .unwrap_or_else(|error| {
                            panic!(
                                "TPK lookup for class {class_id} at {}: {error}",
                                version.text
                            )
                        })
                        .unwrap_or_else(|| {
                            panic!(
                                "TPK layout missing for class {class_id} at {}",
                                version.text
                            )
                        });
                    let mode = fields[4];
                    let tree = match mode {
                        "release" => trees.0,
                        "editor" => trees.1,
                        mode => {
                            panic!("unexpected writer mode `{mode}` on line {}", line_index + 1)
                        }
                    };
                    let expected_length: usize = fields[5].parse().expect("fixture byte length");
                    let bytes = decode_fixture_hex(fields[6]);
                    assert_eq!(
                        bytes.len(),
                        expected_length,
                        "payload size at line {}",
                        line_index + 1
                    );
                    assert!(
                        cases.insert((class_id, version.text.clone(), fields[4].to_owned())),
                        "duplicate generated payload case on line {}",
                        line_index + 1
                    );

                    let decoded = decode_with_tree(
                        &tree,
                        &bytes,
                        Endian::Little,
                        unity_version_aligns_arrays(Some(&version.text)),
                        &[],
                        payload_count,
                        42,
                        &ObjectDecodeLimits::default(),
                    )
                    .unwrap_or_else(|error| {
                        panic!(
                            "Rust failed to decode source {mode} payload for class {class_id} at {}: {error}",
                            version.text
                        )
                    });
                    assert_eq!(
                        decoded.consumed_bytes,
                        bytes.len(),
                        "source {mode} payload tail for class {class_id} at {}",
                        version.text
                    );
                    assert!(
                        decoded.trailing_bytes.is_empty(),
                        "source {mode} payload has trailing bytes for class {class_id} at {}",
                        version.text
                    );
                    payload_bytes = payload_bytes
                        .checked_add(bytes.len())
                        .expect("fixture payload byte count fits usize");
                    payload_count += 1;
                }
                Some("FAIL") => panic!("source generated writer failed: {line}"),
                outcome => panic!(
                    "unexpected source payload fixture record `{outcome:?}` on line {}",
                    line_index + 1
                ),
            }
        }

        assert_eq!(payload_count, 8_922);
        assert_eq!(skipped_count, 539);
        assert_eq!(payload_bytes, 1_482_552);
        assert_eq!(cases.len(), payload_count);
    }

    #[test]
    fn fallback_names_follow_the_raw_data_hash_rule() {
        // The hash is the reflected CRC-32 of the raw bytes.
        assert_eq!(raw_data_hash(b""), crc32_ieee(b""));
        assert_eq!(raw_data_hash(b"abc"), 0x3524_41C2);
        // Unknown names are always synthetic with uppercase hex.
        assert_eq!(
            unknown_asset_name("Texture2D", 0x1234_ABCD),
            "UnknownTexture2D_1234ABCD"
        );
        // Unreadable names keep a stored non-empty name.
        assert_eq!(
            unreadable_asset_name("Mesh", Some("Keep"), 0x1234_ABCD),
            "Keep"
        );
        // Null or empty stored names fall back to the synthetic form.
        assert_eq!(
            unreadable_asset_name("Mesh", None, 0x1234_ABCD),
            "UnreadableMesh_1234ABCD"
        );
        assert_eq!(
            unreadable_asset_name("Mesh", Some(""), 0x1234_ABCD),
            "UnreadableMesh_1234ABCD"
        );
    }

    #[test]
    fn engine_collection_gate_matches_the_factory_tolerance_switch() {
        assert!(is_engine_collection_name(Some("unity default resources")));
        assert!(is_engine_collection_name(Some("unity_default_resources")));
        assert!(is_engine_collection_name(Some("unity builtin extra")));
        assert!(is_engine_collection_name(Some("unity_builtin_extra")));
        // Editor resources are engine resources but not tolerated collections here.
        assert!(!is_engine_collection_name(Some("unity editor resources")));
        assert!(!is_engine_collection_name(Some("level0")));
        assert!(!is_engine_collection_name(Some("Unity Default Resources")));
        assert!(!is_engine_collection_name(Some("")));
        assert!(!is_engine_collection_name(None));
        // The gate drives the tolerance branch end to end.
        let tolerated = NormalReadFacts {
            first_read_ok: false,
            is_engine_collection: is_engine_collection_name(Some("unity builtin extra")),
            is_shader: false,
            version_is_patch: false,
            retry_read_ok: false,
        };
        assert_eq!(
            decide_normal_object(&tolerated),
            (
                NormalObjectOutcome::UseFirstAsset,
                NormalObjectLog::WarningReadError
            )
        );
        let ordinary = NormalReadFacts {
            is_engine_collection: is_engine_collection_name(Some("level0")),
            ..tolerated
        };
        assert_eq!(
            decide_normal_object(&ordinary),
            (
                NormalObjectOutcome::UseUnreadable,
                NormalObjectLog::ErrorReadError
            )
        );
    }

    #[test]
    fn try_read_composition_matches_the_source_control_flow() {
        // A creation miss wraps silently without reading: later facts are ignored.
        for facts in [
            TryReadFacts {
                created: CreatedAssetKind::Unknown,
                read_threw: false,
                tail: ObjectTailOutcome::Exact,
            },
            TryReadFacts {
                created: CreatedAssetKind::Unknown,
                read_threw: true,
                tail: ObjectTailOutcome::IncorrectByteCount,
            },
        ] {
            assert_eq!(
                decide_try_read_object(&facts),
                (TryReadOutcome::UseUnknownObject, TryReadLog::Silent)
            );
        }
        // A thrown read keeps the asset with the exception error, whatever the tail.
        let thrown = TryReadFacts {
            created: CreatedAssetKind::Serialized,
            read_threw: true,
            tail: ObjectTailOutcome::Exact,
        };
        assert_eq!(
            decide_try_read_object(&thrown),
            (
                TryReadOutcome::UseCreatedAsset,
                TryReadLog::ErrorReadException
            )
        );
        // Clean and padded tails stay silent; the Chinese tail warns; leftovers error.
        let base = TryReadFacts {
            created: CreatedAssetKind::TypeTreeObject,
            read_threw: false,
            tail: ObjectTailOutcome::Exact,
        };
        assert_eq!(
            decide_try_read_object(&base),
            (TryReadOutcome::UseCreatedAsset, TryReadLog::Silent)
        );
        assert_eq!(
            decide_try_read_object(&TryReadFacts {
                tail: ObjectTailOutcome::ZeroPadding,
                ..base
            }),
            (TryReadOutcome::UseCreatedAsset, TryReadLog::Silent)
        );
        assert_eq!(
            decide_try_read_object(&TryReadFacts {
                tail: ObjectTailOutcome::ChineseTextureFields,
                ..base
            }),
            (
                TryReadOutcome::UseCreatedAsset,
                TryReadLog::WarningChineseTextureFields
            )
        );
        assert_eq!(
            decide_try_read_object(&TryReadFacts {
                tail: ObjectTailOutcome::IncorrectByteCount,
                ..base
            }),
            (
                TryReadOutcome::UseCreatedAsset,
                TryReadLog::ErrorIncorrectByteCount
            )
        );
    }

    #[test]
    fn player_settings_class_matches_the_typetree_predicate() {
        assert!(super::is_player_settings_class(
            super::PLAYER_SETTINGS_CLASS_ID
        ));
        for class_id in [0, 28, 48, 114, 115, 128, 130, -1, i32::MAX] {
            assert!(!super::is_player_settings_class(class_id), "{class_id}");
        }
    }

    #[test]
    fn monobehaviour_structures_skip_the_generated_base_fields() {
        use super::{MONO_BEHAVIOUR_EDITOR_CLASS_ID_FIELD, MONO_BEHAVIOUR_NAME_FIELD};
        assert_eq!(MONO_BEHAVIOUR_NAME_FIELD, "m_Name");
        assert_eq!(
            MONO_BEHAVIOUR_EDITOR_CLASS_ID_FIELD,
            "m_EditorClassIdentifier"
        );
        // Newer generations carry both markers: fields start after the later one.
        assert_eq!(
            super::monobehaviour_structure_start_index(&[
                "m_GameObject",
                "m_Enabled",
                "m_EditorHideFlags",
                "m_Name",
                "m_EditorClassIdentifier",
                "m_Material",
            ]),
            5
        );
        // Legacy generations without the editor marker start after `m_Name`.
        assert_eq!(
            super::monobehaviour_structure_start_index(&[
                "m_GameObject",
                "m_Enabled",
                "m_Name",
                "health",
            ]),
            3
        );
        // The maximum wins when the markers arrive out of order.
        assert_eq!(
            super::monobehaviour_structure_start_index(
                &["m_EditorClassIdentifier", "m_Name", "x",]
            ),
            2
        );
        // The first match wins, like the source `IndexOf`.
        assert_eq!(
            super::monobehaviour_structure_start_index(&["m_Name", "m_Name", "x"]),
            1
        );
        // Both markers absent starts at zero.
        assert_eq!(
            super::monobehaviour_structure_start_index(&["health", "speed"]),
            0
        );
        assert_eq!(super::monobehaviour_structure_start_index(&[]), 0);
    }

    #[test]
    fn structure_resolvers_follow_the_creation_path() {
        // Engine TypeTreeObjects from Tpk roots never get a resolver, so
        // SerializeReference fields can never resolve inside them.
        assert_eq!(
            super::select_structure_resolver(true),
            super::StructureResolverKind::Null
        );
        // MonoBehaviour TypeTree structures resolve against the reference types.
        assert_eq!(
            super::select_structure_resolver(false),
            super::StructureResolverKind::FromReferenceTypes
        );
    }

    #[test]
    fn typetree_reads_consume_the_matching_tree_and_convert_into_the_other() {
        use super::TypeTreeReadPlan;
        // Single-tree objects read their only fields in either mode.
        assert_eq!(
            super::select_typetree_read_plan(false, true),
            TypeTreeReadPlan::SingleFields
        );
        assert_eq!(
            super::select_typetree_read_plan(false, false),
            TypeTreeReadPlan::SingleFields
        );
        // Double-tree release reads consume release fields, then convert to editor.
        assert_eq!(
            super::select_typetree_read_plan(true, true),
            TypeTreeReadPlan::ReleaseThenConvertToEditor
        );
        // Double-tree editor reads consume editor fields, then convert to release.
        assert_eq!(
            super::select_typetree_read_plan(true, false),
            TypeTreeReadPlan::EditorThenConvertToRelease
        );
    }

    #[test]
    fn read_path_dispatch_matches_the_source_order() {
        assert_eq!(
            classify_read_path(true, 28),
            AssetReadPath::UnreadableUnsupportedVersion
        );
        assert_eq!(
            classify_read_path(false, MONO_BEHAVIOUR_CLASS_ID),
            AssetReadPath::MonoBehaviour
        );
        assert_eq!(classify_read_path(false, 28), AssetReadPath::Normal);
        // The version gate wins over the class dispatch.
        assert_eq!(
            classify_read_path(true, MONO_BEHAVIOUR_CLASS_ID),
            AssetReadPath::UnreadableUnsupportedVersion
        );
        assert_eq!(MONO_BEHAVIOUR_CLASS_ID, 114);
    }

    #[test]
    fn stripped_type_ids_read_as_monobehaviours_before_dispatch() {
        // The `ReadData` fallback: negatives become 114, everything else is identity.
        assert_eq!(super::resolve_read_class_id(-1), MONO_BEHAVIOUR_CLASS_ID);
        assert_eq!(super::resolve_read_class_id(-2), MONO_BEHAVIOUR_CLASS_ID);
        assert_eq!(
            super::resolve_read_class_id(i32::MIN),
            MONO_BEHAVIOUR_CLASS_ID
        );
        for type_id in [0, 1, 28, 113, 114, 115, 129, i32::MAX] {
            assert_eq!(super::resolve_read_class_id(type_id), type_id);
        }
        // The handoff routes a stripped object down the script path, while the
        // version gate still wins over the fallback like the source call order.
        assert_eq!(
            super::classify_read_data_path(false, -1),
            AssetReadPath::MonoBehaviour
        );
        assert_eq!(
            super::classify_read_data_path(false, 28),
            AssetReadPath::Normal
        );
        assert_eq!(
            super::classify_read_data_path(true, -1),
            AssetReadPath::UnreadableUnsupportedVersion
        );
    }

    #[test]
    fn read_class_id_uses_type_id_instead_of_legacy_class_id_field() {
        let object = crate::serialized_file::metadata::ObjectInfo {
            path_id: 1,
            byte_start: 0,
            byte_size: 0,
            serialized_type_index: None,
            raw_type_id: 1_480_428_607,
            class_id: Some(0),
            script_type_index: None,
            is_destroyed: None,
            is_stripped: None,
            resolved_type_index: Some(0),
        };
        assert_eq!(super::object_read_class_id(&object), 1_480_428_607);

        let stripped = crate::serialized_file::metadata::ObjectInfo {
            raw_type_id: -1,
            ..object
        };
        assert_eq!(
            super::object_read_class_id(&stripped),
            super::MONO_BEHAVIOUR_CLASS_ID
        );
    }

    #[test]
    fn tail_classification_matches_the_source_branches() {
        assert_eq!(classify_object_tail(&[], false), ObjectTailOutcome::Exact);
        assert_eq!(
            classify_object_tail(&[0, 0, 0], true),
            ObjectTailOutcome::ZeroPadding
        );
        // Zero padding wins over the texture rule on all-zero tails.
        assert_eq!(
            classify_object_tail(&[0; 24], true),
            ObjectTailOutcome::ZeroPadding
        );
        assert_eq!(
            classify_object_tail(&[1; 24], true),
            ObjectTailOutcome::ChineseTextureFields
        );
        // The 24-byte rule needs a Texture2D.
        assert_eq!(
            classify_object_tail(&[1; 24], false),
            ObjectTailOutcome::IncorrectByteCount
        );
        assert_eq!(
            classify_object_tail(&[1, 2, 3], true),
            ObjectTailOutcome::IncorrectByteCount
        );
        assert_eq!(CHINESE_TEXTURE_EXTRA_BYTES, 24);
    }

    #[test]
    fn normal_fallback_chain_matches_the_source_order() {
        let clean = NormalReadFacts {
            first_read_ok: true,
            is_engine_collection: false,
            is_shader: false,
            version_is_patch: false,
            retry_read_ok: false,
        };
        assert_eq!(
            decide_normal_object(&clean),
            (NormalObjectOutcome::UseFirstAsset, NormalObjectLog::Silent)
        );
        // Engine collections tolerate failures: info for shaders, warning otherwise.
        let shader = NormalReadFacts {
            first_read_ok: false,
            is_engine_collection: true,
            is_shader: true,
            version_is_patch: true,
            retry_read_ok: true,
        };
        assert_eq!(
            decide_normal_object(&shader),
            (
                NormalObjectOutcome::UseFirstAsset,
                NormalObjectLog::InfoEngineShaderTrees
            )
        );
        let engine = NormalReadFacts {
            is_shader: false,
            ..shader
        };
        assert_eq!(
            decide_normal_object(&engine),
            (
                NormalObjectOutcome::UseFirstAsset,
                NormalObjectLog::WarningReadError
            )
        );
        // A successful Patch retry wins silently; a failed one falls through.
        let retry_ok = NormalReadFacts {
            first_read_ok: false,
            is_engine_collection: false,
            is_shader: false,
            version_is_patch: true,
            retry_read_ok: true,
        };
        assert_eq!(
            decide_normal_object(&retry_ok),
            (
                NormalObjectOutcome::UseRetriedAsset,
                NormalObjectLog::Silent
            )
        );
        let retry_failed = NormalReadFacts {
            retry_read_ok: false,
            ..retry_ok
        };
        assert_eq!(
            decide_normal_object(&retry_failed),
            (
                NormalObjectOutcome::UseUnreadable,
                NormalObjectLog::ErrorReadError
            )
        );
        // Non-patch failures go straight to unreadable.
        let plain = NormalReadFacts {
            version_is_patch: false,
            ..retry_failed
        };
        assert_eq!(
            decide_normal_object(&plain),
            (
                NormalObjectOutcome::UseUnreadable,
                NormalObjectLog::ErrorReadError
            )
        );
    }

    #[test]
    fn monobehaviour_structure_follows_the_source_branches() {
        let parsed = MonoBehaviourReadFacts {
            read_threw: false,
            has_type_tree: true,
            type_tree_parse_ok: true,
            structure_try_read_ok: true,
        };
        assert_eq!(
            decide_monobehaviour_structure(&parsed),
            (
                MonoBehaviourStructureOutcome::UseParsedStructure,
                MonoBehaviourReadLog::Silent
            )
        );
        // A failed TryRead clears the structure instead of assigning it.
        let unreadable_tree = MonoBehaviourReadFacts {
            structure_try_read_ok: false,
            ..parsed
        };
        assert_eq!(
            decide_monobehaviour_structure(&unreadable_tree),
            (
                MonoBehaviourStructureOutcome::ClearStructure,
                MonoBehaviourReadLog::Silent
            )
        );
        // An absent type tree and an unparseable one both fall to the lazy remainder.
        for facts in [
            MonoBehaviourReadFacts {
                has_type_tree: false,
                type_tree_parse_ok: true,
                structure_try_read_ok: true,
                ..parsed
            },
            MonoBehaviourReadFacts {
                has_type_tree: true,
                type_tree_parse_ok: false,
                structure_try_read_ok: true,
                ..parsed
            },
            MonoBehaviourReadFacts {
                has_type_tree: false,
                type_tree_parse_ok: false,
                structure_try_read_ok: false,
                ..parsed
            },
        ] {
            assert_eq!(
                decide_monobehaviour_structure(&facts),
                (
                    MonoBehaviourStructureOutcome::UseUnloadedRemainder,
                    MonoBehaviourReadLog::Silent
                )
            );
        }
        // A thrown read keeps the prior structure and logs the mismatch, winning over
        // every structure fact.
        let thrown = MonoBehaviourReadFacts {
            read_threw: true,
            has_type_tree: true,
            type_tree_parse_ok: true,
            structure_try_read_ok: true,
        };
        assert_eq!(
            decide_monobehaviour_structure(&thrown),
            (
                MonoBehaviourStructureOutcome::KeepExisting,
                MonoBehaviourReadLog::ErrorLayoutMismatch
            )
        );
    }

    #[test]
    fn factory_diagnostic_texts_match_the_source_formats() {
        assert_eq!(
            incorrect_byte_count_message(12, 48, "Texture2D", "2019.4.0f1", "Windows", "res", "a/b"),
            "Read 12 but expected 48 for asset type Texture2D. V: 2019.4.0f1 P: Windows N: res Path: a/b"
        );
        assert_eq!(
            read_exception_message(
                "Mesh",
                "2022.3.1f1",
                "Android",
                "level0",
                "c/d",
                "System.IO.EndOfStreamException: boom"
            ),
            "Error during reading of asset type Mesh. V: 2022.3.1f1 P: Android N: level0 Path: c/d\nSystem.IO.EndOfStreamException: boom"
        );
        // Note the source's `MonoBehavior` (single-u) spelling is in the method name only;
        // the message body has no such word.
        assert_eq!(
            monobehaviour_layout_mismatch_message("MonoBehaviour MB", "Script S", "InvalidCastException"),
            "Unable to read MonoBehaviour MB, because script Script S layout mismatched binary content (InvalidCastException)."
        );
        assert_eq!(
            chinese_texture_warning_message("Logo"),
            "Texture Logo had an extra 24 bytes, which were assumed to be non-standard Chinese fields."
        );
    }

    #[test]
    fn chinese_tail_layout_fills_the_24_byte_suffix() {
        assert_eq!(CHINESE_TEXTURE_ORIGINAL_WIDTH_OFFSET, 0);
        assert_eq!(CHINESE_TEXTURE_ORIGINAL_HEIGHT_OFFSET, 4);
        assert_eq!(CHINESE_TEXTURE_ORIGINAL_GUID_OFFSET, 8);
        // Two Int32 reads plus the 16-byte GUID release read consume the tail exactly.
        assert_eq!(
            CHINESE_TEXTURE_ORIGINAL_GUID_OFFSET + CHINESE_TEXTURE_ORIGINAL_GUID_SIZE,
            CHINESE_TEXTURE_EXTRA_BYTES
        );
        assert_eq!(CHINESE_TEXTURE_EXTRA_BYTES, 24);
    }

    #[test]
    fn patch_build_bump_wraps_like_the_unchecked_cast() {
        assert_eq!(bump_patch_build(2019, 4, 40), (2019, 4, 41));
        assert_eq!(bump_patch_build(2022, 3, u16::MAX), (2022, 3, 0));
    }

    #[test]
    fn creation_outcomes_follow_the_fallback_order() {
        assert_eq!(
            classify_created_asset(true, true),
            CreatedAssetKind::Serialized
        );
        assert_eq!(
            classify_created_asset(false, true),
            CreatedAssetKind::TypeTreeObject
        );
        assert_eq!(
            classify_created_asset(false, false),
            CreatedAssetKind::Unknown
        );
    }

    /// Differential against the shipped `AssetRipper.Import.dll` engine table
    /// (probe `Temp/opencode/monoutilsprobe` at 2019.4.0f1, SDK 10.0.401): the
    /// `ENGINE` rows of `mono_utils_fixture.txt` pin every arm and the two
    /// rejections.
    #[test]
    fn engine_asset_names_match_the_reference_runtime() {
        let fixture = include_str!("mono_utils_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split('|');
            if parts.next() != Some("ENGINE") {
                continue;
            }
            let name = parts.next().expect("a name");
            let outcome = parts.next().expect("an outcome");
            let detail = parts.next().expect("a detail");
            assert!(parts.next().is_none(), "{line}");
            match engine_asset_kind(name) {
                Some(kind) => {
                    assert_eq!(outcome, "ok", "{line}");
                    assert_eq!(kind.concrete_class_2019_4(), detail, "{line}");
                }
                None => assert_eq!(outcome, "NotSupportedException", "{line}"),
            }
            rows += 1;
        }
        assert_eq!(rows, 23, "all probed engine rows are asserted");
        assert!(EngineAssetKind::Gradient.takes_version());
        assert!(!EngineAssetKind::Vector3.takes_version());
    }

    // -----------------------------------------------------------------------
    // Fixture-driven factory engine differential
    // -----------------------------------------------------------------------

    use crate::serialized_file::file::SerializedFile;

    struct FixtureCase {
        name: String,
        bytes: Vec<u8>,
        collection_name: String,
        rows: Vec<String>,
    }

    fn fixture_cases(fixture: &str) -> Vec<FixtureCase> {
        let mut cases: Vec<FixtureCase> = Vec::new();
        for line in fixture.lines() {
            if let Some(rest) = line.strip_prefix("SF_HEX|") {
                let (name, hex) = rest.split_once('|').expect("SF_HEX name|hex");
                cases.push(FixtureCase {
                    name: name.to_owned(),
                    bytes: hex_bytes(hex),
                    collection_name: String::new(),
                    rows: Vec::new(),
                });
            } else if let Some(rest) = line.strip_prefix("SF_READ|") {
                let (name, row) = rest.split_once('|').expect("SF_READ name|row");
                if let Some(case) = cases.iter_mut().find(|case| case.name == name) {
                    if let Some(file_name) = row
                        .strip_prefix("FILE|")
                        .and_then(|fields| fields.split('|').next())
                    {
                        case.collection_name = file_name.to_owned();
                    }
                    case.rows.push(row.to_owned());
                }
            }
        }
        cases
    }

    fn hex_bytes(hex: &str) -> Vec<u8> {
        assert!(hex.len().is_multiple_of(2), "odd hex length");
        let bytes = hex.as_bytes();
        let mut out = Vec::with_capacity(hex.len() / 2);
        let mut index = 0;
        while index < bytes.len() {
            let digit = |byte: u8| match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                other => panic!("invalid hex byte {other}"),
            };
            out.push((digit(bytes[index]) << 4) | digit(bytes[index + 1]));
            index += 2;
        }
        out
    }

    fn row_value<'a>(case: &'a FixtureCase, prefix: &str) -> Option<&'a str> {
        case.rows.iter().find_map(|row| row.strip_prefix(prefix))
    }

    fn assert_case(case: &FixtureCase, layouts: &ReferenceClassLayouts) {
        let file = SerializedFile::parse(&case.bytes)
            .unwrap_or_else(|error| panic!("{} parse: {error}", case.name));
        let is_editor_collection = case.collection_name.ends_with(".unity");
        let facts = FactoryFileFacts {
            collection_name: &case.collection_name,
            file_path: if is_editor_collection {
                &case.collection_name
            } else {
                ""
            },
            platform: if is_editor_collection {
                "NoTarget"
            } else {
                "StandaloneWin64Player"
            },
        };
        let asset = materialize_object(&file, &facts, layouts, 0, &ObjectDecodeLimits::default())
            .unwrap_or_else(|error| panic!("{} materialize: {error}", case.name));

        let reference_type = row_value(case, "ASSET|42|class=")
            .and_then(|row| row.split("|type=").nth(1))
            .and_then(|row| row.split('|').next())
            .expect("an ASSET row");
        let reference_classname = row_value(case, "ASSET|42|class=")
            .and_then(|row| row.split("|classname=").nth(1))
            .and_then(|row| row.split('|').next())
            .expect("a classname");
        let reference_name = row_value(case, "ASSET|42|class=")
            .and_then(|row| row.split("|name=").nth(1))
            .expect("a name");

        let engine_type = match &asset.kind {
            FactoryAssetKind::Generated(name) => name.as_str(),
            FactoryAssetKind::TypeTreeObject => "DoubleTypeTreeObject",
            FactoryAssetKind::Unknown => "UnknownObject",
            FactoryAssetKind::Unreadable => "UnreadableObject",
        };
        assert_eq!(engine_type, reference_type, "{} type", case.name);
        match &asset.kind {
            FactoryAssetKind::TypeTreeObject => {
                let type_name = asset
                    .payload
                    .as_ref()
                    .map(|payload| payload.type_name.as_str());
                assert_eq!(
                    type_name,
                    Some(reference_classname),
                    "{} TPK class name",
                    case.name
                );
            }
            _ => assert_eq!(
                asset.class_name.as_ref(),
                reference_classname,
                "{} class name",
                case.name
            ),
        }
        let expected_name = if reference_name == "NULL" {
            ""
        } else {
            reference_name
        };
        assert_eq!(asset.name, expected_name, "{} name", case.name);

        // The reference error rows must match the engine log exactly.
        let reference_logs: Vec<&str> = case
            .rows
            .iter()
            .filter_map(|row| row.strip_prefix("LOG|Error|Import|"))
            .collect();
        let reference_warnings: Vec<&str> = case
            .rows
            .iter()
            .filter_map(|row| row.strip_prefix("LOG|Warning|Import|"))
            .collect();
        let reference_infos: Vec<&str> = case
            .rows
            .iter()
            .filter_map(|row| row.strip_prefix("LOG|Info|Import|"))
            .collect();
        if let Some(expected) = reference_logs.first() {
            let expected = expected.replace('␠', " ");
            assert_eq!(asset.log_level, FactoryLogLevel::Error, "{} log", case.name);
            if case.name == "patch_retry_light_failed" {
                let context = "Error during reading of asset type Light. V: 2017.1.0p0 P: StandaloneWin64Player N: patch_retry_light_failed.assets Path: ";
                let actual = asset
                    .log_message
                    .as_deref()
                    .expect("failed Patch diagnostic");
                assert!(
                    expected.starts_with(context),
                    "source diagnostic context: {expected}"
                );
                assert!(
                    actual.starts_with(context),
                    "Rust diagnostic context: {actual}"
                );
                assert!(expected.contains("System.ArgumentOutOfRangeException"));
                assert!(expected.contains("Parameter 'length'"));
                assert!(actual.contains("truncated input"));
                assert!(
                    !actual.contains("2017.1.1f0"),
                    "retry diagnostic must not replace the original failure"
                );
            } else {
                assert_eq!(
                    asset.log_message.as_deref(),
                    Some(expected.as_str()),
                    "{} log text",
                    case.name
                );
            }
        } else if let Some(expected) = reference_warnings.first() {
            let expected = expected.replace('␠', " ");
            assert_eq!(
                asset.log_level,
                FactoryLogLevel::Warning,
                "{} log",
                case.name
            );
            assert_eq!(
                asset.log_message.as_deref(),
                Some(expected.as_str()),
                "{} log text",
                case.name
            );
        } else if let Some(expected) = reference_infos.first() {
            let expected = (*expected).to_owned();
            assert_eq!(asset.log_level, FactoryLogLevel::Info, "{} log", case.name);
            assert_eq!(asset.log_message, None, "{} stored log text", case.name);
            assert_eq!(
                expected,
                format!(
                    "Shader with PathID 42 in {} uses engine type trees.",
                    case.collection_name
                )
            );
        } else {
            assert_eq!(
                asset.log_level,
                FactoryLogLevel::Silent,
                "{} log",
                case.name
            );
            assert_eq!(asset.log_message, None, "{} log text", case.name);
        }

        // Case-specific tail, structure, and field fidelity.
        match case.name.as_str() {
            "textasset_notree" => {
                assert_eq!(
                    classify_type_tree_usability(file.metadata.types[0].type_tree.as_ref()),
                    TypeTreeUsability::Empty
                );
                assert_eq!(asset.tail, Some(ObjectTailOutcome::Exact));
                assert_eq!(asset.payload.unwrap().consumed_bytes, 20);
            }
            "textasset_tree" => {
                assert_eq!(
                    classify_type_tree_usability(file.metadata.types[0].type_tree.as_ref()),
                    TypeTreeUsability::Present
                );
                assert_eq!(asset.tail, Some(ObjectTailOutcome::Exact));
            }
            "textasset_padding" => {
                assert_eq!(asset.tail, Some(ObjectTailOutcome::ZeroPadding));
                assert_eq!(asset.payload.as_ref().unwrap().trailing_bytes, [0, 0, 0, 0]);
            }
            "textasset_bad_tail" => {
                assert_eq!(asset.tail, Some(ObjectTailOutcome::IncorrectByteCount));
                assert_eq!(
                    asset.payload.as_ref().unwrap().consumed_bytes,
                    20,
                    "partially read fields stay available"
                );
                assert_eq!(asset.raw.len(), 25, "raw payload preserved");
            }
            "unknown_class" => {
                assert_eq!(asset.class_name.as_ref(), "7");
                assert_eq!(asset.raw, [1, 2, 3, 4, 5, 6, 7, 8]);
                assert!(asset.payload.is_none());
            }
            "unknown_no_tree" => {
                assert_eq!(asset.kind, FactoryAssetKind::Unknown);
                assert_eq!(asset.class_id, 9999);
                assert_eq!(asset.raw, [1, 2, 3, 4, 5, 6, 7, 8]);
                assert!(asset.payload.is_none());
            }
            "tpk_navmesh" => {
                assert_eq!(asset.tail, Some(ObjectTailOutcome::Exact));
                assert_eq!(
                    asset
                        .payload
                        .as_ref()
                        .unwrap()
                        .root
                        .unique_field("m_Name")
                        .unwrap()
                        .unwrap(),
                    &SerializedValue::StringBytes(b"NavMeshData".to_vec())
                );
            }
            "tpk_navmesh_editor" => {
                assert!(
                    !file
                        .object_read_flags(&case.collection_name, &case.collection_name)
                        .is_release(),
                    "the source fixture is a NoTarget .unity file"
                );
                assert_eq!(asset.kind, FactoryAssetKind::TypeTreeObject);
                assert_eq!(asset.tail, Some(ObjectTailOutcome::Exact));
                assert_eq!(
                    asset.raw,
                    hex_bytes(
                        row_value(case, "EDITOR_HEX|")
                            .expect("the source factory records editor serialization")
                    )
                );
                assert_eq!(
                    row_value(case, "RELEASE_HEX|"),
                    Some("0C0000005363656E654E61764D657368")
                );
                let root = &asset.payload.as_ref().unwrap().root;
                assert_eq!(
                    root.unique_field("m_ObjectHideFlags").unwrap().unwrap(),
                    &SerializedValue::U32(3)
                );
                assert_eq!(
                    root.unique_field("m_PrefabParentObject").unwrap().unwrap(),
                    &SerializedValue::PPtr {
                        file_id: 0,
                        path_id: 0,
                    }
                );
                assert_eq!(
                    root.unique_field("m_PrefabInternal").unwrap().unwrap(),
                    &SerializedValue::PPtr {
                        file_id: 0,
                        path_id: 0,
                    }
                );
                assert_eq!(
                    root.unique_field("m_Name").unwrap().unwrap(),
                    &SerializedValue::StringBytes(b"SceneNavMesh".to_vec())
                );
            }
            "unreadable_version" => {
                assert!(asset.payload.is_none());
                assert_eq!(asset.raw.len(), 20);
            }
            "unreadable_no_tree" => {
                assert_eq!(asset.kind, FactoryAssetKind::Unreadable);
                assert_eq!(asset.class_id, 9999);
                assert_eq!(asset.raw, [1, 2, 3, 4, 5, 6, 7, 8]);
                assert!(asset.payload.is_none());
            }
            "engine_default_resources" => {
                assert!(asset.kind.is_recoverable());
                assert_eq!(asset.tail, Some(ObjectTailOutcome::IncorrectByteCount));
            }
            "mono_unloaded" => {
                let structure = asset.structure.as_ref().expect("a structure decision");
                assert_eq!(
                    structure.outcome,
                    MonoBehaviourStructureOutcome::UseUnloadedRemainder
                );
                assert_eq!(structure.unloaded_remainder, [0xDE, 0xAD, 0xBE, 0xEF]);
            }
            "mono_unloaded_valid" => {
                let structure = asset.structure.as_ref().expect("a structure decision");
                assert_eq!(
                    structure.outcome,
                    MonoBehaviourStructureOutcome::UseUnloadedRemainder
                );
                assert_eq!(
                    structure.unloaded_remainder,
                    [8, 7, 6, 5, 4, 3, 2, 1, 0xD2, 0x04, 0, 0, 0, 0, 0xC0, 0x3F]
                );
            }
            "mono_with_structure" => {
                let structure = asset.structure.as_ref().expect("a structure decision");
                assert_eq!(
                    structure.outcome,
                    MonoBehaviourStructureOutcome::UseParsedStructure,
                    "{} source structure outcome",
                    case.name
                );
                let fields = structure.fields.as_ref().expect("parsed structure fields");
                assert_eq!(
                    fields.unique_field("health").unwrap().unwrap(),
                    &SerializedValue::I32(100)
                );
                assert!(row_value(case, "STRUCTURE|SerializableStructure").is_some());
            }
            "monoscript_notree" => {
                let root = &asset.payload.as_ref().unwrap().root;
                assert_eq!(
                    root.unique_field("m_ClassName").unwrap().unwrap(),
                    &SerializedValue::StringBytes(b"HeroController".to_vec())
                );
                assert_eq!(
                    root.unique_field("m_AssemblyName").unwrap().unwrap(),
                    &SerializedValue::StringBytes(b"Assembly-CSharp".to_vec())
                );
                assert_eq!(
                    root.unique_field("m_ExecutionOrder").unwrap().unwrap(),
                    &SerializedValue::I32(200)
                );
            }
            "gameobject_4_alpha_notree" => {
                assert_eq!(
                    asset.kind,
                    FactoryAssetKind::Generated("GameObject_4".to_owned())
                );
                assert_eq!(asset.tail, Some(ObjectTailOutcome::Exact));
                let payload = asset
                    .payload
                    .as_ref()
                    .expect("generated GameObject payload");
                assert_eq!(payload.type_name, "GameObject");
                assert_eq!(payload.consumed_bytes, 15);
                assert!(payload.trailing_bytes.is_empty());
            }
            "texture_chinese_tail_le" | "texture_chinese_tail_be" => {
                let expected = ChineseTextureFields {
                    original_width: 2048,
                    original_height: 1024,
                    original_asset_guid: [0x1122_3344, 0x5566_7788, 0x90AB_CDEF, 0x0123_4567],
                };
                assert_eq!(asset.class_id, TEXTURE2D_CLASS_ID);
                assert_eq!(asset.name, "Logo");
                assert_eq!(asset.tail, Some(ObjectTailOutcome::ChineseTextureFields));
                assert_eq!(asset.chinese_texture_fields, Some(expected));
                assert_eq!(asset.log_level, FactoryLogLevel::Warning);
                assert_eq!(
                    row_value(case, "CHINESE_TEXTURE|"),
                    Some("2048|1024|287454020,1432778632,2427178479,19088743")
                );
                assert_eq!(
                    asset.payload.as_ref().unwrap().trailing_bytes.len(),
                    CHINESE_TEXTURE_EXTRA_BYTES
                );

                let workspace_asset = materialize_object_for_workspace(
                    &file,
                    &facts,
                    layouts,
                    0,
                    &ObjectDecodeLimits::default(),
                )
                .unwrap();
                assert_eq!(workspace_asset.state.chinese_texture_fields, Some(expected));
                assert!(workspace_asset.is_fully_materialized());
            }
            "engine_default_resources_shader"
            | "engine_default_resources_underscore_shader"
            | "engine_builtin_extra_shader"
            | "engine_builtin_extra_underscore_shader" => {
                assert_eq!(asset.class_id, SHADER_CLASS_ID);
                assert!(asset.kind.is_recoverable());
                assert_eq!(asset.tail, Some(ObjectTailOutcome::IncorrectByteCount));
                assert_eq!(asset.log_level, FactoryLogLevel::Info);
                assert_eq!(asset.log_message, None);
                let workspace_asset = materialize_object_for_workspace(
                    &file,
                    &facts,
                    layouts,
                    0,
                    &ObjectDecodeLimits::default(),
                )
                .unwrap();
                assert!(!workspace_asset.is_fully_materialized());
            }
            "patch_retry_light" => {
                assert_eq!(
                    file.metadata.prefix.unity_version_text.as_deref(),
                    Some("2017.1.0p0")
                );
                assert_eq!(
                    asset.kind,
                    FactoryAssetKind::Generated("Light_2017_1_0_p4".to_owned())
                );
                assert_eq!(asset.tail, Some(ObjectTailOutcome::Exact));
                let payload = asset.payload.as_ref().expect("retried Light payload");
                assert!(payload.trailing_bytes.is_empty());
                assert!(payload
                    .root
                    .unique_field("m_FalloffTable")
                    .unwrap()
                    .is_none());
                let workspace_asset = materialize_object_for_workspace(
                    &file,
                    &facts,
                    layouts,
                    0,
                    &ObjectDecodeLimits::default(),
                )
                .unwrap();
                assert_eq!(
                    workspace_asset.state.kind,
                    FactoryAssetKind::Generated("Light_2017_1_0_p4".to_owned())
                );
                assert!(workspace_asset.is_fully_materialized());
            }
            "patch_retry_light_failed" => {
                assert_eq!(asset.kind, FactoryAssetKind::Unreadable);
                assert!(asset.payload.is_none());
                assert_eq!(asset.raw.len(), 151);
                let workspace_asset = materialize_object_for_workspace(
                    &file,
                    &facts,
                    layouts,
                    0,
                    &ObjectDecodeLimits::default(),
                )
                .unwrap();
                assert_eq!(workspace_asset.state.kind, FactoryAssetKind::Unreadable);
                assert!(!workspace_asset.is_fully_materialized());
            }
            "patch_retry_light_editor" => {
                assert!(
                    !file
                        .object_read_flags(&case.collection_name, &case.collection_name)
                        .is_release(),
                    "the source fixture is a NoTarget .unity file"
                );
                assert_eq!(
                    file.metadata.prefix.unity_version_text.as_deref(),
                    Some("2017.1.0p0")
                );
                assert_eq!(
                    asset.kind,
                    FactoryAssetKind::Generated("Light_2017_1_0_p4".to_owned())
                );
                assert_eq!(asset.tail, Some(ObjectTailOutcome::Exact));
                assert!(asset
                    .payload
                    .as_ref()
                    .expect("editor retry payload")
                    .trailing_bytes
                    .is_empty());
                let workspace_asset = materialize_object_for_workspace(
                    &file,
                    &facts,
                    layouts,
                    0,
                    &ObjectDecodeLimits::default(),
                )
                .unwrap();
                assert_eq!(
                    workspace_asset.state.kind,
                    FactoryAssetKind::Generated("Light_2017_1_0_p4".to_owned())
                );
                assert!(workspace_asset.is_fully_materialized());
            }
            other => panic!("unexpected case {other}"),
        }
    }

    /// Every reference-authored case in `asset_factory_fixture.txt` is replayed through
    /// the production engine and compared row by row with the shipped factory.
    #[test]
    fn factory_differential_replays_every_reference_case() {
        let fixture = include_str!("asset_factory_fixture.txt");
        let layouts = reference_class_layouts();
        let cases = fixture_cases(fixture);
        assert_eq!(cases.len(), 25, "all probe cases are replayed");
        for case in &cases {
            assert_case(case, layouts);
        }
    }
