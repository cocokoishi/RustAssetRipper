//! Bounded view of the serialized fields consumed by `AnimationClipConverter`.
//!
//! This adapter deliberately does not rewrite a `SerializedValue` tree. It produces the
//! converter's typed input, including pre-existing curve lists which the source processor
//! retains and appends to.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::numerics::{Quat, Vec3};
use crate::resources::animation::clip_object::{
    AnimationClipObject, BindingObject, ClipEventObject, ConstantClipObject, DenseClipObject,
    FloatCurveObject, MuscleClipObject, PPtrCurveObject, PPtrKeyframeObject, PPtrValue,
    QuaternionCurveObject, StreamedClipObject, Vector3CurveObject,
};
use crate::resources::animation_curve::curve_presence;
use crate::resources::curve_wrappers::{CurveEnvelope, ScriptReference};
use crate::resources::keyframe::{keyframe_presence, KeyframeView};
use crate::resources::keyframe_quaternion::KeyframeQuaternionView;
use crate::resources::keyframe_vector3::KeyframeVector3View;
use crate::resources::muscle_clip_info::MuscleClipConstantView;
use crate::resources::vertex_data_blob::{EndianType, UnityVersion};
use crate::serialized_file::metadata::TypeTree;
use crate::serialized_file::object::{
    default_value_for_type_tree_node, ObjectDefaultValueLimits, SerializedFieldValue,
    SerializedValue, SerializedValueLookupError,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SerializedAnimationClipError {
    Lookup {
        field: String,
        source: SerializedValueLookupError,
    },
    MissingField {
        field: String,
    },
    AmbiguousAlias {
        aliases: Vec<String>,
    },
    UnexpectedType {
        field: String,
        expected: &'static str,
        actual: String,
    },
    InvalidNumber {
        field: String,
        value: String,
    },
    UnsupportedPointerFileId {
        field: String,
        file_id: i32,
    },
    InconsistentVersionField {
        field: String,
    },
    Mutation {
        field: String,
        message: String,
    },
}

impl Display for SerializedAnimationClipError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lookup { field, source } => {
                write!(formatter, "AnimationClip field {field:?}: {source}")
            }
            Self::MissingField { field } => {
                write!(formatter, "AnimationClip is missing required field {field:?}")
            }
            Self::AmbiguousAlias { aliases } => write!(
                formatter,
                "AnimationClip contains more than one spelling for field aliases {}",
                aliases.join(", ")
            ),
            Self::UnexpectedType {
                field,
                expected,
                actual,
            } => write!(
                formatter,
                "AnimationClip field {field:?} has type {actual}, expected {expected}"
            ),
            Self::InvalidNumber { field, value } => {
                write!(formatter, "AnimationClip field {field:?} has invalid value {value}")
            }
            Self::UnsupportedPointerFileId { field, file_id } => write!(
                formatter,
                "AnimationClip field {field:?} uses FileID {file_id}, which this conversion view cannot represent without collection identity"
            ),
            Self::InconsistentVersionField { field } => write!(
                formatter,
                "AnimationClip field {field:?} is inconsistent with the versioned generated layout"
            ),
            Self::Mutation { field, message } => {
                write!(formatter, "AnimationClip field {field:?} could not be updated: {message}")
            }
        }
    }
}

impl Error for SerializedAnimationClipError {}

/// Read the converter inputs from a decoded generated AnimationClip payload.
///
/// Existing curve records are read because the source processor appends generated curves to
/// these lists. Script pointers are accepted only when their source FileID is local; resolving
/// external scripts requires the caller's collection graph.
pub fn read_animation_clip_for_conversion(
    root: &SerializedValue,
    version: UnityVersion,
    endian: EndianType,
) -> Result<AnimationClipObject, SerializedAnimationClipError> {
    let clip_binding_constant = field(
        root,
        "m_ClipBindingConstant",
        &["m_ClipBindingConstant", "clipBindingConstant"],
    )?;
    let has_clip_binding_constant = clip_binding_constant.is_some();
    let bindings = match clip_binding_constant {
        Some(value) => array_field(
            value,
            "m_ClipBindingConstant.genericBindings",
            &["genericBindings", "GenericBindings"],
        )?
        .iter()
        .enumerate()
        .map(|(index, value)| read_binding(value, index))
        .collect::<Result<Vec<_>, _>>()?,
        None => Vec::new(),
    };
    let pptr_curve_mapping = match clip_binding_constant {
        Some(value) => array_field(
            value,
            "m_ClipBindingConstant.pptrCurveMapping",
            &["pptrCurveMapping", "PptrCurveMapping"],
        )?
        .iter()
        .enumerate()
        .map(|(index, value)| read_optional_pptr(value, &format!("pptrCurveMapping[{index}]")))
        .collect::<Result<Vec<_>, _>>()?,
        None => Vec::new(),
    };

    let muscle = required_field(root, "m_MuscleClip", &["m_MuscleClip", "muscleClip"])?;
    let clip_data = required_field(muscle, "m_MuscleClip.m_Clip", &["m_Clip", "clip"])?;
    let clip_data = required_field(clip_data, "m_MuscleClip.m_Clip.data", &["data", "Data"])?;
    let streamed_value = required_field(
        clip_data,
        "m_MuscleClip.m_Clip.data.m_StreamedClip",
        &["m_StreamedClip", "streamedClip"],
    )?;
    let streamed_data = array_field(streamed_value, "m_StreamedClip.data", &["data", "Data"])?
        .iter()
        .enumerate()
        .map(|(index, value)| read_u32(value, &format!("m_StreamedClip.data[{index}]")))
        .collect::<Result<Vec<_>, _>>()?;
    let mut curve_count_u32 = optional_field(
        streamed_value,
        "m_StreamedClip.curveCount_UInt32",
        &["curveCount_UInt32", "curveCountUInt32"],
    )?
    .map(|value| read_unsigned(value, "m_StreamedClip.curveCount_UInt32"))
    .transpose()?;
    let mut curve_count_u16 = optional_field(
        streamed_value,
        "m_StreamedClip.curveCount_UInt16",
        &["curveCount_UInt16", "curveCountUInt16"],
    )?
    .map(|value| read_unsigned(value, "m_StreamedClip.curveCount_UInt16"))
    .transpose()?
    .unwrap_or(0);
    if curve_count_u32.is_none() && curve_count_u16 == 0 {
        if let Some(value) = optional_field(
            streamed_value,
            "m_StreamedClip.curveCount",
            &["curveCount", "CurveCount"],
        )? {
            match value {
                SerializedValue::U16(_) | SerializedValue::I16(_) => {
                    curve_count_u16 = read_unsigned(value, "m_StreamedClip.curveCount")?;
                }
                _ => {
                    curve_count_u32 = Some(read_unsigned(value, "m_StreamedClip.curveCount")?);
                }
            }
        }
    }
    let discrete_curve_count = optional_field(
        streamed_value,
        "m_StreamedClip.discreteCurveCount",
        &["discreteCurveCount", "DiscreteCurveCount"],
    )?
    .map(|value| read_unsigned(value, "m_StreamedClip.discreteCurveCount"))
    .transpose()?
    .unwrap_or(0);
    let streamed = StreamedClipObject {
        data: streamed_data,
        curve_count_u32: curve_count_u32
            .map(|value| checked_u32(value, "m_StreamedClip.curveCount_UInt32"))
            .transpose()?,
        curve_count_u16: checked_u16(curve_count_u16, "m_StreamedClip.curveCount_UInt16")?,
        discrete_curve_count: checked_u16(
            discrete_curve_count,
            "m_StreamedClip.discreteCurveCount",
        )?,
    };

    let dense_value = required_field(
        clip_data,
        "m_MuscleClip.m_Clip.data.m_DenseClip",
        &["m_DenseClip", "denseClip"],
    )?;
    let dense = DenseClipObject {
        frame_count: read_signed(
            required_field(
                dense_value,
                "m_DenseClip.m_FrameCount",
                &["m_FrameCount", "frameCount"],
            )?,
            "m_DenseClip.m_FrameCount",
        )?,
        curve_count: read_u32(
            required_field(
                dense_value,
                "m_DenseClip.m_CurveCount",
                &["m_CurveCount", "curveCount"],
            )?,
            "m_DenseClip.m_CurveCount",
        )?,
        sample_rate: read_f32(
            required_field(
                dense_value,
                "m_DenseClip.m_SampleRate",
                &["m_SampleRate", "sampleRate"],
            )?,
            "m_DenseClip.m_SampleRate",
        )?,
        begin_time: read_f32(
            required_field(
                dense_value,
                "m_DenseClip.m_BeginTime",
                &["m_BeginTime", "beginTime"],
            )?,
            "m_DenseClip.m_BeginTime",
        )?,
        sample_array: read_f32_array(
            dense_value,
            "m_DenseClip.m_SampleArray",
            &["m_SampleArray", "sampleArray"],
        )?,
    };
    let constant_value = optional_field(
        clip_data,
        "m_MuscleClip.m_Clip.data.m_ConstantClip",
        &["m_ConstantClip", "constantClip"],
    )?;
    let constant = match constant_value {
        Some(value) => ConstantClipObject {
            data: read_f32_array(value, "m_ConstantClip.data", &["data", "Data"])?,
        },
        None => ConstantClipObject::default(),
    };
    let constant_view_scope = optional_field(
        muscle,
        "m_MuscleClipInfo",
        &[
            "m_MuscleClipInfo",
            "m_AnimationClipSettings",
            "muscleClipInfo",
        ],
    )?
    .unwrap_or(muscle);
    let constant_view = read_constant_view(constant_view_scope)?;
    let events = match optional_field(root, "m_Events", &["m_Events", "events"])? {
        Some(SerializedValue::Array(values)) => values
            .iter()
            .enumerate()
            .map(|(index, value)| read_event(value, index))
            .collect::<Result<Vec<_>, _>>()?,
        Some(value @ SerializedValue::Struct(_)) => {
            array_field(value, "m_Events", &["data", "Data"])?
                .iter()
                .enumerate()
                .map(|(index, value)| read_event(value, index))
                .collect::<Result<Vec<_>, _>>()?
        }
        Some(value) => return Err(unexpected_type("m_Events", "array or array wrapper", value)),
        None => Vec::new(),
    };

    Ok(AnimationClipObject {
        version,
        endian,
        has_clip_binding_constant,
        bindings,
        pptr_curve_mapping,
        muscle_clip: MuscleClipObject {
            streamed,
            dense,
            has_constant_clip: constant_value.is_some(),
            constant,
            constant_view,
        },
        muscle_clip_info: Default::default(),
        has_euler_curves: field(root, "m_EulerCurves", &["m_EulerCurves", "eulerCurves"])?
            .is_some(),
        has_pptr_curves: field(root, "m_PPtrCurves", &["m_PPtrCurves", "pptrCurves"])?.is_some(),
        sample_rate: read_f32(
            required_field(root, "m_SampleRate", &["m_SampleRate", "sampleRate"])?,
            "m_SampleRate",
        )?,
        has_legacy: field(root, "m_Legacy", &["m_Legacy", "legacy"])?.is_some(),
        legacy: optional_field(root, "m_Legacy", &["m_Legacy", "legacy"])?
            .map(|value| read_bool(value, "m_Legacy"))
            .transpose()?
            .unwrap_or(false),
        animation_type: optional_field(
            root,
            "m_AnimationType",
            &["m_AnimationType", "animationType"],
        )?
        .map(|value| read_signed(value, "m_AnimationType"))
        .transpose()?
        .unwrap_or(0),
        position_curves: read_vector3_curves(
            root,
            version,
            "m_PositionCurves",
            &["m_PositionCurves", "positionCurves"],
        )?,
        rotation_curves: read_quaternion_curves(
            root,
            version,
            "m_RotationCurves",
            &["m_RotationCurves", "rotationCurves"],
        )?,
        scale_curves: read_vector3_curves(
            root,
            version,
            "m_ScaleCurves",
            &["m_ScaleCurves", "scaleCurves"],
        )?,
        euler_curves: read_vector3_curves(
            root,
            version,
            "m_EulerCurves",
            &["m_EulerCurves", "eulerCurves"],
        )?,
        float_curves: read_float_curves(
            root,
            version,
            "m_FloatCurves",
            &["m_FloatCurves", "floatCurves"],
        )?,
        pptr_curves: read_pptr_curves(root, "m_PPtrCurves", &["m_PPtrCurves", "pptrCurves"])?,
        events,
    })
}

/// Apply the converted curve output to a copy of the source AnimationClip value tree.
///
/// The source processor appends to existing curve lists and initializes `MuscleClipInfo`.
/// This writer preserves existing struct fields and fills newly appended array elements from
/// their actual TypeTree subtrees, so version-specific serialized members remain present.
pub fn write_animation_clip_conversion(
    root: &mut SerializedValue,
    type_tree: &TypeTree,
    object_index: usize,
    converted: &AnimationClipObject,
    max_array_elements: usize,
) -> Result<(), SerializedAnimationClipError> {
    let original = read_animation_clip_for_conversion(root, converted.version, converted.endian)?;
    sync_vector3_curves(
        root,
        type_tree,
        object_index,
        "m_PositionCurves",
        &["m_PositionCurves", "positionCurves"],
        original.position_curves.len(),
        &converted.position_curves,
        max_array_elements,
    )?;
    sync_quaternion_curves(
        root,
        type_tree,
        object_index,
        "m_RotationCurves",
        &["m_RotationCurves", "rotationCurves"],
        original.rotation_curves.len(),
        &converted.rotation_curves,
        max_array_elements,
    )?;
    sync_vector3_curves(
        root,
        type_tree,
        object_index,
        "m_ScaleCurves",
        &["m_ScaleCurves", "scaleCurves"],
        original.scale_curves.len(),
        &converted.scale_curves,
        max_array_elements,
    )?;
    sync_vector3_curves(
        root,
        type_tree,
        object_index,
        "m_EulerCurves",
        &["m_EulerCurves", "eulerCurves"],
        original.euler_curves.len(),
        &converted.euler_curves,
        max_array_elements,
    )?;
    sync_float_curves(
        root,
        type_tree,
        object_index,
        original.float_curves.len(),
        &converted.float_curves,
        max_array_elements,
    )?;
    sync_pptr_curves(
        root,
        type_tree,
        object_index,
        original.pptr_curves.len(),
        &converted.pptr_curves,
        max_array_elements,
    )?;
    write_muscle_clip_info(root, converted.muscle_clip_info)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn sync_vector3_curves(
    root: &mut SerializedValue,
    type_tree: &TypeTree,
    object_index: usize,
    label: &'static str,
    aliases: &'static [&'static str],
    original_count: usize,
    curves: &[Vector3CurveObject],
    max_array_elements: usize,
) -> Result<(), SerializedAnimationClipError> {
    let Some(array_node) = type_tree_field_node(type_tree, aliases, label)? else {
        if curves.is_empty() {
            return Ok(());
        }
        return Err(mutation_error(label, "TypeTree array field is absent"));
    };
    let element_node = type_tree_array_element_node(type_tree, array_node, label)?;
    let curve_node =
        type_tree_subtree_field_node(type_tree, element_node, &["curve", "Curve"], label)?;
    let key_node = type_tree_array_element_node(
        type_tree,
        type_tree_subtree_field_node(type_tree, curve_node, &["m_Curve", "curve", "Curve"], label)?,
        label,
    )?;
    let values = array_field_mut(root, aliases, label)?;
    let Some(values) = values else {
        if curves.is_empty() {
            return Ok(());
        }
        return Err(mutation_error(label, "serialized curve array is absent"));
    };
    validate_curve_count(label, values.len(), original_count, curves.len())?;
    if curves.len() > max_array_elements {
        return Err(mutation_error(
            label,
            format!(
                "curve count {} exceeds limit {max_array_elements}",
                curves.len()
            ),
        ));
    }
    for (index, curve) in curves.iter().enumerate() {
        if index == values.len() {
            let mut value = default_value_for_type_tree_node(
                type_tree,
                object_index,
                element_node,
                ObjectDefaultValueLimits::default(),
            )
            .map_err(|error| mutation_error(label, error.to_string()))?;
            write_vector3_curve(
                &mut value,
                type_tree,
                object_index,
                key_node,
                curve,
                0,
                max_array_elements,
                label,
            )?;
            values
                .try_reserve(1)
                .map_err(|_| mutation_error(label, "could not allocate curve array"))?;
            values.push(value);
        } else {
            let existing_key_count = curve_key_array_len(&values[index], label)?;
            write_vector3_curve(
                &mut values[index],
                type_tree,
                object_index,
                key_node,
                curve,
                existing_key_count,
                max_array_elements,
                label,
            )?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn sync_quaternion_curves(
    root: &mut SerializedValue,
    type_tree: &TypeTree,
    object_index: usize,
    label: &'static str,
    aliases: &'static [&'static str],
    original_count: usize,
    curves: &[QuaternionCurveObject],
    max_array_elements: usize,
) -> Result<(), SerializedAnimationClipError> {
    let Some(array_node) = type_tree_field_node(type_tree, aliases, label)? else {
        if curves.is_empty() {
            return Ok(());
        }
        return Err(mutation_error(label, "TypeTree array field is absent"));
    };
    let element_node = type_tree_array_element_node(type_tree, array_node, label)?;
    let curve_node =
        type_tree_subtree_field_node(type_tree, element_node, &["curve", "Curve"], label)?;
    let key_array_node =
        type_tree_subtree_field_node(type_tree, curve_node, &["m_Curve", "curve", "Curve"], label)?;
    let key_node = type_tree_array_element_node(type_tree, key_array_node, label)?;
    let values = array_field_mut(root, aliases, label)?;
    let Some(values) = values else {
        if curves.is_empty() {
            return Ok(());
        }
        return Err(mutation_error(label, "serialized curve array is absent"));
    };
    validate_curve_count(label, values.len(), original_count, curves.len())?;
    if curves.len() > max_array_elements {
        return Err(mutation_error(
            label,
            format!("curve count exceeds limit {max_array_elements}"),
        ));
    }
    for (index, curve) in curves.iter().enumerate() {
        if index == values.len() {
            let mut value = default_value_for_type_tree_node(
                type_tree,
                object_index,
                element_node,
                ObjectDefaultValueLimits::default(),
            )
            .map_err(|error| mutation_error(label, error.to_string()))?;
            write_quaternion_curve(
                &mut value,
                type_tree,
                object_index,
                key_node,
                curve,
                0,
                max_array_elements,
                label,
            )?;
            values
                .try_reserve(1)
                .map_err(|_| mutation_error(label, "could not allocate curve array"))?;
            values.push(value);
        } else {
            let existing_key_count = curve_key_array_len(&values[index], label)?;
            write_quaternion_curve(
                &mut values[index],
                type_tree,
                object_index,
                key_node,
                curve,
                existing_key_count,
                max_array_elements,
                label,
            )?;
        }
    }
    Ok(())
}

fn sync_float_curves(
    root: &mut SerializedValue,
    type_tree: &TypeTree,
    object_index: usize,
    original_count: usize,
    curves: &[FloatCurveObject],
    max_array_elements: usize,
) -> Result<(), SerializedAnimationClipError> {
    const LABEL: &str = "m_FloatCurves";
    const ALIASES: &[&str] = &["m_FloatCurves", "floatCurves"];
    let Some(array_node) = type_tree_field_node(type_tree, ALIASES, LABEL)? else {
        if curves.is_empty() {
            return Ok(());
        }
        return Err(mutation_error(LABEL, "TypeTree array field is absent"));
    };
    let element_node = type_tree_array_element_node(type_tree, array_node, LABEL)?;
    let curve_node =
        type_tree_subtree_field_node(type_tree, element_node, &["curve", "Curve"], LABEL)?;
    let key_array_node =
        type_tree_subtree_field_node(type_tree, curve_node, &["m_Curve", "curve", "Curve"], LABEL)?;
    let key_node = type_tree_array_element_node(type_tree, key_array_node, LABEL)?;
    let values = array_field_mut(root, ALIASES, LABEL)?;
    let Some(values) = values else {
        if curves.is_empty() {
            return Ok(());
        }
        return Err(mutation_error(LABEL, "serialized curve array is absent"));
    };
    validate_curve_count(LABEL, values.len(), original_count, curves.len())?;
    if curves.len() > max_array_elements {
        return Err(mutation_error(
            LABEL,
            format!("curve count exceeds limit {max_array_elements}"),
        ));
    }
    for (index, curve) in curves.iter().enumerate() {
        if index == values.len() {
            let mut value = default_value_for_type_tree_node(
                type_tree,
                object_index,
                element_node,
                ObjectDefaultValueLimits::default(),
            )
            .map_err(|error| mutation_error(LABEL, error.to_string()))?;
            write_float_curve(
                &mut value,
                type_tree,
                object_index,
                key_node,
                curve,
                0,
                max_array_elements,
                LABEL,
            )?;
            values
                .try_reserve(1)
                .map_err(|_| mutation_error(LABEL, "could not allocate curve array"))?;
            values.push(value);
        } else {
            let existing_key_count = curve_key_array_len(&values[index], LABEL)?;
            write_float_curve(
                &mut values[index],
                type_tree,
                object_index,
                key_node,
                curve,
                existing_key_count,
                max_array_elements,
                LABEL,
            )?;
        }
    }
    Ok(())
}

fn sync_pptr_curves(
    root: &mut SerializedValue,
    type_tree: &TypeTree,
    object_index: usize,
    original_count: usize,
    curves: &[PPtrCurveObject],
    max_array_elements: usize,
) -> Result<(), SerializedAnimationClipError> {
    const LABEL: &str = "m_PPtrCurves";
    const ALIASES: &[&str] = &["m_PPtrCurves", "pptrCurves"];
    let Some(array_node) = type_tree_field_node(type_tree, ALIASES, LABEL)? else {
        if curves.is_empty() {
            return Ok(());
        }
        return Err(mutation_error(LABEL, "TypeTree array field is absent"));
    };
    let element_node = type_tree_array_element_node(type_tree, array_node, LABEL)?;
    let key_array_node =
        type_tree_subtree_field_node(type_tree, element_node, &["curve", "Curve"], LABEL)?;
    let key_node = type_tree_array_element_node(type_tree, key_array_node, LABEL)?;
    let values = array_field_mut(root, ALIASES, LABEL)?;
    let Some(values) = values else {
        if curves.is_empty() {
            return Ok(());
        }
        return Err(mutation_error(LABEL, "serialized curve array is absent"));
    };
    validate_curve_count(LABEL, values.len(), original_count, curves.len())?;
    if curves.len() > max_array_elements {
        return Err(mutation_error(
            LABEL,
            format!("curve count exceeds limit {max_array_elements}"),
        ));
    }
    for (index, curve) in curves.iter().enumerate() {
        if index == values.len() {
            let mut value = default_value_for_type_tree_node(
                type_tree,
                object_index,
                element_node,
                ObjectDefaultValueLimits::default(),
            )
            .map_err(|error| mutation_error(LABEL, error.to_string()))?;
            write_pptr_curve(
                &mut value,
                type_tree,
                object_index,
                key_node,
                curve,
                0,
                max_array_elements,
                LABEL,
            )?;
            values
                .try_reserve(1)
                .map_err(|_| mutation_error(LABEL, "could not allocate curve array"))?;
            values.push(value);
        } else {
            let existing_key_count = pptr_curve_key_array_len(&values[index], LABEL)?;
            write_pptr_curve(
                &mut values[index],
                type_tree,
                object_index,
                key_node,
                curve,
                existing_key_count,
                max_array_elements,
                LABEL,
            )?;
        }
    }
    Ok(())
}

fn validate_curve_count(
    field: &'static str,
    serialized_count: usize,
    original_count: usize,
    converted_count: usize,
) -> Result<(), SerializedAnimationClipError> {
    if serialized_count != original_count {
        return Err(mutation_error(
            field,
            format!(
                "serialized curve count {serialized_count} differs from the decoded source count {original_count}"
            ),
        ));
    }
    if converted_count < original_count {
        return Err(mutation_error(
            field,
            format!("converter removed curves: {original_count} source, {converted_count} output"),
        ));
    }
    Ok(())
}

fn curve_key_array_len(
    curve: &SerializedValue,
    field: &'static str,
) -> Result<usize, SerializedAnimationClipError> {
    let Some(curve_value) = field_mut_alias_name(curve, &["curve", "Curve"], field)? else {
        return Err(mutation_error(field, "curve record has no curve field"));
    };
    let curve_value = curve
        .unique_field_in_base_chain(curve_value)
        .map_err(|error| mutation_error(field, error.to_string()))?
        .ok_or_else(|| mutation_error(field, "curve field disappeared"))?;
    let Some(key_name) = field_mut_alias_name(curve_value, &["m_Curve", "curve", "Curve"], field)?
    else {
        return Err(mutation_error(field, "animation curve has no key array"));
    };
    let key_array = curve_value
        .unique_field_in_base_chain(key_name)
        .map_err(|error| mutation_error(field, error.to_string()))?
        .ok_or_else(|| mutation_error(field, "key array disappeared"))?;
    match key_array {
        SerializedValue::Array(keys) => Ok(keys.len()),
        _ => Err(mutation_error(field, "curve key field is not an array")),
    }
}

fn pptr_curve_key_array_len(
    curve: &SerializedValue,
    field: &'static str,
) -> Result<usize, SerializedAnimationClipError> {
    let Some(key_name) = field_mut_alias_name(curve, &["curve", "Curve"], field)? else {
        return Err(mutation_error(field, "PPtr curve record has no key array"));
    };
    let key_array = curve
        .unique_field_in_base_chain(key_name)
        .map_err(|error| mutation_error(field, error.to_string()))?
        .ok_or_else(|| mutation_error(field, "PPtr key array disappeared"))?;
    match key_array {
        SerializedValue::Array(keys) => Ok(keys.len()),
        _ => Err(mutation_error(
            field,
            "PPtr curve key field is not an array",
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn write_vector3_curve(
    value: &mut SerializedValue,
    type_tree: &TypeTree,
    object_index: usize,
    key_node: usize,
    curve: &Vector3CurveObject,
    original_key_count: usize,
    max_array_elements: usize,
    field: &'static str,
) -> Result<(), SerializedAnimationClipError> {
    set_string_field(value, &["path", "Path"], &curve.path, field)?;
    let curve_value = required_field_mut(value, &["curve", "Curve"], field)?;
    write_curve_envelope(curve_value, curve.envelope, field)?;
    let keys = required_array_field_mut(curve_value, &["m_Curve", "curve", "Curve"], field)?;
    sync_key_count(
        field,
        keys.len(),
        original_key_count,
        curve.curve.len(),
        max_array_elements,
    )?;
    for (index, key) in curve.curve.iter().enumerate() {
        if index == keys.len() {
            let mut value = default_value_for_type_tree_node(
                type_tree,
                object_index,
                key_node,
                ObjectDefaultValueLimits::default(),
            )
            .map_err(|error| mutation_error(field, error.to_string()))?;
            write_vector3_key(&mut value, *key, field)?;
            keys.try_reserve(1)
                .map_err(|_| mutation_error(field, "could not allocate key array"))?;
            keys.push(value);
        } else {
            write_vector3_key(&mut keys[index], *key, field)?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn write_quaternion_curve(
    value: &mut SerializedValue,
    type_tree: &TypeTree,
    object_index: usize,
    key_node: usize,
    curve: &QuaternionCurveObject,
    original_key_count: usize,
    max_array_elements: usize,
    field: &'static str,
) -> Result<(), SerializedAnimationClipError> {
    set_string_field(value, &["path", "Path"], &curve.path, field)?;
    let curve_value = required_field_mut(value, &["curve", "Curve"], field)?;
    write_curve_envelope(curve_value, curve.envelope, field)?;
    let keys = required_array_field_mut(curve_value, &["m_Curve", "curve", "Curve"], field)?;
    sync_key_count(
        field,
        keys.len(),
        original_key_count,
        curve.curve.len(),
        max_array_elements,
    )?;
    for (index, key) in curve.curve.iter().enumerate() {
        if index == keys.len() {
            let mut value = default_value_for_type_tree_node(
                type_tree,
                object_index,
                key_node,
                ObjectDefaultValueLimits::default(),
            )
            .map_err(|error| mutation_error(field, error.to_string()))?;
            write_quaternion_key(&mut value, *key, field)?;
            keys.try_reserve(1)
                .map_err(|_| mutation_error(field, "could not allocate key array"))?;
            keys.push(value);
        } else {
            write_quaternion_key(&mut keys[index], *key, field)?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn write_float_curve(
    value: &mut SerializedValue,
    type_tree: &TypeTree,
    object_index: usize,
    key_node: usize,
    curve: &FloatCurveObject,
    original_key_count: usize,
    max_array_elements: usize,
    field: &'static str,
) -> Result<(), SerializedAnimationClipError> {
    set_string_field(value, &["path", "Path"], &curve.path, field)?;
    set_string_field(value, &["attribute", "Attribute"], &curve.attribute, field)?;
    set_signed_field(
        value,
        &["classID", "ClassID"],
        i64::from(curve.class_id),
        field,
    )?;
    set_pointer_field_if_present(
        value,
        &["script", "Script"],
        curve.script.map(|script| PPtrValue {
            file_id: script.file_id,
            path_id: script.path_id,
        }),
        field,
    )?;
    set_signed_field_if_present(value, &["flags", "Flags"], i64::from(curve.flags), field)?;
    let curve_value = required_field_mut(value, &["curve", "Curve"], field)?;
    write_curve_envelope(curve_value, curve.envelope, field)?;
    let keys = required_array_field_mut(curve_value, &["m_Curve", "curve", "Curve"], field)?;
    sync_key_count(
        field,
        keys.len(),
        original_key_count,
        curve.curve.len(),
        max_array_elements,
    )?;
    for (index, key) in curve.curve.iter().enumerate() {
        if index == keys.len() {
            let mut value = default_value_for_type_tree_node(
                type_tree,
                object_index,
                key_node,
                ObjectDefaultValueLimits::default(),
            )
            .map_err(|error| mutation_error(field, error.to_string()))?;
            write_float_key(&mut value, *key, field)?;
            keys.try_reserve(1)
                .map_err(|_| mutation_error(field, "could not allocate key array"))?;
            keys.push(value);
        } else {
            write_float_key(&mut keys[index], *key, field)?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn write_pptr_curve(
    value: &mut SerializedValue,
    type_tree: &TypeTree,
    object_index: usize,
    key_node: usize,
    curve: &PPtrCurveObject,
    original_key_count: usize,
    max_array_elements: usize,
    field: &'static str,
) -> Result<(), SerializedAnimationClipError> {
    set_string_field(value, &["path", "Path"], &curve.path, field)?;
    set_string_field(value, &["attribute", "Attribute"], &curve.attribute, field)?;
    set_signed_field(
        value,
        &["classID", "ClassID"],
        i64::from(curve.class_id),
        field,
    )?;
    set_pointer_field_if_present(
        value,
        &["script", "Script"],
        curve.script.map(|script| PPtrValue {
            file_id: script.file_id,
            path_id: script.path_id,
        }),
        field,
    )?;
    set_signed_field_if_present(value, &["flags", "Flags"], i64::from(curve.flags), field)?;
    let keys = required_array_field_mut(value, &["curve", "Curve"], field)?;
    sync_key_count(
        field,
        keys.len(),
        original_key_count,
        curve.curve.len(),
        max_array_elements,
    )?;
    for (index, key) in curve.curve.iter().enumerate() {
        if index == keys.len() {
            let mut value = default_value_for_type_tree_node(
                type_tree,
                object_index,
                key_node,
                ObjectDefaultValueLimits::default(),
            )
            .map_err(|error| mutation_error(field, error.to_string()))?;
            write_pptr_key(&mut value, *key, field)?;
            keys.try_reserve(1)
                .map_err(|_| mutation_error(field, "could not allocate key array"))?;
            keys.push(value);
        } else {
            write_pptr_key(&mut keys[index], *key, field)?;
        }
    }
    Ok(())
}

fn sync_key_count(
    field: &'static str,
    serialized_count: usize,
    original_count: usize,
    converted_count: usize,
    maximum: usize,
) -> Result<(), SerializedAnimationClipError> {
    if serialized_count != original_count {
        return Err(mutation_error(
            field,
            format!(
                "serialized key count {serialized_count} differs from the decoded source count {original_count}"
            ),
        ));
    }
    if converted_count < original_count {
        return Err(mutation_error(
            field,
            format!("converter removed keys: {original_count} source, {converted_count} output"),
        ));
    }
    if converted_count > maximum {
        return Err(mutation_error(
            field,
            format!("key count {converted_count} exceeds limit {maximum}"),
        ));
    }
    Ok(())
}

fn write_curve_envelope(
    curve: &mut SerializedValue,
    envelope: CurveEnvelope,
    field: &'static str,
) -> Result<(), SerializedAnimationClipError> {
    set_signed_field(
        curve,
        &["m_PreInfinity", "preInfinity", "PreInfinity"],
        i64::from(envelope.pre_infinity),
        field,
    )?;
    set_signed_field(
        curve,
        &["m_PostInfinity", "postInfinity", "PostInfinity"],
        i64::from(envelope.post_infinity),
        field,
    )?;
    if let Some(rotation_order) = envelope.rotation_order {
        set_signed_field(
            curve,
            &["m_RotationOrder", "rotationOrder", "RotationOrder"],
            i64::from(rotation_order),
            field,
        )?;
    }
    Ok(())
}

fn write_vector3_key(
    key: &mut SerializedValue,
    value: KeyframeVector3View,
    field: &'static str,
) -> Result<(), SerializedAnimationClipError> {
    set_float_field(key, &["time", "Time"], value.time, field)?;
    set_vec3_field(key, &["value", "Value"], value.value, field)?;
    set_vec3_field(key, &["inSlope", "InSlope"], value.in_slope, field)?;
    set_vec3_field(key, &["outSlope", "OutSlope"], value.out_slope, field)?;
    set_keyframe_tangent_mode(key, value.tangent_mode, field)?;
    if let Some(weight) = value.in_weight {
        set_vec3_field(key, &["inWeight", "InWeight"], weight, field)?;
    }
    if let Some(weight) = value.out_weight {
        set_vec3_field(key, &["outWeight", "OutWeight"], weight, field)?;
    }
    if let Some(weighted_mode) = value.weighted_mode {
        set_signed_field(
            key,
            &["weightedMode", "WeightedMode"],
            i64::from(weighted_mode),
            field,
        )?;
    }
    Ok(())
}

fn write_quaternion_key(
    key: &mut SerializedValue,
    value: KeyframeQuaternionView,
    field: &'static str,
) -> Result<(), SerializedAnimationClipError> {
    set_float_field(key, &["time", "Time"], value.time, field)?;
    set_quaternion_field(key, &["value", "Value"], value.value, field)?;
    set_quaternion_field(key, &["inSlope", "InSlope"], value.in_slope, field)?;
    set_quaternion_field(key, &["outSlope", "OutSlope"], value.out_slope, field)?;
    set_keyframe_tangent_mode(key, value.tangent_mode, field)?;
    if let Some(weight) = value.in_weight {
        set_quaternion_field(key, &["inWeight", "InWeight"], weight, field)?;
    }
    if let Some(weight) = value.out_weight {
        set_quaternion_field(key, &["outWeight", "OutWeight"], weight, field)?;
    }
    if let Some(weighted_mode) = value.weighted_mode {
        set_signed_field(
            key,
            &["weightedMode", "WeightedMode"],
            i64::from(weighted_mode),
            field,
        )?;
    }
    Ok(())
}

fn write_float_key(
    key: &mut SerializedValue,
    value: KeyframeView,
    field: &'static str,
) -> Result<(), SerializedAnimationClipError> {
    set_float_field(key, &["time", "Time"], value.time, field)?;
    set_float_field(key, &["value", "Value"], value.value, field)?;
    set_float_field(key, &["inSlope", "InSlope"], value.in_slope, field)?;
    set_float_field(key, &["outSlope", "OutSlope"], value.out_slope, field)?;
    set_keyframe_tangent_mode(key, value.tangent_mode, field)?;
    if let Some(weight) = value.in_weight {
        set_float_field(key, &["inWeight", "InWeight"], weight, field)?;
    }
    if let Some(weight) = value.out_weight {
        set_float_field(key, &["outWeight", "OutWeight"], weight, field)?;
    }
    if let Some(weighted_mode) = value.weighted_mode {
        set_signed_field(
            key,
            &["weightedMode", "WeightedMode"],
            i64::from(weighted_mode),
            field,
        )?;
    }
    Ok(())
}

fn write_pptr_key(
    key: &mut SerializedValue,
    value: PPtrKeyframeObject,
    field: &'static str,
) -> Result<(), SerializedAnimationClipError> {
    set_float_field(key, &["time", "Time"], value.time, field)?;
    set_pointer_field(key, &["value", "Value"], value.value, field)
}

fn write_muscle_clip_info(
    root: &mut SerializedValue,
    view: crate::resources::muscle_clip_info::MuscleClipInfoView,
) -> Result<(), SerializedAnimationClipError> {
    let Some(muscle) = optional_field_mut(root, &["m_MuscleClip", "muscleClip"], "m_MuscleClip")?
    else {
        return Ok(());
    };
    let Some(serialized_info) = optional_field_mut(
        muscle,
        &[
            "m_MuscleClipInfo",
            "m_AnimationClipSettings",
            "muscleClipInfo",
        ],
        "m_MuscleClipInfo",
    )?
    else {
        return Ok(());
    };
    for (aliases, value) in [
        (
            &["startTime", "StartTime", "m_StartTime"][..],
            view.start_time,
        ),
        (&["stopTime", "StopTime", "m_StopTime"][..], view.stop_time),
        (
            &[
                "orientationOffsetY",
                "OrientationOffsetY",
                "m_OrientationOffsetY",
            ][..],
            view.orientation_offset_y,
        ),
        (&["level", "Level", "m_Level"][..], view.level),
        (
            &["cycleOffset", "CycleOffset", "m_CycleOffset"][..],
            view.cycle_offset,
        ),
        (
            &[
                "additiveReferencePoseTime",
                "AdditiveReferencePoseTime",
                "m_AdditiveReferencePoseTime",
            ][..],
            view.additive_reference_pose_time,
        ),
    ] {
        set_float_field_if_present(serialized_info, aliases, value, "m_MuscleClipInfo")?;
    }
    for (aliases, value) in [
        (&["loopTime", "LoopTime", "m_LoopTime"][..], view.loop_time),
        (
            &["loopBlend", "LoopBlend", "m_LoopBlend"][..],
            view.loop_blend,
        ),
        (
            &[
                "loopBlendOrientation",
                "LoopBlendOrientation",
                "m_LoopBlendOrientation",
            ][..],
            view.loop_blend_orientation,
        ),
        (
            &[
                "loopBlendPositionY",
                "LoopBlendPositionY",
                "m_LoopBlendPositionY",
            ][..],
            view.loop_blend_position_y,
        ),
        (
            &[
                "loopBlendPositionXZ",
                "LoopBlendPositionXZ",
                "m_LoopBlendPositionXZ",
            ][..],
            view.loop_blend_position_xz,
        ),
        (
            &[
                "keepOriginalOrientation",
                "KeepOriginalOrientation",
                "m_KeepOriginalOrientation",
            ][..],
            view.keep_original_orientation,
        ),
        (
            &[
                "keepOriginalPositionY",
                "KeepOriginalPositionY",
                "m_KeepOriginalPositionY",
            ][..],
            view.keep_original_position_y,
        ),
        (
            &[
                "keepOriginalPositionXZ",
                "KeepOriginalPositionXZ",
                "m_KeepOriginalPositionXZ",
            ][..],
            view.keep_original_position_xz,
        ),
        (
            &["heightFromFeet", "HeightFromFeet", "m_HeightFromFeet"][..],
            view.height_from_feet,
        ),
        (
            &[
                "hasAdditiveReferencePose",
                "HasAdditiveReferencePose",
                "m_HasAdditiveReferencePose",
            ][..],
            view.has_additive_reference_pose,
        ),
        (&["mirror", "Mirror", "m_Mirror"][..], view.mirror),
    ] {
        set_bool_field_if_present(serialized_info, aliases, value, "m_MuscleClipInfo")?;
    }
    Ok(())
}

fn field_mut_alias_name(
    value: &SerializedValue,
    aliases: &'static [&'static str],
    field: &'static str,
) -> Result<Option<&'static str>, SerializedAnimationClipError> {
    let mut found = None;
    for &alias in aliases {
        if value
            .unique_field_in_base_chain(alias)
            .map_err(|source| SerializedAnimationClipError::Lookup {
                field: field.to_owned(),
                source,
            })?
            .is_some()
        {
            if found.is_some() {
                return Err(SerializedAnimationClipError::AmbiguousAlias {
                    aliases: aliases.iter().map(|name| (*name).to_owned()).collect(),
                });
            }
            found = Some(alias);
        }
    }
    Ok(found)
}

fn required_field_mut<'a>(
    value: &'a mut SerializedValue,
    aliases: &'static [&'static str],
    field: &'static str,
) -> Result<&'a mut SerializedValue, SerializedAnimationClipError> {
    let alias = field_mut_alias_name(value, aliases, field)?.ok_or_else(|| {
        SerializedAnimationClipError::MissingField {
            field: field.to_owned(),
        }
    })?;
    value
        .unique_field_in_base_chain_mut(alias)
        .map_err(|source| SerializedAnimationClipError::Mutation {
            field: field.to_owned(),
            message: source.to_string(),
        })?
        .ok_or_else(|| SerializedAnimationClipError::MissingField {
            field: field.to_owned(),
        })
}

fn optional_field_mut<'a>(
    value: &'a mut SerializedValue,
    aliases: &'static [&'static str],
    field: &'static str,
) -> Result<Option<&'a mut SerializedValue>, SerializedAnimationClipError> {
    let Some(alias) = field_mut_alias_name(value, aliases, field)? else {
        return Ok(None);
    };
    value
        .unique_field_in_base_chain_mut(alias)
        .map_err(|source| SerializedAnimationClipError::Mutation {
            field: field.to_owned(),
            message: source.to_string(),
        })
}

fn array_field_mut<'a>(
    value: &'a mut SerializedValue,
    aliases: &'static [&'static str],
    field: &'static str,
) -> Result<Option<&'a mut Vec<SerializedValue>>, SerializedAnimationClipError> {
    let Some(value) = optional_field_mut(value, aliases, field)? else {
        return Ok(None);
    };
    match value {
        SerializedValue::Array(values) => Ok(Some(values)),
        _ => Err(mutation_error(
            field,
            "serialized curve field is not an array",
        )),
    }
}

fn required_array_field_mut<'a>(
    value: &'a mut SerializedValue,
    aliases: &'static [&'static str],
    field: &'static str,
) -> Result<&'a mut Vec<SerializedValue>, SerializedAnimationClipError> {
    let value = required_field_mut(value, aliases, field)?;
    match value {
        SerializedValue::Array(values) => Ok(values),
        _ => Err(mutation_error(
            field,
            "serialized key field is not an array",
        )),
    }
}

fn set_string_field(
    value: &mut SerializedValue,
    aliases: &'static [&'static str],
    new_value: &str,
    field: &'static str,
) -> Result<(), SerializedAnimationClipError> {
    let target = required_field_mut(value, aliases, field)?;
    match target {
        SerializedValue::StringBytes(bytes) => {
            *bytes = new_value.as_bytes().to_vec();
            Ok(())
        }
        other => Err(mutation_error(
            field,
            format!(
                "field has type {}, expected string",
                serialized_type_name(other)
            ),
        )),
    }
}

fn set_signed_field(
    value: &mut SerializedValue,
    aliases: &'static [&'static str],
    new_value: i64,
    field: &'static str,
) -> Result<(), SerializedAnimationClipError> {
    set_signed_field_if_present(value, aliases, new_value, field)?
        .then_some(())
        .ok_or_else(|| {
            mutation_error(
                field,
                format!("required field {} is absent", aliases.join(" or ")),
            )
        })
}

fn set_signed_field_if_present(
    value: &mut SerializedValue,
    aliases: &'static [&'static str],
    new_value: i64,
    field: &'static str,
) -> Result<bool, SerializedAnimationClipError> {
    let Some(target) = optional_field_mut(value, aliases, field)? else {
        return Ok(false);
    };
    match target {
        SerializedValue::I8(slot) => {
            *slot = i8::try_from(new_value)
                .map_err(|_| mutation_error(field, "value does not fit i8"))?
        }
        SerializedValue::U8(slot) => {
            *slot = u8::try_from(new_value)
                .map_err(|_| mutation_error(field, "value does not fit u8"))?
        }
        SerializedValue::I16(slot) => {
            *slot = i16::try_from(new_value)
                .map_err(|_| mutation_error(field, "value does not fit i16"))?
        }
        SerializedValue::U16(slot) => {
            *slot = u16::try_from(new_value)
                .map_err(|_| mutation_error(field, "value does not fit u16"))?
        }
        SerializedValue::I32(slot) => {
            *slot = i32::try_from(new_value)
                .map_err(|_| mutation_error(field, "value does not fit i32"))?
        }
        SerializedValue::U32(slot) => {
            *slot = u32::try_from(new_value)
                .map_err(|_| mutation_error(field, "value does not fit u32"))?
        }
        SerializedValue::I64(slot) => *slot = new_value,
        SerializedValue::U64(slot) => {
            *slot = u64::try_from(new_value)
                .map_err(|_| mutation_error(field, "value does not fit u64"))?
        }
        other => {
            return Err(mutation_error(
                field,
                format!(
                    "field has type {}, expected integer",
                    serialized_type_name(other)
                ),
            ))
        }
    }
    Ok(true)
}

/// Keep the generated keyframe `TangentMode` property in editor-format output when a recovered
/// TypeTree omits it. AssetRipper's `IKeyframe_*` interfaces expose this property unconditionally;
/// `KeyframeExtensions.SetValues` assigns it when the converter creates a key.
fn set_keyframe_tangent_mode(
    key: &mut SerializedValue,
    tangent_mode: i32,
    field: &'static str,
) -> Result<(), SerializedAnimationClipError> {
    const TANGENT_MODE: &[&str] = &["tangentMode", "TangentMode"];
    if field_mut_alias_name(key, TANGENT_MODE, field)?.is_some() {
        set_signed_field(key, TANGENT_MODE, i64::from(tangent_mode), field)?;
        return Ok(());
    }
    let anchor = field_mut_alias_name(key, &["outSlope", "OutSlope"], field)?.ok_or_else(|| {
        mutation_error(
            field,
            "cannot insert generated tangentMode without an outSlope field",
        )
    })?;
    insert_i32_field_after_base_chain(key, anchor, "tangentMode", tangent_mode, field)
}

fn insert_i32_field_after_base_chain(
    value: &mut SerializedValue,
    anchor: &'static str,
    name: &'static str,
    field_value: i32,
    field: &'static str,
) -> Result<(), SerializedAnimationClipError> {
    let SerializedValue::Struct(fields) = value else {
        return Err(mutation_error(
            field,
            "cannot insert a generated keyframe field into a non-struct value",
        ));
    };
    let mut anchor_index = None;
    let mut base_index = None;
    for (index, child) in fields.iter().enumerate() {
        if child.name == anchor {
            if anchor_index.replace(index).is_some() {
                return Err(SerializedAnimationClipError::Lookup {
                    field: field.to_owned(),
                    source: SerializedValueLookupError::DuplicateField {
                        name: anchor.to_owned(),
                        match_count: 2,
                    },
                });
            }
        } else if child.name == "Base" && base_index.replace(index).is_some() {
            return Err(SerializedAnimationClipError::Lookup {
                field: field.to_owned(),
                source: SerializedValueLookupError::DuplicateField {
                    name: "Base".to_owned(),
                    match_count: 2,
                },
            });
        }
    }
    if let Some(index) = anchor_index {
        fields
            .try_reserve(1)
            .map_err(|_| mutation_error(field, "could not allocate a keyframe field"))?;
        fields.insert(
            index + 1,
            SerializedFieldValue {
                name: name.to_owned(),
                value: SerializedValue::I32(field_value),
            },
        );
        return Ok(());
    }
    let Some(index) = base_index else {
        return Err(mutation_error(
            field,
            format!("keyframe has no {anchor:?} field in its Base chain"),
        ));
    };
    insert_i32_field_after_base_chain(&mut fields[index].value, anchor, name, field_value, field)
}

fn set_float_field(
    value: &mut SerializedValue,
    aliases: &'static [&'static str],
    new_value: f32,
    field: &'static str,
) -> Result<(), SerializedAnimationClipError> {
    let target = required_field_mut(value, aliases, field)?;
    match target {
        SerializedValue::F32(slot) => *slot = new_value,
        SerializedValue::F64(slot) => *slot = f64::from(new_value),
        other => {
            return Err(mutation_error(
                field,
                format!(
                    "field has type {}, expected float",
                    serialized_type_name(other)
                ),
            ))
        }
    }
    Ok(())
}

fn set_float_field_if_present(
    value: &mut SerializedValue,
    aliases: &'static [&'static str],
    new_value: f32,
    field: &'static str,
) -> Result<(), SerializedAnimationClipError> {
    if field_mut_alias_name(value, aliases, field)?.is_some() {
        set_float_field(value, aliases, new_value, field)?;
    }
    Ok(())
}

fn set_bool_field(
    value: &mut SerializedValue,
    aliases: &'static [&'static str],
    new_value: bool,
    field: &'static str,
) -> Result<(), SerializedAnimationClipError> {
    let target = required_field_mut(value, aliases, field)?;
    match target {
        SerializedValue::Bool(slot) => *slot = new_value,
        other => {
            return Err(mutation_error(
                field,
                format!(
                    "field has type {}, expected boolean",
                    serialized_type_name(other)
                ),
            ))
        }
    }
    Ok(())
}

fn set_bool_field_if_present(
    value: &mut SerializedValue,
    aliases: &'static [&'static str],
    new_value: bool,
    field: &'static str,
) -> Result<(), SerializedAnimationClipError> {
    if field_mut_alias_name(value, aliases, field)?.is_some() {
        set_bool_field(value, aliases, new_value, field)?;
    }
    Ok(())
}

fn set_pointer_field(
    value: &mut SerializedValue,
    aliases: &'static [&'static str],
    new_value: Option<PPtrValue>,
    field: &'static str,
) -> Result<(), SerializedAnimationClipError> {
    let target = required_field_mut(value, aliases, field)?;
    let new_value = new_value.unwrap_or_default();
    match target {
        SerializedValue::PPtr { file_id, path_id } => {
            *file_id = new_value.file_id;
            *path_id = new_value.path_id;
        }
        other => {
            return Err(mutation_error(
                field,
                format!(
                    "field has type {}, expected PPtr",
                    serialized_type_name(other)
                ),
            ))
        }
    }
    Ok(())
}

fn set_pointer_field_if_present(
    value: &mut SerializedValue,
    aliases: &'static [&'static str],
    new_value: Option<PPtrValue>,
    field: &'static str,
) -> Result<(), SerializedAnimationClipError> {
    if field_mut_alias_name(value, aliases, field)?.is_some() {
        set_pointer_field(value, aliases, new_value, field)?;
    }
    Ok(())
}

fn set_vec3_field(
    value: &mut SerializedValue,
    aliases: &'static [&'static str],
    new_value: Vec3,
    field: &'static str,
) -> Result<(), SerializedAnimationClipError> {
    let target = required_field_mut(value, aliases, field)?;
    for (names, component) in [
        (&["x", "X", "m_X"][..], new_value.x),
        (&["y", "Y", "m_Y"][..], new_value.y),
        (&["z", "Z", "m_Z"][..], new_value.z),
    ] {
        set_float_field(target, names, component, field)?;
    }
    Ok(())
}

fn set_quaternion_field(
    value: &mut SerializedValue,
    aliases: &'static [&'static str],
    new_value: Quat,
    field: &'static str,
) -> Result<(), SerializedAnimationClipError> {
    let target = required_field_mut(value, aliases, field)?;
    for (names, component) in [
        (&["x", "X", "m_X"][..], new_value.x),
        (&["y", "Y", "m_Y"][..], new_value.y),
        (&["z", "Z", "m_Z"][..], new_value.z),
        (&["w", "W", "m_W"][..], new_value.w),
    ] {
        set_float_field(target, names, component, field)?;
    }
    Ok(())
}

fn type_tree_field_node(
    type_tree: &TypeTree,
    aliases: &'static [&'static str],
    field: &'static str,
) -> Result<Option<usize>, SerializedAnimationClipError> {
    let mut matches = type_tree.nodes.iter().enumerate().filter(|(_, node)| {
        node.name
            .as_deref()
            .is_some_and(|name| aliases.contains(&name))
    });
    let Some((node_index, _)) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err(mutation_error(field, "TypeTree field name is ambiguous"));
    }
    Ok(Some(node_index))
}

fn type_tree_subtree_field_node(
    type_tree: &TypeTree,
    root_node: usize,
    aliases: &[&str],
    field: &'static str,
) -> Result<usize, SerializedAnimationClipError> {
    let Some(root) = type_tree.nodes.get(root_node) else {
        return Err(mutation_error(
            field,
            "TypeTree subtree root is out of range",
        ));
    };
    let mut matches = type_tree
        .nodes
        .iter()
        .enumerate()
        .skip(root_node + 1)
        .take_while(|(_, node)| node.level > root.level)
        .filter(|(_, node)| {
            node.name
                .as_deref()
                .is_some_and(|name| aliases.contains(&name))
        });
    let Some((node_index, _)) = matches.next() else {
        return Err(mutation_error(
            field,
            format!("TypeTree subtree has no field {}", aliases.join(" or ")),
        ));
    };
    if matches.next().is_some() {
        return Err(mutation_error(
            field,
            format!(
                "TypeTree subtree field {} is ambiguous",
                aliases.join(" or ")
            ),
        ));
    }
    Ok(node_index)
}

fn type_tree_array_element_node(
    type_tree: &TypeTree,
    array_node: usize,
    field: &'static str,
) -> Result<usize, SerializedAnimationClipError> {
    let array_children = direct_type_tree_children(type_tree, array_node, field)?;
    let array_container = array_children
        .into_iter()
        .find(|index| {
            type_tree.nodes[*index].name.as_deref() == Some("Array")
                || type_tree.nodes[*index].type_name.as_deref() == Some("Array")
        })
        .ok_or_else(|| mutation_error(field, "TypeTree array has no Array node"))?;
    direct_type_tree_children(type_tree, array_container, field)?
        .into_iter()
        .find(|index| type_tree.nodes[*index].name.as_deref() == Some("data"))
        .ok_or_else(|| mutation_error(field, "TypeTree Array node has no data element"))
}

fn direct_type_tree_children(
    type_tree: &TypeTree,
    parent: usize,
    field: &'static str,
) -> Result<Vec<usize>, SerializedAnimationClipError> {
    let Some(parent_node) = type_tree.nodes.get(parent) else {
        return Err(mutation_error(field, "TypeTree node index is out of range"));
    };
    let mut children = Vec::new();
    for (index, node) in type_tree.nodes.iter().enumerate().skip(parent + 1) {
        if node.level <= parent_node.level {
            break;
        }
        if node.level == parent_node.level.saturating_add(1) {
            children.push(index);
        }
    }
    Ok(children)
}

fn mutation_error(field: &'static str, message: impl Into<String>) -> SerializedAnimationClipError {
    SerializedAnimationClipError::Mutation {
        field: field.to_owned(),
        message: message.into(),
    }
}

fn serialized_type_name(value: &SerializedValue) -> &'static str {
    match value {
        SerializedValue::Bool(_) => "Bool",
        SerializedValue::I8(_) => "I8",
        SerializedValue::U8(_) => "U8",
        SerializedValue::I16(_) => "I16",
        SerializedValue::U16(_) => "U16",
        SerializedValue::Char(_) => "Char",
        SerializedValue::I32(_) => "I32",
        SerializedValue::U32(_) => "U32",
        SerializedValue::I64(_) => "I64",
        SerializedValue::U64(_) => "U64",
        SerializedValue::F32(_) => "F32",
        SerializedValue::F64(_) => "F64",
        SerializedValue::StringBytes(_) => "String",
        SerializedValue::Bytes(_) => "Bytes",
        SerializedValue::Array(_) => "Array",
        SerializedValue::Struct(_) => "Struct",
        SerializedValue::PPtr { .. } => "PPtr",
        SerializedValue::ManagedReferenceId { .. } => "ManagedReferenceId",
        SerializedValue::ManagedReferenceRegistry(_) => "ManagedReferenceRegistry",
        SerializedValue::Raw { .. } => "Raw",
    }
}

fn read_binding(
    value: &SerializedValue,
    index: usize,
) -> Result<BindingObject, SerializedAnimationClipError> {
    let path = format!("genericBindings[{index}]");
    let script = optional_field(value, &format!("{path}.script"), &["script", "Script"])?
        .map(|value| read_script_reference(value, &format!("{path}.script")))
        .transpose()?
        .flatten();
    Ok(BindingObject {
        path: read_u32(
            required_field(value, &format!("{path}.path"), &["path", "Path"])?,
            &format!("{path}.path"),
        )?,
        attribute: read_u32(
            required_field(
                value,
                &format!("{path}.attribute"),
                &["attribute", "Attribute"],
            )?,
            &format!("{path}.attribute"),
        )?,
        class_id: read_signed(
            required_field(
                value,
                &format!("{path}.typeID"),
                &["typeID", "classID", "ClassID"],
            )?,
            &format!("{path}.typeID"),
        )?,
        custom_type: read_u8(
            required_field(
                value,
                &format!("{path}.customType"),
                &["customType", "CustomType"],
            )?,
            &format!("{path}.customType"),
        )?,
        is_pptr_curve: optional_field(
            value,
            &format!("{path}.isPPtrCurve"),
            &["isPPtrCurve", "IsPPtrCurve"],
        )?
        .map(|value| read_bool(value, &format!("{path}.isPPtrCurve")))
        .transpose()?
        .unwrap_or(false),
        script,
    })
}

fn read_vector3_curves(
    root: &SerializedValue,
    version: UnityVersion,
    label: &str,
    aliases: &[&str],
) -> Result<Vec<Vector3CurveObject>, SerializedAnimationClipError> {
    let Some(value) = field(root, label, aliases)? else {
        return Ok(Vec::new());
    };
    as_array(value, label)?
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let path = format!("{label}[{index}]");
            let (envelope, curve) = read_vector3_animation_curve(
                required_field(value, &format!("{path}.curve"), &["curve", "Curve"])?,
                version,
                &format!("{path}.curve"),
            )?;
            Ok(Vector3CurveObject {
                path: read_string(
                    required_field(value, &format!("{path}.path"), &["path", "Path"])?,
                    &format!("{path}.path"),
                )?,
                envelope,
                curve,
            })
        })
        .collect()
}

fn read_quaternion_curves(
    root: &SerializedValue,
    version: UnityVersion,
    label: &str,
    aliases: &[&str],
) -> Result<Vec<QuaternionCurveObject>, SerializedAnimationClipError> {
    let Some(value) = field(root, label, aliases)? else {
        return Ok(Vec::new());
    };
    as_array(value, label)?
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let path = format!("{label}[{index}]");
            let (envelope, curve) = read_quaternion_animation_curve(
                required_field(value, &format!("{path}.curve"), &["curve", "Curve"])?,
                version,
                &format!("{path}.curve"),
            )?;
            Ok(QuaternionCurveObject {
                path: read_string(
                    required_field(value, &format!("{path}.path"), &["path", "Path"])?,
                    &format!("{path}.path"),
                )?,
                envelope,
                curve,
            })
        })
        .collect()
}

fn read_float_curves(
    root: &SerializedValue,
    version: UnityVersion,
    label: &str,
    aliases: &[&str],
) -> Result<Vec<FloatCurveObject>, SerializedAnimationClipError> {
    let Some(value) = field(root, label, aliases)? else {
        return Ok(Vec::new());
    };
    as_array(value, label)?
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let path = format!("{label}[{index}]");
            let (envelope, curve) = read_single_animation_curve(
                required_field(value, &format!("{path}.curve"), &["curve", "Curve"])?,
                version,
                &format!("{path}.curve"),
            )?;
            Ok(FloatCurveObject {
                path: read_string(
                    required_field(value, &format!("{path}.path"), &["path", "Path"])?,
                    &format!("{path}.path"),
                )?,
                attribute: read_string(
                    required_field(
                        value,
                        &format!("{path}.attribute"),
                        &["attribute", "Attribute"],
                    )?,
                    &format!("{path}.attribute"),
                )?,
                class_id: read_signed(
                    required_field(value, &format!("{path}.classID"), &["classID", "ClassID"])?,
                    &format!("{path}.classID"),
                )?,
                script: optional_field(value, &format!("{path}.script"), &["script", "Script"])?
                    .map(|value| read_script_reference(value, &format!("{path}.script")))
                    .transpose()?
                    .flatten(),
                flags: optional_field(value, &format!("{path}.flags"), &["flags", "Flags"])?
                    .map(|value| read_signed(value, &format!("{path}.flags")))
                    .transpose()?
                    .unwrap_or(0),
                envelope,
                curve,
            })
        })
        .collect()
}

fn read_pptr_curves(
    root: &SerializedValue,
    label: &str,
    aliases: &[&str],
) -> Result<Vec<PPtrCurveObject>, SerializedAnimationClipError> {
    let Some(value) = field(root, label, aliases)? else {
        return Ok(Vec::new());
    };
    as_array(value, label)?
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let path = format!("{label}[{index}]");
            let script = optional_field(value, &format!("{path}.script"), &["script", "Script"])?
                .map(|value| read_script_reference(value, &format!("{path}.script")))
                .transpose()?
                .flatten();
            let curve = array_field(value, &format!("{path}.curve"), &["curve", "Curve"])?
                .iter()
                .enumerate()
                .map(|(key_index, key)| {
                    read_pptr_keyframe(key, &format!("{path}.curve[{key_index}]"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(PPtrCurveObject {
                path: read_string(
                    required_field(value, &format!("{path}.path"), &["path", "Path"])?,
                    &format!("{path}.path"),
                )?,
                attribute: read_string(
                    required_field(
                        value,
                        &format!("{path}.attribute"),
                        &["attribute", "Attribute"],
                    )?,
                    &format!("{path}.attribute"),
                )?,
                class_id: read_signed(
                    required_field(value, &format!("{path}.classID"), &["classID", "ClassID"])?,
                    &format!("{path}.classID"),
                )?,
                script,
                flags: optional_field(value, &format!("{path}.flags"), &["flags", "Flags"])?
                    .map(|value| read_signed(value, &format!("{path}.flags")))
                    .transpose()?
                    .unwrap_or(0),
                curve,
            })
        })
        .collect()
}

fn read_vector3_animation_curve(
    value: &SerializedValue,
    version: UnityVersion,
    label: &str,
) -> Result<(CurveEnvelope, Vec<KeyframeVector3View>), SerializedAnimationClipError> {
    let envelope = read_curve_envelope(value, version, label)?;
    let curve = array_field(
        value,
        &format!("{label}.m_Curve"),
        &["m_Curve", "curve", "Curve"],
    )?
    .iter()
    .enumerate()
    .map(|(index, value)| {
        read_vector3_keyframe(value, version, &format!("{label}.m_Curve[{index}]"))
    })
    .collect::<Result<Vec<_>, _>>()?;
    Ok((envelope, curve))
}

fn read_quaternion_animation_curve(
    value: &SerializedValue,
    version: UnityVersion,
    label: &str,
) -> Result<(CurveEnvelope, Vec<KeyframeQuaternionView>), SerializedAnimationClipError> {
    let envelope = read_curve_envelope(value, version, label)?;
    let curve = array_field(
        value,
        &format!("{label}.m_Curve"),
        &["m_Curve", "curve", "Curve"],
    )?
    .iter()
    .enumerate()
    .map(|(index, value)| {
        read_quaternion_keyframe(value, version, &format!("{label}.m_Curve[{index}]"))
    })
    .collect::<Result<Vec<_>, _>>()?;
    Ok((envelope, curve))
}

fn read_single_animation_curve(
    value: &SerializedValue,
    version: UnityVersion,
    label: &str,
) -> Result<(CurveEnvelope, Vec<KeyframeView>), SerializedAnimationClipError> {
    let envelope = read_curve_envelope(value, version, label)?;
    let curve = array_field(
        value,
        &format!("{label}.m_Curve"),
        &["m_Curve", "curve", "Curve"],
    )?
    .iter()
    .enumerate()
    .map(|(index, value)| {
        read_single_keyframe(value, version, &format!("{label}.m_Curve[{index}]"))
    })
    .collect::<Result<Vec<_>, _>>()?;
    Ok((envelope, curve))
}

fn read_curve_envelope(
    value: &SerializedValue,
    version: UnityVersion,
    label: &str,
) -> Result<CurveEnvelope, SerializedAnimationClipError> {
    let rotation_order = optional_field(
        value,
        &format!("{label}.m_RotationOrder"),
        &["m_RotationOrder", "rotationOrder", "RotationOrder"],
    )?
    .map(|value| read_signed(value, &format!("{label}.m_RotationOrder")))
    .transpose()?;
    if curve_presence(version).rotation_order != rotation_order.is_some() {
        return Err(SerializedAnimationClipError::InconsistentVersionField {
            field: format!("{label}.m_RotationOrder"),
        });
    }
    Ok(CurveEnvelope {
        pre_infinity: read_signed(
            required_field(
                value,
                &format!("{label}.m_PreInfinity"),
                &["m_PreInfinity", "preInfinity", "PreInfinity"],
            )?,
            &format!("{label}.m_PreInfinity"),
        )?,
        post_infinity: read_signed(
            required_field(
                value,
                &format!("{label}.m_PostInfinity"),
                &["m_PostInfinity", "postInfinity", "PostInfinity"],
            )?,
            &format!("{label}.m_PostInfinity"),
        )?,
        rotation_order,
    })
}

fn read_single_keyframe(
    value: &SerializedValue,
    version: UnityVersion,
    label: &str,
) -> Result<KeyframeView, SerializedAnimationClipError> {
    let presence = keyframe_presence(version);
    Ok(KeyframeView {
        time: read_f32(
            required_field(value, &format!("{label}.time"), &["time", "Time"])?,
            &format!("{label}.time"),
        )?,
        value: read_f32(
            required_field(value, &format!("{label}.value"), &["value", "Value"])?,
            &format!("{label}.value"),
        )?,
        in_slope: read_f32(
            required_field(value, &format!("{label}.inSlope"), &["inSlope", "InSlope"])?,
            &format!("{label}.inSlope"),
        )?,
        out_slope: read_f32(
            required_field(
                value,
                &format!("{label}.outSlope"),
                &["outSlope", "OutSlope"],
            )?,
            &format!("{label}.outSlope"),
        )?,
        in_weight: read_versioned_field(
            value,
            label,
            "inWeight",
            &["inWeight", "InWeight"],
            presence.in_weight,
            read_f32,
        )?,
        out_weight: read_versioned_field(
            value,
            label,
            "outWeight",
            &["outWeight", "OutWeight"],
            presence.out_weight,
            read_f32,
        )?,
        tangent_mode: optional_field(
            value,
            &format!("{label}.tangentMode"),
            &["tangentMode", "TangentMode"],
        )?
        .map(|value| read_signed(value, &format!("{label}.tangentMode")))
        .transpose()?
        .unwrap_or(0),
        weighted_mode: read_versioned_field(
            value,
            label,
            "weightedMode",
            &["weightedMode", "WeightedMode"],
            presence.weighted_mode,
            read_signed,
        )?,
    })
}

fn read_vector3_keyframe(
    value: &SerializedValue,
    version: UnityVersion,
    label: &str,
) -> Result<KeyframeVector3View, SerializedAnimationClipError> {
    let presence = keyframe_presence(version);
    Ok(KeyframeVector3View {
        time: read_f32(
            required_field(value, &format!("{label}.time"), &["time", "Time"])?,
            &format!("{label}.time"),
        )?,
        value: read_vec3(
            required_field(value, &format!("{label}.value"), &["value", "Value"])?,
            &format!("{label}.value"),
        )?,
        in_slope: read_vec3(
            required_field(value, &format!("{label}.inSlope"), &["inSlope", "InSlope"])?,
            &format!("{label}.inSlope"),
        )?,
        out_slope: read_vec3(
            required_field(
                value,
                &format!("{label}.outSlope"),
                &["outSlope", "OutSlope"],
            )?,
            &format!("{label}.outSlope"),
        )?,
        in_weight: read_versioned_field(
            value,
            label,
            "inWeight",
            &["inWeight", "InWeight"],
            presence.in_weight,
            read_vec3,
        )?,
        out_weight: read_versioned_field(
            value,
            label,
            "outWeight",
            &["outWeight", "OutWeight"],
            presence.out_weight,
            read_vec3,
        )?,
        tangent_mode: optional_field(
            value,
            &format!("{label}.tangentMode"),
            &["tangentMode", "TangentMode"],
        )?
        .map(|value| read_signed(value, &format!("{label}.tangentMode")))
        .transpose()?
        .unwrap_or(0),
        weighted_mode: read_versioned_field(
            value,
            label,
            "weightedMode",
            &["weightedMode", "WeightedMode"],
            presence.weighted_mode,
            read_signed,
        )?,
    })
}

fn read_quaternion_keyframe(
    value: &SerializedValue,
    version: UnityVersion,
    label: &str,
) -> Result<KeyframeQuaternionView, SerializedAnimationClipError> {
    let presence = keyframe_presence(version);
    Ok(KeyframeQuaternionView {
        time: read_f32(
            required_field(value, &format!("{label}.time"), &["time", "Time"])?,
            &format!("{label}.time"),
        )?,
        value: read_quat(
            required_field(value, &format!("{label}.value"), &["value", "Value"])?,
            &format!("{label}.value"),
        )?,
        in_slope: read_quat(
            required_field(value, &format!("{label}.inSlope"), &["inSlope", "InSlope"])?,
            &format!("{label}.inSlope"),
        )?,
        out_slope: read_quat(
            required_field(
                value,
                &format!("{label}.outSlope"),
                &["outSlope", "OutSlope"],
            )?,
            &format!("{label}.outSlope"),
        )?,
        in_weight: read_versioned_field(
            value,
            label,
            "inWeight",
            &["inWeight", "InWeight"],
            presence.in_weight,
            read_quat,
        )?,
        out_weight: read_versioned_field(
            value,
            label,
            "outWeight",
            &["outWeight", "OutWeight"],
            presence.out_weight,
            read_quat,
        )?,
        tangent_mode: optional_field(
            value,
            &format!("{label}.tangentMode"),
            &["tangentMode", "TangentMode"],
        )?
        .map(|value| read_signed(value, &format!("{label}.tangentMode")))
        .transpose()?
        .unwrap_or(0),
        weighted_mode: read_versioned_field(
            value,
            label,
            "weightedMode",
            &["weightedMode", "WeightedMode"],
            presence.weighted_mode,
            read_signed,
        )?,
    })
}

fn read_versioned_field<T>(
    value: &SerializedValue,
    label: &str,
    name: &str,
    aliases: &[&str],
    expected: bool,
    read: fn(&SerializedValue, &str) -> Result<T, SerializedAnimationClipError>,
) -> Result<Option<T>, SerializedAnimationClipError> {
    let field_label = format!("{label}.{name}");
    let found = optional_field(value, &field_label, aliases)?;
    if expected != found.is_some() {
        return Err(SerializedAnimationClipError::InconsistentVersionField { field: field_label });
    }
    found.map(|value| read(value, &field_label)).transpose()
}

fn read_pptr_keyframe(
    value: &SerializedValue,
    label: &str,
) -> Result<PPtrKeyframeObject, SerializedAnimationClipError> {
    Ok(PPtrKeyframeObject {
        time: read_f32(
            required_field(value, &format!("{label}.time"), &["time", "Time"])?,
            &format!("{label}.time"),
        )?,
        value: read_optional_pptr(
            required_field(value, &format!("{label}.value"), &["value", "Value"])?,
            &format!("{label}.value"),
        )?,
    })
}

fn read_vec3(value: &SerializedValue, label: &str) -> Result<Vec3, SerializedAnimationClipError> {
    let values = read_vector_components(value, label, &["x", "X"], &["y", "Y"], &["z", "Z"], None)?;
    Ok(Vec3 {
        x: values[0],
        y: values[1],
        z: values[2],
    })
}

fn read_quat(value: &SerializedValue, label: &str) -> Result<Quat, SerializedAnimationClipError> {
    let values = read_vector_components(
        value,
        label,
        &["x", "X"],
        &["y", "Y"],
        &["z", "Z"],
        Some(&["w", "W"]),
    )?;
    Ok(Quat {
        x: values[0],
        y: values[1],
        z: values[2],
        w: values[3],
    })
}

fn read_vector_components(
    value: &SerializedValue,
    label: &str,
    x_aliases: &[&str],
    y_aliases: &[&str],
    z_aliases: &[&str],
    w_aliases: Option<&[&str]>,
) -> Result<Vec<f32>, SerializedAnimationClipError> {
    if let Some(w_aliases) = w_aliases {
        if let SerializedValue::Array(values) = value {
            if values.len() != 4 {
                return Err(unexpected_type(label, "four-component vector", value));
            }
            return values
                .iter()
                .enumerate()
                .map(|(index, value)| read_f32(value, &format!("{label}[{index}]")))
                .collect();
        }
        return Ok(vec![
            read_component(value, label, x_aliases)?,
            read_component(value, label, y_aliases)?,
            read_component(value, label, z_aliases)?,
            read_component(value, label, w_aliases)?,
        ]);
    }
    if let SerializedValue::Array(values) = value {
        if values.len() != 3 {
            return Err(unexpected_type(label, "three-component vector", value));
        }
        return values
            .iter()
            .enumerate()
            .map(|(index, value)| read_f32(value, &format!("{label}[{index}]")))
            .collect();
    }
    Ok(vec![
        read_component(value, label, x_aliases)?,
        read_component(value, label, y_aliases)?,
        read_component(value, label, z_aliases)?,
    ])
}

fn read_component(
    value: &SerializedValue,
    label: &str,
    aliases: &[&str],
) -> Result<f32, SerializedAnimationClipError> {
    read_f32(
        required_field(value, &format!("{label}.{}", aliases[0]), aliases)?,
        &format!("{label}.{}", aliases[0]),
    )
}

fn read_event(
    value: &SerializedValue,
    index: usize,
) -> Result<ClipEventObject, SerializedAnimationClipError> {
    let path = format!("m_Events[{index}]");
    Ok(ClipEventObject {
        function_name: read_string(
            required_field(
                value,
                &format!("{path}.functionName"),
                &["functionName", "FunctionName"],
            )?,
            &format!("{path}.functionName"),
        )?,
        data: read_string(
            required_field(value, &format!("{path}.data"), &["data", "Data"])?,
            &format!("{path}.data"),
        )?,
        float_parameter: read_f32(
            required_field(
                value,
                &format!("{path}.floatParameter"),
                &["floatParameter", "FloatParameter"],
            )?,
            &format!("{path}.floatParameter"),
        )?,
        int_parameter: read_signed(
            required_field(
                value,
                &format!("{path}.intParameter"),
                &["intParameter", "IntParameter"],
            )?,
            &format!("{path}.intParameter"),
        )?,
        message_options: optional_field(
            value,
            &format!("{path}.messageOptions"),
            &["messageOptions", "MessageOptions"],
        )?
        .map(|value| read_signed(value, &format!("{path}.messageOptions")))
        .transpose()?
        .unwrap_or(0),
        object_reference_parameter: optional_field(
            value,
            &format!("{path}.objectReferenceParameter"),
            &["objectReferenceParameter", "ObjectReferenceParameter"],
        )?
        .map(|value| read_optional_pptr(value, &format!("{path}.objectReferenceParameter")))
        .transpose()?
        .flatten(),
        time: read_f32(
            required_field(value, &format!("{path}.time"), &["time", "Time"])?,
            &format!("{path}.time"),
        )?,
    })
}

fn read_constant_view(
    value: &SerializedValue,
) -> Result<MuscleClipConstantView, SerializedAnimationClipError> {
    macro_rules! f32_field {
        ($name:literal, $($alias:literal),+) => {
            optional_field(value, $name, &[$($alias),+])?
                .map(|value| read_f32(value, $name))
                .transpose()?
                .unwrap_or(0.0)
        };
    }
    macro_rules! bool_field {
        ($name:literal, $($alias:literal),+) => {
            optional_field(value, $name, &[$($alias),+])?
                .map(|value| read_bool(value, $name))
                .transpose()?
                .unwrap_or(false)
        };
    }
    Ok(MuscleClipConstantView {
        start_time: f32_field!("m_StartTime", "m_StartTime", "startTime"),
        stop_time: f32_field!("m_StopTime", "m_StopTime", "stopTime"),
        orientation_offset_y: f32_field!(
            "m_OrientationOffsetY",
            "m_OrientationOffsetY",
            "orientationOffsetY"
        ),
        level: f32_field!("m_Level", "m_Level", "level"),
        cycle_offset: f32_field!("m_CycleOffset", "m_CycleOffset", "cycleOffset"),
        loop_time: bool_field!("m_LoopTime", "m_LoopTime", "loopTime"),
        loop_blend: bool_field!("m_LoopBlend", "m_LoopBlend", "loopBlend"),
        loop_blend_orientation: bool_field!(
            "m_LoopBlendOrientation",
            "m_LoopBlendOrientation",
            "loopBlendOrientation"
        ),
        loop_blend_position_y: bool_field!(
            "m_LoopBlendPositionY",
            "m_LoopBlendPositionY",
            "loopBlendPositionY"
        ),
        loop_blend_position_xz: bool_field!(
            "m_LoopBlendPositionXZ",
            "m_LoopBlendPositionXZ",
            "loopBlendPositionXZ"
        ),
        keep_original_orientation: bool_field!(
            "m_KeepOriginalOrientation",
            "m_KeepOriginalOrientation",
            "keepOriginalOrientation"
        ),
        keep_original_position_y: bool_field!(
            "m_KeepOriginalPositionY",
            "m_KeepOriginalPositionY",
            "keepOriginalPositionY"
        ),
        keep_original_position_xz: bool_field!(
            "m_KeepOriginalPositionXZ",
            "m_KeepOriginalPositionXZ",
            "keepOriginalPositionXZ"
        ),
        height_from_feet: bool_field!("m_HeightFromFeet", "m_HeightFromFeet", "heightFromFeet"),
    })
}

fn required_field<'a>(
    value: &'a SerializedValue,
    label: &str,
    aliases: &[&str],
) -> Result<&'a SerializedValue, SerializedAnimationClipError> {
    field(value, label, aliases)?.ok_or_else(|| SerializedAnimationClipError::MissingField {
        field: label.to_owned(),
    })
}

fn optional_field<'a>(
    value: &'a SerializedValue,
    label: &str,
    aliases: &[&str],
) -> Result<Option<&'a SerializedValue>, SerializedAnimationClipError> {
    field(value, label, aliases)
}

fn field<'a>(
    value: &'a SerializedValue,
    label: &str,
    aliases: &[&str],
) -> Result<Option<&'a SerializedValue>, SerializedAnimationClipError> {
    let mut found = None;
    for alias in aliases {
        let result = value.unique_field_in_base_chain(alias).map_err(|source| {
            SerializedAnimationClipError::Lookup {
                field: label.to_owned(),
                source,
            }
        })?;
        if let Some(result) = result {
            if found.is_some() {
                return Err(SerializedAnimationClipError::AmbiguousAlias {
                    aliases: aliases.iter().map(|alias| (*alias).to_owned()).collect(),
                });
            }
            found = Some(result);
        }
    }
    Ok(found)
}

fn array_field<'a>(
    value: &'a SerializedValue,
    label: &str,
    aliases: &[&str],
) -> Result<&'a [SerializedValue], SerializedAnimationClipError> {
    as_array(required_field(value, label, aliases)?, label)
}

fn as_array<'a>(
    value: &'a SerializedValue,
    label: &str,
) -> Result<&'a [SerializedValue], SerializedAnimationClipError> {
    value
        .array_elements()
        .ok_or_else(|| unexpected_type(label, "array", value))
}

fn read_optional_pptr(
    value: &SerializedValue,
    label: &str,
) -> Result<Option<PPtrValue>, SerializedAnimationClipError> {
    let (file_id, path_id) = match value {
        SerializedValue::PPtr { file_id, path_id } => (*file_id, *path_id),
        SerializedValue::Struct(_) => (
            read_signed(
                required_field(value, label, &["m_FileID", "fileID", "FileID_"])?,
                label,
            )?,
            read_i64(
                required_field(
                    value,
                    label,
                    &["m_PathID", "pathID", "PathID__Int64", "PathID_"],
                )?,
                label,
            )?,
        ),
        other => return Err(unexpected_type(label, "PPtr", other)),
    };
    if path_id == 0 {
        if file_id != 0 {
            return Err(SerializedAnimationClipError::UnsupportedPointerFileId {
                field: label.to_owned(),
                file_id,
            });
        }
        return Ok(None);
    }
    Ok(Some(PPtrValue { file_id, path_id }))
}

fn read_script_reference(
    value: &SerializedValue,
    label: &str,
) -> Result<Option<ScriptReference>, SerializedAnimationClipError> {
    read_optional_pptr(value, label)?
        .map(|pointer| {
            Ok(ScriptReference {
                file_id: pointer.file_id,
                path_id: pointer.path_id,
            })
        })
        .transpose()
}

fn read_f32_array(
    value: &SerializedValue,
    label: &str,
    aliases: &[&str],
) -> Result<Vec<f32>, SerializedAnimationClipError> {
    array_field(value, label, aliases)?
        .iter()
        .enumerate()
        .map(|(index, value)| read_f32(value, &format!("{label}[{index}]")))
        .collect()
}

fn read_f32(value: &SerializedValue, label: &str) -> Result<f32, SerializedAnimationClipError> {
    match value {
        SerializedValue::F32(value) => Ok(*value),
        other => Err(unexpected_type(label, "f32", other)),
    }
}

fn read_string(
    value: &SerializedValue,
    label: &str,
) -> Result<String, SerializedAnimationClipError> {
    match value {
        SerializedValue::StringBytes(value) => String::from_utf8(value.clone()).map_err(|error| {
            SerializedAnimationClipError::InvalidNumber {
                field: label.to_owned(),
                value: format!("invalid UTF-8: {error}"),
            }
        }),
        other => Err(unexpected_type(label, "UTF-8 string", other)),
    }
}

fn read_signed(value: &SerializedValue, label: &str) -> Result<i32, SerializedAnimationClipError> {
    match value {
        SerializedValue::I8(value) => Ok(i32::from(*value)),
        SerializedValue::U8(value) => Ok(i32::from(*value)),
        SerializedValue::I16(value) => Ok(i32::from(*value)),
        SerializedValue::U16(value) | SerializedValue::Char(value) => Ok(i32::from(*value)),
        SerializedValue::I32(value) => Ok(*value),
        SerializedValue::U32(value) => {
            i32::try_from(*value).map_err(|_| invalid_number(label, value))
        }
        other => Err(unexpected_type(label, "integer", other)),
    }
}

fn read_unsigned(
    value: &SerializedValue,
    label: &str,
) -> Result<u64, SerializedAnimationClipError> {
    match value {
        SerializedValue::U8(value) => Ok(u64::from(*value)),
        SerializedValue::I8(value) if *value >= 0 => Ok(*value as u64),
        SerializedValue::U16(value) | SerializedValue::Char(value) => Ok(u64::from(*value)),
        SerializedValue::I16(value) if *value >= 0 => Ok(*value as u64),
        SerializedValue::U32(value) => Ok(u64::from(*value)),
        SerializedValue::I32(value) if *value >= 0 => Ok(*value as u64),
        SerializedValue::U64(value) => Ok(*value),
        SerializedValue::I64(value) if *value >= 0 => Ok(*value as u64),
        other => Err(unexpected_type(label, "nonnegative integer", other)),
    }
}

fn read_i64(value: &SerializedValue, label: &str) -> Result<i64, SerializedAnimationClipError> {
    match value {
        SerializedValue::I8(value) => Ok(i64::from(*value)),
        SerializedValue::U8(value) => Ok(i64::from(*value)),
        SerializedValue::I16(value) => Ok(i64::from(*value)),
        SerializedValue::U16(value) | SerializedValue::Char(value) => Ok(i64::from(*value)),
        SerializedValue::I32(value) => Ok(i64::from(*value)),
        SerializedValue::U32(value) => Ok(i64::from(*value)),
        SerializedValue::I64(value) => Ok(*value),
        SerializedValue::U64(value) => {
            i64::try_from(*value).map_err(|_| invalid_number(label, value))
        }
        other => Err(unexpected_type(label, "integer", other)),
    }
}

fn read_u32(value: &SerializedValue, label: &str) -> Result<u32, SerializedAnimationClipError> {
    checked_u32(read_unsigned(value, label)?, label)
}

fn read_u8(value: &SerializedValue, label: &str) -> Result<u8, SerializedAnimationClipError> {
    let value = read_unsigned(value, label)?;
    u8::try_from(value).map_err(|_| invalid_number(label, &value))
}

fn checked_u32(value: u64, label: &str) -> Result<u32, SerializedAnimationClipError> {
    u32::try_from(value).map_err(|_| invalid_number(label, &value))
}

fn checked_u16(value: u64, label: &str) -> Result<u16, SerializedAnimationClipError> {
    u16::try_from(value).map_err(|_| invalid_number(label, &value))
}

fn read_bool(value: &SerializedValue, label: &str) -> Result<bool, SerializedAnimationClipError> {
    match value {
        SerializedValue::Bool(value) => Ok(*value),
        SerializedValue::U8(value) => Ok(*value != 0),
        SerializedValue::I32(value) => Ok(*value != 0),
        other => Err(unexpected_type(label, "boolean", other)),
    }
}

fn invalid_number(label: &str, value: &impl Display) -> SerializedAnimationClipError {
    SerializedAnimationClipError::InvalidNumber {
        field: label.to_owned(),
        value: value.to_string(),
    }
}

fn unexpected_type(
    label: &str,
    expected: &'static str,
    value: &SerializedValue,
) -> SerializedAnimationClipError {
    SerializedAnimationClipError::UnexpectedType {
        field: label.to_owned(),
        expected,
        actual: match value {
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
            SerializedValue::ManagedReferenceId { .. } => "managed reference id",
            SerializedValue::ManagedReferenceRegistry(_) => "managed reference registry",
            SerializedValue::Raw { .. } => "raw value",
        }
        .to_owned(),
    }
}
