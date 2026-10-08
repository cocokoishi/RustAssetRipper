//! The production `AnimationClipConverter`: emits curves into mutable clip objects.
//!
//! Ports `AssetRipper.Processing/AnimationClips/AnimationClipConverter.cs` end to end:
//! `ProcessInner`, `InitializeBindingsCache`, `GenerateFramesFromStreamedClip`,
//! `ProcessStreams`, `ProcessDenses`, `ProcessConstant`, `AddTransformCurve`,
//! `AddDefaultCurve` (GameObject/Script/Engine), `AddCustomCurve`,
//! `AddAnimatorMuscleCurve`, `AddFloatKeyframe`, `AddPPtrKeyframe`, `GetCurvePath`,
//! `GetReversedPath`, `CalculateLastConstantTime`, and `TryGetNextFrame`.
//!
//! The source `StreamedCurveKey` is a reference type, so slope calculation mutates the
//! frame's keys in place; this port indexes the frame vectors and mutates through
//! `split_at_mut` so the same mutation order is observable.
//!
//! Two reference behaviors are reproduced deliberately rather than "fixed":
//! - `GenerateFramesFromStreamedClip` copies the logical `u32` words in native byte order
//!   on both branches of `CpuEndiannessMatchesCollection`, then reads them back with the
//!   collection endianness.
//! - `AddPPtrKeyframe` silently drops a PPtr keyframe when the clip has no `PPtrCurves`
//!   list (`Has_PPtrCurves_C74()` false), and `AddTransformCurve` drops an Euler keyframe
//!   when `Has_EulerCurves_C74()` is false.

use std::collections::HashMap;
use std::error::Error;
use std::fmt::{self, Display};

use crate::numerics::{Quat, Vec3};

use super::clip_object::{
    AnimationClipObject, FloatCurveObject, PPtrCurveObject, PPtrKeyframeObject,
    QuaternionCurveObject, Vector3CurveObject,
};
use super::field_hashes;
use super::scene_facts::AnimationSceneFacts;
use crate::resources::animation_curve::set_default_rotation_order_and_curve_loop_type;
use crate::resources::avatar::HumanoidMuscleError;
use crate::resources::clip_convert::{
    cached_binding, constant_frame_count, default_curve_target, dense_frame_time,
    initialize_bindings_cache, is_transform_binding, ClipConvertError, ConverterBinding,
    DefaultCurveTarget, CLASS_ID_GAME_OBJECT, CLASS_ID_MONO_BEHAVIOUR, DEFAULT_FLOAT_WEIGHT,
};
use crate::resources::curve_wrappers::{
    quaternion_curve_set_values, vector3_curve_set_values,
    vector3_curve_set_values_with_rotation_order, ScriptReference,
};
use crate::resources::custom_curve::{
    is_unknown_path, reverse_ascii_crc32, to_attribute_name, BindingCustomType, CustomCurveError,
    CustomCurveSceneFacts, MISSED_PROPERTY_PREFIX, SCRIPT_PROPERTY_PREFIX,
    TYPE_TREE_PROPERTY_PREFIX, UNKNOWN_PATH_PREFIX,
};
use crate::resources::keyframe::{
    keyframe_presence, set_values_slopes, KeyframeView, WEIGHTED_MODE_NONE,
};
use crate::resources::keyframe_quaternion::KeyframeQuaternionView;
use crate::resources::keyframe_vector3::KeyframeVector3View;
use crate::resources::muscle_clip_info::initialize_from_constant;
use crate::resources::path_checksum::{PathChecksumCache, PathChecksumError, ProcessedScriptSet};
use crate::resources::streamed_curve::{StreamedCurveError, StreamedFrame};
use crate::resources::tangent_mode::{to_tangent, FREE_FREE};

/// `ClassIDType.Animator`.
pub const CLASS_ID_ANIMATOR: i32 = 95;
/// `ClassIDType.SkinnedMeshRenderer`.
pub const CLASS_ID_SKINNED_MESH_RENDERER: i32 = 137;
/// `EditorCurveBindingFlags.PPtr` (the `PPtrCurve.Flags` literal the source writes).
pub const PPTR_CURVE_FLAGS: i32 = 2;

/// The class ids whose generated classes implement `IRenderer`, in the order a
/// `TryGetComponent<IRenderer>()` scan can encounter them. Derived from the shipped
/// generated assembly (probe `tools/probes/anim-convert`, `renderers` mode).
pub const RENDERER_CLASS_IDS: &[i32] = &[
    23,  // MeshRenderer
    25,  // Renderer
    96,  // TrailRenderer
    120, // LineRenderer
    137, // SkinnedMeshRenderer
    199, // ParticleSystemRenderer
    212, // SpriteRenderer
];

/// Why clip conversion could not complete.
#[derive(Clone, Debug, PartialEq)]
pub enum ConvertError {
    /// The converter math failed (missing binding, invalid transform type).
    Convert(ClipConvertError),
    /// A streamed frame could not be read.
    Streamed(StreamedCurveError),
    /// A custom curve attribute could not be named.
    CustomCurve(CustomCurveError),
    /// A humanoid muscle attribute could not be named.
    HumanoidMuscle(HumanoidMuscleError),
    /// The checksum cache refused an insertion.
    Checksum(PathChecksumError),
    /// A dense or constant sample position ran past its sample array.
    SampleOutOfRange {
        /// The requested position.
        position: usize,
        /// The array length.
        length: usize,
    },
    /// A PPtr-curve keyframe indexed past the binding constant's PPtr mapping.
    PptrMappingOutOfRange {
        /// The requested mapping index.
        index: i32,
        /// The mapping length.
        length: usize,
    },
}

impl Display for ConvertError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Convert(error) => write!(formatter, "{error}"),
            Self::Streamed(error) => write!(formatter, "{error}"),
            Self::CustomCurve(error) => write!(formatter, "{error}"),
            Self::HumanoidMuscle(error) => write!(formatter, "{error}"),
            Self::Checksum(error) => write!(formatter, "{error}"),
            Self::SampleOutOfRange { position, length } => write!(
                formatter,
                "sample position {position} is past the {length}-element sample array"
            ),
            Self::PptrMappingOutOfRange { index, length } => write!(
                formatter,
                "PPtr-curve mapping index {index} is past the {length}-element mapping"
            ),
        }
    }
}

impl Error for ConvertError {}

impl From<ClipConvertError> for ConvertError {
    fn from(error: ClipConvertError) -> Self {
        Self::Convert(error)
    }
}

impl From<StreamedCurveError> for ConvertError {
    fn from(error: StreamedCurveError) -> Self {
        Self::Streamed(error)
    }
}

impl From<CustomCurveError> for ConvertError {
    fn from(error: CustomCurveError) -> Self {
        Self::CustomCurve(error)
    }
}

impl From<PathChecksumError> for ConvertError {
    fn from(error: PathChecksumError) -> Self {
        Self::Checksum(error)
    }
}

impl From<HumanoidMuscleError> for ConvertError {
    fn from(error: HumanoidMuscleError) -> Self {
        Self::HumanoidMuscle(error)
    }
}

/// The caller-supplied facts conversion needs beyond the clip itself.
pub struct ConverterContext<'a> {
    /// The materialized scene facts.
    pub scene: &'a dyn AnimationSceneFacts,
    /// The shared path checksum cache (`PathChecksumCache`).
    pub checksum_cache: &'a mut PathChecksumCache,
    /// The `processedAssets` set behind `PathChecksumCache.Add(IMonoScript)`.
    pub processed_scripts: &'a mut ProcessedScriptSet,
}

/// The `CurveData` dictionary key: path, attribute, class id, and script asset.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct CurveDataKey {
    path: String,
    attribute: String,
    class_id: i32,
    script: Option<ScriptReference>,
}

/// Curve-bucket indexes, mirroring the source's per-flavor dictionaries.
#[derive(Default)]
struct Buckets {
    translations: HashMap<String, usize>,
    rotations: HashMap<String, usize>,
    scales: HashMap<String, usize>,
    eulers: HashMap<String, usize>,
    floats: HashMap<CurveDataKey, usize>,
    pptrs: HashMap<CurveDataKey, usize>,
}

/// `AnimationClipConverter.Process`: the whole conversion sequence.
pub fn process_clip(
    clip: &mut AnimationClipObject,
    context: &mut ConverterContext<'_>,
) -> Result<(), ConvertError> {
    if !clip.has_clip_binding_constant {
        return Ok(());
    }
    let converter_bindings: Vec<ConverterBinding> = clip
        .bindings
        .iter()
        .map(|binding| ConverterBinding {
            path: binding.path,
            attribute: binding.attribute,
            class_id: binding.class_id,
            custom_type: binding.custom_type,
            is_pptr_curve: binding.is_pptr_curve,
            script: binding.script,
        })
        .collect();
    let cache = initialize_bindings_cache(&converter_bindings)?;

    let mut streamed_frames = generate_frames_from_streamed_clip(clip)?;
    let mut buckets = Buckets::default();
    process_streams(
        clip,
        &mut streamed_frames,
        &cache,
        &converter_bindings,
        context,
        &mut buckets,
    )?;

    let streamed_curve_count = clip.muscle_clip.streamed.curve_count();
    process_denses(
        clip,
        streamed_curve_count,
        &cache,
        &converter_bindings,
        context,
        &mut buckets,
    )?;

    if clip.muscle_clip.has_constant_clip {
        let pre_constant_curves =
            streamed_curve_count as i64 + i64::from(clip.muscle_clip.dense.curve_count);
        let last_constant_time = calculate_last_constant_time(
            &streamed_frames,
            clip.muscle_clip.constant_view.stop_time,
            clip.sample_rate,
            &cache,
            &converter_bindings,
            |position| {
                converter_bindings
                    .get(position)
                    .map(|binding| binding.is_pptr_curve)
            },
        )?;
        process_constant(
            clip,
            pre_constant_curves,
            last_constant_time,
            &cache,
            &converter_bindings,
            context,
            &mut buckets,
        )?;
    }

    let constant_view = clip.muscle_clip.constant_view;
    initialize_from_constant(&mut clip.muscle_clip_info, &constant_view);
    Ok(())
}

/// `AnimationClipConverter.GenerateFramesFromStreamedClip`.
///
/// The source copies the logical `u32` words in native byte order on both branches (the
/// match branch is a raw `MemoryMarshal.Cast` copy, the mismatch branch writes the CPU
/// endianness), then reads the buffer with the collection endianness. A big-endian
/// collection on a little-endian host therefore misreads, exactly like the reference.
pub fn generate_frames_from_streamed_clip(
    clip: &AnimationClipObject,
) -> Result<Vec<StreamedFrame>, ConvertError> {
    let data = &clip.muscle_clip.streamed.data;
    if data.is_empty() {
        return Ok(Vec::new());
    }
    let mut buffer = vec![0u8; data.len() * 4];
    for (index, word) in data.iter().enumerate() {
        buffer[index * 4..index * 4 + 4].copy_from_slice(&word.to_ne_bytes());
    }

    let mut frames = Vec::new();
    let mut position = 0usize;
    while position < buffer.len() {
        let (frame, next) = StreamedFrame::read(&buffer, clip.endian, clip.version, position)?;
        position = next;
        frames.push(frame);
    }
    Ok(frames)
}

/// The `ProcessStreams` first-frame fix: past the first frame, frame zero starts at time
/// zero so PPtr curves open at `Time = 0` instead of `float.MinValue`.
fn fix_first_frame_time(frames: &mut [StreamedFrame]) {
    if frames.len() > 1 {
        frames[0].time = 0.0;
    }
}

#[allow(clippy::too_many_arguments)]
fn process_streams(
    clip: &mut AnimationClipObject,
    frames: &mut [StreamedFrame],
    cache: &[usize],
    bindings: &[ConverterBinding],
    context: &mut ConverterContext<'_>,
    buckets: &mut Buckets,
) -> Result<(), ConvertError> {
    let mut curve_values = [0.0f32; 4];
    let mut in_slope_values = [0.0f32; 4];
    let mut out_slope_values = [0.0f32; 4];
    let use_neg_inf_slopes = clip.supports_negative_infinity_slopes();

    fix_first_frame_time(frames);
    let frame_count = frames.len().saturating_sub(1);
    for frame_index in 0..frame_count {
        let do_slope_calc = frame_index != frame_count - 1;
        let mut curve_index = 0usize;
        while curve_index < frames[frame_index].curves.len() {
            let curve_id = frames[frame_index].curves[curve_index].index;
            let position = cached_binding(cache, curve_id)?;
            let binding = *bindings
                .get(position)
                .ok_or(ClipConvertError::MissingBinding {
                    curve_index: curve_id,
                })?;
            let path = get_curve_path(context.checksum_cache, binding.path);
            let frame_time = frames[frame_index].time;
            if is_transform_binding(binding.class_id, binding.attribute) {
                let dimension =
                    crate::resources::custom_curve::TransformType(binding.attribute as i32)
                        .dimension()?;
                if frame_index == 0 {
                    curve_index += dimension;
                    continue;
                }
                for offset in 0..dimension {
                    if do_slope_calc {
                        if let Some((next_frame, next_curve)) =
                            crate::resources::streamed_curve::try_get_next_frame(
                                frames,
                                frame_index,
                                curve_id,
                            )
                        {
                            let next_time = frames[next_frame].time;
                            let (current, later) = frames.split_at_mut(next_frame);
                            let key = &mut current[frame_index].curves[curve_index];
                            let next_key = &mut later[0].curves[next_curve + offset];
                            key.calculate_slopes(
                                frame_time,
                                next_time,
                                next_key,
                                use_neg_inf_slopes,
                            );
                        }
                    }
                    let key = &frames[frame_index].curves[curve_index];
                    curve_values[offset] = key.value();
                    in_slope_values[offset] = key.in_slope;
                    out_slope_values[offset] = key.out_slope;
                    curve_index += 1;
                }
                add_transform_curve(
                    clip,
                    frame_time,
                    binding,
                    &curve_values,
                    &in_slope_values,
                    &out_slope_values,
                    0,
                    &path,
                    buckets,
                )?;
                continue;
            }

            if !binding.is_pptr_curve {
                if frame_index == 0 {
                    curve_index += 1;
                    continue;
                }
                if do_slope_calc {
                    if let Some((next_frame, next_curve)) =
                        crate::resources::streamed_curve::try_get_next_frame(
                            frames,
                            frame_index,
                            curve_id,
                        )
                    {
                        let next_time = frames[next_frame].time;
                        let (current, later) = frames.split_at_mut(next_frame);
                        let key = &mut current[frame_index].curves[curve_index];
                        let next_key = &mut later[0].curves[next_curve];
                        key.calculate_slopes(frame_time, next_time, next_key, use_neg_inf_slopes);
                    }
                }
            }
            let key = &frames[frame_index].curves[curve_index];
            let value = key.value();
            let in_slope = key.in_slope;
            let out_slope = key.out_slope;
            if binding.custom_type == BindingCustomType::NONE.0 {
                add_default_curve(
                    clip, binding, &path, frame_time, value, in_slope, out_slope, context, buckets,
                )?;
            } else {
                add_custom_curve(
                    clip, binding, &path, frame_time, value, in_slope, out_slope, context, buckets,
                )?;
            }
            curve_index += 1;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn process_denses(
    clip: &mut AnimationClipObject,
    pre_dense_curves: i32,
    cache: &[usize],
    bindings: &[ConverterBinding],
    context: &mut ConverterContext<'_>,
    buckets: &mut Buckets,
) -> Result<(), ConvertError> {
    let frame_count = clip.muscle_clip.dense.frame_count;
    let curve_count = clip.muscle_clip.dense.curve_count;
    if frame_count == 0 || curve_count == 0 {
        return Ok(());
    }
    let slope_values = [0.0f32; 4];
    let sample_rate = clip.muscle_clip.dense.sample_rate;
    let begin_time = clip.muscle_clip.dense.begin_time;
    let sample_array = clip.muscle_clip.dense.sample_array.clone();

    for frame_index in 0..frame_count {
        let time = dense_frame_time(frame_index, sample_rate, begin_time);
        let frame_offset = frame_index as i64 * i64::from(curve_count);
        let mut curve_index = 0i64;
        while curve_index < i64::from(curve_count) {
            let index = i64::from(pre_dense_curves) + curve_index;
            let position = cached_binding(cache, i32::try_from(index).unwrap_or(i32::MAX))?;
            let binding = *bindings
                .get(position)
                .ok_or(ClipConvertError::MissingBinding {
                    curve_index: i32::try_from(index).unwrap_or(i32::MAX),
                })?;
            let path = get_curve_path(context.checksum_cache, binding.path);
            let frame_position = usize::try_from(frame_offset + curve_index).unwrap_or(usize::MAX);
            if is_transform_binding(binding.class_id, binding.attribute) {
                add_transform_curve(
                    clip,
                    time,
                    binding,
                    &sample_array,
                    &slope_values,
                    &slope_values,
                    frame_position,
                    &path,
                    buckets,
                )?;
                let dimension =
                    crate::resources::custom_curve::TransformType(binding.attribute as i32)
                        .dimension()?;
                curve_index += dimension as i64;
            } else {
                let value =
                    *sample_array
                        .get(frame_position)
                        .ok_or(ConvertError::SampleOutOfRange {
                            position: frame_position,
                            length: sample_array.len(),
                        })?;
                if binding.custom_type == BindingCustomType::NONE.0 {
                    add_default_curve(
                        clip, binding, &path, time, value, 0.0, 0.0, context, buckets,
                    )?;
                } else {
                    add_custom_curve(
                        clip, binding, &path, time, value, 0.0, 0.0, context, buckets,
                    )?;
                }
                curve_index += 1;
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn process_constant(
    clip: &mut AnimationClipObject,
    pre_constant_curves: i64,
    last_frame: f32,
    cache: &[usize],
    bindings: &[ConverterBinding],
    context: &mut ConverterContext<'_>,
    buckets: &mut Buckets,
) -> Result<(), ConvertError> {
    let data = clip.muscle_clip.constant.data.clone();
    if data.is_empty() {
        return Ok(());
    }
    let slope_values = [0.0f32; 4];
    let mut time = 0.0f32;
    let frames = constant_frame_count(last_frame);
    for _ in 0..frames {
        let mut curve_index = 0i64;
        while curve_index < data.len() as i64 {
            let index = pre_constant_curves + curve_index;
            let position = cached_binding(cache, i32::try_from(index).unwrap_or(i32::MAX))?;
            let binding = *bindings
                .get(position)
                .ok_or(ClipConvertError::MissingBinding {
                    curve_index: i32::try_from(index).unwrap_or(i32::MAX),
                })?;
            let path = get_curve_path(context.checksum_cache, binding.path);
            if is_transform_binding(binding.class_id, binding.attribute) {
                add_transform_curve(
                    clip,
                    time,
                    binding,
                    &data,
                    &slope_values,
                    &slope_values,
                    curve_index as usize,
                    &path,
                    buckets,
                )?;
                let dimension =
                    crate::resources::custom_curve::TransformType(binding.attribute as i32)
                        .dimension()?;
                curve_index += dimension as i64;
            } else {
                let value = data[curve_index as usize];
                if binding.custom_type == BindingCustomType::NONE.0 {
                    add_default_curve(
                        clip, binding, &path, time, value, 0.0, 0.0, context, buckets,
                    )?;
                } else {
                    add_custom_curve(
                        clip, binding, &path, time, value, 0.0, 0.0, context, buckets,
                    )?;
                }
                curve_index += 1;
            }
        }
        time += last_frame;
    }
    Ok(())
}

/// `AnimationClipConverter.AddTransformCurve`.
#[allow(clippy::too_many_arguments)]
fn add_transform_curve(
    clip: &mut AnimationClipObject,
    time: f32,
    binding: ConverterBinding,
    curve_values: &[f32],
    in_slope_values: &[f32],
    out_slope_values: &[f32],
    offset: usize,
    path: &str,
    buckets: &mut Buckets,
) -> Result<(), ConvertError> {
    let version = clip.version;
    let tangent_mode = to_tangent(FREE_FREE, version);
    match crate::resources::custom_curve::TransformType(binding.attribute as i32) {
        crate::resources::custom_curve::TransformType::TRANSLATION => {
            let index = match buckets.translations.get(path) {
                Some(index) => *index,
                None => {
                    let mut curve = Vector3CurveObject {
                        path: path.to_owned(),
                        ..Vector3CurveObject::default()
                    };
                    let mut view = crate::resources::curve_wrappers::Vector3CurveView {
                        envelope: crate::resources::curve_wrappers::CurveEnvelope {
                            rotation_order: clip.curve_presence().rotation_order.then_some(0),
                            ..Default::default()
                        },
                        ..Default::default()
                    };
                    vector3_curve_set_values(&mut view, path);
                    curve.envelope = view.envelope;
                    clip.position_curves.push(curve);
                    let index = clip.position_curves.len() - 1;
                    buckets.translations.insert(path.to_owned(), index);
                    index
                }
            };
            let mut key = KeyframeVector3View::for_version(version);
            key.value = vec3(
                curve_values[offset],
                curve_values[offset + 1],
                curve_values[offset + 2],
            );
            key.in_slope = vec3(in_slope_values[0], in_slope_values[1], in_slope_values[2]);
            key.out_slope = vec3(
                out_slope_values[0],
                out_slope_values[1],
                out_slope_values[2],
            );
            key.time = time;
            key.tangent_mode = tangent_mode;
            key.weighted_mode = Some(WEIGHTED_MODE_NONE);
            key.in_weight = Some(vec3(
                DEFAULT_FLOAT_WEIGHT,
                DEFAULT_FLOAT_WEIGHT,
                DEFAULT_FLOAT_WEIGHT,
            ));
            key.out_weight = Some(vec3(
                DEFAULT_FLOAT_WEIGHT,
                DEFAULT_FLOAT_WEIGHT,
                DEFAULT_FLOAT_WEIGHT,
            ));
            clip.position_curves[index].curve.push(key);
        }
        crate::resources::custom_curve::TransformType::ROTATION => {
            let index = match buckets.rotations.get(path) {
                Some(index) => *index,
                None => {
                    let mut curve = QuaternionCurveObject {
                        path: path.to_owned(),
                        ..QuaternionCurveObject::default()
                    };
                    let mut view = crate::resources::curve_wrappers::QuaternionCurveView {
                        envelope: crate::resources::curve_wrappers::CurveEnvelope {
                            rotation_order: clip.curve_presence().rotation_order.then_some(0),
                            ..Default::default()
                        },
                        ..Default::default()
                    };
                    quaternion_curve_set_values(&mut view, path);
                    curve.envelope = view.envelope;
                    clip.rotation_curves.push(curve);
                    let index = clip.rotation_curves.len() - 1;
                    buckets.rotations.insert(path.to_owned(), index);
                    index
                }
            };
            let mut key = KeyframeQuaternionView::for_version(version);
            key.value = quat(
                curve_values[offset],
                curve_values[offset + 1],
                curve_values[offset + 2],
                curve_values[offset + 3],
            );
            key.in_slope = quat(
                in_slope_values[0],
                in_slope_values[1],
                in_slope_values[2],
                in_slope_values[3],
            );
            key.out_slope = quat(
                out_slope_values[0],
                out_slope_values[1],
                out_slope_values[2],
                out_slope_values[3],
            );
            key.time = time;
            key.tangent_mode = tangent_mode;
            key.weighted_mode = Some(WEIGHTED_MODE_NONE);
            key.in_weight = Some(quat(
                DEFAULT_FLOAT_WEIGHT,
                DEFAULT_FLOAT_WEIGHT,
                DEFAULT_FLOAT_WEIGHT,
                DEFAULT_FLOAT_WEIGHT,
            ));
            key.out_weight = Some(quat(
                DEFAULT_FLOAT_WEIGHT,
                DEFAULT_FLOAT_WEIGHT,
                DEFAULT_FLOAT_WEIGHT,
                DEFAULT_FLOAT_WEIGHT,
            ));
            clip.rotation_curves[index].curve.push(key);
        }
        crate::resources::custom_curve::TransformType::SCALING => {
            let index = match buckets.scales.get(path) {
                Some(index) => *index,
                None => {
                    let mut curve = Vector3CurveObject {
                        path: path.to_owned(),
                        ..Vector3CurveObject::default()
                    };
                    let mut view = crate::resources::curve_wrappers::Vector3CurveView {
                        envelope: crate::resources::curve_wrappers::CurveEnvelope {
                            rotation_order: clip.curve_presence().rotation_order.then_some(0),
                            ..Default::default()
                        },
                        ..Default::default()
                    };
                    vector3_curve_set_values(&mut view, path);
                    curve.envelope = view.envelope;
                    clip.scale_curves.push(curve);
                    let index = clip.scale_curves.len() - 1;
                    buckets.scales.insert(path.to_owned(), index);
                    index
                }
            };
            let mut key = KeyframeVector3View::for_version(version);
            key.value = vec3(
                curve_values[offset],
                curve_values[offset + 1],
                curve_values[offset + 2],
            );
            key.in_slope = vec3(in_slope_values[0], in_slope_values[1], in_slope_values[2]);
            key.out_slope = vec3(
                out_slope_values[0],
                out_slope_values[1],
                out_slope_values[2],
            );
            key.time = time;
            key.tangent_mode = tangent_mode;
            key.weighted_mode = Some(WEIGHTED_MODE_NONE);
            key.in_weight = Some(vec3(
                DEFAULT_FLOAT_WEIGHT,
                DEFAULT_FLOAT_WEIGHT,
                DEFAULT_FLOAT_WEIGHT,
            ));
            key.out_weight = Some(vec3(
                DEFAULT_FLOAT_WEIGHT,
                DEFAULT_FLOAT_WEIGHT,
                DEFAULT_FLOAT_WEIGHT,
            ));
            clip.scale_curves[index].curve.push(key);
        }
        crate::resources::custom_curve::TransformType::EULER_ROTATION => {
            let index = match buckets.eulers.get(path) {
                Some(index) => *index,
                None => {
                    if !clip.has_euler_curves {
                        return Ok(());
                    }
                    let mut curve = Vector3CurveObject {
                        path: path.to_owned(),
                        ..Vector3CurveObject::default()
                    };
                    let mut view = crate::resources::curve_wrappers::Vector3CurveView {
                        envelope: crate::resources::curve_wrappers::CurveEnvelope {
                            rotation_order: clip.curve_presence().rotation_order.then_some(0),
                            ..Default::default()
                        },
                        ..Default::default()
                    };
                    vector3_curve_set_values_with_rotation_order(
                        &mut view,
                        path,
                        i32::from(binding.custom_type),
                    );
                    curve.envelope = view.envelope;
                    clip.euler_curves.push(curve);
                    let index = clip.euler_curves.len() - 1;
                    buckets.eulers.insert(path.to_owned(), index);
                    index
                }
            };
            let mut key = KeyframeVector3View::for_version(version);
            key.value = vec3(
                curve_values[offset],
                curve_values[offset + 1],
                curve_values[offset + 2],
            );
            key.in_slope = vec3(in_slope_values[0], in_slope_values[1], in_slope_values[2]);
            key.out_slope = vec3(
                out_slope_values[0],
                out_slope_values[1],
                out_slope_values[2],
            );
            key.time = time;
            key.tangent_mode = tangent_mode;
            key.weighted_mode = Some(WEIGHTED_MODE_NONE);
            key.in_weight = Some(vec3(
                DEFAULT_FLOAT_WEIGHT,
                DEFAULT_FLOAT_WEIGHT,
                DEFAULT_FLOAT_WEIGHT,
            ));
            key.out_weight = Some(vec3(
                DEFAULT_FLOAT_WEIGHT,
                DEFAULT_FLOAT_WEIGHT,
                DEFAULT_FLOAT_WEIGHT,
            ));
            clip.euler_curves[index].curve.push(key);
        }
        other => {
            return Err(ConvertError::Convert(ClipConvertError::InvalidTransform(
                CustomCurveError::InvalidTransformType { value: other.0 },
            )));
        }
    }
    Ok(())
}

/// `AnimationClipConverter.AddDefaultCurve` routing plus its three emission sites.
#[allow(clippy::too_many_arguments)]
fn add_default_curve(
    clip: &mut AnimationClipObject,
    binding: ConverterBinding,
    path: &str,
    time: f32,
    value: f32,
    in_tangent: f32,
    out_tangent: f32,
    context: &mut ConverterContext<'_>,
    buckets: &mut Buckets,
) -> Result<(), ConvertError> {
    match default_curve_target(binding.class_id) {
        DefaultCurveTarget::GameObject => {
            let attribute =
                match field_hashes::try_get_path(CLASS_ID_GAME_OBJECT, binding.attribute) {
                    Some(found) => found.to_owned(),
                    None => get_reversed_path(MISSED_PROPERTY_PREFIX, binding.attribute),
                };
            let key = CurveDataKey {
                path: path.to_owned(),
                attribute,
                class_id: CLASS_ID_GAME_OBJECT,
                script: None,
            };
            add_float_keyframe(
                clip,
                key,
                time,
                value,
                in_tangent,
                out_tangent,
                context,
                buckets,
            )
        }
        DefaultCurveTarget::Script => {
            if let Some(script) = binding.script {
                if let Some(facts) = context.scene.script_facts(script) {
                    if context.processed_scripts.insert(&facts.identity) {
                        let fields = facts.fields.clone();
                        context.checksum_cache.add_fields(&fields, "", 0)?;
                    }
                }
            }
            let attribute = match context.checksum_cache.try_get_path(binding.attribute) {
                Some(found) => found.to_owned(),
                None => get_reversed_path(SCRIPT_PROPERTY_PREFIX, binding.attribute),
            };
            let key = CurveDataKey {
                path: path.to_owned(),
                attribute,
                class_id: CLASS_ID_MONO_BEHAVIOUR,
                script: binding.script,
            };
            emit_keyframe(
                clip,
                key,
                time,
                value,
                in_tangent,
                out_tangent,
                binding.is_pptr_curve,
                context,
                buckets,
            )
        }
        DefaultCurveTarget::Engine => {
            let attribute = match field_hashes::try_get_path(binding.class_id, binding.attribute) {
                Some(found) => found.to_owned(),
                None => get_reversed_path(TYPE_TREE_PROPERTY_PREFIX, binding.attribute),
            };
            let key = CurveDataKey {
                path: path.to_owned(),
                attribute,
                class_id: binding.class_id,
                script: None,
            };
            emit_keyframe(
                clip,
                key,
                time,
                value,
                in_tangent,
                out_tangent,
                binding.is_pptr_curve,
                context,
                buckets,
            )
        }
    }
}

/// `AnimationClipConverter.AddCustomCurve`.
#[allow(clippy::too_many_arguments)]
fn add_custom_curve(
    clip: &mut AnimationClipObject,
    binding: ConverterBinding,
    path: &str,
    time: f32,
    value: f32,
    in_tangent: f32,
    out_tangent: f32,
    context: &mut ConverterContext<'_>,
    buckets: &mut Buckets,
) -> Result<(), ConvertError> {
    let custom_type = BindingCustomType(binding.custom_type);
    if custom_type == BindingCustomType::ANIMATOR_MUSCLE {
        let attribute = crate::resources::clip_convert::animator_muscle_attribute(
            binding.attribute,
            clip.version,
        )?;
        let key = CurveDataKey {
            path: String::new(),
            attribute,
            class_id: CLASS_ID_ANIMATOR,
            script: None,
        };
        return add_float_keyframe(
            clip,
            key,
            time,
            value,
            in_tangent,
            out_tangent,
            context,
            buckets,
        );
    }

    let scene = resolve_custom_curve_scene(custom_type, binding.attribute, path, context.scene);
    let attribute = to_attribute_name(custom_type, binding.attribute, path, &scene)?;
    let key = CurveDataKey {
        path: path.to_owned(),
        attribute,
        class_id: binding.class_id,
        script: binding.script,
    };
    if binding.is_pptr_curve {
        add_pptr_keyframe(clip, key, time, value as i32, buckets)
    } else {
        add_float_keyframe(
            clip,
            key,
            time,
            value,
            in_tangent,
            out_tangent,
            context,
            buckets,
        )
    }
}

/// The `CustomCurveResolver` root/child/component walks for the two scene-dependent
/// binding types. Every other branch ignores the scene.
fn resolve_custom_curve_scene<'a>(
    custom_type: BindingCustomType,
    attribute: u32,
    path: &str,
    scene: &'a dyn AnimationSceneFacts,
) -> CustomCurveSceneFacts<'a> {
    let mut facts = CustomCurveSceneFacts::default();
    if is_unknown_path(path) {
        return facts;
    }
    match custom_type {
        BindingCustomType::BLEND_SHAPE => {
            for root in scene.roots() {
                let Some(child) = scene.find_child(*root, path) else {
                    continue;
                };
                let Some(skin) = scene.component(child, &[CLASS_ID_SKINNED_MESH_RENDERER]) else {
                    continue;
                };
                let Some(mesh) = scene.skinned_mesh(skin) else {
                    continue;
                };
                if let Some(name) = scene.blend_shape_name(mesh, attribute) {
                    facts.blend_shape = Some(name);
                    return facts;
                }
            }
        }
        BindingCustomType::RENDERER_MATERIAL => {
            let crc28 = attribute & 0x0FFF_FFFF;
            for root in scene.roots() {
                let Some(child) = scene.find_child(*root, path) else {
                    continue;
                };
                let Some(renderer) = scene.component(child, RENDERER_CLASS_IDS) else {
                    continue;
                };
                if let Some(name) = scene.material_property_name(renderer, crc28) {
                    facts.material_property = Some(name);
                    return facts;
                }
            }
        }
        _ => {}
    }
    facts
}

#[allow(clippy::too_many_arguments)]
fn emit_keyframe(
    clip: &mut AnimationClipObject,
    key: CurveDataKey,
    time: f32,
    value: f32,
    in_tangent: f32,
    out_tangent: f32,
    is_pptr_curve: bool,
    context: &mut ConverterContext<'_>,
    buckets: &mut Buckets,
) -> Result<(), ConvertError> {
    if is_pptr_curve {
        add_pptr_keyframe(clip, key, time, value as i32, buckets)
    } else {
        add_float_keyframe(
            clip,
            key,
            time,
            value,
            in_tangent,
            out_tangent,
            context,
            buckets,
        )
    }
}

/// `AnimationClipConverter.AddFloatKeyframe`.
#[allow(clippy::too_many_arguments)]
fn add_float_keyframe(
    clip: &mut AnimationClipObject,
    key: CurveDataKey,
    time: f32,
    value: f32,
    in_tangent: f32,
    out_tangent: f32,
    _context: &mut ConverterContext<'_>,
    buckets: &mut Buckets,
) -> Result<(), ConvertError> {
    let index = match buckets.floats.get(&key) {
        Some(index) => *index,
        None => {
            let mut curve = FloatCurveObject {
                path: key.path.clone(),
                attribute: key.attribute.clone(),
                class_id: key.class_id,
                script: key.script,
                flags: 0,
                ..FloatCurveObject::default()
            };
            let mut envelope = clip.float_curve_view();
            set_default_rotation_order_and_curve_loop_type(&mut envelope);
            curve.envelope = crate::resources::curve_wrappers::CurveEnvelope {
                pre_infinity: envelope.pre_infinity,
                post_infinity: envelope.post_infinity,
                rotation_order: envelope.rotation_order,
            };
            clip.float_curves.push(curve);
            let index = clip.float_curves.len() - 1;
            buckets.floats.insert(key, index);
            index
        }
    };
    let presence = keyframe_presence(clip.version);
    let mut keyframe = KeyframeView {
        in_weight: presence.in_weight.then_some(0.0),
        out_weight: presence.out_weight.then_some(0.0),
        weighted_mode: presence.weighted_mode.then_some(0),
        ..KeyframeView::default()
    };
    set_values_slopes(
        &mut keyframe,
        clip.version,
        time,
        value,
        in_tangent,
        out_tangent,
        DEFAULT_FLOAT_WEIGHT,
    );
    clip.float_curves[index].curve.push(keyframe);
    Ok(())
}

/// `AnimationClipConverter.AddPPtrKeyframe`.
fn add_pptr_keyframe(
    clip: &mut AnimationClipObject,
    key: CurveDataKey,
    time: f32,
    index: i32,
    buckets: &mut Buckets,
) -> Result<(), ConvertError> {
    let bucket = match buckets.pptrs.get(&key) {
        Some(bucket) => *bucket,
        None => {
            if !clip.has_pptr_curves {
                return Ok(());
            }
            clip.pptr_curves.push(PPtrCurveObject {
                path: key.path.clone(),
                attribute: key.attribute.clone(),
                class_id: key.class_id,
                script: key.script,
                flags: PPTR_CURVE_FLAGS,
                curve: Vec::new(),
            });
            let bucket = clip.pptr_curves.len() - 1;
            buckets.pptrs.insert(key, bucket);
            bucket
        }
    };
    let value =
        if index < 0 {
            None
        } else {
            let position = index as usize;
            Some(*clip.pptr_curve_mapping.get(position).ok_or(
                ConvertError::PptrMappingOutOfRange {
                    index,
                    length: clip.pptr_curve_mapping.len(),
                },
            )?)
        };
    clip.pptr_curves[bucket].curve.push(PPtrKeyframeObject {
        time,
        value: value.flatten(),
    });
    Ok(())
}

/// `AnimationClipConverter.GetCurvePath`.
fn get_curve_path(cache: &PathChecksumCache, hash: u32) -> String {
    match cache.try_get_path(hash) {
        Some(path) => path.to_owned(),
        None => get_reversed_path(UNKNOWN_PATH_PREFIX, hash),
    }
}

/// `AnimationClipConverter.GetReversedPath`.
pub fn get_reversed_path(prefix: &str, hash: u32) -> String {
    reverse_ascii_crc32(&format!("{prefix}0x{hash:X}_"), hash)
}

/// Re-exported so callers can pin the exact rounding the constant-time rule uses.
pub use crate::resources::clip_convert::calculate_last_constant_time;

/// A three-component value as the converter writes it.
fn vec3(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3 { x, y, z }
}

/// A four-component value as the converter writes it.
fn quat(x: f32, y: f32, z: f32, w: f32) -> Quat {
    Quat { x, y, z, w }
}
