//! Exporter registration order ported from `ProjectExporter`.
//!
//! The baseline registers one exporter stack per asset type in `ProjectExporter ..ctor` and
//! `DoFinalOverrides`. Order matters: `ObjectHandlerStack.OverrideHandler` lets a later
//! registration shadow earlier ones for the same type, and `allowInheritance` extends a
//! registration to derived generated types. Every entry below keeps its source position, its
//! inheritance flag, and the configuration gate around it:
//!
//! - `ExportSettings.ExportUnreadableAssets` selects real unknown/unreadable exporters or
//!   `Skip` dummies (both non-inheriting and `Serialized`-typed).
//! - `ExportSettings.SpriteExportMode.Yaml` adds the YAML sprite exporters.
//! - A non-YAML `LightmapTextureExportFormat` adds the EXR/image lightmap exporter.
//! - `Version >= 2020.2` adds the texture-array exporters.
//! - `ShaderExportMode.Yaml` selects `YamlShaderExporter`, otherwise `DummyShaderTextExporter`;
//!   `SimpleShaderExporter` always registers second for `IShader`.
//! - `AudioExportFormat.Native` and supported converted formats add their audio exporters.
//! - `DoFinalOverrides` registers the engine-assets exporter (from stored resource data or
//!   embedded data) and the deleted-assets exporter absolutely last.
//!
//! Notable shadowing the table preserves: `TerrainYamlExporter` unconditionally follows the
//! texture exporters for `ITexture2D`; `ScriptableObjectGroupExporter` follows
//! `ScriptableObjectExporter` for `IMonoBehaviour`; `AnimatorControllerExporter`,
//! `EngineAssetsExporter`, and `DeletedAssetsExporter` all target `IUnityObjectBase` in that
//! order. Resolving which registration wins for a concrete generated type needs the generated
//! hierarchy, which the frozen snapshot does not contain; the table therefore records the stack
//! without claiming resolution.
//!
//! Deliberately out of this table (audited, not missing):
//! - `RawTextureExporter` is `internal` and never constructed or registered anywhere in the
//!   frozen baseline, so it contributes no row.
//! - `AssemblyDefinitionExporter` is a static helper called by the script collections, not
//!   an `IAssetExporter`, so it contributes no row.
//! - The five `ExportHandler.GetPostExporters` post-exporters (`ProjectVersionPostExporter`,
//!   `PackageManifestPostExporter`, `StreamingAssetsPostExporter`, `DllPostExporter`,
//!   `PathIdMapExporter`) run after export through `IPostExporter`, not through the asset
//!   exporter stack, so they contribute no rows.
//! - The abstract bases (`BinaryAssetExporter`, `YamlExporterBase`, `ShaderExporterBase`,
//!   `DummyAssetExporter` with its private empty/skip implementers selected by
//!   `DummyAssetExporter.Get`) never register directly; the dummy rows record the selected
//!   `(isEmptyCollection, isMetaType)` arguments instead.
//! - `PrimaryContentExporter` keeps its own `OverrideHandler` stack outside `ProjectExporter`
//!   and is not part of this order.

use super::settings::ImageExportFormat;

/// Which image format a non-YAML lightmap export converts through.
///
/// `LightmapTextureExportFormat.Exr` forces `ImageExportFormat.Exr`; the `Image` mode reuses
/// the configured image format. (The `Yaml` mode never constructs this exporter, so it has
/// no mapping here.) The exporter's `PreferOriginalTextureExtension` passes the settings
/// value through unchanged; only the format argument is decided here.
pub const fn lightmap_conversion_format(
    is_exr_mode: bool,
    configured: ImageExportFormat,
) -> ImageExportFormat {
    if is_exr_mode {
        ImageExportFormat::Exr
    } else {
        configured
    }
}

/// Which engine-asset source `DoFinalOverrides` registers.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EngineAssetSource {
    /// Stored `EngineResourceData` from the settings singleton.
    StoredResourceData,
    /// Version-matched data embedded in the exporter assembly.
    EmbeddedData,
}

/// The `DoFinalOverrides` engine source: stored resource data wins when present.
pub const fn engine_asset_source(has_stored_resource_data: bool) -> EngineAssetSource {
    if has_stored_resource_data {
        EngineAssetSource::StoredResourceData
    } else {
        EngineAssetSource::EmbeddedData
    }
}

/// The configuration gate around one registration.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RegistrationGate {
    /// Registered unconditionally.
    Always,
    /// `ExportSettings.ExportUnreadableAssets` is enabled.
    ExportUnreadableAssets,
    /// `ExportSettings.ExportUnreadableAssets` is disabled.
    NoExportUnreadableAssets,
    /// `ExportSettings.SpriteExportMode` is `Yaml`.
    SpriteExportModeYaml,
    /// `ExportSettings.LightmapTextureExportFormat` is not `Yaml`.
    LightmapTextureExportFormatNonYaml,
    /// The export Unity version is at least 2020.2.
    VersionAtLeast2020_2,
    /// `ExportSettings.ShaderExportMode` is `Yaml`.
    ShaderExportModeYaml,
    /// `ExportSettings.ShaderExportMode` is not `Yaml`.
    ShaderExportModeNonYaml,
    /// `ExportSettings.AudioExportFormat` is `Native`.
    AudioExportFormatNative,
    /// `AudioClipExporter.IsSupportedExportFormat` accepts the configured format.
    AudioExportFormatSupported,
}

/// One `OverrideExporter` call in source order.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ExporterRegistration {
    /// The asset interface or class the exporter is registered for.
    pub target: &'static str,
    /// The exporter type name.
    pub exporter: &'static str,
    /// The `allowInheritance` argument.
    pub allow_inheritance: bool,
    /// The configuration gate around the call.
    pub gate: RegistrationGate,
    /// True only for the two `DoFinalOverrides` registrations.
    pub final_override: bool,
    /// For `OverrideDummyExporter` entries: the `isEmptyCollection` argument.
    pub dummy_is_empty_collection: Option<bool>,
    /// For `OverrideDummyExporter` entries: the `isMetaType` argument.
    pub dummy_is_meta_type: Option<bool>,
}

/// Every exporter registration in `ProjectExporter` source order.
pub const EXPORTER_REGISTRATIONS: &[ExporterRegistration] = &[
    ExporterRegistration {
        target: "IUnityObjectBase",
        exporter: "DefaultYamlExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "IGlobalGameManager",
        exporter: "ManagerAssetExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "TypeTreeObject",
        exporter: "ManagerAssetExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "IMonoBehaviour",
        exporter: "ScriptableObjectExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "IPrefabInstance",
        exporter: "SceneYamlExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "IGameObject",
        exporter: "SceneYamlExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "IComponent",
        exporter: "SceneYamlExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "ILevelGameManager",
        exporter: "SceneYamlExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "IBuildSettings",
        exporter: "DummyAssetExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: Some(true),
        dummy_is_meta_type: Some(false),
    },
    ExporterRegistration {
        target: "IPreloadData",
        exporter: "DummyAssetExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: Some(true),
        dummy_is_meta_type: Some(false),
    },
    ExporterRegistration {
        target: "IAssetBundle",
        exporter: "DummyAssetExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: Some(true),
        dummy_is_meta_type: Some(false),
    },
    ExporterRegistration {
        target: "IAssetBundleManifest",
        exporter: "DummyAssetExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: Some(true),
        dummy_is_meta_type: Some(false),
    },
    ExporterRegistration {
        target: "IMonoManager",
        exporter: "DummyAssetExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: Some(true),
        dummy_is_meta_type: Some(false),
    },
    ExporterRegistration {
        target: "IResourceManager",
        exporter: "DummyAssetExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: Some(true),
        dummy_is_meta_type: Some(false),
    },
    ExporterRegistration {
        target: "IShaderNameRegistry",
        exporter: "DummyAssetExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: Some(true),
        dummy_is_meta_type: Some(false),
    },
    ExporterRegistration {
        target: "ISceneAsset",
        exporter: "SceneAssetExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "UnknownObject",
        exporter: "UnknownObjectExporter",
        allow_inheritance: false,
        gate: RegistrationGate::ExportUnreadableAssets,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "UnreadableObject",
        exporter: "UnreadableObjectExporter",
        allow_inheritance: false,
        gate: RegistrationGate::ExportUnreadableAssets,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "UnknownObject",
        exporter: "DummyAssetExporter",
        allow_inheritance: true,
        gate: RegistrationGate::NoExportUnreadableAssets,
        final_override: false,
        dummy_is_empty_collection: Some(false),
        dummy_is_meta_type: Some(false),
    },
    ExporterRegistration {
        target: "UnreadableObject",
        exporter: "DummyAssetExporter",
        allow_inheritance: true,
        gate: RegistrationGate::NoExportUnreadableAssets,
        final_override: false,
        dummy_is_empty_collection: Some(false),
        dummy_is_meta_type: Some(false),
    },
    ExporterRegistration {
        target: "IMesh",
        exporter: "YamlStreamedAssetExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "IImageTexture",
        exporter: "YamlStreamedAssetExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "ITextAsset",
        exporter: "TextAssetExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "IMovieTexture",
        exporter: "MovieTextureAssetExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "IVideoClip",
        exporter: "VideoClipExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "ITexture2D",
        exporter: "TextureAssetExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "ISprite",
        exporter: "TextureAssetExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "SpriteInformationObject",
        exporter: "TextureAssetExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "ISprite",
        exporter: "YamlSpriteExporter",
        allow_inheritance: true,
        gate: RegistrationGate::SpriteExportModeYaml,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "ISpriteAtlas",
        exporter: "YamlSpriteExporter",
        allow_inheritance: true,
        gate: RegistrationGate::SpriteExportModeYaml,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "ITexture2D",
        exporter: "LightmapTextureAssetExporter",
        allow_inheritance: true,
        gate: RegistrationGate::LightmapTextureExportFormatNonYaml,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "ICubemapArray",
        exporter: "TextureArrayAssetExporter",
        allow_inheritance: true,
        gate: RegistrationGate::VersionAtLeast2020_2,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "ITexture2DArray",
        exporter: "TextureArrayAssetExporter",
        allow_inheritance: true,
        gate: RegistrationGate::VersionAtLeast2020_2,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "ITexture3D",
        exporter: "TextureArrayAssetExporter",
        allow_inheritance: true,
        gate: RegistrationGate::VersionAtLeast2020_2,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "IFont",
        exporter: "FontAssetExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "IMaterial",
        exporter: "FontAssetExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "ITexture",
        exporter: "FontAssetExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "IShader",
        exporter: "YamlShaderExporter",
        allow_inheritance: true,
        gate: RegistrationGate::ShaderExportModeYaml,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "IShader",
        exporter: "DummyShaderTextExporter",
        allow_inheritance: true,
        gate: RegistrationGate::ShaderExportModeNonYaml,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "IShader",
        exporter: "SimpleShaderExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "IAudioClip",
        exporter: "YamlAudioExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "IAudioClip",
        exporter: "NativeAudioExporter",
        allow_inheritance: true,
        gate: RegistrationGate::AudioExportFormatNative,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "IAudioClip",
        exporter: "AudioClipExporter",
        allow_inheritance: true,
        gate: RegistrationGate::AudioExportFormatSupported,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "IAudioMixer",
        exporter: "AudioMixerExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "IAudioMixerEffectController",
        exporter: "AudioMixerExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "IAudioMixerGroup",
        exporter: "AudioMixerExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "IAudioMixerSnapshot",
        exporter: "AudioMixerExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "ITerrainData",
        exporter: "TerrainYamlExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "ITexture2D",
        exporter: "TerrainYamlExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "IMonoScript",
        exporter: "ScriptExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "IUnityObjectBase",
        exporter: "AnimatorControllerExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "IMonoBehaviour",
        exporter: "ScriptableObjectGroupExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "ScriptableObjectGroup",
        exporter: "ScriptableObjectGroupExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: false,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "IUnityObjectBase",
        exporter: "EngineAssetsExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: true,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
    ExporterRegistration {
        target: "IUnityObjectBase",
        exporter: "DeletedAssetsExporter",
        allow_inheritance: true,
        gate: RegistrationGate::Always,
        final_override: true,
        dummy_is_empty_collection: None,
        dummy_is_meta_type: None,
    },
];

/// All registrations for one target, in source order.
pub fn registrations_for_target(
    target: &str,
) -> impl Iterator<Item = &'static ExporterRegistration> + '_ {
    EXPORTER_REGISTRATIONS
        .iter()
        .filter(move |registration| registration.target == target)
}

#[cfg(test)]
mod tests {
    use super::super::settings::ImageExportFormat;
    use super::{
        engine_asset_source, lightmap_conversion_format, registrations_for_target,
        EngineAssetSource, RegistrationGate, EXPORTER_REGISTRATIONS,
    };

    fn positions(target: &str) -> Vec<usize> {
        EXPORTER_REGISTRATIONS
            .iter()
            .enumerate()
            .filter(|(_, registration)| registration.target == target)
            .map(|(index, _)| index)
            .collect()
    }

    #[test]
    fn table_covers_every_ctor_and_final_registration_in_order() {
        // 55 OverrideExporter calls: 53 in the constructor plus 2 final overrides.
        assert_eq!(EXPORTER_REGISTRATIONS.len(), 55);
        assert_eq!(
            EXPORTER_REGISTRATIONS.first().unwrap().target,
            "IUnityObjectBase"
        );
        assert_eq!(
            EXPORTER_REGISTRATIONS.first().unwrap().exporter,
            "DefaultYamlExporter"
        );
        let finals = &EXPORTER_REGISTRATIONS[EXPORTER_REGISTRATIONS.len() - 2..];
        assert_eq!(finals[0].exporter, "EngineAssetsExporter");
        assert!(finals[0].final_override);
        assert_eq!(finals[1].exporter, "DeletedAssetsExporter");
        assert!(finals[1].final_override);
        assert!(EXPORTER_REGISTRATIONS[..EXPORTER_REGISTRATIONS.len() - 2]
            .iter()
            .all(|registration| !registration.final_override));
    }

    #[test]
    fn texture_registrations_preserve_the_terrain_shadowing_order() {
        // TerrainYamlExporter unconditionally follows both texture exporters for ITexture2D.
        let texture: Vec<&str> = registrations_for_target("ITexture2D")
            .map(|registration| registration.exporter)
            .collect();
        assert_eq!(
            texture,
            [
                "TextureAssetExporter",
                "LightmapTextureAssetExporter",
                "TerrainYamlExporter"
            ]
        );
        assert_eq!(positions("ITexture2D").len(), 3);
    }

    #[test]
    fn shader_and_audio_registrations_keep_their_gated_pairs() {
        let shader: Vec<(RegistrationGate, &str)> = registrations_for_target("IShader")
            .map(|registration| (registration.gate, registration.exporter))
            .collect();
        assert_eq!(
            shader,
            [
                (RegistrationGate::ShaderExportModeYaml, "YamlShaderExporter"),
                (
                    RegistrationGate::ShaderExportModeNonYaml,
                    "DummyShaderTextExporter"
                ),
                (RegistrationGate::Always, "SimpleShaderExporter"),
            ]
        );
        let audio: Vec<&str> = registrations_for_target("IAudioClip")
            .map(|registration| registration.exporter)
            .collect();
        assert_eq!(
            audio,
            [
                "YamlAudioExporter",
                "NativeAudioExporter",
                "AudioClipExporter"
            ]
        );
    }

    #[test]
    fn dummy_registrations_carry_their_empty_and_meta_arguments() {
        for target in [
            "IBuildSettings",
            "IPreloadData",
            "IAssetBundle",
            "IAssetBundleManifest",
            "IMonoManager",
            "IResourceManager",
            "IShaderNameRegistry",
        ] {
            let registrations: Vec<_> = registrations_for_target(target).collect();
            assert_eq!(registrations.len(), 1, "{target}");
            assert_eq!(registrations[0].exporter, "DummyAssetExporter");
            assert_eq!(registrations[0].dummy_is_empty_collection, Some(true));
            assert_eq!(registrations[0].dummy_is_meta_type, Some(false));
        }
        for target in ["UnknownObject", "UnreadableObject"] {
            let registrations: Vec<_> = registrations_for_target(target).collect();
            assert_eq!(registrations.len(), 2, "{target}");
            // The real exporters are non-inheriting; the Skip dummies inherit.
            assert!(!registrations[0].allow_inheritance);
            assert!(registrations[1].allow_inheritance);
            assert_eq!(registrations[1].dummy_is_empty_collection, Some(false));
            assert_eq!(registrations[1].dummy_is_meta_type, Some(false));
        }
    }

    #[test]
    fn ctor_and_final_selection_rules_match_source_expressions() {
        // The lightmap exporter converts through Exr in Exr mode, else the configured format.
        assert_eq!(
            lightmap_conversion_format(true, ImageExportFormat::Png),
            ImageExportFormat::Exr
        );
        assert_eq!(
            lightmap_conversion_format(false, ImageExportFormat::Jpeg),
            ImageExportFormat::Jpeg
        );
        // Stored engine resource data wins; otherwise the version-matched embedded data.
        assert_eq!(
            engine_asset_source(true),
            EngineAssetSource::StoredResourceData
        );
        assert_eq!(engine_asset_source(false), EngineAssetSource::EmbeddedData);
    }

    #[test]
    fn catch_all_and_group_registrations_keep_source_positions() {
        let base: Vec<&str> = registrations_for_target("IUnityObjectBase")
            .map(|registration| registration.exporter)
            .collect();
        assert_eq!(
            base,
            [
                "DefaultYamlExporter",
                "AnimatorControllerExporter",
                "EngineAssetsExporter",
                "DeletedAssetsExporter"
            ]
        );
        let behaviour: Vec<&str> = registrations_for_target("IMonoBehaviour")
            .map(|registration| registration.exporter)
            .collect();
        assert_eq!(
            behaviour,
            ["ScriptableObjectExporter", "ScriptableObjectGroupExporter"]
        );
        let font_material: Vec<&str> = registrations_for_target("IMaterial")
            .map(|registration| registration.exporter)
            .collect();
        assert_eq!(font_material, ["FontAssetExporter"]);
    }

    #[test]
    fn full_registry_shape_matches_source_call_order() {
        // Every OverrideExporter/OverrideDummyExporter call in ProjectExporter..ctor plus
        // the two DoFinalOverrides calls, in source order: target, exporter, inheritance,
        // gate, final-override flag, and dummy arguments. The IShader switch contributes
        // two rows (one call site, two mutually exclusive gates); the unreadable-assets
        // if/else contributes two rows (one branch runs per configuration).
        let shape: Vec<String> = EXPORTER_REGISTRATIONS
            .iter()
            .map(|registration| {
                format!(
                    "{}|{}|{}|{:?}|{}|{:?}|{:?}",
                    registration.target,
                    registration.exporter,
                    registration.allow_inheritance,
                    registration.gate,
                    registration.final_override,
                    registration.dummy_is_empty_collection,
                    registration.dummy_is_meta_type,
                )
            })
            .collect();
        assert_eq!(
            shape,
            [
                "IUnityObjectBase|DefaultYamlExporter|true|Always|false|None|None",
                "IGlobalGameManager|ManagerAssetExporter|true|Always|false|None|None",
                "TypeTreeObject|ManagerAssetExporter|true|Always|false|None|None",
                "IMonoBehaviour|ScriptableObjectExporter|true|Always|false|None|None",
                "IPrefabInstance|SceneYamlExporter|true|Always|false|None|None",
                "IGameObject|SceneYamlExporter|true|Always|false|None|None",
                "IComponent|SceneYamlExporter|true|Always|false|None|None",
                "ILevelGameManager|SceneYamlExporter|true|Always|false|None|None",
                "IBuildSettings|DummyAssetExporter|true|Always|false|Some(true)|Some(false)",
                "IPreloadData|DummyAssetExporter|true|Always|false|Some(true)|Some(false)",
                "IAssetBundle|DummyAssetExporter|true|Always|false|Some(true)|Some(false)",
                "IAssetBundleManifest|DummyAssetExporter|true|Always|false|Some(true)|Some(false)",
                "IMonoManager|DummyAssetExporter|true|Always|false|Some(true)|Some(false)",
                "IResourceManager|DummyAssetExporter|true|Always|false|Some(true)|Some(false)",
                "IShaderNameRegistry|DummyAssetExporter|true|Always|false|Some(true)|Some(false)",
                "ISceneAsset|SceneAssetExporter|true|Always|false|None|None",
                "UnknownObject|UnknownObjectExporter|false|ExportUnreadableAssets|false|None|None",
                "UnreadableObject|UnreadableObjectExporter|false|ExportUnreadableAssets|false|None|None",
                "UnknownObject|DummyAssetExporter|true|NoExportUnreadableAssets|false|Some(false)|Some(false)",
                "UnreadableObject|DummyAssetExporter|true|NoExportUnreadableAssets|false|Some(false)|Some(false)",
                "IMesh|YamlStreamedAssetExporter|true|Always|false|None|None",
                "IImageTexture|YamlStreamedAssetExporter|true|Always|false|None|None",
                "ITextAsset|TextAssetExporter|true|Always|false|None|None",
                "IMovieTexture|MovieTextureAssetExporter|true|Always|false|None|None",
                "IVideoClip|VideoClipExporter|true|Always|false|None|None",
                "ITexture2D|TextureAssetExporter|true|Always|false|None|None",
                "ISprite|TextureAssetExporter|true|Always|false|None|None",
                "SpriteInformationObject|TextureAssetExporter|true|Always|false|None|None",
                "ISprite|YamlSpriteExporter|true|SpriteExportModeYaml|false|None|None",
                "ISpriteAtlas|YamlSpriteExporter|true|SpriteExportModeYaml|false|None|None",
                "ITexture2D|LightmapTextureAssetExporter|true|LightmapTextureExportFormatNonYaml|false|None|None",
                "ICubemapArray|TextureArrayAssetExporter|true|VersionAtLeast2020_2|false|None|None",
                "ITexture2DArray|TextureArrayAssetExporter|true|VersionAtLeast2020_2|false|None|None",
                "ITexture3D|TextureArrayAssetExporter|true|VersionAtLeast2020_2|false|None|None",
                "IFont|FontAssetExporter|true|Always|false|None|None",
                "IMaterial|FontAssetExporter|true|Always|false|None|None",
                "ITexture|FontAssetExporter|true|Always|false|None|None",
                "IShader|YamlShaderExporter|true|ShaderExportModeYaml|false|None|None",
                "IShader|DummyShaderTextExporter|true|ShaderExportModeNonYaml|false|None|None",
                "IShader|SimpleShaderExporter|true|Always|false|None|None",
                "IAudioClip|YamlAudioExporter|true|Always|false|None|None",
                "IAudioClip|NativeAudioExporter|true|AudioExportFormatNative|false|None|None",
                "IAudioClip|AudioClipExporter|true|AudioExportFormatSupported|false|None|None",
                "IAudioMixer|AudioMixerExporter|true|Always|false|None|None",
                "IAudioMixerEffectController|AudioMixerExporter|true|Always|false|None|None",
                "IAudioMixerGroup|AudioMixerExporter|true|Always|false|None|None",
                "IAudioMixerSnapshot|AudioMixerExporter|true|Always|false|None|None",
                "ITerrainData|TerrainYamlExporter|true|Always|false|None|None",
                "ITexture2D|TerrainYamlExporter|true|Always|false|None|None",
                "IMonoScript|ScriptExporter|true|Always|false|None|None",
                "IUnityObjectBase|AnimatorControllerExporter|true|Always|false|None|None",
                "IMonoBehaviour|ScriptableObjectGroupExporter|true|Always|false|None|None",
                "ScriptableObjectGroup|ScriptableObjectGroupExporter|true|Always|false|None|None",
                "IUnityObjectBase|EngineAssetsExporter|true|Always|true|None|None",
                "IUnityObjectBase|DeletedAssetsExporter|true|Always|true|None|None",
            ]
        );
    }

    #[test]
    fn distinct_exporter_census_matches_baseline() {
        let mut exporters: Vec<&str> = EXPORTER_REGISTRATIONS
            .iter()
            .map(|registration| registration.exporter)
            .collect();
        exporters.sort_unstable();
        exporters.dedup();
        assert_eq!(
            exporters,
            [
                "AnimatorControllerExporter",
                "AudioClipExporter",
                "AudioMixerExporter",
                "DefaultYamlExporter",
                "DeletedAssetsExporter",
                "DummyAssetExporter",
                "DummyShaderTextExporter",
                "EngineAssetsExporter",
                "FontAssetExporter",
                "LightmapTextureAssetExporter",
                "ManagerAssetExporter",
                "MovieTextureAssetExporter",
                "NativeAudioExporter",
                "SceneAssetExporter",
                "SceneYamlExporter",
                "ScriptExporter",
                "ScriptableObjectExporter",
                "ScriptableObjectGroupExporter",
                "SimpleShaderExporter",
                "TerrainYamlExporter",
                "TextAssetExporter",
                "TextureArrayAssetExporter",
                "TextureAssetExporter",
                "UnknownObjectExporter",
                "UnreadableObjectExporter",
                "VideoClipExporter",
                "YamlAudioExporter",
                "YamlShaderExporter",
                "YamlSpriteExporter",
                "YamlStreamedAssetExporter",
            ]
        );
    }
}
