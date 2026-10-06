//! `SubMeshData`, `SubMeshExtensions`, `IndexFormatExtensions`, and `Bounds`.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/SubMeshData.cs`, `SubMeshExtensions.cs`,
//! `IndexFormatExtensions.cs`, and `Bounds.cs`. Sub-mesh records translate between the serialized
//! `FirstByte`/`IsTriStrip` representation and the processed `FirstIndex`/`MeshTopology` view.

use crate::numerics::Vec3;

/// `AssetRipper.SourceGenerated.Enums.IndexFormat`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndexFormat {
    /// 16-bit indices.
    UInt16,
    /// 32-bit indices.
    UInt32,
}

impl IndexFormat {
    /// `IndexFormatExtensions.Size`.
    pub const fn size(self) -> i32 {
        match self {
            Self::UInt16 => 2,
            Self::UInt32 => 4,
        }
    }
}

/// `MeshData.IndexFormat`: 32-bit only once the vertex count exceeds a `ushort`.
pub fn index_format_for_vertex_count(vertex_count: usize) -> IndexFormat {
    if vertex_count > u16::MAX as usize {
        IndexFormat::UInt32
    } else {
        IndexFormat::UInt16
    }
}

/// `AssetRipper.SourceGenerated.Enums.MeshTopology`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MeshTopology(pub i32);

impl MeshTopology {
    /// `Triangles`.
    pub const TRIANGLES: Self = Self(0);
    /// `TriangleStrip`.
    pub const TRIANGLE_STRIP: Self = Self(1);
    /// `Quads`.
    pub const QUADS: Self = Self(2);
    /// `Lines`.
    pub const LINES: Self = Self(3);
    /// `LineStrip`.
    pub const LINE_STRIP: Self = Self(4);
    /// `Points`.
    pub const POINTS: Self = Self(5);

    /// Map an enum member name to its value.
    pub const fn from_name(name: &str) -> Option<Self> {
        Some(match name.as_bytes() {
            b"Triangles" => Self::TRIANGLES,
            b"TriangleStrip" => Self::TRIANGLE_STRIP,
            b"Quads" => Self::QUADS,
            b"Lines" => Self::LINES,
            b"LineStrip" => Self::LINE_STRIP,
            b"Points" => Self::POINTS,
            _ => return None,
        })
    }
}

/// `AssetRipper.SourceGenerated.Extensions.Bounds`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bounds {
    /// Center.
    pub center: Vec3,
    /// Half-extent.
    pub extent: Vec3,
}

impl Bounds {
    /// `Bounds.CalculateFromVertexArray`.
    pub fn calculate_from_vertex_array(vertices: &[Vec3]) -> Self {
        let Some(first) = vertices.first() else {
            return Self::default();
        };
        let mut min_x = first.x;
        let mut min_y = first.y;
        let mut min_z = first.z;
        let mut max_x = first.x;
        let mut max_y = first.y;
        let mut max_z = first.z;
        for vertex in &vertices[1..] {
            if vertex.x < min_x {
                min_x = vertex.x;
            } else if vertex.x > max_x {
                max_x = vertex.x;
            }
            if vertex.y < min_y {
                min_y = vertex.y;
            } else if vertex.y > max_y {
                max_y = vertex.y;
            }
            if vertex.z < min_z {
                min_z = vertex.z;
            } else if vertex.z > max_z {
                max_z = vertex.z;
            }
        }
        Self {
            center: Vec3 {
                x: (max_x + min_x) / 2.0,
                y: (max_y + min_y) / 2.0,
                z: (max_z + min_z) / 2.0,
            },
            extent: Vec3 {
                x: (max_x - min_x) / 2.0,
                y: (max_y - min_y) / 2.0,
                z: (max_z - min_z) / 2.0,
            },
        }
    }
}

/// The serialized sub-mesh fields needed to build or write a [`SubMeshData`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SubMeshView {
    /// `BaseVertex`.
    pub base_vertex: u32,
    /// `FirstByte` (the offset in the index buffer, in bytes).
    pub first_byte: u32,
    /// `FirstVertex`.
    pub first_vertex: u32,
    /// `IndexCount`.
    pub index_count: u32,
    /// `TriangleCount`.
    pub triangle_count: u32,
    /// `VertexCount`.
    pub vertex_count: u32,
    /// `Topology` when the version serializes it.
    pub topology: Option<i32>,
    /// `IsTriStrip` for versions without a topology field.
    pub is_tri_strip: u32,
    /// `LocalAABB`.
    pub local_bounds: Bounds,
}

impl SubMeshView {
    /// `SubMeshExtensions.GetTopology`.
    pub fn get_topology(&self) -> MeshTopology {
        match self.topology {
            Some(value) => MeshTopology(value),
            None => {
                if self.is_tri_strip != 0 {
                    MeshTopology::TRIANGLE_STRIP
                } else {
                    MeshTopology::TRIANGLES
                }
            }
        }
    }

    /// `SubMeshExtensions.SetTopology`.
    pub fn set_topology(&mut self, topology: MeshTopology) {
        match &mut self.topology {
            Some(slot) => *slot = topology.0,
            None => self.is_tri_strip = topology.0 as u32,
        }
    }

    /// `SubMeshExtensions.GetFirstIndex`.
    pub fn get_first_index(&self, is_16_bit_indices: bool) -> u32 {
        if is_16_bit_indices {
            self.first_byte / 2
        } else {
            self.first_byte / 4
        }
    }

    /// `SubMeshExtensions.SetFirstIndex`.
    pub fn set_first_index(&mut self, is_16_bit_indices: bool, first_index: u32) {
        self.first_byte = if is_16_bit_indices {
            first_index.wrapping_mul(2)
        } else {
            first_index.wrapping_mul(4)
        };
    }
}

/// `SubMeshData`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SubMeshData {
    /// `BaseVertex`.
    pub base_vertex: u32,
    /// `FirstIndex` (offset in the index buffer).
    pub first_index: i32,
    /// `FirstVertex`.
    pub first_vertex: i32,
    /// `IndexCount`.
    pub index_count: i32,
    /// `TriangleCount`.
    pub triangle_count: i32,
    /// `VertexCount`.
    pub vertex_count: i32,
    /// `Topology`.
    pub topology: MeshTopology,
    /// `LocalBounds`.
    pub local_bounds: Bounds,
}

impl SubMeshData {
    /// `SubMeshData.Create(ISubMesh, IndexFormat)`.
    pub fn create(sub_mesh: &SubMeshView, index_format: IndexFormat) -> Self {
        Self {
            base_vertex: sub_mesh.base_vertex,
            first_index: sub_mesh.first_byte as i32 / index_format.size(),
            first_vertex: sub_mesh.first_vertex as i32,
            index_count: sub_mesh.index_count as i32,
            triangle_count: sub_mesh.triangle_count as i32,
            vertex_count: sub_mesh.vertex_count as i32,
            topology: sub_mesh.get_topology(),
            local_bounds: sub_mesh.local_bounds,
        }
    }

    /// `SubMeshData.CopyTo`.
    pub fn copy_to(&self, destination: &mut SubMeshView, index_format: IndexFormat) {
        destination.base_vertex = self.base_vertex;
        destination.first_byte = self.first_index.wrapping_mul(index_format.size()) as u32;
        destination.first_vertex = self.first_vertex as u32;
        destination.index_count = self.index_count as u32;
        destination.triangle_count = self.triangle_count as u32;
        destination.vertex_count = self.vertex_count as u32;
        destination.set_topology(self.topology);
        destination.local_bounds = self.local_bounds;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_i32(value: &str) -> i32 {
        value.parse().unwrap()
    }

    fn parse_u32(value: &str) -> u32 {
        value.parse().unwrap()
    }

    fn parse_bounds(parts: &[&str]) -> Bounds {
        Bounds {
            center: Vec3 {
                x: f32::from_bits(parse_u32(parts[0])),
                y: f32::from_bits(parse_u32(parts[1])),
                z: f32::from_bits(parse_u32(parts[2])),
            },
            extent: Vec3 {
                x: f32::from_bits(parse_u32(parts[3])),
                y: f32::from_bits(parse_u32(parts[4])),
                z: f32::from_bits(parse_u32(parts[5])),
            },
        }
    }

    fn bounds_bits(bounds: Bounds) -> [u32; 6] {
        [
            bounds.center.x.to_bits(),
            bounds.center.y.to_bits(),
            bounds.center.z.to_bits(),
            bounds.extent.x.to_bits(),
            bounds.extent.y.to_bits(),
            bounds.extent.z.to_bits(),
        ]
    }

    fn bounds_cases() -> Vec<Vec<Vec3>> {
        vec![
            vec![],
            vec![
                Vec3 {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                Vec3 {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                },
                Vec3 {
                    x: -1.0,
                    y: -2.0,
                    z: -3.0,
                },
                Vec3 {
                    x: 4.0,
                    y: 0.0,
                    z: 0.0,
                },
            ],
            vec![Vec3 {
                x: 2.0,
                y: 2.0,
                z: 2.0,
            }],
            vec![
                Vec3 {
                    x: -5.0,
                    y: 1.0,
                    z: 1.0,
                },
                Vec3 {
                    x: -5.0,
                    y: -1.0,
                    z: 1.0,
                },
            ],
        ]
    }

    /// Byte-for-byte differential against the reference `SubMeshData`/`SubMeshExtensions`/
    /// `Bounds` (probe `Temp/opencode/smprobe`).
    #[test]
    fn sub_mesh_layer_matches_the_reference_runtime() {
        let fixture = include_str!("sub_mesh_data_fixture.txt");
        let cases = bounds_cases();
        let mut sub_rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            match parts[0] {
                "ENUM" => {
                    let topology = MeshTopology::from_name(parts[1]).expect("topology name");
                    assert_eq!(topology.0, parse_i32(parts[2]), "{line}");
                }
                "SETFIRST" => {
                    let mut view = SubMeshView::default();
                    view.set_first_index(true, 7);
                    assert_eq!(view.first_byte, parse_u32(parts[2]), "{line}");
                    let mut view = SubMeshView::default();
                    view.set_first_index(false, 7);
                    assert_eq!(view.first_byte, parse_u32(parts[3]), "{line}");
                }
                "BOUNDS" => {
                    let index: usize = parts[1].parse().unwrap();
                    let bounds = Bounds::calculate_from_vertex_array(&cases[index]);
                    let center: Vec<u32> = parts[2].split(',').map(parse_u32).collect();
                    let extent: Vec<u32> = parts[3].split(',').map(parse_u32).collect();
                    let expected = [
                        center[0], center[1], center[2], extent[0], extent[1], extent[2],
                    ];
                    assert_eq!(bounds_bits(bounds), expected, "{line}");
                }
                "SUB" => {
                    let index_format = match parts[2] {
                        "UInt16" => IndexFormat::UInt16,
                        "UInt32" => IndexFormat::UInt32,
                        other => panic!("unknown index format {other}"),
                    };
                    assert_eq!(index_format.size(), parse_i32(parts[3]), "{line}");
                    let src: Vec<&str> = parts[4].split(',').collect();
                    let view = SubMeshView {
                        base_vertex: parse_u32(src[0]),
                        first_byte: parse_u32(src[1]),
                        first_vertex: parse_u32(src[2]),
                        index_count: parse_u32(src[3]),
                        triangle_count: parse_u32(src[4]),
                        vertex_count: parse_u32(src[5]),
                        topology: Some(parse_i32(src[6])),
                        is_tri_strip: parse_u32(src[7]),
                        local_bounds: parse_bounds(&src[9..15]),
                    };
                    if src[8] == "False" {
                        let view = SubMeshView {
                            topology: None,
                            ..view
                        };
                        assert_sub(line, &view, index_format, parts[5], parts[6], false);
                    } else {
                        assert_sub(line, &view, index_format, parts[5], parts[6], true);
                    }
                }
                other => panic!("unknown fixture row {other}"),
            }
            sub_rows += 1;
        }
        assert_eq!(sub_rows, 28);
    }

    fn assert_sub(
        line: &str,
        view: &SubMeshView,
        index_format: IndexFormat,
        out_csv: &str,
        copy_csv: &str,
        has_topology: bool,
    ) {
        let data = SubMeshData::create(view, index_format);
        let out: Vec<&str> = out_csv.split(',').collect();
        assert_eq!(data.base_vertex, parse_u32(out[0]), "{line}");
        assert_eq!(data.first_index, parse_i32(out[1]), "{line}");
        assert_eq!(data.first_vertex, parse_i32(out[2]), "{line}");
        assert_eq!(data.index_count, parse_i32(out[3]), "{line}");
        assert_eq!(data.triangle_count, parse_i32(out[4]), "{line}");
        assert_eq!(data.vertex_count, parse_i32(out[5]), "{line}");
        assert_eq!(data.topology.0, parse_i32(out[6]), "{line}");
        assert_eq!(
            bounds_bits(data.local_bounds),
            bounds_bits(parse_bounds(&out[7..13])),
            "{line}"
        );

        let mut destination = SubMeshView {
            topology: if has_topology { Some(0) } else { None },
            ..SubMeshView::default()
        };
        data.copy_to(&mut destination, index_format);
        let copy: Vec<&str> = copy_csv.split(',').collect();
        assert_eq!(destination.first_byte, parse_u32(copy[0]), "{line}");
        assert_eq!(destination.base_vertex, parse_u32(copy[1]), "{line}");
        assert_eq!(destination.first_vertex, parse_u32(copy[2]), "{line}");
        assert_eq!(destination.index_count, parse_u32(copy[3]), "{line}");
        assert_eq!(destination.triangle_count, parse_u32(copy[4]), "{line}");
        assert_eq!(destination.vertex_count, parse_u32(copy[5]), "{line}");
        assert_eq!(destination.get_topology().0, parse_i32(copy[6]), "{line}");
        assert_eq!(destination.is_tri_strip, parse_u32(copy[7]), "{line}");
        assert_eq!(
            bounds_bits(destination.local_bounds),
            bounds_bits(parse_bounds(&copy[8..14])),
            "{line}"
        );
    }

    #[test]
    fn index_format_threshold_matches_mesh_data() {
        assert_eq!(index_format_for_vertex_count(0), IndexFormat::UInt16);
        assert_eq!(
            index_format_for_vertex_count(u16::MAX as usize),
            IndexFormat::UInt16
        );
        assert_eq!(
            index_format_for_vertex_count(u16::MAX as usize + 1),
            IndexFormat::UInt32
        );
    }

    #[test]
    fn index_math_edges_match_unchecked_source_semantics() {
        // Unknown topology names decode as absent.
        assert_eq!(MeshTopology::from_name("Triangle"), None);
        assert_eq!(MeshTopology::from_name(""), None);
        // `GetFirstIndex` truncates like the source unsigned division.
        let view = SubMeshView {
            first_byte: 7,
            ..SubMeshView::default()
        };
        assert_eq!(view.get_first_index(true), 3);
        assert_eq!(view.get_first_index(false), 1);
        // The source multiplies in an unchecked context, so full-range products wrap
        // instead of saturating or failing; `wrapping_mul` preserves that exactly.
        let mut view = SubMeshView::default();
        view.set_first_index(true, u32::MAX);
        assert_eq!(view.first_byte, u32::MAX.wrapping_mul(2));
        let data = SubMeshData {
            base_vertex: 0,
            first_index: -1,
            first_vertex: 0,
            index_count: 0,
            triangle_count: 0,
            vertex_count: 0,
            topology: MeshTopology::TRIANGLES,
            local_bounds: Bounds::default(),
        };
        let mut destination = SubMeshView::default();
        data.copy_to(&mut destination, IndexFormat::UInt16);
        assert_eq!(destination.first_byte, (-1i32).wrapping_mul(2) as u32);
    }
}
