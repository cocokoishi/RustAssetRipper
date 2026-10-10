use super::test_support::{make_file, node};
use super::ObjectDecodeErrorKind;
use crate::serialized_file::metadata::TypeTree;
#[test]
fn tree_usability_distinguishes_absent_and_empty_trees() {
    use super::TypeTreeUsability;
    assert_eq!(
        super::classify_type_tree_usability(None),
        TypeTreeUsability::Missing
    );
    let empty = TypeTree {
        nodes: Vec::new(),
        string_buffer: Vec::new(),
    };
    assert_eq!(
        super::classify_type_tree_usability(Some(&empty)),
        TypeTreeUsability::Empty
    );
    let present = TypeTree {
        nodes: vec![node(0, "Root", "Root", -1, 0)],
        string_buffer: Vec::new(),
    };
    assert_eq!(
        super::classify_type_tree_usability(Some(&present)),
        TypeTreeUsability::Present
    );
    // The decode prelude reports each fact with its own error.
    let (file, header, mut metadata) = make_file(&[0]);
    metadata.types[0].type_tree = Some(empty);
    let error = metadata
        .decode_object_payload(&file, &header, 0)
        .unwrap_err();
    assert!(matches!(
        error.kind(),
        ObjectDecodeErrorKind::EmptyTypeTree { object_index: 0 }
    ));
    metadata.types[0].type_tree = None;
    let error = metadata
        .decode_object_payload(&file, &header, 0)
        .unwrap_err();
    assert!(matches!(
        error.kind(),
        ObjectDecodeErrorKind::MissingTypeTree { object_index: 0 }
    ));
}

#[test]
fn pre_3_5_payloads_are_refused_before_decoding() {
    assert!(!super::version_is_unsupported(3, 5));
    assert!(!super::version_is_unsupported(4, 0));
    assert!(!super::version_is_unsupported(2022, 3));
    assert!(super::version_is_unsupported(3, 4));
    assert!(super::version_is_unsupported(2, 6));
    assert!(super::version_is_unsupported(0, 0));
    // A positively identified pre-3.5 file is refused with its own error.
    let full = [0x78, 0x56, 0x34, 0x12, 1];
    let (file, header, mut metadata) = make_file(&full);
    metadata.prefix.unity_version_text = Some("3.4.2f3".to_owned());
    let error = metadata
        .decode_object_payload(&file, &header, 0)
        .unwrap_err();
    assert!(matches!(
        error.kind(),
        ObjectDecodeErrorKind::UnreadableUnsupportedVersion { object_index: 0 }
    ));
    // The 3.5 boundary itself, missing text, and garbage text all proceed.
    metadata.prefix.unity_version_text = Some("3.5.0b1".to_owned());
    assert!(metadata.decode_object_payload(&file, &header, 0).is_ok());
    metadata.prefix.unity_version_text = None;
    assert!(metadata.decode_object_payload(&file, &header, 0).is_ok());
    metadata.prefix.unity_version_text = Some("not-a-version".to_owned());
    assert!(metadata.decode_object_payload(&file, &header, 0).is_ok());
}

#[test]
fn recovery_class_names_follow_the_source_enum_rule() {
    // Defined IDs render the enum member name.
    assert_eq!(super::class_id_type_name(1), Some("GameObject"));
    assert_eq!(super::class_id_type_name(28), Some("Texture2D"));
    assert_eq!(super::class_id_type_name(48), Some("Shader"));
    assert_eq!(super::class_id_type_name(114), Some("MonoBehaviour"));
    assert_eq!(super::class_id_type_name(115), Some("MonoScript"));
    assert_eq!(super::class_id_type_name(194), Some("NavMeshData_194"));
    assert_eq!(
        super::class_id_type_name(2126867596),
        Some("TilemapEditorUserSettings")
    );
    // `PlayerSettings` (129) is not a member; gaps and unknown IDs read `None`.
    assert_eq!(super::class_id_type_name(129), None);
    assert_eq!(super::class_id_type_name(3), None);
    assert_eq!(super::class_id_type_name(99999), None);
    // `RawDataObject.ClassName` renders undefined values numerically.
    assert_eq!(super::raw_data_class_name(48).as_ref(), "Shader");
    assert_eq!(super::raw_data_class_name(3).as_ref(), "3");
    assert_eq!(super::raw_data_class_name(99999).as_ref(), "99999");
    assert_eq!(super::raw_data_class_name(-5).as_ref(), "-5");
    // The fact composes with the factory naming shape: `Unknown{ClassName}_{Hash:X}`.
    assert_eq!(
        format!("Unknown{}_1234ABCD", super::raw_data_class_name(28)),
        "UnknownTexture2D_1234ABCD"
    );
}
