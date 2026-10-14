use super::super::object_conversion::{
    apply_game_object_header_backfill, apply_generic_header_backfill,
    prefab_header_backfill_version,
};
use crate::export::unity_yaml::{
    UnityYamlDocument, UnityYamlMappingStyle, UnityYamlNode, UnityYamlScalar, UnityYamlWriteLimits,
};
use crate::serialized_file::object::{SerializedFieldValue, SerializedValue};

fn signed(value: i64) -> UnityYamlNode {
    UnityYamlNode::scalar(UnityYamlScalar::Signed(value))
}

fn game_object_body() -> UnityYamlNode {
    let mut body = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    body.insert_field("serializedVersion", signed(6)).unwrap();
    body.insert_field(
        "m_Component",
        UnityYamlNode::sequence(crate::export::unity_yaml::UnityYamlSequenceStyle::Block),
    )
    .unwrap();
    body.insert_field("m_Layer", signed(0)).unwrap();
    body.insert_field("m_Name", UnityYamlNode::string("probe"))
        .unwrap();
    body.insert_field("m_Tag", signed(0)).unwrap();
    body.insert_field("m_IsActive", signed(1)).unwrap();
    body
}

fn source_root() -> SerializedValue {
    SerializedValue::Struct(vec![SerializedFieldValue {
        name: "m_Tag".to_owned(),
        value: SerializedValue::I32(0),
    }])
}

fn render(document: &UnityYamlDocument) -> String {
    crate::export::unity_yaml::write_unity_yaml_documents(
        std::slice::from_ref(document),
        UnityYamlWriteLimits::default(),
    )
    .unwrap()
}

fn key_order(output: &str, keys: &[&str]) {
    let mut cursor = 0;
    for key in keys {
        let position = output[cursor..]
            .find(key)
            .unwrap_or_else(|| panic!("rendered YAML has no {key:?}"));
        cursor += position + key.len();
    }
}

#[test]
fn backfill_versions_cover_2018_4_and_2019_4_only() {
    assert!(prefab_header_backfill_version(2018, 4));
    assert!(prefab_header_backfill_version(2019, 4));
    assert!(!prefab_header_backfill_version(2022, 3));
    assert!(!prefab_header_backfill_version(2018, 3));
    assert!(!prefab_header_backfill_version(6000, 0));
}

#[test]
fn game_object_backfill_inserts_headers_and_tag_string() {
    let mut document = UnityYamlDocument::object(1, 42, "GameObject", game_object_body(), false)
        .expect("test GameObject document builds");
    apply_game_object_header_backfill(&mut document, &source_root(), false)
        .expect("backfill succeeds");
    let output = render(&document);
    key_order(
        &output,
        &[
            "serializedVersion",
            "m_ObjectHideFlags",
            "m_CorrespondingSourceObject",
            "m_PrefabInstance",
            "m_PrefabAsset",
            "m_Component",
            "m_Layer",
            "m_Name",
            "m_TagString",
            "m_Icon",
            "m_NavMeshLayer",
            "m_StaticEditorFlags",
            "m_IsActive",
        ],
    );
    assert!(output.contains("m_TagString: Untagged"));
    assert!(output.contains("m_StaticEditorFlags: 0"));
    assert!(!output.contains("m_Tag:"));
}

#[test]
fn game_object_backfill_applies_static_mark() {
    let mut document = UnityYamlDocument::object(1, 42, "GameObject", game_object_body(), false)
        .expect("test GameObject document builds");
    apply_game_object_header_backfill(&mut document, &source_root(), true)
        .expect("backfill succeeds");
    let output = render(&document);
    assert!(output.contains("m_StaticEditorFlags: 4294967295"));
}

#[test]
fn generic_backfill_inserts_universal_headers_only() {
    let mut body = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    body.insert_field("serializedVersion", signed(3)).unwrap();
    body.insert_field("m_Enabled", signed(1)).unwrap();
    let mut document = UnityYamlDocument::object(33, 43, "MeshFilter", body, false)
        .expect("test component document builds");
    apply_generic_header_backfill(&mut document).expect("backfill succeeds");
    // A second pass is a no-op: insertion skips fields already present.
    apply_generic_header_backfill(&mut document).expect("backfill is idempotent");
    let output = render(&document);
    key_order(
        &output,
        &[
            "serializedVersion",
            "m_ObjectHideFlags",
            "m_CorrespondingSourceObject",
            "m_PrefabInstance",
            "m_PrefabAsset",
            "m_Enabled",
        ],
    );
    assert_eq!(
        output.match_indices("m_ObjectHideFlags").count(),
        1,
        "headers are not duplicated"
    );
}
