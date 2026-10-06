//! Asset-creation dispatch ported from `AssetRipper.Import.AssetCreation.GameAssetFactory`.
//!
//! Covers the first audited slice of `SER-TT-ABSENT-001`: the `ReadAsset` top dispatch,
//! the `ReadNormalObject` fallback chain with its engine-collection gate, the
//! `TryReadNormalObject` tail rules, the `CreateAsset` three outcomes, the `CreateEngineAsset` name switch,
//! the `ReadMonoBehaviour` structure-source decision, the `MakeError_*` and
//! `LogMonoBehaviorReadException` diagnostic texts, the `RawDataObject`/`UnknownObject`/
//! `UnreadableObject` hash and synthetic-name rules, and the `ReadExtraTextureFields`
//! 24-byte tail layout. Byte reading,
//! node building, and assembly-backed MonoBehaviour structures stay outside: they need
//! generated objects and live assemblies. Version facts arrive precomputed because the
//! Rust tree has no parsed Unity-version model yet.
//!
//! Deliberately open: the generated `AssetFactory.CreateSerialized` class switch (emitted
//! by `AssetRipper.AssemblyDumper` `Pass940_MakeAssetFactory`, not snapshot logic), the
//! `TypeTreeNodeStruct`/`SerializableStructure` node building behind the MonoBehaviour
//! structure facts, the live-assembly `UnloadedStructure` materialization, and the actual
//! `EndianSpanReader` byte consumption behind the tail-layout offsets.
//!
//! Differential evidence: the `ENGINE` rows of `mono_utils_fixture.txt` (probe
//! `Temp/opencode/monoutilsprobe` resolving all 21 engine names at 2019.4.0f1, SDK
//! 10.0.401); the dispatch and fallback rules are source-read over caller facts.

use crate::resources::blend_shape::crc32_ieee;

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

/// `SpecialFileNames.IsDefaultResourceOrBuiltinExtra` as `ReadNormalObject` calls it:
/// the owning collection's raw name is a default resource or a builtin-extra file.
///
/// This is narrower than the engine-resource predicate (editor resources count as
/// engine resources but are not tolerated here), so the fallback chain must use
/// exactly this gate: only these collections keep failed reads instead of turning
/// them unreadable. The caller feeds the result into `NormalReadFacts.is_engine_collection`.
/// A missing name never matches, mirroring the source null comparison.
pub fn is_engine_collection_name(name: Option<&str>) -> bool {
    matches!(
        name,
        Some(
            "unity default resources"
                | "unity_default_resources"
                | "unity builtin extra"
                | "unity_builtin_extra"
        )
    )
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

/// `RawDataObject.RawDataHash`: a Crc32 hash of the raw bytes, reusing the
/// shipped-`Checksum.dll`-pinned words.
pub fn raw_data_hash(data: &[u8]) -> u32 {
    crc32_ieee(data)
}

/// `UnknownObject.Name`: always the synthetic `Unknown{ClassName}_{Hash:X}` form (the
/// setter is a no-op). The class name arrives as a caller fact.
pub fn unknown_asset_name(class_name: &str, raw_data_hash: u32) -> String {
    format!("Unknown{class_name}_{raw_data_hash:X}")
}

/// `UnreadableObject.Name`: the stored name unless it is null or empty, otherwise the
/// synthetic `Unreadable{ClassName}_{Hash:X}` form.
pub fn unreadable_asset_name(
    class_name: &str,
    stored_name: Option<&str>,
    raw_data_hash: u32,
) -> String {
    match stored_name {
        Some(name) if !name.is_empty() => name.to_owned(),
        _ => format!("Unreadable{class_name}_{raw_data_hash:X}"),
    }
}

/// Caller facts behind one `ReadMonoBehaviour` structure decision
/// (`GameAssetFactory.ReadMonoBehaviour`).
///
/// Node building, `SerializableStructure` reads, and assembly-backed `UnloadedStructure`
/// materialization stay caller-side: they need generated objects and live assemblies.
/// This kernel only decides which source the structure comes from and what is logged.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MonoBehaviourReadFacts {
    /// `monoBehaviour.Read` or the structure path threw; the layout-mismatch error
    /// fires and the previously stored structure is left untouched.
    pub read_threw: bool,
    /// `type is not null`: a serialized type carrying an old-type TypeTree is present.
    pub has_type_tree: bool,
    /// `TypeTreeNodeStruct.TryMakeFromTypeTree` accepted the old type.
    pub type_tree_parse_ok: bool,
    /// `structure.TryRead(...)` consumed the remainder cleanly.
    pub structure_try_read_ok: bool,
}

/// Where a freshly read MonoBehaviour's structure comes from.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MonoBehaviourStructureOutcome {
    /// The TypeTree-built structure read cleanly and is assigned to the behaviour.
    UseParsedStructure,
    /// The TypeTree-built structure failed its read: `Structure` is cleared to null.
    ClearStructure,
    /// No usable TypeTree: an `UnloadedStructure` lazily covers the bytes left unread
    /// by `monoBehaviour.Read` (`assetData.Slice(reader.Position)`).
    UseUnloadedRemainder,
    /// The read threw: no assignment happens; the prior structure stands.
    KeepExisting,
}

/// The log accompanying a MonoBehaviour read.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MonoBehaviourReadLog {
    /// Parsed-structure assignments and lazy fallbacks stay silent.
    Silent,
    /// Thrown reads log the layout-mismatch error.
    ErrorLayoutMismatch,
}

/// `ReadMonoBehaviour`: a thrown read keeps the prior structure with an error log; a
/// parsed TypeTree structure is assigned on a clean `TryRead` and cleared otherwise;
/// without a usable TypeTree the unread remainder backs an `UnloadedStructure`.
///
/// Both TypeTree conditions are required together: a present-but-unparseable type tree
/// falls through to the lazy remainder exactly like an absent one.
pub fn decide_monobehaviour_structure(
    facts: &MonoBehaviourReadFacts,
) -> (MonoBehaviourStructureOutcome, MonoBehaviourReadLog) {
    if facts.read_threw {
        (
            MonoBehaviourStructureOutcome::KeepExisting,
            MonoBehaviourReadLog::ErrorLayoutMismatch,
        )
    } else if facts.has_type_tree && facts.type_tree_parse_ok {
        if facts.structure_try_read_ok {
            (
                MonoBehaviourStructureOutcome::UseParsedStructure,
                MonoBehaviourReadLog::Silent,
            )
        } else {
            (
                MonoBehaviourStructureOutcome::ClearStructure,
                MonoBehaviourReadLog::Silent,
            )
        }
    } else {
        (
            MonoBehaviourStructureOutcome::UseUnloadedRemainder,
            MonoBehaviourReadLog::Silent,
        )
    }
}

/// `MakeError_IncorrectNumberOfBytesRead`: the exact diagnostic text. The class type
/// name, Unity version, platform, collection name, and file path arrive preformatted
/// as caller facts because the Rust tree has no parsed version or collection model.
pub fn incorrect_byte_count_message(
    position: i32,
    length: i32,
    class_type_name: &str,
    version: &str,
    platform: &str,
    collection_name: &str,
    file_path: &str,
) -> String {
    format!(
        "Read {position} but expected {length} for asset type {class_type_name}. \
         V: {version} P: {platform} N: {collection_name} Path: {file_path}"
    )
}

/// `MakeError_ReadException`: the exact diagnostic text with the appended exception.
pub fn read_exception_message(
    class_type_name: &str,
    version: &str,
    platform: &str,
    collection_name: &str,
    file_path: &str,
    exception: &str,
) -> String {
    format!(
        "Error during reading of asset type {class_type_name}. \
         V: {version} P: {platform} N: {collection_name} Path: {file_path}\n{exception}"
    )
}

/// `LogMonoBehaviorReadException`: the exact layout-mismatch error text (note the
/// source's `MonoBehavior` spelling). Behaviour and structure displays arrive as caller
/// facts; only the exception type name is embedded, not the full exception text.
pub fn monobehaviour_layout_mismatch_message(
    behaviour: &str,
    structure: &str,
    exception_type: &str,
) -> String {
    format!(
        "Unable to read {behaviour}, because script {structure} layout mismatched \
         binary content ({exception_type})."
    )
}

/// Byte offset of `m_OriginalWidth` (little-endian Int32) in the non-standard Chinese
/// texture tail (`GameAssetFactory.ReadExtraTextureFields`).
pub const CHINESE_TEXTURE_ORIGINAL_WIDTH_OFFSET: usize = 0;
/// Byte offset of `m_OriginalHeight` (little-endian Int32) in the tail.
pub const CHINESE_TEXTURE_ORIGINAL_HEIGHT_OFFSET: usize = 4;
/// Byte offset of `m_OriginalAssetGuid` (GUID release read, 16 bytes) in the tail.
pub const CHINESE_TEXTURE_ORIGINAL_GUID_OFFSET: usize = 8;
/// Byte size of the GUID release read: 8 + 16 fills the 24-byte tail exactly, so the
/// trailing `data[0..3]` lines in the source comments describe reserved slots the
/// reader deliberately does not consume.
pub const CHINESE_TEXTURE_ORIGINAL_GUID_SIZE: usize = 16;

/// `ReadExtraTextureFields` warning: the exact text logged after consuming the tail.
pub fn chinese_texture_warning_message(texture_name: &str) -> String {
    format!(
        "Texture {texture_name} had an extra 24 bytes, which were assumed to be \
         non-standard Chinese fields."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_names_follow_the_raw_data_hash_rule() {
        // The hash is the reflected CRC-32 of the raw bytes.
        assert_eq!(raw_data_hash(b""), crc32_ieee(b""));
        assert_eq!(raw_data_hash(b"abc"), 0x3524_41C2);
        // Unknown names are always synthetic with uppercase hex.
        assert_eq!(
            unknown_asset_name("Texture2D", 0x1234_ABCD),
            "UnknownTexture2D_1234ABCD"
        );
        // Unreadable names keep a stored non-empty name.
        assert_eq!(
            unreadable_asset_name("Mesh", Some("Keep"), 0x1234_ABCD),
            "Keep"
        );
        // Null or empty stored names fall back to the synthetic form.
        assert_eq!(
            unreadable_asset_name("Mesh", None, 0x1234_ABCD),
            "UnreadableMesh_1234ABCD"
        );
        assert_eq!(
            unreadable_asset_name("Mesh", Some(""), 0x1234_ABCD),
            "UnreadableMesh_1234ABCD"
        );
    }

    #[test]
    fn engine_collection_gate_matches_the_factory_tolerance_switch() {
        assert!(is_engine_collection_name(Some("unity default resources")));
        assert!(is_engine_collection_name(Some("unity_default_resources")));
        assert!(is_engine_collection_name(Some("unity builtin extra")));
        assert!(is_engine_collection_name(Some("unity_builtin_extra")));
        // Editor resources are engine resources but not tolerated collections here.
        assert!(!is_engine_collection_name(Some("unity editor resources")));
        assert!(!is_engine_collection_name(Some("level0")));
        assert!(!is_engine_collection_name(Some("Unity Default Resources")));
        assert!(!is_engine_collection_name(Some("")));
        assert!(!is_engine_collection_name(None));
        // The gate drives the tolerance branch end to end.
        let tolerated = NormalReadFacts {
            first_read_ok: false,
            is_engine_collection: is_engine_collection_name(Some("unity builtin extra")),
            is_shader: false,
            version_is_patch: false,
            retry_read_ok: false,
        };
        assert_eq!(
            decide_normal_object(&tolerated),
            (
                NormalObjectOutcome::UseFirstAsset,
                NormalObjectLog::WarningReadError
            )
        );
        let ordinary = NormalReadFacts {
            is_engine_collection: is_engine_collection_name(Some("level0")),
            ..tolerated
        };
        assert_eq!(
            decide_normal_object(&ordinary),
            (
                NormalObjectOutcome::UseUnreadable,
                NormalObjectLog::ErrorReadError
            )
        );
    }

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
    fn monobehaviour_structure_follows_the_source_branches() {
        let parsed = MonoBehaviourReadFacts {
            read_threw: false,
            has_type_tree: true,
            type_tree_parse_ok: true,
            structure_try_read_ok: true,
        };
        assert_eq!(
            decide_monobehaviour_structure(&parsed),
            (
                MonoBehaviourStructureOutcome::UseParsedStructure,
                MonoBehaviourReadLog::Silent
            )
        );
        // A failed TryRead clears the structure instead of assigning it.
        let unreadable_tree = MonoBehaviourReadFacts {
            structure_try_read_ok: false,
            ..parsed
        };
        assert_eq!(
            decide_monobehaviour_structure(&unreadable_tree),
            (
                MonoBehaviourStructureOutcome::ClearStructure,
                MonoBehaviourReadLog::Silent
            )
        );
        // An absent type tree and an unparseable one both fall to the lazy remainder.
        for facts in [
            MonoBehaviourReadFacts {
                has_type_tree: false,
                type_tree_parse_ok: true,
                structure_try_read_ok: true,
                ..parsed
            },
            MonoBehaviourReadFacts {
                has_type_tree: true,
                type_tree_parse_ok: false,
                structure_try_read_ok: true,
                ..parsed
            },
            MonoBehaviourReadFacts {
                has_type_tree: false,
                type_tree_parse_ok: false,
                structure_try_read_ok: false,
                ..parsed
            },
        ] {
            assert_eq!(
                decide_monobehaviour_structure(&facts),
                (
                    MonoBehaviourStructureOutcome::UseUnloadedRemainder,
                    MonoBehaviourReadLog::Silent
                )
            );
        }
        // A thrown read keeps the prior structure and logs the mismatch, winning over
        // every structure fact.
        let thrown = MonoBehaviourReadFacts {
            read_threw: true,
            has_type_tree: true,
            type_tree_parse_ok: true,
            structure_try_read_ok: true,
        };
        assert_eq!(
            decide_monobehaviour_structure(&thrown),
            (
                MonoBehaviourStructureOutcome::KeepExisting,
                MonoBehaviourReadLog::ErrorLayoutMismatch
            )
        );
    }

    #[test]
    fn factory_diagnostic_texts_match_the_source_formats() {
        assert_eq!(
            incorrect_byte_count_message(12, 48, "Texture2D", "2019.4.0f1", "Windows", "res", "a/b"),
            "Read 12 but expected 48 for asset type Texture2D. V: 2019.4.0f1 P: Windows N: res Path: a/b"
        );
        assert_eq!(
            read_exception_message(
                "Mesh",
                "2022.3.1f1",
                "Android",
                "level0",
                "c/d",
                "System.IO.EndOfStreamException: boom"
            ),
            "Error during reading of asset type Mesh. V: 2022.3.1f1 P: Android N: level0 Path: c/d\nSystem.IO.EndOfStreamException: boom"
        );
        // Note the source's `MonoBehavior` (single-u) spelling is in the method name only;
        // the message body has no such word.
        assert_eq!(
            monobehaviour_layout_mismatch_message("MonoBehaviour MB", "Script S", "InvalidCastException"),
            "Unable to read MonoBehaviour MB, because script Script S layout mismatched binary content (InvalidCastException)."
        );
        assert_eq!(
            chinese_texture_warning_message("Logo"),
            "Texture Logo had an extra 24 bytes, which were assumed to be non-standard Chinese fields."
        );
    }

    #[test]
    fn chinese_tail_layout_fills_the_24_byte_suffix() {
        assert_eq!(CHINESE_TEXTURE_ORIGINAL_WIDTH_OFFSET, 0);
        assert_eq!(CHINESE_TEXTURE_ORIGINAL_HEIGHT_OFFSET, 4);
        assert_eq!(CHINESE_TEXTURE_ORIGINAL_GUID_OFFSET, 8);
        // Two Int32 reads plus the 16-byte GUID release read consume the tail exactly.
        assert_eq!(
            CHINESE_TEXTURE_ORIGINAL_GUID_OFFSET + CHINESE_TEXTURE_ORIGINAL_GUID_SIZE,
            CHINESE_TEXTURE_EXTRA_BYTES
        );
        assert_eq!(CHINESE_TEXTURE_EXTRA_BYTES, 24);
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
