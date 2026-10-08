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
/// Class-83 `AudioClip`.
pub const AUDIO_CLIP_CLASS_ID: i32 = 83;
/// Class-156 `TerrainData`.
pub const TERRAIN_CLASS_ID: i32 = 156;
/// Class-238 `NavMeshData`.
pub const NAVMESH_CLASS_ID: i32 = 238;

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
    /// Shader text (class 48).
    ShaderText,
    /// A YAML text asset owned by the project YAML path (class 21).
    MaterialText,
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
        ResourceExportFamily::ShaderText => {
            let view = ShaderView::from_value(request.root)
                .map_err(|error| ResourceWiringError::Shader(ShaderTextExportError::View(error)))?;
            plan_shader_export(request.name, &view, request.shader_mode)?
        }
        ResourceExportFamily::MaterialText => return Ok(None),
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
