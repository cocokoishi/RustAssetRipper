use super::test_support::{element_layout, layout_node};
#[test]
fn managed_reference_elements_require_the_full_source_predicate() {
    // `managedReference` holding one `rid` child matches.
    let layout = element_layout(vec![
        layout_node(0, "item", "managedReference"),
        layout_node(1, "rid", "SInt64"),
    ]);
    assert!(super::is_managed_reference_element(&layout, 0, 0, 0).unwrap());
    // The legacy `id` child matches too.
    let layout = element_layout(vec![
        layout_node(0, "item", "managedRefArrayItem"),
        layout_node(1, "id", "SInt32"),
    ]);
    assert!(super::is_managed_reference_element(&layout, 0, 0, 0).unwrap());
    // A wrong child name does not match.
    let layout = element_layout(vec![
        layout_node(0, "item", "managedReference"),
        layout_node(1, "value", "SInt64"),
    ]);
    assert!(!super::is_managed_reference_element(&layout, 0, 0, 0).unwrap());
    // Two children do not match.
    let layout = element_layout(vec![
        layout_node(0, "item", "managedReference"),
        layout_node(1, "rid", "SInt64"),
        layout_node(1, "extra", "SInt32"),
    ]);
    assert!(!super::is_managed_reference_element(&layout, 0, 0, 0).unwrap());
    // No children do not match.
    let layout = element_layout(vec![layout_node(0, "item", "managedRefArrayItem")]);
    assert!(!super::is_managed_reference_element(&layout, 0, 0, 0).unwrap());
    // Other element types never match, even with a `rid` child.
    let layout = element_layout(vec![
        layout_node(0, "item", "Generic Mono"),
        layout_node(1, "rid", "SInt64"),
    ]);
    assert!(!super::is_managed_reference_element(&layout, 0, 0, 0).unwrap());
}

#[test]
fn indexed_managed_references_require_the_legacy_identifier() {
    // The legacy `id` child selects the indexed shape.
    let layout = element_layout(vec![
        layout_node(0, "item", "managedReference"),
        layout_node(1, "id", "SInt32"),
    ]);
    assert!(super::is_indexed_managed_reference_element(&layout, 0, 0, 0).unwrap());
    // The modern `rid` child does not.
    let layout = element_layout(vec![
        layout_node(0, "item", "managedReference"),
        layout_node(1, "rid", "SInt64"),
    ]);
    assert!(!super::is_indexed_managed_reference_element(&layout, 0, 0, 0).unwrap());
    // The array-item spelling participates as well.
    let layout = element_layout(vec![
        layout_node(0, "item", "managedRefArrayItem"),
        layout_node(1, "id", "SInt32"),
    ]);
    assert!(super::is_indexed_managed_reference_element(&layout, 0, 0, 0).unwrap());
    // Non-reference elements never select the indexed shape.
    let layout = element_layout(vec![
        layout_node(0, "item", "Generic Mono"),
        layout_node(1, "id", "SInt32"),
    ]);
    assert!(!super::is_indexed_managed_reference_element(&layout, 0, 0, 0).unwrap());
    // Two children do not match, even when the first is `id`.
    let layout = element_layout(vec![
        layout_node(0, "item", "managedReference"),
        layout_node(1, "id", "SInt32"),
        layout_node(1, "extra", "SInt32"),
    ]);
    assert!(!super::is_indexed_managed_reference_element(&layout, 0, 0, 0).unwrap());
}

#[test]
fn managed_registry_and_data_nodes_follow_the_source_predicates() {
    // `IsManagedReferencesRegistry`: the name pair plus more than one subnode.
    assert!(super::is_managed_references_registry(
        "ManagedReferencesRegistry",
        "references",
        2
    ));
    assert!(!super::is_managed_references_registry(
        "ManagedReferencesRegistry",
        "references",
        1
    ));
    assert!(!super::is_managed_references_registry(
        "ManagedReferencesRegistry",
        "references",
        0
    ));
    assert!(!super::is_managed_references_registry(
        "ManagedReferencesRegistry",
        "other",
        2
    ));
    assert!(!super::is_managed_references_registry(
        "other",
        "references",
        2
    ));
    // `IsReferencedObjectData`: the dynamic holder carries no subnodes; a
    // same-named node with children is an ordinary struct, not dynamic data.
    assert!(super::is_referenced_object_data(
        "ReferencedObjectData",
        "data",
        0
    ));
    assert!(!super::is_referenced_object_data(
        "ReferencedObjectData",
        "data",
        1
    ));
    assert!(!super::is_referenced_object_data(
        "ReferencedObjectData",
        "other",
        0
    ));
    assert!(!super::is_referenced_object_data("other", "data", 0));
}
