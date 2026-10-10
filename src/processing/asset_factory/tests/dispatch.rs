use super::super::materialize::decode_with_tree;
use super::*;
use crate::io::binary::Endian;
use crate::resources::blend_shape::crc32_ieee;
use crate::serialized_file::metadata::unity_version_aligns_arrays;
use crate::serialized_file::object::ObjectDecodeLimits;

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
    let fixture = include_str!("../../source_factory_payload_fixture.txt");
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
                let version = FactoryUnityVersion::parse(fields[2]).expect("fixture Unity version");
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
                let version = FactoryUnityVersion::parse(fields[2]).expect("fixture Unity version");
                let source_name = layouts
                    .generated_class_name(class_id, &version)
                    .unwrap_or_else(|error| panic!("class {class_id} at {}: {error}", version.text))
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
        super::monobehaviour_structure_start_index(&["m_EditorClassIdentifier", "m_Name", "x",]),
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
    let fixture = include_str!("../../mono_utils_fixture.txt");
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
