//! `TryCreateCollection` claim predicates for every concrete exporter.
//!
//! `ProjectExporter.CreateCollection` walks the registration stack and keeps the first
//! collection whose exporter claims the asset, so each exporter's claim predicate decides the
//! resource matrix. Generated asset types do not exist on the Rust side; every predicate input
//! below is a caller-supplied fact (asset kind, main-asset kind, validity flags, settings
//! flags, exporter state), and decoding/converting payloads stays outside.
//!
//! Claim rules in source order of the exporter inventory:
//!
//! - `DefaultYamlExporter` always claims into a plain asset collection.
//! - `ManagerAssetExporter` claims global managers and player-settings objects, routing editor
//!   build settings to their own collection.
//! - `ScriptableObjectExporter` claims MonoBehaviours, routing components on GameObjects to the
//!   shared empty collection.
//! - `ScriptableObjectGroupExporter` claims scriptable-object groups via their main asset.
//! - `SceneYamlExporter` claims scene and prefab hierarchies via their main asset.
//! - `YamlStreamedAssetExporter` claims meshes and image textures.
//! - `AnimatorControllerExporter` claims animator controllers via their main asset.
//! - `AudioMixerExporter` always claims: mixer controllers map to the mixer collection and
//!   everything else reaching it maps to a fail collection.
//! - `TerrainYamlExporter` claims terrain data via its main asset.
//! - `YamlSpriteExporter` claims sprites into an asset collection and sprite atlases into the
//!   shared empty collection, declining everything else.
//! - `TextureAssetExporter` claims sprite-information mains, except bare sprites when sprite
//!   export is disabled (`SpriteExportMode` is YAML).
//! - `RawTextureExporter` claims every 2D texture; `TextureArrayAssetExporter` claims image
//!   textures with intact data and no main asset; `LightmapTextureAssetExporter` claims
//!   lighting-data mains.
//! - `FontAssetExporter`, `MovieTextureAssetExporter`, and `TextAssetExporter` claim their asset
//!   only with valid payloads (non-empty font/movie data, non-empty script); `VideoClipExporter`
//!   additionally requires intact external resources.
//! - `AudioClipExporter` claims decodable non-empty clips, overriding the extension to `wav`
//!   when `PreferWav` meets an `ogg` decode; `NativeAudioExporter` and `YamlAudioExporter`
//!   claim every audio clip. Only `Default` and `PreferWav` are supported converted formats.
//! - `SimpleShaderExporter` claims shaders carrying decompiled source text (non-empty, without
//!   `Program` or `SubProgram`); the other shader exporters claim every shader.
//! - `ScriptExporter` claims MonoScripts: the first claim creates the script (or empty-script,
//!   without an assembly manager) collection and later claims redirect.
//! - `UnknownObjectExporter` and `UnreadableObjectExporter` claim their raw asset kinds, and
//!   `SceneAssetExporter` claims scene assets.
//! - `EngineAssetsExporter` redirects engine files and cache hits, declines the reserved font
//!   material/texture names, and declines everything else.
//! - `DeletedAssetsExporter` claims deleted-information mains.

use super::settings::AudioExportFormat;

/// The asset itself, named after the source interface the claim checks.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ClaimAssetKind {
    GlobalGameManager,
    PlayerSettingsObject,
    EditorBuildSettings,
    MonoBehaviour,
    Mesh,
    ImageTexture,
    Texture2D,
    TextureArray,
    Sprite,
    SpriteAtlas,
    MovieTexture,
    TextAsset,
    VideoClip,
    AudioClip,
    Shader,
    MonoScript,
    Font,
    Material,
    SceneAsset,
    UnknownObject,
    UnreadableObject,
    Other,
}

/// The asset's main asset, named after the source interface the claim checks.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ClaimMainKind {
    Absent,
    SpriteInformation,
    SceneHierarchy,
    PrefabHierarchy,
    ScriptableObjectGroup,
    TerrainData,
    AnimatorController,
    AudioMixerController,
    DeletedInformation,
    LightingData,
    Font,
    Other,
}

/// The `AudioClipDecoder.TryDecode` outcome driving the audio-clip claim.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AudioClaimDecode {
    /// Decoding failed: warn and decline.
    Undecodable,
    /// Decoding succeeded but produced no data: warn and decline.
    Empty,
    /// Decoding succeeded; `is_ogg` selects the `PreferWav` override.
    Decoded { is_ogg: bool },
}

/// The shader source-text state driving the simple-shader claim.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ShaderTextState {
    /// The shader has no script payload.
    NoScript,
    /// Non-empty script without `Program` or `SubProgram`.
    DecompiledSource,
    /// Empty script, or one still containing compiled program sections.
    CompiledOrEmpty,
}

/// Every caller-supplied fact the claim predicates read.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ClaimFacts {
    pub asset: ClaimAssetKind,
    pub main: ClaimMainKind,
    /// `MonoBehaviour.IsComponentOnGameObject` for the scriptable-object branch.
    pub on_game_object: bool,
    /// The exporter-specific validity check: non-empty font/movie/script data, intact video
    /// external resources, intact texture-array data.
    pub data_valid: bool,
    /// `SpriteExportMode` is not YAML, enabling converted sprite export.
    pub export_sprites: bool,
    pub audio_decode: AudioClaimDecode,
    /// `AudioExportFormat` is `PreferWav`.
    pub prefer_wav: bool,
    pub shader_text: ShaderTextState,
    /// `ScriptExporter` has already claimed once (`HasDecompiled`).
    pub script_claimed_before: bool,
    /// `ScriptExporter.AssemblyManager.IsSet`.
    pub assembly_manager_is_set: bool,
    /// The asset file is a default-resource or builtin-extra engine file.
    pub engine_file: bool,
    /// The asset carries a reserved font name (`Font Material`, `Font Texture`).
    pub reserved_font_name: bool,
    /// The engine-asset cache contains the asset or sprite texture.
    pub cache_hit: bool,
}

impl Default for ClaimFacts {
    fn default() -> Self {
        Self {
            asset: ClaimAssetKind::Other,
            main: ClaimMainKind::Other,
            on_game_object: false,
            data_valid: false,
            export_sprites: false,
            audio_decode: AudioClaimDecode::Undecodable,
            prefer_wav: false,
            shader_text: ShaderTextState::NoScript,
            script_claimed_before: false,
            assembly_manager_is_set: false,
            engine_file: false,
            reserved_font_name: false,
            cache_hit: false,
        }
    }
}

/// What a claim produces when it accepts.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ClaimOutcome {
    /// The exporter declines; the stack tries the next exporter.
    Decline,
    /// The named export collection is created.
    Collection(&'static str),
    /// The shared `EmptyExportCollection.Instance` is returned.
    EmptyCollection,
    /// A `FailExportCollection` is created: still a claim, exporting nothing.
    FailCollection,
    /// A redirect collection pointing at engine or script assets.
    RedirectCollection,
}

/// Run one exporter's `TryCreateCollection` predicate over caller-supplied facts.
///
/// The returned collection names are the exact baseline types. Unknown exporter names
/// decline, keeping the stack walk total without inventing handlers.
pub fn claim_collection(exporter: &str, facts: &ClaimFacts) -> ClaimOutcome {
    match exporter {
        "DefaultYamlExporter" => ClaimOutcome::Collection("AssetExportCollection"),
        "ManagerAssetExporter" => match facts.asset {
            ClaimAssetKind::EditorBuildSettings => {
                ClaimOutcome::Collection("EditorBuildSettingsExportCollection")
            }
            ClaimAssetKind::GlobalGameManager | ClaimAssetKind::PlayerSettingsObject => {
                ClaimOutcome::Collection("ManagerExportCollection")
            }
            _ => ClaimOutcome::Decline,
        },
        "ScriptableObjectExporter" => match facts.asset {
            ClaimAssetKind::MonoBehaviour if facts.on_game_object => ClaimOutcome::EmptyCollection,
            ClaimAssetKind::MonoBehaviour => {
                ClaimOutcome::Collection("ScriptableObjectExportCollection")
            }
            _ => ClaimOutcome::Decline,
        },
        "ScriptableObjectGroupExporter" => match facts.main {
            ClaimMainKind::ScriptableObjectGroup => {
                ClaimOutcome::Collection("ScriptableObjectGroupExportCollection")
            }
            _ => ClaimOutcome::Decline,
        },
        "SceneYamlExporter" => match facts.main {
            ClaimMainKind::SceneHierarchy => ClaimOutcome::Collection("SceneExportCollection"),
            ClaimMainKind::PrefabHierarchy => ClaimOutcome::Collection("PrefabExportCollection"),
            _ => ClaimOutcome::Decline,
        },
        "YamlStreamedAssetExporter" => match facts.asset {
            ClaimAssetKind::Mesh | ClaimAssetKind::ImageTexture => {
                ClaimOutcome::Collection("YamlStreamedAssetExportCollection")
            }
            _ => ClaimOutcome::Decline,
        },
        "AnimatorControllerExporter" => match facts.main {
            ClaimMainKind::AnimatorController => {
                ClaimOutcome::Collection("AnimatorControllerExportCollection")
            }
            _ => ClaimOutcome::Decline,
        },
        // Mixer groups and snapshots serialize into their mixer's YAML asset; anything else
        // reaching this exporter still claims, but as a fail collection.
        "AudioMixerExporter" => match facts.main {
            ClaimMainKind::AudioMixerController => {
                ClaimOutcome::Collection("AudioMixerExportCollection")
            }
            _ => ClaimOutcome::FailCollection,
        },
        "TerrainYamlExporter" => match facts.main {
            ClaimMainKind::TerrainData => ClaimOutcome::Collection("TerrainYamlExportCollection"),
            _ => ClaimOutcome::Decline,
        },
        "YamlSpriteExporter" => match facts.asset {
            ClaimAssetKind::Sprite => ClaimOutcome::Collection("AssetExportCollection"),
            ClaimAssetKind::SpriteAtlas => ClaimOutcome::EmptyCollection,
            _ => ClaimOutcome::Decline,
        },
        "TextureAssetExporter" => match facts.main {
            ClaimMainKind::SpriteInformation
                if facts.export_sprites || facts.asset != ClaimAssetKind::Sprite =>
            {
                ClaimOutcome::Collection("TextureExportCollection")
            }
            _ => ClaimOutcome::Decline,
        },
        "RawTextureExporter" => match facts.asset {
            ClaimAssetKind::Texture2D => ClaimOutcome::Collection("RawTextureExportCollection"),
            _ => ClaimOutcome::Decline,
        },
        "TextureArrayAssetExporter" => match facts.asset {
            ClaimAssetKind::TextureArray
                if facts.data_valid && facts.main == ClaimMainKind::Absent =>
            {
                ClaimOutcome::Collection("TextureArrayAssetExportCollection")
            }
            _ => ClaimOutcome::Decline,
        },
        "LightmapTextureAssetExporter" => match facts.main {
            ClaimMainKind::LightingData => ClaimOutcome::Collection("LightmapExportCollection"),
            _ => ClaimOutcome::Decline,
        },
        "FontAssetExporter" => match (facts.asset, facts.main) {
            (ClaimAssetKind::Font, ClaimMainKind::Font) if facts.data_valid => {
                ClaimOutcome::Collection("FontAssetExportCollection")
            }
            _ => ClaimOutcome::Decline,
        },
        "MovieTextureAssetExporter" => match facts.asset {
            ClaimAssetKind::MovieTexture if facts.data_valid => {
                ClaimOutcome::Collection("MovieTextureAssetExportCollection")
            }
            _ => ClaimOutcome::Decline,
        },
        "TextAssetExporter" => match facts.asset {
            ClaimAssetKind::TextAsset if facts.data_valid => {
                ClaimOutcome::Collection("TextAssetExportCollection")
            }
            _ => ClaimOutcome::Decline,
        },
        "VideoClipExporter" => match facts.asset {
            ClaimAssetKind::VideoClip if facts.data_valid => {
                ClaimOutcome::Collection("VideoClipExportCollection")
            }
            _ => ClaimOutcome::Decline,
        },
        "AudioClipExporter" => match (facts.asset, facts.audio_decode) {
            (ClaimAssetKind::AudioClip, AudioClaimDecode::Decoded { .. }) => {
                ClaimOutcome::Collection("AudioClipExportCollection")
            }
            _ => ClaimOutcome::Decline,
        },
        "NativeAudioExporter" | "YamlAudioExporter" => match facts.asset {
            ClaimAssetKind::AudioClip => {
                ClaimOutcome::Collection(if exporter == "NativeAudioExporter" {
                    "NativeAudioExportCollection"
                } else {
                    "YamlAudioExportCollection"
                })
            }
            _ => ClaimOutcome::Decline,
        },
        "SimpleShaderExporter" => match (facts.asset, facts.shader_text) {
            (ClaimAssetKind::Shader, ShaderTextState::DecompiledSource) => {
                ClaimOutcome::Collection("ShaderExportCollection")
            }
            _ => ClaimOutcome::Decline,
        },
        "DummyShaderTextExporter" | "YamlShaderExporter" => match facts.asset {
            ClaimAssetKind::Shader => ClaimOutcome::Collection("ShaderExportCollection"),
            _ => ClaimOutcome::Decline,
        },
        "ScriptExporter" => match facts.asset {
            ClaimAssetKind::MonoScript if facts.script_claimed_before => {
                ClaimOutcome::RedirectCollection
            }
            ClaimAssetKind::MonoScript if facts.assembly_manager_is_set => {
                ClaimOutcome::Collection("ScriptExportCollection")
            }
            ClaimAssetKind::MonoScript => ClaimOutcome::Collection("EmptyScriptExportCollection"),
            _ => ClaimOutcome::Decline,
        },
        "UnknownObjectExporter" => match facts.asset {
            ClaimAssetKind::UnknownObject => ClaimOutcome::Collection("UnknownExportCollection"),
            _ => ClaimOutcome::Decline,
        },
        "UnreadableObjectExporter" => match facts.asset {
            ClaimAssetKind::UnreadableObject => {
                ClaimOutcome::Collection("UnreadableExportCollection")
            }
            _ => ClaimOutcome::Decline,
        },
        "SceneAssetExporter" => match facts.asset {
            ClaimAssetKind::SceneAsset => ClaimOutcome::Collection("SceneAssetExportCollection"),
            _ => ClaimOutcome::Decline,
        },
        "EngineAssetsExporter" => {
            if facts.engine_file {
                return ClaimOutcome::RedirectCollection;
            }
            match facts.asset {
                ClaimAssetKind::Material | ClaimAssetKind::Texture2D
                    if facts.reserved_font_name =>
                {
                    ClaimOutcome::Decline
                }
                _ => match facts.main {
                    ClaimMainKind::SpriteInformation if facts.cache_hit => {
                        ClaimOutcome::RedirectCollection
                    }
                    _ if facts.cache_hit => ClaimOutcome::RedirectCollection,
                    _ => ClaimOutcome::Decline,
                },
            }
        }
        "DeletedAssetsExporter" => match facts.main {
            ClaimMainKind::DeletedInformation => {
                ClaimOutcome::Collection("DeletedAssetsExportCollection")
            }
            _ => ClaimOutcome::Decline,
        },
        _ => ClaimOutcome::Decline,
    }
}

/// `AudioClipExporter.IsSupportedExportFormat`: only `Default` and `PreferWav` convert.
pub const fn is_supported_audio_export_format(format: AudioExportFormat) -> bool {
    matches!(
        format,
        AudioExportFormat::Default | AudioExportFormat::PreferWav
    )
}

/// The `AudioClipExportCollection` extension: `PreferWav` meeting an `ogg` decode selects
/// `wav`, otherwise the decoded extension is kept.
pub fn audio_clip_extension(decoded_extension: &str, prefer_wav: bool, is_ogg: bool) -> &str {
    if prefer_wav && is_ogg {
        "wav"
    } else {
        decoded_extension
    }
}

/// `SimpleShaderExporter.HasDecompiledShaderText`: non-empty text with no compiled sections.
///
/// Both `Contains` checks are ordinal and case-sensitive like the source.
pub fn has_decompiled_shader_text(text: &str) -> bool {
    !text.is_empty() && !text.contains("Program") && !text.contains("SubProgram")
}

#[cfg(test)]
mod tests {
    use super::super::settings::AudioExportFormat;
    use super::{
        audio_clip_extension, claim_collection, has_decompiled_shader_text,
        is_supported_audio_export_format, AudioClaimDecode, ClaimAssetKind, ClaimFacts,
        ClaimMainKind, ClaimOutcome, ShaderTextState,
    };

    fn facts(asset: ClaimAssetKind, main: ClaimMainKind) -> ClaimFacts {
        ClaimFacts {
            asset,
            main,
            ..ClaimFacts::default()
        }
    }

    #[test]
    fn yaml_and_manager_claims_follow_asset_and_main_kinds() {
        use ClaimAssetKind::{
            EditorBuildSettings, GlobalGameManager, MonoBehaviour, PlayerSettingsObject,
        };
        use ClaimMainKind::{PrefabHierarchy, SceneHierarchy, ScriptableObjectGroup};
        // The default exporter is the catch-all.
        assert_eq!(
            claim_collection("DefaultYamlExporter", &ClaimFacts::default()),
            ClaimOutcome::Collection("AssetExportCollection")
        );
        assert_eq!(
            claim_collection(
                "ManagerAssetExporter",
                &facts(GlobalGameManager, ClaimMainKind::Other)
            ),
            ClaimOutcome::Collection("ManagerExportCollection")
        );
        assert_eq!(
            claim_collection(
                "ManagerAssetExporter",
                &facts(PlayerSettingsObject, ClaimMainKind::Other)
            ),
            ClaimOutcome::Collection("ManagerExportCollection")
        );
        assert_eq!(
            claim_collection(
                "ManagerAssetExporter",
                &facts(EditorBuildSettings, ClaimMainKind::Other)
            ),
            ClaimOutcome::Collection("EditorBuildSettingsExportCollection")
        );
        assert_eq!(
            claim_collection(
                "ManagerAssetExporter",
                &facts(MonoBehaviour, ClaimMainKind::Other)
            ),
            ClaimOutcome::Decline
        );
        // Components on GameObjects go to the shared empty collection.
        let on_object = ClaimFacts {
            asset: MonoBehaviour,
            on_game_object: true,
            ..ClaimFacts::default()
        };
        assert_eq!(
            claim_collection("ScriptableObjectExporter", &on_object),
            ClaimOutcome::EmptyCollection
        );
        assert_eq!(
            claim_collection(
                "ScriptableObjectExporter",
                &facts(MonoBehaviour, ClaimMainKind::Other)
            ),
            ClaimOutcome::Collection("ScriptableObjectExportCollection")
        );
        assert_eq!(
            claim_collection(
                "ScriptableObjectGroupExporter",
                &facts(MonoBehaviour, ScriptableObjectGroup)
            ),
            ClaimOutcome::Collection("ScriptableObjectGroupExportCollection")
        );
        assert_eq!(
            claim_collection("SceneYamlExporter", &facts(MonoBehaviour, SceneHierarchy)),
            ClaimOutcome::Collection("SceneExportCollection")
        );
        assert_eq!(
            claim_collection("SceneYamlExporter", &facts(MonoBehaviour, PrefabHierarchy)),
            ClaimOutcome::Collection("PrefabExportCollection")
        );
        assert_eq!(
            claim_collection(
                "SceneYamlExporter",
                &facts(MonoBehaviour, ClaimMainKind::Other)
            ),
            ClaimOutcome::Decline
        );
    }

    #[test]
    fn mixer_never_declines_and_sprite_modes_branch() {
        use ClaimAssetKind::{Mesh, Sprite, SpriteAtlas};
        assert_eq!(
            claim_collection(
                "AudioMixerExporter",
                &facts(Mesh, ClaimMainKind::AudioMixerController)
            ),
            ClaimOutcome::Collection("AudioMixerExportCollection")
        );
        // Anything else reaching the mixer exporter still claims, as a fail collection.
        assert_eq!(
            claim_collection("AudioMixerExporter", &facts(Mesh, ClaimMainKind::Other)),
            ClaimOutcome::FailCollection
        );
        assert_eq!(
            claim_collection("YamlSpriteExporter", &facts(Sprite, ClaimMainKind::Other)),
            ClaimOutcome::Collection("AssetExportCollection")
        );
        assert_eq!(
            claim_collection(
                "YamlSpriteExporter",
                &facts(SpriteAtlas, ClaimMainKind::Other)
            ),
            ClaimOutcome::EmptyCollection
        );
        assert_eq!(
            claim_collection("YamlSpriteExporter", &facts(Mesh, ClaimMainKind::Other)),
            ClaimOutcome::Decline
        );
    }

    #[test]
    fn texture_claims_follow_mains_validity_and_sprite_mode() {
        use ClaimAssetKind::{ImageTexture, Sprite, Texture2D, TextureArray};
        use ClaimMainKind::{LightingData, SpriteInformation};
        // Bare sprites are skipped only when sprite export is disabled.
        let bare_sprite = ClaimFacts {
            asset: Sprite,
            main: SpriteInformation,
            ..ClaimFacts::default()
        };
        assert_eq!(
            claim_collection("TextureAssetExporter", &bare_sprite),
            ClaimOutcome::Decline
        );
        let enabled_sprite = ClaimFacts {
            export_sprites: true,
            ..bare_sprite
        };
        assert_eq!(
            claim_collection("TextureAssetExporter", &enabled_sprite),
            ClaimOutcome::Collection("TextureExportCollection")
        );
        // Non-sprite assets with sprite information always claim.
        assert_eq!(
            claim_collection("TextureAssetExporter", &facts(Texture2D, SpriteInformation)),
            ClaimOutcome::Collection("TextureExportCollection")
        );
        assert_eq!(
            claim_collection(
                "RawTextureExporter",
                &facts(Texture2D, ClaimMainKind::Other)
            ),
            ClaimOutcome::Collection("RawTextureExportCollection")
        );
        assert_eq!(
            claim_collection(
                "RawTextureExporter",
                &facts(ImageTexture, ClaimMainKind::Other)
            ),
            ClaimOutcome::Decline
        );
        // Arrays need intact data and no main asset.
        let array = ClaimFacts {
            asset: TextureArray,
            main: ClaimMainKind::Absent,
            data_valid: true,
            ..ClaimFacts::default()
        };
        assert_eq!(
            claim_collection("TextureArrayAssetExporter", &array),
            ClaimOutcome::Collection("TextureArrayAssetExportCollection")
        );
        let array_with_main = ClaimFacts {
            main: SpriteInformation,
            ..array
        };
        assert_eq!(
            claim_collection("TextureArrayAssetExporter", &array_with_main),
            ClaimOutcome::Decline
        );
        assert_eq!(
            claim_collection(
                "LightmapTextureAssetExporter",
                &facts(Texture2D, LightingData)
            ),
            ClaimOutcome::Collection("LightmapExportCollection")
        );
    }

    #[test]
    fn payload_gated_binary_claims_need_valid_data() {
        use ClaimAssetKind::{AudioClip, Font, MovieTexture, TextAsset, VideoClip};
        use ClaimMainKind::Font as FontMain;
        let font = ClaimFacts {
            asset: Font,
            main: FontMain,
            data_valid: true,
            ..ClaimFacts::default()
        };
        assert_eq!(
            claim_collection("FontAssetExporter", &font),
            ClaimOutcome::Collection("FontAssetExportCollection")
        );
        let empty_font = ClaimFacts {
            data_valid: false,
            ..font
        };
        assert_eq!(
            claim_collection("FontAssetExporter", &empty_font),
            ClaimOutcome::Decline
        );
        // The font main alone is not enough without the matching asset kind.
        assert_eq!(
            claim_collection("FontAssetExporter", &facts(ClaimAssetKind::Other, FontMain)),
            ClaimOutcome::Decline
        );
        for (exporter, asset, collection) in [
            (
                "MovieTextureAssetExporter",
                MovieTexture,
                "MovieTextureAssetExportCollection",
            ),
            ("TextAssetExporter", TextAsset, "TextAssetExportCollection"),
            ("VideoClipExporter", VideoClip, "VideoClipExportCollection"),
        ] {
            let valid = ClaimFacts {
                asset,
                data_valid: true,
                ..ClaimFacts::default()
            };
            assert_eq!(
                claim_collection(exporter, &valid),
                ClaimOutcome::Collection(collection),
                "{exporter}"
            );
            let invalid = ClaimFacts {
                data_valid: false,
                ..valid
            };
            assert_eq!(
                claim_collection(exporter, &invalid),
                ClaimOutcome::Decline,
                "{exporter}"
            );
        }
        // Native and YAML audio exporters claim every clip.
        assert_eq!(
            claim_collection(
                "NativeAudioExporter",
                &facts(AudioClip, ClaimMainKind::Other)
            ),
            ClaimOutcome::Collection("NativeAudioExportCollection")
        );
        assert_eq!(
            claim_collection("YamlAudioExporter", &facts(AudioClip, ClaimMainKind::Other)),
            ClaimOutcome::Collection("YamlAudioExportCollection")
        );
    }

    #[test]
    fn audio_clip_claims_follow_decode_and_wav_override() {
        use ClaimAssetKind::AudioClip;
        let decoded_ogg = ClaimFacts {
            asset: AudioClip,
            audio_decode: AudioClaimDecode::Decoded { is_ogg: true },
            prefer_wav: true,
            ..ClaimFacts::default()
        };
        assert_eq!(
            claim_collection("AudioClipExporter", &decoded_ogg),
            ClaimOutcome::Collection("AudioClipExportCollection")
        );
        // Failed and empty decodes warn and decline.
        for decode in [AudioClaimDecode::Undecodable, AudioClaimDecode::Empty] {
            let declined = ClaimFacts {
                asset: AudioClip,
                audio_decode: decode,
                ..ClaimFacts::default()
            };
            assert_eq!(
                claim_collection("AudioClipExporter", &declined),
                ClaimOutcome::Decline
            );
        }
        // The wav override applies only to PreferWav meeting ogg.
        assert_eq!(audio_clip_extension("ogg", true, true), "wav");
        assert_eq!(audio_clip_extension("ogg", false, true), "ogg");
        assert_eq!(audio_clip_extension("ogg", true, false), "ogg");
        assert_eq!(audio_clip_extension("mp3", true, false), "mp3");
        // Only Default and PreferWav are supported converted formats.
        assert!(is_supported_audio_export_format(AudioExportFormat::Default));
        assert!(is_supported_audio_export_format(
            AudioExportFormat::PreferWav
        ));
        assert!(!is_supported_audio_export_format(AudioExportFormat::Native));
        assert!(!is_supported_audio_export_format(AudioExportFormat::Yaml));
    }

    #[test]
    fn shader_script_engine_and_raw_claims_follow_source_branches() {
        use ClaimAssetKind::{
            Material, MonoScript, SceneAsset, Shader, UnknownObject, UnreadableObject,
        };
        use ClaimMainKind::DeletedInformation;
        // Only decompiled source text is claimed by the simple exporter.
        let decompiled = ClaimFacts {
            asset: Shader,
            shader_text: ShaderTextState::DecompiledSource,
            ..ClaimFacts::default()
        };
        assert_eq!(
            claim_collection("SimpleShaderExporter", &decompiled),
            ClaimOutcome::Collection("ShaderExportCollection")
        );
        for text in [ShaderTextState::NoScript, ShaderTextState::CompiledOrEmpty] {
            let declined = ClaimFacts {
                shader_text: text,
                ..decompiled
            };
            assert_eq!(
                claim_collection("SimpleShaderExporter", &declined),
                ClaimOutcome::Decline
            );
        }
        assert!(has_decompiled_shader_text("Shader \"x\" { }"));
        assert!(!has_decompiled_shader_text(""));
        assert!(!has_decompiled_shader_text("SubProgram \"d3d11\" { }"));
        assert!(!has_decompiled_shader_text("Program \"vp\" { }"));
        // The remaining shader exporters claim every shader.
        for exporter in ["DummyShaderTextExporter", "YamlShaderExporter"] {
            assert_eq!(
                claim_collection(exporter, &facts(Shader, ClaimMainKind::Other)),
                ClaimOutcome::Collection("ShaderExportCollection"),
                "{exporter}"
            );
        }
        // Script claims are stateful: first creates, later redirects.
        let first = ClaimFacts {
            asset: MonoScript,
            assembly_manager_is_set: true,
            ..ClaimFacts::default()
        };
        assert_eq!(
            claim_collection("ScriptExporter", &first),
            ClaimOutcome::Collection("ScriptExportCollection")
        );
        let first_unmanaged = ClaimFacts {
            assembly_manager_is_set: false,
            ..first
        };
        assert_eq!(
            claim_collection("ScriptExporter", &first_unmanaged),
            ClaimOutcome::Collection("EmptyScriptExportCollection")
        );
        let later = ClaimFacts {
            script_claimed_before: true,
            ..first
        };
        assert_eq!(
            claim_collection("ScriptExporter", &later),
            ClaimOutcome::RedirectCollection
        );
        // Engine files and cache hits redirect; reserved font names decline.
        let engine = ClaimFacts {
            engine_file: true,
            ..ClaimFacts::default()
        };
        assert_eq!(
            claim_collection("EngineAssetsExporter", &engine),
            ClaimOutcome::RedirectCollection
        );
        let reserved = ClaimFacts {
            asset: Material,
            reserved_font_name: true,
            cache_hit: true,
            ..ClaimFacts::default()
        };
        assert_eq!(
            claim_collection("EngineAssetsExporter", &reserved),
            ClaimOutcome::Decline
        );
        let cached = ClaimFacts {
            cache_hit: true,
            ..ClaimFacts::default()
        };
        assert_eq!(
            claim_collection("EngineAssetsExporter", &cached),
            ClaimOutcome::RedirectCollection
        );
        assert_eq!(
            claim_collection("EngineAssetsExporter", &ClaimFacts::default()),
            ClaimOutcome::Decline
        );
        // Raw and deleted claims follow their asset kinds.
        assert_eq!(
            claim_collection(
                "UnknownObjectExporter",
                &facts(UnknownObject, ClaimMainKind::Other)
            ),
            ClaimOutcome::Collection("UnknownExportCollection")
        );
        assert_eq!(
            claim_collection(
                "UnreadableObjectExporter",
                &facts(UnreadableObject, ClaimMainKind::Other)
            ),
            ClaimOutcome::Collection("UnreadableExportCollection")
        );
        assert_eq!(
            claim_collection(
                "DeletedAssetsExporter",
                &facts(Material, DeletedInformation)
            ),
            ClaimOutcome::Collection("DeletedAssetsExportCollection")
        );
        assert_eq!(
            claim_collection(
                "SceneAssetExporter",
                &facts(SceneAsset, ClaimMainKind::Other)
            ),
            ClaimOutcome::Collection("SceneAssetExportCollection")
        );
        assert_eq!(
            claim_collection("SceneAssetExporter", &facts(Material, ClaimMainKind::Other)),
            ClaimOutcome::Decline
        );
    }
}
