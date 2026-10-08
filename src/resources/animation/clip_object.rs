//! Mutable generated `AnimationClip` objects for the conversion pipeline.
//!
//! `AnimationClipConverter` mutates live generated clip objects: it appends curve
//! wrappers, sets their envelopes, and adds keyframes. This module holds the mutable
//! destination those writes target, mirroring the serialized `AnimationClip` fields the
//! converter touches (`PositionCurves`, `RotationCurves`, `ScaleCurves`, `EulerCurves`,
//! `FloatCurves`, `PPtrCurves`, `Events`, `SampleRate`, `MuscleClip`, and
//! `ClipBindingConstant`).
//!
//! Every curve list keeps serialized order; the converter appends, so encounter order is
//! the observable order. Events are carried through untouched, because the converter
//! never rewrites them.

use crate::numerics::{Quat, Vec3};

use crate::resources::animation_curve::{
    AnimationCurveQuaternionView, AnimationCurveVector3View, AnimationCurveView, CurvePresence,
};
use crate::resources::curve_wrappers::CurveEnvelope;
use crate::resources::keyframe::KeyframeView;
use crate::resources::keyframe_quaternion::KeyframeQuaternionView;
use crate::resources::keyframe_vector3::KeyframeVector3View;
use crate::resources::muscle_clip_info::MuscleClipInfoView;
use crate::resources::vertex_data_blob::{EndianType, UnityVersion};

/// A PPtr value as the converter copies it between collections.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct PPtrValue {
    /// `m_FileID`.
    pub file_id: i32,
    /// `m_PathID`.
    pub path_id: i64,
}

/// One `GenericBinding` as the converter reads it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct BindingObject {
    /// `Path`.
    pub path: u32,
    /// `Attribute`.
    pub attribute: u32,
    /// `GetClassID()`.
    pub class_id: i32,
    /// `CustomType`.
    pub custom_type: u8,
    /// `IsPPtrCurve() != 0`.
    pub is_pptr_curve: bool,
    /// `Script` resolved to the referenced script asset, if any.
    pub script: Option<crate::resources::curve_wrappers::ScriptReference>,
}

/// One `StreamedClip`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StreamedClipObject {
    /// `Data`.
    pub data: Vec<u32>,
    /// `CurveCount_UInt32`.
    pub curve_count_u32: Option<u32>,
    /// `CurveCount_UInt16`.
    pub curve_count_u16: u16,
    /// `DiscreteCurveCount`.
    pub discrete_curve_count: u16,
}

impl StreamedClipObject {
    /// `StreamedClipExtensions.CurveCount`.
    pub fn curve_count(&self) -> i32 {
        match self.curve_count_u32 {
            Some(count) => count as i32,
            None => self.curve_count_u16 as i32 + self.discrete_curve_count as i32,
        }
    }
}

/// One `DenseClip`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DenseClipObject {
    /// `FrameCount`.
    pub frame_count: i32,
    /// `CurveCount`.
    pub curve_count: u32,
    /// `SampleRate`.
    pub sample_rate: f32,
    /// `BeginTime`.
    pub begin_time: f32,
    /// `SampleArray`.
    pub sample_array: Vec<f32>,
}

/// One `ConstantClip`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConstantClipObject {
    /// `Data`.
    pub data: Vec<f32>,
}

/// The `MuscleClipConstant` fields the converter reads.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MuscleClipObject {
    /// `StreamedClip`.
    pub streamed: StreamedClipObject,
    /// `DenseClip`.
    pub dense: DenseClipObject,
    /// `Has_ConstantClip`.
    pub has_constant_clip: bool,
    /// `ConstantClip`.
    pub constant: ConstantClipObject,
    /// The `MuscleClipConstant` fields copied into `MuscleClipInfo` at the end of
    /// conversion.
    pub constant_view: crate::resources::muscle_clip_info::MuscleClipConstantView,
}

/// One `Vector3Curve`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Vector3CurveObject {
    /// `Path`.
    pub path: String,
    /// `Curve` envelope (`PreInfinity`/`PostInfinity`/`RotationOrder`).
    pub envelope: CurveEnvelope,
    /// `Curve.Curve` in serialized order.
    pub curve: Vec<KeyframeVector3View>,
}

/// One `QuaternionCurve`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QuaternionCurveObject {
    /// `Path`.
    pub path: String,
    /// `Curve` envelope.
    pub envelope: CurveEnvelope,
    /// `Curve.Curve` in serialized order.
    pub curve: Vec<KeyframeQuaternionView>,
}

/// One `FloatCurve`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FloatCurveObject {
    /// `Path`.
    pub path: String,
    /// `Attribute`.
    pub attribute: String,
    /// `ClassID`.
    pub class_id: i32,
    /// `Script`.
    pub script: Option<crate::resources::curve_wrappers::ScriptReference>,
    /// `Flags`.
    pub flags: i32,
    /// `Curve` envelope.
    pub envelope: CurveEnvelope,
    /// `Curve.Curve` in serialized order.
    pub curve: Vec<KeyframeView>,
}

/// One `PPtrKeyframe`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PPtrKeyframeObject {
    /// `Time`.
    pub time: f32,
    /// `Value` (`None` for the source's null PPtr write).
    pub value: Option<PPtrValue>,
}

/// One `PPtrCurve`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PPtrCurveObject {
    /// `Path`.
    pub path: String,
    /// `Attribute`.
    pub attribute: String,
    /// `ClassID`.
    pub class_id: i32,
    /// `Script`.
    pub script: Option<crate::resources::curve_wrappers::ScriptReference>,
    /// `Flags`.
    pub flags: i32,
    /// `Curve` in serialized order.
    pub curve: Vec<PPtrKeyframeObject>,
}

/// One `AnimationEvent`, carried through conversion untouched.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClipEventObject {
    /// `FunctionName`.
    pub function_name: String,
    /// `Data`.
    pub data: String,
    /// `FloatParameter`.
    pub float_parameter: f32,
    /// `IntParameter`.
    pub int_parameter: i32,
    /// `MessageOptions`.
    pub message_options: i32,
    /// `ObjectReferenceParameter`.
    pub object_reference_parameter: Option<PPtrValue>,
    /// `Time`.
    pub time: f32,
}

/// A mutable generated `AnimationClip` for the conversion pipeline.
#[derive(Clone, Debug, PartialEq)]
pub struct AnimationClipObject {
    /// The owning collection's Unity version.
    pub version: UnityVersion,
    /// The owning collection's endianness.
    pub endian: EndianType,
    /// `Has_ClipBindingConstant_C74`: conversion only runs when this holds.
    pub has_clip_binding_constant: bool,
    /// `ClipBindingConstant.GenericBindings`.
    pub bindings: Vec<BindingObject>,
    /// `ClipBindingConstant.PptrCurveMapping`.
    pub pptr_curve_mapping: Vec<Option<PPtrValue>>,
    /// `MuscleClip_C74`.
    pub muscle_clip: MuscleClipObject,
    /// `MuscleClipInfo_C74`.
    pub muscle_clip_info: MuscleClipInfoView,
    /// `Has_EulerCurves_C74`.
    pub has_euler_curves: bool,
    /// `Has_PPtrCurves_C74`.
    pub has_pptr_curves: bool,
    /// `SampleRate_C74`.
    pub sample_rate: f32,
    /// `Has_Legacy_C74`.
    pub has_legacy: bool,
    /// `Legacy_C74`.
    pub legacy: bool,
    /// `AnimationType_C74`.
    pub animation_type: i32,
    /// `PositionCurves_C74`.
    pub position_curves: Vec<Vector3CurveObject>,
    /// `RotationCurves_C74`.
    pub rotation_curves: Vec<QuaternionCurveObject>,
    /// `ScaleCurves_C74`.
    pub scale_curves: Vec<Vector3CurveObject>,
    /// `EulerCurves_C74`.
    pub euler_curves: Vec<Vector3CurveObject>,
    /// `FloatCurves_C74`.
    pub float_curves: Vec<FloatCurveObject>,
    /// `PPtrCurves_C74`.
    pub pptr_curves: Vec<PPtrCurveObject>,
    /// `Events_C74`.
    pub events: Vec<ClipEventObject>,
}

impl AnimationClipObject {
    /// The version gate for the optional curve-envelope `RotationOrder` field.
    pub fn curve_presence(&self) -> CurvePresence {
        crate::resources::animation_curve::curve_presence(self.version)
    }

    /// `AnimationClipExtensions.GetLegacy`.
    pub fn get_legacy(&self) -> bool {
        if self.has_legacy {
            self.legacy
        } else {
            self.animation_type == crate::resources::animation_clip::ANIMATION_TYPE_LEGACY
        }
    }

    /// `AnimationClipExtensions.SupportsNegativeInfinitySlopes`.
    pub fn supports_negative_infinity_slopes(&self) -> bool {
        self.version.major > 2021
    }

    /// Build the single-float envelope view used by `SetValues`.
    pub fn float_curve_view(&self) -> AnimationCurveView {
        AnimationCurveView {
            rotation_order: self.curve_presence().rotation_order.then_some(0),
            ..AnimationCurveView::default()
        }
    }

    /// Build the Vector3 envelope view used by `SetValues`.
    pub fn vector3_curve_view(&self) -> AnimationCurveVector3View {
        AnimationCurveVector3View {
            rotation_order: self.curve_presence().rotation_order.then_some(0),
            ..AnimationCurveVector3View::default()
        }
    }

    /// Build the Quaternion envelope view used by `SetValues`.
    pub fn quaternion_curve_view(&self) -> AnimationCurveQuaternionView {
        AnimationCurveQuaternionView {
            rotation_order: self.curve_presence().rotation_order.then_some(0),
            ..AnimationCurveQuaternionView::default()
        }
    }
}

/// A three-component value as the converter writes it.
pub fn vec3(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3 { x, y, z }
}

/// A four-component value as the converter writes it.
pub fn quat(x: f32, y: f32, z: f32, w: f32) -> Quat {
    Quat { x, y, z, w }
}
