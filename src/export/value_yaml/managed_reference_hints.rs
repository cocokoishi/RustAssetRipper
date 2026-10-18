use std::collections::HashMap;

use crate::serialized_file::managed_registry::ManagedRegistryEntryData;
use crate::serialized_file::metadata::SerializedTypeReference;
use crate::serialized_file::object::SerializedValue;

use super::layout::pointer_target_class_ids_from_type_tree;
use super::model::{
    UnityYamlObjectConversionError, UnityYamlObjectConversionHints, UnityYamlObjectConversionLimits,
};
use super::paths::{child_field_path, indexed_field_path, try_copy_string, ConversionBudget};

struct TypedPointerHintContext<'a> {
    data_root_path: &'a str,
    targets: &'a HashMap<String, i32>,
    reference_types: &'a [SerializedTypeReference],
    hints: &'a mut UnityYamlObjectConversionHints,
    limits: UnityYamlObjectConversionLimits,
    budget: &'a mut ConversionBudget,
}

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
                let targets = if let Some(type_tree) = entry
                    .reference_type_index
                    .and_then(|index| reference_types.get(index))
                    .and_then(|reference| reference.type_tree.as_ref())
                {
                    pointer_target_class_ids_from_type_tree(type_tree, limits)?
                } else {
                    Default::default()
                };
                let mut context = TypedPointerHintContext {
                    data_root_path: &data_path,
                    targets: &targets,
                    reference_types,
                    hints,
                    limits,
                    budget,
                };
                collect_typed_pointer_hints(data, &data_path, depth + 2, &mut context)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn collect_typed_pointer_hints(
    value: &SerializedValue,
    field_path: &str,
    depth: usize,
    context: &mut TypedPointerHintContext<'_>,
) -> Result<(), UnityYamlObjectConversionError> {
    context.budget.visit(depth)?;
    context.budget.check_field_path(field_path)?;

    match value {
        SerializedValue::Struct(fields) => {
            for field in fields {
                let child_path =
                    child_field_path(field_path, &field.name, context.limits.max_field_path_bytes)?;
                collect_typed_pointer_hints(&field.value, &child_path, depth + 1, context)?;
            }
        }
        SerializedValue::Array(values) => {
            for (index, element) in values.iter().enumerate() {
                let element_path =
                    indexed_field_path(field_path, index, context.limits.max_field_path_bytes)?;
                collect_typed_pointer_hints(element, &element_path, depth + 1, context)?;
            }
        }
        SerializedValue::PPtr { .. } => {
            let relative_path = if field_path == context.data_root_path {
                ""
            } else {
                let Some(relative_path) = field_path
                    .strip_prefix(context.data_root_path)
                    .and_then(|path| path.strip_prefix('.'))
                else {
                    return Ok(());
                };
                relative_path
            };
            if let Some(class_id) = target_class_id_for_path(relative_path, context.targets)? {
                insert_pointer_target_hint(context.hints, field_path, class_id)?;
            }
        }
        SerializedValue::ManagedReferenceRegistry(_) => {
            collect_pointer_hints(
                value,
                field_path,
                depth,
                context.reference_types,
                context.hints,
                context.limits,
                context.budget,
            )?;
        }
        _ => {}
    }
    Ok(())
}

fn target_class_id_for_path(
    field_path: &str,
    targets: &std::collections::HashMap<String, i32>,
) -> Result<Option<i32>, UnityYamlObjectConversionError> {
    if let Some(class_id) = targets.get(field_path) {
        return Ok(Some(*class_id));
    }

    let mut matching_class_id = None;
    for (pattern, class_id) in targets {
        if !type_tree_path_matches(pattern, field_path) {
            continue;
        }
        if matching_class_id.is_some_and(|previous| previous != *class_id) {
            return Err(UnityYamlObjectConversionError::InvalidTypeTree {
                node_index: 0,
                reason: "ambiguous PPtr target classes share one field path",
            });
        }
        matching_class_id = Some(*class_id);
    }
    Ok(matching_class_id)
}

fn type_tree_path_matches(pattern: &str, field_path: &str) -> bool {
    let pattern = pattern.as_bytes();
    let field_path = field_path.as_bytes();
    let mut pattern_index = 0;
    let mut path_index = 0;

    while pattern_index < pattern.len() && path_index < field_path.len() {
        if pattern.get(pattern_index..pattern_index + 3) == Some(b"[*]") {
            if field_path[path_index] != b'[' {
                return false;
            }
            let digit_start = path_index + 1;
            path_index = digit_start;
            while field_path.get(path_index).is_some_and(u8::is_ascii_digit) {
                path_index += 1;
            }
            if path_index == digit_start || field_path.get(path_index) != Some(&b']') {
                return false;
            }
            pattern_index += 3;
            path_index += 1;
        } else if pattern[pattern_index] == field_path[path_index] {
            pattern_index += 1;
            path_index += 1;
        } else {
            return false;
        }
    }

    pattern_index == pattern.len() && path_index == field_path.len()
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
