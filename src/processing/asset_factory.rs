//! Asset-creation dispatch ported from `AssetRipper.Import.AssetCreation.GameAssetFactory`.
//!
//! Covers the first audited slice of `SER-TT-ABSENT-001`: the `ReadAsset` top dispatch,
//! the `ReadNormalObject` fallback chain, the `TryReadNormalObject` tail rules, the
//! `CreateAsset` three outcomes, and the `CreateEngineAsset` name switch. Byte reading,
//! node building, and assembly-backed MonoBehaviour structures stay outside: they need
//! generated objects and live assemblies. Version facts arrive precomputed because the
//! Rust tree has no parsed Unity-version model yet.
//!
//! Differential evidence: the `ENGINE` rows of `mono_utils_fixture.txt` (probe
//! `Temp/opencode/monoutilsprobe` resolving all 21 engine names at 2019.4.0f1, SDK
//! 10.0.401); the dispatch and fallback rules are source-read over caller facts.

/// The class ID routed to the MonoBehaviour read path.
pub const MONO_BEHAVIOUR_CLASS_ID: i32 = 114;

/// Trailing bytes that mark the non-standard Chinese texture layout.
pub const CHINESE_TEXTURE_EXTRA_BYTES: usize = 24;

/// The `ReadAsset` top dispatch.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AssetReadPath {
    /// Below Unity 3.5 (or a stripped version): unreadable without reading.
    UnreadableUnsupportedVersion,
    /// Class 114: the MonoBehaviour path (needs an assembly manager).
    MonoBehaviour,
    /// Every other class: the normal generated-or-TypeTree path.
    Normal,
}

/// `ReadAsset`: stripped/pre-3.5 versions are unreadable, MonoBehaviours take the
/// script path, and everything else reads normally.
pub fn classify_read_path(version_before_3_5: bool, class_id: i32) -> AssetReadPath {
    if version_before_3_5 {
        AssetReadPath::UnreadableUnsupportedVersion
    } else if class_id == MONO_BEHAVIOUR_CLASS_ID {
        AssetReadPath::MonoBehaviour
    } else {
        AssetReadPath::Normal
    }
}

/// How a normal read consumes its payload (`TryReadNormalObject` tail rules).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ObjectTailOutcome {
    /// Every byte consumed: success.
    Exact,
    /// Only zero bytes remain: assumed alignment padding.
    ZeroPadding,
    /// Exactly 24 bytes remain on a Texture2D: non-standard Chinese fields.
    ChineseTextureFields,
    /// Anything else: an incorrect-byte-count error.
    IncorrectByteCount,
}

/// Classify the bytes left after a normal read: exact, all-zero padding, the
/// 24-byte Chinese texture tail, or an error. `remaining` is the unread suffix.
pub fn classify_object_tail(remaining: &[u8], is_texture2d: bool) -> ObjectTailOutcome {
    if remaining.is_empty() {
        ObjectTailOutcome::Exact
    } else if remaining.iter().all(|byte| *byte == 0) {
        ObjectTailOutcome::ZeroPadding
    } else if remaining.len() == CHINESE_TEXTURE_EXTRA_BYTES && is_texture2d {
        ObjectTailOutcome::ChineseTextureFields
    } else {
        ObjectTailOutcome::IncorrectByteCount
    }
}

/// Which asset a failed normal read resolves to (`ReadNormalObject` fallback chain).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum NormalObjectOutcome {
    /// The first asset stands (read cleanly, or an engine-collection tolerance).
    UseFirstAsset,
    /// The Patch-version retry with a bumped build read cleanly.
    UseRetriedAsset,
    /// An `UnreadableObject` preserving the asset identity (and INamed name).
    UseUnreadable,
}

/// The log the fallback chain emits alongside its outcome.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum NormalObjectLog {
    /// Clean reads and successful retries stay silent.
    Silent,
    /// Engine-collection shaders note the engine type trees.
    InfoEngineShaderTrees,
    /// Engine-collection failures warn with the read error.
    WarningReadError,
    /// Final failures log the read error.
    ErrorReadError,
}

/// Caller facts behind one `ReadNormalObject` decision.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NormalReadFacts {
    /// The first read consumed its payload cleanly.
    pub first_read_ok: bool,
    /// The owning collection is a default resource or builtin extra.
    pub is_engine_collection: bool,
    /// The asset is a shader (info, not warning, on engine collections).
    pub is_shader: bool,
    /// The collection version is a Patch type (eligible for the build-bump retry).
    pub version_is_patch: bool,
    /// The bumped-build retry consumed its payload cleanly.
    pub retry_read_ok: bool,
}

/// `ReadNormalObject`: clean reads win silently; engine collections tolerate failures
/// (info for shaders, warning otherwise); Patch versions retry once with a bumped
/// build; everything else becomes unreadable with an error log.
pub fn decide_normal_object(facts: &NormalReadFacts) -> (NormalObjectOutcome, NormalObjectLog) {
    if facts.first_read_ok {
        (NormalObjectOutcome::UseFirstAsset, NormalObjectLog::Silent)
    } else if facts.is_engine_collection {
        let log = if facts.is_shader {
            NormalObjectLog::InfoEngineShaderTrees
        } else {
            NormalObjectLog::WarningReadError
        };
        (NormalObjectOutcome::UseFirstAsset, log)
    } else if facts.version_is_patch && facts.retry_read_ok {
        (
            NormalObjectOutcome::UseRetriedAsset,
            NormalObjectLog::Silent,
        )
    } else {
        (
            NormalObjectOutcome::UseUnreadable,
            NormalObjectLog::ErrorReadError,
        )
    }
}

/// The Patch-version retry version: same major/minor with a wrapping build bump
/// (`unchecked((ushort)(Build + 1))`).
pub fn bump_patch_build(major: u32, minor: u32, build: u16) -> (u32, u32, u16) {
    (major, minor, build.wrapping_add(1))
}

/// What `CreateAsset` produces: a generated serialized asset, a TPK-backed
/// `TypeTreeObject` (never needing a type resolver), or nothing (the caller wraps
/// an `UnknownObject` around the raw bytes).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CreatedAssetKind {
    Serialized,
    TypeTreeObject,
    Unknown,
}

/// `CreateAsset`: generated classes win, TPK type trees back engine assets, and a
/// miss leaves the unknown-object path.
pub fn classify_created_asset(serialized_exists: bool, tpk_exists: bool) -> CreatedAssetKind {
    if serialized_exists {
        CreatedAssetKind::Serialized
    } else if tpk_exists {
        CreatedAssetKind::TypeTreeObject
    } else {
        CreatedAssetKind::Unknown
    }
}

/// One `CreateEngineAsset` arm: unknown names throw `NotSupportedException`.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EngineAssetKind {
    Guid,
    Hash128,
    Vector2,
    Vector2Int,
    Vector3,
    Vector3Int,
    Vector4,
    Rect,
    Bounds,
    BoundsInt,
    Quaternion,
    Matrix4x4,
    Color,
    Color32,
    LayerMask,
    AnimationCurve,
    Gradient,
    RectOffset,
    GuiStyle,
    PropertyName,
    SphericalHarmonicsL2,
}

impl EngineAssetKind {
    /// Whether the source constructor takes the collection Unity version.
    pub const fn takes_version(self) -> bool {
        matches!(
            self,
            Self::Hash128
                | Self::AnimationCurve
                | Self::Gradient
                | Self::GuiStyle
                | Self::PropertyName
                | Self::SphericalHarmonicsL2
        )
    }

    /// The concrete subclass the probe resolved at 2019.4.0f1.
    pub const fn concrete_class_2019_4(self) -> &'static str {
        match self {
            Self::Guid => "AssetRipper.SourceGenerated.Subclasses.GUID.GUID",
            Self::Hash128 => "AssetRipper.SourceGenerated.Subclasses.Hash128.Hash128_5",
            Self::Vector2 => "AssetRipper.SourceGenerated.Subclasses.Vector2f.Vector2f",
            Self::Vector2Int => {
                "AssetRipper.SourceGenerated.Subclasses.Vector2Int.Vector2Int"
            }
            Self::Vector3 => "AssetRipper.SourceGenerated.Subclasses.Vector3f.Vector3f",
            Self::Vector3Int => {
                "AssetRipper.SourceGenerated.Subclasses.Vector3Int.Vector3Int"
            }
            Self::Vector4 => "AssetRipper.SourceGenerated.Subclasses.Vector4f.Vector4f",
            Self::Rect => "AssetRipper.SourceGenerated.Subclasses.Rectf.Rectf",
            Self::Bounds => "AssetRipper.SourceGenerated.Subclasses.AABB.AABB",
            Self::BoundsInt => "AssetRipper.SourceGenerated.Subclasses.AABBInt.AABBInt",
            Self::Quaternion => {
                "AssetRipper.SourceGenerated.Subclasses.Quaternionf.Quaternionf"
            }
            Self::Matrix4x4 => {
                "AssetRipper.SourceGenerated.Subclasses.Matrix4x4f.Matrix4x4f"
            }
            Self::Color => "AssetRipper.SourceGenerated.Subclasses.ColorRGBAf.ColorRGBAf",
            Self::Color32 => "AssetRipper.SourceGenerated.Subclasses.ColorRGBA32.ColorRGBA32",
            Self::LayerMask => "AssetRipper.SourceGenerated.Subclasses.LayerMask.LayerMask",
            Self::AnimationCurve => "AssetRipper.SourceGenerated.Subclasses.AnimationCurve_Single.AnimationCurve_Single_2018",
            Self::Gradient => "AssetRipper.SourceGenerated.Subclasses.Gradient.Gradient_5_6",
            Self::RectOffset => {
                "AssetRipper.SourceGenerated.Subclasses.RectOffset.RectOffset"
            }
            Self::GuiStyle => {
                "AssetRipper.SourceGenerated.Subclasses.GUIStyle.GUIStyle_2017_1"
            }
            Self::PropertyName => {
                "AssetRipper.SourceGenerated.Subclasses.PropertyName.PropertyName_2019"
            }
            Self::SphericalHarmonicsL2 => "AssetRipper.SourceGenerated.Subclasses.SphericalHarmonicsL2.SphericalHarmonicsL2_5",
        }
    }
}

/// The `CreateEngineAsset` name switch.
pub fn engine_asset_kind(name: &str) -> Option<EngineAssetKind> {
    match name {
        "GUID" => Some(EngineAssetKind::Guid),
        "Hash128" => Some(EngineAssetKind::Hash128),
        "Vector2" => Some(EngineAssetKind::Vector2),
        "Vector2Int" => Some(EngineAssetKind::Vector2Int),
        "Vector3" => Some(EngineAssetKind::Vector3),
        "Vector3Int" => Some(EngineAssetKind::Vector3Int),
        "Vector4" => Some(EngineAssetKind::Vector4),
        "Rect" => Some(EngineAssetKind::Rect),
        "Bounds" => Some(EngineAssetKind::Bounds),
        "BoundsInt" => Some(EngineAssetKind::BoundsInt),
        "Quaternion" => Some(EngineAssetKind::Quaternion),
        "Matrix4x4" => Some(EngineAssetKind::Matrix4x4),
        "Color" => Some(EngineAssetKind::Color),
        "Color32" => Some(EngineAssetKind::Color32),
        "LayerMask" => Some(EngineAssetKind::LayerMask),
        "AnimationCurve" => Some(EngineAssetKind::AnimationCurve),
        "Gradient" => Some(EngineAssetKind::Gradient),
        "RectOffset" => Some(EngineAssetKind::RectOffset),
        "GUIStyle" => Some(EngineAssetKind::GuiStyle),
        "PropertyName" => Some(EngineAssetKind::PropertyName),
        "SphericalHarmonicsL2" => Some(EngineAssetKind::SphericalHarmonicsL2),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_path_dispatch_matches_the_source_order() {
        assert_eq!(
            classify_read_path(true, 28),
            AssetReadPath::UnreadableUnsupportedVersion
        );
        assert_eq!(
            classify_read_path(false, MONO_BEHAVIOUR_CLASS_ID),
            AssetReadPath::MonoBehaviour
        );
        assert_eq!(classify_read_path(false, 28), AssetReadPath::Normal);
        // The version gate wins over the class dispatch.
        assert_eq!(
            classify_read_path(true, MONO_BEHAVIOUR_CLASS_ID),
            AssetReadPath::UnreadableUnsupportedVersion
        );
        assert_eq!(MONO_BEHAVIOUR_CLASS_ID, 114);
    }

    #[test]
    fn tail_classification_matches_the_source_branches() {
        assert_eq!(classify_object_tail(&[], false), ObjectTailOutcome::Exact);
        assert_eq!(
            classify_object_tail(&[0, 0, 0], true),
            ObjectTailOutcome::ZeroPadding
        );
        // Zero padding wins over the texture rule on all-zero tails.
        assert_eq!(
            classify_object_tail(&[0; 24], true),
            ObjectTailOutcome::ZeroPadding
        );
        assert_eq!(
            classify_object_tail(&[1; 24], true),
            ObjectTailOutcome::ChineseTextureFields
        );
        // The 24-byte rule needs a Texture2D.
        assert_eq!(
            classify_object_tail(&[1; 24], false),
            ObjectTailOutcome::IncorrectByteCount
        );
        assert_eq!(
            classify_object_tail(&[1, 2, 3], true),
            ObjectTailOutcome::IncorrectByteCount
        );
        assert_eq!(CHINESE_TEXTURE_EXTRA_BYTES, 24);
    }

    #[test]
    fn normal_fallback_chain_matches_the_source_order() {
        let clean = NormalReadFacts {
            first_read_ok: true,
            is_engine_collection: false,
            is_shader: false,
            version_is_patch: false,
            retry_read_ok: false,
        };
        assert_eq!(
            decide_normal_object(&clean),
            (NormalObjectOutcome::UseFirstAsset, NormalObjectLog::Silent)
        );
        // Engine collections tolerate failures: info for shaders, warning otherwise.
        let shader = NormalReadFacts {
            first_read_ok: false,
            is_engine_collection: true,
            is_shader: true,
            version_is_patch: true,
            retry_read_ok: true,
        };
        assert_eq!(
            decide_normal_object(&shader),
            (
                NormalObjectOutcome::UseFirstAsset,
                NormalObjectLog::InfoEngineShaderTrees
            )
        );
        let engine = NormalReadFacts {
            is_shader: false,
            ..shader
        };
        assert_eq!(
            decide_normal_object(&engine),
            (
                NormalObjectOutcome::UseFirstAsset,
                NormalObjectLog::WarningReadError
            )
        );
        // A successful Patch retry wins silently; a failed one falls through.
        let retry_ok = NormalReadFacts {
            first_read_ok: false,
            is_engine_collection: false,
            is_shader: false,
            version_is_patch: true,
            retry_read_ok: true,
        };
        assert_eq!(
            decide_normal_object(&retry_ok),
            (
                NormalObjectOutcome::UseRetriedAsset,
                NormalObjectLog::Silent
            )
        );
        let retry_failed = NormalReadFacts {
            retry_read_ok: false,
            ..retry_ok
        };
        assert_eq!(
            decide_normal_object(&retry_failed),
            (
                NormalObjectOutcome::UseUnreadable,
                NormalObjectLog::ErrorReadError
            )
        );
        // Non-patch failures go straight to unreadable.
        let plain = NormalReadFacts {
            version_is_patch: false,
            ..retry_failed
        };
        assert_eq!(
            decide_normal_object(&plain),
            (
                NormalObjectOutcome::UseUnreadable,
                NormalObjectLog::ErrorReadError
            )
        );
    }

    #[test]
    fn patch_build_bump_wraps_like_the_unchecked_cast() {
        assert_eq!(bump_patch_build(2019, 4, 40), (2019, 4, 41));
        assert_eq!(bump_patch_build(2022, 3, u16::MAX), (2022, 3, 0));
    }

    #[test]
    fn creation_outcomes_follow_the_fallback_order() {
        assert_eq!(
            classify_created_asset(true, true),
            CreatedAssetKind::Serialized
        );
        assert_eq!(
            classify_created_asset(false, true),
            CreatedAssetKind::TypeTreeObject
        );
        assert_eq!(
            classify_created_asset(false, false),
            CreatedAssetKind::Unknown
        );
    }

    /// Differential against the shipped `AssetRipper.Import.dll` engine table
    /// (probe `Temp/opencode/monoutilsprobe` at 2019.4.0f1, SDK 10.0.401): the
    /// `ENGINE` rows of `mono_utils_fixture.txt` pin every arm and the two
    /// rejections.
    #[test]
    fn engine_asset_names_match_the_reference_runtime() {
        let fixture = include_str!("mono_utils_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.split('|');
            if parts.next() != Some("ENGINE") {
                continue;
            }
            let name = parts.next().expect("a name");
            let outcome = parts.next().expect("an outcome");
            let detail = parts.next().expect("a detail");
            assert!(parts.next().is_none(), "{line}");
            match engine_asset_kind(name) {
                Some(kind) => {
                    assert_eq!(outcome, "ok", "{line}");
                    assert_eq!(kind.concrete_class_2019_4(), detail, "{line}");
                }
                None => assert_eq!(outcome, "NotSupportedException", "{line}"),
            }
            rows += 1;
        }
        assert_eq!(rows, 23, "all probed engine rows are asserted");
        assert!(EngineAssetKind::Gradient.takes_version());
        assert!(!EngineAssetKind::Vector3.takes_version());
    }
}
