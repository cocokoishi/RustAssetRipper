//! `TryCreateCollection` admission rules for every concrete exporter in the snapshot.
//!
//! Each rule is a pure function of caller-supplied asset facts; generated type checks
//! (`is IShader`, `Has_Script`, integrity predicates) stay caller-side while the
//! admission outcome — which collection, or refusal — is pinned here. Object decoding,
//! bitmap conversion, and YAML writing stay with their owners.
//!
//! Also recorded: the `ScriptableObject` extension branches (`guiskin`/`brush`), the
//! font multi-export single-font assumption, the AND-accumulation of the unknown-object
//! multi export, the throwing type queries of `DeletedAssetsExporter`, and the dead
//! `TerrainExportMode` setting (declared and shown in the GUI dropdown, never consumed
//! by export code).

use super::settings::AudioExportFormat;

/// What the main asset of a queried asset is, for `MainAsset` switches.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MainAssetKind {
    /// `ITerrainData`.
    TerrainData,
    /// `IAnimatorController`.
    AnimatorController,
    /// `SceneHierarchyObject`.
    SceneHierarchy,
    /// `PrefabHierarchyObject`.
    PrefabHierarchy,
    /// `ScriptableObjectGroup`.
    PlayableGroup,
    /// `SpriteInformationObject`.
    SpriteInfo,
    /// `DeletedAssetsInformation`.
    DeletedInfo,
    /// `IFont` with valid data.
    Font,
    /// Anything else.
    Other,
}

/// Which collection a successful admission builds.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AdmittedCollection {
    /// A plain `AssetExportCollection`.
    Asset,
    /// `ShaderExportCollection` via the shared shader base.
    Shader,
    /// `YamlShaderExportCollection` with the `asset` extension and patch plan.
    YamlShader,
    /// `TerrainYamlExportCollection` with splat-texture gathering.
    Terrain,
    /// `AnimatorControllerExportCollection`.
    AnimatorController,
    /// `SceneAssetExportCollection`.
    SceneAsset,
    /// `ManagerExportCollection`, or the editor-build-settings variant.
    Manager,
    /// `EditorBuildSettingsExportCollection`.
    EditorBuildSettings,
    /// `SceneExportCollection` for scene hierarchies.
    Scene,
    /// `PrefabExportCollection` for prefab hierarchies.
    Prefab,
    /// `ScriptableObjectExportCollection`, or the shared empty singleton.
    ScriptableObject,
    /// The shared `EmptyExportCollection.Instance`.
    EmptySingleton,
    /// `ScriptableObjectGroupExportCollection`.
    PlayableGroup,
    /// `AudioClipExportCollection` with a decoded extension.
    AudioClip,
    /// `NativeAudioExportCollection` with raw bytes.
    NativeAudio,
    /// `YamlAudioExportCollection` with `.resS` staging.
    YamlAudio,
    /// `FontAssetExportCollection` with material and texture members.
    Font,
    /// `MovieTextureExportCollection`.
    MovieTexture,
    /// `TextAssetExportCollection`.
    Text,
    /// `VideoClipExportCollection`.
    VideoClip,
    /// `TextureExportCollection` with sprite handling.
    Texture,
    /// `TextureArrayAssetExportCollection`.
    TextureArray,
    /// The private `LightmapExportCollection` of the lightmap exporter.
    Lightmap,
    /// `RawTextureExportCollection`.
    RawTexture,
    /// `YamlSpriteExporter` admits sprites to a plain asset collection.
    SpriteAsset,
}

/// Asset facts consulted by the admission switches.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct AdmissionFacts<'a> {
    /// `asset is IShader`.
    pub is_shader: bool,
    /// The shader exposes a script (`Has_Script`).
    pub shader_has_script: bool,
    /// The script text passes the decompiled-text gate.
    pub shader_decompiled_text_ok: bool,
    /// The asset's main asset kind.
    pub main_kind: MainAssetKind,
    /// `asset is IGlobalGameManager or TypeTreeObject{ IsPlayerSettings }`.
    pub is_manager_asset: bool,
    /// `asset is IEditorBuildSettings`.
    pub is_editor_build_settings: bool,
    /// `asset is IMonoBehaviour`.
    pub is_mono_behaviour: bool,
    /// The behaviour sits on a GameObject (routes to the empty singleton).
    pub mono_on_game_object: bool,
    /// `asset is ISceneAsset`.
    pub is_scene_asset: bool,
    /// `asset is IAudioClip`.
    pub is_audio_clip: bool,
    /// Decoding succeeded with nonempty output.
    pub audio_decode_usable: bool,
    /// `asset is IMovieTexture` with valid movie data.
    pub has_movie_data: bool,
    /// `asset is ITextAsset` with a nonempty script.
    pub has_text_script: bool,
    /// The main asset is lighting data (lightmap exporter).
    pub main_is_lighting_data: bool,
    /// `asset is IVideoClip` with integer external resources.
    pub has_video_integrity: bool,
    /// An integral image texture with no main asset (array exporter).
    pub is_standalone_image_texture: bool,
    /// `asset is ITexture2D` (raw exporter).
    pub is_texture_2d: bool,
    /// `asset is UnknownObject`.
    pub is_unknown_object: bool,
    /// `asset is UnreadableObject`.
    pub is_unreadable_object: bool,
    /// `asset is ISprite`.
    pub is_sprite: bool,
    /// `asset is ISpriteAtlas` (routes to the empty singleton).
    pub is_sprite_atlas: bool,
    /// The texture exporter may emit sprites (`SpriteExportMode is not Yaml`).
    pub texture_exports_sprites: bool,
    /// The configured audio format.
    pub audio_format: AudioExportFormat,
    /// The decoder-reported audio extension for admitted clips.
    pub audio_extension: &'a str,
}

/// One exporter's admission rule in registration order.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ExporterId {
    /// `ShaderExporterBase`: every shader.
    ShaderBase,
    /// `SimpleShaderExporter`: shaders with decompiled script text.
    SimpleShader,
    /// `YamlShaderExporter`: every shader.
    YamlShader,
    /// `DummyShaderTextExporter`: every shader, via its base.
    DummyShader,
    /// `TerrainYamlExporter`: terrain mains.
    Terrain,
    /// `AnimatorControllerExporter`: controller mains.
    AnimatorController,
    /// `DefaultYamlExporter`: everything.
    DefaultYaml,
    /// `ManagerAssetExporter`: managers and player-settings objects.
    Manager,
    /// `SceneYamlExporter`: scene and prefab hierarchy mains.
    SceneYaml,
    /// `ScriptableObjectExporter`: behaviours.
    ScriptableObject,
    /// `ScriptableObjectGroupExporter`: playable-group mains.
    PlayableGroup,
    /// `SceneAssetExporter`: scene assets.
    SceneAsset,
    /// `AudioClipExporter`: clips with usable decodes under supported formats.
    AudioClip,
    /// `NativeAudioExporter`: clips under the native format.
    NativeAudio,
    /// `YamlAudioExporter`: every clip.
    YamlAudio,
    /// `FontAssetExporter`: font mains with data.
    Font,
    /// `MovieTextureAssetExporter`: movie textures with data.
    MovieTexture,
    /// `TextAssetExporter`: text assets with scripts.
    TextAsset,
    /// `VideoClipExporter`: clips with integer resources.
    VideoClip,
    /// `TextureAssetExporter`: sprite-information mains, modulo the sprite rule.
    Texture,
    /// `TextureArrayAssetExporter`: standalone integral image textures.
    TextureArray,
    /// `RawTextureExporter`: every 2D texture.
    RawTexture,
    /// `LightmapTextureAssetExporter`: lighting-data mains.
    Lightmap,
    /// `YamlSpriteExporter`: sprites, plus Atlases to the empty singleton.
    YamlSprite,
    /// `UnknownObjectExporter`: unknown marker objects.
    Unknown,
    /// `UnreadableObjectExporter`: unreadable marker objects.
    Unreadable,
    /// `DeletedAssetsExporter`: deleted-information mains.
    Deleted,
}

/// `TryCreateCollection` for one exporter: the admitted collection or refusal.
///
/// `audio_extension` is carried into the audio-clip arm; decode usability and format
/// support follow the audio export module. The lightmap arm reuses the texture-array
/// admission shape (lighting-data mains); bitmap conversion stays exporter-side.
pub fn try_create(
    exporter: ExporterId,
    facts: AdmissionFacts<'_>,
) -> Option<(AdmittedCollection, String)> {
    match exporter {
        ExporterId::ShaderBase | ExporterId::DummyShader => facts
            .is_shader
            .then(|| (AdmittedCollection::Shader, String::new())),
        ExporterId::SimpleShader => {
            (facts.is_shader && facts.shader_has_script && facts.shader_decompiled_text_ok)
                .then(|| (AdmittedCollection::Shader, String::new()))
        }
        ExporterId::YamlShader => facts
            .is_shader
            .then(|| (AdmittedCollection::YamlShader, String::new())),
        ExporterId::Terrain => (facts.main_kind == MainAssetKind::TerrainData)
            .then(|| (AdmittedCollection::Terrain, String::new())),
        ExporterId::AnimatorController => (facts.main_kind == MainAssetKind::AnimatorController)
            .then(|| (AdmittedCollection::AnimatorController, String::new())),
        ExporterId::DefaultYaml => Some((AdmittedCollection::Asset, String::new())),
        ExporterId::Manager => facts.is_manager_asset.then(|| {
            if facts.is_editor_build_settings {
                (AdmittedCollection::EditorBuildSettings, String::new())
            } else {
                (AdmittedCollection::Manager, String::new())
            }
        }),
        ExporterId::SceneYaml => match facts.main_kind {
            MainAssetKind::SceneHierarchy => Some((AdmittedCollection::Scene, String::new())),
            MainAssetKind::PrefabHierarchy => Some((AdmittedCollection::Prefab, String::new())),
            _ => None,
        },
        ExporterId::ScriptableObject => facts.is_mono_behaviour.then(|| {
            if facts.mono_on_game_object {
                (AdmittedCollection::EmptySingleton, String::new())
            } else {
                (AdmittedCollection::ScriptableObject, String::new())
            }
        }),
        ExporterId::PlayableGroup => (facts.main_kind == MainAssetKind::PlayableGroup)
            .then(|| (AdmittedCollection::PlayableGroup, String::new())),
        ExporterId::SceneAsset => facts
            .is_scene_asset
            .then(|| (AdmittedCollection::SceneAsset, String::new())),
        ExporterId::AudioClip => {
            let supported = matches!(
                facts.audio_format,
                AudioExportFormat::Default | AudioExportFormat::PreferWav
            );
            (facts.is_audio_clip && supported && facts.audio_decode_usable).then(|| {
                let extension = if facts.audio_format == AudioExportFormat::PreferWav
                    && facts.audio_extension == "ogg"
                {
                    "wav".to_owned()
                } else {
                    facts.audio_extension.to_owned()
                };
                (AdmittedCollection::AudioClip, extension)
            })
        }
        ExporterId::NativeAudio => (facts.is_audio_clip
            && matches!(facts.audio_format, AudioExportFormat::Native))
        .then(|| (AdmittedCollection::NativeAudio, String::new())),
        ExporterId::YamlAudio => facts
            .is_audio_clip
            .then(|| (AdmittedCollection::YamlAudio, String::new())),
        ExporterId::Font => (facts.main_kind == MainAssetKind::Font)
            .then(|| (AdmittedCollection::Font, String::new())),
        ExporterId::MovieTexture => facts
            .has_movie_data
            .then(|| (AdmittedCollection::MovieTexture, String::new())),
        ExporterId::TextAsset => facts
            .has_text_script
            .then(|| (AdmittedCollection::Text, String::new())),
        ExporterId::VideoClip => facts
            .has_video_integrity
            .then(|| (AdmittedCollection::VideoClip, String::new())),
        ExporterId::Texture => (facts.main_kind == MainAssetKind::SpriteInfo
            && (facts.texture_exports_sprites || !facts.is_sprite))
            .then(|| (AdmittedCollection::Texture, String::new())),
        ExporterId::TextureArray => facts
            .is_standalone_image_texture
            .then(|| (AdmittedCollection::TextureArray, String::new())),
        ExporterId::RawTexture => facts
            .is_texture_2d
            .then(|| (AdmittedCollection::RawTexture, String::new())),
        ExporterId::Lightmap => facts
            .main_is_lighting_data
            .then(|| (AdmittedCollection::Lightmap, String::new())),
        ExporterId::YamlSprite => {
            if facts.is_sprite {
                Some((AdmittedCollection::SpriteAsset, String::new()))
            } else if facts.is_sprite_atlas {
                Some((AdmittedCollection::EmptySingleton, String::new()))
            } else {
                None
            }
        }
        ExporterId::Unknown => facts
            .is_unknown_object
            .then(|| (AdmittedCollection::Asset, String::new())),
        ExporterId::Unreadable => facts
            .is_unreadable_object
            .then(|| (AdmittedCollection::Asset, String::new())),
        ExporterId::Deleted => (facts.main_kind == MainAssetKind::DeletedInfo)
            .then(|| (AdmittedCollection::Asset, String::new())),
    }
}

/// `ScriptableObjectExportCollection.GetExportExtension`: GUI skins and brushes take
/// fixed extensions, everything else falls back to the base extension.
pub fn scriptable_object_extension(is_gui_skin: bool, is_brush: bool) -> Option<&'static str> {
    if is_gui_skin {
        Some("guiskin")
    } else if is_brush {
        Some("brush")
    } else {
        None
    }
}

/// `FontAssetExporter` multi export assumes exactly one font (`Single` throws otherwise).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FontMultiExport {
    /// Exactly one font: its bytes are written.
    WriteSingle,
    /// Zero or several fonts: the source `Single` call throws.
    ThrowsSingleViolation,
}

/// The single-font assumption, checked before any write.
pub const fn plan_font_multi_export(font_count: usize) -> FontMultiExport {
    if font_count == 1 {
        FontMultiExport::WriteSingle
    } else {
        FontMultiExport::ThrowsSingleViolation
    }
}

/// The unknown-object multi export AND-accumulates per-asset results: every asset still
/// runs, and the aggregate is true only when all succeed.
pub fn unknown_multi_export_and(results: &[bool]) -> bool {
    results.iter().all(|succeeded| *succeeded)
}

/// `TerrainExportMode` has three modes, but no export code reads the setting: the GUI
/// dropdown is its only consumer, so terrain exports always take the YAML path.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TerrainExportMode {
    /// The only Unity-usable terrain format.
    Yaml,
    /// Mesh conversion Unity cannot import.
    Mesh,
    /// Height heatmap for visualization only.
    Heatmap,
}

/// The dead setting always resolves to the YAML exporter regardless of mode.
pub const fn select_terrain_exporter(_mode: TerrainExportMode) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::{
        plan_font_multi_export, scriptable_object_extension, select_terrain_exporter, try_create,
        unknown_multi_export_and, AdmissionFacts, AdmittedCollection, ExporterId, MainAssetKind,
        TerrainExportMode,
    };
    use crate::export::settings::AudioExportFormat;

    fn facts() -> AdmissionFacts<'static> {
        AdmissionFacts {
            is_shader: false,
            shader_has_script: false,
            shader_decompiled_text_ok: false,
            main_kind: MainAssetKind::Other,
            is_manager_asset: false,
            is_editor_build_settings: false,
            is_mono_behaviour: false,
            mono_on_game_object: false,
            is_scene_asset: false,
            is_audio_clip: false,
            audio_decode_usable: false,
            has_movie_data: false,
            has_text_script: false,
            has_video_integrity: false,
            main_is_lighting_data: false,
            is_standalone_image_texture: false,
            is_texture_2d: false,
            is_unknown_object: false,
            is_unreadable_object: false,
            is_sprite: false,
            is_sprite_atlas: false,
            texture_exports_sprites: true,
            audio_format: AudioExportFormat::Default,
            audio_extension: "ogg",
        }
    }

    #[test]
    fn shader_and_yaml_bases_admit_unconditionally() {
        let plain = facts();
        assert_eq!(
            try_create(ExporterId::DefaultYaml, plain).map(|entry| entry.0),
            Some(AdmittedCollection::Asset)
        );
        let shader = AdmissionFacts {
            is_shader: true,
            ..plain
        };
        for exporter in [
            ExporterId::ShaderBase,
            ExporterId::DummyShader,
            ExporterId::YamlShader,
        ] {
            assert!(try_create(exporter, shader).is_some(), "{exporter:?}");
        }
        assert!(try_create(ExporterId::ShaderBase, plain).is_none());
        // Simple needs the script gate plus decompiled text.
        assert!(try_create(
            ExporterId::SimpleShader,
            AdmissionFacts {
                shader_has_script: true,
                shader_decompiled_text_ok: true,
                ..shader
            }
        )
        .is_some());
        assert!(try_create(
            ExporterId::SimpleShader,
            AdmissionFacts {
                shader_has_script: true,
                ..shader
            }
        )
        .is_none());
    }

    #[test]
    fn main_asset_switches_route_terrain_animators_scenes_and_groups() {
        let plain = facts();
        assert_eq!(
            try_create(
                ExporterId::Terrain,
                AdmissionFacts {
                    main_kind: MainAssetKind::TerrainData,
                    ..plain
                }
            )
            .map(|entry| entry.0),
            Some(AdmittedCollection::Terrain)
        );
        assert!(try_create(ExporterId::Terrain, plain).is_none());
        assert_eq!(
            try_create(
                ExporterId::AnimatorController,
                AdmissionFacts {
                    main_kind: MainAssetKind::AnimatorController,
                    ..plain
                }
            )
            .map(|entry| entry.0),
            Some(AdmittedCollection::AnimatorController)
        );
        assert_eq!(
            try_create(
                ExporterId::SceneYaml,
                AdmissionFacts {
                    main_kind: MainAssetKind::SceneHierarchy,
                    ..plain
                }
            )
            .map(|entry| entry.0),
            Some(AdmittedCollection::Scene)
        );
        assert_eq!(
            try_create(
                ExporterId::SceneYaml,
                AdmissionFacts {
                    main_kind: MainAssetKind::PrefabHierarchy,
                    ..plain
                }
            )
            .map(|entry| entry.0),
            Some(AdmittedCollection::Prefab)
        );
        assert!(try_create(ExporterId::SceneYaml, plain).is_none());
        assert_eq!(
            try_create(
                ExporterId::PlayableGroup,
                AdmissionFacts {
                    main_kind: MainAssetKind::PlayableGroup,
                    ..plain
                }
            )
            .map(|entry| entry.0),
            Some(AdmittedCollection::PlayableGroup)
        );
        assert_eq!(
            try_create(
                ExporterId::Deleted,
                AdmissionFacts {
                    main_kind: MainAssetKind::DeletedInfo,
                    ..plain
                }
            )
            .map(|entry| entry.0),
            Some(AdmittedCollection::Asset)
        );
    }

    #[test]
    fn managers_scripts_and_scenes_follow_membership_branches() {
        let plain = facts();
        assert_eq!(
            try_create(
                ExporterId::Manager,
                AdmissionFacts {
                    is_manager_asset: true,
                    ..plain
                }
            )
            .map(|entry| entry.0),
            Some(AdmittedCollection::Manager)
        );
        assert_eq!(
            try_create(
                ExporterId::Manager,
                AdmissionFacts {
                    is_manager_asset: true,
                    is_editor_build_settings: true,
                    ..plain
                }
            )
            .map(|entry| entry.0),
            Some(AdmittedCollection::EditorBuildSettings)
        );
        assert!(try_create(ExporterId::Manager, plain).is_none());
        assert_eq!(
            try_create(
                ExporterId::ScriptableObject,
                AdmissionFacts {
                    is_mono_behaviour: true,
                    ..plain
                }
            )
            .map(|entry| entry.0),
            Some(AdmittedCollection::ScriptableObject)
        );
        assert_eq!(
            try_create(
                ExporterId::ScriptableObject,
                AdmissionFacts {
                    is_mono_behaviour: true,
                    mono_on_game_object: true,
                    ..plain
                }
            )
            .map(|entry| entry.0),
            Some(AdmittedCollection::EmptySingleton)
        );
        assert_eq!(
            try_create(
                ExporterId::SceneAsset,
                AdmissionFacts {
                    is_scene_asset: true,
                    ..plain
                }
            )
            .map(|entry| entry.0),
            Some(AdmittedCollection::SceneAsset)
        );
        // GUI skins and brushes take fixed extensions; others fall back to base.
        assert_eq!(scriptable_object_extension(true, false), Some("guiskin"));
        assert_eq!(scriptable_object_extension(false, true), Some("brush"));
        assert_eq!(scriptable_object_extension(false, false), None);
    }

    #[test]
    fn media_and_texture_admissions_follow_data_gates() {
        let plain = facts();
        // Audio mirrors the winner chains: converted under supported formats with usable
        // decodes (carrying the wav substitution), native under Native, YAML for clips.
        let clip = AdmissionFacts {
            is_audio_clip: true,
            audio_decode_usable: true,
            ..plain
        };
        assert_eq!(
            try_create(ExporterId::AudioClip, clip),
            Some((AdmittedCollection::AudioClip, "ogg".to_owned()))
        );
        assert_eq!(
            try_create(
                ExporterId::AudioClip,
                AdmissionFacts {
                    audio_format: AudioExportFormat::PreferWav,
                    ..clip
                }
            ),
            Some((AdmittedCollection::AudioClip, "wav".to_owned()))
        );
        assert!(try_create(ExporterId::NativeAudio, clip).is_none());
        assert_eq!(
            try_create(
                ExporterId::NativeAudio,
                AdmissionFacts {
                    audio_format: AudioExportFormat::Native,
                    ..clip
                }
            )
            .map(|entry| entry.0),
            Some(AdmittedCollection::NativeAudio)
        );
        assert_eq!(
            try_create(ExporterId::YamlAudio, clip).map(|entry| entry.0),
            Some(AdmittedCollection::YamlAudio)
        );
        assert!(try_create(ExporterId::AudioClip, plain).is_none());
        // Media gates: movie data, nonempty scripts, integer video resources.
        assert!(try_create(
            ExporterId::MovieTexture,
            AdmissionFacts {
                has_movie_data: true,
                ..plain
            }
        )
        .is_some());
        assert!(try_create(ExporterId::MovieTexture, plain).is_none());
        assert!(try_create(
            ExporterId::TextAsset,
            AdmissionFacts {
                has_text_script: true,
                ..plain
            }
        )
        .is_some());
        assert!(try_create(ExporterId::TextAsset, plain).is_none());
        assert!(try_create(
            ExporterId::VideoClip,
            AdmissionFacts {
                has_video_integrity: true,
                ..plain
            }
        )
        .is_some());
        // Textures: sprite-info mains modulo the sprite rule, standalone arrays, raw 2D.
        assert!(try_create(
            ExporterId::Texture,
            AdmissionFacts {
                main_kind: MainAssetKind::SpriteInfo,
                is_sprite: true,
                ..plain
            }
        )
        .is_some());
        assert!(try_create(
            ExporterId::Texture,
            AdmissionFacts {
                main_kind: MainAssetKind::SpriteInfo,
                is_sprite: true,
                texture_exports_sprites: false,
                ..plain
            }
        )
        .is_none());
        assert!(try_create(
            ExporterId::TextureArray,
            AdmissionFacts {
                is_standalone_image_texture: true,
                ..plain
            }
        )
        .is_some());
        assert!(try_create(
            ExporterId::RawTexture,
            AdmissionFacts {
                is_texture_2d: true,
                ..plain
            }
        )
        .is_some());
        assert_eq!(
            try_create(
                ExporterId::Lightmap,
                AdmissionFacts {
                    main_is_lighting_data: true,
                    ..plain
                }
            )
            .map(|entry| entry.0),
            Some(AdmittedCollection::Lightmap)
        );
        assert!(try_create(ExporterId::Lightmap, plain).is_none());
        // Sprites route to asset collections, Atlases to the empty singleton.
        assert_eq!(
            try_create(
                ExporterId::YamlSprite,
                AdmissionFacts {
                    is_sprite: true,
                    ..plain
                }
            )
            .map(|entry| entry.0),
            Some(AdmittedCollection::SpriteAsset)
        );
        assert_eq!(
            try_create(
                ExporterId::YamlSprite,
                AdmissionFacts {
                    is_sprite_atlas: true,
                    ..plain
                }
            )
            .map(|entry| entry.0),
            Some(AdmittedCollection::EmptySingleton)
        );
        // Markers route by marker type.
        assert!(try_create(
            ExporterId::Unknown,
            AdmissionFacts {
                is_unknown_object: true,
                ..plain
            }
        )
        .is_some());
        assert!(try_create(
            ExporterId::Unreadable,
            AdmissionFacts {
                is_unreadable_object: true,
                ..plain
            }
        )
        .is_some());
        // Unknown multi exports AND-accumulate: every asset still runs.
        assert!(unknown_multi_export_and(&[true, true]));
        assert!(!unknown_multi_export_and(&[true, false]));
        assert!(unknown_multi_export_and(&[]));
    }

    #[test]
    fn terrain_mode_is_dead_and_font_multi_assumes_single() {
        // No export code reads TerrainExportMode; every mode takes the YAML path.
        for mode in [
            TerrainExportMode::Yaml,
            TerrainExportMode::Mesh,
            TerrainExportMode::Heatmap,
        ] {
            assert!(select_terrain_exporter(mode), "{mode:?}");
        }
        assert_eq!(
            plan_font_multi_export(1),
            super::FontMultiExport::WriteSingle
        );
        assert_eq!(
            plan_font_multi_export(0),
            super::FontMultiExport::ThrowsSingleViolation
        );
        assert_eq!(
            plan_font_multi_export(2),
            super::FontMultiExport::ThrowsSingleViolation
        );
    }
}
