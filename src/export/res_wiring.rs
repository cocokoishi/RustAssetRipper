//! Production wiring for the resource/exporter families that task-5 owns: texture, model and
//! GLB (mesh, terrain, navmesh), audio, and shader text.
//!
//! This module is the single shared entry point the project export spine calls for the primary
//! content exporters. It mirrors the baseline `AssetRipper.Export.PrimaryContent` extractor
//! selection:
//!
//! - `TextureAssetExporter`/`TextureArrayAssetExporter`/`RawTextureExportCollection` own the
//!   class-28 `Texture2D`, class-89 `Cubemap`, class-117 `Texture3D`, class-187 `Texture2DArray`,
//!   and class-188 `CubemapArray` objects; [`TextureExportTarget::from_value`] reproduces the
//!   CLI target selection (`Texture2DView` for 28 and 89 with the cubemap vertical-flip skip,
//!   the array views otherwise) and [`export_texture_target`] runs the same production decoders
//!   `export-texture` uses.
//! - `GlbMeshExporter` owns class-43 `Mesh` (via `MeshData.TryMakeFromMesh`), `GlbTerrainExporter`
//!   owns class-156 `TerrainData` (via `GlbTerrainBuilder`), and `GlbNavMeshExporter` owns
//!   class-238 `NavMeshData` (via `GetHeightMeshes`).
//! - `AudioClipDecoder` owns class-83 `AudioClip`; [`plan_audio_export`] applies the same
//!   source-order and passthrough rules as `export-audio`.
//! - `DummyShaderTextExporter`/`SimpleShaderExporter` own class-48 `Shader`;
//!   [`plan_shader_export`] runs the same mode stack as `export-shader`.
//!
//! Class-21 `Material` emission stays with the YAML collection path (`Material` is a text asset,
//! not a primary-content binary), so [`resource_export_family`] reports
//! [`ResourceExportFamily::MaterialText`] for it and [`plan_resource_export`] returns `Ok(None)`.
//!
//! Every planner returns [`PlannedResourceExport`], which carries the Unity-project-relative file
//! name and the bytes; the export spine is responsible only for writing them inside the
//! `Assets/` tree and for the `.meta` sidecar. The exact spine call is:
//!
//! ```text
//! let plan = res_wiring::plan_resource_export(&res_wiring::ResourceExportRequest {
//!     name: &object_name, class_id, root: &payload.root,
//!     version, endian, stream_content, platform, platform_blob_little_endian,
//!     image_format, shader_mode, limits,
//! })?;
//! if let Some(plan) = plan { write(plan.file_name, plan.bytes); }
//! ```

use std::error::Error;
use std::fmt::{self, Display};

use crate::export::audio_export::{try_decode_audio, AudioDecodeError};
use crate::export::settings::ShaderExportMode;
use crate::export::shader_export::{export_shader_text, ShaderTextExportError};
use crate::resources::glb::{
    build_glb_with_limits, build_navmesh_glb_with_limits, build_terrain_glb_with_limits,
    GlbBuildLimits, GlbError, NavMeshHeightMesh, TerrainMeshInput,
};
use crate::resources::image_export::ImageExportFormat;
use crate::resources::mesh::{MeshView, MeshViewError};
use crate::resources::packed_bit_vector::PackedBitVectorLimits;
use crate::resources::shader::ShaderView;
use crate::resources::terrain_heatmap::{TerrainHeatmapError, TerrainHeightmap};
use crate::resources::texture::{
    export_cubemap_array_image, export_texture2d_array_image, export_texture2d_image,
    export_texture3d_image, Texture2DView, TextureExportError, CUBEMAP_CLASS_ID,
};
use crate::resources::texture_array::{
    CubemapArrayView, Texture2DArrayView, Texture3DView, TextureArrayError, CUBEMAPARRAY_CLASS_ID,
    TEXTURE2DARRAY_CLASS_ID, TEXTURE3D_CLASS_ID,
};
use crate::resources::vertex_data_blob::{EndianType, UnityVersion};
use crate::serialized_file::object::{SerializedFieldValue, SerializedValue};

/// Class-28 `Texture2D`.
pub const TEXTURE2D_CLASS_ID: i32 = 28;
/// Class-43 `Mesh`.
pub const MESH_CLASS_ID: i32 = 43;
/// Class-49 `TextAsset`.
pub const TEXT_ASSET_CLASS_ID: i32 = 49;
/// Class-83 `AudioClip`.
pub const AUDIO_CLIP_CLASS_ID: i32 = 83;
/// Class-115 `MonoScript`.
pub const MONO_SCRIPT_CLASS_ID: i32 = 115;
/// Class-128 `Font`.
pub const FONT_CLASS_ID: i32 = 128;
/// Class-152 `MovieTexture`.
pub const MOVIE_TEXTURE_CLASS_ID: i32 = 152;
/// Class-156 `TerrainData`.
pub const TERRAIN_CLASS_ID: i32 = 156;
/// Class-238 `NavMeshData`.
pub const NAVMESH_CLASS_ID: i32 = 238;
/// Class-329 `VideoClip`.
pub const VIDEO_CLIP_CLASS_ID: i32 = 329;

/// The exporter family a class ID belongs to.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ResourceExportFamily {
    /// A decode-and-write image export (classes 28, 89, 117, 187, 188).
    Texture,
    /// A GLB model export (class 43).
    Mesh,
    /// A GLB terrain export (class 156).
    Terrain,
    /// A GLB navmesh export (class 238).
    NavMesh,
    /// A native audio passthrough (class 83).
    Audio,
    /// `BinaryAssetContentExtractor`'s four arms: classes 49, 128, 152, 329.
    ///
    /// Unlike the other families this one has no single extension: a text asset is `bytes`, a font's
    /// comes from its own four-character code, a movie texture is `ogv`, and a video clip's comes from
    /// its original path. The planned file name carries the right one, so
    /// [`ResourceExportFamily::export_extension`] answers only the fallback.
    Binary,
    /// Shader text (class 48).
    ShaderText,
    /// A YAML text asset owned by the project YAML path (class 21).
    MaterialText,
    /// A whole-hierarchy GLB export owned by `GlbModelExporter`.
    ///
    /// Unlike the other families this one is not keyed by the asset's own class: the reference
    /// registers `GlbModelExporter` for `GameObjectHierarchyObject`, `IGameObject`, `IComponent`
    /// and `ILevelGameManager`, and `PrimaryContentExporter` tries the handler stack *before* the
    /// deleted-asset handler. A hierarchy object is a generated class-29 `SceneAsset` for a scene
    /// and a generated class-1001 `PrefabInstance` for a prefab, so both arrive here through the
    /// hierarchy path rather than through their own class.
    HierarchyGlb,
}

impl ResourceExportFamily {
    /// The file extension the family's export collection writes.
    ///
    /// From the reference's `ExportExtension` overrides: `GlbExportCollection`,
    /// `GlbSceneModelExportCollection` and `GlbPrefabModelExportCollection` all answer `"glb"`.
    pub const fn export_extension(self) -> &'static str {
        match self {
            Self::Texture => "png",
            Self::Mesh | Self::Terrain | Self::NavMesh | Self::HierarchyGlb => "glb",
            Self::Audio => "wav",
            Self::ShaderText | Self::MaterialText => "asset",
            // Only a fallback: each binary arm's real extension is in the planned file name.
            Self::Binary => "bytes",
        }
    }

    /// True when the family exports through a GLB container.
    pub const fn is_glb(self) -> bool {
        matches!(
            self,
            Self::Mesh | Self::Terrain | Self::NavMesh | Self::HierarchyGlb
        )
    }
}

/// `IContentExtractor` selection by Unity class ID.
///
/// Returns `None` for classes this family does not own, so the caller can fall through to the
/// YAML/other collection paths instead of silently dropping the object.
pub fn resource_export_family(class_id: i32) -> Option<ResourceExportFamily> {
    match class_id {
        TEXTURE2D_CLASS_ID
        | CUBEMAP_CLASS_ID
        | TEXTURE3D_CLASS_ID
        | TEXTURE2DARRAY_CLASS_ID
        | CUBEMAPARRAY_CLASS_ID => Some(ResourceExportFamily::Texture),
        MESH_CLASS_ID => Some(ResourceExportFamily::Mesh),
        TERRAIN_CLASS_ID => Some(ResourceExportFamily::Terrain),
        NAVMESH_CLASS_ID => Some(ResourceExportFamily::NavMesh),
        AUDIO_CLIP_CLASS_ID => Some(ResourceExportFamily::Audio),
        48 => Some(ResourceExportFamily::ShaderText),
        21 => Some(ResourceExportFamily::MaterialText),
        _ => None,
    }
}

/// Whether a generated asset is a scene or prefab hierarchy, which `GlbModelExporter` owns.
///
/// The reference registers the exporter against the `GameObjectHierarchyObject` type rather than
/// against a class ID, and it is tried before the deleted-asset handler. A whole-hierarchy export
/// is therefore selected by *what the object is* - a generated hierarchy root - not by its class,
/// so this is a separate entry point from [`resource_export_family`].
pub const fn hierarchy_glb_family(is_hierarchy_root: bool) -> Option<ResourceExportFamily> {
    if is_hierarchy_root {
        Some(ResourceExportFamily::HierarchyGlb)
    } else {
        None
    }
}

/// One planned primary-content export: a Unity-project-relative file name plus its bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct PlannedResourceExport {
    /// The `Assets/`-relative file name, including the format extension.
    pub file_name: String,
    /// The complete file bytes.
    pub bytes: Vec<u8>,
    /// The family that produced the bytes.
    pub family: ResourceExportFamily,
}

/// Bounds for one resource-export planning pass.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ResourceExportLimits {
    /// The GLB container cap (binary chunk plus JSON).
    pub glb: GlbBuildLimits,
    /// The packed-vector decode bounds used while assembling `MeshData`.
    pub packed_bit_vector: PackedBitVectorLimits,
}

/// A failure while planning a primary-content export.
#[derive(Debug)]
pub enum ResourceWiringError {
    /// The class ID has no primary-content family.
    UnsupportedClass {
        /// The Unity class ID.
        class_id: i32,
    },
    /// A texture payload could not be read.
    Texture(TextureExportError),
    /// An array texture payload could not be read.
    TextureArray(TextureArrayError),
    /// The mesh payload could not be read.
    Mesh(MeshViewError),
    /// The class-43 payload produced no `MeshData`.
    NoMeshData {
        /// The object name.
        name: String,
    },
    /// The terrain payload could not be read.
    Terrain(TerrainHeatmapError),
    /// The navmesh payload could not be read.
    NavMesh(String),
    /// A GLB container could not be built.
    Glb(GlbError),
    /// The audio payload could not be decoded.
    Audio(AudioDecodeError),
    /// The audio clip fields could not be read.
    AudioView(String),
    /// A font's `m_FontData` could not be read.
    FontData(String),
    /// The video clip fields could not be read, or its content was not supplied.
    VideoClip(String),
    /// The shader text could not be produced.
    Shader(ShaderTextExportError),
    /// A field lookup in the decoded payload failed.
    FieldLookup(String),
    /// A required field is absent or wrongly typed.
    WrongType {
        /// The field name.
        field: &'static str,
        /// The represented value kind.
        actual: &'static str,
    },
    /// A dimension does not fit the signed source type.
    DimensionOutOfRange {
        /// The field name.
        field: &'static str,
        /// The offending value.
        value: usize,
    },
}

impl Display for ResourceWiringError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedClass { class_id } => {
                write!(
                    formatter,
                    "class {class_id} has no primary-content exporter"
                )
            }
            Self::Texture(error) => write!(formatter, "{error}"),
            Self::TextureArray(error) => write!(formatter, "{error}"),
            Self::Mesh(error) => write!(formatter, "{error}"),
            Self::NoMeshData { name } => {
                write!(formatter, "mesh '{name}' produced no exportable MeshData")
            }
            Self::Terrain(error) => write!(formatter, "{error}"),
            Self::NavMesh(message) => write!(formatter, "navmesh export failed: {message}"),
            Self::Glb(error) => write!(formatter, "{error}"),
            Self::Audio(error) => write!(formatter, "{error}"),
            Self::AudioView(message) => write!(formatter, "audio clip read failed: {message}"),
            Self::FontData(message) => write!(formatter, "font data read failed: {message}"),
            Self::VideoClip(message) => write!(formatter, "video clip read failed: {message}"),
            Self::Shader(error) => write!(formatter, "{error}"),
            Self::FieldLookup(message) => write!(formatter, "field lookup failed: {message}"),
            Self::WrongType { field, actual } => {
                write!(
                    formatter,
                    "field '{field}' is {actual}, not the expected shape"
                )
            }
            Self::DimensionOutOfRange { field, value } => {
                write!(formatter, "field '{field}' value {value} does not fit i32")
            }
        }
    }
}

impl Error for ResourceWiringError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Texture(error) => Some(error),
            Self::TextureArray(error) => Some(error),
            Self::Mesh(error) => Some(error),
            Self::Terrain(error) => Some(error),
            Self::Glb(error) => Some(error),
            Self::Audio(error) => Some(error),
            Self::Shader(error) => Some(error),
            _ => None,
        }
    }
}

impl From<TextureExportError> for ResourceWiringError {
    fn from(error: TextureExportError) -> Self {
        Self::Texture(error)
    }
}

impl From<TextureArrayError> for ResourceWiringError {
    fn from(error: TextureArrayError) -> Self {
        Self::TextureArray(error)
    }
}

impl From<MeshViewError> for ResourceWiringError {
    fn from(error: MeshViewError) -> Self {
        Self::Mesh(error)
    }
}

impl From<TerrainHeatmapError> for ResourceWiringError {
    fn from(error: TerrainHeatmapError) -> Self {
        Self::Terrain(error)
    }
}

impl From<GlbError> for ResourceWiringError {
    fn from(error: GlbError) -> Self {
        Self::Glb(error)
    }
}

impl From<AudioDecodeError> for ResourceWiringError {
    fn from(error: AudioDecodeError) -> Self {
        Self::Audio(error)
    }
}

impl From<ShaderTextExportError> for ResourceWiringError {
    fn from(error: ShaderTextExportError) -> Self {
        Self::Shader(error)
    }
}

/// The decoded texture target, mirroring the `export-texture` dispatch.
#[derive(Clone, Debug, PartialEq)]
pub enum TextureExportTarget {
    /// A class-28 `Texture2D` or class-89 `Cubemap` through the shared `ITexture2D` view.
    Texture2D {
        /// The decoded view.
        view: Texture2DView,
        /// `texture is not ICubemap`: cubemaps skip the vertical flip.
        flip_y: bool,
    },
    /// A class-117 `Texture3D`.
    Texture3D {
        /// The decoded view.
        view: Texture3DView,
    },
    /// A class-187 `Texture2DArray`.
    Texture2DArray {
        /// The decoded view.
        view: Texture2DArrayView,
    },
    /// A class-188 `CubemapArray`.
    CubemapArray {
        /// The decoded view.
        view: CubemapArrayView,
    },
}

impl TextureExportTarget {
    /// Reads the target for a texture class ID, exactly like the `export-texture` CLI.
    pub fn from_value(class_id: i32, root: &SerializedValue) -> Result<Self, ResourceWiringError> {
        match class_id {
            TEXTURE2D_CLASS_ID | CUBEMAP_CLASS_ID => Ok(Self::Texture2D {
                view: Texture2DView::from_value(root).map_err(TextureExportError::View)?,
                flip_y: class_id != CUBEMAP_CLASS_ID,
            }),
            TEXTURE3D_CLASS_ID => Ok(Self::Texture3D {
                view: Texture3DView::from_value(root)?,
            }),
            TEXTURE2DARRAY_CLASS_ID => Ok(Self::Texture2DArray {
                view: Texture2DArrayView::from_value(root)?,
            }),
            CUBEMAPARRAY_CLASS_ID => Ok(Self::CubemapArray {
                view: CubemapArrayView::from_value(root)?,
            }),
            other => Err(ResourceWiringError::UnsupportedClass { class_id: other }),
        }
    }

    /// Whether the inline image data is empty, which routes the export to the stream.
    pub fn image_data_is_empty(&self) -> bool {
        match self {
            Self::Texture2D { view, .. } => view.image_data.is_empty(),
            Self::Texture3D { view } => view.image_data.is_empty(),
            Self::Texture2DArray { view } => view.image_data.is_empty(),
            Self::CubemapArray { view } => view.image_data.is_empty(),
        }
    }

    /// The `m_StreamData` descriptor when the payload carries one.
    pub fn stream_data(&self) -> Option<&crate::resource::StreamedResourceInfo> {
        match self {
            Self::Texture2D { view, .. } => view.stream_data.as_ref(),
            Self::Texture3D { view } => view.stream_data.as_ref(),
            Self::Texture2DArray { view } => view.stream_data.as_ref(),
            Self::CubemapArray { view } => view.stream_data.as_ref(),
        }
    }
}

/// Runs the production texture decode/write pipeline for one target.
pub fn export_texture_target(
    target: &TextureExportTarget,
    stream_content: Option<&[u8]>,
    platform: i32,
    platform_blob_little_endian: bool,
    format: ImageExportFormat,
) -> Result<Vec<u8>, ResourceWiringError> {
    let bytes = match target {
        TextureExportTarget::Texture2D { view, flip_y } => export_texture2d_image(
            view,
            stream_content,
            platform,
            platform_blob_little_endian,
            *flip_y,
            format,
        )?,
        TextureExportTarget::Texture3D { view } => {
            export_texture3d_image(view, stream_content, format)?
        }
        TextureExportTarget::Texture2DArray { view } => {
            export_texture2d_array_image(view, stream_content, format)?
        }
        TextureExportTarget::CubemapArray { view } => {
            export_cubemap_array_image(view, stream_content, format)?
        }
    };
    Ok(bytes)
}

/// Plans one texture export as `{name}.{extension}`.
pub fn plan_texture_export(
    name: &str,
    class_id: i32,
    root: &SerializedValue,
    stream_content: Option<&[u8]>,
    platform: i32,
    platform_blob_little_endian: bool,
    format: ImageExportFormat,
) -> Result<PlannedResourceExport, ResourceWiringError> {
    let target = TextureExportTarget::from_value(class_id, root)?;
    let bytes = export_texture_target(
        &target,
        stream_content,
        platform,
        platform_blob_little_endian,
        format,
    )?;
    Ok(PlannedResourceExport {
        file_name: format!("{name}.{}", format.file_extension()),
        bytes,
        family: ResourceExportFamily::Texture,
    })
}

/// Plans one class-43 mesh GLB export through `MeshView::read_mesh_data` and `build_glb`.
pub fn plan_mesh_export(
    name: &str,
    root: &SerializedValue,
    version: UnityVersion,
    endian: EndianType,
    limits: ResourceExportLimits,
) -> Result<PlannedResourceExport, ResourceWiringError> {
    let view = MeshView::from_value(root, version, endian)?;
    let mesh = view
        .read_mesh_data(limits.packed_bit_vector)?
        .ok_or_else(|| ResourceWiringError::NoMeshData {
            name: name.to_owned(),
        })?;
    let export = build_glb_with_limits(name, &mesh, limits.glb)?;
    Ok(PlannedResourceExport {
        file_name: format!("{name}.glb"),
        bytes: export.bytes,
        family: ResourceExportFamily::Mesh,
    })
}

/// `Heightmap.Scale` of a class-156 payload. An absent field reads as the generated default.
pub fn read_heightmap_scale(root: &SerializedValue) -> Result<[f32; 3], ResourceWiringError> {
    let heightmap = lookup_field(root, "m_Heightmap")?;
    let Some(heightmap) = heightmap else {
        return Err(ResourceWiringError::WrongType {
            field: "m_Heightmap",
            actual: "absent",
        });
    };
    let fields = struct_fields(heightmap, "m_Heightmap")?;
    let Some(scale) = find_field(fields, "m_Scale")? else {
        return Ok([0.0; 3]);
    };
    let scale_fields = struct_fields(scale, "m_Scale")?;
    Ok([
        float_field(scale_fields, "x"),
        float_field(scale_fields, "y"),
        float_field(scale_fields, "z"),
    ])
}

/// Plans one class-156 terrain GLB export through `TerrainHeightmap` and the source builder.
pub fn plan_terrain_export(
    name: &str,
    root: &SerializedValue,
    limits: ResourceExportLimits,
) -> Result<PlannedResourceExport, ResourceWiringError> {
    let heightmap = TerrainHeightmap::from_value(root)?;
    let scale = read_heightmap_scale(root)?;
    let input = TerrainMeshInput {
        name,
        width: i32::try_from(heightmap.width).map_err(|_| {
            ResourceWiringError::DimensionOutOfRange {
                field: "m_Width",
                value: heightmap.width,
            }
        })?,
        height: i32::try_from(heightmap.height).map_err(|_| {
            ResourceWiringError::DimensionOutOfRange {
                field: "m_Height",
                value: heightmap.height,
            }
        })?,
        resolution: i32::try_from(heightmap.resolution).map_err(|_| {
            ResourceWiringError::DimensionOutOfRange {
                field: "m_Resolution",
                value: heightmap.resolution,
            }
        })?,
        scale,
        heights: &heightmap.heights,
    };
    let export = build_terrain_glb_with_limits(&input, limits.glb)?;
    Ok(PlannedResourceExport {
        file_name: format!("{name}.glb"),
        bytes: export.bytes,
        family: ResourceExportFamily::Terrain,
    })
}

/// Owned `HeightMeshData` storage so a navmesh read outlives the decoded payload borrow.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NavMeshHeightMeshOwned {
    /// `HeightMeshData.Indices`.
    pub indices: Vec<i32>,
    /// `HeightMeshData.Vertices`.
    pub vertices: Vec<[f32; 3]>,
}

impl NavMeshHeightMeshOwned {
    /// Borrows the owned storage as the builder input.
    pub fn as_borrowed(&self) -> NavMeshHeightMesh<'_> {
        NavMeshHeightMesh {
            indices: &self.indices,
            vertices: &self.vertices,
        }
    }
}

/// Reads `m_HeightMeshes` from a decoded class-238 payload.
pub fn read_navmesh_height_meshes(
    root: &SerializedValue,
) -> Result<Vec<NavMeshHeightMeshOwned>, ResourceWiringError> {
    let Some(list) = lookup_field(root, "m_HeightMeshes")? else {
        return Ok(Vec::new());
    };
    let SerializedValue::Array(entries) = list else {
        return Err(ResourceWiringError::WrongType {
            field: "m_HeightMeshes",
            actual: "not an array",
        });
    };
    let mut meshes = Vec::with_capacity(entries.len());
    for entry in entries {
        let fields = struct_fields(entry, "m_HeightMeshes")?;
        let mut mesh = NavMeshHeightMeshOwned::default();
        if let Some(indices) = find_field_any(fields, &["m_Indices", "Indices"])? {
            let SerializedValue::Array(values) = indices else {
                return Err(ResourceWiringError::WrongType {
                    field: "m_Indices",
                    actual: "not an array",
                });
            };
            mesh.indices = values
                .iter()
                .map(|value| match value {
                    SerializedValue::I32(value) => Ok(*value),
                    SerializedValue::U32(value) => {
                        i32::try_from(*value).map_err(|_| ResourceWiringError::WrongType {
                            field: "m_Indices",
                            actual: "a u32 above i32::MAX",
                        })
                    }
                    _ => Err(ResourceWiringError::WrongType {
                        field: "m_Indices",
                        actual: "not an integer",
                    }),
                })
                .collect::<Result<Vec<_>, _>>()?;
        }
        if let Some(vertices) = find_field_any(fields, &["m_Vertices", "Vertices"])? {
            let SerializedValue::Array(values) = vertices else {
                return Err(ResourceWiringError::WrongType {
                    field: "m_Vertices",
                    actual: "not an array",
                });
            };
            mesh.vertices = values
                .iter()
                .map(|value| {
                    let vertex_fields = struct_fields(value, "m_Vertices")?;
                    Ok([
                        float_field(vertex_fields, "x"),
                        float_field(vertex_fields, "y"),
                        float_field(vertex_fields, "z"),
                    ])
                })
                .collect::<Result<Vec<_>, ResourceWiringError>>()?;
        }
        meshes.push(mesh);
    }
    Ok(meshes)
}

/// Plans one class-238 navmesh GLB export.
pub fn plan_navmesh_export(
    name: &str,
    root: &SerializedValue,
    limits: ResourceExportLimits,
) -> Result<PlannedResourceExport, ResourceWiringError> {
    let owned = read_navmesh_height_meshes(root)?;
    plan_navmesh_export_from_meshes(name, &owned, limits)
}

/// Plans one navmesh GLB export from already-read height meshes.
pub fn plan_navmesh_export_from_meshes(
    name: &str,
    meshes: &[NavMeshHeightMeshOwned],
    limits: ResourceExportLimits,
) -> Result<PlannedResourceExport, ResourceWiringError> {
    let borrowed: Vec<NavMeshHeightMesh<'_>> = meshes
        .iter()
        .map(NavMeshHeightMeshOwned::as_borrowed)
        .collect();
    let export = build_navmesh_glb_with_limits(name, &borrowed, limits.glb)?;
    Ok(PlannedResourceExport {
        file_name: format!("{name}.glb"),
        bytes: export.bytes,
        family: ResourceExportFamily::NavMesh,
    })
}

/// Plans one class-83 audio export: the native bytes plus their source extension.
pub fn plan_audio_export(
    name: &str,
    sound_type: Option<i32>,
    data: &[u8],
) -> Result<PlannedResourceExport, ResourceWiringError> {
    let (bytes, extension) = try_decode_audio(name, sound_type, data)?;
    Ok(PlannedResourceExport {
        file_name: format!("{name}.{extension}"),
        bytes,
        family: ResourceExportFamily::Audio,
    })
}

/// Plans one of `BinaryAssetContentExtractor`'s four arms.
///
/// The primary-content binary extractor writes an asset's own bytes with an arm-specific extension.
/// It is the one extractor whose payload had no producer in this module, because the Unity-project
/// pipeline exports these assets as YAML rather than as raw bytes.
///
/// `Ok(None)` means the arm **declines**, matching [`plan_resource_export`]'s convention: the caller
/// falls through to the next handler. Every arm requires a non-empty payload, which is what makes an
/// empty asset decline rather than write an empty file. `MovieTexture` keeps `Has_MovieData` and
/// non-emptiness as two separate conditions, as the source does, so an absent field and an empty one
/// are distinguishable even though both decline.
///
/// `stream_content` is the resolved `m_ExternalResources` range for a video clip. A clip whose
/// descriptor passed the integrity gate but whose bytes were not supplied is an error rather than a
/// decline, because the reference reaches its `GetContent` and fails there.
pub fn plan_binary_export(
    name: &str,
    class_id: i32,
    root: &SerializedValue,
    stream_content: Option<&[u8]>,
) -> Result<Option<PlannedResourceExport>, ResourceWiringError> {
    let plan = match class_id {
        TEXT_ASSET_CLASS_ID => {
            let data = byte_array_field(root, "m_Script")?.unwrap_or_default();
            if data.is_empty() {
                return Ok(None);
            }
            binary_plan(name, "bytes", data)
        }
        FONT_CLASS_ID => {
            let data = crate::export::resource_extensions::font_data_bytes(root)
                .map_err(|error| ResourceWiringError::FontData(error.to_string()))?
                .unwrap_or_default();
            if data.is_empty() {
                return Ok(None);
            }
            // The extension is read from the payload's own four-character code.
            let extension = crate::export::resource_extensions::font_extension(&data);
            binary_plan(name, extension, data)
        }
        MOVIE_TEXTURE_CLASS_ID => {
            let Some(data) = byte_array_field(root, "m_MovieData")? else {
                return Ok(None);
            };
            if data.is_empty() {
                return Ok(None);
            }
            binary_plan(name, "ogv", data)
        }
        VIDEO_CLIP_CLASS_ID => {
            let view = crate::resources::video_clip::VideoClipView::from_value(root)
                .map_err(|error| ResourceWiringError::VideoClip(error.to_string()))?;
            if !view.check_integrity() {
                return Ok(None);
            }
            let Some(data) = stream_content else {
                return Err(ResourceWiringError::VideoClip(
                    "video clip passed its integrity gate but no resource content was supplied"
                        .to_owned(),
                ));
            };
            let extension = crate::export::resource_extensions::video_export_extension(
                view.original_path.as_deref().unwrap_or(""),
                crate::export::asset_path::HostPathPlatform::host(),
            );
            binary_plan(name, extension, data.to_vec())
        }
        _ => return Ok(None),
    };
    Ok(Some(plan))
}

/// The planned export for one binary arm.
fn binary_plan(name: &str, extension: &str, bytes: Vec<u8>) -> PlannedResourceExport {
    PlannedResourceExport {
        file_name: format!("{name}.{extension}"),
        bytes,
        family: ResourceExportFamily::Binary,
    }
}

/// A byte-array field in either serialized shape: packed `Bytes`, or an `Array` of `U8`.
///
/// The two shapes are both real: a TypeTree-decoded payload keeps a byte array as `Bytes`, while a
/// generated-class read may produce the element list. `resource_extensions::font_data_bytes` makes
/// the same distinction for `m_FontData`, and reports a non-`U8` element rather than coercing it.
fn byte_array_field(
    root: &SerializedValue,
    name: &'static str,
) -> Result<Option<Vec<u8>>, ResourceWiringError> {
    let Some(value) = lookup_field(root, name)? else {
        return Ok(None);
    };
    match value {
        SerializedValue::Bytes(bytes) => Ok(Some(bytes.clone())),
        SerializedValue::StringBytes(bytes) => Ok(Some(bytes.clone())),
        SerializedValue::Array(values) => {
            let mut output = Vec::with_capacity(values.len());
            for (index, value) in values.iter().enumerate() {
                let SerializedValue::U8(byte) = value else {
                    return Err(ResourceWiringError::FieldLookup(format!(
                        "{name}[{index}] is not a byte"
                    )));
                };
                output.push(*byte);
            }
            Ok(Some(output))
        }
        _ => Ok(None),
    }
}

/// Plans one class-48 shader text export. `ShaderExportMode::Yaml` has no text form and stays
/// with the YAML collection, reported as [`ShaderTextExportError::YamlModeWritesAsset`].
pub fn plan_shader_export(
    name: &str,
    view: &ShaderView,
    mode: ShaderExportMode,
) -> Result<PlannedResourceExport, ResourceWiringError> {
    let bytes = export_shader_text(view, mode)?;
    Ok(PlannedResourceExport {
        file_name: format!("{name}.shader"),
        bytes,
        family: ResourceExportFamily::ShaderText,
    })
}

/// One resource object supplied to [`plan_resource_export`].
pub struct ResourceExportRequest<'a> {
    /// The decoded `m_Name`, used for the file name.
    pub name: &'a str,
    /// The Unity class ID.
    pub class_id: i32,
    /// The decoded object payload.
    pub root: &'a SerializedValue,
    /// The Unity version of the owning SerializedFile.
    pub version: UnityVersion,
    /// The file endianness.
    pub endian: EndianType,
    /// The resolved streamed-resource bytes, when the object has one.
    pub stream_content: Option<&'a [u8]>,
    /// The SerializedFile target platform discriminant.
    pub platform: i32,
    /// Whether the platform blob is little-endian.
    pub platform_blob_little_endian: bool,
    /// The configured image format.
    pub image_format: ImageExportFormat,
    /// The configured shader export mode.
    pub shader_mode: ShaderExportMode,
    /// The export bounds.
    pub limits: ResourceExportLimits,
}

/// Dispatches one object to its primary-content exporter.
///
/// Returns `Ok(None)` for classes the primary-content pipeline does not own (for example the
/// class-21 `Material` text asset), so the export spine keeps its existing YAML route.
pub fn plan_resource_export(
    request: &ResourceExportRequest<'_>,
) -> Result<Option<PlannedResourceExport>, ResourceWiringError> {
    let Some(family) = resource_export_family(request.class_id) else {
        return Ok(None);
    };
    let plan = match family {
        ResourceExportFamily::Texture => plan_texture_export(
            request.name,
            request.class_id,
            request.root,
            request.stream_content,
            request.platform,
            request.platform_blob_little_endian,
            request.image_format,
        )?,
        ResourceExportFamily::Mesh => plan_mesh_export(
            request.name,
            request.root,
            request.version,
            request.endian,
            request.limits,
        )?,
        ResourceExportFamily::Terrain => {
            plan_terrain_export(request.name, request.root, request.limits)?
        }
        ResourceExportFamily::NavMesh => {
            plan_navmesh_export(request.name, request.root, request.limits)?
        }
        ResourceExportFamily::Audio => {
            let view = crate::resources::audio::AudioClipView::from_value(request.root)
                .map_err(|error| ResourceWiringError::AudioView(error.to_string()))?;
            match view.audio_data_source() {
                crate::resources::audio::AudioDataSource::Inline(bytes) => {
                    plan_audio_export(request.name, view.sound_type_discriminant(), bytes)?
                }
                crate::resources::audio::AudioDataSource::Resource(info) => {
                    // A streamed clip takes the resolved `.resS` bytes when the caller supplied
                    // them; the descriptor alone is an explicit refusal, like the CLI.
                    match request.stream_content {
                        Some(bytes) => {
                            plan_audio_export(request.name, view.sound_type_discriminant(), bytes)?
                        }
                        None => {
                            return Err(ResourceWiringError::AudioView(format!(
                                "audio clip '{}' is streamed through '{}' but no resource content was supplied",
                                request.name, info.path
                            )))
                        }
                    }
                }
                crate::resources::audio::AudioDataSource::Empty => {
                    return Err(ResourceWiringError::AudioView(format!(
                        "audio clip '{}' has neither inline data nor a resolved stream",
                        request.name
                    )))
                }
            }
        }
        ResourceExportFamily::HierarchyGlb => {
            // A whole-hierarchy GLB is not a single-asset export: it needs the scene graph and the
            // per-node decoded meshes, so it goes through `crate::export::scene_glb` rather than
            // through this per-asset planner. Returning `None` keeps the caller's fall-through
            // honest instead of emitting a one-object GLB that would look like a hierarchy export.
            return Ok(None);
        }
        ResourceExportFamily::ShaderText => {
            let view = ShaderView::from_value(request.root)
                .map_err(|error| ResourceWiringError::Shader(ShaderTextExportError::View(error)))?;
            plan_shader_export(request.name, &view, request.shader_mode)?
        }
        ResourceExportFamily::MaterialText => return Ok(None),
        // The binary arms are not dispatched here: their classes are absent from
        // `resource_export_family` and go through `plan_binary_export`, whose gates differ per arm.
        ResourceExportFamily::Binary => return Ok(None),
    };
    Ok(Some(plan))
}

fn lookup_field<'a>(
    root: &'a SerializedValue,
    name: &'static str,
) -> Result<Option<&'a SerializedValue>, ResourceWiringError> {
    root.unique_field_in_base_chain(name)
        .map_err(|error| ResourceWiringError::FieldLookup(format!("{name}: {error}")))
}

fn struct_fields<'a>(
    value: &'a SerializedValue,
    field: &'static str,
) -> Result<&'a [SerializedFieldValue], ResourceWiringError> {
    match value {
        SerializedValue::Struct(fields) => Ok(fields),
        _ => Err(ResourceWiringError::WrongType {
            field,
            actual: "not a struct",
        }),
    }
}

fn find_field<'a>(
    fields: &'a [SerializedFieldValue],
    name: &'static str,
) -> Result<Option<&'a SerializedValue>, ResourceWiringError> {
    find_field_any(fields, &[name])
}

fn find_field_any<'a>(
    fields: &'a [SerializedFieldValue],
    names: &[&'static str],
) -> Result<Option<&'a SerializedValue>, ResourceWiringError> {
    let mut found: Option<&SerializedValue> = None;
    for name in names {
        for field in fields.iter().filter(|field| field.name == *name) {
            if found.is_some() {
                return Err(ResourceWiringError::FieldLookup(format!(
                    "field '{name}' occurs more than once"
                )));
            }
            found = Some(&field.value);
        }
    }
    Ok(found)
}

fn float_field(fields: &[SerializedFieldValue], name: &'static str) -> f32 {
    fields
        .iter()
        .find(|field| field.name == name)
        .map(|field| match &field.value {
            SerializedValue::F32(value) => *value,
            SerializedValue::F64(value) => *value as f32,
            _ => 0.0,
        })
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::settings::ShaderExportMode;
    use crate::resources::image_export::ImageExportFormat;

    fn field(name: &str, value: SerializedValue) -> SerializedFieldValue {
        SerializedFieldValue {
            name: name.to_owned(),
            value,
        }
    }

    fn struct_value(fields: Vec<SerializedFieldValue>) -> SerializedValue {
        SerializedValue::Struct(fields)
    }

    /// A payload with one byte-array field, in the packed shape.
    fn bytes_payload(name: &str, bytes: &[u8]) -> SerializedValue {
        struct_value(vec![field(name, SerializedValue::Bytes(bytes.to_vec()))])
    }

    /// `BinaryAssetContentExtractor`'s four arms, each with its own gate and extension.
    #[test]
    fn the_binary_arms_plan_their_own_extension_and_gate() {
        // A text asset with data writes `.bytes`; an empty one declines so the stack continues.
        let plan = plan_binary_export(
            "story",
            TEXT_ASSET_CLASS_ID,
            &bytes_payload("m_Script", b"hello"),
            None,
        )
        .unwrap()
        .expect("a non-empty text asset is planned");
        assert_eq!(plan.file_name, "story.bytes");
        assert_eq!(plan.bytes, b"hello");
        assert_eq!(plan.family, ResourceExportFamily::Binary);
        assert_eq!(
            plan_binary_export(
                "story",
                TEXT_ASSET_CLASS_ID,
                &bytes_payload("m_Script", b""),
                None
            )
            .unwrap(),
            None,
            "an empty payload declines rather than writing an empty file"
        );
        // A missing field is also a decline.
        assert_eq!(
            plan_binary_export("story", TEXT_ASSET_CLASS_ID, &struct_value(vec![]), None).unwrap(),
            None
        );

        // A font's extension comes from its own four-character code.
        let ttf = plan_binary_export(
            "face",
            FONT_CLASS_ID,
            &bytes_payload("m_FontData", &[0x00, 0x01, 0x00, 0x00, 0xAA]),
            None,
        )
        .unwrap()
        .expect("a non-empty font is planned");
        assert_eq!(ttf.file_name, "face.ttf");
        let otf = plan_binary_export(
            "face",
            FONT_CLASS_ID,
            &bytes_payload("m_FontData", b"OTTOxxxx"),
            None,
        )
        .unwrap()
        .expect("a non-empty font is planned");
        assert_eq!(otf.file_name, "face.otf");

        // A movie texture keeps `Has_MovieData` and non-emptiness separate: both decline, but for
        // different reasons, and the field's absence is not treated as an empty one.
        assert_eq!(
            plan_binary_export("clip", MOVIE_TEXTURE_CLASS_ID, &struct_value(vec![]), None)
                .unwrap(),
            None
        );
        assert_eq!(
            plan_binary_export(
                "clip",
                MOVIE_TEXTURE_CLASS_ID,
                &bytes_payload("m_MovieData", b""),
                None
            )
            .unwrap(),
            None
        );
        let movie = plan_binary_export(
            "clip",
            MOVIE_TEXTURE_CLASS_ID,
            &bytes_payload("m_MovieData", b"OGVDATA"),
            None,
        )
        .unwrap()
        .expect("a non-empty movie texture is planned");
        assert_eq!(movie.file_name, "clip.ogv");
        assert_eq!(movie.bytes, b"OGVDATA");

        // A class outside the four arms is not this planner's business.
        assert_eq!(
            plan_binary_export("mesh", MESH_CLASS_ID, &struct_value(vec![]), None).unwrap(),
            None
        );
    }

    /// A video clip's extension comes from its original path, and its bytes from the caller.
    #[test]
    fn the_video_clip_arm_needs_both_a_descriptor_and_content() {
        let intact = || {
            struct_value(vec![
                field(
                    "m_ExternalResources",
                    SerializedValue::Struct(vec![
                        field(
                            "m_Source",
                            SerializedValue::StringBytes(b"shared.resS".to_vec()),
                        ),
                        field("m_Offset", SerializedValue::U64(0)),
                        field("m_Size", SerializedValue::U64(64)),
                    ]),
                ),
                field(
                    "m_OriginalPath",
                    SerializedValue::StringBytes(b"movies/intro.webm".to_vec()),
                ),
            ])
        };

        let plan = plan_binary_export("intro", VIDEO_CLIP_CLASS_ID, &intact(), Some(b"BYTES"))
            .unwrap()
            .expect("an intact clip with content is planned");
        assert_eq!(plan.file_name, "intro.webm");
        assert_eq!(plan.bytes, b"BYTES");

        // A clip that passed its descriptor gate but has no content is an error, not a decline:
        // the reference reaches its `GetContent` and fails there.
        assert!(plan_binary_export("intro", VIDEO_CLIP_CLASS_ID, &intact(), None).is_err());

        // A clip whose descriptor names no usable range declines before any content is needed.
        let broken = struct_value(vec![field(
            "m_OriginalPath",
            SerializedValue::StringBytes(b"movies/intro.webm".to_vec()),
        )]);
        assert_eq!(
            plan_binary_export("intro", VIDEO_CLIP_CLASS_ID, &broken, Some(b"BYTES")).unwrap(),
            None
        );
    }

    /// The packed-bytes and element-list shapes are both accepted, and a non-byte element is
    /// reported rather than coerced.
    #[test]
    fn a_byte_array_field_accepts_both_shapes() {
        let listed = struct_value(vec![field(
            "m_Script",
            SerializedValue::Array(vec![
                SerializedValue::U8(1),
                SerializedValue::U8(2),
                SerializedValue::U8(3),
            ]),
        )]);
        let plan = plan_binary_export("story", TEXT_ASSET_CLASS_ID, &listed, None)
            .unwrap()
            .expect("the element-list shape is accepted");
        assert_eq!(plan.bytes, vec![1, 2, 3]);

        let wrong = struct_value(vec![field(
            "m_Script",
            SerializedValue::Array(vec![SerializedValue::I32(1)]),
        )]);
        assert!(
            plan_binary_export("story", TEXT_ASSET_CLASS_ID, &wrong, None).is_err(),
            "a non-byte element is reported"
        );
    }

    #[test]
    fn exporter_family_selection_matches_the_baseline_extractors() {
        assert_eq!(
            resource_export_family(28),
            Some(ResourceExportFamily::Texture)
        );
        assert_eq!(
            resource_export_family(89),
            Some(ResourceExportFamily::Texture)
        );
        assert_eq!(
            resource_export_family(117),
            Some(ResourceExportFamily::Texture)
        );
        assert_eq!(
            resource_export_family(187),
            Some(ResourceExportFamily::Texture)
        );
        assert_eq!(
            resource_export_family(188),
            Some(ResourceExportFamily::Texture)
        );
        assert_eq!(resource_export_family(43), Some(ResourceExportFamily::Mesh));
        assert_eq!(
            resource_export_family(156),
            Some(ResourceExportFamily::Terrain)
        );
        assert_eq!(
            resource_export_family(238),
            Some(ResourceExportFamily::NavMesh)
        );
        assert_eq!(
            resource_export_family(83),
            Some(ResourceExportFamily::Audio)
        );
        assert_eq!(
            resource_export_family(48),
            Some(ResourceExportFamily::ShaderText)
        );
        assert_eq!(
            resource_export_family(21),
            Some(ResourceExportFamily::MaterialText)
        );
        // Classes with no primary-content extractor stay unowned.
        assert_eq!(resource_export_family(1), None);
        assert_eq!(resource_export_family(28_000), None);
    }

    /// The hierarchy GLB family is selected by what the object is, not by its class.
    ///
    /// The reference registers `GlbModelExporter` for `GameObjectHierarchyObject`,
    /// `IGameObject`, `IComponent` and `ILevelGameManager`, so a hierarchy root is chosen by type
    /// rather than through `resource_export_family`. Keeping the two entry points separate is what
    /// stops a plain class-1 GameObject from being mistaken for a whole-hierarchy export.
    #[test]
    fn the_hierarchy_glb_family_is_selected_by_object_kind_not_class_id() {
        assert_eq!(
            hierarchy_glb_family(true),
            Some(ResourceExportFamily::HierarchyGlb)
        );
        assert_eq!(hierarchy_glb_family(false), None);

        // A class-1 GameObject and a class-4 Transform have no class-keyed GLB family; a mesh
        // does, and it is the single-mesh one.
        assert_eq!(resource_export_family(1), None);
        assert_eq!(resource_export_family(4), None);
        assert_eq!(resource_export_family(43), Some(ResourceExportFamily::Mesh));
    }

    /// Every family's extension matches the reference's `ExportExtension` override.
    #[test]
    fn export_family_extensions_match_the_reference_collections() {
        assert_eq!(ResourceExportFamily::Mesh.export_extension(), "glb");
        assert_eq!(ResourceExportFamily::Terrain.export_extension(), "glb");
        assert_eq!(ResourceExportFamily::NavMesh.export_extension(), "glb");
        // `GlbSceneModelExportCollection` and `GlbPrefabModelExportCollection` both answer "glb".
        assert_eq!(ResourceExportFamily::HierarchyGlb.export_extension(), "glb");
        assert_eq!(ResourceExportFamily::Texture.export_extension(), "png");
        assert_eq!(ResourceExportFamily::Audio.export_extension(), "wav");

        // Every GLB family reports itself as one, and no other family does.
        assert!(ResourceExportFamily::Mesh.is_glb());
        assert!(ResourceExportFamily::Terrain.is_glb());
        assert!(ResourceExportFamily::NavMesh.is_glb());
        assert!(ResourceExportFamily::HierarchyGlb.is_glb());
        assert!(!ResourceExportFamily::Texture.is_glb());
        assert!(!ResourceExportFamily::Audio.is_glb());
        assert!(!ResourceExportFamily::ShaderText.is_glb());
        assert!(!ResourceExportFamily::MaterialText.is_glb());
    }

    /// A hierarchy root does not go through the per-asset planner.
    ///
    /// A whole-hierarchy GLB needs the scene graph and the per-node decoded meshes, so
    /// `plan_resource_export` reports `None` for the family and the caller routes it to
    /// `crate::export::scene_glb`. Emitting a one-object GLB here would be a silent degradation
    /// that looks like a hierarchy export. The family is therefore absent from the class-keyed
    /// classifier, which is the property that keeps the two paths apart.
    #[test]
    fn the_per_asset_planner_does_not_own_the_hierarchy_family() {
        // Class 29 (a scene's SceneAsset) and class 1001 (a prefab) both resolve through the
        // hierarchy path, so neither is claimed by the class-keyed classifier.
        assert_eq!(resource_export_family(29), None);
        assert_eq!(resource_export_family(1001), None);
        // A hierarchy root is still recognised as a hierarchy export.
        assert_eq!(
            hierarchy_glb_family(true),
            Some(ResourceExportFamily::HierarchyGlb)
        );
    }

    #[test]
    fn resource_export_limits_default_to_the_container_and_packed_bounds() {
        let limits = ResourceExportLimits::default();
        assert_eq!(limits.glb, GlbBuildLimits::default());
        assert_eq!(limits.packed_bit_vector, PackedBitVectorLimits::default());
    }

    #[test]
    fn cubemaps_skip_the_vertical_flip_and_2d_textures_keep_it() {
        let root = struct_value(vec![
            field("m_Name", SerializedValue::StringBytes(b"Cube".to_vec())),
            field("m_Width", SerializedValue::I32(1)),
            field("m_Height", SerializedValue::I32(1)),
            field("m_TextureFormat", SerializedValue::I32(4)),
            field("image data", SerializedValue::Bytes(vec![0, 0, 0, 255])),
        ]);
        let texture = TextureExportTarget::from_value(28, &root).unwrap();
        match &texture {
            TextureExportTarget::Texture2D { flip_y, .. } => assert!(*flip_y),
            other => panic!("unexpected target {other:?}"),
        }
        let cubemap = TextureExportTarget::from_value(89, &root).unwrap();
        match &cubemap {
            TextureExportTarget::Texture2D { flip_y, .. } => assert!(!*flip_y),
            other => panic!("unexpected target {other:?}"),
        }
        assert!(!texture.image_data_is_empty());
        assert!(texture.stream_data().is_none());
        // A non-texture class is rejected rather than silently mishandled.
        assert!(matches!(
            TextureExportTarget::from_value(43, &root),
            Err(ResourceWiringError::UnsupportedClass { class_id: 43 })
        ));
    }

    #[test]
    fn terrain_plan_reads_the_heightmap_and_scale_through_the_source_builder() {
        // A two-by-two heightmap with a non-uniform scale: the fixture-backed builder emits
        // POSITION plus TEXCOORD_0 and the JSON node is named after the terrain.
        let root = struct_value(vec![
            field("m_Name", SerializedValue::StringBytes(b"Terrain".to_vec())),
            field(
                "m_Heightmap",
                struct_value(vec![
                    field("m_Width", SerializedValue::I32(2)),
                    field("m_Height", SerializedValue::I32(2)),
                    field("m_Resolution", SerializedValue::I32(2)),
                    field(
                        "m_Scale",
                        struct_value(vec![
                            field("x", SerializedValue::F32(1.0)),
                            field("y", SerializedValue::F32(2.0)),
                            field("z", SerializedValue::F32(4.0)),
                        ]),
                    ),
                    field(
                        "m_Heights",
                        SerializedValue::Array(
                            [0i16, 1000, 2000, 3000]
                                .into_iter()
                                .map(SerializedValue::I16)
                                .collect(),
                        ),
                    ),
                ]),
            ),
        ]);
        let plan = plan_terrain_export("Terrain", &root, ResourceExportLimits::default()).unwrap();
        assert_eq!(plan.file_name, "Terrain.glb");
        assert_eq!(plan.family, ResourceExportFamily::Terrain);
        assert_eq!(&plan.bytes[0..4], b"glTF");
        assert!(plan.bytes.len() > 100);
        let has_texcoord = plan
            .bytes
            .windows(b"TEXCOORD_0".len())
            .any(|window| window == b"TEXCOORD_0");
        assert!(has_texcoord, "the terrain primitive carries TEXCOORD_0");
        // The scale is read from the nested heightmap.
        assert_eq!(read_heightmap_scale(&root).unwrap(), [1.0, 2.0, 4.0]);
        // An absent scale reads as the generated default.
        let no_scale = struct_value(vec![field(
            "m_Heightmap",
            struct_value(vec![
                field("m_Width", SerializedValue::I32(2)),
                field("m_Height", SerializedValue::I32(2)),
                field("m_Resolution", SerializedValue::I32(0)),
                field(
                    "m_Heights",
                    SerializedValue::Array(
                        [0i16, 1, 2, 3]
                            .into_iter()
                            .map(SerializedValue::I16)
                            .collect(),
                    ),
                ),
            ]),
        )]);
        assert_eq!(read_heightmap_scale(&no_scale).unwrap(), [0.0, 0.0, 0.0]);
    }

    #[test]
    fn navmesh_plan_reads_height_meshes_and_emits_position_only() {
        let root = struct_value(vec![field(
            "m_HeightMeshes",
            SerializedValue::Array(vec![struct_value(vec![
                field(
                    "m_Indices",
                    SerializedValue::Array(vec![
                        SerializedValue::I32(0),
                        SerializedValue::I32(1),
                        SerializedValue::I32(2),
                    ]),
                ),
                field(
                    "m_Vertices",
                    SerializedValue::Array(vec![
                        struct_value(vec![
                            field("x", SerializedValue::F32(0.0)),
                            field("y", SerializedValue::F32(0.0)),
                            field("z", SerializedValue::F32(0.0)),
                        ]),
                        struct_value(vec![
                            field("x", SerializedValue::F32(1.0)),
                            field("y", SerializedValue::F32(0.0)),
                            field("z", SerializedValue::F32(0.0)),
                        ]),
                        struct_value(vec![
                            field("x", SerializedValue::F32(0.0)),
                            field("y", SerializedValue::F32(1.0)),
                            field("z", SerializedValue::F32(0.0)),
                        ]),
                    ]),
                ),
            ])]),
        )]);
        let read = read_navmesh_height_meshes(&root).unwrap();
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].indices, vec![0, 1, 2]);
        assert_eq!(read[0].vertices.len(), 3);
        let plan = plan_navmesh_export("Nav", &root, ResourceExportLimits::default()).unwrap();
        assert_eq!(plan.file_name, "Nav.glb");
        assert_eq!(plan.family, ResourceExportFamily::NavMesh);
        assert!(!plan.bytes.windows(11).any(|window| window == b"TEXCOORD_0"));
        // A non-triangle index count is the source's explicit refusal.
        let bad = struct_value(vec![field(
            "m_HeightMeshes",
            SerializedValue::Array(vec![struct_value(vec![
                field(
                    "m_Indices",
                    SerializedValue::Array(vec![SerializedValue::I32(0), SerializedValue::I32(1)]),
                ),
                field(
                    "m_Vertices",
                    SerializedValue::Array(vec![
                        struct_value(vec![
                            field("x", SerializedValue::F32(0.0)),
                            field("y", SerializedValue::F32(0.0)),
                            field("z", SerializedValue::F32(0.0)),
                        ]),
                        struct_value(vec![
                            field("x", SerializedValue::F32(1.0)),
                            field("y", SerializedValue::F32(0.0)),
                            field("z", SerializedValue::F32(0.0)),
                        ]),
                    ]),
                ),
            ])]),
        )]);
        assert!(matches!(
            plan_navmesh_export("Nav", &bad, ResourceExportLimits::default()),
            Err(ResourceWiringError::Glb(
                GlbError::NavMeshIndexNotTriangles {
                    mesh_index: 0,
                    index_count: 2
                }
            ))
        ));
    }

    #[test]
    fn audio_plan_keeps_the_native_bytes_and_source_extension() {
        // A tracker sound type passes the bytes through under its source extension.
        let plan = plan_audio_export("Music", Some(0), b"MODDATA").unwrap();
        assert_eq!(plan.family, ResourceExportFamily::Audio);
        assert!(plan.file_name.starts_with("Music."));
        assert_eq!(plan.bytes, b"MODDATA");
        // Empty data is an explicit refusal, exactly like the CLI.
        assert!(matches!(
            plan_audio_export("Music", None, b""),
            Err(ResourceWiringError::Audio(_))
        ));
    }

    #[test]
    fn shader_plan_renders_dummy_text_and_refuses_the_yaml_mode() {
        let view = ShaderView {
            name: None,
            script: Some(b"Shader \"T\" {\nSubShader {\nProgram \"vp\" {\n}\n}\n}".to_vec()),
            has_parsed_form: false,
            parsed: None,
            non_modifiable_textures: None,
        };
        let plan = plan_shader_export("T", &view, ShaderExportMode::Dummy).unwrap();
        assert_eq!(plan.file_name, "T.shader");
        assert_eq!(plan.family, ResourceExportFamily::ShaderText);
        assert!(plan
            .bytes
            .starts_with(b"Shader \"T\" {\n\t//DummyShaderTextExporter\n"));
        assert!(matches!(
            plan_shader_export("T", &view, ShaderExportMode::Yaml),
            Err(ResourceWiringError::Shader(
                ShaderTextExportError::YamlModeWritesAsset
            ))
        ));
    }

    #[test]
    fn resource_export_dispatch_returns_none_for_unowned_classes() {
        let root = struct_value(vec![field(
            "m_Name",
            SerializedValue::StringBytes(b"X".to_vec()),
        )]);
        let request = ResourceExportRequest {
            name: "X",
            class_id: 1,
            root: &root,
            version: UnityVersion {
                major: 2019,
                minor: 4,
                patch: 0,
            },
            endian: EndianType::Little,
            stream_content: None,
            platform: 0,
            platform_blob_little_endian: true,
            image_format: ImageExportFormat::Png,
            shader_mode: ShaderExportMode::Dummy,
            limits: ResourceExportLimits::default(),
        };
        assert_eq!(plan_resource_export(&request).unwrap(), None);
        // The class-21 material text asset is owned by the YAML path.
        let material = ResourceExportRequest {
            class_id: 21,
            ..request
        };
        assert_eq!(plan_resource_export(&material).unwrap(), None);
    }
}
