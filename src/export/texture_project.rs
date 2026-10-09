//! Standalone `TextureExportCollection` project output for source-generated class-28 textures.
//!
//! The source `SpriteProcessor` gives a texture without sprites a generated
//! `SpriteInformationObject`; the collection then converts the texture and writes a
//! `TextureImporter`. This module owns the converted image and the source-versioned importer
//! body. Stream lookup stays with the project workflow, which owns the resource-file scope.

use std::error::Error;
use std::io;

use crate::processing::scene_naming::SceneUnityVersion;
use crate::resources::image_export::ImageExportFormat;
use crate::resources::texture::{export_texture2d_image, Texture2DView, TextureExportError};
use crate::serialized_file::object::SerializedValue;

use super::importer_reads::calculate_max_texture_size;
use super::settings_file::ExportSettingsFile;
use super::unity_yaml::{
    UnityYamlMappingStyle, UnityYamlNode, UnityYamlScalar, UnityYamlSequenceStyle,
};

/// The source importer layouts replayed by the committed project-export fixtures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextureImporterLayout {
    Unity2019_4,
    Unity2022_3,
}

impl TextureImporterLayout {
    pub fn for_project_version(version: SceneUnityVersion) -> Option<Self> {
        match (version.major, version.minor) {
            (2019, 4) => Some(Self::Unity2019_4),
            (2022, 3) => Some(Self::Unity2022_3),
            _ => None,
        }
    }
}

/// All values needed to plan a converted texture project file and its importer sidecar.
#[derive(Clone, Debug)]
pub struct TextureProjectPlan {
    pub view: Texture2DView,
    pub image_format: ImageExportFormat,
    pub extension: String,
    pub importer: UnityYamlNode,
}

impl TextureProjectPlan {
    pub fn export_image(
        &self,
        stream_content: Option<&[u8]>,
        platform: i32,
        body_is_little_endian: bool,
    ) -> Result<Vec<u8>, TextureExportError> {
        export_texture2d_image(
            &self.view,
            stream_content,
            platform,
            body_is_little_endian,
            true,
            self.image_format,
        )
    }
}

/// Select the source image format and build the versioned no-sprite `TextureImporter`.
///
/// Bundle-decoded workspace objects have no retained `OriginalPath`, so the source's
/// `GetBestExtension` input is `None`; a configured format therefore controls the image route.
pub fn plan_texture_project_export(
    root: &SerializedValue,
    version: SceneUnityVersion,
    settings: &ExportSettingsFile,
    asset_bundle_name: Option<&str>,
) -> Result<TextureProjectPlan, Box<dyn Error>> {
    let layout = TextureImporterLayout::for_project_version(version).ok_or_else(|| {
        invalid_data(format!(
            "TextureImporter serialization is not source-pinned for Unity {}.{}",
            version.major, version.minor
        ))
    })?;
    let view = Texture2DView::from_value(root)?;
    let image_format = ImageExportFormat::from_settings_format(settings.image_export_format);
    let extension = settings
        .image_export_format
        .texture_extension(settings.prefer_original_texture_extension, None);
    let importer = texture_importer(root, &view, layout, asset_bundle_name)?;
    Ok(TextureProjectPlan {
        view,
        image_format,
        extension,
        importer,
    })
}

fn texture_importer(
    root: &SerializedValue,
    texture: &Texture2DView,
    layout: TextureImporterLayout,
    asset_bundle_name: Option<&str>,
) -> Result<UnityYamlNode, Box<dyn Error>> {
    let facts = TextureImporterFacts::from_value(root, texture)?;
    let modern = layout == TextureImporterLayout::Unity2022_3;
    let mut importer = mapping(vec![
        ("serializedVersion", signed(if modern { 12 } else { 11 })),
        ("internalIDToNameTable", empty_sequence()),
        ("externalObjects", empty_mapping()),
        ("mipmaps", mipmaps(facts.enable_mipmap, facts.srgb_texture)?),
        ("bumpmap", bumpmap(modern)?),
        ("isReadable", boolean_int(facts.is_readable)),
        ("streamingMipmaps", boolean_int(facts.streaming_mipmaps)),
        (
            "streamingMipmapsPriority",
            signed(i64::from(facts.streaming_mipmaps_priority)),
        ),
    ])?;
    if modern {
        importer.insert_field("vTOnly", signed(0))?;
        importer.insert_field("ignoreMipmapLimit", signed(0))?;
    }
    importer.insert_field("grayScaleToAlpha", signed(0))?;
    importer.insert_field("generateCubemap", signed(6))?;
    importer.insert_field("cubemapConvolution", signed(0))?;
    importer.insert_field("seamlessCubemap", signed(0))?;
    importer.insert_field("textureFormat", signed(i64::from(facts.format)))?;
    importer.insert_field("maxTextureSize", signed(i64::from(facts.max_texture_size)))?;
    importer.insert_field("textureSettings", texture_settings(&facts)?)?;
    importer.insert_field("nPOTScale", signed(0))?;
    importer.insert_field("lightmap", signed(0))?;
    importer.insert_field("compressionQuality", signed(50))?;
    importer.insert_field("spriteMode", signed(1))?;
    importer.insert_field("spriteExtrude", signed(1))?;
    importer.insert_field("spriteMeshType", signed(0))?;
    importer.insert_field("alignment", signed(0))?;
    importer.insert_field("spritePivot", point2(0.5, 0.5)?)?;
    importer.insert_field("spritePixelsToUnits", float(100.0))?;
    importer.insert_field("spriteBorder", point4(0.0, 0.0, 0.0, 0.0)?)?;
    importer.insert_field("spriteGenerateFallbackPhysicsShape", signed(1))?;
    importer.insert_field("alphaUsage", signed(1))?;
    importer.insert_field("alphaIsTransparency", signed(1))?;
    importer.insert_field("spriteTessellationDetail", signed(-1))?;
    importer.insert_field("textureType", signed(if facts.normal_map { 1 } else { 0 }))?;
    importer.insert_field("textureShape", signed(1))?;
    importer.insert_field("singleChannelComponent", signed(0))?;
    if modern {
        importer.insert_field("flipbookRows", signed(0))?;
        importer.insert_field("flipbookColumns", signed(0))?;
    }
    importer.insert_field("maxTextureSizeSet", signed(0))?;
    importer.insert_field("compressionQualitySet", signed(0))?;
    importer.insert_field("textureFormatSet", signed(0))?;
    if modern {
        importer.insert_field("ignorePngGamma", signed(0))?;
    }
    importer.insert_field("applyGammaDecoding", signed(0))?;
    if modern {
        importer.insert_field("swizzle", signed(50_462_976))?;
        importer.insert_field("cookieLightType", signed(0))?;
    }
    importer.insert_field(
        "platformSettings",
        platform_settings(facts.max_texture_size, modern)?,
    )?;
    importer.insert_field("spriteSheet", sprite_sheet(modern)?)?;
    if modern {
        importer.insert_field("mipmapLimitGroupName", UnityYamlNode::string(""))?;
    } else {
        importer.insert_field("spritePackingTag", UnityYamlNode::string(""))?;
    }
    importer.insert_field("output", texture_output(modern)?)?;
    importer.insert_field("pSDRemoveMatte", signed(0))?;
    if !modern {
        importer.insert_field("pSDShowRemoveMatteOption", signed(0))?;
    }
    importer.insert_field("userData", UnityYamlNode::string(""))?;
    importer.insert_field(
        "assetBundleName",
        UnityYamlNode::string(asset_bundle_name.unwrap_or("")),
    )?;
    importer.insert_field("assetBundleVariant", UnityYamlNode::string(""))?;
    Ok(importer)
}

#[derive(Clone, Copy, Debug)]
struct TextureImporterFacts {
    enable_mipmap: bool,
    srgb_texture: bool,
    is_readable: bool,
    streaming_mipmaps: bool,
    streaming_mipmaps_priority: i32,
    format: i32,
    max_texture_size: i32,
    filter_mode: i32,
    aniso: i32,
    mip_bias: f32,
    wrap_u: i32,
    wrap_v: i32,
    wrap_w: i32,
    normal_map: bool,
}

impl TextureImporterFacts {
    fn from_value(root: &SerializedValue, texture: &Texture2DView) -> Result<Self, Box<dyn Error>> {
        let color_space = optional_i32(root, &["m_ColorSpace", "ColorSpace_C28E"])?.unwrap_or(0);
        let streaming_mipmaps =
            optional_bool(root, &["m_StreamingMipmaps", "StreamingMipmaps_C28"])?.unwrap_or(false);
        let streaming_mipmaps_priority = optional_i32(
            root,
            &["m_StreamingMipmapsPriority", "StreamingMipmapsPriority_C28"],
        )?
        .unwrap_or(0);
        let is_readable =
            optional_bool(root, &["m_IsReadable", "IsReadable_C28"])?.unwrap_or(false);
        let width = i32::try_from(texture.width)
            .map_err(|_| invalid_data("Texture2D width exceeds importer integer range"))?;
        let height = i32::try_from(texture.height)
            .map_err(|_| invalid_data("Texture2D height exceeds importer integer range"))?;
        let texture_settings = field(root, &["m_TextureSettings", "TextureSettings_C28"])?;
        let get_setting = |names: &[&str]| -> Result<i32, Box<dyn Error>> {
            optional_i32_from(texture_settings, names).map(|value| value.unwrap_or(0))
        };
        let get_wrap = |axis: &str| -> Result<i32, Box<dyn Error>> {
            let axis_name = match axis {
                "U" => "m_WrapU",
                "V" => "m_WrapV",
                _ => "m_WrapW",
            };
            Ok(optional_i32_from(texture_settings, &[axis_name, "m_WrapMode"])?.unwrap_or(0))
        };
        Ok(Self {
            enable_mipmap: texture.has_mips(),
            srgb_texture: color_space == 1,
            is_readable,
            streaming_mipmaps,
            streaming_mipmaps_priority,
            format: texture.texture_format,
            max_texture_size: calculate_max_texture_size(width, height),
            filter_mode: get_setting(&["m_FilterMode", "FilterMode"])?,
            aniso: get_setting(&["m_Aniso", "Aniso"])?,
            mip_bias: optional_f32_from(texture_settings, &["m_MipBias", "MipBias"])?
                .unwrap_or(0.0),
            wrap_u: get_wrap("U")?,
            wrap_v: get_wrap("V")?,
            wrap_w: get_wrap("W")?,
            normal_map: matches!(texture.lightmap_format, 3 | 4 | 10),
        })
    }
}

fn mipmaps(enable_mipmap: bool, srgb_texture: bool) -> Result<UnityYamlNode, Box<dyn Error>> {
    mapping(vec![
        ("mipMapMode", signed(0)),
        ("enableMipMap", boolean_int(enable_mipmap)),
        ("sRGBTexture", boolean_int(srgb_texture)),
        ("linearTexture", signed(0)),
        ("fadeOut", signed(0)),
        ("borderMipMap", signed(0)),
        ("mipMapsPreserveCoverage", signed(0)),
        ("alphaTestReferenceValue", float(0.5)),
        ("mipMapFadeDistanceStart", signed(1)),
        ("mipMapFadeDistanceEnd", signed(3)),
    ])
}

fn bumpmap(modern: bool) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut node = mapping(vec![
        ("convertToNormalMap", signed(0)),
        ("externalNormalMap", signed(0)),
        ("heightScale", float(0.25)),
        ("normalMapFilter", signed(0)),
    ])?;
    if modern {
        node.insert_field("flipGreenChannel", signed(0))?;
    }
    Ok(node)
}

fn texture_settings(facts: &TextureImporterFacts) -> Result<UnityYamlNode, Box<dyn Error>> {
    mapping(vec![
        ("serializedVersion", signed(2)),
        ("filterMode", signed(i64::from(facts.filter_mode))),
        ("aniso", signed(i64::from(facts.aniso))),
        ("mipBias", float(f64::from(facts.mip_bias))),
        ("wrapU", signed(i64::from(facts.wrap_u))),
        ("wrapV", signed(i64::from(facts.wrap_v))),
        ("wrapW", signed(i64::from(facts.wrap_w))),
    ])
}

fn platform_settings(max_texture_size: i32, modern: bool) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut item = mapping(vec![
        ("serializedVersion", signed(3)),
        (
            "buildTarget",
            UnityYamlNode::plain_string("DefaultTexturePlatform"),
        ),
        ("maxTextureSize", signed(i64::from(max_texture_size))),
        ("resizeAlgorithm", signed(0)),
        ("textureFormat", signed(-1)),
        ("textureCompression", signed(2)),
        ("compressionQuality", signed(50)),
        ("crunchedCompression", signed(0)),
        ("allowsAlphaSplitting", signed(0)),
        ("overridden", signed(0)),
    ])?;
    if modern {
        item.insert_field("ignorePlatformSupport", signed(0))?;
    }
    item.insert_field("androidETC2FallbackOverride", signed(0))?;
    item.insert_field("forceMaximumCompressionQuality_BC6H_BC7", signed(0))?;
    sequence_block(vec![item])
}

fn sprite_sheet(modern: bool) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut node = mapping(vec![
        ("serializedVersion", signed(2)),
        ("sprites", empty_sequence()),
        ("outline", empty_sequence()),
        ("physicsShape", empty_sequence()),
        ("bones", empty_sequence()),
        ("spriteID", UnityYamlNode::string("")),
        ("internalID", signed(0)),
        ("vertices", empty_sequence()),
        ("indices", UnityYamlNode::string("")),
        ("edges", empty_sequence()),
        ("weights", empty_sequence()),
        ("secondaryTextures", empty_sequence()),
    ])?;
    if modern {
        node.insert_field("nameFileIdTable", empty_mapping())?;
    }
    Ok(node)
}

fn texture_output(modern: bool) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut instructions = mapping(vec![
        ("compressedFormat", signed(0)),
        ("uncompressedFormat", signed(0)),
        ("desiredFormat", signed(0)),
        ("usageMode", signed(0)),
        ("colorSpace", signed(0)),
        ("androidETC2FallbackFormat", signed(0)),
        ("width", signed(0)),
        ("height", signed(0)),
    ])?;
    if modern {
        instructions.insert_field("depth", signed(0))?;
        instructions.insert_field("cubeIntermediateSize", signed(0))?;
        instructions.insert_field("cubeMode", signed(0))?;
        instructions.insert_field("cubeLayout", signed(0))?;
    }
    instructions.insert_field("compressionQuality", signed(0))?;
    instructions.insert_field("androidETC2FallbackDownscale", signed(0))?;
    if modern {
        instructions.insert_field("vtOnly", signed(0))?;
    }
    mapping(vec![
        ("textureImportInstructions", instructions),
        (
            "sourceTextureInformation",
            mapping(vec![
                ("width", signed(0)),
                ("height", signed(0)),
                ("doesTextureContainAlpha", signed(0)),
                ("sourceWasHDR", signed(0)),
            ])?,
        ),
        ("importInspectorWarnings", UnityYamlNode::string("")),
    ])
}

fn point2(x: f32, y: f32) -> Result<UnityYamlNode, Box<dyn Error>> {
    mapping_style(
        UnityYamlMappingStyle::Flow,
        vec![("x", float(f64::from(x))), ("y", float(f64::from(y)))],
    )
}

fn point4(x: f32, y: f32, z: f32, w: f32) -> Result<UnityYamlNode, Box<dyn Error>> {
    mapping_style(
        UnityYamlMappingStyle::Flow,
        vec![
            ("x", float(f64::from(x))),
            ("y", float(f64::from(y))),
            ("z", float(f64::from(z))),
            ("w", float(f64::from(w))),
        ],
    )
}

fn mapping(fields: Vec<(&str, UnityYamlNode)>) -> Result<UnityYamlNode, Box<dyn Error>> {
    mapping_style(UnityYamlMappingStyle::Block, fields)
}

fn mapping_style(
    style: UnityYamlMappingStyle,
    fields: Vec<(&str, UnityYamlNode)>,
) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut node = UnityYamlNode::mapping(style);
    for (name, value) in fields {
        node.insert_field(name, value)?;
    }
    Ok(node)
}

fn sequence_block(items: Vec<UnityYamlNode>) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut sequence = UnityYamlNode::sequence(UnityYamlSequenceStyle::Block);
    for item in items {
        sequence.push(item)?;
    }
    Ok(sequence)
}

fn empty_mapping() -> UnityYamlNode {
    UnityYamlNode::mapping(UnityYamlMappingStyle::Flow)
}

fn empty_sequence() -> UnityYamlNode {
    UnityYamlNode::sequence(UnityYamlSequenceStyle::Flow)
}

fn signed(value: i64) -> UnityYamlNode {
    UnityYamlNode::scalar(UnityYamlScalar::Signed(value))
}

fn boolean_int(value: bool) -> UnityYamlNode {
    signed(i64::from(value))
}

fn float(value: f64) -> UnityYamlNode {
    UnityYamlNode::scalar(UnityYamlScalar::Float64(value))
}

fn optional_i32(root: &SerializedValue, names: &[&str]) -> Result<Option<i32>, Box<dyn Error>> {
    optional_i32_from(Some(root), names)
}

fn optional_i32_from(
    root: Option<&SerializedValue>,
    names: &[&str],
) -> Result<Option<i32>, Box<dyn Error>> {
    let Some(value) = root
        .and_then(|root| field(root, names).transpose())
        .transpose()?
    else {
        return Ok(None);
    };
    let number = match value {
        SerializedValue::I8(number) => i32::from(*number),
        SerializedValue::U8(number) => i32::from(*number),
        SerializedValue::I16(number) => i32::from(*number),
        SerializedValue::U16(number) => i32::from(*number),
        SerializedValue::I32(number) => *number,
        SerializedValue::U32(number) => i32::try_from(*number).map_err(|_| {
            invalid_data(format!("texture importer field {:?} exceeds i32", names[0]))
        })?,
        _ => {
            return Err(invalid_data(format!(
                "texture importer field {:?} is not an integer",
                names[0]
            )))
        }
    };
    Ok(Some(number))
}

fn optional_bool(root: &SerializedValue, names: &[&str]) -> Result<Option<bool>, Box<dyn Error>> {
    let Some(value) = field(root, names)? else {
        return Ok(None);
    };
    match value {
        SerializedValue::Bool(value) => Ok(Some(*value)),
        SerializedValue::I32(value) => Ok(Some(*value != 0)),
        SerializedValue::U8(value) => Ok(Some(*value != 0)),
        _ => Err(invalid_data(format!(
            "texture importer field {:?} is not a boolean",
            names[0]
        ))),
    }
}

fn optional_f32_from(
    root: Option<&SerializedValue>,
    names: &[&str],
) -> Result<Option<f32>, Box<dyn Error>> {
    let Some(value) = root
        .and_then(|root| field(root, names).transpose())
        .transpose()?
    else {
        return Ok(None);
    };
    match value {
        SerializedValue::F32(value) => Ok(Some(*value)),
        SerializedValue::F64(value) => Ok(Some(*value as f32)),
        _ => Err(invalid_data(format!(
            "texture importer field {:?} is not a floating-point value",
            names[0]
        ))),
    }
}

fn field<'a>(
    root: &'a SerializedValue,
    names: &[&str],
) -> Result<Option<&'a SerializedValue>, Box<dyn Error>> {
    let mut found = None;
    for name in names {
        match root
            .unique_field(name)
            .map_err(|error| Box::new(error) as Box<dyn Error>)?
        {
            Some(_) if found.is_some() => {
                return Err(invalid_data(format!(
                    "texture importer aliases {:?} are both present",
                    names
                )))
            }
            Some(value) => found = Some(value),
            None => {}
        }
    }
    Ok(found)
}

fn invalid_data(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(io::Error::new(io::ErrorKind::InvalidData, message.into()))
}
