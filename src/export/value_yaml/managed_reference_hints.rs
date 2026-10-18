use crate::serialized_file::managed_registry::ManagedRegistryEntryData;
use crate::serialized_file::metadata::SerializedTypeReference;
use crate::serialized_file::object::SerializedValue;

use super::layout::pointer_target_class_ids_from_type_tree;
use super::model::{
    UnityYamlObjectConversionError, UnityYamlObjectConversionHints, UnityYamlObjectConversionLimits,
};
use super::paths::{child_field_path, indexed_field_path, try_copy_string, ConversionBudget};

pub(crate) fn add_managed_reference_pointer_target_hints(
    root: &SerializedValue,
    reference_types: &[SerializedTypeReference],
    hints: &mut UnityYamlObjectConversionHints,
    limits: UnityYamlObjectConversionLimits,
) -> Result<(), UnityYamlObjectConversionError> {
    let mut budget = ConversionBudget::new(limits);
    collect_pointer_hints(root, "", 0, reference_types, hints, limits, &mut budget)
}

fn collect_pointer_hints(
    value: &SerializedValue,
    field_path: &str,
    depth: usize,
    reference_types: &[SerializedTypeReference],
    hints: &mut UnityYamlObjectConversionHints,
    limits: UnityYamlObjectConversionLimits,
    budget: &mut ConversionBudget,
) -> Result<(), UnityYamlObjectConversionError> {
    budget.visit(depth)?;
    budget.check_field_path(field_path)?;

    match value {
        SerializedValue::Struct(fields) => {
            for field in fields {
                let child_path =
                    child_field_path(field_path, &field.name, limits.max_field_path_bytes)?;
                collect_pointer_hints(
                    &field.value,
                    &child_path,
                    depth + 1,
                    reference_types,
                    hints,
                    limits,
                    budget,
                )?;
            }
        }
        SerializedValue::Array(values) => {
            for (index, element) in values.iter().enumerate() {
                let element_path =
                    indexed_field_path(field_path, index, limits.max_field_path_bytes)?;
                collect_pointer_hints(
                    element,
                    &element_path,
                    depth + 1,
                    reference_types,
                    hints,
                    limits,
                    budget,
                )?;
            }
        }
        SerializedValue::ManagedReferenceRegistry(registry) => {
            let reference_ids_path =
                child_field_path(field_path, "RefIds", limits.max_field_path_bytes)?;
            for (entry_index, entry) in registry.entries.iter().enumerate() {
                budget.visit(depth + 1)?;
                let entry_path = indexed_field_path(
                    &reference_ids_path,
                    entry_index,
                    limits.max_field_path_bytes,
                )?;
                let ManagedRegistryEntryData::Content(data) = &entry.data else {
                    continue;
                };
                let data_path = child_field_path(&entry_path, "data", limits.max_field_path_bytes)?;
                if let Some(type_tree) = entry
                    .reference_type_index
                    .and_then(|index| reference_types.get(index))
                    .and_then(|reference| reference.type_tree.as_ref())
                {
                    let targets = pointer_target_class_ids_from_type_tree(type_tree, limits)?;
                    for (relative_path, class_id) in targets {
                        let target_path = if relative_path.is_empty() {
                            data_path.clone()
                        } else {
                            child_field_path(
                                &data_path,
                                &relative_path,
                                limits.max_field_path_bytes,
                            )?
                        };
                        insert_pointer_target_hint(hints, &target_path, class_id)?;
                    }
                }
                collect_pointer_hints(
                    data,
                    &data_path,
                    depth + 2,
                    reference_types,
                    hints,
                    limits,
                    budget,
                )?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn insert_pointer_target_hint(
    hints: &mut UnityYamlObjectConversionHints,
    field_path: &str,
    class_id: i32,
) -> Result<(), UnityYamlObjectConversionError> {
    if let Some(previous) = hints.pointer_target_class_ids.get(field_path) {
        if *previous != class_id {
            return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                node_index: 0,
                reason: "ambiguous PPtr target classes share one field path",
            });
        }
        return Ok(());
    }
    hints.pointer_target_class_ids.try_reserve(1).map_err(|_| {
        UnityYamlObjectConversionError::AllocationFailed {
            resource: "Unity YAML PPtr target-class hints",
            requested: hints.pointer_target_class_ids.len().saturating_add(1),
        }
    })?;
    hints.pointer_target_class_ids.insert(
        try_copy_string(field_path, "Unity YAML PPtr target-class path")?,
        class_id,
    );
    Ok(())
}
