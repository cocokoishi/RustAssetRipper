    use super::*;
    use crate::numerics::BoneWeight4;
    use crate::resources::sub_mesh_data::SubMeshData;

    /// `GlbCoordinateConversion.ToGltfQuaternionConvert` composes two signs.
    ///
    /// The source multiplies the axis of rotation by `axisFlipScale * CoordinateSpaceConversionScale`,
    /// that is `-1 * (-1, 1, 1)`, which is `(1, -1, -1)`. Applying only the coordinate scale - the
    /// `(-1, 1, 1)` a plain `ToGltfVector3Convert` gives - would be wrong on all three axes, and
    /// that was exactly the state of this helper before the scene-level builder became its first
    /// caller. The `w` component is unchanged.
    ///
    /// The expected values below are written from the reference formula, not from the
    /// implementation, so the test does not simply restate the code it checks.
    #[test]
    fn quaternion_conversion_composes_the_axis_flip_with_the_coordinate_scale() {
        // axisFlipScale * CoordinateSpaceConversionScale = -1 * (-1, 1, 1) = (1, -1, -1).
        const AXIS_FLIP_TIMES_SCALE: [f32; 3] = [1.0, -1.0, -1.0];
        fn expected(x: f32, y: f32, z: f32, w: f32) -> [f32; 4] {
            [
                x * AXIS_FLIP_TIMES_SCALE[0],
                y * AXIS_FLIP_TIMES_SCALE[1],
                z * AXIS_FLIP_TIMES_SCALE[2],
                w,
            ]
        }

        let identity = to_gltf_quaternion(Quat {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        });
        assert_eq!(
            [identity.x, identity.y, identity.z, identity.w],
            expected(0.0, 0.0, 0.0, 1.0)
        );

        // A rotation about +X keeps its axis (both signs cancel), and `w` is untouched.
        let about_x = to_gltf_quaternion(Quat {
            x: 0.5,
            y: 0.0,
            z: 0.0,
            w: 0.5,
        });
        assert_eq!(
            [about_x.x, about_x.y, about_x.z, about_x.w],
            expected(0.5, 0.0, 0.0, 0.5)
        );

        // A rotation about +Y or +Z flips, which a coordinate-scale-only implementation misses.
        for (x, y, z) in [(0.0, 0.25, 0.0), (0.0, 0.0, 0.25), (0.1, 0.2, 0.3)] {
            let converted = to_gltf_quaternion(Quat { x, y, z, w: 0.9 });
            assert_eq!(
                [converted.x, converted.y, converted.z, converted.w],
                expected(x, y, z, 0.9),
                "({x}, {y}, {z})"
            );
        }
    }

    /// The quaternion and vector conversions over the reference's own recorded outputs.
    ///
    /// `glb_coordinate_fixture.txt` holds the rows the isolated baseline build produced through
    /// the probe's `quat` mode, which invokes the shipped `GlbCoordinateConversion` methods by
    /// reflection. This pins the port against the reference runtime rather than against a reading
    /// of its source, which matters here because the two conversions differ by a sign that the
    /// source expresses through a private constant.
    #[test]
    fn coordinate_conversions_match_the_reference_runtime_rows() {
        let fixture = include_str!("glb_coordinate_fixture.txt");
        let mut coordinate_scale: Option<[f32; 3]> = None;
        let mut quaternion_rows = 0usize;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<&str> = line.split('|').collect();
            match fields[0] {
                "SCALE" => {
                    let parsed = parse_f32_list(fields[1], 3);
                    assert_eq!(parsed, [-1.0, 1.0, 1.0]);
                    coordinate_scale = Some([parsed[0], parsed[1], parsed[2]]);
                }
                "TANGENT_SCALE" => {
                    assert_eq!(parse_f32_list(fields[1], 4), [-1.0, 1.0, 1.0, -1.0]);
                }
                "QCOORD" => {
                    let scale = coordinate_scale.expect("the coordinate scale row comes first");
                    let input = parse_f32_list(fields[1], 4);
                    let expected_quaternion = parse_f32_list(fields[2], 4);
                    let expected_vector = parse_f32_list(fields[3], 3);

                    let converted = to_gltf_quaternion(Quat {
                        x: input[0],
                        y: input[1],
                        z: input[2],
                        w: input[3],
                    });
                    // The fixture records IEEE negative zero, which compares equal to 0.0.
                    assert_eq!(
                        [converted.x, converted.y, converted.z, converted.w].as_slice(),
                        expected_quaternion.as_slice(),
                        "quaternion row {}",
                        fields[1]
                    );

                    let vector = to_gltf_vector3(vec3(input[0], input[1], input[2]));
                    assert_eq!(
                        [vector.x, vector.y, vector.z].as_slice(),
                        expected_vector.as_slice(),
                        "vector row {}",
                        fields[1]
                    );
                    // The two conversions must stay distinct: the quaternion carries an extra
                    // axis flip, so their signs agree only on x.
                    assert_eq!(expected_quaternion[0], -expected_vector[0]);
                    assert_eq!(expected_quaternion[1], -expected_vector[1]);
                    assert_eq!(expected_quaternion[2], -expected_vector[2]);
                    assert_eq!(scale[0], -1.0);
                    quaternion_rows += 1;
                }
                other => panic!("unknown fixture row {other}"),
            }
        }
        assert_eq!(quaternion_rows, 5, "every recorded row is checked");
    }

    /// Parse a comma-separated float list of an expected length.
    fn parse_f32_list(text: &str, expected: usize) -> Vec<f32> {
        let values: Vec<f32> = text.split(',').map(|v| v.parse().unwrap()).collect();
        assert_eq!(values.len(), expected, "{text}");
        values
    }

    /// A scene plan's geometry attachment emits real primitives on the meshed nodes.
    ///
    /// Covers the seam between the two halves of the scene builder: the plan decides which
    /// sub-meshes a node wants, and this fills them with geometry. Also pins the two behaviours a
    /// naive implementation gets wrong - a node with no decoded mesh keeps no geometry rather
    /// than failing the build, and a sub-mesh index the mesh does not have is reported and
    /// skipped rather than panicking.
    #[test]
    fn plan_geometry_attaches_to_meshed_nodes_only() {
        use crate::resources::glb_level::{
            build_level_plan, build_scene_document, GlbLevelMesh, GlbLevelTransform,
        };

        fn node(name: &str, mesh: Option<GlbLevelMesh>) -> GlbLevelTransform {
            GlbLevelTransform {
                game_object_name: name.to_owned(),
                local_position: [0.0, 0.0, 0.0],
                local_rotation: [0.0, 0.0, 0.0, 1.0],
                local_scale: [1.0, 1.0, 1.0],
                children: Vec::new(),
                mesh,
            }
        }
        // A dynamic renderer takes every sub-mesh in order.
        let dynamic = GlbLevelMesh {
            sub_mesh_count: 1,
            is_16_bit_indices: true,
            mesh_type: None,
            has_renderer: true,
            has_static_batch_info: false,
            static_batch_sub_mesh_count: 0,
            has_subset_indices: true,
            subset_indices_count: 0,
            subset_indices: Some(Vec::new()),
            static_batch_info: None,
            source: None,
        };
        // A static renderer that selects sub-mesh 0 explicitly.
        let static_mesh = GlbLevelMesh {
            has_subset_indices: true,
            subset_indices_count: 1,
            subset_indices: Some(vec![0]),
            ..dynamic.clone()
        };

        let plan = build_level_plan(
            &[
                node("WithMesh", Some(dynamic)),
                node("WithoutData", Some(static_mesh)),
                node("NoMesh", None),
            ],
            true,
        )
        .expect("the plan builds");
        let mut document = build_scene_document(&plan);

        // Only the first node has decoded geometry; the second has a mesh selection but no data.
        let meshes = vec![Some(triangle_mesh()), None, None];
        let warnings = attach_plan_geometry(&mut document, &plan, &meshes, GlbBuildLimits::default())
            .expect("geometry attaches");
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].contains("WithoutData"),
            "the node without data is named: {warnings:?}"
        );

        let bytes = document.into_glb();
        let json = {
            let length = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
            String::from_utf8(bytes[20..20 + length].to_vec()).expect("the JSON chunk is UTF-8")
        };

        // Exactly one mesh was emitted, and the node that owns it references mesh 0.
        assert!(json.contains("\"meshes\":["), "{json}");
        assert_eq!(json.matches("\"primitives\":[").count(), 1, "{json}");
        assert!(json.contains("\"mesh\":0"), "{json}");
        // The other two nodes carry no mesh reference.
        assert!(!json.contains("WithoutData\",\"mesh"), "{json}");
        // The primitive is a triangle with positions and an index accessor.
        assert!(json.contains("\"POSITION\":"), "{json}");
        assert!(json.contains("\"indices\":"), "{json}");
        assert!(json.contains("\"mode\":4"), "{json}");
        // A binary chunk followed the JSON, so the whole buffer is not text.
        assert!(bytes.len() > 20 + json.len(), "a binary chunk is present");
    }

    /// A sub-mesh index the mesh does not have is reported, not panicked on.
    #[test]
    fn plan_geometry_reports_an_out_of_range_sub_mesh() {
        use crate::resources::glb_level::{
            build_level_plan, build_scene_document, GlbLevelMesh, GlbLevelTransform,
        };

        let plan = build_level_plan(
            &[GlbLevelTransform {
                game_object_name: "Bad".to_owned(),
                local_position: [0.0, 0.0, 0.0],
                local_rotation: [0.0, 0.0, 0.0, 1.0],
                local_scale: [1.0, 1.0, 1.0],
                children: Vec::new(),
                mesh: Some(GlbLevelMesh {
                    sub_mesh_count: 1,
                    is_16_bit_indices: true,
                    mesh_type: None,
                    has_renderer: true,
                    has_static_batch_info: false,
                    static_batch_sub_mesh_count: 0,
                    has_subset_indices: true,
                    subset_indices_count: 1,
                    subset_indices: Some(vec![7]),
                    static_batch_info: None,
                    source: None,
                }),
            }],
            true,
        )
        .expect("the plan builds");
        let mut document = build_scene_document(&plan);
        let warnings =
            attach_plan_geometry(&mut document, &plan, &[Some(triangle_mesh())], GlbBuildLimits::default())
                .expect("an out-of-range selection is not fatal");
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("sub-mesh 7"), "{warnings:?}");
        // Nothing was emitted, so the node keeps no mesh reference.
        let json = {
            let bytes = document.into_glb();
            let length = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]) as usize;
            String::from_utf8(bytes[20..20 + length].to_vec()).expect("the JSON chunk is UTF-8")
        };
        assert!(!json.contains("\"meshes\":"), "{json}");
    }

    fn vec3(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3 { x, y, z }
    }

    /// A `SubMeshData` covering `index_count` indices from `first_index`.
    fn sub_mesh(topology: MeshTopology, first_index: i32, index_count: i32) -> SubMeshData {
        SubMeshData {
            base_vertex: 0,
            first_index,
            first_vertex: 0,
            index_count,
            triangle_count: index_count / 3,
            vertex_count: 0,
            topology,
            local_bounds: Default::default(),
        }
    }

    fn triangle_mesh() -> MeshData {
        MeshData {
            vertices: vec![
                vec3(0.0, 0.0, 0.0),
                vec3(1.0, 0.0, 0.0),
                vec3(0.0, 1.0, 0.0),
            ],
            normals: None,
            tangents: None,
            colors: None,
            uv0: None,
            uv1: None,
            uv2: None,
            uv3: None,
            uv4: None,
            uv5: None,
            uv6: None,
            uv7: None,
            skin: None,
            bind_pose: None,
            processed_index_buffer: vec![0, 1, 2],
            sub_meshes: vec![sub_mesh(MeshTopology::TRIANGLES, 0, 3)],
        }
    }

    #[test]
    fn coordinate_conversion_matches_the_source_scales() {
        let converted = to_gltf_vector3(vec3(1.0, 2.0, 3.0));
        assert_eq!([converted.x, converted.y, converted.z], [-1.0, 2.0, 3.0]);
        let tangent = to_gltf_tangent(Vec4::new(1.0, 2.0, 3.0, 1.0));
        assert_eq!(
            [tangent.x, tangent.y, tangent.z, tangent.w],
            [-1.0, 2.0, 3.0, -1.0]
        );
        // Reference-runtime output for (1, 0, 0, 1), captured from the isolated baseline build:
        // `QUAT|1|0|0|1|1,-0,-0,1|-1,0,0`. The quaternion keeps a positive x while a plain
        // `ToGltfVector3Convert` of the same vector gives -1; the difference is the axis flip.
        let rotation = to_gltf_quaternion(Quat {
            x: 1.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        });
        assert_eq!(
            [rotation.x, rotation.y, rotation.z, rotation.w],
            [1.0, -0.0, -0.0, 1.0]
        );
    }

    #[test]
    fn mesh_type_follows_the_source_precedence() {
        let mesh = triangle_mesh();
        assert_eq!(get_mesh_type(&mesh).bits(), 0);

        let mut mesh = triangle_mesh();
        mesh.normals = Some(vec![Vec3::default(); 3]);
        assert!(get_mesh_type(&mesh).contains(GlbMeshType::NORMAL));
        assert!(!get_mesh_type(&mesh).contains(GlbMeshType::TANGENT));

        mesh.tangents = Some(vec![Vec4::default(); 3]);
        assert!(get_mesh_type(&mesh).contains(GlbMeshType::TANGENT));

        mesh.colors = Some(vec![
            crate::numerics::ColorFloat::new(1.0, 1.0, 1.0, 1.0);
            3
        ]);
        assert!(get_mesh_type(&mesh).contains(GlbMeshType::COLOR1));

        mesh.skin = Some(vec![BoneWeight4::new(1.0, 0.0, 0.0, 0.0, 0, 0, 0, 0); 3]);
        assert!(get_mesh_type(&mesh).contains(GlbMeshType::JOINTS4));
    }

    #[test]
    fn triangles_are_wound_in_reverse() {
        let mesh = triangle_mesh();
        let exported = build_glb("Tri", &mesh).unwrap();
        assert!(exported.warnings.is_empty());
        assert_eq!(exported.emitted_source_indices, vec![vec![2, 1, 0]]);
        let text = String::from_utf8_lossy(&exported.bytes);
        assert!(text.contains("\"version\":\"2.0\""));
        assert!(text.contains("SubMesh_0"));
        assert!(text.contains("\"mode\":4"));
        // Small meshes use unsigned-short indices, matching the SharpGLTF writer.
        assert!(text.contains("\"componentType\":5123"));
    }

    #[test]
    fn out_of_range_sub_mesh_is_an_error() {
        let mut mesh = triangle_mesh();
        mesh.sub_meshes[0].index_count = 9;
        match build_glb("Bad", &mesh) {
            Err(GlbError::IndexOutOfRange {
                sub_mesh,
                required_length,
                available_length,
                ..
            }) => {
                assert_eq!(sub_mesh, 0);
                assert_eq!(required_length, 9);
                assert_eq!(available_length, 3);
            }
            other => panic!("expected an out-of-range error, got {other:?}"),
        }
    }

    #[test]
    fn partial_trailing_triangle_is_reported() {
        let mut mesh = triangle_mesh();
        mesh.processed_index_buffer = vec![0, 1, 2, 0];
        mesh.sub_meshes[0].index_count = 4;
        let exported = build_glb("Partial", &mesh).unwrap();
        assert_eq!(exported.warnings.len(), 1);
        assert!(exported.warnings[0].contains("trailing indices"));
    }

    #[test]
    fn triangle_strip_is_de_stripified_with_alternating_winding() {
        let mut mesh = triangle_mesh();
        mesh.vertices = (0..5).map(|value| vec3(value as f32, 0.0, 0.0)).collect();
        mesh.processed_index_buffer = vec![0, 1, 2, 3, 4];
        mesh.sub_meshes = vec![sub_mesh(MeshTopology::TRIANGLE_STRIP, 0, 5)];
        let primitive = build_primitive(&mesh, "Strip", 0).unwrap();
        assert_eq!(primitive.source_indices, vec![2, 1, 0, 2, 3, 1, 4, 3, 2]);
        assert_eq!(primitive.vertices.len(), 5);
        assert_eq!(primitive.mode, 4);
        assert!(primitive.warnings.is_empty());
    }

    #[test]
    fn degenerate_strip_triangles_are_skipped() {
        let mut mesh = triangle_mesh();
        mesh.vertices = (0..3).map(|value| vec3(value as f32, 0.0, 0.0)).collect();
        // The (2,2,1) window is degenerate; the (2,1,0) window is not.
        mesh.processed_index_buffer = vec![0, 1, 2, 2];
        mesh.sub_meshes = vec![sub_mesh(MeshTopology::TRIANGLE_STRIP, 0, 4)];
        let primitive = build_primitive(&mesh, "Degenerate", 0).unwrap();
        assert_eq!(primitive.source_indices, vec![2, 1, 0]);
    }

    #[test]
    fn fully_degenerate_strip_emits_nothing() {
        let mut mesh = triangle_mesh();
        mesh.vertices = (0..3).map(|value| vec3(value as f32, 0.0, 0.0)).collect();
        mesh.processed_index_buffer = vec![0, 1, 1, 2];
        mesh.sub_meshes = vec![sub_mesh(MeshTopology::TRIANGLE_STRIP, 0, 4)];
        let primitive = build_primitive(&mesh, "Degenerate", 0).unwrap();
        assert!(primitive.source_indices.is_empty());
    }

    #[test]
    fn quads_emit_two_triangles_each() {
        let mut mesh = triangle_mesh();
        mesh.vertices = (0..4).map(|value| vec3(value as f32, 0.0, 0.0)).collect();
        mesh.processed_index_buffer = vec![0, 1, 2, 3];
        mesh.sub_meshes = vec![sub_mesh(MeshTopology::QUADS, 0, 4)];
        let primitive = build_primitive(&mesh, "Quads", 0).unwrap();
        assert_eq!(primitive.source_indices, vec![3, 2, 1, 3, 1, 0]);
        assert_eq!(primitive.mode, 4);
    }

    #[test]
    fn lines_are_emitted_in_reverse_pair_order() {
        let mut mesh = triangle_mesh();
        mesh.vertices = (0..4).map(|value| vec3(value as f32, 0.0, 0.0)).collect();
        mesh.processed_index_buffer = vec![0, 1, 2, 3];
        mesh.sub_meshes = vec![sub_mesh(MeshTopology::LINES, 0, 4)];
        let primitive = build_primitive(&mesh, "Lines", 0).unwrap();
        assert_eq!(primitive.source_indices, vec![1, 0, 3, 2]);
        assert_eq!(primitive.mode, 1);
    }

    #[test]
    fn line_strip_becomes_a_line_list() {
        let mut mesh = triangle_mesh();
        mesh.vertices = (0..4).map(|value| vec3(value as f32, 0.0, 0.0)).collect();
        mesh.processed_index_buffer = vec![0, 1, 2, 3];
        mesh.sub_meshes = vec![sub_mesh(MeshTopology::LINE_STRIP, 0, 4)];
        let primitive = build_primitive(&mesh, "LineStrip", 0).unwrap();
        assert_eq!(primitive.source_indices, vec![1, 0, 2, 1, 3, 2]);
        assert_eq!(primitive.mode, 1);
    }

    #[test]
    fn points_are_emitted_in_order() {
        let mut mesh = triangle_mesh();
        mesh.vertices = (0..3).map(|value| vec3(value as f32, 0.0, 0.0)).collect();
        mesh.processed_index_buffer = vec![2, 0, 1];
        mesh.sub_meshes = vec![sub_mesh(MeshTopology::POINTS, 0, 3)];
        let primitive = build_primitive(&mesh, "Points", 0).unwrap();
        assert_eq!(primitive.source_indices, vec![2, 0, 1]);
        assert_eq!(primitive.mode, 0);
    }

    #[test]
    fn triangles_are_wound_in_reverse_source_order() {
        let mut mesh = triangle_mesh();
        mesh.processed_index_buffer = vec![0, 1, 2, 2, 1, 3];
        mesh.vertices = (0..4).map(|value| vec3(value as f32, 0.0, 0.0)).collect();
        mesh.sub_meshes = vec![sub_mesh(MeshTopology::TRIANGLES, 0, 6)];
        let primitive = build_primitive(&mesh, "Tris", 0).unwrap();
        assert_eq!(primitive.source_indices, vec![2, 1, 0, 3, 1, 2]);
    }

    #[test]
    fn unsupported_topology_is_an_error() {
        let mut mesh = triangle_mesh();
        mesh.sub_meshes = vec![sub_mesh(MeshTopology(9), 0, 3)];
        match build_glb("Weird", &mesh) {
            Err(GlbError::UnsupportedTopology { topology, .. }) => assert_eq!(topology, 9),
            other => panic!("expected an unsupported topology error, got {other:?}"),
        }
    }

    #[test]
    fn non_finite_positions_are_rejected() {
        let mut mesh = triangle_mesh();
        mesh.vertices[1].x = f32::NAN;
        match build_glb("NaN", &mesh) {
            Err(GlbError::NonFinitePosition { component, .. }) => assert_eq!(component, "x"),
            other => panic!("expected a non-finite position error, got {other:?}"),
        }
    }

    #[test]
    fn empty_mesh_is_rejected() {
        let mut mesh = triangle_mesh();
        mesh.vertices.clear();
        assert_eq!(build_glb("Empty", &mesh), Err(GlbError::NoVertices));
    }

    #[test]
    fn glb_container_is_well_formed() {
        let mesh = triangle_mesh();
        let exported = build_glb("Container", &mesh).unwrap();
        let bytes = &exported.bytes;
        assert_eq!(&bytes[0..4], b"glTF");
        assert_eq!(
            u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
            2,
            "version"
        );
        assert_eq!(
            u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize,
            bytes.len(),
            "total length"
        );
        let json_length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        assert_eq!(&bytes[16..20], b"JSON");
        assert_eq!(json_length % 4, 0, "JSON chunk is 4-byte aligned");
        let bin_header = 20 + json_length;
        assert_eq!(
            &bytes[bin_header + 4..bin_header + 8],
            b"BIN\0",
            "binary chunk type"
        );
        let bin_length =
            u32::from_le_bytes(bytes[bin_header..bin_header + 4].try_into().unwrap()) as usize;
        assert_eq!(bin_length % 4, 0, "binary chunk is 4-byte aligned");
        assert_eq!(bin_header + 8 + bin_length, bytes.len());
    }

    #[test]
    fn emitted_index_counts_match_the_topology_rewrite() {
        assert_eq!(emitted_index_count(MeshTopology::TRIANGLES, 6), 6);
        assert_eq!(emitted_index_count(MeshTopology::TRIANGLES, 7), 6);
        assert_eq!(emitted_index_count(MeshTopology::QUADS, 8), 12);
        assert_eq!(emitted_index_count(MeshTopology::LINES, 4), 4);
        assert_eq!(emitted_index_count(MeshTopology::POINTS, 5), 5);
        assert_eq!(emitted_index_count(MeshTopology::LINE_STRIP, 4), 6);
        assert_eq!(emitted_index_count(MeshTopology::LINE_STRIP, 1), 0);
        assert_eq!(emitted_index_count(MeshTopology::TRIANGLE_STRIP, 5), 9);
        assert_eq!(emitted_index_count(MeshTopology::TRIANGLE_STRIP, 2), 0);
    }

    #[test]
    fn identical_vertices_are_shared() {
        let mut mesh = triangle_mesh();
        mesh.processed_index_buffer = vec![0, 0, 0];
        let exported = build_glb("Shared", &mesh).unwrap();
        let text = String::from_utf8_lossy(&exported.bytes);
        // One shared vertex still needs one accessor entry for POSITION.
        assert_eq!(text.matches("\"POSITION\":").count(), 1);
        assert_eq!(text.matches("\"componentType\":5123").count(), 1);
    }

    #[test]
    fn color_channels_quantize_with_truncation() {
        // Probe values from the SharpGLTF writer: 0.5 becomes 127, not 128.
        assert_eq!(quantize_color_channel(0.0), 0);
        assert_eq!(quantize_color_channel(0.25), 63);
        assert_eq!(quantize_color_channel(0.5), 127);
        assert_eq!(quantize_color_channel(0.999), 254);
        assert_eq!(quantize_color_channel(1.0), 255);
        assert_eq!(quantize_color_channel(1.5), 255);
        assert_eq!(quantize_color_channel(-0.5), 0);
        // Any non-finite channel zeroes the whole color, including finite alpha.
        assert_eq!(quantize_color([0.5, 0.5, 0.5, 1.0]), [127, 127, 127, 255]);
        assert_eq!(quantize_color([f32::INFINITY, 0.5, 0.5, 1.0]), [0, 0, 0, 0]);
        assert_eq!(quantize_color([0.5, 0.5, 0.5, f32::NAN]), [0, 0, 0, 0]);
    }

    #[test]
    fn skin_bindings_follow_the_sparse_weight_rules() {
        // Duplicate indices merge by summation, then renormalize to one.
        assert_eq!(
            process_skin_bindings(&[1, 1, 0, 0], &[0.4, 0.6, 0.0, 0.0], "M", 0).unwrap(),
            (Some([1, 0, 0, 0]), Some([1.0, 0.0, 0.0, 0.0]))
        );
        // Exact-zero weights drop out; sub-unit sums scale up.
        assert_eq!(
            process_skin_bindings(&[0, 1, 2, 3], &[0.3, 0.3, 0.0, 0.0], "M", 0).unwrap(),
            (Some([0, 1, 0, 0]), Some([0.5, 0.5, 0.0, 0.0]))
        );
        // An exact unit sum is left untouched.
        assert_eq!(
            process_skin_bindings(&[2, 0, 0, 0], &[1.0, 0.0, 0.0, 0.0], "M", 0).unwrap(),
            (Some([2, 0, 0, 0]), Some([1.0, 0.0, 0.0, 0.0]))
        );
        // An empty set stays zero, mirroring the skipped renormalization.
        assert_eq!(
            process_skin_bindings(&[5, 6, 0, 0], &[0.0, 0.0, 0.0, 0.0], "M", 0).unwrap(),
            (Some([0, 0, 0, 0]), Some([0.0, 0.0, 0.0, 0.0]))
        );
        // Negative indices fail with context.
        assert_eq!(
            process_skin_bindings(&[-1, 0, 0, 0], &[1.0, 0.0, 0.0, 0.0], "M", 7).unwrap_err(),
            GlbError::NegativeBoneIndex {
                mesh: "M".to_owned(),
                vertex: 7,
                bone_index: -1,
            }
        );
        // Indices beyond the USHORT encoding fail with context.
        assert_eq!(
            process_skin_bindings(&[70000, 0, 0, 0], &[1.0, 0.0, 0.0, 0.0], "M", 7).unwrap_err(),
            GlbError::BoneIndexTooLarge {
                mesh: "M".to_owned(),
                vertex: 7,
                bone_index: 70000,
            }
        );
    }

    #[test]
    fn accessor_widths_follow_the_writer_rules() {
        let mut builder = GlbBuilder::default();
        // Colors are normalized unsigned bytes.
        let color = builder
            .push_color_accessor(&[[1.0, 0.0, 0.5, 0.25]])
            .unwrap();
        assert!(builder.accessors[color].contains("\"componentType\":5121"));
        assert!(builder.accessors[color].contains("\"normalized\":true"));
        // Joints fit in a byte while every lane does, and widen otherwise.
        let small = builder.push_joints_accessor(&[[1, 2, 3, 4]]).unwrap();
        assert!(builder.accessors[small].contains("\"componentType\":5121"));
        assert!(!builder.accessors[small].contains("normalized"));
        let big = builder.push_joints_accessor(&[[300, 0, 0, 0]]).unwrap();
        assert!(builder.accessors[big].contains("\"componentType\":5123"));
        // Indices fit in a short below the reserved 65535.
        let short = builder.push_index_accessor(&[0, 1, 65534]).unwrap();
        assert!(builder.accessors[short].contains("\"componentType\":5123"));
        let long = builder.push_index_accessor(&[0, 1, 65535]).unwrap();
        assert!(builder.accessors[long].contains("\"componentType\":5125"));
    }

    #[test]
    fn oversized_output_is_refused_with_context() {
        let mesh = triangle_mesh();
        let error = build_glb_with_limits(
            "Tri",
            &mesh,
            GlbBuildLimits {
                max_output_bytes: 16,
            },
        )
        .unwrap_err();
        assert!(matches!(error, GlbError::OutputTooLarge { limit: 16, .. }));
        assert!(error.to_string().contains("too large to export"));
        // The default cap is the source 2Gb buffer ceiling.
        assert_eq!(
            GlbBuildLimits::default().max_output_bytes,
            2 * 1024 * 1024 * 1024
        );
        build_glb("Tri", &mesh).unwrap();
    }

    #[test]
    fn json_escaping_is_applied_to_names() {
        let mesh = triangle_mesh();
        let exported = build_glb("a\"b\\c", &mesh).unwrap();
        let text = String::from_utf8_lossy(&exported.bytes);
        assert!(text.contains("a\\\"b\\\\c"));
    }

    #[test]
    fn renderer_mesh_dispatch_matches_the_source_gates() {
        // An empty static-batch list forces the dynamic path.
        assert!(references_dynamic_mesh(true, 0, false, 0));
        // An empty subset list forces the dynamic path.
        assert!(references_dynamic_mesh(false, 5, true, 0));
        // Either empty list wins on its own, even beside a non-empty one.
        assert!(references_dynamic_mesh(true, 0, true, 3));
        assert!(references_dynamic_mesh(true, 2, true, 0));
        // Non-empty lists stay on the static path.
        assert!(!references_dynamic_mesh(true, 2, false, 0));
        assert!(!references_dynamic_mesh(false, 5, true, 3));
        assert!(!references_dynamic_mesh(false, 5, false, 9));
        // Explicit subset indices win over the static-batch range.
        assert_eq!(subset_indices(Some(&[2, 0]), Some((0, 3))), vec![2, 0]);
        // Otherwise the batch range expands from its first sub-mesh.
        assert_eq!(subset_indices(None, Some((1, 3))), vec![1, 2, 3]);
        assert_eq!(subset_indices(None, Some((5, 0))), Vec::<u32>::new());
        // Otherwise the renderer contributes no sub-meshes.
        assert!(subset_indices(None, None).is_empty());
    }

    /// The decoded container of a GLB produced by this module.
    struct DecodedGlb {
        json: String,
        bin: Vec<u8>,
    }

    /// Parses the GLB container independently of the writer (magic, version, chunk framing).
    fn decode_glb(bytes: &[u8]) -> DecodedGlb {
        assert!(bytes.len() >= 20, "GLB is shorter than its header");
        assert_eq!(&bytes[0..4], b"glTF");
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 2);
        assert_eq!(
            u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize,
            bytes.len()
        );
        let json_length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        assert_eq!(&bytes[16..20], b"JSON");
        let json = String::from_utf8(bytes[20..20 + json_length].to_vec()).unwrap();
        let offset = 20 + json_length;
        let mut bin = Vec::new();
        if offset + 8 <= bytes.len() {
            let bin_length =
                u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
            assert_eq!(&bytes[offset + 4..offset + 8], b"BIN\0");
            bin = bytes[offset + 8..offset + 8 + bin_length].to_vec();
        }
        DecodedGlb { json, bin }
    }

    /// The raw text of a top-level JSON array value.
    fn json_array<'a>(json: &'a str, key: &str) -> Option<&'a str> {
        let bytes = json.as_bytes();
        let needle = format!("\"{key}\"");
        let mut object_depth = 0usize;
        let mut array_depth = 0usize;
        let mut in_string = false;
        let mut escaped = false;
        let mut index = 0usize;

        while index < bytes.len() {
            let byte = bytes[index];
            if in_string {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    in_string = false;
                }
                index += 1;
                continue;
            }

            match byte {
                b'"' => {
                    if object_depth == 1 && array_depth == 0 && json[index..].starts_with(&needle) {
                        let mut cursor = index + needle.len();
                        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
                            cursor += 1;
                        }
                        if bytes.get(cursor) == Some(&b':') {
                            cursor += 1;
                            while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
                                cursor += 1;
                            }
                            if bytes.get(cursor) == Some(&b'[') {
                                let start = cursor + 1;
                                let mut depth = 1usize;
                                let mut array_string = false;
                                let mut array_escape = false;
                                for end in start..bytes.len() {
                                    let array_byte = bytes[end];
                                    if array_string {
                                        if array_escape {
                                            array_escape = false;
                                        } else if array_byte == b'\\' {
                                            array_escape = true;
                                        } else if array_byte == b'"' {
                                            array_string = false;
                                        }
                                    } else {
                                        match array_byte {
                                            b'"' => array_string = true,
                                            b'[' => depth += 1,
                                            b']' => {
                                                depth -= 1;
                                                if depth == 0 {
                                                    return Some(&json[start..end]);
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                return None;
                            }
                        }
                    }
                    in_string = true;
                }
                b'{' => object_depth += 1,
                b'}' => object_depth = object_depth.saturating_sub(1),
                b'[' => array_depth += 1,
                b']' => array_depth = array_depth.saturating_sub(1),
                _ => {}
            }
            index += 1;
        }
        None
    }

    /// Splits a flat array of JSON objects into its object bodies.
    fn json_objects(array: &str) -> Vec<String> {
        array
            .split("},{")
            .map(|part| part.trim_matches(|character| character == '{' || character == '}'))
            .filter(|part| !part.is_empty())
            .map(str::to_owned)
            .collect()
    }

    /// The integer value of a flat object field.
    fn json_number(object: &str, key: &str) -> Option<i64> {
        let needle = format!("\"{key}\":");
        let start = object.find(&needle)? + needle.len();
        let rest = &object[start..];
        let end = rest
            .find(|character: char| !(character.is_ascii_digit() || character == '-'))
            .unwrap_or(rest.len());
        rest[..end].parse().ok()
    }

    /// The string value of a flat object field.
    fn json_string(object: &str, key: &str) -> Option<String> {
        let needle = format!("\"{key}\":\"");
        let start = object.find(&needle)? + needle.len();
        let end = object[start..].find('"')? + start;
        Some(object[start..end].to_owned())
    }

    /// The decoded floats of one accessor, in accessor order.
    fn accessor_floats(glb: &DecodedGlb, accessor: usize) -> Vec<f32> {
        let accessors = json_objects(json_array(&glb.json, "accessors").unwrap());
        let views = json_objects(json_array(&glb.json, "bufferViews").unwrap());
        let object = &accessors[accessor];
        let view = &views[json_number(object, "bufferView").unwrap() as usize];
        let count = json_number(object, "count").unwrap() as usize;
        let components = match json_string(object, "type").unwrap().as_str() {
            "VEC2" => 2,
            "VEC3" => 3,
            _ => 4,
        };
        let offset = json_number(view, "byteOffset").unwrap() as usize;
        (0..count * components)
            .map(|index| {
                let start = offset + index * 4;
                f32::from_le_bytes(glb.bin[start..start + 4].try_into().unwrap())
            })
            .collect()
    }

    /// The decoded indices of one accessor.
    fn accessor_indices(glb: &DecodedGlb, accessor: usize) -> Vec<u32> {
        let accessors = json_objects(json_array(&glb.json, "accessors").unwrap());
        let views = json_objects(json_array(&glb.json, "bufferViews").unwrap());
        let object = &accessors[accessor];
        let view = &views[json_number(object, "bufferView").unwrap() as usize];
        let count = json_number(object, "count").unwrap() as usize;
        let component_type = json_number(object, "componentType").unwrap();
        let offset = json_number(view, "byteOffset").unwrap() as usize;
        (0..count)
            .map(|index| match component_type {
                5123 => {
                    let start = offset + index * 2;
                    u16::from_le_bytes(glb.bin[start..start + 2].try_into().unwrap()) as u32
                }
                _ => {
                    let start = offset + index * 4;
                    u32::from_le_bytes(glb.bin[start..start + 4].try_into().unwrap())
                }
            })
            .collect()
    }

    /// The single primitive body of the single-mesh JSON layout.
    fn single_primitive(json: &str) -> String {
        let anchor = json.find("\"primitives\":[").unwrap();
        let start = anchor + "\"primitives\":[".len();
        let end = json[start..].find(']').unwrap() + start;
        json[start..end]
            .trim()
            .trim_matches(|character| character == '{' || character == '}')
            .to_owned()
    }

    /// The accessor number of a named primitive attribute.
    fn attribute_accessor(primitive: &str, name: &str) -> Option<usize> {
        let anchor = primitive.find("\"attributes\":{")? + "\"attributes\":{".len();
        let end = primitive[anchor..].find('}')? + anchor;
        json_number(&primitive[anchor..end], name).map(|value| value as usize)
    }

    /// The reference exception name a `GlbError` stands in for.
    fn reference_exception(error: &GlbError) -> &'static str {
        match error {
            GlbError::TerrainUvScaleDivisionByZero { .. } => "DivideByZeroException",
            GlbError::TerrainGridNotSquare { .. }
            | GlbError::TerrainHeightsTooShort { .. }
            | GlbError::VertexIndexOutOfRange { .. }
            | GlbError::IndexOutOfRange { .. } => "IndexOutOfRangeException",
            GlbError::NavMeshIndexNotTriangles { .. } => "Exception",
            other => panic!("unmapped reference exception for {other}"),
        }
    }

    /// One parsed fixture case.
    #[derive(Debug, Default)]
    struct BuilderCase {
        tag: String,
        nav: bool,
        version: String,
        name: String,
        width: i32,
        height: i32,
        resolution: i32,
        scale: [f32; 3],
        heights: Vec<i16>,
        height_count: usize,
        read_back: (i32, i32, i32),
        nav_meshes: Vec<(Vec<[f32; 3]>, Vec<i32>)>,
        positions: Vec<[f32; 3]>,
        uvs: Vec<[f32; 2]>,
        indices: Vec<u32>,
        has_uv: bool,
        nodes: Vec<String>,
        empty_nodes: Option<usize>,
        error: Option<String>,
    }

    fn parse_f32(text: &str) -> f32 {
        let value: f32 = text.parse().expect("a probe float");
        if value == 0.0 && text.starts_with('-') {
            -0.0
        } else {
            value
        }
    }

    fn parse_csv_i32(text: &str) -> Vec<i32> {
        text.split(',')
            .filter(|part| !part.is_empty())
            .map(|part| part.parse().expect("a probe integer"))
            .collect()
    }

    /// Round-trips `tools/probes/res-glb` through the production builders.
    ///
    /// Every row was produced by the shipped `GlbTerrainBuilder`/`GlbNavMeshExporter` over real
    /// generated objects; the test asserts the container structure, node names, and the decoded
    /// POSITION/TEXCOORD_0/indices bit-for-bit (including the sign of zero).
    #[test]
    fn glb_builders_match_the_reference_runtime() {
        let fixture = include_str!("glb_builders_fixture.txt");
        let mut cases: Vec<BuilderCase> = Vec::new();
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<&str> = line.split('|').collect();
            match fields[0] {
                "GIN" => {
                    let case = BuilderCase {
                        tag: fields[1].to_owned(),
                        nav: false,
                        version: fields[2].to_owned(),
                        width: fields[3].parse().unwrap(),
                        height: fields[4].parse().unwrap(),
                        resolution: fields[5].parse().unwrap(),
                        scale: [
                            parse_f32(fields[6]),
                            parse_f32(fields[7]),
                            parse_f32(fields[8]),
                        ],
                        name: fields[9].to_owned(),
                        height_count: fields[10].parse().unwrap(),
                        ..BuilderCase::default()
                    };
                    cases.push(case);
                }
                "GHTS" => {
                    let case = cases.last_mut().expect("a GHTS row follows a GIN row");
                    case.heights = fields[2]
                        .split(',')
                        .filter(|part| !part.is_empty())
                        .map(|part| part.parse().expect("a probe height"))
                        .collect();
                    assert_eq!(case.heights.len(), case.height_count, "{line}");
                }
                "GREAD" => {
                    let case = cases.last_mut().expect("a GREAD row follows a GIN row");
                    case.read_back = (
                        fields[2].parse().unwrap(),
                        fields[3].parse().unwrap(),
                        fields[4].parse().unwrap(),
                    );
                }
                "NIN" => {
                    cases.push(BuilderCase {
                        tag: fields[1].to_owned(),
                        nav: true,
                        version: fields[2].to_owned(),
                        name: fields[3].to_owned(),
                        ..BuilderCase::default()
                    });
                }
                "NVTS" => {
                    let case = cases.last_mut().expect("an NVTS row follows a NIN row");
                    let index: usize = fields[2].parse().unwrap();
                    let vertices: Vec<[f32; 3]> = fields[3]
                        .split(';')
                        .filter(|part| !part.is_empty())
                        .map(|vertex| {
                            let components: Vec<f32> = vertex.split(',').map(parse_f32).collect();
                            [components[0], components[1], components[2]]
                        })
                        .collect();
                    while case.nav_meshes.len() <= index {
                        case.nav_meshes.push((Vec::new(), Vec::new()));
                    }
                    case.nav_meshes[index].0 = vertices;
                }
                "NIDX" => {
                    let case = cases.last_mut().expect("an NIDX row follows a NIN row");
                    let index: usize = fields[2].parse().unwrap();
                    let indices = parse_csv_i32(fields[3]);
                    case.nav_meshes[index].1 = indices;
                }
                "GMESH" => {
                    let _ = cases.last();
                }
                "GPOS" => {
                    let case = cases.last_mut().unwrap();
                    let index: usize = fields[2].parse().unwrap();
                    assert_eq!(index, case.positions.len(), "{line}");
                    case.positions.push([
                        parse_f32(fields[3]),
                        parse_f32(fields[4]),
                        parse_f32(fields[5]),
                    ]);
                }
                "GUV" => {
                    let case = cases.last_mut().unwrap();
                    let index: usize = fields[2].parse().unwrap();
                    assert_eq!(index, case.uvs.len(), "{line}");
                    case.has_uv = true;
                    case.uvs.push([parse_f32(fields[3]), parse_f32(fields[4])]);
                }
                "GIDX" => {
                    let case = cases.last_mut().unwrap();
                    case.indices = parse_csv_i32(fields[2])
                        .into_iter()
                        .map(|value| value as u32)
                        .collect();
                }
                "GNODE" => {
                    let case = cases.last_mut().unwrap();
                    case.nodes.push(fields[2].to_owned());
                }
                "GEMPTY" => {
                    let case = cases.last_mut().unwrap();
                    case.empty_nodes = Some(
                        fields[3]
                            .strip_prefix("nodes=")
                            .expect("a nodes= count")
                            .parse()
                            .unwrap(),
                    );
                }
                "GERR" => {
                    let case = cases.last_mut().unwrap();
                    if fields[2] == "inner" {
                        case.error = Some(fields[3].to_owned());
                    }
                }
                other => panic!("unexpected fixture row {other}"),
            }
        }
        assert_eq!(cases.len(), 9, "every probed case is asserted");

        let mut mesh_cases = 0;
        for case in &cases {
            if !case.nav {
                match case.version.as_str() {
                    "2019.4.0f1" => assert_eq!(
                        case.read_back,
                        (0, 0, case.resolution),
                        "{} uses the modern Heightmap stubs",
                        case.tag
                    ),
                    "5.0.0f1" => assert_eq!(
                        case.read_back,
                        (case.width, case.height, 0),
                        "{} uses the legacy Heightmap stubs",
                        case.tag
                    ),
                    version => {
                        panic!("unmapped generated Heightmap behavior for {version}")
                    }
                }
            }
            let result = if case.nav {
                let meshes: Vec<NavMeshHeightMesh<'_>> = case
                    .nav_meshes
                    .iter()
                    .map(|(vertices, indices)| NavMeshHeightMesh { indices, vertices })
                    .collect();
                build_navmesh_glb(&case.name, &meshes)
            } else {
                let input = TerrainMeshInput {
                    name: &case.name,
                    width: case.read_back.0,
                    height: case.read_back.1,
                    resolution: case.read_back.2,
                    scale: case.scale,
                    heights: &case.heights,
                };
                build_terrain_glb(&input)
            };
            match (&case.error, result) {
                (Some(exception), Err(error)) => {
                    assert_eq!(reference_exception(&error), *exception, "{}", case.tag);
                }
                (Some(exception), Ok(_)) => {
                    panic!("{} should fail like the reference {exception}", case.tag)
                }
                (None, Err(error)) => panic!("{} failed: {error}", case.tag),
                (None, Ok(export)) => {
                    let glb = decode_glb(&export.bytes);
                    assert_eq!(
                        json_array(&glb.json, "nodes")
                            .unwrap()
                            .matches("\"name\"")
                            .count(),
                        case.empty_nodes.unwrap_or(1),
                        "{}; GLB JSON: {}",
                        case.tag,
                        glb.json
                    );
                    assert!(glb.json.contains(&format!("\"name\":\"{}\"", case.name)));
                    if case.empty_nodes.is_some() {
                        assert_eq!(export.emitted_source_indices.len(), 1);
                        assert!(export.emitted_source_indices[0].is_empty());
                        assert!(!glb.json.contains("\"meshes\""));
                        assert!(glb.bin.is_empty());
                        continue;
                    }
                    assert_eq!(export.emitted_source_indices.len(), 1);
                    mesh_cases += 1;
                    let primitive = single_primitive(&glb.json);
                    let position_accessor =
                        attribute_accessor(&primitive, "POSITION").expect("a POSITION accessor");
                    let positions = accessor_floats(&glb, position_accessor);
                    assert_eq!(positions.len(), case.positions.len() * 3, "{}", case.tag);
                    for (index, expected) in case.positions.iter().enumerate() {
                        for axis in 0..3 {
                            assert_eq!(
                                positions[index * 3 + axis].to_bits(),
                                expected[axis].to_bits(),
                                "{} POSITION vertex {index} axis {axis}",
                                case.tag
                            );
                        }
                    }
                    assert_eq!(
                        attribute_accessor(&primitive, "TEXCOORD_0").is_some(),
                        case.has_uv,
                        "{}",
                        case.tag
                    );
                    if case.has_uv {
                        let uv_accessor = attribute_accessor(&primitive, "TEXCOORD_0").unwrap();
                        let uvs = accessor_floats(&glb, uv_accessor);
                        assert_eq!(uvs.len(), case.uvs.len() * 2, "{}", case.tag);
                        for (index, expected) in case.uvs.iter().enumerate() {
                            for axis in 0..2 {
                                assert_eq!(
                                    uvs[index * 2 + axis].to_bits(),
                                    expected[axis].to_bits(),
                                    "{} TEXCOORD_0 vertex {index} axis {axis}",
                                    case.tag
                                );
                            }
                        }
                    }
                    let index_accessor =
                        json_number(&primitive, "indices").expect("an index accessor") as usize;
                    assert_eq!(
                        accessor_indices(&glb, index_accessor),
                        case.indices,
                        "{}",
                        case.tag
                    );
                    if case.nav {
                        assert_eq!(
                            export.emitted_source_indices[0]
                                .iter()
                                .map(|value| *value as usize)
                                .collect::<Vec<_>>(),
                            case.positions_source_order(),
                            "{}",
                            case.tag
                        );
                    }
                }
            }
        }
        assert_eq!(
            mesh_cases, 5,
            "four terrain and one navmesh case carry geometry"
        );
    }

    impl BuilderCase {
        /// The source vertex index sequence the reference's AddTriangle calls produced.
        fn positions_source_order(&self) -> Vec<usize> {
            self.nav_meshes
                .iter()
                .flat_map(|(_, indices)| {
                    indices
                        .chunks_exact(3)
                        .flat_map(|triangle| {
                            [triangle[2], triangle[1], triangle[0]]
                                .into_iter()
                                .map(|value| value as usize)
                        })
                        .collect::<Vec<_>>()
                })
                .collect()
        }
    }
