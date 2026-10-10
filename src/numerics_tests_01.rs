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

    #[test]
    fn relative_distances_match_probed_runtime() {
        use super::relative_distance::{
            average, bone_weight, color, mat4, scalar, scalar_squared, vec2, vec2_squared, vec3,
            vec4,
        };
        use super::{BoneWeight4, ColorFloat, Mat4, Vec2, Vec3, Vec4};
        // Pinned probe words from the isolated dotnet battery.
        assert_eq!(scalar(5.0, -3.0).to_bits(), 0x3F800000);
        assert_eq!(scalar(0.0, 0.0).to_bits(), 0x00000000);
        assert_eq!(scalar(1.0, 2.0).to_bits(), 0x3EAAAAAB);
        assert_eq!(scalar(-1.0, -2.0).to_bits(), 0x3EAAAAAB);
        assert_eq!(scalar_squared(5.0, 3.0).to_bits(), 0x3D800000);
        assert_eq!(
            vec2_squared(Vec2::new(1.0, 0.0), Vec2::new(2.0, 0.0)).to_bits(),
            0x3DE38E39
        );
        assert_eq!(
            vec2(Vec2::new(1.0, 2.0), Vec2::new(4.0, 6.0)).to_bits(),
            0x3F077D85
        );
        assert_eq!(
            color(ColorFloat::BLACK, ColorFloat::WHITE).to_bits(),
            0x3F13CD3A
        );
        assert_eq!(
            bone_weight(
                BoneWeight4::new(0.25, 0.25, 0.25, 0.25, 0, 1, 2, 3),
                BoneWeight4::new(0.5, 0.5, 0.0, 0.0, 0, 1, 2, 3),
            )
            .to_bits(),
            0x3F2AAAAB
        );
        assert_eq!(
            mat4(
                Mat4::identity(),
                Mat4::create_scale(Vec3 {
                    x: 2.0,
                    y: 2.0,
                    z: 2.0
                })
            )
            .to_bits(),
            0x3D800000
        );
        assert_eq!(
            average(&[1.0, 2.0], &[1.0, 4.0], scalar),
            Ok((f32::from_bits(0x3EAAAAAB), 2))
        );
        // Opposite signs saturate at one and symmetry is bit-exact.
        assert_eq!(scalar(5.0, -3.0), 1.0);
        assert_eq!(scalar(1.5, -0.25), scalar(-0.25, 1.5));
        // Axis-aligned distances agree exactly across dimensions, like the reference test.
        let one_d = scalar(1.0, 2.0);
        assert_eq!(vec2(Vec2::new(1.0, 0.0), Vec2::new(2.0, 0.0)), one_d);
        assert_eq!(
            vec3(
                Vec3 {
                    x: 1.0,
                    y: 0.0,
                    z: 0.0
                },
                Vec3 {
                    x: 2.0,
                    y: 0.0,
                    z: 0.0
                },
            ),
            one_d
        );
        assert_eq!(
            vec4(Vec4::new(1.0, 0.0, 0.0, 0.0), Vec4::new(2.0, 0.0, 0.0, 0.0)),
            one_d
        );
        // Zero equals zero in every dimension.
        assert_eq!(scalar(0.0, 0.0), 0.0);
        assert_eq!(vec2(Vec2::ZERO, Vec2::ZERO), 0.0);
        assert_eq!(
            vec3(
                Vec3 {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0
                },
                Vec3 {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0
                },
            ),
            0.0
        );
        assert_eq!(vec4(Vec4::ZERO, Vec4::ZERO), 0.0);
        // `Vec4::as_vec3` drops the fourth lane for mesh/model callers.
        assert_eq!(
            Vec4::new(1.0, 2.0, 3.0, 4.0).as_vec3(),
            Vec3 {
                x: 1.0,
                y: 2.0,
                z: 3.0
            }
        );
        // Deterministic fixture properties: self-zero, nonnegativity, symmetry, and the
        // triangle inequality, mirroring RelativeDistanceTests without randomness.
        let fixture = [-2.0f32, -1.5, -1.0, -0.25, 0.0, 0.25, 0.5, 1.0, 1.5, 2.0];
        for value in fixture {
            assert_eq!(scalar(value, value), 0.0, "{value}");
        }
        for window in fixture.windows(2) {
            let (first, second) = (window[0], window[1]);
            let distance = scalar(first, second);
            assert!(distance >= 0.0, "{first} {second}");
            assert_eq!(distance, scalar(second, first));
        }
        for window in fixture.windows(3) {
            let (first, second, third) = (window[0], window[1], window[2]);
            assert!(
                scalar(first, second) + scalar(second, third) >= scalar(first, third),
                "{first} {second} {third}"
            );
        }
        // Array averaging reports sums with counts and rejects mismatched lengths.
        assert!(average(&[1.0], &[1.0, 2.0], scalar).is_err());
    }
