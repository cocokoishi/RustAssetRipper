use crate::serialized_file::metadata::SerializedFileMetadata;
use crate::serialized_file::object::{
    ObjectDefaultValueLimits, ObjectEncodeLimits, ParsedObjectPayload, SerializedValue,
};

use super::identity_plan::WorkspacePrefabProcessingError;
use super::processed_assets::{processed_asset_operation_error, WorkspaceProcessedPrefabAssetPlan};
use super::*;
use crate::processing::scene::SceneRelationshipTarget;
use crate::processing::scene_hierarchy::WorkspaceSceneObjectIdentity;

const TRANSFORM_CLASS_ID: i32 = 4;
/// Source `GameObjectExtensions.AddComponent` insertion for a generated Transform.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceGeneratedComponentAddition {
    pub game_object: WorkspaceSceneObjectIdentity,
    pub component_class_id: i32,
    pub appended_index: usize,
    pub component_asset: WorkspaceProcessedAssetIdentity,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceSourceGameObjectComponentSlot {
    pub source_array_index: usize,
    pub file_id: i32,
    pub path_id: i64,
    pub target: SceneRelationshipTarget,
}

/// A non-mutating view of the source component slots and the generated Transform append.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceGameObjectComponentListOverlay {
    pub game_object: WorkspaceSceneObjectIdentity,
    pub source_slots: Vec<WorkspaceSourceGameObjectComponentSlot>,
    pub generated_component: WorkspaceGeneratedComponentAddition,
}

/// Typed root and encoded bytes for one planned, newly generated Transform.
///
/// The caller can place `root` into a `SerializedObjectInsertion` when creating the generated
/// collection file. The bytes are returned as direct evidence that the selected existing type
/// entry can encode this value; they do not create the collection or its object record.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceGeneratedTransformPayload {
    pub asset: WorkspaceProcessedAssetIdentity,
    pub type_index: usize,
    pub root: SerializedValue,
    pub bytes: Vec<u8>,
}

/// Materialize the planned Transform constructor values against an existing serialized type.
///
/// This applies the source `TransformExtensions.InitializeDefault` values and the checked
/// `GameObject_C4P` dependency from the processed-asset plan. The selected type must be an
/// existing class-ID-4 TypeTree; this does not synthesize a missing serialized type entry.
pub fn materialize_planned_generated_transform_payload(
    plan: &WorkspaceProcessedPrefabAssetPlan,
    asset_identity: WorkspaceProcessedAssetIdentity,
    metadata: &SerializedFileMetadata,
    type_index: usize,
    default_limits: ObjectDefaultValueLimits,
    encode_limits: ObjectEncodeLimits,
) -> Result<WorkspaceGeneratedTransformPayload, WorkspacePrefabProcessingError> {
    if !plan.is_complete() {
        return Err(processed_asset_operation_error(
            "PrefabProcessor asset allocation plan is incomplete".to_owned(),
        ));
    }
    if asset_identity.class_id != TRANSFORM_CLASS_ID {
        return Err(processed_asset_operation_error(format!(
            "generated asset class ID {} is not Transform class ID {TRANSFORM_CLASS_ID}",
            asset_identity.class_id
        )));
    }
    let collection = plan
        .collections
        .get(asset_identity.collection_index)
        .ok_or_else(|| {
            processed_asset_operation_error(format!(
                "generated Transform collection {} is unavailable",
                asset_identity.collection_index
            ))
        })?;
    let mut matching_assets = collection
        .assets
        .iter()
        .filter(|asset| asset.identity == asset_identity);
    let asset = matching_assets.next().ok_or_else(|| {
        processed_asset_operation_error(format!(
            "generated Transform {:?} is absent from its planned collection",
            asset_identity
        ))
    })?;
    if matching_assets.next().is_some() {
        return Err(processed_asset_operation_error(format!(
            "generated Transform {:?} occurs more than once in its planned collection",
            asset_identity
        )));
    }
    let WorkspaceProcessedPrefabAssetKind::GeneratedTransform {
        game_object,
        default_values,
    } = &asset.kind
    else {
        return Err(processed_asset_operation_error(format!(
            "planned asset {:?} is not a generated Transform",
            asset_identity
        )));
    };

    let mut matching_pointers = plan.constructor_pointers.iter().filter(|pointer| {
        pointer.source_asset == asset_identity
            && pointer.field == WorkspaceGeneratedPointerField::TransformGameObject
    });
    let constructor_pointer = matching_pointers.next().ok_or_else(|| {
        generated_transform_materialization_error(
            *game_object,
            "planned Transform has no GameObject constructor PPtr",
        )
    })?;
    if matching_pointers.next().is_some() {
        return Err(generated_transform_materialization_error(
            *game_object,
            "planned Transform has duplicate GameObject constructor PPtrs",
        ));
    }
    if constructor_pointer.target != *game_object
        || constructor_pointer.path_id != game_object.path_id
        || constructor_pointer.file_id <= 0
    {
        return Err(generated_transform_materialization_error(
            *game_object,
            "planned GameObject constructor PPtr disagrees with the generated asset kind",
        ));
    }
    let matching_dependencies = collection
        .dependencies
        .iter()
        .filter(|dependency| dependency.file_id == constructor_pointer.file_id);
    let mut matching_dependencies = matching_dependencies;
    let Some(dependency) = matching_dependencies.next() else {
        return Err(generated_transform_materialization_error(
            *game_object,
            "planned GameObject FileID is absent from the generated collection dependency table",
        ));
    };
    if matching_dependencies.next().is_some()
        || dependency.target
            != (WorkspaceProcessedDependencyTarget::SerializedFileCollection {
                file_index: game_object.file_index,
            })
    {
        return Err(generated_transform_materialization_error(
            *game_object,
            "planned GameObject FileID does not uniquely target its source SerializedFile",
        ));
    }

    let serialized_type = metadata.types.get(type_index).ok_or_else(|| {
        generated_transform_materialization_error(
            *game_object,
            &format!(
                "Transform type index {type_index} is outside the {}-entry type table",
                metadata.types.len()
            ),
        )
    })?;
    if serialized_type.raw_type_id != TRANSFORM_CLASS_ID {
        return Err(generated_transform_materialization_error(
            *game_object,
            &format!(
                "serialized type {type_index} has class ID {}, expected {TRANSFORM_CLASS_ID}",
                serialized_type.raw_type_id
            ),
        ));
    }
    let object_index = metadata.objects.len();
    let mut root = metadata
        .default_object_value_for_type(object_index, type_index, default_limits)
        .map_err(|error| {
            generated_transform_materialization_error(*game_object, &error.to_string())
        })?;

    set_transform_vector3(
        &mut root,
        *game_object,
        "m_LocalPosition",
        default_values.local_position,
    )?;
    set_transform_quaternion(
        &mut root,
        *game_object,
        "m_LocalRotation",
        default_values.local_rotation,
    )?;
    set_transform_vector3(
        &mut root,
        *game_object,
        "m_LocalScale",
        default_values.local_scale,
    )?;
    // The 2019.4 class-4 TPK omits `m_RootOrder`; the baseline setter is a silent no-op there,
    // so a missing field must not fail the generated Transform.
    set_optional_transform_i32(
        &mut root,
        *game_object,
        "m_RootOrder",
        default_values.root_order,
    )?;
    if let Some(euler_hint) = root
        .unique_field_in_base_chain_mut("m_LocalEulerAnglesHint")
        .map_err(|error| {
            generated_transform_materialization_error(*game_object, &error.to_string())
        })?
    {
        set_vector3_components(
            euler_hint,
            *game_object,
            "m_LocalEulerAnglesHint",
            default_values.local_euler_angles_hint_if_present,
        )?;
    }
    let game_object_field = root
        .unique_field_in_base_chain_mut("m_GameObject")
        .map_err(|error| {
            generated_transform_materialization_error(*game_object, &error.to_string())
        })?
        .ok_or_else(|| {
            generated_transform_materialization_error(
                *game_object,
                "Transform TypeTree has no m_GameObject field",
            )
        })?;
    match game_object_field {
        SerializedValue::PPtr { file_id, path_id } => {
            *file_id = constructor_pointer.file_id;
            *path_id = constructor_pointer.path_id;
        }
        other => {
            return Err(generated_transform_materialization_error(
                *game_object,
                &format!(
                    "Transform m_GameObject has value shape {} instead of PPtr",
                    component_value_kind(other)
                ),
            ));
        }
    }

    let bytes = metadata
        .encode_object_payload_for_type(object_index, type_index, &root, &[], encode_limits)
        .map_err(|error| {
            generated_transform_materialization_error(*game_object, &error.to_string())
        })?;
    Ok(WorkspaceGeneratedTransformPayload {
        asset: asset_identity,
        type_index,
        root,
        bytes,
    })
}

fn set_transform_vector3(
    root: &mut SerializedValue,
    game_object: WorkspaceSceneObjectIdentity,
    field_name: &str,
    value: crate::processing::scene::SceneVector3,
) -> Result<(), WorkspacePrefabProcessingError> {
    let vector = root
        .unique_field_in_base_chain_mut(field_name)
        .map_err(|error| {
            generated_transform_materialization_error(game_object, &error.to_string())
        })?
        .ok_or_else(|| {
            generated_transform_materialization_error(
                game_object,
                &format!("Transform TypeTree has no {field_name} field"),
            )
        })?;
    set_vector3_components(vector, game_object, field_name, value)
}

fn set_vector3_components(
    vector: &mut SerializedValue,
    game_object: WorkspaceSceneObjectIdentity,
    field_name: &str,
    value: crate::processing::scene::SceneVector3,
) -> Result<(), WorkspacePrefabProcessingError> {
    for (name, component) in [("x", value.x), ("y", value.y), ("z", value.z)] {
        let field_path = format!("{field_name}.{name}");
        let field = vector
            .unique_field_in_base_chain_mut(name)
            .map_err(|error| {
                generated_transform_materialization_error(game_object, &error.to_string())
            })?
            .ok_or_else(|| {
                generated_transform_materialization_error(
                    game_object,
                    &format!("Transform TypeTree has no {field_path} field"),
                )
            })?;
        match field {
            SerializedValue::F32(current) => *current = component,
            other => {
                return Err(generated_transform_materialization_error(
                    game_object,
                    &format!(
                        "Transform {field_path} has value shape {} instead of f32",
                        component_value_kind(other)
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn set_transform_quaternion(
    root: &mut SerializedValue,
    game_object: WorkspaceSceneObjectIdentity,
    field_name: &str,
    value: crate::processing::scene::SceneQuaternion,
) -> Result<(), WorkspacePrefabProcessingError> {
    let quaternion = root
        .unique_field_in_base_chain_mut(field_name)
        .map_err(|error| {
            generated_transform_materialization_error(game_object, &error.to_string())
        })?
        .ok_or_else(|| {
            generated_transform_materialization_error(
                game_object,
                &format!("Transform TypeTree has no {field_name} field"),
            )
        })?;
    for (name, component) in [
        ("x", value.x),
        ("y", value.y),
        ("z", value.z),
        ("w", value.w),
    ] {
        let field_path = format!("{field_name}.{name}");
        let field = quaternion
            .unique_field_in_base_chain_mut(name)
            .map_err(|error| {
                generated_transform_materialization_error(game_object, &error.to_string())
            })?
            .ok_or_else(|| {
                generated_transform_materialization_error(
                    game_object,
                    &format!("Transform TypeTree has no {field_path} field"),
                )
            })?;
        match field {
            SerializedValue::F32(current) => *current = component,
            other => {
                return Err(generated_transform_materialization_error(
                    game_object,
                    &format!(
                        "Transform {field_path} has value shape {} instead of f32",
                        component_value_kind(other)
                    ),
                ));
            }
        }
    }
    Ok(())
}

/// Set an optional Transform integer field; an absent field is the baseline's no-op setter.
fn set_optional_transform_i32(
    root: &mut SerializedValue,
    game_object: WorkspaceSceneObjectIdentity,
    field_name: &str,
    value: i32,
) -> Result<(), WorkspacePrefabProcessingError> {
    let Some(field) = root
        .unique_field_in_base_chain_mut(field_name)
        .map_err(|error| {
            generated_transform_materialization_error(game_object, &error.to_string())
        })?
    else {
        return Ok(());
    };
    match field {
        SerializedValue::I32(current) => {
            *current = value;
            Ok(())
        }
        other => Err(generated_transform_materialization_error(
            game_object,
            &format!(
                "Transform {field_name} has value shape {} instead of i32",
                component_value_kind(other)
            ),
        )),
    }
}

fn generated_transform_materialization_error(
    game_object: WorkspaceSceneObjectIdentity,
    message: &str,
) -> WorkspacePrefabProcessingError {
    WorkspacePrefabProcessingError::WorkspaceOperationFailed {
        operation: "generated Transform payload materialization",
        message: format!(
            "source GameObject file {} object {} PathID {}: {message}",
            game_object.file_index, game_object.object_index, game_object.path_id
        ),
    }
}

/// Materialize one checked component-list overlay as an edited GameObject payload.
///
/// Existing component values and their order are retained. The appended element reuses the
/// source element shape, or an explicit TypeTree-derived template when the source array is empty.
/// This edits only the decoded payload; callers must also write the planned external dependency
/// entry before treating the resulting PPtr as a complete SerializedFile edit.
pub fn materialize_game_object_component_append(
    source_payload: &ParsedObjectPayload,
    overlay: &WorkspaceGameObjectComponentListOverlay,
    component_file_id: i32,
    empty_element_template: Option<&SerializedValue>,
    max_component_elements: usize,
) -> Result<ParsedObjectPayload, WorkspacePrefabProcessingError> {
    let game_object = overlay.game_object;
    if source_payload.object_index != game_object.object_index
        || source_payload.path_id != game_object.path_id
        || overlay.generated_component.game_object != game_object
        || overlay.generated_component.component_class_id != 4
        || overlay.generated_component.component_asset.class_id != 4
        || overlay.generated_component.appended_index != overlay.source_slots.len()
    {
        return Err(component_materialization_error(
            game_object,
            "payload or generated Transform identity does not match the component overlay",
        ));
    }

    let component_array = source_payload
        .root
        .unique_field_in_base_chain("m_Component")
        .map_err(|error| component_materialization_error(game_object, &error.to_string()))?
        .ok_or_else(|| {
            component_materialization_error(game_object, "GameObject has no m_Component field")
        })?;
    let source_elements = component_array.array_elements().ok_or_else(|| {
        component_materialization_error(game_object, "m_Component is not an array")
    })?;
    if source_elements.len() != overlay.source_slots.len() {
        return Err(component_materialization_error(
            game_object,
            &format!(
                "m_Component has {} entries, overlay describes {}",
                source_elements.len(),
                overlay.source_slots.len()
            ),
        ));
    }
    for (index, (element, slot)) in source_elements
        .iter()
        .zip(&overlay.source_slots)
        .enumerate()
    {
        if slot.source_array_index != index {
            return Err(component_materialization_error(
                game_object,
                &format!(
                    "overlay slot {} is stored at array index {index}",
                    slot.source_array_index
                ),
            ));
        }
        let pointer = component_pair_pointer(element).map_err(|message| {
            component_materialization_error(
                game_object,
                &format!("source m_Component[{index}] {message}"),
            )
        })?;
        if pointer != (slot.file_id, slot.path_id) {
            return Err(component_materialization_error(
                game_object,
                &format!(
                    "source m_Component[{index}] PPtr ({}, {}) differs from overlay PPtr ({}, {})",
                    pointer.0, pointer.1, slot.file_id, slot.path_id
                ),
            ));
        }
    }

    let mut appended_element = if let Some(last) = source_elements.last() {
        last.clone()
    } else {
        empty_element_template.cloned().ok_or_else(|| {
            component_materialization_error(
                game_object,
                "empty m_Component array requires a TypeTree-derived element template",
            )
        })?
    };
    replace_component_pair_pointer(
        &mut appended_element,
        component_file_id,
        overlay.generated_component.component_asset.path_id,
    )
    .map_err(|message| component_materialization_error(game_object, &message))?;

    let mut edited = source_payload.clone();
    let array = edited
        .root
        .unique_field_in_base_chain_mut("m_Component")
        .map_err(|error| component_materialization_error(game_object, &error.to_string()))?
        .ok_or_else(|| {
            component_materialization_error(game_object, "GameObject has no m_Component field")
        })?;
    array
        .push_array_element_bounded(appended_element, max_component_elements)
        .map_err(|error| component_materialization_error(game_object, &error.to_string()))?;
    Ok(edited)
}

/// Materialize a component append using the FileID assigned by the completed Prefab plan.
pub fn materialize_planned_game_object_component_append(
    plan: &WorkspaceProcessedPrefabAssetPlan,
    source_payload: &ParsedObjectPayload,
    overlay: &WorkspaceGameObjectComponentListOverlay,
    empty_element_template: Option<&SerializedValue>,
    max_component_elements: usize,
) -> Result<ParsedObjectPayload, WorkspacePrefabProcessingError> {
    let game_object = overlay.game_object;
    if !plan.is_complete() {
        return Err(component_materialization_error(
            game_object,
            "PrefabProcessor asset allocation plan is incomplete",
        ));
    }
    let collection_index = overlay.generated_component.component_asset.collection_index;
    let mut matching_appends = plan.source_dependency_appends.iter().filter(|append| {
        append.source_file_index == game_object.file_index
            && append.pointer_field == WorkspaceSourceDependencyPointerField::GameObjectComponent
            && append.target_collection
                == (WorkspaceSourceDependencyAppendTarget::PrefabProcessedCollection {
                    collection_index,
                })
    });
    let append = matching_appends.next().ok_or_else(|| {
        component_materialization_error(
            game_object,
            "Prefab plan has no source dependency append for the generated collection",
        )
    })?;
    if matching_appends.next().is_some() {
        return Err(component_materialization_error(
            game_object,
            "Prefab plan has duplicate source dependency appends for the generated collection",
        ));
    }
    let component_file_id = append.file_id.ok_or_else(|| {
        component_materialization_error(
            game_object,
            "source dependency FileID is deferred or unavailable",
        )
    })?;
    materialize_game_object_component_append(
        source_payload,
        overlay,
        component_file_id,
        empty_element_template,
        max_component_elements,
    )
}

fn component_pair_pointer(value: &SerializedValue) -> Result<(i32, i64), String> {
    if let Some(pointer) = value.pptr() {
        return Ok(pointer);
    }
    let fields = value.struct_fields().ok_or_else(|| {
        format!(
            "has unsupported value shape {}",
            component_value_kind(value)
        )
    })?;
    if fields.len() != 1 {
        return Err("has a ComponentPair value with fields besides component".to_owned());
    }
    let mut component_fields = fields.iter().filter(|field| field.name == "component");
    let component = component_fields
        .next()
        .ok_or_else(|| "has no component field".to_owned())?;
    if component_fields.next().is_some() {
        return Err("has duplicate component fields".to_owned());
    }
    component
        .value
        .pptr()
        .ok_or_else(|| "component field is not a PPtr".to_owned())
}

fn replace_component_pair_pointer(
    value: &mut SerializedValue,
    file_id: i32,
    path_id: i64,
) -> Result<(), String> {
    if matches!(value, SerializedValue::PPtr { .. }) {
        *value = SerializedValue::PPtr { file_id, path_id };
        return Ok(());
    }
    let fields = match value {
        SerializedValue::Struct(fields) => fields,
        _ => {
            return Err(format!(
                "template has unsupported value shape {}",
                component_value_kind(value)
            ));
        }
    };
    if fields.len() != 1 {
        return Err("template has fields besides ComponentPair.component".to_owned());
    }
    let mut component_fields = fields.iter_mut().filter(|field| field.name == "component");
    let component = component_fields
        .next()
        .ok_or_else(|| "template has no component field".to_owned())?;
    if component_fields.next().is_some() {
        return Err("template has duplicate component fields".to_owned());
    }
    if !matches!(component.value, SerializedValue::PPtr { .. }) {
        return Err("template component field is not a PPtr".to_owned());
    }
    component.value = SerializedValue::PPtr { file_id, path_id };
    Ok(())
}

fn component_value_kind(value: &SerializedValue) -> &'static str {
    match value {
        SerializedValue::Bool(_) => "bool",
        SerializedValue::I8(_) => "i8",
        SerializedValue::U8(_) => "u8",
        SerializedValue::I16(_) => "i16",
        SerializedValue::U16(_) => "u16",
        SerializedValue::Char(_) => "char",
        SerializedValue::I32(_) => "i32",
        SerializedValue::U32(_) => "u32",
        SerializedValue::I64(_) => "i64",
        SerializedValue::U64(_) => "u64",
        SerializedValue::F32(_) => "f32",
        SerializedValue::F64(_) => "f64",
        SerializedValue::StringBytes(_) => "string",
        SerializedValue::Bytes(_) => "bytes",
        SerializedValue::Array(_) => "array",
        SerializedValue::Struct(_) => "struct",
        SerializedValue::PPtr { .. } => "PPtr",
        SerializedValue::ManagedReferenceId { .. } => "managed reference ID",
        SerializedValue::ManagedReferenceRegistry(_) => "managed reference registry",
        SerializedValue::Raw { .. } => "raw",
    }
}

fn component_materialization_error(
    game_object: WorkspaceSceneObjectIdentity,
    message: &str,
) -> WorkspacePrefabProcessingError {
    WorkspacePrefabProcessingError::WorkspaceOperationFailed {
        operation: "GameObject component-list materialization",
        message: format!(
            "file {} object {} PathID {}: {message}",
            game_object.file_index, game_object.object_index, game_object.path_id
        ),
    }
}
