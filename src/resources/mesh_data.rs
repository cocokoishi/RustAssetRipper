//! `MeshData` ported from `AssetRipper.SourceGenerated.Extensions/MeshData.cs`.
//!
//! Combines the vertex-data-blob and compressed-mesh decode paths (with the source's `??=`
//! fallbacks) into the processed mesh view the model exporters consume: vertices, normals,
//! tangents, colors, eight UV channels, skin, bind poses, a `u32` index buffer, and sub-meshes.
//!
//! The index accessors apply the source's per-channel post-processing: UVs flip Y, tangents snap
//! their `W` to `±1`, and skin weights are defaulted or renormalized. This module only merges
//! already-decoded channels; the field reading that produces the blob and the compressed mesh
//! stays with the caller.

use crate::numerics::{BoneWeight4, ColorFloat, Vec2, Vec3, Vec4};

use super::compressed_mesh::{DecompressedMesh, Vec2 as CompressedVec2, Vec3 as CompressedVec3};
use super::sub_mesh_data::{index_format_for_vertex_count, IndexFormat, SubMeshData};
use super::vertex_data_blob::MeshData as BlobMeshData;

/// `MeshData`: the processed mesh view.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshData {
    /// Vertices.
    pub vertices: Vec<Vec3>,
    /// Normals.
    pub normals: Option<Vec<Vec3>>,
    /// Tangents.
    pub tangents: Option<Vec<Vec4>>,
    /// Vertex colors.
    pub colors: Option<Vec<ColorFloat>>,
    /// UV0.
    pub uv0: Option<Vec<Vec2>>,
    /// UV1.
    pub uv1: Option<Vec<Vec2>>,
    /// UV2.
    pub uv2: Option<Vec<Vec2>>,
    /// UV3.
    pub uv3: Option<Vec<Vec2>>,
    /// UV4.
    pub uv4: Option<Vec<Vec2>>,
    /// UV5.
    pub uv5: Option<Vec<Vec2>>,
    /// UV6.
    pub uv6: Option<Vec<Vec2>>,
    /// UV7.
    pub uv7: Option<Vec<Vec2>>,
    /// Bone weights and indices.
    pub skin: Option<Vec<BoneWeight4>>,
    /// Bind poses.
    pub bind_pose: Option<Vec<[[f32; 4]; 4]>>,
    /// The processed `u32` index buffer.
    pub processed_index_buffer: Vec<u32>,
    /// Processed sub-meshes.
    pub sub_meshes: Vec<SubMeshData>,
}

/// The already-decoded inputs `try_make_from_parts` merges.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshParts {
    /// The `VertexDataBlob.ReadData` output, when the mesh has vertex data.
    pub blob: Option<BlobMeshData>,
    /// The `CompressedMesh.Decompress` output.
    pub compressed: DecompressedMesh,
    /// `mesh.BindPose`, used when the compressed mesh has no bind poses.
    pub mesh_bind_pose: Option<Vec<[[f32; 4]; 4]>>,
    /// `mesh.GetProcessedIndexBuffer()`, used when the compressed mesh has no triangles.
    pub mesh_index_buffer: Vec<u32>,
    /// `mesh.Skin`, used when neither the blob nor the compressed mesh has skin.
    pub mesh_skin: Option<Vec<BoneWeight4>>,
    /// `SubMeshData.Create(mesh)`.
    pub sub_meshes: Vec<SubMeshData>,
}

impl MeshData {
    /// `MeshData.TryMakeFromMesh` after both decode paths have run.
    pub fn try_make_from_parts(parts: MeshParts) -> Option<Self> {
        let MeshParts {
            blob,
            compressed,
            mesh_bind_pose,
            mesh_index_buffer,
            mesh_skin,
            sub_meshes,
        } = parts;

        let vertices: Vec<Vec3> = match blob
            .as_ref()
            .map(|data| data.vertices.as_slice())
            .filter(|vertices| !vertices.is_empty())
        {
            Some(vertices) => vertices.to_vec(),
            None => compressed
                .vertices
                .as_ref()?
                .iter()
                .map(|value| vec3(value.x, value.y, value.z))
                .collect(),
        };

        let normals = blob
            .as_ref()
            .and_then(|data| data.normals.clone())
            .or_else(|| {
                compressed
                    .normals
                    .as_ref()
                    .map(|values| convert_vec3s(values))
            });
        let tangents = blob
            .as_ref()
            .and_then(|data| data.tangents.clone())
            .or_else(|| {
                compressed
                    .tangents
                    .as_ref()
                    .map(|values| convert_vec4s(values))
            });
        let colors = blob
            .as_ref()
            .and_then(|data| data.colors.clone())
            .or_else(|| compressed.colors.clone());

        let blob_uvs = |index: usize| blob.as_ref().and_then(|data| data.uv(index).cloned());
        let compressed_uv = |index: usize| {
            compressed.uvs[index]
                .as_ref()
                .map(|values| values.iter().copied().map(convert_vec2).collect::<Vec<_>>())
        };
        let uv0 = blob_uvs(0).or_else(|| compressed_uv(0));
        let uv1 = blob_uvs(1).or_else(|| compressed_uv(1));
        let uv2 = blob_uvs(2).or_else(|| compressed_uv(2));
        let uv3 = blob_uvs(3).or_else(|| compressed_uv(3));
        let uv4 = blob_uvs(4).or_else(|| compressed_uv(4));
        let uv5 = blob_uvs(5).or_else(|| compressed_uv(5));
        let uv6 = blob_uvs(6).or_else(|| compressed_uv(6));
        let uv7 = blob_uvs(7).or_else(|| compressed_uv(7));

        let skin = blob
            .as_ref()
            .and_then(|data| data.skin.clone())
            .or_else(|| compressed.skin.clone())
            .or(mesh_skin);
        let bind_pose = compressed.bind_pose.clone().or(mesh_bind_pose);
        let processed_index_buffer = compressed
            .processed_index_buffer
            .clone()
            .unwrap_or(mesh_index_buffer);

        Some(Self {
            vertices,
            normals,
            tangents,
            colors,
            uv0,
            uv1,
            uv2,
            uv3,
            uv4,
            uv5,
            uv6,
            uv7,
            skin,
            bind_pose,
            processed_index_buffer,
            sub_meshes,
        })
    }

    /// `MeshData.Empty`: no channels, no indices, no sub-meshes.
    pub fn empty() -> Self {
        Self {
            vertices: Vec::new(),
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
            processed_index_buffer: Vec::new(),
            sub_meshes: Vec::new(),
        }
    }

    /// `MeshData.HasNormals`.
    pub fn has_normals(&self) -> bool {
        self.normals
            .as_ref()
            .is_some_and(|values| values.len() == self.vertices.len())
    }

    /// `MeshData.HasTangents`.
    pub fn has_tangents(&self) -> bool {
        self.tangents
            .as_ref()
            .is_some_and(|values| values.len() == self.vertices.len())
    }

    /// `MeshData.HasColors`.
    pub fn has_colors(&self) -> bool {
        self.colors
            .as_ref()
            .is_some_and(|values| values.len() == self.vertices.len())
    }

    /// `MeshData.HasSkin`.
    pub fn has_skin(&self) -> bool {
        self.skin
            .as_ref()
            .is_some_and(|values| values.len() == self.vertices.len())
    }

    fn channel_has(&self, index: usize) -> bool {
        self.uv(index)
            .is_some_and(|values| values.len() == self.vertices.len())
    }

    /// `MeshData.HasUV0`.
    pub fn has_uv0(&self) -> bool {
        self.channel_has(0)
    }

    /// `MeshData.HasUV1`.
    pub fn has_uv1(&self) -> bool {
        self.channel_has(1)
    }

    /// `MeshData.HasUV2`.
    pub fn has_uv2(&self) -> bool {
        self.channel_has(2)
    }

    /// `MeshData.HasUV3`.
    pub fn has_uv3(&self) -> bool {
        self.channel_has(3)
    }

    /// `MeshData.HasUV4`.
    pub fn has_uv4(&self) -> bool {
        self.channel_has(4)
    }

    /// `MeshData.HasUV5`.
    pub fn has_uv5(&self) -> bool {
        self.channel_has(5)
    }

    /// `MeshData.HasUV6`.
    pub fn has_uv6(&self) -> bool {
        self.channel_has(6)
    }

    /// `MeshData.HasUV7`.
    pub fn has_uv7(&self) -> bool {
        self.channel_has(7)
    }

    fn uv(&self, index: usize) -> Option<&Vec<Vec2>> {
        match index {
            0 => self.uv0.as_ref(),
            1 => self.uv1.as_ref(),
            2 => self.uv2.as_ref(),
            3 => self.uv3.as_ref(),
            4 => self.uv4.as_ref(),
            5 => self.uv5.as_ref(),
            6 => self.uv6.as_ref(),
            7 => self.uv7.as_ref(),
            _ => None,
        }
    }

    /// `MeshData.UVCount`.
    pub fn uv_count(&self) -> usize {
        let mut count = 0;
        for index in 0..8 {
            match self.uv(index) {
                Some(values) if values.len() == self.vertices.len() => count = index + 1,
                _ => break,
            }
        }
        count
    }

    /// `MeshData.IndexFormat`.
    pub fn index_format(&self) -> IndexFormat {
        index_format_for_vertex_count(self.vertices.len())
    }

    /// `MeshData.TryGetVertexAtIndex`.
    pub fn try_get_vertex_at_index(&self, index: u32) -> Vec3 {
        self.vertices[index as usize]
    }

    /// `MeshData.TryGetNormalAtIndex`.
    pub fn try_get_normal_at_index(&self, index: u32) -> Vec3 {
        self.normals
            .as_ref()
            .filter(|values| !values.is_empty())
            .map_or_else(Vec3::default, |values| values[index as usize])
    }

    /// `MeshData.TryGetTangentAtIndex`: snap `W` to `±1` unless it already is.
    pub fn try_get_tangent_at_index(&self, index: u32) -> Vec4 {
        let value = self
            .tangents
            .as_ref()
            .filter(|values| !values.is_empty())
            .map_or_else(Vec4::default, |values| values[index as usize]);
        if value.w == -1.0 || value.w == 1.0 {
            value
        } else if value.w < 0.0 {
            Vec4 { w: -1.0, ..value }
        } else {
            Vec4 { w: 1.0, ..value }
        }
    }

    /// `MeshData.TryGetColorAtIndex`.
    pub fn try_get_color_at_index(&self, index: u32) -> ColorFloat {
        self.colors
            .as_ref()
            .filter(|values| !values.is_empty())
            .map_or_else(ColorFloat::default, |values| values[index as usize])
    }

    fn try_get_uv_at_index(&self, channel: usize, index: u32) -> Vec2 {
        let value = self
            .uv(channel)
            .filter(|values| !values.is_empty())
            .map_or_else(Vec2::default, |values| values[index as usize]);
        Vec2 {
            y: 1.0 - value.y,
            ..value
        }
    }

    /// `MeshData.TryGetUV0AtIndex`.
    pub fn try_get_uv0_at_index(&self, index: u32) -> Vec2 {
        self.try_get_uv_at_index(0, index)
    }

    /// `MeshData.TryGetUV1AtIndex`.
    pub fn try_get_uv1_at_index(&self, index: u32) -> Vec2 {
        self.try_get_uv_at_index(1, index)
    }

    /// `MeshData.TryGetUV2AtIndex`.
    pub fn try_get_uv2_at_index(&self, index: u32) -> Vec2 {
        self.try_get_uv_at_index(2, index)
    }

    /// `MeshData.TryGetUV3AtIndex`.
    pub fn try_get_uv3_at_index(&self, index: u32) -> Vec2 {
        self.try_get_uv_at_index(3, index)
    }

    /// `MeshData.TryGetUV4AtIndex`.
    pub fn try_get_uv4_at_index(&self, index: u32) -> Vec2 {
        self.try_get_uv_at_index(4, index)
    }

    /// `MeshData.TryGetUV5AtIndex`.
    pub fn try_get_uv5_at_index(&self, index: u32) -> Vec2 {
        self.try_get_uv_at_index(5, index)
    }

    /// `MeshData.TryGetUV6AtIndex`.
    pub fn try_get_uv6_at_index(&self, index: u32) -> Vec2 {
        self.try_get_uv_at_index(6, index)
    }

    /// `MeshData.TryGetUV7AtIndex`.
    pub fn try_get_uv7_at_index(&self, index: u32) -> Vec2 {
        self.try_get_uv_at_index(7, index)
    }

    /// `MeshData.TryGetSkinAtIndex`: invalid weights become uniform; unnormalized weights scale.
    pub fn try_get_skin_at_index(&self, index: u32) -> BoneWeight4 {
        let value = self
            .skin
            .as_ref()
            .filter(|values| !values.is_empty())
            .map_or_else(BoneWeight4::default, |values| values[index as usize]);
        if value == BoneWeight4::default() || value.any_weights_negative() {
            BoneWeight4::new(0.25, 0.25, 0.25, 0.25, 0, 0, 0, 0)
        } else if !value.is_normalized() {
            value.normalize_weights()
        } else {
            value
        }
    }
}

fn vec3(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3 { x, y, z }
}

fn convert_vec2(value: CompressedVec2) -> Vec2 {
    Vec2 {
        x: value.x,
        y: value.y,
    }
}

fn convert_vec3(value: CompressedVec3) -> Vec3 {
    Vec3 {
        x: value.x,
        y: value.y,
        z: value.z,
    }
}

fn convert_vec3s(values: &[CompressedVec3]) -> Vec<Vec3> {
    values.iter().copied().map(convert_vec3).collect()
}

fn convert_vec4s(values: &[super::compressed_mesh::Vec4]) -> Vec<Vec4> {
    values
        .iter()
        .map(|value| Vec4 {
            x: value.x,
            y: value.y,
            z: value.z,
            w: value.w,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::compressed_mesh::{decompress, CompressedMesh};
    use crate::resources::packed_bit_vector::PackedBitVectorLimits;
    use crate::resources::sub_mesh_data::MeshTopology;

    /// The five vertex-channel lists the reference probe fed into `FillWithCompressedMeshData`.
    type TriangleInputs = (Vec<Vec3>, Vec<Vec3>, Vec<Vec4>, Vec<ColorFloat>, Vec<Vec2>);

    /// The `MeshData.CreateTriangleMesh` inputs the reference probe used.
    fn triangle_inputs() -> TriangleInputs {
        (
            vec![
                vec3(0.0, 0.0, 0.0),
                vec3(1.0, 0.0, 0.0),
                vec3(0.0, 1.0, 0.0),
            ],
            vec![
                vec3(0.0, 0.0, 1.0),
                vec3(0.0, 0.0, 1.0),
                vec3(0.0, 0.0, 1.0),
            ],
            vec![
                Vec4::new(1.0, 0.0, 0.0, 1.0),
                Vec4::new(1.0, 0.0, 0.0, 1.0),
                Vec4::new(1.0, 0.0, 0.0, 1.0),
            ],
            vec![
                ColorFloat::new(1.0, 0.0, 0.0, 1.0),
                ColorFloat::new(0.0, 1.0, 0.0, 1.0),
                ColorFloat::new(0.0, 0.0, 1.0, 1.0),
            ],
            vec![
                Vec2 { x: 0.0, y: 0.0 },
                Vec2 { x: 1.0, y: 0.0 },
                Vec2 { x: 0.0, y: 1.0 },
            ],
        )
    }

    /// Builds the compressed mesh the reference `FillWithCompressedMeshData` produced for a Unity
    /// 5+ mesh.
    fn filled_compressed_mesh() -> CompressedMesh {
        let (vertices, normals, tangents, colors, uv) = triangle_inputs();
        let limits = PackedBitVectorLimits::default();
        let mut mesh = CompressedMesh {
            has_uv_info: true,
            has_float_colors: true,
            has_colors: true,
            has_bind_poses: false,
            ..CompressedMesh::default()
        };
        let vertices: Vec<CompressedVec3> = vertices
            .iter()
            .map(|value| CompressedVec3::new(value.x, value.y, value.z))
            .collect();
        let normals: Vec<CompressedVec3> = normals
            .iter()
            .map(|value| CompressedVec3::new(value.x, value.y, value.z))
            .collect();
        let tangents: Vec<crate::resources::compressed_mesh::Vec4> = tangents
            .iter()
            .map(|value| {
                crate::resources::compressed_mesh::Vec4::new(value.x, value.y, value.z, value.w)
            })
            .collect();
        let uv: Vec<CompressedVec2> = uv
            .iter()
            .map(|value| CompressedVec2::new(value.x, value.y))
            .collect();
        mesh.set_vertices(&vertices, limits).unwrap();
        mesh.set_normals(&normals, limits).unwrap();
        mesh.set_tangents(&tangents, limits).unwrap();
        mesh.set_float_colors(&colors, limits).unwrap();
        mesh.set_uv([&uv[..], &uv[..], &[], &[], &[], &[], &[], &[]], limits)
            .unwrap();
        mesh.set_triangles(&[0, 1, 2], limits).unwrap();
        mesh
    }

    /// The reference-runtime differential for the class-43 read path.
    ///
    /// `src/resources/mesh_read_fixture.txt` is the output of probe `Temp/opencode/meshread`, which
    /// built real generated `Mesh_5`/`Mesh_2017`/`Mesh_2019`/`Mesh_2022` objects, filled each with
    /// the reference `MeshData.CreateTriangleMesh` values through
    /// `MeshExtensions.FillWithCompressedMeshData`, and called `ReadData` and `TryMakeFromMesh`.
    /// All four versions produced identical rows, so this test reconstructs the same inputs with
    /// the Rust compressed-mesh packers and asserts the reference values.
    #[test]
    fn compressed_mesh_read_path_matches_the_reference_runtime() {
        let fixture = include_str!("mesh_read_fixture.txt");
        let mut versions = 0;
        let mut current_version = String::new();
        let mut saw_read = false;
        let mut saw_data = false;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert!(parts.len() >= 3, "{line}");
            let kind = parts[0];
            if kind == "READ" {
                current_version = parts[1].to_owned();
                versions += 1;
                assert_eq!(parts[2], "IsSet=True", "{line}");
                assert_eq!(parts[3], "HasAnyVertices=True", "{line}");
                assert_eq!(parts[4], "IsCombined=False", "{line}");
                assert_eq!(parts[5], "IndexFormat=UInt16", "{line}");
                assert_eq!(parts[6], "Is16=True", "{line}");
                saw_read = true;
                continue;
            }
            assert!(!current_version.is_empty(), "{line}");
            match kind {
                "VERT" => assert_eq!(
                    parse_vec3(parts[2]),
                    vec![
                        vec3(0.0, 0.0, 0.0),
                        vec3(1.0, 0.0, 0.0),
                        vec3(0.0, 1.0, 0.0)
                    ],
                    "{line}"
                ),
                "NORM" => assert_eq!(
                    parse_vec3(parts[2]),
                    vec![vec3(0.0, 0.0, 1.0); 3],
                    "{line}"
                ),
                "TANG" => assert_eq!(
                    parse_vec4(parts[2]),
                    vec![Vec4::new(1.0, 0.0, 0.0, 1.0); 3],
                    "{line}"
                ),
                "COLO" => assert_eq!(
                    parse_colors(parts[2]),
                    vec![
                        ColorFloat::new(1.0, 0.0, 0.0, 1.0),
                        ColorFloat::new(0.0, 1.0, 0.0, 1.0),
                        ColorFloat::new(0.0, 0.0, 1.0, 1.0),
                    ],
                    "{line}"
                ),
                "UV0" => assert_eq!(
                    parse_vec2(parts[2]),
                    vec![
                        Vec2 { x: 0.0, y: 0.0 },
                        Vec2 { x: 1.0, y: 0.0 },
                        Vec2 { x: 0.0, y: 1.0 }
                    ],
                    "{line}"
                ),
                "UV7" => assert_eq!(parts[2], "null", "the reference has no UV7: {line}"),
                "SKIN" | "BIND" => {
                    assert_eq!(parts[2], "[]", "the reference returns an empty array: {line}")
                }
                "INDX" => assert_eq!(parts[2], "[0,1,2]", "{line}"),
                "DATA" => {
                    saw_data = true;
                    assert_eq!(
                        parts[2..].join("|"),
                        "HasNormals=True|HasTangents=True|HasColors=True|HasSkin=False|UVCount=2|IndexFormat=UInt16",
                        "{line}"
                    );
                }
                "DATASUB" => assert_eq!(
                    parts[2..].join("|"),
                    "Count=1|BaseVertex=0|FirstIndex=0|IndexCount=3|TriangleCount=1|VertexCount=3|Topology=Triangles",
                    "{line}"
                ),
                _ => panic!("unexpected fixture row {line}"),
            }
        }
        assert!(saw_read, "the fixture has READ rows");
        assert!(saw_data, "the fixture has DATA rows");
        assert_eq!(versions, 4, "four Unity versions were probed");

        // Now reconstruct the same inputs and check the Rust produces the reference values.
        let limits = PackedBitVectorLimits::default();
        let compressed = filled_compressed_mesh();
        let decompressed = decompress(&compressed, limits).unwrap();
        let (vertices, normals, tangents, colors, uv) = triangle_inputs();
        let decompressed_vertices: Vec<Vec3> = decompressed
            .vertices
            .as_ref()
            .unwrap()
            .iter()
            .map(|value| convert_vec3(*value))
            .collect();
        assert_eq!(decompressed_vertices, vertices);
        let decompressed_normals: Vec<Vec3> = decompressed
            .normals
            .as_ref()
            .unwrap()
            .iter()
            .map(|value| convert_vec3(*value))
            .collect();
        assert_eq!(decompressed_normals, normals);
        assert_eq!(
            decompressed.tangents.as_ref().unwrap().len(),
            tangents.len()
        );
        assert_eq!(decompressed.colors.as_deref(), Some(colors.as_slice()));
        let decompressed_uv0: Vec<Vec2> = decompressed.uvs[0]
            .as_ref()
            .unwrap()
            .iter()
            .map(|value| convert_vec2(*value))
            .collect();
        assert_eq!(decompressed_uv0, uv);
        let decompressed_uv1: Vec<Vec2> = decompressed.uvs[1]
            .as_ref()
            .unwrap()
            .iter()
            .map(|value| convert_vec2(*value))
            .collect();
        assert_eq!(decompressed_uv1, uv);
        assert_eq!(decompressed.uvs[7], None, "the reference has no UV7");
        assert_eq!(decompressed.skin, None);
        assert_eq!(decompressed.bind_pose, None);
        assert_eq!(
            decompressed.processed_index_buffer.as_deref(),
            Some([0u32, 1, 2].as_slice())
        );

        let sub_mesh = SubMeshData {
            base_vertex: 0,
            first_index: 0,
            first_vertex: 0,
            index_count: 3,
            triangle_count: 1,
            vertex_count: 3,
            topology: MeshTopology::TRIANGLES,
            local_bounds: Default::default(),
        };
        let data = MeshData::try_make_from_parts(MeshParts {
            blob: None,
            compressed: decompressed,
            mesh_bind_pose: None,
            mesh_index_buffer: Vec::new(),
            mesh_skin: None,
            sub_meshes: vec![sub_mesh],
        })
        .expect("the reference reports TryMakeFromMesh == True");
        assert!(data.has_normals());
        assert!(data.has_tangents());
        assert!(data.has_colors());
        assert!(!data.has_skin());
        assert_eq!(data.uv_count(), 2);
        assert_eq!(data.index_format(), IndexFormat::UInt16);
        assert_eq!(data.vertices, vertices);
        assert_eq!(data.uv0.as_deref(), Some(uv.as_slice()));
        assert_eq!(data.uv1.as_deref(), Some(uv.as_slice()));
        assert_eq!(data.uv7, None);
        assert_eq!(data.processed_index_buffer, vec![0, 1, 2]);
        assert_eq!(data.sub_meshes.len(), 1);
        assert_eq!(data.sub_meshes[0].first_index, 0);
        assert_eq!(data.sub_meshes[0].index_count, 3);
        assert_eq!(data.sub_meshes[0].triangle_count, 1);
        assert_eq!(data.sub_meshes[0].vertex_count, 3);
        assert_eq!(data.sub_meshes[0].topology, MeshTopology::TRIANGLES);
    }

    /// Parses a reference `<x, y, z>` row.
    fn parse_vec3(text: &str) -> Vec<Vec3> {
        parse_tuple_list(text, 3)
            .into_iter()
            .map(|values| vec3(values[0], values[1], values[2]))
            .collect()
    }

    /// Parses a reference `<x, y, z, w>` row.
    fn parse_vec4(text: &str) -> Vec<Vec4> {
        parse_tuple_list(text, 4)
            .into_iter()
            .map(|values| Vec4::new(values[0], values[1], values[2], values[3]))
            .collect()
    }

    /// Parses a reference `<x, y>` row.
    fn parse_vec2(text: &str) -> Vec<Vec2> {
        parse_tuple_list(text, 2)
            .into_iter()
            .map(|values| Vec2 {
                x: values[0],
                y: values[1],
            })
            .collect()
    }

    /// Parses a reference `[R:r G:g B:b A:a]` row.
    fn parse_colors(text: &str) -> Vec<ColorFloat> {
        let inner = text
            .trim_start_matches('[')
            .trim_end_matches(']')
            .trim_matches(|character| character == '[' || character == ']');
        inner
            .split("],[")
            .map(|entry| {
                let mut values = [0.0f32; 4];
                for token in entry.split_whitespace() {
                    let (name, value) = token.split_once(':').expect("a `Name:value` token");
                    let slot = match name {
                        "R" => 0,
                        "G" => 1,
                        "B" => 2,
                        "A" => 3,
                        other => panic!("unexpected colour channel {other}"),
                    };
                    values[slot] = value.parse().expect("a colour component");
                }
                ColorFloat::new(values[0], values[1], values[2], values[3])
            })
            .collect()
    }

    /// Parses `[<a, b>,<c, d>]` into a list of component vectors.
    fn parse_tuple_list(text: &str, components: usize) -> Vec<Vec<f32>> {
        let inner = text.trim_start_matches('[').trim_end_matches(']');
        inner
            .split(">,<")
            .map(|entry| {
                let entry = entry
                    .trim_start_matches('<')
                    .trim_end_matches('>')
                    .trim_matches(|character| character == '<' || character == '>');
                let values: Vec<f32> = entry
                    .split(',')
                    .map(|value| value.trim().parse().expect("a numeric component"))
                    .collect();
                assert_eq!(values.len(), components, "row '{text}' component count");
                values
            })
            .collect()
    }

    fn empty_parts() -> MeshParts {
        MeshParts {
            blob: None,
            compressed: DecompressedMesh::default(),
            mesh_bind_pose: None,
            mesh_index_buffer: Vec::new(),
            mesh_skin: None,
            sub_meshes: Vec::new(),
        }
    }

    #[test]
    fn no_vertices_means_no_mesh_data() {
        assert!(MeshData::try_make_from_parts(empty_parts()).is_none());
    }

    #[test]
    fn compressed_vertices_are_used_when_the_blob_is_empty() {
        let compressed = DecompressedMesh {
            vertices: Some(vec![
                CompressedVec3::new(1.0, 2.0, 3.0),
                CompressedVec3::new(4.0, 5.0, 6.0),
            ]),
            ..DecompressedMesh::default()
        };
        let parts = MeshParts {
            compressed,
            ..empty_parts()
        };
        let data = MeshData::try_make_from_parts(parts).unwrap();
        assert_eq!(
            data.vertices,
            vec![vec3(1.0, 2.0, 3.0), vec3(4.0, 5.0, 6.0)]
        );
        assert_eq!(data.index_format(), IndexFormat::UInt16);
    }

    #[test]
    fn blob_vertices_win_over_compressed() {
        let blob = BlobMeshData {
            vertices: vec![vec3(9.0, 9.0, 9.0)],
            normals: Some(vec![vec3(0.0, 0.0, 1.0)]),
            ..BlobMeshData::default()
        };
        let compressed = DecompressedMesh {
            vertices: Some(vec![CompressedVec3::new(1.0, 1.0, 1.0)]),
            ..DecompressedMesh::default()
        };
        let parts = MeshParts {
            blob: Some(blob),
            compressed,
            ..empty_parts()
        };
        let data = MeshData::try_make_from_parts(parts).unwrap();
        assert_eq!(data.vertices, vec![vec3(9.0, 9.0, 9.0)]);
        assert!(data.has_normals());
    }

    #[test]
    fn tangent_w_is_snapped_to_the_sign() {
        let compressed = DecompressedMesh {
            vertices: Some(vec![CompressedVec3::new(0.0, 0.0, 0.0)]),
            tangents: Some(vec![
                super::super::compressed_mesh::Vec4::new(1.0, 0.0, 0.0, 0.3),
                super::super::compressed_mesh::Vec4::new(0.0, 1.0, 0.0, -0.2),
                super::super::compressed_mesh::Vec4::new(0.0, 0.0, 1.0, 1.0),
            ]),
            ..DecompressedMesh::default()
        };
        let parts = MeshParts {
            compressed,
            ..empty_parts()
        };
        let data = MeshData::try_make_from_parts(parts).unwrap();
        // Only one vertex, so the accessor clamps to the first tangent.
        assert_eq!(data.try_get_tangent_at_index(0).w, 1.0);
    }

    #[test]
    fn uv_accessor_flips_y() {
        let mut uvs: [Option<Vec<CompressedVec2>>; 8] = Default::default();
        uvs[0] = Some(vec![CompressedVec2::new(0.25, 0.75)]);
        let compressed = DecompressedMesh {
            vertices: Some(vec![CompressedVec3::new(0.0, 0.0, 0.0)]),
            uvs,
            ..DecompressedMesh::default()
        };
        let parts = MeshParts {
            compressed,
            ..empty_parts()
        };
        let data = MeshData::try_make_from_parts(parts).unwrap();
        let uv = data.try_get_uv0_at_index(0);
        assert_eq!(uv.x, 0.25);
        assert_eq!(uv.y, 0.25);
        assert_eq!(data.uv_count(), 1);
    }

    #[test]
    fn invalid_skin_weights_become_uniform() {
        let compressed = DecompressedMesh {
            vertices: Some(vec![CompressedVec3::new(0.0, 0.0, 0.0)]),
            skin: Some(vec![BoneWeight4::new(0.0, 0.0, 0.0, 0.0, 0, 0, 0, 0)]),
            ..DecompressedMesh::default()
        };
        let parts = MeshParts {
            compressed,
            ..empty_parts()
        };
        let data = MeshData::try_make_from_parts(parts).unwrap();
        let skin = data.try_get_skin_at_index(0);
        assert_eq!(skin.weights, [0.25, 0.25, 0.25, 0.25]);
    }

    #[test]
    fn compressed_index_buffer_wins_over_the_mesh_fallback() {
        let compressed = DecompressedMesh {
            vertices: Some(vec![CompressedVec3::new(0.0, 0.0, 0.0)]),
            processed_index_buffer: Some(vec![0, 1, 2]),
            ..DecompressedMesh::default()
        };
        let parts = MeshParts {
            compressed,
            mesh_index_buffer: vec![9, 9, 9],
            ..empty_parts()
        };
        let data = MeshData::try_make_from_parts(parts).unwrap();
        assert_eq!(data.processed_index_buffer, vec![0, 1, 2]);
    }

    #[test]
    fn empty_has_no_channels_and_uv_presence_matches_lengths() {
        let empty = MeshData::empty();
        assert!(empty.vertices.is_empty());
        assert!(!empty.has_normals());
        assert!(!empty.has_tangents());
        assert!(!empty.has_colors());
        assert!(!empty.has_skin());
        assert_eq!(empty.uv_count(), 0);
        assert!(!empty.has_uv0());
        assert!(!empty.has_uv7());

        // A channel counts only when present with exactly one entry per vertex.
        let one = vec![Vec2 { x: 0.0, y: 0.0 }];
        let two = vec![Vec2 { x: 0.0, y: 0.0 }, Vec2 { x: 1.0, y: 1.0 }];
        let matched = MeshData {
            uv0: Some(one.clone()),
            uv3: Some(one.clone()),
            uv7: Some(two.clone()),
            ..MeshData::empty()
        };
        let matched = MeshData {
            vertices: vec![Vec3 {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }],
            ..matched
        };
        assert!(matched.has_uv0());
        assert!(!matched.has_uv1());
        assert!(matched.has_uv3());
        assert!(!matched.has_uv7());
        assert_eq!(matched.uv_count(), 1);
    }
}
