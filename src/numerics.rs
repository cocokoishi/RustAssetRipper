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
//! - `Range<T>` / `DiscontinuousRange<T>` port the half-open interval algebra (contains,
//!   strict ordering, intersection, adjacency-aware union, ordered merging, two-pointer
//!   intersect/subtract, bounded negate) over any partially ordered copyable endpoint.
//!   Two probed runtime quirks are replicated exactly: `subtract` drops residues once the
//!   cutter list is exhausted, and multi-member `negate` fails on its gap construction.
//!   `IComparableExtensions` needs no port: the four predicates are plain operators here.
//! - Value types mirror their sources field-for-field: `Vector2i`/`Vector3i` integer
//!   vectors (including the fall-through-to-Z member lookup and the throwing
//!   member-by-value search), `BoneWeight1`/`BoneWeight4` weights with exact-sum
//!   normalization rules, and `Color32`/`ColorFloat` conversions with truncating,
//!   saturating byte mapping and NaN-propagating clamps.
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

/// An integer 2D vector mirroring `Vector2i`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Vector2i {
    pub x: i32,
    pub y: i32,
}

impl Vector2i {
    /// `new Vector2i(x, y)`.
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

impl core::fmt::Display for Vector2i {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "[{}, {}]", self.x, self.y)
    }
}

/// An integer 3D vector mirroring `Vector3i`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Vector3i {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

/// No member holds the requested value.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct VectorMemberError {
    pub value: i32,
}

impl core::fmt::Display for VectorMemberError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "Member with value {} wasn't found", self.value)
    }
}

impl std::error::Error for VectorMemberError {}

impl Vector3i {
    /// `new Vector3i(x, y, z)`.
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// The zero vector (`Vector3i.Zero`).
    pub const ZERO: Self = Self { x: 0, y: 0, z: 0 };

    /// `GetValueByMember`: wraps the index with `% 3`, then returns X, Y, or Z.
    ///
    /// Negative remainders fall through to Z exactly like the source, because only `0`
    /// and `1` have dedicated branches.
    pub fn get_value_by_member(self, member: i32) -> i32 {
        match member % 3 {
            0 => self.x,
            1 => self.y,
            _ => self.z,
        }
    }

    /// `GetMemberByValue`: the first matching member index, else an error.
    pub fn get_member_by_value(self, value: i32) -> Result<i32, VectorMemberError> {
        if self.x == value {
            Ok(0)
        } else if self.y == value {
            Ok(1)
        } else if self.z == value {
            Ok(2)
        } else {
            Err(VectorMemberError { value })
        }
    }

    /// `ContainsValue`.
    pub fn contains_value(self, value: i32) -> bool {
        self.x == value || self.y == value || self.z == value
    }
}

impl core::fmt::Display for Vector3i {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "[{}, {}, {}]", self.x, self.y, self.z)
    }
}

/// A single bone weight with its index, mirroring `BoneWeight1`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BoneWeight1 {
    pub weight: f32,
    pub index: i32,
}

impl BoneWeight1 {
    /// `new BoneWeight1(weight, index)`.
    pub const fn new(weight: f32, index: i32) -> Self {
        Self { weight, index }
    }
}

/// Four bone weights with indices, mirroring `BoneWeight4`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BoneWeight4 {
    pub weights: [f32; 4],
    pub indices: [i32; 4],
}

impl BoneWeight4 {
    /// The lane count.
    pub const COUNT: usize = 4;

    /// `new BoneWeight4(weight0..3, index0..3)`.
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        weight0: f32,
        weight1: f32,
        weight2: f32,
        weight3: f32,
        index0: i32,
        index1: i32,
        index2: i32,
        index3: i32,
    ) -> Self {
        Self {
            weights: [weight0, weight1, weight2, weight3],
            indices: [index0, index1, index2, index3],
        }
    }

    /// Indexed weight access in source field order.
    pub fn weight(self, lane: usize) -> f32 {
        self.weights[lane]
    }

    /// Indexed bone-index access in source field order.
    pub fn index(self, lane: usize) -> i32 {
        self.indices[lane]
    }

    /// `AnyWeightsNegative`.
    pub fn any_weights_negative(self) -> bool {
        self.weights[0] < 0.0
            || self.weights[1] < 0.0
            || self.weights[2] < 0.0
            || self.weights[3] < 0.0
    }

    /// `Sum`, accumulated left to right like the source.
    pub fn sum(self) -> f32 {
        self.weights[0] + self.weights[1] + self.weights[2] + self.weights[3]
    }

    /// `Normalized`: the exact float sum equals one.
    pub fn is_normalized(self) -> bool {
        self.sum() == 1.0
    }

    /// `NormalizeWeights`: empty weights become uniform; otherwise scale by `1 / sum`.
    pub fn normalize_weights(self) -> Self {
        let sum = self.sum();
        if sum == 0.0 {
            Self {
                weights: [0.25, 0.25, 0.25, 0.25],
                indices: self.indices,
            }
        } else {
            let inverse = 1.0 / sum;
            Self {
                weights: [
                    self.weights[0] * inverse,
                    self.weights[1] * inverse,
                    self.weights[2] * inverse,
                    self.weights[3] * inverse,
                ],
                indices: self.indices,
            }
        }
    }
}

impl core::fmt::Display for BoneWeight4 {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            formatter,
            "BoneWeight4: {{ Weights = [{}, {}, {}, {}], Indices = [{}, {}, {}, {}] }}",
            self.weights[0],
            self.weights[1],
            self.weights[2],
            self.weights[3],
            self.indices[0],
            self.indices[1],
            self.indices[2],
            self.indices[3],
        )
    }
}

/// An RGBA color as four bytes, mirroring `Color32`.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Color32 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color32 {
    /// `new Color32(r, g, b, a)`.
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Opaque black.
    pub const BLACK: Self = Self::new(0, 0, 0, 255);
    /// Opaque white.
    pub const WHITE: Self = Self::new(255, 255, 255, 255);

    /// `Rgba`: the little-endian reinterpretation of the four bytes.
    pub const fn rgba(self) -> u32 {
        (self.r as u32) | ((self.g as u32) << 8) | ((self.b as u32) << 16) | ((self.a as u32) << 24)
    }

    /// `FromRgba`: the inverse reinterpretation.
    pub const fn from_rgba(rgba: u32) -> Self {
        Self {
            r: (rgba & 0xFF) as u8,
            g: ((rgba >> 8) & 0xFF) as u8,
            b: ((rgba >> 16) & 0xFF) as u8,
            a: ((rgba >> 24) & 0xFF) as u8,
        }
    }

    /// `Color.FromArgb` payload: `(A << 24) | (R << 16) | (G << 8) | B`.
    ///
    /// The `System.Drawing.Color` wrapper itself stays out of the port; this exposes the
    /// exact integer the source hands to it so drawing consumers can reuse it verbatim.
    pub const fn to_argb_u32(self) -> u32 {
        ((self.a as u32) << 24) | ((self.r as u32) << 16) | ((self.g as u32) << 8) | (self.b as u32)
    }

    /// `ConvertFloatToByte`: NaN and non-positive scales map to zero, scales at or above
    /// the byte maximum saturate, and in-between values truncate toward zero.
    pub fn float_to_byte(value: f32) -> u8 {
        if value.is_nan() {
            return 0;
        }
        let scaled = value * 255.0;
        if scaled <= 0.0 {
            0
        } else if scaled >= 255.0 {
            255
        } else {
            scaled as u8
        }
    }
}

impl From<Color32> for ColorFloat {
    fn from(color: Color32) -> Self {
        Self {
            r: f32::from(color.r) / 255.0,
            g: f32::from(color.g) / 255.0,
            b: f32::from(color.b) / 255.0,
            a: f32::from(color.a) / 255.0,
        }
    }
}

impl From<ColorFloat> for Color32 {
    fn from(color: ColorFloat) -> Self {
        Self {
            r: Self::float_to_byte(color.r),
            g: Self::float_to_byte(color.g),
            b: Self::float_to_byte(color.b),
            a: Self::float_to_byte(color.a),
        }
    }
}

impl core::fmt::Display for Color32 {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            formatter,
            "[R:{} G:{} B:{} A:{}]",
            self.r, self.g, self.b, self.a
        )
    }
}

/// An RGBA color as four floats, mirroring `ColorFloat`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ColorFloat {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl ColorFloat {
    /// `new ColorFloat(r, g, b, a)`.
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Opaque black.
    pub const BLACK: Self = Self::new(0.0, 0.0, 0.0, 1.0);
    /// Opaque white.
    pub const WHITE: Self = Self::new(1.0, 1.0, 1.0, 1.0);

    /// `Clamp` to `0..=1` with source NaN propagation.
    ///
    /// `f32::clamp` propagates NaN inputs (verified against the probe, which yields NaN
    /// for NaN lanes), so it matches the source `Math.Min(Math.Max(...))` chain here.
    pub fn clamp(self) -> Self {
        Self {
            r: self.r.clamp(0.0, 1.0),
            g: self.g.clamp(0.0, 1.0),
            b: self.b.clamp(0.0, 1.0),
            a: self.a.clamp(0.0, 1.0),
        }
    }

    /// Component-wise addition.
    #[allow(clippy::should_implement_trait)]
    pub fn add(self, other: Self) -> Self {
        Self {
            r: self.r + other.r,
            g: self.g + other.g,
            b: self.b + other.b,
            a: self.a + other.a,
        }
    }

    /// Component-wise subtraction.
    #[allow(clippy::should_implement_trait)]
    pub fn sub(self, other: Self) -> Self {
        Self {
            r: self.r - other.r,
            g: self.g - other.g,
            b: self.b - other.b,
            a: self.a - other.a,
        }
    }

    /// Scalar multiplication.
    pub fn mul_scalar(self, scalar: f32) -> Self {
        Self {
            r: self.r * scalar,
            g: self.g * scalar,
            b: self.b * scalar,
            a: self.a * scalar,
        }
    }

    /// Scalar division.
    pub fn div_scalar(self, scalar: f32) -> Self {
        Self {
            r: self.r / scalar,
            g: self.g / scalar,
            b: self.b / scalar,
            a: self.a / scalar,
        }
    }
}

impl core::ops::Add for ColorFloat {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        self.add(other)
    }
}

impl core::ops::Sub for ColorFloat {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        self.sub(other)
    }
}

impl core::ops::Mul<f32> for ColorFloat {
    type Output = Self;
    fn mul(self, scalar: f32) -> Self {
        self.mul_scalar(scalar)
    }
}

impl core::ops::Div<f32> for ColorFloat {
    type Output = Self;
    fn div(self, scalar: f32) -> Self {
        self.div_scalar(scalar)
    }
}

impl core::fmt::Display for ColorFloat {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            formatter,
            "[R:{:.2} G:{:.2} B:{:.2} A:{:.2}]",
            self.r, self.g, self.b, self.a
        )
    }
}

/// A half-open `[start, end)` range, mirroring `Range<T>`.
///
/// The bound is `PartialOrd` rather than a total order so float instantiations keep
/// working; NaN therefore follows IEEE partial-order semantics instead of the source
/// `IComparable<T>` total order. The reference suite has no NaN range cases, and all
/// ported tests avoid NaN endpoints.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Range<T> {
    /// Inclusive start; must be strictly less than `end`.
    pub start: T,
    /// Exclusive end; must be strictly greater than `start`.
    pub end: T,
}

/// A range construction or combination that the source reports by throwing.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RangeError;

impl core::fmt::Display for RangeError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "Invalid range")
    }
}

impl std::error::Error for RangeError {}

impl<T: PartialOrd + Copy> Range<T> {
    /// `new Range(start, end)`; fails when `start >= end` like the source constructor.
    pub fn new(start: T, end: T) -> Result<Self, RangeError> {
        if start < end {
            Ok(Self { start, end })
        } else {
            Err(RangeError)
        }
    }

    /// `Contains(value)`: `start <= value < end`.
    pub fn contains_point(self, value: T) -> bool {
        self.start <= value && value < self.end
    }

    /// `Contains(range)`: full containment including equal endpoints.
    pub fn contains_range(self, other: Self) -> bool {
        self.start <= other.start && other.end <= self.end
    }

    /// `IsStrictlyLess`: `end <= other.start`.
    pub fn is_strictly_less(self, other: Self) -> bool {
        self.end <= other.start
    }

    /// `IsStrictlyGreater`: `start >= other.end`.
    pub fn is_strictly_greater(self, other: Self) -> bool {
        self.start >= other.end
    }

    /// `Intersects(other)`: either start point lies inside the other range.
    pub fn intersects(self, other: Self) -> bool {
        self.contains_point(other.start) || other.contains_point(self.start)
    }

    /// `Intersects(other, out intersection)`.
    pub fn intersection(self, other: Self) -> Option<Self> {
        if self.intersects(other) {
            Some(Self {
                start: max_value(self.start, other.start),
                end: min_value(self.end, other.end),
            })
        } else {
            None
        }
    }

    /// `CanUnion`: intersecting or exactly adjacent ranges.
    pub fn can_union(self, other: Self) -> bool {
        self.intersects(other) || self.start == other.end || self.end == other.start
    }

    /// `CanUnion(other, out union)`.
    pub fn union(self, other: Self) -> Option<Self> {
        if self.can_union(other) {
            Some(Self {
                start: min_value(self.start, other.start),
                end: max_value(self.end, other.end),
            })
        } else {
            None
        }
    }

    /// `MakeUnion`: fails for disjoint, non-adjacent ranges like the source.
    pub fn make_union(self, other: Self) -> Result<Self, RangeError> {
        self.union(other).ok_or(RangeError)
    }

    /// `MakeIntersection`: fails for non-intersecting ranges like the source.
    pub fn make_intersection(self, other: Self) -> Result<Self, RangeError> {
        self.intersection(other).ok_or(RangeError)
    }
}

fn min_value<T: PartialOrd + Copy>(left: T, right: T) -> T {
    if left < right {
        left
    } else {
        right
    }
}

fn max_value<T: PartialOrd + Copy>(left: T, right: T) -> T {
    if left > right {
        left
    } else {
        right
    }
}

impl<T: core::fmt::Display> core::fmt::Display for Range<T> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(formatter, "{} : {}", self.start, self.end)
    }
}

/// A sorted, normalized, possibly empty list of disjoint ranges.
///
/// Mirrors `DiscontinuousRange<T>`: every constructor merges overlapping or adjacent
/// inputs through the same first-union/last-union scan as the source, so iteration order
/// and counts match. Set operations replicate the source two-pointer walks exactly,
/// including two confirmed runtime quirks (see `subtract` and `negate`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DiscontinuousRange<T> {
    ranges: Vec<Range<T>>,
}

impl<T> DiscontinuousRange<T> {
    /// The empty range.
    pub const fn empty() -> Self {
        Self { ranges: Vec::new() }
    }

    /// The number of disjoint ranges.
    pub fn count(&self) -> usize {
        self.ranges.len()
    }

    /// Range by position in sorted order.
    pub fn get(&self, index: usize) -> Option<Range<T>>
    where
        T: Copy,
    {
        self.ranges.get(index).copied()
    }

    /// Sorted disjoint ranges in order.
    pub fn as_slice(&self) -> &[Range<T>] {
        &self.ranges
    }

    /// No ranges at all.
    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    /// Exactly one range (the empty range counts as not continuous, like the source).
    pub fn is_continuous(&self) -> bool {
        self.ranges.len() == 1
    }
}

impl<T: PartialOrd + Copy> DiscontinuousRange<T> {
    /// A single range with no merging needed.
    pub fn from_range(range: Range<T>) -> Self {
        Self {
            ranges: vec![range],
        }
    }

    /// Merges an unordered collection exactly like the source params constructor.
    pub fn from_vec(ranges: Vec<Range<T>>) -> Self {
        let mut result = Self::empty();
        for range in ranges {
            result.add(range);
        }
        result
    }

    /// The union constructor `new DiscontinuousRange(range1, range2)`.
    pub fn from_union(first: Self, second: Self) -> Self {
        if first.is_empty() {
            return second;
        }
        if second.is_empty() {
            return first;
        }
        let mut result = first;
        for range in second.ranges {
            result.add(range);
        }
        result
    }

    /// Equals a single range: exactly one member equal to it.
    pub fn equals_single(&self, other: Range<T>) -> bool {
        self.ranges.len() == 1 && self.ranges[0] == other
    }

    /// `Contains(point)`: some member holds the point.
    pub fn contains_point(&self, value: T) -> bool {
        self.ranges.iter().any(|range| range.contains_point(value))
    }

    /// `Contains(range)`: some member fully holds the range.
    pub fn contains_range(&self, range: Range<T>) -> bool {
        self.ranges
            .iter()
            .any(|member| member.contains_range(range))
    }

    /// `Contains(other)`: every member of `other` is held by some member here.
    pub fn contains_all(&self, other: &Self) -> bool {
        other.ranges.iter().all(|range| self.contains_range(*range))
    }

    /// `Intersects(range)`.
    pub fn intersects_range(&self, range: Range<T>) -> bool {
        self.ranges.iter().any(|member| member.intersects(range))
    }

    /// `Intersects(other)` via the source two-pointer walk.
    pub fn intersects(&self, other: &Self) -> bool {
        let mut i = 0;
        let mut j = 0;
        while i < self.count() && j < other.count() {
            let left = self.ranges[i];
            let right = other.ranges[j];
            if left.intersects(right) {
                return true;
            } else if left.is_strictly_less(right) {
                i += 1;
            } else {
                j += 1;
            }
        }
        false
    }

    /// `Union`: merges both member lists.
    pub fn union(&self, other: &Self) -> Self {
        Self::from_union(self.clone(), other.clone())
    }

    /// `Intersect`: pairwise intersections in order, directly stored without
    /// re-normalization like the source (outputs are already disjoint and sorted).
    pub fn intersect(&self, other: &Self) -> Self {
        let mut ranges = Vec::new();
        let mut i = 0;
        let mut j = 0;
        while i < self.count() && j < other.count() {
            let left = self.ranges[i];
            let right = other.ranges[j];
            if let Some(intersection) = left.intersection(right) {
                ranges.push(intersection);
                if left.end <= right.end {
                    i += 1;
                } else {
                    j += 1;
                }
            } else if left.is_strictly_less(right) {
                i += 1;
            } else {
                j += 1;
            }
        }
        Self { ranges }
    }

    /// `Subtract`: members of `self` minus members of `other`, replicating the source
    /// walk exactly, including its confirmed tail-drop quirk: once `other` is exhausted
    /// through the strictly-less/greater branches, remaining `self` members are dropped
    /// rather than emitted (probed: `[10..20] minus [0..5]` yields empty, and a trailing
    /// `[20..30]` member vanishes when `other` runs out first).
    ///
    /// Infallible by construction: every emitted pair is strictly ordered because all
    /// inputs are normalized on construction, so the inner `expect` documents an
    /// invariant rather than a fallible path.
    pub fn subtract(&self, other: &Self) -> Self {
        let mut ranges = Vec::new();
        let mut i = 0;
        let mut j = 0;
        let mut ended_inside: Option<T> = None;
        while i < self.count() && j < other.count() {
            let current = self.ranges[i];
            let cutting = other.ranges[j];
            if current.is_strictly_less(cutting) {
                if let Some(pending) = ended_inside.take() {
                    ranges.push(
                        Range::new(pending, current.end).expect(
                            "subtract inputs are normalized, so pending starts stay ordered",
                        ),
                    );
                }
                i += 1;
            } else if current.is_strictly_greater(cutting) {
                j += 1;
            } else {
                if current.start < cutting.start {
                    if let Some(pending) = ended_inside.take() {
                        ranges.push(Range::new(pending, cutting.start).expect(
                            "subtract inputs are normalized, so pending starts stay ordered",
                        ));
                    } else {
                        ranges.push(
                            Range::new(current.start, cutting.start)
                                .expect("subtract overlap guarantees a strictly ordered prefix"),
                        );
                    }
                }
                if current.end <= cutting.end {
                    i += 1;
                } else if j < other.count() - 1 {
                    ended_inside = Some(cutting.end);
                    j += 1;
                } else {
                    ranges.push(
                        Range::new(cutting.end, current.end)
                            .expect("subtract overlap guarantees a strictly ordered suffix"),
                    );
                    j += 1;
                }
            }
        }
        Self { ranges }
    }

    /// `Negate(minimum, maximum)`: the complement within the bounding pair.
    ///
    /// Replicates a confirmed source quirk: gap construction reads the current member
    /// start instead of the next member start, so any multi-member input fails exactly
    /// like the probed runtime (`ArgumentException`, here `Err(RangeError)`). Single
    /// members and the empty range behave normally.
    pub fn negate(&self, minimum: T, maximum: T) -> Result<Self, RangeError> {
        if self.is_empty() {
            return Ok(Self::from_range(Range::new(minimum, maximum)?));
        }
        let mut ranges = Vec::with_capacity(self.count() + 1);
        let first = self.ranges[0].start;
        if first != minimum {
            ranges.push(Range::new(minimum, first)?);
        }
        for i in 0..self.count() - 1 {
            ranges.push(Range::new(self.ranges[i].end, self.ranges[i].start)?);
        }
        let last = self.ranges[self.count() - 1].end;
        if last != maximum {
            ranges.push(Range::new(last, maximum)?);
        }
        Ok(Self { ranges })
    }

    /// The explicit `Range<T>` conversion: succeeds only for continuous ranges.
    pub fn as_continuous(&self) -> Result<Range<T>, RangeError> {
        if self.is_continuous() {
            Ok(self.ranges[0])
        } else {
            Err(RangeError)
        }
    }

    /// The source `Add` scan: merges unionable runs, inserts before the first
    /// strictly greater member, or appends.
    fn add(&mut self, range: Range<T>) {
        let count = self.count();
        let mut first_union = count;
        let mut last_union = count;
        for (index, member) in self.ranges.iter().enumerate() {
            if member.can_union(range) {
                if first_union == count {
                    first_union = index;
                }
                last_union = index;
            } else if first_union < count {
                break;
            } else if member.is_strictly_greater(range) {
                self.ranges.insert(index, range);
                return;
            }
        }
        if first_union == count {
            self.ranges.push(range);
        } else {
            let merged = self.ranges[first_union].union(range).unwrap_or(range);
            self.ranges[first_union] = merged;
            if first_union < last_union {
                self.ranges.drain(first_union + 1..=last_union);
            }
        }
    }
}

impl<T: core::fmt::Display> core::fmt::Display for DiscontinuousRange<T> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.is_empty() {
            return write!(formatter, "Empty");
        }
        let mut first = true;
        for range in &self.ranges {
            if !first {
                write!(formatter, ", ")?;
            }
            first = false;
            write!(formatter, "{range}")?;
        }
        Ok(())
    }
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

    // Float literals below mirror the reference `Color32Tests` decimals exactly; they round
    // to the same `f32` values on both runtimes (verified by the passing round-trip assert).
    #[test]
    #[allow(clippy::excessive_precision)]
    fn value_types_match_probed_runtime() {
        use super::{BoneWeight1, BoneWeight4, Color32, ColorFloat, Vector2i, Vector3i};
        // Packed-word layout matches shifts and the unsafe reinterpretation alike.
        let color = Color32::new(0x0A, 0x14, 0x1E, 0x28);
        assert_eq!(color.rgba(), 0x281E140A);
        assert_eq!(Color32::from_rgba(0x281E140A), color);
        assert_eq!(
            Color32::new(33, 57, 199, 255).rgba(),
            33 | (57 << 8) | (199 << 16) | (255 << 24)
        );
        assert_eq!(color.to_string(), "[R:10 G:20 B:30 A:40]");
        assert_eq!(Color32::BLACK, Color32::new(0, 0, 0, 255));
        assert_eq!(Color32::WHITE, Color32::new(255, 255, 255, 255));
        // The drawing-color integer packs alpha first, exactly as probed.
        assert_eq!(color.to_argb_u32(), 0x280A141E);
        // Byte conversion truncates, saturates, and zeroes NaN, as probed.
        for (input, expected) in [
            (f32::NAN, 0),
            (-0.5, 0),
            (0.0, 0),
            (0.001, 0),
            (0.5, 127),
            (0.999, 254),
            (1.0, 255),
            (1.5, 255),
            (f32::INFINITY, 255),
            (f32::NEG_INFINITY, 0),
        ] {
            assert_eq!(Color32::float_to_byte(input), expected, "{input}");
        }
        // Color32/float round-trips reproduce the probed float bits.
        let float: ColorFloat = color.into();
        assert_eq!(
            [
                float.r.to_bits(),
                float.g.to_bits(),
                float.b.to_bits(),
                float.a.to_bits()
            ],
            [0x3D20A0A1, 0x3DA0A0A1, 0x3DF0F0F1, 0x3E20A0A1]
        );
        assert_eq!(float.to_string(), "[R:0.04 G:0.08 B:0.12 A:0.16]");
        let back: Color32 =
            ColorFloat::new(0.0392156877, 0.0784313753, 0.117647059, 0.156862751).into();
        assert_eq!(back, Color32::new(10, 20, 30, 40));
        // Arithmetic is component-wise single precision, pinned by probed bits.
        let sum = ColorFloat::new(0.1, 0.2, 0.3, 0.4) + ColorFloat::new(0.5, 0.6, 0.7, 0.8);
        assert_eq!(
            [
                sum.r.to_bits(),
                sum.g.to_bits(),
                sum.b.to_bits(),
                sum.a.to_bits()
            ],
            [0x3F19999A, 0x3F4CCCCD, 0x3F800000, 0x3F99999A]
        );
        let difference = ColorFloat::new(5.0, 6.0, 7.0, 8.0) - ColorFloat::new(1.0, 2.0, 3.0, 4.0);
        assert_eq!(difference, ColorFloat::new(4.0, 4.0, 4.0, 4.0));
        let scaled = ColorFloat::new(0.1, 0.2, 0.3, 0.4) * 2.0;
        assert_eq!(
            [
                scaled.r.to_bits(),
                scaled.g.to_bits(),
                scaled.b.to_bits(),
                scaled.a.to_bits()
            ],
            [0x3E4CCCCD, 0x3ECCCCCD, 0x3F19999A, 0x3F4CCCCD]
        );
        let halved = ColorFloat::new(0.1, 0.2, 0.3, 0.4) / 2.0;
        assert_eq!(
            [
                halved.r.to_bits(),
                halved.g.to_bits(),
                halved.b.to_bits(),
                halved.a.to_bits()
            ],
            [0x3D4CCCCD, 0x3DCCCCCD, 0x3E19999A, 0x3E4CCCCD]
        );
        // Clamp propagates NaN and pins infinities/edges, as probed.
        // NaN payloads differ across runtimes (.NET uses 0xFFC00000, Rust 0x7FC00000),
        // so NaN is checked by predicate while all other lanes pin exact bits.
        let nan_clamped = ColorFloat::new(f32::NAN, f32::NAN, f32::NAN, f32::NAN).clamp();
        assert!(nan_clamped.r.is_nan());
        assert!(nan_clamped.g.is_nan());
        assert!(nan_clamped.b.is_nan());
        assert!(nan_clamped.a.is_nan());
        for (input, expected_bits) in [
            (-0.1, 0x00000000),
            (0.0, 0x00000000),
            (0.3, 0x3E99999A),
            (1.2, 0x3F800000),
            (f32::INFINITY, 0x3F800000),
            (f32::NEG_INFINITY, 0x00000000),
        ] {
            let clamped = ColorFloat::new(input, input, input, input).clamp();
            assert_eq!(clamped.r.to_bits(), expected_bits, "{input}");
            assert_eq!(clamped.g.to_bits(), expected_bits);
            assert_eq!(clamped.b.to_bits(), expected_bits);
            assert_eq!(clamped.a.to_bits(), expected_bits);
        }
        assert_eq!(
            ColorFloat::new(-0.1, 1.2, 0.3, -1.3).clamp(),
            ColorFloat::new(0.0, 1.0, 0.3, 0.0)
        );
        assert_eq!(ColorFloat::BLACK, ColorFloat::new(0.0, 0.0, 0.0, 1.0));
        assert_eq!(ColorFloat::WHITE, ColorFloat::new(1.0, 1.0, 1.0, 1.0));
        assert_eq!(
            ColorFloat::BLACK.to_string(),
            "[R:0.00 G:0.00 B:0.00 A:1.00]"
        );
        // Bone weights follow exact-sum rules and uniform fallback, as probed.
        let default_normalized = BoneWeight4::default().normalize_weights();
        assert_eq!(default_normalized.weights, [0.25, 0.25, 0.25, 0.25]);
        assert!(default_normalized.is_normalized());
        let full = BoneWeight4::new(1.0, 1.0, 1.0, 1.0, 4, 5, 6, 7).normalize_weights();
        assert_eq!(full.weights, [0.25, 0.25, 0.25, 0.25]);
        assert_eq!(full.indices, [4, 5, 6, 7]);
        let unit = BoneWeight4::new(0.1, 0.2, 0.3, 0.4, 0, 1, 2, 3);
        assert!(unit.is_normalized());
        assert!(!unit.any_weights_negative());
        assert_eq!(
            unit.to_string(),
            "BoneWeight4: { Weights = [0.1, 0.2, 0.3, 0.4], Indices = [0, 1, 2, 3] }"
        );
        let negative = BoneWeight4::new(-0.1, 0.2, 0.3, 0.4, 0, 1, 2, 3);
        assert!(negative.any_weights_negative());
        assert!(!negative.is_normalized());
        assert_eq!(BoneWeight4::COUNT, 4);
        assert_eq!(
            BoneWeight1::new(0.5, 7),
            BoneWeight1 {
                weight: 0.5,
                index: 7
            }
        );
        // Integer vectors wrap, search, and display exactly like the source.
        let vector = Vector3i::new(10, 20, 30);
        assert_eq!(vector.get_value_by_member(0), 10);
        assert_eq!(vector.get_value_by_member(1), 20);
        assert_eq!(vector.get_value_by_member(2), 30);
        assert_eq!(vector.get_value_by_member(3), 10);
        assert_eq!(vector.get_value_by_member(-1), 30);
        assert_eq!(vector.get_value_by_member(4), 20);
        assert_eq!(vector.get_member_by_value(10), Ok(0));
        assert_eq!(vector.get_member_by_value(20), Ok(1));
        assert_eq!(vector.get_member_by_value(30), Ok(2));
        assert!(vector.get_member_by_value(99).is_err());
        assert!(vector.contains_value(20));
        assert!(!vector.contains_value(99));
        assert_eq!(vector.to_string(), "[10, 20, 30]");
        assert_eq!(Vector3i::ZERO, Vector3i::new(0, 0, 0));
        assert_eq!(Vector2i::new(3, -4).to_string(), "[3, -4]");
    }

    #[test]
    fn ranges_follow_reference_cases() {
        use super::{DiscontinuousRange, Range};
        fn range(start: i32, end: i32) -> Range<i32> {
            Range::new(start, end).unwrap()
        }
        let zero_to_ten = range(0, 10);
        // Intersection, unions, containment, and strict ordering mirror RangeTests.
        assert_eq!(
            zero_to_ten.make_intersection(range(1, 11)),
            Ok(range(1, 10))
        );
        assert_eq!(zero_to_ten.make_union(range(1, 11)), Ok(range(0, 11)));
        assert_eq!(zero_to_ten.make_union(range(10, 20)), Ok(range(0, 20)));
        assert!(zero_to_ten.contains_range(zero_to_ten));
        assert!(zero_to_ten.contains_point(0));
        assert!(zero_to_ten.contains_point(5));
        assert!(!zero_to_ten.contains_point(10));
        assert!(!zero_to_ten.contains_point(-10));
        assert!(!zero_to_ten.contains_point(100));
        assert!(Range::new(4, 3).is_err());
        assert!(Range::new(4, 4).is_err());
        assert!(zero_to_ten.make_union(range(20, 30)).is_err());
        assert!(zero_to_ten.make_intersection(range(20, 30)).is_err());
        assert!(zero_to_ten.is_strictly_less(range(10, 20)));
        assert!(range(10, 20).is_strictly_greater(zero_to_ten));
        assert!(!range(10, 20).is_strictly_less(zero_to_ten));
        assert!(!zero_to_ten.is_strictly_greater(range(10, 20)));
        assert!(!range(10, 20).is_strictly_less(range(0, 11)));
        assert!(!range(0, 11).is_strictly_less(range(10, 20)));
        assert!(!range(10, 20).is_strictly_greater(range(0, 11)));
        assert!(!range(0, 11).is_strictly_greater(range(10, 20)));
        assert_eq!(zero_to_ten.to_string(), "0 : 10");
        // Adjacent ranges intersect nothing but still union.
        assert!(!zero_to_ten.intersects(range(10, 20)));
        assert!(zero_to_ten.can_union(range(10, 20)));
        assert!(DiscontinuousRange::<i32>::empty().is_empty());
    }

    #[test]
    fn discontinuous_ranges_follow_reference_cases() {
        use super::{DiscontinuousRange, Range};
        fn range(start: i32, end: i32) -> Range<i32> {
            Range::new(start, end).unwrap()
        }
        fn list(ranges: Vec<Range<i32>>) -> DiscontinuousRange<i32> {
            DiscontinuousRange::from_vec(ranges)
        }
        // Fixture numbers mirror DiscontinuousRangeTests exactly, including its
        // `ZeroToTen = (0, 110)` naming quirk.
        let min_to_zero = range(i32::MIN, 0);
        let zero_to_three = range(0, 3);
        let three_to_four = range(3, 4);
        let four_to_five = range(4, 5);
        let five_to_seven = range(5, 7);
        let seven_to_nine = range(7, 9);
        let nine_to_ten = range(9, 10);
        let zero_to_ten = range(0, 110);
        let ten_to_twenty = range(10, 20);
        let zero_to_twenty = range(0, 20);
        let zero_to_max = range(0, i32::MAX);
        let twenty_to_max = range(20, i32::MAX);
        let min_to_max = range(i32::MIN, i32::MAX);
        assert_eq!(
            list(vec![zero_to_three, seven_to_nine, four_to_five]).count(),
            3
        );
        assert_eq!(
            list(vec![
                zero_to_three,
                seven_to_nine,
                range(0, 11),
                four_to_five
            ])
            .count(),
            1
        );
        assert_eq!(
            list(vec![zero_to_three, seven_to_nine, four_to_five]),
            list(vec![seven_to_nine, four_to_five, zero_to_three])
        );
        assert_eq!(
            list(vec![ten_to_twenty, twenty_to_max, zero_to_ten]),
            list(vec![zero_to_max])
        );
        assert_eq!(
            list(vec![twenty_to_max, zero_to_ten]).union(&list(vec![zero_to_twenty])),
            list(vec![zero_to_max])
        );
        assert_eq!(
            list(vec![min_to_zero]).negate(i32::MIN, i32::MAX).unwrap(),
            list(vec![zero_to_max])
        );
        assert_eq!(
            list(vec![min_to_max]).subtract(&DiscontinuousRange::from_range(zero_to_twenty)),
            list(vec![min_to_zero, twenty_to_max])
        );
        assert_eq!(
            list(vec![min_to_max]).subtract(&DiscontinuousRange::from_range(min_to_zero)),
            list(vec![zero_to_max])
        );
        assert_eq!(
            list(vec![min_to_max]).subtract(&DiscontinuousRange::from_range(zero_to_max)),
            list(vec![min_to_zero])
        );
        assert_eq!(
            list(vec![ten_to_twenty]).subtract(&DiscontinuousRange::from_range(zero_to_max)),
            DiscontinuousRange::empty()
        );
        assert_eq!(
            list(vec![zero_to_max]).subtract(&list(vec![min_to_zero, zero_to_twenty])),
            list(vec![twenty_to_max])
        );
        assert_eq!(
            list(vec![zero_to_max]).subtract(&list(vec![min_to_zero, twenty_to_max])),
            list(vec![zero_to_twenty])
        );
        assert_eq!(
            list(vec![zero_to_max]).subtract(&list(vec![
                min_to_zero,
                zero_to_three,
                three_to_four,
                five_to_seven,
                seven_to_nine,
                ten_to_twenty,
            ])),
            list(vec![four_to_five, nine_to_ten, twenty_to_max])
        );
        assert_eq!(
            list(vec![zero_to_max]).intersect(&list(vec![min_to_zero, zero_to_twenty])),
            list(vec![zero_to_twenty])
        );
        assert!(list(vec![zero_to_max]).intersects_range(zero_to_twenty));
        assert!(list(vec![zero_to_max]).contains_all(&list(vec![zero_to_twenty])));
        assert!(list(vec![zero_to_three]).equals_single(zero_to_three));
        assert!(list(vec![zero_to_three]).as_continuous().is_ok());
        assert_eq!(
            list(vec![zero_to_three, seven_to_nine, four_to_five]).to_string(),
            "0 : 3, 4 : 5, 7 : 9"
        );
        assert_eq!(DiscontinuousRange::<i32>::empty().to_string(), "Empty");
    }

    #[test]
    fn discontinuous_range_quirks_match_probed_runtime() {
        use super::{DiscontinuousRange, Range};
        fn range(start: i32, end: i32) -> Range<i32> {
            Range::new(start, end).unwrap()
        }
        // Multi-member negate fails exactly like the probed runtime, while empty and
        // single-member negations behave normally.
        assert!(DiscontinuousRange::from_vec(vec![range(0, 3), range(7, 9)])
            .negate(i32::MIN, i32::MAX)
            .is_err());
        assert_eq!(
            DiscontinuousRange::<i32>::empty()
                .negate(i32::MIN, i32::MAX)
                .unwrap(),
            DiscontinuousRange::from_range(range(i32::MIN, i32::MAX))
        );
        // Subtracting a strictly lesser or strictly greater cutter drops everything,
        // and leftovers vanish once the cutter list runs out first.
        assert_eq!(
            DiscontinuousRange::from_range(range(10, 20))
                .subtract(&DiscontinuousRange::from_range(range(0, 5))),
            DiscontinuousRange::empty()
        );
        assert_eq!(
            DiscontinuousRange::from_range(range(0, 20))
                .subtract(&DiscontinuousRange::from_range(range(30, 40))),
            DiscontinuousRange::empty()
        );
        assert_eq!(
            DiscontinuousRange::from_vec(vec![range(0, 10), range(20, 30)]).subtract(
                &DiscontinuousRange::from_vec(vec![range(0, 5), range(6, 7)])
            ),
            DiscontinuousRange::from_vec(vec![range(5, 6), range(7, 10)])
        );
        // Explicit conversion succeeds only for continuous ranges.
        assert!(DiscontinuousRange::from_vec(vec![range(0, 3), range(7, 9)])
            .as_continuous()
            .is_err());
        // Two-pointer intersect skips non-overlapping heads on either side.
        assert!(!DiscontinuousRange::from_range(range(0, 3))
            .intersects(&DiscontinuousRange::from_range(range(7, 9))));
    }
}
