//! Asset-field reads behind every `CreateImporter` path in the frozen baseline.
//!
//! The importer *classes* come from the external `AssetRipper.SourceGenerated` package, so
//! their member lists, `Has_*` thresholds, and YAML serialization stay open. What the
//! snapshot determines exactly — and what this module pins — is the data flow on the
//! collection side: which asset fields each override reads, which gate names it consults,
//! which fixed values it writes, and the three pure selectors (`CalculateMaxTextureSize`
//! and the cubemap/sprite branches) whose inputs and outputs are all snapshot facts.
//!
//! Two genuine source asymmetries are pinned rather than normalized:
//!
//! - `GenerateIHVImporter` and the modern `PrefabExportCollection` branch copy
//!   `AssetBundleName` on a bare null check, with no `Has_AssetBundleName_R` gate, while
//!   every other importer path gates on it.
//! - Float-to-int casts (`FontSize`, `EndFrame`) truncate toward zero for sane finite
//!   values; NaN and out-of-range inputs stay open because neither runtime edge is pinned
//!   by a snapshot test and the two runtimes differ there.

/// One asset-field read performed while building an importer.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ImporterAssetRead {
    /// The importer object receiving the value.
    pub importer: &'static str,
    /// The source asset field or expression being read.
    pub reads: &'static str,
    /// The gate consulted first, as written in the snapshot (`"none"` when unconditional).
    pub gate: &'static str,
    /// The assignment or call the read feeds.
    pub effect: &'static str,
}

/// `ImporterFactory.CalculateMaxTextureSize`: round the longest side up to a power of two,
/// then clamp to Unity's 2048..=16384 texture-size window.
///
/// The power-of-two result is cast back to a signed integer *before* clamping, exactly like
/// the source `(int)` cast, so gigantic sides wrap negative and bottom out at the minimum.
pub fn calculate_max_texture_size(width: i32, height: i32) -> i32 {
    let longest = width.max(height) as u32;
    (round_up_to_power_of_2(longest) as i32).clamp(2048, 16384)
}

/// `BitOperations.RoundUpToPowerOf2` for `uint`, including the wraparound to zero.
fn round_up_to_power_of_2(value: u32) -> u32 {
    let mut rounded = value.wrapping_sub(1);
    rounded |= rounded >> 1;
    rounded |= rounded >> 2;
    rounded |= rounded >> 4;
    rounded |= rounded >> 8;
    rounded |= rounded >> 16;
    rounded.wrapping_add(1)
}

/// `TextureImporterGenerateCubemap` literal names selected in `GenerateTextureImporter`.
pub mod cubemap_literals {
    /// `origin is ICubemap`.
    pub const FULL_CUBEMAP: &str = "FullCubemap";
    /// Non-cubemap with `TextureShape` support.
    pub const AUTO_CUBEMAP: &str = "AutoCubemap";
    /// Non-cubemap without `TextureShape` support.
    pub const NONE: &str = "None";
}

/// The cubemap branch: cubemaps always map fully, otherwise the `Has_TextureShape` gate picks
/// automatic mapping and its absence picks none.
pub fn select_generate_cubemap(is_cubemap: bool, has_texture_shape: bool) -> &'static str {
    if is_cubemap {
        cubemap_literals::FULL_CUBEMAP
    } else if has_texture_shape {
        cubemap_literals::AUTO_CUBEMAP
    } else {
        cubemap_literals::NONE
    }
}

/// `TextureImporterNPOTScale` literal names.
pub mod npot_literals {
    /// Cubemaps only.
    pub const TO_NEAREST: &str = "ToNearest";
    /// Everything else.
    pub const NONE: &str = "None";
}

/// Only cubemaps scale to the nearest power of two; sprites would break if scaled.
pub fn select_npot_scale(is_cubemap: bool) -> &'static str {
    if is_cubemap {
        npot_literals::TO_NEAREST
    } else {
        npot_literals::NONE
    }
}

/// `TextureImporterType` fallback names used around the data-driven texture type.
pub mod texture_type_literals {
    /// The data-driven default for non-cubemaps and shaped importers.
    pub const DATA_DRIVEN: &str = "data.TextureType";
    /// Cubemaps without `TextureShape` support.
    pub const ADVANCED: &str = "Advanced";
    /// Normal-mapped textures.
    pub const NORMAL_MAP: &str = "NormalMap";
    /// Ordinary textures.
    pub const DEFAULT: &str = "Default";
}

/// `Has_TextureShape() || origin is not ICubemap` keeps the data type; a shapeless cubemap
/// falls back to `Advanced`.
pub fn select_texture_type_source(is_cubemap: bool, has_texture_shape: bool) -> &'static str {
    if has_texture_shape || !is_cubemap {
        texture_type_literals::DATA_DRIVEN
    } else {
        texture_type_literals::ADVANCED
    }
}

/// Caller-side facts for `TextureUsageModeExtensions.IsNormalmap` in
/// `AssetRipper/Source/AssetRipper.SourceGenerated.Extensions/TextureUsageModeExtensions.cs`.
///
/// The generated `TextureUsageMode` enum stays caller-side; the caller maps its value onto
/// this kernel enum (anything that is not one of the three normal-map modes maps to `Other`)
/// and lets [`is_normalmap_usage_mode`] apply the snapshot predicate.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TextureUsageMode {
    /// `TextureUsageMode.NormalmapDXT5nm`.
    NormalmapDXT5nm,
    /// `TextureUsageMode.NormalmapPlain`.
    NormalmapPlain,
    /// `TextureUsageMode.NormalmapASTCnm`.
    NormalmapASTCnm,
    /// Every other `TextureUsageMode` value.
    Other,
}

/// `TextureUsageModeExtensions.IsNormalmap` in
/// `AssetRipper/Source/AssetRipper.SourceGenerated.Extensions/TextureUsageModeExtensions.cs`:
/// exactly `NormalmapDXT5nm`, `NormalmapPlain`, and `NormalmapASTCnm` count as normalmaps.
pub fn is_normalmap_usage_mode(mode: TextureUsageMode) -> bool {
    matches!(
        mode,
        TextureUsageMode::NormalmapDXT5nm
            | TextureUsageMode::NormalmapPlain
            | TextureUsageMode::NormalmapASTCnm
    )
}

/// The `TextureType` choice shared by all four `TextureImporterData` branches in
/// `AssetRipper/Source/AssetRipper.Export.UnityProjects/Textures/ImporterFactory.cs`
/// (lines 123-125, 141-143, 157-159, 173-175): a normal-mapped source becomes `NormalMap`,
/// everything else becomes `Default`. The caller supplies the `IsNormalmap()` fact because
/// each branch reads it off a different accessor (`LightmapFormat_C28E`, `UsageModeE`, or
/// `GetLightmapFormat()`).
pub fn select_texture_importer_type(is_normalmap: bool) -> &'static str {
    if is_normalmap {
        texture_type_literals::NORMAL_MAP
    } else {
        texture_type_literals::DEFAULT
    }
}

/// `EnableMipMap` for the `ITexture2DArray` and `ICubemapArray` branches in
/// `AssetRipper/Source/AssetRipper.Export.UnityProjects/Textures/ImporterFactory.cs`
/// (lines 133, 149): `MipCount > 1`, strictly greater — a single mip level means no mipmaps.
pub fn mip_count_enables_mipmap(mip_count: i32) -> bool {
    mip_count > 1
}

/// `EnableMipMap` for the `ITexture3D` branch in
/// `AssetRipper/Source/AssetRipper.Export.UnityProjects/Textures/ImporterFactory.cs`
/// (line 165): `Has_MipCount() && MipCount > 1 || Has_MipMap() && MipMap`.
///
/// `&&` binds tighter than `||`, so either mip source can enable mipmaps on its own, and an
/// absent field contributes `false` through its `Has_*` gate without being read.
pub fn select_texture_3d_enable_mipmap(
    has_mip_count: bool,
    mip_count: i32,
    has_mip_map: bool,
    mip_map: bool,
) -> bool {
    (has_mip_count && mip_count > 1) || (has_mip_map && mip_map)
}

/// `TextureImporterShape` literal names selected by the `TextureImporterData` branches in
/// `AssetRipper/Source/AssetRipper.Export.UnityProjects/Textures/ImporterFactory.cs`.
pub mod texture_shape_literals {
    /// `ITexture2D` branch when the texture is not a cubemap.
    pub const TEXTURE_2D: &str = "Texture2D";
    /// `ITexture2D` branch when the texture is a cubemap.
    pub const TEXTURE_CUBE: &str = "TextureCube";
    /// `ITexture2DArray` branch, and — per the source's own
    /// `//Maybe this should be TextureCube` note — the `ICubemapArray` branch too.
    pub const TEXTURE_2D_ARRAY: &str = "Texture2DArray";
    /// `ITexture3D` branch.
    pub const TEXTURE_3D: &str = "Texture3D";
}

/// The `ITexture2D` shape branch in
/// `AssetRipper/Source/AssetRipper.Export.UnityProjects/Textures/ImporterFactory.cs`
/// (lines 126-128): `texture2D is ICubemap` selects `TextureCube`, anything else `Texture2D`.
pub fn select_texture_2d_shape(is_cubemap: bool) -> &'static str {
    if is_cubemap {
        texture_shape_literals::TEXTURE_CUBE
    } else {
        texture_shape_literals::TEXTURE_2D
    }
}

/// Fixed `TextureShape` of the `ITexture2DArray` branch in
/// `AssetRipper/Source/AssetRipper.Export.UnityProjects/Textures/ImporterFactory.cs`
/// (line 144): always `Texture2DArray`, with no asset read.
pub const TEXTURE_2D_ARRAY_IMPORTER_SHAPE: &str = texture_shape_literals::TEXTURE_2D_ARRAY;

/// Fixed `TextureShape` of the `ICubemapArray` branch in
/// `AssetRipper/Source/AssetRipper.Export.UnityProjects/Textures/ImporterFactory.cs`
/// (line 160): always `Texture2DArray`, exactly as written — the source carries a
/// `//Maybe this should be TextureCube` comment, which is preserved here as documented
/// behavior rather than "fixed" to `TextureCube`.
pub const CUBEMAP_ARRAY_IMPORTER_SHAPE: &str = texture_shape_literals::TEXTURE_2D_ARRAY;

/// Fixed `TextureShape` of the `ITexture3D` branch in
/// `AssetRipper/Source/AssetRipper.Export.UnityProjects/Textures/ImporterFactory.cs`
/// (line 176): always `Texture3D`, with no asset read.
pub const TEXTURE_3D_IMPORTER_SHAPE: &str = texture_shape_literals::TEXTURE_3D;

/// Fixed streaming facts shared by the `ITexture2DArray`, `ICubemapArray`, and `ITexture3D`
/// branches in `AssetRipper/Source/AssetRipper.Export.UnityProjects/Textures/ImporterFactory.cs`
/// (lines 135-136, 151-152, 167-168): arrays and volumes never stream — `StreamingMipmaps`
/// is `false` and the priority is `default` — with no asset read. Only the `ITexture2D`
/// branch copies `StreamingMipmaps_C28` / `StreamingMipmapsPriority_C28` off the asset.
pub const NON_2D_STREAMING_MIPMAPS: bool = false;
/// Fixed streaming priority for the non-`ITexture2D` branches (see [`NON_2D_STREAMING_MIPMAPS`]).
pub const NON_2D_STREAMING_MIPMAPS_PRIORITY: i32 = 0;

/// `SpriteImportMode` literal names for the texture sprite branches.
pub mod sprite_mode_literals {
    /// No sprites, or exactly one sprite matching the texture rect and name.
    pub const SINGLE: &str = "Single";
    /// One differing sprite, or several sprites.
    pub const MULTIPLE: &str = "Multiple";
}

/// Inputs to the `AddSprites` three-way branch that the snapshot reads off the sprite set.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SpriteBranchInput {
    /// How many sprites the texture carries: 0, 1, or more.
    pub sprite_count: usize,
    /// `sprite.Rect == sprite.RD.TextureRect && sprite.Name == texture.Name` (one-sprite case).
    pub single_sprite_matches_texture: bool,
    /// `TextureExportCollection.m_exportSprites`.
    pub export_sprites: bool,
}

/// The outcome of the `AddSprites` branch that the plan layer needs.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SpriteBranchPlan {
    /// `SpriteModeE` literal name.
    pub sprite_mode: &'static str,
    /// Whether `TextureTypeE` is forced to `Sprite`.
    pub forces_sprite_texture_type: bool,
    /// Whether the sprite sheet and ID-to-name table are appended.
    pub appends_sheet_and_id_table: bool,
}

/// The `AddSprites` branch: empty sets take plain single-sprite defaults, a lone matching
/// sprite stays single, and anything else goes multiple; sheets and ID tables follow only
/// when the collection exports sprites.
pub fn select_sprite_branch(input: SpriteBranchInput) -> SpriteBranchPlan {
    let single =
        input.sprite_count == 0 || (input.sprite_count == 1 && input.single_sprite_matches_texture);
    SpriteBranchPlan {
        sprite_mode: if single {
            sprite_mode_literals::SINGLE
        } else {
            sprite_mode_literals::MULTIPLE
        },
        forces_sprite_texture_type: input.sprite_count >= 1,
        appends_sheet_and_id_table: input.export_sprites && input.sprite_count >= 1,
    }
}

/// Asset reads of `AudioExportCollection.CreateImporter`.
pub const AUDIO_IMPORTER_READS: &[ImporterAssetRead] = &[
    ImporterAssetRead {
        importer: "AudioImporter",
        reads: "Asset.P_3D",
        gate: "none",
        effect: "importer.P_3D",
    },
    ImporterAssetRead {
        importer: "AudioImporter",
        reads: "Asset.Ambisonic",
        gate: "none",
        effect: "importer.Ambisonic",
    },
    ImporterAssetRead {
        importer: "AudioImporter",
        reads: "Asset.CompressionFormat",
        gate: "Has_DefaultSettings()",
        effect: "importer.DefaultSettings.CompressionFormat",
    },
    ImporterAssetRead {
        importer: "AudioImporter",
        reads: "Asset.LoadType",
        gate: "Has_DefaultSettings()",
        effect: "importer.DefaultSettings.LoadType",
    },
    ImporterAssetRead {
        importer: "AudioImporter",
        reads: "Asset.PreloadAudioData",
        gate: "Has_DefaultSettings()",
        effect: "importer.DefaultSettings.PreloadAudioData",
    },
    ImporterAssetRead {
        importer: "AudioImporter",
        reads: "Asset.Format",
        gate: "none",
        effect: "importer.Format",
    },
    ImporterAssetRead {
        importer: "AudioImporter",
        reads: "Asset.LoadInBackground",
        gate: "none",
        effect: "importer.LoadInBackground",
    },
    ImporterAssetRead {
        importer: "AudioImporter",
        reads: "Asset.PreloadAudioData",
        gate: "none",
        effect: "importer.PreloadAudioData",
    },
    ImporterAssetRead {
        importer: "AudioImporter",
        reads: "Asset.Stream",
        gate: "none",
        effect: "importer.Stream",
    },
    ImporterAssetRead {
        importer: "AudioImporter",
        reads: "Asset.UseHardware",
        gate: "none",
        effect: "importer.UseHardware",
    },
    ImporterAssetRead {
        importer: "AudioImporter",
        reads: "Asset.AssetBundleName",
        gate: "Has_AssetBundleName_R() and non-null",
        effect: "importer.AssetBundleName_R",
    },
];

/// Asset reads of `FontAssetExportCollection.CreateImporter`.
pub const FONT_IMPORTER_READS: &[ImporterAssetRead] = &[
    ImporterAssetRead {
        importer: "TrueTypeFontImporter",
        reads: "origin.FontSize",
        gate: "none",
        effect: "importer.FontSize as an int cast",
    },
    ImporterAssetRead {
        importer: "TrueTypeFontImporter",
        reads: "origin.FontNames[0]",
        gate: "FontNames.Count > 0",
        effect: "importer.FontName",
    },
    ImporterAssetRead {
        importer: "TrueTypeFontImporter",
        reads: "origin.FontNames (every entry)",
        gate: "FontNames.Count > 0",
        effect: "importer.FontNames append",
    },
    ImporterAssetRead {
        importer: "TrueTypeFontImporter",
        reads: "origin.FallbackFonts via PPtrConverter",
        gate: "Has_FallbackFonts() and Has_FallbackFontReferences()",
        effect: "importer.FallbackFontReferences AddNew with copied values",
    },
    ImporterAssetRead {
        importer: "TrueTypeFontImporter",
        reads: "origin.CharacterSpacing",
        gate: "none",
        effect: "importer.CharacterSpacing",
    },
    ImporterAssetRead {
        importer: "TrueTypeFontImporter",
        reads: "origin.CharacterPadding",
        gate: "none",
        effect: "importer.CharacterPadding",
    },
    ImporterAssetRead {
        importer: "TrueTypeFontImporter",
        reads: "origin.FontRenderingMode",
        gate: "none",
        effect: "importer.FontRenderingMode",
    },
    ImporterAssetRead {
        importer: "TrueTypeFontImporter",
        reads: "origin.UseLegacyBoundsCalculation",
        gate: "none",
        effect: "importer.UseLegacyBoundsCalculation",
    },
    ImporterAssetRead {
        importer: "TrueTypeFontImporter",
        reads: "origin.ShouldRoundAdvanceValue",
        gate: "none",
        effect: "importer.ShouldRoundAdvanceValue",
    },
    ImporterAssetRead {
        importer: "TrueTypeFontImporter",
        reads: "Asset.AssetBundleName",
        gate: "Has_AssetBundleName_R() and non-null",
        effect: "instance.AssetBundleName_R",
    },
];

/// Asset reads of `VideoClipExportCollection.CreateImporter`.
pub const VIDEO_IMPORTER_READS: &[ImporterAssetRead] = &[
    ImporterAssetRead {
        importer: "VideoClipImporter",
        reads: "Asset.FrameCount",
        gate: "none",
        effect: "importer.EndFrame as an int cast",
    },
    ImporterAssetRead {
        importer: "VideoClipImporter",
        reads: "Asset.Height",
        gate: "Has_SourceFileSize()",
        effect: "importer.OriginalHeight as an int cast",
    },
    ImporterAssetRead {
        importer: "VideoClipImporter",
        reads: "Asset.Width",
        gate: "Has_SourceFileSize()",
        effect: "importer.OriginalWidth as an int cast",
    },
    ImporterAssetRead {
        importer: "VideoClipImporter",
        reads: "Asset.ExternalResources.Size",
        gate: "Has_SourceFileSize()",
        effect: "importer.SourceFileSize",
    },
    ImporterAssetRead {
        importer: "VideoClipImporter",
        reads: "Asset.FrameRate",
        gate: "none",
        effect: "importer.FrameRate",
    },
    ImporterAssetRead {
        importer: "VideoClipImporter",
        reads: "importer.EndFrame",
        gate: "none",
        effect: "importer.FrameCount always equals EndFrame",
    },
    ImporterAssetRead {
        importer: "VideoClipImporter",
        reads: "Asset.AssetBundleName",
        gate: "Has_AssetBundleName_R() and non-null",
        effect: "importer.AssetBundleName_R",
    },
];

/// Asset reads of `ShaderExportCollection.CreateImporter`.
pub const SHADER_IMPORTER_READS: &[ImporterAssetRead] = &[
    ImporterAssetRead {
        importer: "ShaderImporter",
        reads: "Asset.NonModifiableTextures via PPtrConverter",
        gate: "Has_NonModifiableTextures() on both importer and asset",
        effect: "importer.NonModifiableTextures AddNew key/value pairs",
    },
    ImporterAssetRead {
        importer: "ShaderImporter",
        reads: "Asset.AssetBundleName",
        gate: "Has_AssetBundleName_R() and non-null",
        effect: "importer.AssetBundleName_R",
    },
];

/// Asset reads feeding `TextureImporterData` for `ITexture2D` assets.
pub const TEXTURE_2D_DATA_READS: &[ImporterAssetRead] = &[
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "texture2D.Mips",
        gate: "none",
        effect: "EnableMipMap",
    },
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "texture2D.ColorSpace_C28E == ColorSpace.Linear",
        gate: "none",
        effect: "SRGBTexture",
    },
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "texture2D.StreamingMipmaps_C28",
        gate: "none",
        effect: "StreamingMipmaps",
    },
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "texture2D.StreamingMipmapsPriority_C28",
        gate: "none",
        effect: "StreamingMipmapsPriority",
    },
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "texture2D.IsReadable_C28",
        gate: "none",
        effect: "IsReadable",
    },
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "texture2D.Format_C28E",
        gate: "none",
        effect: "Format",
    },
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "texture2D.Width_C28 and Height_C28",
        gate: "none",
        effect: "MaxTextureSize via CalculateMaxTextureSize",
    },
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "texture2D.TextureSettings_C28",
        gate: "none",
        effect: "TextureSettings",
    },
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "texture2D.LightmapFormat_C28E.IsNormalmap()",
        gate: "none",
        effect: "TextureType NormalMap, else Default",
    },
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "texture2D is ICubemap",
        gate: "none",
        effect: "TextureShape TextureCube, else Texture2D",
    },
];

/// Asset reads feeding `TextureImporterData` for `ITexture2DArray` assets.
pub const TEXTURE_2D_ARRAY_DATA_READS: &[ImporterAssetRead] = &[
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "texture2DArray.MipCount > 1",
        gate: "none",
        effect: "EnableMipMap",
    },
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "texture2DArray.ColorSpaceE == ColorSpace.Linear",
        gate: "none",
        effect: "SRGBTexture",
    },
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "texture2DArray.IsReadable",
        gate: "none",
        effect: "IsReadable",
    },
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "texture2DArray.FormatE",
        gate: "none",
        effect: "Format",
    },
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "texture2DArray.Width and Height",
        gate: "none",
        effect: "MaxTextureSize via CalculateMaxTextureSize",
    },
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "texture2DArray.TextureSettings",
        gate: "none",
        effect: "TextureSettings",
    },
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "texture2DArray.UsageModeE.IsNormalmap()",
        gate: "Has_UsageMode()",
        effect: "TextureType NormalMap, else Default",
    },
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "branch fixed (no asset field)",
        gate: "none",
        effect: "TextureShape Texture2DArray",
    },
];

/// Asset reads feeding `TextureImporterData` for `ICubemapArray` assets.
///
/// Note the source passes `Width` twice to `CalculateMaxTextureSize`; the table records
/// the call as written rather than correcting it to width and height.
pub const CUBEMAP_ARRAY_DATA_READS: &[ImporterAssetRead] = &[
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "cubemapArray.MipCount > 1",
        gate: "none",
        effect: "EnableMipMap",
    },
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "cubemapArray.ColorSpaceE == ColorSpace.Linear",
        gate: "none",
        effect: "SRGBTexture",
    },
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "cubemapArray.IsReadable",
        gate: "none",
        effect: "IsReadable",
    },
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "cubemapArray.FormatE",
        gate: "none",
        effect: "Format",
    },
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "cubemapArray.Width and Width",
        gate: "none",
        effect: "MaxTextureSize via CalculateMaxTextureSize",
    },
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "cubemapArray.TextureSettings",
        gate: "none",
        effect: "TextureSettings",
    },
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "cubemapArray.UsageModeE.IsNormalmap()",
        gate: "Has_UsageMode()",
        effect: "TextureType NormalMap, else Default",
    },
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "branch fixed (no asset field; source notes 'Maybe this should be TextureCube')",
        gate: "none",
        effect: "TextureShape Texture2DArray",
    },
];

/// Asset reads feeding `TextureImporterData` for `ITexture3D` assets.
pub const TEXTURE_3D_DATA_READS: &[ImporterAssetRead] = &[
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "texture3D.MipCount > 1",
        gate: "Has_MipCount()",
        effect: "EnableMipMap when either mip source reports mipmaps",
    },
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "texture3D.MipMap",
        gate: "Has_MipMap()",
        effect: "EnableMipMap when either mip source reports mipmaps",
    },
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "texture3D.ColorSpaceE == ColorSpace.Linear",
        gate: "none",
        effect: "SRGBTexture",
    },
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "texture3D.IsReadable",
        gate: "none",
        effect: "IsReadable",
    },
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "texture3D.GetTextureFormat()",
        gate: "none",
        effect: "Format",
    },
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "texture3D.Width and Height",
        gate: "none",
        effect: "MaxTextureSize via CalculateMaxTextureSize",
    },
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "texture3D.TextureSettings",
        gate: "none",
        effect: "TextureSettings",
    },
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "texture3D.GetLightmapFormat().IsNormalmap()",
        gate: "none",
        effect: "TextureType NormalMap, else Default",
    },
    ImporterAssetRead {
        importer: "TextureImporterData",
        reads: "branch fixed (no asset field)",
        gate: "none",
        effect: "TextureShape Texture3D",
    },
];

/// Asset reads of `ImporterFactory.GenerateIHVImporter`.
///
/// The bundle copy here has no `Has_AssetBundleName_R` gate: a bare null check only.
pub const IHV_IMPORTER_READS: &[ImporterAssetRead] = &[
    ImporterAssetRead {
        importer: "IHVImageFormatImporter",
        reads: "origin.IsReadable_C28",
        gate: "none",
        effect: "instance.IsReadable after SetToDefault",
    },
    ImporterAssetRead {
        importer: "IHVImageFormatImporter",
        reads: "origin.ColorSpace_C28E == ColorSpace.Linear",
        gate: "none",
        effect: "instance.SRGBTexture after SetToDefault",
    },
    ImporterAssetRead {
        importer: "IHVImageFormatImporter",
        reads: "origin.StreamingMipmaps_C28",
        gate: "none",
        effect: "instance.StreamingMipmaps after SetToDefault",
    },
    ImporterAssetRead {
        importer: "IHVImageFormatImporter",
        reads: "origin.StreamingMipmapsPriority_C28",
        gate: "none",
        effect: "instance.StreamingMipmapsPriority after SetToDefault",
    },
    ImporterAssetRead {
        importer: "IHVImageFormatImporter",
        reads: "origin.AssetBundleName",
        gate: "non-null only, no Has_ gate",
        effect: "instance.AssetBundleName_R",
    },
];

/// Asset reads of the modern `PrefabExportCollection.CreateImporter` branch.
///
/// Like the IHV path, the bundle copy has no `Has_AssetBundleName_R` gate.
pub const PREFAB_IMPORTER_READS: &[ImporterAssetRead] = &[ImporterAssetRead {
    importer: "PrefabImporter",
    reads: "RootGameObject.AssetBundleName",
    gate: "non-null only, no Has_ gate",
    effect: "importer.AssetBundleName_R",
}];

#[cfg(test)]
mod tests {
    use super::{
        calculate_max_texture_size, is_normalmap_usage_mode, mip_count_enables_mipmap,
        select_generate_cubemap, select_npot_scale, select_sprite_branch, select_texture_2d_shape,
        select_texture_3d_enable_mipmap, select_texture_importer_type, select_texture_type_source,
        texture_shape_literals, SpriteBranchInput, TextureUsageMode, AUDIO_IMPORTER_READS,
        CUBEMAP_ARRAY_DATA_READS, CUBEMAP_ARRAY_IMPORTER_SHAPE, FONT_IMPORTER_READS,
        IHV_IMPORTER_READS, NON_2D_STREAMING_MIPMAPS, NON_2D_STREAMING_MIPMAPS_PRIORITY,
        PREFAB_IMPORTER_READS, SHADER_IMPORTER_READS, TEXTURE_2D_ARRAY_DATA_READS,
        TEXTURE_2D_ARRAY_IMPORTER_SHAPE, TEXTURE_2D_DATA_READS, TEXTURE_3D_DATA_READS,
        TEXTURE_3D_IMPORTER_SHAPE, VIDEO_IMPORTER_READS,
    };

    #[test]
    fn max_texture_size_rounds_up_and_clamps_to_unity_window() {
        // Degenerate and tiny inputs bottom out at the 2048 minimum.
        assert_eq!(calculate_max_texture_size(0, 0), 2048);
        assert_eq!(calculate_max_texture_size(1, 1), 2048);
        assert_eq!(calculate_max_texture_size(100, 100), 2048);
        assert_eq!(calculate_max_texture_size(2048, 100), 2048);
        assert_eq!(calculate_max_texture_size(-5, 100), 2048);
        // Powers of two pass through; other widths round up.
        assert_eq!(calculate_max_texture_size(2049, 100), 4096);
        assert_eq!(calculate_max_texture_size(3000, 100), 4096);
        assert_eq!(calculate_max_texture_size(4096, 4096), 4096);
        // The longest side drives the result, capped at 16384.
        assert_eq!(calculate_max_texture_size(100, 9000), 16384);
        assert_eq!(calculate_max_texture_size(16384, 16384), 16384);
        assert_eq!(calculate_max_texture_size(20000, 100), 16384);
        // Gigantic sides wrap negative through the signed cast and bottom out, like the source.
        assert_eq!(calculate_max_texture_size(1_073_741_825, 1), 2048);
    }

    #[test]
    fn cubemap_branches_match_importer_factory_conditions() {
        assert_eq!(select_generate_cubemap(true, true), "FullCubemap");
        assert_eq!(select_generate_cubemap(true, false), "FullCubemap");
        assert_eq!(select_generate_cubemap(false, true), "AutoCubemap");
        assert_eq!(select_generate_cubemap(false, false), "None");
        assert_eq!(select_npot_scale(true), "ToNearest");
        assert_eq!(select_npot_scale(false), "None");
        // Only a shapeless cubemap falls back to Advanced; everything else uses the data type.
        assert_eq!(select_texture_type_source(true, false), "Advanced");
        assert_eq!(select_texture_type_source(true, true), "data.TextureType");
        assert_eq!(select_texture_type_source(false, false), "data.TextureType");
        assert_eq!(select_texture_type_source(false, true), "data.TextureType");
    }

    #[test]
    fn sprite_branches_match_add_sprites_conditions() {
        let empty = select_sprite_branch(SpriteBranchInput {
            sprite_count: 0,
            single_sprite_matches_texture: false,
            export_sprites: true,
        });
        assert_eq!(empty.sprite_mode, "Single");
        assert!(!empty.forces_sprite_texture_type);
        assert!(!empty.appends_sheet_and_id_table);
        let matching = select_sprite_branch(SpriteBranchInput {
            sprite_count: 1,
            single_sprite_matches_texture: true,
            export_sprites: true,
        });
        assert_eq!(matching.sprite_mode, "Single");
        assert!(matching.forces_sprite_texture_type);
        assert!(matching.appends_sheet_and_id_table);
        let differing = select_sprite_branch(SpriteBranchInput {
            sprite_count: 1,
            single_sprite_matches_texture: false,
            export_sprites: true,
        });
        assert_eq!(differing.sprite_mode, "Multiple");
        assert!(differing.forces_sprite_texture_type);
        assert!(differing.appends_sheet_and_id_table);
        // Sheets and ID tables follow only when the collection exports sprites.
        let withheld = select_sprite_branch(SpriteBranchInput {
            sprite_count: 3,
            single_sprite_matches_texture: false,
            export_sprites: false,
        });
        assert_eq!(withheld.sprite_mode, "Multiple");
        assert!(withheld.forces_sprite_texture_type);
        assert!(!withheld.appends_sheet_and_id_table);
    }

    #[test]
    fn read_tables_cover_every_importer_path_with_named_gates() {
        // Row counts pin the tables against silent row loss.
        assert_eq!(AUDIO_IMPORTER_READS.len(), 11);
        assert_eq!(FONT_IMPORTER_READS.len(), 10);
        assert_eq!(VIDEO_IMPORTER_READS.len(), 7);
        assert_eq!(SHADER_IMPORTER_READS.len(), 2);
        assert_eq!(TEXTURE_2D_DATA_READS.len(), 10);
        assert_eq!(TEXTURE_2D_ARRAY_DATA_READS.len(), 8);
        assert_eq!(CUBEMAP_ARRAY_DATA_READS.len(), 8);
        assert_eq!(TEXTURE_3D_DATA_READS.len(), 9);
        assert_eq!(IHV_IMPORTER_READS.len(), 5);
        assert_eq!(PREFAB_IMPORTER_READS.len(), 1);
        // The default-settings gate guards exactly the three sample-settings copies.
        let gated: Vec<&str> = AUDIO_IMPORTER_READS
            .iter()
            .filter(|row| row.gate == "Has_DefaultSettings()")
            .map(|row| row.reads)
            .collect();
        assert_eq!(
            gated,
            [
                "Asset.CompressionFormat",
                "Asset.LoadType",
                "Asset.PreloadAudioData",
            ]
        );
        // The video dimension triple shares one gate.
        assert!(
            VIDEO_IMPORTER_READS
                .iter()
                .filter(|row| row.gate == "Has_SourceFileSize()")
                .count()
                == 3
        );
        // The texture-shape normal-map gate appears on both array kinds.
        for table in [TEXTURE_2D_ARRAY_DATA_READS, CUBEMAP_ARRAY_DATA_READS] {
            assert!(table.iter().any(|row| row.gate == "Has_UsageMode()"));
        }
        // The cubemap-array width-doubling quirk is recorded verbatim.
        assert!(CUBEMAP_ARRAY_DATA_READS
            .iter()
            .any(|row| row.reads == "cubemapArray.Width and Width"));
        // Every non-2D branch pins its fixed TextureShape; the cubemap-array row keeps
        // the source's 'Maybe this should be TextureCube' note instead of correcting it.
        assert!(TEXTURE_2D_ARRAY_DATA_READS
            .iter()
            .any(|row| row.effect == "TextureShape Texture2DArray" && row.gate == "none"));
        assert!(CUBEMAP_ARRAY_DATA_READS
            .iter()
            .any(|row| row.effect == "TextureShape Texture2DArray"
                && row.reads.contains("Maybe this should be TextureCube")));
        assert!(TEXTURE_3D_DATA_READS
            .iter()
            .any(|row| row.effect == "TextureShape Texture3D" && row.gate == "none"));
        // The two ungated bundle copies are pinned, not normalized.
        for table in [IHV_IMPORTER_READS, PREFAB_IMPORTER_READS] {
            assert!(table
                .iter()
                .any(|row| row.gate == "non-null only, no Has_ gate"));
        }
    }

    #[test]
    fn normalmap_predicate_matches_usage_mode_extension() {
        // Exactly the three modes named in TextureUsageModeExtensions.IsNormalmap.
        assert!(is_normalmap_usage_mode(TextureUsageMode::NormalmapDXT5nm));
        assert!(is_normalmap_usage_mode(TextureUsageMode::NormalmapPlain));
        assert!(is_normalmap_usage_mode(TextureUsageMode::NormalmapASTCnm));
        assert!(!is_normalmap_usage_mode(TextureUsageMode::Other));
    }

    #[test]
    fn importer_type_follows_normalmap_fact_in_every_branch() {
        // Shared by the ITexture2D, ITexture2DArray, ICubemapArray, and ITexture3D branches.
        assert_eq!(select_texture_importer_type(true), "NormalMap");
        assert_eq!(select_texture_importer_type(false), "Default");
    }

    #[test]
    fn array_mipmaps_need_strictly_more_than_one_mip() {
        // ITexture2DArray and ICubemapArray: `MipCount > 1`, not `>= 1`.
        assert!(!mip_count_enables_mipmap(0));
        assert!(!mip_count_enables_mipmap(1));
        assert!(mip_count_enables_mipmap(2));
        assert!(mip_count_enables_mipmap(13));
        assert!(!mip_count_enables_mipmap(-3));
    }

    #[test]
    fn texture_3d_mipmaps_follow_either_gated_mip_source() {
        // Neither source present: no mipmaps regardless of the stored values.
        assert!(!select_texture_3d_enable_mipmap(false, 99, false, true));
        assert!(!select_texture_3d_enable_mipmap(false, 0, false, false));
        // MipCount alone enables only when present and strictly above one.
        assert!(select_texture_3d_enable_mipmap(true, 2, false, false));
        assert!(!select_texture_3d_enable_mipmap(true, 1, false, false));
        assert!(!select_texture_3d_enable_mipmap(true, 0, false, true));
        // MipMap alone enables only when present and set; `&&` binds tighter than `||`,
        // so each disjunct stands on its own gate plus its own value check.
        assert!(select_texture_3d_enable_mipmap(false, 0, true, true));
        assert!(!select_texture_3d_enable_mipmap(false, 99, true, false));
        assert!(!select_texture_3d_enable_mipmap(true, 1, true, false));
        // Either source suffices when both are present.
        assert!(select_texture_3d_enable_mipmap(true, 4, true, false));
        assert!(select_texture_3d_enable_mipmap(true, 1, true, true));
        assert!(select_texture_3d_enable_mipmap(true, 4, true, true));
    }

    #[test]
    fn texture_shapes_match_per_branch_assignments() {
        // ITexture2D: `texture2D is ICubemap` picks TextureCube, else Texture2D.
        assert_eq!(select_texture_2d_shape(true), "TextureCube");
        assert_eq!(select_texture_2d_shape(false), "Texture2D");
        assert_eq!(
            select_texture_2d_shape(true),
            texture_shape_literals::TEXTURE_CUBE
        );
        assert_eq!(
            select_texture_2d_shape(false),
            texture_shape_literals::TEXTURE_2D
        );
        // The fixed branches keep the source's shapes, including the cubemap-array quirk:
        // Texture2DArray even though the source wonders about TextureCube.
        assert_eq!(TEXTURE_2D_ARRAY_IMPORTER_SHAPE, "Texture2DArray");
        assert_eq!(CUBEMAP_ARRAY_IMPORTER_SHAPE, "Texture2DArray");
        assert_eq!(TEXTURE_3D_IMPORTER_SHAPE, "Texture3D");
        assert_eq!(
            CUBEMAP_ARRAY_IMPORTER_SHAPE,
            TEXTURE_2D_ARRAY_IMPORTER_SHAPE
        );
    }

    #[test]
    fn non_2d_branches_never_stream_mipmaps() {
        // ITexture2DArray, ICubemapArray, and ITexture3D write fixed values with no
        // asset read; only ITexture2D copies the streaming fields off the asset.
        // Pinned at compile time (a runtime assert on constants trips Clippy).
        const { assert!(!NON_2D_STREAMING_MIPMAPS) };
        const { assert!(NON_2D_STREAMING_MIPMAPS_PRIORITY == 0) };
    }
}
