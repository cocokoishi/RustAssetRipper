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
    use super::{registrations_for_target, RegistrationGate, EXPORTER_REGISTRATIONS};

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
}
