use super::super::object_conversion::{
    apply_class_editor_field_backfill, apply_custom_data_label_backfill,
    apply_game_object_header_backfill, apply_generic_header_backfill,
    apply_keyframe_tangent_mode_backfill, apply_renderer_gi_backfill,
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

fn keyframe_without_tangent_mode() -> UnityYamlNode {
    let mut key = UnityYamlNode::mapping(UnityYamlMappingStyle::Flow);
    for (name, value) in [
        ("time", UnityYamlScalar::Float32(0.5)),
        ("value", UnityYamlScalar::Float32(1.0)),
        ("inSlope", UnityYamlScalar::Float32(0.0)),
        ("outSlope", UnityYamlScalar::Float32(0.0)),
        ("weightedMode", UnityYamlScalar::Signed(0)),
        ("inWeight", UnityYamlScalar::Float32(0.33)),
        ("outWeight", UnityYamlScalar::Float32(0.33)),
    ] {
        key.insert_field(name, UnityYamlNode::scalar(value))
            .unwrap();
    }
    key
}

#[test]
fn tangent_mode_backfill_targets_curve_keyframes_only() {
    let mut curve = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    let mut keys =
        UnityYamlNode::sequence(crate::export::unity_yaml::UnityYamlSequenceStyle::Block);
    keys.push(keyframe_without_tangent_mode()).unwrap();
    let mut complete = keyframe_without_tangent_mode();
    complete.insert_field("tangentMode", signed(7)).unwrap();
    keys.push(complete).unwrap();
    curve.insert_field("m_Curve", keys).unwrap();
    let mut body = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    body.insert_field("serializedVersion", signed(2)).unwrap();
    body.insert_field("rolloffCustomCurve", curve).unwrap();
    body.insert_field("m_Enabled", signed(1)).unwrap();
    let mut document = UnityYamlDocument::object(82, 44, "AudioSource", body, false)
        .expect("test AudioSource document builds");
    apply_keyframe_tangent_mode_backfill(&mut document);
    // A second pass is a no-op: insertion skips keyframes already completed.
    apply_keyframe_tangent_mode_backfill(&mut document);
    let output = render(&document);
    key_order(
        &output,
        &[
            "outSlope",
            "tangentMode: 0",
            "weightedMode",
            "tangentMode: 7",
        ],
    );
    assert_eq!(
        output.match_indices("tangentMode").count(),
        2,
        "exactly the missing keyframe gains the default"
    );
}

#[test]
fn custom_data_labels_follow_present_stream_fields_only() {
    let mut body = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    body.insert_field("mode0", signed(1)).unwrap();
    body.insert_field("color0", signed(0)).unwrap();
    body.insert_field("vector0_0", signed(0)).unwrap();
    body.insert_field("colorful", signed(0)).unwrap();
    body.insert_field("vector0", signed(0)).unwrap();
    let mut document = UnityYamlDocument::object(198, 45, "ParticleSystem", body, false)
        .expect("test ParticleSystem document builds");
    apply_custom_data_label_backfill(&mut document);
    // A second pass is a no-op: insertion skips labels already written.
    apply_custom_data_label_backfill(&mut document);
    let output = render(&document);
    key_order(
        &output,
        &[
            "color0",
            "colorLabel0:",
            "vector0_0",
            "vectorLabel0_0:",
            "colorful",
            "vector0",
        ],
    );
    assert_eq!(
        output.match_indices("colorLabel0").count(),
        1,
        "the color label is not duplicated"
    );
    assert_eq!(
        output.match_indices("vectorLabel0_0").count(),
        1,
        "the vector label is not duplicated"
    );
    assert!(
        !output.contains("colorfulLabel"),
        "non-indexed fields gain no label"
    );
}

#[test]
fn renderer_gi_backfill_inserts_canonical_defaults_in_order() {
    // Mirrors the observed serialized shape: sources lacking the lightmap
    // block gain every field in canonical order; present values are kept.
    let mut body = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    body.insert_field("serializedVersion", signed(4)).unwrap();
    body.insert_field("m_Enabled", signed(1)).unwrap();
    body.insert_field("m_LightProbeVolumeOverride", signed(0))
        .unwrap();
    body.insert_field("m_MinimumChartSize", signed(9)).unwrap();
    body.insert_field("m_LightmapIndex", signed(7)).unwrap();
    body.insert_field("m_AdditionalVertexStreams", signed(0))
        .unwrap();
    body.insert_field("m_SortingLayerID", signed(0)).unwrap();
    let mut document = UnityYamlDocument::object(23, 46, "MeshRenderer", body, false)
        .expect("test MeshRenderer document builds");
    apply_renderer_gi_backfill(&mut document);
    // A second pass is a no-op: insertion skips fields already present.
    apply_renderer_gi_backfill(&mut document);
    let output = render(&document);
    key_order(
        &output,
        &[
            "m_LightProbeVolumeOverride",
            "m_ScaleInLightmap: 1",
            "m_PreserveUVs: 0",
            "m_IgnoreNormalsForChartDetection: 0",
            "m_ImportantGI: 0",
            "m_StitchLightmapSeams: 0",
            "m_SelectedEditorRenderState: 3",
            "m_AutoUVMaxDistance: 0.5",
            "m_AutoUVMaxAngle: 89",
            "m_LightmapParameters: {fileID: 0}",
            // Present fields keep their values and relative order after the
            // inserted block instead of being rewritten to defaults, while
            // release-only baked fields are removed.
            "m_MinimumChartSize: 9",
            "m_SortingLayerID",
        ],
    );
    assert!(
        !output.contains("m_LightmapIndex"),
        "baked lightmap indices are removed"
    );
    assert!(
        !output.contains("m_AdditionalVertexStreams"),
        "additional vertex streams are removed"
    );
}

#[test]
fn class_editor_field_deltas_insert_defaults_and_drop_release_fields() {
    // Animator: the empty warning message slots between its neighbors.
    let mut animator = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    animator
        .insert_field("m_LinearVelocityBlending", signed(0))
        .unwrap();
    animator
        .insert_field("m_HasTransformHierarchy", signed(1))
        .unwrap();
    let mut document = UnityYamlDocument::object(95, 47, "Animator", animator, false)
        .expect("test Animator document builds");
    apply_class_editor_field_backfill(&mut document, 95);
    let output = render(&document);
    key_order(
        &output,
        &[
            "m_LinearVelocityBlending",
            "m_WarningMessage:",
            "m_HasTransformHierarchy",
        ],
    );
    // Light: shadow metrics slot after the color temperature while the
    // release-only baking output is removed.
    let mut light = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    light
        .insert_field("m_UseColorTemperature", signed(0))
        .unwrap();
    let mut baking = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    baking.insert_field("serializedVersion", signed(2)).unwrap();
    light.insert_field("m_BakingOutput", baking).unwrap();
    light.insert_field("m_Cookie", signed(0)).unwrap();
    let mut document = UnityYamlDocument::object(108, 48, "Light", light, false)
        .expect("test Light document builds");
    apply_class_editor_field_backfill(&mut document, 108);
    let output = render(&document);
    key_order(
        &output,
        &[
            "m_UseColorTemperature",
            "m_ShadowRadius: 0",
            "m_ShadowAngle: 0",
            "m_Cookie",
        ],
    );
    assert!(
        !output.contains("m_BakingOutput"),
        "release-only baking output is removed"
    );
    // ParentConstraint: the lock flag slots before the sources.
    let mut constraint = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    constraint
        .insert_field("m_IsContraintActive", signed(1))
        .unwrap();
    constraint.insert_field("m_Sources", signed(0)).unwrap();
    let mut document =
        UnityYamlDocument::object(1773428102, 49, "ParentConstraint", constraint, false)
            .expect("test ParentConstraint document builds");
    apply_class_editor_field_backfill(&mut document, 1773428102);
    let output = render(&document);
    key_order(
        &output,
        &["m_IsContraintActive", "m_IsLocked: 0", "m_Sources"],
    );
    // Unlisted classes select no completion.
    let mut other = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    other.insert_field("m_Enabled", signed(1)).unwrap();
    let mut document = UnityYamlDocument::object(33, 50, "MeshFilter", other, false)
        .expect("test MeshFilter document builds");
    apply_class_editor_field_backfill(&mut document, 33);
    assert!(!render(&document).contains("m_WarningMessage"));
}

#[test]
fn mesh_optimizer_flag_slots_before_stream_data() {
    let mut body = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    body.insert_field("m_MeshMetrics", signed(0)).unwrap();
    let mut stream = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    stream.insert_field("offset", signed(0)).unwrap();
    body.insert_field("m_StreamData", stream).unwrap();
    let mut document =
        UnityYamlDocument::object(43, 51, "Mesh", body, false).expect("test Mesh document builds");
    apply_class_editor_field_backfill(&mut document, 43);
    // A second pass is a no-op: insertion skips fields already present.
    apply_class_editor_field_backfill(&mut document, 43);
    let output = render(&document);
    key_order(
        &output,
        &["m_MeshMetrics", "m_MeshOptimized: 1", "m_StreamData"],
    );
    assert_eq!(
        output.match_indices("m_MeshOptimized").count(),
        1,
        "the optimizer flag is not duplicated"
    );
}
