//! Mesh UV-distribution metric, ported from `MeshExtensions.CalculateMeshMetric`,
//! `TriangleArea`, and `TriangleEnumerable`.
//!
//! The reference computes two metrics per mesh (`MeshMetrics_0_` from `UV0`, `MeshMetrics_1_`
//! from `UV1`) as the average triangle area divided by the total UV area, skipping UV-space
//! degenerates below `1e-9`. Empty vertex/UV buffers, or a UV-set index past the sub-mesh list,
//! yield the `1.0` default. `TriangleEnumerable` rewrites strips (degenerate removal, alternating
//! winding) and quads into triangles; line/point topologies are rejected.
//!
//! One deliberate difference: the source indexes the buffers without checking. A partial trailing
//! index group, an index outside the vertex/UV buffers, or an unsupported topology throws in the
//! reference; here those are explicit [`MeshMetricError`] values carrying the sub-mesh position,
//! because the callers of this crate must not turn malformed payloads into panics.
//!
//! Also ported are the processed-index-buffer codecs that feed the metric pipeline:
//! `UShortToUInt`/`UIntToUShort` (the guarded lane conversions) and the
//! `SetProcessedIndexBuffer` byte assembly over caller facts (index width and file endianness).
//! The collection writes around the assembly stay caller-side.

use std::error::Error;
use std::fmt::{self, Display};

use crate::numerics::{Vec2, Vec3};

use super::sub_mesh_data::{MeshTopology, SubMeshData};

/// The default metric the reference returns for empty or inapplicable inputs.
pub const DEFAULT_MESH_METRIC: f32 = 1.0;

/// The UV-area threshold below which the reference skips a triangle.
pub const UV_AREA_THRESHOLD: f32 = 1e-9;

/// A failure while enumerating triangles or computing a mesh metric.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MeshMetricError {
    /// The topology is not triangles, a strip, or quads.
    UnsupportedTopology {
        /// The zero-based sub-mesh position.
        sub_mesh: usize,
        /// The raw `MeshTopology` value.
        topology: i32,
    },
    /// A trailing index group is not a complete triangle/quad group, or the sub-mesh
    /// range overruns the index buffer (the source constructor throws in that case).
    PartialTrailingGroup {
        /// The zero-based sub-mesh position.
        sub_mesh: usize,
        /// The raw `MeshTopology` value.
        topology: i32,
        /// How many trailing indices could not form a group.
        trailing: u64,
    },
    /// An index does not fit the vertex/UV buffers.
    IndexOutOfRange {
        /// The zero-based sub-mesh position.
        sub_mesh: usize,
        /// The offending index.
        index: u32,
        /// The vertex buffer length.
        vertices: usize,
        /// The UV buffer length.
        uvs: usize,
    },
    /// A lane-conversion buffer is shorter than the requested index count (the source
    /// `ArgumentOutOfRangeException` on `indexCount`).
    IndexBufferTooShort {
        /// The available lane count.
        available: usize,
        /// The requested index count.
        required: usize,
    },
}

impl Display for MeshMetricError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedTopology { sub_mesh, topology } => write!(
                formatter,
                "sub-mesh {sub_mesh} has unsupported MeshTopology {topology} for triangle enumeration"
            ),
            Self::PartialTrailingGroup {
                sub_mesh,
                topology,
                trailing,
            } => write!(
                formatter,
                "sub-mesh {sub_mesh} (topology {topology}) has {trailing} trailing indices that do not form a complete group"
            ),
            Self::IndexOutOfRange {
                sub_mesh,
                index,
                vertices,
                uvs,
            } => write!(
                formatter,
                "sub-mesh {sub_mesh} index {index} is outside the vertex buffer ({vertices}) and UV buffer ({uvs})"
            ),
            Self::IndexBufferTooShort { available, required } => write!(
                formatter,
                "index conversion needs {required} lanes but the buffer holds {available}"
            ),
        }
    }
}

impl Error for MeshMetricError {}

/// Whether `TriangleEnumerable` accepts the topology.
pub const fn is_triangle_topology(topology: MeshTopology) -> bool {
    matches!(
        topology,
        MeshTopology::TRIANGLES | MeshTopology::TRIANGLE_STRIP | MeshTopology::QUADS
    )
}

/// `TriangleEnumerable`: the emitted index triples of one sub-mesh range.
///
/// The winding matches the source exactly, including the strip flip-flop and the quad split into
/// `(q, q+1, q+2)` + `(q, q+2, q+3)`.
pub fn triangle_triples(
    sub_mesh: usize,
    topology: MeshTopology,
    indices: &[u32],
) -> Result<Vec<(u32, u32, u32)>, MeshMetricError> {
    if !is_triangle_topology(topology) {
        return Err(MeshMetricError::UnsupportedTopology {
            sub_mesh,
            topology: topology.0,
        });
    }
    let mut triples = Vec::new();
    match topology {
        MeshTopology::TRIANGLES => {
            let mut offset = 0;
            while offset + 3 <= indices.len() {
                triples.push((indices[offset], indices[offset + 1], indices[offset + 2]));
                offset += 3;
            }
            if offset != indices.len() {
                return Err(MeshMetricError::PartialTrailingGroup {
                    sub_mesh,
                    topology: topology.0,
                    trailing: (indices.len() - offset) as u64,
                });
            }
        }
        MeshTopology::TRIANGLE_STRIP => {
            // Every window of three consecutive indices is one triangle; a one- or
            // two-index tail forms no window and yields nothing, exactly like the source.
            let mut offset = 0;
            while offset + 3 <= indices.len() {
                let (a, b, c) = (indices[offset], indices[offset + 1], indices[offset + 2]);
                if a != b && a != c && b != c {
                    if offset & 1 == 1 {
                        triples.push((b, a, c));
                    } else {
                        triples.push((a, b, c));
                    }
                }
                offset += 1;
            }
        }
        _ => {
            // Quads: the only remaining supported topology.
            let mut offset = 0;
            while offset + 4 <= indices.len() {
                triples.push((indices[offset], indices[offset + 1], indices[offset + 2]));
                triples.push((indices[offset], indices[offset + 2], indices[offset + 3]));
                offset += 4;
            }
            if offset != indices.len() {
                return Err(MeshMetricError::PartialTrailingGroup {
                    sub_mesh,
                    topology: topology.0,
                    trailing: (indices.len() - offset) as u64,
                });
            }
        }
    }
    Ok(triples)
}

/// `TriangleArea(Vector3, Vector3, Vector3)`: `Cross(b - a, c - a).Length() * 0.5`.
///
/// The subtraction, cross-product, and length follow the `System.Numerics` operation order so the
/// single-precision result matches the reference bit-for-bit on finite inputs.
pub fn triangle_area_v3(a: Vec3, b: Vec3, c: Vec3) -> f32 {
    let ab = Vec3 {
        x: b.x - a.x,
        y: b.y - a.y,
        z: b.z - a.z,
    };
    let ac = Vec3 {
        x: c.x - a.x,
        y: c.y - a.y,
        z: c.z - a.z,
    };
    let cross = Vec3 {
        x: ab.y * ac.z - ab.z * ac.y,
        y: ab.z * ac.x - ab.x * ac.z,
        z: ab.x * ac.y - ab.y * ac.x,
    };
    cross.length() * 0.5
}

/// `TriangleArea(Vector2, Vector2, Vector2)`, via the `AsVector3` lift.
pub fn triangle_area_v2(a: Vec2, b: Vec2, c: Vec2) -> f32 {
    triangle_area_v3(
        Vec3 {
            x: a.x,
            y: a.y,
            z: 0.0,
        },
        Vec3 {
            x: b.x,
            y: b.y,
            z: 0.0,
        },
        Vec3 {
            x: c.x,
            y: c.y,
            z: 0.0,
        },
    )
}

/// `MeshExtensions.UShortToUInt`: widen `count` lanes, rejecting short buffers like the
/// source guard.
pub fn ushort_to_uint(source: &[u16], count: usize) -> Result<Vec<u32>, MeshMetricError> {
    if source.len() < count {
        return Err(MeshMetricError::IndexBufferTooShort {
            available: source.len(),
            required: count,
        });
    }
    Ok(source
        .iter()
        .take(count)
        .map(|lane| u32::from(*lane))
        .collect())
}

/// `MeshExtensions.UIntToUShort`: narrow `count` lanes with the source wrapping cast,
/// rejecting short buffers like the source guard.
pub fn uint_to_ushort(source: &[u32], count: usize) -> Result<Vec<u16>, MeshMetricError> {
    if source.len() < count {
        return Err(MeshMetricError::IndexBufferTooShort {
            available: source.len(),
            required: count,
        });
    }
    Ok(source.iter().take(count).map(|lane| *lane as u16).collect())
}

/// `MeshExtensions.SetProcessedIndexBuffer` byte assembly over caller facts.
///
/// `is_16_bit` is the `Is16BitIndices` outcome and `file_big_endian` the collection
/// endianness; the host endianness never leaks in because both branches spell the byte
/// order explicitly, matching the three source branches bit-for-bit. The destination
/// buffer write stays caller-side.
pub fn encode_processed_index_buffer(
    indices: &[u32],
    is_16_bit: bool,
    file_big_endian: bool,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(indices.len() * if is_16_bit { 2 } else { 4 });
    for &index in indices {
        if is_16_bit {
            let narrow = index as u16;
            bytes.extend(if file_big_endian {
                narrow.to_be_bytes()
            } else {
                narrow.to_le_bytes()
            });
        } else {
            bytes.extend(if file_big_endian {
                index.to_be_bytes()
            } else {
                index.to_le_bytes()
            });
        }
    }
    bytes
}

/// `MeshExtensions.CalculateMeshMetric`.
///
/// `uv_set_index` selects both the UV buffer (`UV0`/`UV1`) and the sub-mesh slot, exactly as the
/// two call sites do (`subMeshList[uvSetIndex]`). Returns the `1.0` default for empty buffers or
/// an out-of-range UV-set index, and an explicit error for malformed index data.
#[allow(clippy::too_many_arguments)]
pub fn calculate_mesh_metric(
    sub_mesh: usize,
    vertices: &[Vec3],
    uvs: &[Vec2],
    index_buffer: &[u32],
    sub_meshes: &[SubMeshData],
    uv_set_index: usize,
) -> Result<f32, MeshMetricError> {
    if vertices.is_empty() || uvs.is_empty() || uv_set_index >= sub_meshes.len() {
        return Ok(DEFAULT_MESH_METRIC);
    }
    let record = &sub_meshes[uv_set_index];
    let first = record.first_index.max(0) as usize;
    let count = record.index_count.max(0) as usize;
    let available = index_buffer.len().saturating_sub(first);
    if count > available {
        return Err(MeshMetricError::PartialTrailingGroup {
            sub_mesh,
            topology: record.topology.0,
            trailing: (count - available) as u64,
        });
    }
    let triples = triangle_triples(
        sub_mesh,
        record.topology,
        &index_buffer[first..first + count],
    )?;
    let mut vertex_area_sum = 0.0f32;
    let mut uv_area_sum = 0.0f32;
    let mut n = 0u32;
    for (ia, ib, ic) in triples {
        let (ia, ib, ic) = (ia as usize, ib as usize, ic as usize);
        if ia >= vertices.len() || ib >= vertices.len() || ic >= vertices.len() {
            return Err(MeshMetricError::IndexOutOfRange {
                sub_mesh,
                index: ia.min(ib).min(ic) as u32,
                vertices: vertices.len(),
                uvs: uvs.len(),
            });
        }
        if ia >= uvs.len() || ib >= uvs.len() || ic >= uvs.len() {
            return Err(MeshMetricError::IndexOutOfRange {
                sub_mesh,
                index: ia.min(ib).min(ic) as u32,
                vertices: vertices.len(),
                uvs: uvs.len(),
            });
        }
        let uv_area = triangle_area_v2(uvs[ia], uvs[ib], uvs[ic]);
        if uv_area < UV_AREA_THRESHOLD {
            continue;
        }
        vertex_area_sum += triangle_area_v3(vertices[ia], vertices[ib], vertices[ic]);
        uv_area_sum += uv_area;
        n += 1;
    }
    if n == 0 || uv_area_sum == 0.0 {
        return Ok(DEFAULT_MESH_METRIC);
    }
    Ok(vertex_area_sum / (n as f32) / uv_area_sum)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::numerics::{Vec2, Vec3};

    fn vec3(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3 { x, y, z }
    }

    fn vec2(x: f32, y: f32) -> Vec2 {
        Vec2 { x, y }
    }

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

    /// The reference-runtime differential for `CalculateMeshMetric`.
    ///
    /// `src/resources/mesh_metric_fixture.txt` holds direct invocations of the private static
    /// `MeshExtensions.CalculateMeshMetric` in the shipped extensions assembly (probe
    /// `Temp/opencode/meshread`, expression-tree delegate because `ReadOnlySpan` parameters
    /// cannot be boxed for `MethodInfo.Invoke`). Each case is reconstructed here with the Rust
    /// types and must match the probed word exactly.
    #[test]
    fn metric_matches_the_reference_runtime() {
        let fixture = include_str!("mesh_metric_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            if parts[0] != "MTRCDIRECT" {
                continue;
            }
            assert_eq!(parts.len(), 3, "{line}");
            assert_eq!(parts[0], "MTRCDIRECT", "{line}");
            let expected: f32 = parts[2].parse().expect("a probe float word");
            let unit_triangle = vec![
                vec3(0.0, 0.0, 0.0),
                vec3(1.0, 0.0, 0.0),
                vec3(0.0, 1.0, 0.0),
            ];
            let unit_uv = vec![vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0)];
            let indices = vec![0, 1, 2];
            let one_triangle = vec![sub_mesh(MeshTopology::TRIANGLES, 0, 3)];
            let actual = match parts[1] {
                "square" => {
                    calculate_mesh_metric(0, &unit_triangle, &unit_uv, &indices, &one_triangle, 0)
                        .unwrap()
                }
                "scaled" => {
                    let scaled = vec![vec2(0.0, 0.0), vec2(2.0, 0.0), vec2(0.0, 2.0)];
                    calculate_mesh_metric(0, &unit_triangle, &scaled, &indices, &one_triangle, 0)
                        .unwrap()
                }
                "flat" => {
                    let flat = vec![vec2(0.0, 0.0); 3];
                    calculate_mesh_metric(0, &unit_triangle, &flat, &indices, &one_triangle, 0)
                        .unwrap()
                }
                "empty" => calculate_mesh_metric(0, &[], &[], &[], &one_triangle, 0).unwrap(),
                "strip" => {
                    let vertices = vec![
                        vec3(0.0, 0.0, 0.0),
                        vec3(1.0, 0.0, 0.0),
                        vec3(1.0, 1.0, 0.0),
                        vec3(0.0, 1.0, 0.0),
                    ];
                    let uvs = vec![
                        vec2(0.0, 0.0),
                        vec2(1.0, 0.0),
                        vec2(1.0, 1.0),
                        vec2(0.0, 1.0),
                    ];
                    let strip = vec![sub_mesh(MeshTopology::TRIANGLE_STRIP, 0, 4)];
                    calculate_mesh_metric(0, &vertices, &uvs, &[0, 1, 2, 3], &strip, 0).unwrap()
                }
                other => panic!("unexpected fixture case {other}"),
            };
            assert_eq!(actual, expected, "case {} ({line})", parts[1]);
            rows += 1;
        }
        assert_eq!(rows, 5, "all five probed cases are asserted");
    }

    /// The `FillWithCompressedMeshData` metric stamp is version-gated.
    ///
    /// The `MTRCVERSION` fixture rows show the reference stamping `1`/`1` on `Mesh_2019`
    /// (`Has_MeshMetrics_0_/1_` true) and `0`/`0` on `Mesh_5` (both gates false): a pre-2019
    /// generated class discards the stamp because its TypeTree has no `m_MeshMetrics` fields.
    /// The Rust formula itself is version-independent, matching the reference formula; any
    /// future write-path port must gate the stamp on field presence.
    #[test]
    fn metric_stamp_follows_the_generated_class_gates() {
        let fixture = include_str!("mesh_metric_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            if parts[0] != "MTRCVERSION" {
                continue;
            }
            assert_eq!(parts.len(), 4, "{line}");
            match parts[1] {
                "2019.4.0f1" => assert_eq!((parts[2], parts[3]), ("1", "1"), "{line}"),
                "5.0.0f1" => assert_eq!((parts[2], parts[3]), ("0", "0"), "{line}"),
                other => panic!("unexpected fixture version {other}"),
            }
            rows += 1;
        }
        assert_eq!(rows, 2, "both probed versions are asserted");
    }

    #[test]
    fn unit_square_metric_is_one_half() {
        // Two unit right triangles: vertex area 1, uv area 1, n = 2.
        let vertices = vec![
            vec3(0.0, 0.0, 0.0),
            vec3(1.0, 0.0, 0.0),
            vec3(1.0, 1.0, 0.0),
            vec3(0.0, 1.0, 0.0),
        ];
        let uvs = vec![
            vec2(0.0, 0.0),
            vec2(1.0, 0.0),
            vec2(1.0, 1.0),
            vec2(0.0, 1.0),
        ];
        let indices = vec![0, 1, 2, 0, 2, 3];
        let sub_meshes = vec![sub_mesh(MeshTopology::TRIANGLES, 0, 6)];
        assert_eq!(
            calculate_mesh_metric(0, &vertices, &uvs, &indices, &sub_meshes, 0).unwrap(),
            0.5
        );
    }

    #[test]
    fn degenerate_uv_triangles_restore_the_default() {
        let vertices = vec![
            vec3(0.0, 0.0, 0.0),
            vec3(1.0, 0.0, 0.0),
            vec3(0.0, 1.0, 0.0),
        ];
        let uvs = vec![vec2(0.0, 0.0); 3];
        let indices = vec![0, 1, 2];
        let sub_meshes = vec![sub_mesh(MeshTopology::TRIANGLES, 0, 3)];
        assert_eq!(
            calculate_mesh_metric(0, &vertices, &uvs, &indices, &sub_meshes, 0).unwrap(),
            DEFAULT_MESH_METRIC
        );
    }

    #[test]
    fn empty_inputs_and_missing_uv_sets_yield_the_default() {
        let sub_meshes = vec![sub_mesh(MeshTopology::TRIANGLES, 0, 3)];
        assert_eq!(
            calculate_mesh_metric(0, &[], &[vec2(0.0, 0.0)], &[0], &sub_meshes, 0).unwrap(),
            DEFAULT_MESH_METRIC
        );
        assert_eq!(
            calculate_mesh_metric(0, &[vec3(0.0, 0.0, 0.0)], &[], &[0], &sub_meshes, 0).unwrap(),
            DEFAULT_MESH_METRIC
        );
        assert_eq!(
            calculate_mesh_metric(
                0,
                &[vec3(0.0, 0.0, 0.0)],
                &[vec2(0.0, 0.0)],
                &[0],
                &sub_meshes,
                1
            )
            .unwrap(),
            DEFAULT_MESH_METRIC
        );
    }

    #[test]
    fn strips_and_quads_enumerate_like_the_reference() {
        assert_eq!(
            triangle_triples(0, MeshTopology::TRIANGLE_STRIP, &[0, 1, 2, 3]).unwrap(),
            vec![(0, 1, 2), (2, 1, 3)]
        );
        assert_eq!(
            triangle_triples(0, MeshTopology::QUADS, &[0, 1, 2, 3]).unwrap(),
            vec![(0, 1, 2), (0, 2, 3)]
        );
        assert_eq!(
            triangle_triples(0, MeshTopology::TRIANGLE_STRIP, &[0, 1, 1, 2]).unwrap(),
            vec![],
            "both windows are degenerate"
        );
    }

    #[test]
    fn unsupported_topologies_are_rejected() {
        assert_eq!(
            triangle_triples(2, MeshTopology::LINES, &[0, 1]),
            Err(MeshMetricError::UnsupportedTopology {
                sub_mesh: 2,
                topology: 3
            })
        );
        assert_eq!(
            triangle_triples(0, MeshTopology::POINTS, &[0]),
            Err(MeshMetricError::UnsupportedTopology {
                sub_mesh: 0,
                topology: 5
            })
        );
        assert!(!is_triangle_topology(MeshTopology::LINES));
        assert!(is_triangle_topology(MeshTopology::QUADS));
    }

    #[test]
    fn short_strips_yield_nothing_without_error() {
        assert_eq!(
            triangle_triples(0, MeshTopology::TRIANGLE_STRIP, &[0, 1]).unwrap(),
            vec![]
        );
        assert_eq!(
            triangle_triples(0, MeshTopology::TRIANGLE_STRIP, &[0]).unwrap(),
            vec![]
        );
        assert_eq!(
            triangle_triples(0, MeshTopology::TRIANGLE_STRIP, &[]).unwrap(),
            vec![]
        );
    }

    #[test]
    fn partial_trailing_groups_are_explicit_errors() {
        assert_eq!(
            triangle_triples(0, MeshTopology::TRIANGLES, &[0, 1, 2, 0]),
            Err(MeshMetricError::PartialTrailingGroup {
                sub_mesh: 0,
                topology: 0,
                trailing: 1
            })
        );
        assert_eq!(
            triangle_triples(0, MeshTopology::QUADS, &[0, 1, 2, 3, 0]),
            Err(MeshMetricError::PartialTrailingGroup {
                sub_mesh: 0,
                topology: 2,
                trailing: 1
            })
        );
    }

    #[test]
    fn out_of_range_indices_are_explicit_errors() {
        let vertices = vec![vec3(0.0, 0.0, 0.0)];
        let uvs = vec![vec2(0.0, 0.0)];
        let indices = vec![0, 1, 2];
        let sub_meshes = vec![sub_mesh(MeshTopology::TRIANGLES, 0, 3)];
        assert!(matches!(
            calculate_mesh_metric(0, &vertices, &uvs, &indices, &sub_meshes, 0),
            Err(MeshMetricError::IndexOutOfRange { .. })
        ));
    }

    #[test]
    fn triangle_area_matches_the_cross_product_definition() {
        assert_eq!(
            triangle_area_v3(
                vec3(0.0, 0.0, 0.0),
                vec3(1.0, 0.0, 0.0),
                vec3(0.0, 1.0, 0.0)
            ),
            0.5
        );
        assert_eq!(
            triangle_area_v2(vec2(0.0, 0.0), vec2(2.0, 0.0), vec2(0.0, 2.0)),
            2.0
        );
        assert_eq!(
            triangle_area_v3(
                vec3(1.0, 1.0, 1.0),
                vec3(1.0, 1.0, 1.0),
                vec3(2.0, 2.0, 2.0)
            ),
            0.0
        );
    }

    /// The lane conversions guard their length, widen exactly, and narrow by wrapping.
    #[test]
    fn index_lane_conversions_match_the_source_guards() {
        assert_eq!(ushort_to_uint(&[1, 2, 3], 2), Ok(vec![1, 2]));
        assert_eq!(ushort_to_uint(&[0xFFFF], 1), Ok(vec![0xFFFF]));
        assert_eq!(
            ushort_to_uint(&[1, 2], 3),
            Err(MeshMetricError::IndexBufferTooShort {
                available: 2,
                required: 3
            })
        );
        assert_eq!(uint_to_ushort(&[1, 2, 3], 3), Ok(vec![1, 2, 3]));
        assert_eq!(uint_to_ushort(&[70_000], 1), Ok(vec![4_464]));
        assert_eq!(
            uint_to_ushort(&[1], 2),
            Err(MeshMetricError::IndexBufferTooShort {
                available: 1,
                required: 2
            })
        );
    }

    /// The processed-index assembly spells both widths and both byte orders.
    #[test]
    fn processed_index_encoding_matches_the_source_branches() {
        assert_eq!(
            encode_processed_index_buffer(&[0x1234], true, false),
            vec![0x34, 0x12]
        );
        assert_eq!(
            encode_processed_index_buffer(&[0x1234], true, true),
            vec![0x12, 0x34]
        );
        assert_eq!(
            encode_processed_index_buffer(&[1], false, false),
            vec![1, 0, 0, 0]
        );
        assert_eq!(
            encode_processed_index_buffer(&[1], false, true),
            vec![0, 0, 0, 1]
        );
        assert_eq!(
            encode_processed_index_buffer(&[0xABCD_FFFF], true, false),
            vec![0xFF, 0xFF],
            "16-bit lanes narrow by wrapping like the source cast"
        );
        assert!(encode_processed_index_buffer(&[], true, false).is_empty());
    }
}
