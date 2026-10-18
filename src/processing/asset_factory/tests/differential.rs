use super::super::materialize::{SHADER_CLASS_ID, TEXTURE2D_CLASS_ID};
use super::*;
use crate::serialized_file::object::{
    classify_type_tree_usability, ObjectDecodeLimits, SerializedValue, TypeTreeUsability,
};

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
        if matches!(
            case.name.as_str(),
            "patch_retry_light_failed"
                | "patch_retry_graphics_settings_failed"
                | "patch_retry_font_failed"
        ) {
            let actual = asset
                .log_message
                .as_deref()
                .expect("failed Patch retry diagnostic");
            let source_context = expected
                .split_once(" Path:")
                .map(|(context, _)| context)
                .expect("source diagnostic carries the original context");
            let actual_context = actual
                .split_once(" Path:")
                .map(|(context, _)| context)
                .expect("Rust diagnostic carries the original context");
            assert_eq!(actual_context, source_context);
            assert!(actual.contains("truncated input"));
            assert!(
                !actual.contains("2017.1.1f0"),
                "retry diagnostic must not replace the original failure"
            );
            match case.name.as_str() {
                "patch_retry_light_failed" => {
                    assert!(expected.contains("System.ArgumentOutOfRangeException"));
                    assert!(expected.contains("Parameter 'length'"));
                }
                "patch_retry_graphics_settings_failed" => {
                    assert!(expected.contains("System.IndexOutOfRangeException"));
                    assert!(expected.contains("Index was outside the bounds of the array"));
                }
                "patch_retry_font_failed" => {
                    assert!(expected.contains("System.ArgumentOutOfRangeException"));
                    assert!(expected.contains("Parameter 'length'"));
                }
                _ => unreachable!("only Patch retry failures reach this branch"),
            }
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
        "patch_retry_light_failed"
        | "patch_retry_graphics_settings_failed"
        | "patch_retry_font_failed" => {
            assert_eq!(asset.kind, FactoryAssetKind::Unreadable);
            assert!(asset.payload.is_none());
            let expected_raw_len = match case.name.as_str() {
                "patch_retry_light_failed" => 151,
                "patch_retry_graphics_settings_failed" => 229,
                "patch_retry_font_failed" => 95,
                _ => unreachable!("only Patch retry failures reach this branch"),
            };
            assert_eq!(asset.raw.len(), expected_raw_len);
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
    let fixture = include_str!("../../asset_factory_fixture.txt");
    let layouts = reference_class_layouts();
    let cases = fixture_cases(fixture);
    assert_eq!(cases.len(), 27, "all probe cases are replayed");
    for case in &cases {
        assert_case(case, layouts);
    }
}
