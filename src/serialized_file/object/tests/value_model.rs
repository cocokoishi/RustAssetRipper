use super::{SerializedFieldValue, SerializedValue, SerializedValueLookupError};
#[test]
fn structured_values_support_unique_fields_arrays_and_pointer_identity() {
    let value = SerializedValue::Struct(vec![SerializedFieldValue {
        name: "target".to_owned(),
        value: SerializedValue::PPtr {
            file_id: 2,
            path_id: 41,
        },
    }]);
    let target = value.unique_field("target").unwrap().unwrap();
    assert_eq!(target.pptr(), Some((2, 41)));
    assert_eq!(value.unique_field("missing").unwrap(), None);
    assert_eq!(value.array_elements(), None);
    assert_eq!(target.struct_fields(), None);

    let array = SerializedValue::Array(vec![SerializedValue::U8(7)]);
    assert_eq!(array.array_elements().unwrap(), &[SerializedValue::U8(7)]);
    assert_eq!(array.pptr(), None);
}

#[test]
fn structured_field_lookup_rejects_duplicate_names_and_non_structs() {
    let duplicate = SerializedValue::Struct(vec![
        SerializedFieldValue {
            name: "value".to_owned(),
            value: SerializedValue::I32(1),
        },
        SerializedFieldValue {
            name: "value".to_owned(),
            value: SerializedValue::I32(2),
        },
    ]);
    assert_eq!(
        duplicate.unique_field("value"),
        Err(SerializedValueLookupError::DuplicateField {
            name: "value".to_owned(),
            match_count: 2,
        })
    );
    assert_eq!(
        SerializedValue::Bool(true).unique_field("value"),
        Err(SerializedValueLookupError::NotStruct)
    );
}

#[test]
fn inherited_field_lookup_follows_only_base_structs_and_detects_duplicates() {
    let inherited = SerializedValue::Struct(vec![super::SerializedFieldValue {
        name: "Base".to_owned(),
        value: SerializedValue::Struct(vec![super::SerializedFieldValue {
            name: "m_Name".to_owned(),
            value: SerializedValue::StringBytes(b"child".to_vec()),
        }]),
    }]);
    assert_eq!(
        inherited.unique_field_in_base_chain("m_Name").unwrap(),
        Some(&SerializedValue::StringBytes(b"child".to_vec()))
    );

    let duplicate = SerializedValue::Struct(vec![
        super::SerializedFieldValue {
            name: "m_Name".to_owned(),
            value: SerializedValue::StringBytes(b"derived".to_vec()),
        },
        super::SerializedFieldValue {
            name: "Base".to_owned(),
            value: SerializedValue::Struct(vec![super::SerializedFieldValue {
                name: "m_Name".to_owned(),
                value: SerializedValue::StringBytes(b"base".to_vec()),
            }]),
        },
    ]);
    assert_eq!(
        duplicate.unique_field_in_base_chain("m_Name"),
        Err(SerializedValueLookupError::DuplicateField {
            name: "m_Name".to_owned(),
            match_count: 2,
        })
    );
}

#[test]
fn inherited_component_array_mutation_preserves_source_slots_and_checks_bounds() {
    let mut root = SerializedValue::Struct(vec![super::SerializedFieldValue {
        name: "Base".to_owned(),
        value: SerializedValue::Struct(vec![super::SerializedFieldValue {
            name: "m_Component".to_owned(),
            value: SerializedValue::Array(vec![
                SerializedValue::PPtr {
                    file_id: 0,
                    path_id: 0,
                },
                SerializedValue::PPtr {
                    file_id: 2,
                    path_id: 41,
                },
            ]),
        }]),
    }]);

    let components = root
        .unique_field_in_base_chain_mut("m_Component")
        .unwrap()
        .unwrap();
    components
        .push_array_element_bounded(
            SerializedValue::PPtr {
                file_id: 3,
                path_id: 99,
            },
            3,
        )
        .unwrap();
    assert!(matches!(
        components.push_array_element_bounded(
            SerializedValue::PPtr {
                file_id: 4,
                path_id: 100,
            },
            3
        ),
        Err(
            super::SerializedValueMutationError::ArrayElementLimitExceeded {
                requested: 4,
                limit: 3,
            }
        )
    ));

    let components = root
        .unique_field_in_base_chain("m_Component")
        .unwrap()
        .unwrap();
    assert_eq!(
        components.array_elements().unwrap(),
        &[
            SerializedValue::PPtr {
                file_id: 0,
                path_id: 0,
            },
            SerializedValue::PPtr {
                file_id: 2,
                path_id: 41,
            },
            SerializedValue::PPtr {
                file_id: 3,
                path_id: 99,
            },
        ]
    );
}

#[test]
fn inherited_field_mutation_rejects_duplicate_matches() {
    let mut root = SerializedValue::Struct(vec![
        super::SerializedFieldValue {
            name: "m_Component".to_owned(),
            value: SerializedValue::Array(Vec::new()),
        },
        super::SerializedFieldValue {
            name: "Base".to_owned(),
            value: SerializedValue::Struct(vec![super::SerializedFieldValue {
                name: "m_Component".to_owned(),
                value: SerializedValue::Array(Vec::new()),
            }]),
        },
    ]);

    assert!(matches!(
        root.unique_field_in_base_chain_mut("m_Component"),
        Err(super::SerializedValueMutationError::DuplicateField {
            name,
            match_count: 2,
        }) if name == "m_Component"
    ));
}
