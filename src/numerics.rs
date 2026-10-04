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
}
