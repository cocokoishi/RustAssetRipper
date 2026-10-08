//! Curve-wrapper `SetValues` extensions.
//!
//! Mirrors `QuaternionCurveExtensions`, `Vector3CurveExtensions`, `FloatCurveExtensions`, and
//! `PPtrCurveExtensions`. Each sets the curve path (and, for float/PPtr curves, the attribute,
//! class id, and script pointer); the quaternion/vector/float wrappers additionally reset the
//! curve envelope to the default cycle-with-offset loop and ZXY rotation order. The script
//! pointer arrives as a caller-resolved path id because `SetAsset` needs the live collection.

use super::animation_curve::{CURVE_LOOP_CYCLE_WITH_OFFSET, ROTATION_ORDER_ZXY};

/// The envelope shared by the quaternion/vector/single curves.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CurveEnvelope {
    /// `PreInfinity`.
    pub pre_infinity: i32,
    /// `PostInfinity`.
    pub post_infinity: i32,
    /// `RotationOrder`: present only from Unity 5.4.
    pub rotation_order: Option<i32>,
}

fn set_default_envelope(envelope: &mut CurveEnvelope) {
    envelope.pre_infinity = CURVE_LOOP_CYCLE_WITH_OFFSET;
    envelope.post_infinity = CURVE_LOOP_CYCLE_WITH_OFFSET;
    if let Some(slot) = &mut envelope.rotation_order {
        *slot = ROTATION_ORDER_ZXY;
    }
}

/// A script pointer, modeled as the referenced asset's path id.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct ScriptReference {
    /// The referenced script's path id.
    pub path_id: i64,
}

/// A quaternion-curve view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QuaternionCurveView {
    /// `Path`.
    pub path: String,
    /// `Curve` envelope.
    pub envelope: CurveEnvelope,
}

/// `QuaternionCurveExtensions.SetValues`.
pub fn quaternion_curve_set_values(curve: &mut QuaternionCurveView, path: &str) {
    curve.path = path.to_string();
    set_default_envelope(&mut curve.envelope);
}

/// A vector3-curve view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Vector3CurveView {
    /// `Path`.
    pub path: String,
    /// `Curve` envelope.
    pub envelope: CurveEnvelope,
}

/// `Vector3CurveExtensions.SetValues(path)`.
pub fn vector3_curve_set_values(curve: &mut Vector3CurveView, path: &str) {
    curve.path = path.to_string();
    set_default_envelope(&mut curve.envelope);
}

/// `Vector3CurveExtensions.SetValues(path, rotationOrder)`.
pub fn vector3_curve_set_values_with_rotation_order(
    curve: &mut Vector3CurveView,
    path: &str,
    rotation_order: i32,
) {
    curve.path = path.to_string();
    set_default_envelope(&mut curve.envelope);
    if let Some(slot) = &mut curve.envelope.rotation_order {
        *slot = rotation_order;
    }
}

/// A float-curve view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FloatCurveView {
    /// `Path`.
    pub path: String,
    /// `Attribute`.
    pub attribute: String,
    /// `ClassID`.
    pub class_id: i32,
    /// `Script`.
    pub script: Option<ScriptReference>,
    /// `Curve` envelope.
    pub envelope: CurveEnvelope,
}

/// `FloatCurveExtensions.SetValues`.
pub fn float_curve_set_values(
    curve: &mut FloatCurveView,
    path: &str,
    attribute: &str,
    class_id: i32,
    script: ScriptReference,
) {
    curve.path = path.to_string();
    curve.attribute = attribute.to_string();
    curve.class_id = class_id;
    curve.script = Some(script);
    set_default_envelope(&mut curve.envelope);
}

/// A PPtr-curve view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PPtrCurveView {
    /// `Path`.
    pub path: String,
    /// `Attribute`.
    pub attribute: String,
    /// `ClassID`.
    pub class_id: i32,
    /// `Script`.
    pub script: Option<ScriptReference>,
}

/// `PPtrCurveExtensions.SetValues`.
pub fn pptr_curve_set_values(
    curve: &mut PPtrCurveView,
    path: &str,
    attribute: &str,
    class_id: i32,
    script: ScriptReference,
) {
    curve.path = path.to_string();
    curve.attribute = attribute.to_string();
    curve.class_id = class_id;
    curve.script = Some(script);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::animation_curve::{CURVE_LOOP_CYCLE_WITH_OFFSET, ROTATION_ORDER_YZX};

    fn envelope(rotation_present: bool) -> CurveEnvelope {
        CurveEnvelope {
            rotation_order: rotation_present.then_some(0),
            ..CurveEnvelope::default()
        }
    }

    fn assert_envelope(line: &str, envelope: &CurveEnvelope, parts: &[&str]) {
        assert_eq!(
            envelope.pre_infinity,
            parts[7].parse::<i32>().unwrap(),
            "{line}"
        );
        assert_eq!(
            envelope.post_infinity,
            parts[8].parse::<i32>().unwrap(),
            "{line}"
        );
        assert_eq!(
            envelope.rotation_order,
            (parts[9] == "1").then_some(parts[10].parse::<i32>().unwrap()),
            "{line}"
        );
    }

    /// Byte-for-byte differential against the reference curve-wrapper extensions (probe
    /// `Temp/opencode/curve3probe`).
    #[test]
    fn curve_wrappers_match_the_reference_runtime() {
        let fixture = include_str!("curve_wrappers_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "CW", "{line}");
            match parts[1] {
                "QC" => {
                    let mut curve = QuaternionCurveView {
                        envelope: envelope(parts[9] == "1"),
                        ..QuaternionCurveView::default()
                    };
                    quaternion_curve_set_values(&mut curve, parts[3]);
                    assert_eq!(curve.path, parts[3], "{line}");
                    assert_envelope(line, &curve.envelope, &parts);
                }
                "V3" => {
                    let mut curve = Vector3CurveView {
                        envelope: envelope(parts[9] == "1"),
                        ..Vector3CurveView::default()
                    };
                    vector3_curve_set_values(&mut curve, parts[3]);
                    assert_eq!(curve.path, parts[3], "{line}");
                    assert_envelope(line, &curve.envelope, &parts);
                }
                "V3R" => {
                    let mut curve = Vector3CurveView {
                        envelope: envelope(parts[9] == "1"),
                        ..Vector3CurveView::default()
                    };
                    vector3_curve_set_values_with_rotation_order(
                        &mut curve,
                        parts[3],
                        ROTATION_ORDER_YZX,
                    );
                    assert_eq!(curve.path, parts[3], "{line}");
                    assert_envelope(line, &curve.envelope, &parts);
                }
                "FC" => {
                    let mut curve = FloatCurveView {
                        envelope: envelope(parts[9] == "1"),
                        ..FloatCurveView::default()
                    };
                    let script = ScriptReference { path_id: 1 };
                    float_curve_set_values(
                        &mut curve,
                        parts[3],
                        parts[4],
                        parts[5].parse().unwrap(),
                        script,
                    );
                    assert_eq!(curve.path, parts[3], "{line}");
                    assert_eq!(curve.attribute, parts[4], "{line}");
                    assert_eq!(curve.class_id, parts[5].parse::<i32>().unwrap(), "{line}");
                    assert_eq!(curve.script, (parts[6] == "1").then_some(script), "{line}");
                    assert_envelope(line, &curve.envelope, &parts);
                }
                "PC" => {
                    let mut curve = PPtrCurveView::default();
                    let script = ScriptReference { path_id: 1 };
                    pptr_curve_set_values(
                        &mut curve,
                        parts[3],
                        parts[4],
                        parts[5].parse().unwrap(),
                        script,
                    );
                    assert_eq!(curve.path, parts[3], "{line}");
                    assert_eq!(curve.attribute, parts[4], "{line}");
                    assert_eq!(curve.class_id, parts[5].parse::<i32>().unwrap(), "{line}");
                    assert_eq!(curve.script, (parts[6] == "1").then_some(script), "{line}");
                }
                other => panic!("unknown curve wrapper {other} in {line}"),
            }
            rows += 1;
        }
        assert_eq!(rows, 20);
    }

    #[test]
    fn set_values_overwrites_junk_destinations() {
        let junk = CurveEnvelope {
            pre_infinity: 7,
            post_infinity: 9,
            rotation_order: Some(9),
        };
        // Quaternion: envelope resets, path overwrites.
        let mut curve = QuaternionCurveView {
            path: "old".to_string(),
            envelope: junk,
        };
        quaternion_curve_set_values(&mut curve, "new");
        assert_eq!(curve.path, "new");
        assert_eq!(curve.envelope.pre_infinity, CURVE_LOOP_CYCLE_WITH_OFFSET);
        assert_eq!(curve.envelope.post_infinity, CURVE_LOOP_CYCLE_WITH_OFFSET);
        assert_eq!(
            curve.envelope.rotation_order,
            Some(crate::resources::animation_curve::ROTATION_ORDER_ZXY)
        );
        // Vector3 explicit order overrides junk; the legacy slot drops it.
        let mut curve = Vector3CurveView {
            path: "old".to_string(),
            envelope: junk,
        };
        vector3_curve_set_values_with_rotation_order(&mut curve, "new", ROTATION_ORDER_YZX);
        assert_eq!(curve.path, "new");
        assert_eq!(curve.envelope.rotation_order, Some(ROTATION_ORDER_YZX));
        // The plain Vector3 overload resets junk the same way, without an order.
        let mut plain = Vector3CurveView {
            path: "old".to_string(),
            envelope: junk,
        };
        vector3_curve_set_values(&mut plain, "new");
        assert_eq!(plain.path, "new");
        assert_eq!(plain.envelope.pre_infinity, CURVE_LOOP_CYCLE_WITH_OFFSET);
        assert_eq!(plain.envelope.post_infinity, CURVE_LOOP_CYCLE_WITH_OFFSET);
        assert_eq!(
            plain.envelope.rotation_order,
            Some(crate::resources::animation_curve::ROTATION_ORDER_ZXY)
        );
        let mut legacy = Vector3CurveView {
            path: "old".to_string(),
            envelope: CurveEnvelope {
                rotation_order: None,
                ..junk
            },
        };
        vector3_curve_set_values_with_rotation_order(&mut legacy, "new", ROTATION_ORDER_YZX);
        assert_eq!(legacy.envelope.rotation_order, None);
        // Float: every field lands, including the script pointer.
        let script = ScriptReference { path_id: 42 };
        let mut curve = FloatCurveView {
            path: "old".to_string(),
            attribute: "old".to_string(),
            class_id: 1,
            script: Some(ScriptReference { path_id: 1 }),
            envelope: junk,
        };
        float_curve_set_values(&mut curve, "p", "a", 115, script);
        assert_eq!(curve.path, "p");
        assert_eq!(curve.attribute, "a");
        assert_eq!(curve.class_id, 115);
        assert_eq!(curve.script, Some(script));
        assert_eq!(curve.envelope.pre_infinity, CURVE_LOOP_CYCLE_WITH_OFFSET);
        // PPtr: no envelope exists; the identity fields overwrite.
        let mut curve = PPtrCurveView {
            path: "old".to_string(),
            attribute: "old".to_string(),
            class_id: 1,
            script: None,
        };
        pptr_curve_set_values(&mut curve, "p", "a", 115, script);
        assert_eq!(curve.path, "p");
        assert_eq!(curve.attribute, "a");
        assert_eq!(curve.class_id, 115);
        assert_eq!(curve.script, Some(script));
    }
}
