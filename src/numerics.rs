//! Rotation and vector math ported from `AssetRipper.Numerics`.
//!
//! The baseline builds on `System.Numerics` single-precision vectors and quaternions. Only
//! the operation layer is ported here; the types themselves are plain field structs:
//!
//! - `Vector3Extensions.ToQuaternion` converts Euler angles with Unity-specific sign
//!   conventions (negated `W`/`X`, mixed `Y`/`Z` terms) in `f64` intermediates, casting to
//!   `f32` only at the end.
//! - `QuaternionExtensions.ToEulerAngle` rebuilds a rotation matrix in `f64` from
//!   sign-flipped components, normalizes by the squared norm, and selects the regular or
//!   gimbal-locked arctangent branch at sixteen times the float epsilon. The norm
//!   accumulation itself stays in `f32`, widened only afterwards, exactly like the source.
//! - Predicates use exact source thresholds: unit quaternions exceed `1 - 1e-5` on the
//!   `f32` norm, dot-equality exceeds `1 - 1e-5` on the `f64` dot, and normalization zeroes
//!   vectors at or below `1e-5` length. Indexed quaternion access throws out-of-range
//!   errors past index 3 here as explicit errors.
//! - `GeometricMath.AngleFrom3Points` returns clockwise degrees from two arctangents.
//! - `Transformation` composes scale * rotation * translation matrices in the source
//!   row-vector order, with affine point transforms, transposes, fourth-row resets, and
//!   component inverses (zero rotations map to identity). Matrix inversion runs in `f64`
//!   and rounds once, so inverse comparisons use tolerance rather than bit identity.
//! - `Vector3Extensions.Normalize` is dead code in the baseline: the extension method takes
//!   its struct by value, so no caller ever observes the mutation (the snapshot has zero
//!   callers, verified by whole-`Source` search). The Rust `normalized` returns the true
//!   unit vector instead; the differential below pins the mathematically correct outputs.
//!
//! `f64` transcendental results may differ in the last ulp across runtimes, so round-trip
//! tests use tight tolerances rather than claiming cross-runtime bit identity.

/// The shared `kEpsilon` comparison threshold.
pub const K_EPSILON: f32 = 0.00001;
/// Sixteen times the float epsilon: the gimbal-lock branch threshold.
pub const GIMBAL_TEST_THRESHOLD: f64 = 16.0 * 1.19209290E-07;

/// A single-precision 3D vector mirroring `System.Numerics.Vector3`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// A single-precision quaternion mirroring `System.Numerics.Quaternion`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Quat {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

/// An out-of-range quaternion component index.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct QuaternionIndexError {
    pub index: i32,
}

impl core::fmt::Display for QuaternionIndexError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "Index {} is out of bound", self.index)
    }
}

impl std::error::Error for QuaternionIndexError {}

impl Vec3 {
    /// `Vector3.Length()` in single precision.
    pub fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    /// `Vector3Extensions.Dot` in single precision.
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// `Vector3Extensions.Normalize`: short vectors become zero instead of NaN.
    pub fn normalized(self) -> Self {
        let length = self.length();
        if length > K_EPSILON {
            Self {
                x: self.x / length,
                y: self.y / length,
                z: self.z / length,
            }
        } else {
            Self {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }
        }
    }

    /// `Vector3Extensions.EqualsByDot` in single precision.
    pub fn equals_by_dot(self, other: Self) -> bool {
        let self_length = self.length();
        let other_length = other.length();
        if self_length < K_EPSILON {
            return other_length < K_EPSILON;
        }
        if other_length < K_EPSILON {
            return false;
        }
        let deviation = 1.0 - (self.dot(other) / self_length / other_length);
        deviation < K_EPSILON
    }

    /// `VectorExtensions.InvertX`.
    pub const fn invert_x(self) -> Self {
        Self {
            x: -self.x,
            y: self.y,
            z: self.z,
        }
    }

    /// `Vector3Extensions.ToQuaternion` with Unity sign conventions.
    pub fn to_quaternion(self, is_degrees: bool) -> Quat {
        fn radians(angle: f64, is_degrees: bool) -> f64 {
            if is_degrees {
                angle * std::f64::consts::PI / 180.0
            } else {
                angle
            }
        }
        let cy = (radians(f64::from(self.z), is_degrees) * 0.5).cos();
        let sy = (radians(f64::from(self.z), is_degrees) * 0.5).sin();
        let cp = (radians(f64::from(self.y), is_degrees) * 0.5).cos();
        let sp = (radians(f64::from(self.y), is_degrees) * 0.5).sin();
        let cr = (radians(f64::from(self.x), is_degrees) * 0.5).cos();
        let sr = (radians(f64::from(self.x), is_degrees) * 0.5).sin();
        Quat {
            w: -((cr * cp * cy) + (sr * sp * sy)) as f32,
            x: -((sr * cp * cy) - (cr * sp * sy)) as f32,
            y: ((cr * sp * cy) + (sr * cp * sy)) as f32,
            z: ((cr * cp * sy) - (sr * sp * cy)) as f32,
        }
    }
}

impl Quat {
    /// `QuaternionExtensions.GetAt`.
    pub fn get_at(self, index: i32) -> Result<f32, QuaternionIndexError> {
        match index {
            0 => Ok(self.x),
            1 => Ok(self.y),
            2 => Ok(self.z),
            3 => Ok(self.w),
            _ => Err(QuaternionIndexError { index }),
        }
    }

    /// `QuaternionExtensions.SetAt`.
    pub fn set_at(&mut self, index: i32, value: f32) -> Result<(), QuaternionIndexError> {
        match index {
            0 => self.x = value,
            1 => self.y = value,
            2 => self.z = value,
            3 => self.w = value,
            _ => return Err(QuaternionIndexError { index }),
        }
        Ok(())
    }

    /// `QuaternionExtensions.FlipSignAt`.
    pub fn flip_sign_at(&mut self, index: i32) -> Result<(), QuaternionIndexError> {
        let current = self.get_at(index)?;
        self.set_at(index, -current)
    }

    /// `QuaternionExtensions.Dot`, widened before multiplying like the source.
    pub fn dot(self, other: Self) -> f64 {
        f64::from(self.x) * f64::from(other.x)
            + f64::from(self.y) * f64::from(other.y)
            + f64::from(self.z) * f64::from(other.z)
            + f64::from(self.w) * f64::from(other.w)
    }

    /// `QuaternionExtensions.IsUnitQuaternion`: the `f32` norm exceeds `1 - epsilon`.
    pub fn is_unit_quaternion(self) -> bool {
        f64::from(self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w)
            > 1.0 - f64::from(K_EPSILON)
    }

    /// `QuaternionExtensions.IsZero`.
    pub fn is_zero(self) -> bool {
        self.x == 0.0 && self.y == 0.0 && self.z == 0.0 && self.w == 0.0
    }

    /// `QuaternionExtensions.EqualsByDot`.
    pub fn equals_by_dot(self, other: Self) -> bool {
        self.dot(other) > 1.0 - f64::from(K_EPSILON)
    }

    /// `QuaternionExtensions.ToEulerAngle` with the source matrix formulation.
    pub fn to_euler_angle(self, as_degrees: bool) -> Vec3 {
        fn angle(radians: f64, as_degrees: bool) -> f32 {
            (if as_degrees {
                radians * 180.0 / std::f64::consts::PI
            } else {
                radians
            }) as f32
        }
        let qx = self.x;
        let qy = -self.y;
        let qz = -self.z;
        let qw = self.w;
        // Single-precision norm accumulation, widened afterwards like the source.
        let norm = f64::from(qx * qx + qy * qy + qz * qz + qw * qw);
        let s = if norm > 0.0 { 2.0 / norm } else { 0.0 };
        let xs = f64::from(qx) * s;
        let ys = f64::from(qy) * s;
        let zs = f64::from(qz) * s;
        let wx = f64::from(qw) * xs;
        let wy = f64::from(qw) * ys;
        let wz = f64::from(qw) * zs;
        let xx = f64::from(qx) * xs;
        let xy = f64::from(qx) * ys;
        let xz = f64::from(qx) * zs;
        let yy = f64::from(qy) * ys;
        let yz = f64::from(qy) * zs;
        let zz = f64::from(qz) * zs;
        let m00 = 1.0 - (yy + zz);
        // The source computes the full matrix; extraction reads neither of these.
        let _m01 = xy - wz;
        let _m02 = xz + wy;
        let m10 = xy + wz;
        let m11 = 1.0 - (xx + zz);
        let m12 = yz - wx;
        let m20 = xz - wy;
        let m21 = yz + wx;
        let m22 = 1.0 - (xx + yy);
        let test = (m00 * m00 + m10 * m10).sqrt();
        let (eax, eay, eaz) = if test > GIMBAL_TEST_THRESHOLD {
            (m21.atan2(m22), (-m20).atan2(test), m10.atan2(m00))
        } else {
            ((-m12).atan2(m11), (-m20).atan2(test), 0.0)
        };
        Vec3 {
            x: angle(eax, as_degrees),
            y: angle(eay, as_degrees),
            z: angle(eaz, as_degrees),
        }
    }
}

/// A single-precision 4D vector mirroring `System.Numerics.Vector4`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec4 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

/// A row-major 4x4 single-precision matrix mirroring `System.Numerics.Matrix4x4`.
///
/// Field order follows the source (`M11..M44` row by row) and multiplication follows the
/// source row-vector convention, so `Transformation` composition matches the baseline.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Mat4 {
    pub m11: f32,
    pub m12: f32,
    pub m13: f32,
    pub m14: f32,
    pub m21: f32,
    pub m22: f32,
    pub m23: f32,
    pub m24: f32,
    pub m31: f32,
    pub m32: f32,
    pub m33: f32,
    pub m34: f32,
    pub m41: f32,
    pub m42: f32,
    pub m43: f32,
    pub m44: f32,
}

/// Failure to invert a singular matrix.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MatrixInvertError;

impl core::fmt::Display for MatrixInvertError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "Could not invert matrix")
    }
}

impl std::error::Error for MatrixInvertError {}

impl Mat4 {
    /// The multiplicative identity matrix.
    pub const fn identity() -> Self {
        Self {
            m11: 1.0,
            m12: 0.0,
            m13: 0.0,
            m14: 0.0,
            m21: 0.0,
            m22: 1.0,
            m23: 0.0,
            m24: 0.0,
            m31: 0.0,
            m32: 0.0,
            m33: 1.0,
            m34: 0.0,
            m41: 0.0,
            m42: 0.0,
            m43: 0.0,
            m44: 1.0,
        }
    }

    /// `Matrix4x4.CreateScale(scale)`.
    pub const fn create_scale(scale: Vec3) -> Self {
        Self {
            m11: scale.x,
            m12: 0.0,
            m13: 0.0,
            m14: 0.0,
            m21: 0.0,
            m22: scale.y,
            m23: 0.0,
            m24: 0.0,
            m31: 0.0,
            m32: 0.0,
            m33: scale.z,
            m34: 0.0,
            m41: 0.0,
            m42: 0.0,
            m43: 0.0,
            m44: 1.0,
        }
    }

    /// `Matrix4x4.CreateTranslation(position)`.
    pub const fn create_translation(translation: Vec3) -> Self {
        Self {
            m11: 1.0,
            m12: 0.0,
            m13: 0.0,
            m14: 0.0,
            m21: 0.0,
            m22: 1.0,
            m23: 0.0,
            m24: 0.0,
            m31: 0.0,
            m32: 0.0,
            m33: 1.0,
            m34: 0.0,
            m41: translation.x,
            m42: translation.y,
            m43: translation.z,
            m44: 1.0,
        }
    }

    /// `Matrix4x4.CreateFromQuaternion`: rotation matrix from a quaternion.
    pub fn create_from_quaternion(q: Quat) -> Self {
        let x2 = q.x + q.x;
        let y2 = q.y + q.y;
        let z2 = q.z + q.z;
        let xx = q.x * x2;
        let xy = q.x * y2;
        let xz = q.x * z2;
        let yy = q.y * y2;
        let yz = q.y * z2;
        let zz = q.z * z2;
        let wx = q.w * x2;
        let wy = q.w * y2;
        let wz = q.w * z2;
        Self {
            m11: 1.0 - yy - zz,
            m12: xy + wz,
            m13: xz - wy,
            m14: 0.0,
            m21: xy - wz,
            m22: 1.0 - xx - zz,
            m23: yz + wx,
            m24: 0.0,
            m31: xz + wy,
            m32: yz - wx,
            m33: 1.0 - xx - yy,
            m34: 0.0,
            m41: 0.0,
            m42: 0.0,
            m43: 0.0,
            m44: 1.0,
        }
    }

    /// Row-vector matrix product, matching `*` on the source matrix type.
    #[allow(clippy::should_implement_trait)]
    pub fn mul(self, other: Self) -> Self {
        Self {
            m11: self.m11 * other.m11
                + self.m12 * other.m21
                + self.m13 * other.m31
                + self.m14 * other.m41,
            m12: self.m11 * other.m12
                + self.m12 * other.m22
                + self.m13 * other.m32
                + self.m14 * other.m42,
            m13: self.m11 * other.m13
                + self.m12 * other.m23
                + self.m13 * other.m33
                + self.m14 * other.m43,
            m14: self.m11 * other.m14
                + self.m12 * other.m24
                + self.m13 * other.m34
                + self.m14 * other.m44,
            m21: self.m21 * other.m11
                + self.m22 * other.m21
                + self.m23 * other.m31
                + self.m24 * other.m41,
            m22: self.m21 * other.m12
                + self.m22 * other.m22
                + self.m23 * other.m32
                + self.m24 * other.m42,
            m23: self.m21 * other.m13
                + self.m22 * other.m23
                + self.m23 * other.m33
                + self.m24 * other.m43,
            m24: self.m21 * other.m14
                + self.m22 * other.m24
                + self.m23 * other.m34
                + self.m24 * other.m44,
            m31: self.m31 * other.m11
                + self.m32 * other.m21
                + self.m33 * other.m31
                + self.m34 * other.m41,
            m32: self.m31 * other.m12
                + self.m32 * other.m22
                + self.m33 * other.m32
                + self.m34 * other.m42,
            m33: self.m31 * other.m13
                + self.m32 * other.m23
                + self.m33 * other.m33
                + self.m34 * other.m43,
            m34: self.m31 * other.m14
                + self.m32 * other.m24
                + self.m33 * other.m34
                + self.m34 * other.m44,
            m41: self.m41 * other.m11
                + self.m42 * other.m21
                + self.m43 * other.m31
                + self.m44 * other.m41,
            m42: self.m41 * other.m12
                + self.m42 * other.m22
                + self.m43 * other.m32
                + self.m44 * other.m42,
            m43: self.m41 * other.m13
                + self.m42 * other.m23
                + self.m43 * other.m33
                + self.m44 * other.m43,
            m44: self.m41 * other.m14
                + self.m42 * other.m24
                + self.m43 * other.m34
                + self.m44 * other.m44,
        }
    }

    /// `Vector3.Transform` affine form used by the source `*` operator.
    ///
    /// The probed runtime ignores the projection column (`M14/M24/M34/M44`) for this path,
    /// so the port does the same rather than performing a perspective divide.
    pub fn transform_point(self, point: Vec3) -> Vec3 {
        Vec3 {
            x: point.x * self.m11 + point.y * self.m21 + point.z * self.m31 + self.m41,
            y: point.x * self.m12 + point.y * self.m22 + point.z * self.m32 + self.m42,
            z: point.x * self.m13 + point.y * self.m23 + point.z * self.m33 + self.m43,
        }
    }

    /// `Matrix4x4.Transpose`.
    pub const fn transpose(self) -> Self {
        Self {
            m11: self.m11,
            m12: self.m21,
            m13: self.m31,
            m14: self.m41,
            m21: self.m12,
            m22: self.m22,
            m23: self.m32,
            m24: self.m42,
            m31: self.m13,
            m32: self.m23,
            m33: self.m33,
            m34: self.m43,
            m41: self.m14,
            m42: self.m24,
            m43: self.m34,
            m44: self.m44,
        }
    }

    /// Matrix inverse via Cramer/adjugate evaluation.
    ///
    /// The arithmetic runs in `f64` and rounds once at the end. The probed runtime works in
    /// `f32` cofactors, so outputs can differ by a few ulps on ill-conditioned inputs; all
    /// port tests therefore compare inverses within `1e-5` per component instead of by bits.
    #[allow(clippy::needless_range_loop)]
    pub fn invert(self) -> Result<Self, MatrixInvertError> {
        let a = self.to_f64();
        let det = det4(&a);
        if det == 0.0 || !det.is_finite() {
            return Err(MatrixInvertError);
        }
        let inv_det = 1.0 / det;
        let mut out = [[0.0_f64; 4]; 4];
        #[allow(clippy::needless_range_loop)]
        for row in 0..4 {
            for col in 0..4 {
                let sign = if (row + col) % 2 == 0 { 1.0 } else { -1.0 };
                out[col][row] = sign * det3(&minor3(&a, row, col)) * inv_det;
            }
        }
        Ok(Self::from_f64(&out))
    }

    fn to_f64(self) -> [[f64; 4]; 4] {
        [
            [
                f64::from(self.m11),
                f64::from(self.m12),
                f64::from(self.m13),
                f64::from(self.m14),
            ],
            [
                f64::from(self.m21),
                f64::from(self.m22),
                f64::from(self.m23),
                f64::from(self.m24),
            ],
            [
                f64::from(self.m31),
                f64::from(self.m32),
                f64::from(self.m33),
                f64::from(self.m34),
            ],
            [
                f64::from(self.m41),
                f64::from(self.m42),
                f64::from(self.m43),
                f64::from(self.m44),
            ],
        ]
    }

    fn from_f64(m: &[[f64; 4]; 4]) -> Self {
        Self {
            m11: m[0][0] as f32,
            m12: m[0][1] as f32,
            m13: m[0][2] as f32,
            m14: m[0][3] as f32,
            m21: m[1][0] as f32,
            m22: m[1][1] as f32,
            m23: m[1][2] as f32,
            m24: m[1][3] as f32,
            m31: m[2][0] as f32,
            m32: m[2][1] as f32,
            m33: m[2][2] as f32,
            m34: m[2][3] as f32,
            m41: m[3][0] as f32,
            m42: m[3][1] as f32,
            m43: m[3][2] as f32,
            m44: m[3][3] as f32,
        }
    }
}

#[allow(clippy::needless_range_loop)]
fn minor3(m: &[[f64; 4]; 4], skip_row: usize, skip_col: usize) -> [[f64; 3]; 3] {
    let mut out = [[0.0_f64; 3]; 3];
    let mut row_out = 0;
    for row in 0..4 {
        if row == skip_row {
            continue;
        }
        let mut col_out = 0;
        for col in 0..4 {
            if col == skip_col {
                continue;
            }
            out[row_out][col_out] = m[row][col];
            col_out += 1;
        }
        row_out += 1;
    }
    out
}

fn det3(m: &[[f64; 3]; 3]) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}

#[allow(clippy::needless_range_loop)]
fn det4(m: &[[f64; 4]; 4]) -> f64 {
    let mut det = 0.0;
    for col in 0..4 {
        let sign = if col % 2 == 0 { 1.0 } else { -1.0 };
        det += sign * m[0][col] * det3(&minor3(m, 0, col));
    }
    det
}

/// A translation/rotation/scale wrapper mirroring `Transformation`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Transformation {
    pub matrix: Mat4,
}

impl Transformation {
    /// `new Transformation(translation, rotation, scale)` via scale * rotation * translation.
    pub fn new(translation: Vec3, rotation: Quat, scale: Vec3) -> Self {
        Self::create(translation, rotation, scale)
    }

    /// The identity transformation.
    pub const IDENTITY: Self = Self {
        matrix: Mat4::identity(),
    };

    /// Identity with an inverted X axis (Unity to glTF handedness switch).
    pub const IDENTITY_WITH_INVERTED_X: Self = Self {
        matrix: Mat4 {
            m11: -1.0,
            m12: 0.0,
            m13: 0.0,
            m14: 0.0,
            m21: 0.0,
            m22: 1.0,
            m23: 0.0,
            m24: 0.0,
            m31: 0.0,
            m32: 0.0,
            m33: 1.0,
            m34: 0.0,
            m41: 0.0,
            m42: 0.0,
            m43: 0.0,
            m44: 1.0,
        },
    };

    /// `Transformation.Create`.
    pub fn create(translation: Vec3, rotation: Quat, scale: Vec3) -> Self {
        Self {
            matrix: Mat4::create_scale(scale)
                .mul(Mat4::create_from_quaternion(rotation))
                .mul(Mat4::create_translation(translation)),
        }
    }

    /// `Transformation.CreateInverse`: negated translation, inverted rotation, reciprocal scale.
    pub fn create_inverse(translation: Vec3, rotation: Quat, scale: Vec3) -> Self {
        let inverse_translation = Vec3 {
            x: -translation.x,
            y: -translation.y,
            z: -translation.z,
        };
        let inverse_rotation = if rotation.is_zero() {
            Quat {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                w: 1.0,
            }
        } else {
            rotation.inverse()
        };
        let inverse_scale = Vec3 {
            x: 1.0 / scale.x,
            y: 1.0 / scale.y,
            z: 1.0 / scale.z,
        };
        Self {
            matrix: Mat4::create_translation(inverse_translation)
                .mul(Mat4::create_from_quaternion(inverse_rotation))
                .mul(Mat4::create_scale(inverse_scale)),
        }
    }

    /// Compose two transformations (`left * right`).
    pub fn compose(self, other: Self) -> Self {
        Self {
            matrix: self.matrix.mul(other.matrix),
        }
    }

    /// Transform a position vector.
    pub fn transform_point(self, point: Vec3) -> Vec3 {
        self.matrix.transform_point(point)
    }

    /// `Invert`, failing on singular matrices like the source exception path.
    pub fn invert(self) -> Result<Self, MatrixInvertError> {
        Ok(Self {
            matrix: self.matrix.invert()?,
        })
    }

    /// `Transpose`.
    pub const fn transpose(self) -> Self {
        // `Mat4::transpose` is const, so this stays const for pipeline use.
        Self {
            matrix: self.matrix.transpose(),
        }
    }

    /// `RemoveTranslation`: zeroes the fourth row except `M44`.
    pub fn remove_translation(self) -> Self {
        let m = self.matrix;
        Self {
            matrix: Mat4 {
                m41: 0.0,
                m42: 0.0,
                m43: 0.0,
                m11: m.m11,
                m12: m.m12,
                m13: m.m13,
                m14: m.m14,
                m21: m.m21,
                m22: m.m22,
                m23: m.m23,
                m24: m.m24,
                m31: m.m31,
                m32: m.m32,
                m33: m.m33,
                m34: m.m34,
                m44: 1.0,
            },
        }
    }
}

impl Quat {
    /// `Quaternion.Inverse`: conjugate over squared length.
    pub fn inverse(self) -> Self {
        let norm_squared = self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w;
        let inv = 1.0 / norm_squared;
        Self {
            x: -self.x * inv,
            y: -self.y * inv,
            z: -self.z * inv,
            w: self.w * inv,
        }
    }

    /// `Quaternion.CreateFromYawPitchRoll` in single precision.
    pub fn create_from_yaw_pitch_roll(yaw: f32, pitch: f32, roll: f32) -> Self {
        let half_roll = roll * 0.5;
        let half_pitch = pitch * 0.5;
        let half_yaw = yaw * 0.5;
        let (sin_roll, cos_roll) = half_roll.sin_cos();
        let (sin_pitch, cos_pitch) = half_pitch.sin_cos();
        let (sin_yaw, cos_yaw) = half_yaw.sin_cos();
        Self {
            x: cos_yaw * sin_pitch * cos_roll + sin_yaw * cos_pitch * sin_roll,
            y: sin_yaw * cos_pitch * cos_roll - cos_yaw * sin_pitch * sin_roll,
            z: cos_yaw * cos_pitch * sin_roll - sin_yaw * sin_pitch * cos_roll,
            w: cos_yaw * cos_pitch * cos_roll + sin_yaw * sin_pitch * sin_roll,
        }
    }
}

/// `GeometricMath.AngleFrom3Points`: clockwise degrees from the middle point.
pub fn angle_from_3_points(p1: [f32; 2], p2: [f32; 2], p3: [f32; 2]) -> f32 {
    let ax = f64::from(p1[0] - p2[0]);
    let ay = f64::from(p1[1] - p2[1]);
    let bx = f64::from(p3[0] - p2[0]);
    let by = f64::from(p3[1] - p2[1]);
    let mut angle = ay.atan2(ax) - by.atan2(bx);
    if angle < 0.0 {
        angle += 2.0 * std::f64::consts::PI;
    }
    (360.0 * angle / (2.0 * std::f64::consts::PI)) as f32
}

#[cfg(test)]
mod tests {
    use super::{angle_from_3_points, Quat, Vec3, GIMBAL_TEST_THRESHOLD, K_EPSILON};

    fn euler(vec: [f32; 3]) -> Vec3 {
        Vec3 {
            x: vec[0],
            y: vec[1],
            z: vec[2],
        }
    }

    fn close(a: Vec3, b: Vec3, tolerance: f32) -> bool {
        (a.x - b.x).abs() <= tolerance
            && (a.y - b.y).abs() <= tolerance
            && (a.z - b.z).abs() <= tolerance
    }

    #[test]
    fn euler_round_trips_match_reference_vectors() {
        // Reference case 1 round-trips within float tolerance.
        let first = euler([-67.0, 45.0, -162.0]);
        let back = first.to_quaternion(true).to_euler_angle(true);
        assert!(close(back, first, 1e-3), "{back:?}");
        // Reference case 2 is only near-equal even on the reference runtime.
        let second = euler([-67.0, 45.0, 178.0]);
        let second_back = second.to_quaternion(true).to_euler_angle(true);
        assert!(second.equals_by_dot(second_back));
    }

    #[test]
    fn converted_quaternions_are_unit_and_indexable() {
        let quaternion = euler([-67.0, 45.0, -182.0]).to_quaternion(true);
        assert!(quaternion.is_unit_quaternion());
        assert_eq!(quaternion.get_at(0), Ok(quaternion.x));
        assert_eq!(quaternion.get_at(3), Ok(quaternion.w));
        assert!(quaternion.get_at(4).is_err());
        assert!(quaternion.get_at(-1).is_err());
        let mut mutable = Quat::default();
        mutable.set_at(2, 0.5).unwrap();
        assert_eq!(mutable.z, 0.5);
        assert!(mutable.set_at(9, 0.5).is_err());
        mutable.flip_sign_at(2).unwrap();
        assert_eq!(mutable.z, -0.5);
        assert!(mutable.flip_sign_at(9).is_err());
        assert!(Quat::default().is_zero());
        assert!(!quaternion.is_zero());
        // q and -q share the dot equality used across animator sampling.
        let negated = Quat {
            x: -quaternion.x,
            y: -quaternion.y,
            z: -quaternion.z,
            w: -quaternion.w,
        };
        // Dot equality is one-sided: q equals itself, but q and -q give dot ≈ -1 even
        // though both rotations coincide (rotation equivalence needs |dot|, elsewhere).
        assert!(quaternion.equals_by_dot(quaternion));
        assert!(!quaternion.equals_by_dot(negated));
    }

    #[test]
    fn vector_predicates_follow_epsilon_rules() {
        assert_eq!(K_EPSILON, 0.00001);
        assert_eq!(GIMBAL_TEST_THRESHOLD, 16.0 * 1.19209290E-07);
        // Short vectors normalize to zero rather than NaN.
        assert_eq!(euler([1e-9, 0.0, 0.0]).normalized(), euler([0.0, 0.0, 0.0]));
        let unit = euler([0.0, 3.0, 4.0]).normalized();
        assert!((unit.length() - 1.0).abs() < 1e-6);
        // Dot equality treats tiny vectors by emptiness, not direction.
        assert!(euler([0.0, 0.0, 0.0]).equals_by_dot(euler([1e-9, 0.0, 0.0])));
        assert!(!euler([1.0, 0.0, 0.0]).equals_by_dot(euler([1e-9, 0.0, 0.0])));
        assert!(euler([1.0, 2.0, 3.0]).equals_by_dot(euler([2.0, 4.0, 6.0])));
        assert!(!euler([1.0, 0.0, 0.0]).equals_by_dot(euler([0.0, 1.0, 0.0])));
        assert_eq!(euler([1.0, -2.0, 3.0]).invert_x(), euler([-1.0, -2.0, 3.0]));
    }

    #[test]
    fn reference_probe_vectors_match_dotnet_bit_exact() {
        // Isolated runtime differential (dotnet SDK 10.0.401, prior isolated build of
        // AssetRipper.Numerics.dll, 35-line battery): every conversion below reproduces the
        // reference float bits exactly. Only Normalize differs, by design (see below).
        fn bits(v: Vec3) -> [u32; 3] {
            [v.x.to_bits(), v.y.to_bits(), v.z.to_bits()]
        }
        fn qbits(q: Quat) -> [u32; 4] {
            [q.x.to_bits(), q.y.to_bits(), q.z.to_bits(), q.w.to_bits()]
        }
        let first = euler([-67.0, 45.0, -162.0]).to_quaternion(true);
        assert_eq!(
            qbits(first),
            [0xBE7110DD, 0x3F0DB67C, 0xBF3A5691, 0xBEA8846F]
        );
        assert_eq!(
            bits(first.to_euler_angle(true)),
            [0xC2860000, 0x42340000, 0xC3220000]
        );
        // The near-equal reference case lands one ulp above 45 degrees, exactly as probed.
        let second = euler([-67.0, 45.0, 178.0]).to_quaternion(true);
        assert_eq!(
            qbits(second),
            [0x3EA7EB0A, 0xBF011841, 0x3F46237C, 0x3E4A7C37]
        );
        assert_eq!(
            bits(second.to_euler_angle(true)),
            [0xC2860000, 0x42340001, 0x43320000]
        );
        // Zero euler angles preserve negative zero through the sign conventions.
        let zero = euler([0.0, 0.0, 0.0]).to_quaternion(true);
        assert_eq!(
            qbits(zero),
            [0x80000000, 0x00000000, 0x00000000, 0xBF800000]
        );
        assert_eq!(
            bits(zero.to_euler_angle(true)),
            [0x00000000, 0x80000000, 0x00000000]
        );
        // Gimbal-locked pitch round-trips through the low-test branch.
        let pitched = euler([0.0, 90.0, 0.0]).to_quaternion(true);
        assert_eq!(
            qbits(pitched),
            [0x80000000, 0x3F3504F3, 0x00000000, 0xBF3504F3]
        );
        assert_eq!(
            bits(pitched.to_euler_angle(true)),
            [0x80000000, 0x42B40000, 0x00000000]
        );
        // Three-point angles hit exact degree encodings.
        assert_eq!(
            angle_from_3_points([1.0, 0.0], [0.0, 0.0], [0.0, 1.0]).to_bits(),
            0x43870000
        );
        assert_eq!(
            angle_from_3_points([0.0, 1.0], [0.0, 0.0], [1.0, 0.0]).to_bits(),
            0x42B40000
        );
        // Normalization returns the true unit vector: the source extension method takes its
        // struct by value, so no baseline caller ever observes its mutation (no snapshot
        // caller exists). The values below are the mathematically correct results.
        assert_eq!(
            bits(euler([0.0, 3.0, 4.0]).normalized()),
            [0x00000000, 0x3F19999A, 0x3F4CCCCD]
        );
        assert_eq!(
            bits(euler([0.0, 0.0, 0.0]).normalized()),
            [0x00000000, 0x00000000, 0x00000000]
        );
    }

    #[test]
    fn three_point_angles_cover_quadrants() {
        // Right angle at the middle point, clockwise degrees.
        assert!((angle_from_3_points([1.0, 0.0], [0.0, 0.0], [0.0, 1.0]) - 270.0).abs() < 1e-4);
        assert!((angle_from_3_points([0.0, 1.0], [0.0, 0.0], [1.0, 0.0]) - 90.0).abs() < 1e-4);
        // Straight lines give 180 degrees either way.
        assert!((angle_from_3_points([1.0, 0.0], [0.0, 0.0], [-1.0, 0.0]) - 180.0).abs() < 1e-4);
    }

    #[test]
    fn transformation_matrices_match_probed_runtime() {
        use super::{Mat4, Transformation};
        fn mbits(m: Mat4) -> [u32; 16] {
            [
                m.m11.to_bits(),
                m.m12.to_bits(),
                m.m13.to_bits(),
                m.m14.to_bits(),
                m.m21.to_bits(),
                m.m22.to_bits(),
                m.m23.to_bits(),
                m.m24.to_bits(),
                m.m31.to_bits(),
                m.m32.to_bits(),
                m.m33.to_bits(),
                m.m34.to_bits(),
                m.m41.to_bits(),
                m.m42.to_bits(),
                m.m43.to_bits(),
                m.m44.to_bits(),
            ]
        }
        fn mat(list: [u32; 16]) -> Mat4 {
            Mat4 {
                m11: f32::from_bits(list[0]),
                m12: f32::from_bits(list[1]),
                m13: f32::from_bits(list[2]),
                m14: f32::from_bits(list[3]),
                m21: f32::from_bits(list[4]),
                m22: f32::from_bits(list[5]),
                m23: f32::from_bits(list[6]),
                m24: f32::from_bits(list[7]),
                m31: f32::from_bits(list[8]),
                m32: f32::from_bits(list[9]),
                m33: f32::from_bits(list[10]),
                m34: f32::from_bits(list[11]),
                m41: f32::from_bits(list[12]),
                m42: f32::from_bits(list[13]),
                m43: f32::from_bits(list[14]),
                m44: f32::from_bits(list[15]),
            }
        }
        fn close_mat(a: Mat4, b: Mat4, tolerance: f32) -> bool {
            let x = mbits(a);
            let y = mbits(b);
            let af = [
                a.m11, a.m12, a.m13, a.m14, a.m21, a.m22, a.m23, a.m24, a.m31, a.m32, a.m33, a.m34,
                a.m41, a.m42, a.m43, a.m44,
            ];
            let bf = [
                b.m11, b.m12, b.m13, b.m14, b.m21, b.m22, b.m23, b.m24, b.m31, b.m32, b.m33, b.m34,
                b.m41, b.m42, b.m43, b.m44,
            ];
            let _ = (x, y);
            af.iter()
                .zip(bf.iter())
                .all(|(p, q)| (p - q).abs() <= tolerance)
        }
        let identity = Quat {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        };
        // Move-then-scale is bit-exact against the isolated dotnet probe.
        let t1 = Transformation::new(euler([0.0, 0.5, 0.0]), identity, euler([1.0, 1.0, 1.0]));
        let t2 = Transformation::new(euler([0.0, 0.0, 0.0]), identity, euler([1.0, 2.0, 1.0]));
        assert_eq!(
            mbits(t1.compose(t2).matrix),
            [
                0x3F800000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x40000000, 0x00000000,
                0x00000000, 0x00000000, 0x00000000, 0x3F800000, 0x00000000, 0x00000000, 0x3F800000,
                0x00000000, 0x3F800000,
            ]
        );
        // Rotated composition matches, including the non-commutative reverse product.
        let q90 = Quat {
            x: 0.0,
            y: 0.0,
            z: core::f32::consts::FRAC_1_SQRT_2,
            w: core::f32::consts::FRAC_1_SQRT_2,
        };
        let u1 = Transformation::new(euler([0.0, 0.5, 0.0]), q90, euler([1.0, 1.0, 1.0]));
        assert_eq!(
            mbits(u1.compose(t2).matrix),
            [
                0x33800000, 0x3FFFFFFF, 0x00000000, 0x00000000, 0xBF7FFFFF, 0x34000000, 0x00000000,
                0x00000000, 0x00000000, 0x00000000, 0x3F800000, 0x00000000, 0x00000000, 0x3F800000,
                0x00000000, 0x3F800000,
            ]
        );
        assert_eq!(
            mbits(t2.compose(u1).matrix),
            [
                0x33800000, 0x3F7FFFFF, 0x00000000, 0x00000000, 0xBFFFFFFF, 0x34000000, 0x00000000,
                0x00000000, 0x00000000, 0x00000000, 0x3F800000, 0x00000000, 0x00000000, 0x3F000000,
                0x00000000, 0x3F800000,
            ]
        );
        // Identity constructors and the handedness switch are bit-exact.
        assert_eq!(
            mbits(
                Transformation::new(euler([0.0, 0.0, 0.0]), identity, euler([1.0, 1.0, 1.0]))
                    .matrix
            ),
            mbits(Transformation::IDENTITY.matrix)
        );
        assert_eq!(
            mbits(Transformation::IDENTITY_WITH_INVERTED_X.matrix),
            [
                0xBF800000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x3F800000, 0x00000000,
                0x00000000, 0x00000000, 0x00000000, 0x3F800000, 0x00000000, 0x00000000, 0x00000000,
                0x00000000, 0x3F800000,
            ]
        );
        // Affine point transform ignores the projection column, as probed.
        let probe_matrix = mat([
            0x40000000, 0x40400000, 0x40800000, 0x40A00000, 0x40C00000, 0x40E00000, 0x41000000,
            0x41100000, 0x41200000, 0x41300000, 0x41400000, 0x41500000, 0x41600000, 0x41700000,
            0x41800000, 0x41880000,
        ]);
        assert_eq!(
            Transformation {
                matrix: probe_matrix
            }
            .transform_point(euler([2.0, 4.0, 6.0])),
            euler([102.0, 115.0, 128.0])
        );
        // Quaternion rotation matrices are bit-exact for fixed inputs.
        assert_eq!(
            mbits(Mat4::create_from_quaternion(Quat {
                x: 1.0,
                y: 2.0,
                z: 3.0,
                w: 4.0
            })),
            [
                0xC1C80000, 0x41E00000, 0xC1200000, 0x00000000, 0xC1A00000, 0xC1980000, 0x41A00000,
                0x00000000, 0x41B00000, 0x40800000, 0xC1100000, 0x00000000, 0x00000000, 0x00000000,
                0x00000000, 0x3F800000,
            ]
        );
        assert_eq!(
            mbits(Mat4::create_from_quaternion(Quat {
                x: 0.5,
                y: 0.5,
                z: 0.5,
                w: 0.5
            })),
            [
                0x00000000, 0x3F800000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x3F800000,
                0x00000000, 0x3F800000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000,
                0x00000000, 0x3F800000,
            ]
        );
        // Transpose and translation removal are bit-exact.
        assert_eq!(
            mbits(
                Transformation {
                    matrix: mat([
                        0x3F800000, 0x40000000, 0x40400000, 0x40800000, 0x40A00000, 0x40C00000,
                        0x40E00000, 0x41000000, 0x41100000, 0x41200000, 0x41300000, 0x41400000,
                        0x41500000, 0x41600000, 0x41700000, 0x41800000,
                    ])
                }
                .transpose()
                .matrix
            ),
            [
                0x3F800000, 0x40A00000, 0x41100000, 0x41500000, 0x40000000, 0x40C00000, 0x41200000,
                0x41600000, 0x40400000, 0x40E00000, 0x41300000, 0x41700000, 0x40800000, 0x41000000,
                0x41400000, 0x41800000,
            ]
        );
        let ypr = Quat::create_from_yaw_pitch_roll(2.0, 0.5, 3.0);
        let with_translation =
            Transformation::create(euler([2.0, 5.0, -8.0]), ypr, euler([0.8, 2.0, 3.0]));
        let without_translation =
            Transformation::create(euler([0.0, 0.0, 0.0]), ypr, euler([0.8, 2.0, 3.0]));
        assert_eq!(
            with_translation.remove_translation().matrix,
            without_translation.matrix
        );
        // Yaw/pitch/roll and quaternion inverse match the probe within float tolerance;
        // transcendental results are not pinned by bits across runtimes.
        let probed_ypr = Quat {
            x: f32::from_bits(0x3F529E1A),
            y: f32::from_bits(0xBD9AF65E),
            z: f32::from_bits(0x3F01E96B),
            w: f32::from_bits(0x3E7A90D2),
        };
        for (actual, expected) in [
            (ypr.x, probed_ypr.x),
            (ypr.y, probed_ypr.y),
            (ypr.z, probed_ypr.z),
            (ypr.w, probed_ypr.w),
        ] {
            assert!((actual - expected).abs() <= 1e-6, "{ypr:?}");
        }
        let probed_inverse = Quat {
            x: f32::from_bits(0xBF529E1C),
            y: f32::from_bits(0x3D9AF65F),
            z: f32::from_bits(0xBF01E96C),
            w: f32::from_bits(0x3E7A90D4),
        };
        let inverted = ypr.inverse();
        for (actual, expected) in [
            (inverted.x, probed_inverse.x),
            (inverted.y, probed_inverse.y),
            (inverted.z, probed_inverse.z),
            (inverted.w, probed_inverse.w),
        ] {
            assert!((actual - expected).abs() <= 1e-6, "{inverted:?}");
        }
        // Inversion round-trips to identity within the reference 1e-5 budget.
        let inverted_transform = with_translation.invert().unwrap();
        assert!(close_mat(
            with_translation.compose(inverted_transform).matrix,
            Transformation::IDENTITY.matrix,
            1e-5
        ));
        assert!(close_mat(
            inverted_transform.compose(with_translation).matrix,
            Transformation::IDENTITY.matrix,
            1e-5
        ));
        // Component inverses round-trip for translation-only, rotation-only, and scale-only.
        for (translation, rotation, scale) in [
            (euler([2.0, 5.0, -8.0]), identity, euler([1.0, 1.0, 1.0])),
            (euler([0.0, 0.0, 0.0]), ypr, euler([1.0, 1.0, 1.0])),
            (euler([0.0, 0.0, 0.0]), identity, euler([0.8, 2.0, 3.0])),
        ] {
            let original = Transformation::create(translation, rotation, scale);
            let component_inverse = Transformation::create_inverse(translation, rotation, scale);
            assert!(close_mat(
                original.compose(component_inverse).matrix,
                Transformation::IDENTITY.matrix,
                1e-5
            ));
            assert!(close_mat(
                component_inverse.compose(original).matrix,
                Transformation::IDENTITY.matrix,
                1e-5
            ));
        }
        // Zero rotations fall back to identity rotation, as probed.
        assert_eq!(
            mbits(
                Transformation::create_inverse(
                    euler([1.0, 2.0, 3.0]),
                    Quat::default(),
                    euler([2.0, 2.0, 2.0])
                )
                .matrix
            ),
            [
                0x3F000000, 0x00000000, 0x00000000, 0x00000000, 0x00000000, 0x3F000000, 0x00000000,
                0x00000000, 0x00000000, 0x00000000, 0x3F000000, 0x00000000, 0xBF000000, 0xBF800000,
                0xBFC00000, 0x3F800000,
            ]
        );
        // Singular matrices fail instead of returning a bogus transform.
        assert!(Transformation {
            matrix: Mat4::default()
        }
        .invert()
        .is_err());
    }
}
