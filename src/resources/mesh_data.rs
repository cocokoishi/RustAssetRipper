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
}
