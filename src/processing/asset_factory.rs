//! Asset-creation dispatch ported from `AssetRipper.Import.AssetCreation.GameAssetFactory`.
//!
//! Covers the first audited slice of `SER-TT-ABSENT-001`: the `ReadAsset` top dispatch,
//! the `ReadNormalObject` fallback chain with its engine-collection gate, the
//! `TryReadNormalObject` tail rules with their control-flow composition, the `CreateAsset` three outcomes, the `CreateEngineAsset` name switch,
//! the `ReadMonoBehaviour` structure-source decision, the `MakeError_*` and
//! `LogMonoBehaviorReadException` diagnostic texts, the `RawDataObject`/`UnknownObject`/
//! `UnreadableObject` hash and synthetic-name rules, and the `ReadExtraTextureFields`
//! 24-byte tail layout, plus the structure-resolver selection, the player-settings
//! class predicate, the MonoBehaviour structure field-start rule, and the `ReadData`
//! stripped-TypeID fallback with its `ReadAsset` handoff, and the `TypeTreeObject`
//! read/convert plan. Byte reading,
//! node building, and assembly-backed MonoBehaviour structures stay outside: they need
//! generated objects and live assemblies. Version facts arrive precomputed because the
//! Rust tree has no parsed Unity-version model yet.
//!
//! Deliberately open: the generated `AssetFactory.CreateSerialized` class switch (emitted
//! by `AssetRipper.AssemblyDumper` `Pass940_MakeAssetFactory`, not snapshot logic), the
//! `TypeTreeNodeStruct`/`SerializableStructure` node building behind the MonoBehaviour
//! structure facts, the live-assembly `UnloadedStructure` materialization, the actual
//! `EndianSpanReader` byte consumption behind the tail-layout offsets, and the
//! `FromSerializedFile` stripped-version default (a zero version takes the configured
//! default, which needs a unified parsed Unity-version model; versions still travel
//! as caller facts and texts).
//!
//! Differential evidence: the `ENGINE` rows of `mono_utils_fixture.txt` (probe
//! `Temp/opencode/monoutilsprobe` resolving all 21 engine names at 2019.4.0f1, SDK
//! 10.0.401); the dispatch and fallback rules are source-read over caller facts.

use crate::processing::source_tpk::{SourceTpkDatabase, SourceTpkLookupError, SourceTpkTrees};
use crate::resources::blend_shape::crc32_ieee;

/// The class ID routed to the MonoBehaviour read path.
pub const MONO_BEHAVIOUR_CLASS_ID: i32 = 114;

/// The class ID of the player-settings asset (`TypeTreeObject.IsPlayerSettings`).
pub const PLAYER_SETTINGS_CLASS_ID: i32 = 129;

/// Whether a class ID identifies the player-settings asset. Downstream export
/// decisions key off this predicate (`ManagerAssetExporter`, `EditorFormatProcessor`):
/// player-settings `TypeTreeObject`s are handled as manager assets.
pub const fn is_player_settings_class(class_id: i32) -> bool {
    class_id == PLAYER_SETTINGS_CLASS_ID
}

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

/// `SerializedAssetCollection.ReadData` class fallback over caller facts: a stripped
/// (negative) object type ID reads as `MonoBehaviour` (114), because stripped files
/// keep no usable type mapping; every other type ID passes through unchanged.
pub const fn resolve_read_class_id(type_id: i32) -> i32 {
    if type_id < 0 {
        MONO_BEHAVIOUR_CLASS_ID
    } else {
        type_id
    }
}

/// The `ReadData` into `ReadAsset` handoff: each file object resolves its read class
/// through the stripped-TypeID fallback before the top dispatch. This is the
/// production caller behind `resolve_read_class_id`: collection loading never
/// dispatches a raw negative type ID, and the version gate still wins over the
/// fallback exactly like the source call order.
pub fn classify_read_data_path(version_before_3_5: bool, type_id: i32) -> AssetReadPath {
    classify_read_path(version_before_3_5, resolve_read_class_id(type_id))
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

/// What `TryReadNormalObject` returns for one asset.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TryReadOutcome {
    /// A created asset stands (clean, padded, Chinese-tailed, or half-read): the
    /// caller keeps it and reports `TryReadLog`.
    UseCreatedAsset,
    /// Creation missed entirely: an `UnknownObject` wraps the raw bytes silently.
    UseUnknownObject,
}

/// The log `TryReadNormalObject` emits alongside its outcome. Each error variant
/// pairs with its message builder below (`incorrect_byte_count_message`,
/// `read_exception_message`, `chinese_texture_warning_message`).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TryReadLog {
    /// Clean reads, padding, and creation misses stay silent.
    Silent,
    /// The Chinese tail logs its warning while the asset stands.
    WarningChineseTextureFields,
    /// A thrown read logs the read exception.
    ErrorReadException,
    /// A leftover suffix logs the incorrect-byte-count error.
    ErrorIncorrectByteCount,
}

/// Caller facts behind one `TryReadNormalObject` decision. Byte reading stays
/// caller-side (reader-side object.rs territory): this kernel only composes the
/// creation outcome, the throw flag, and the tail classification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TryReadFacts {
    /// The `CreateAsset` outcome for this asset.
    pub created: CreatedAssetKind,
    /// `asset.Read` threw; the half-read asset is kept with the exception error.
    pub read_threw: bool,
    /// The tail classification, meaningful only for a created asset that did not throw.
    pub tail: ObjectTailOutcome,
}

/// `TryReadNormalObject`: a creation miss wraps silently without reading; a thrown
/// read keeps the asset with the exception error; otherwise the tail decides (exact
/// and padding stay silent, the Chinese tail warns, leftovers error).
pub fn decide_try_read_object(facts: &TryReadFacts) -> (TryReadOutcome, TryReadLog) {
    if facts.created == CreatedAssetKind::Unknown {
        (TryReadOutcome::UseUnknownObject, TryReadLog::Silent)
    } else if facts.read_threw {
        (
            TryReadOutcome::UseCreatedAsset,
            TryReadLog::ErrorReadException,
        )
    } else {
        match facts.tail {
            ObjectTailOutcome::Exact | ObjectTailOutcome::ZeroPadding => {
                (TryReadOutcome::UseCreatedAsset, TryReadLog::Silent)
            }
            ObjectTailOutcome::ChineseTextureFields => (
                TryReadOutcome::UseCreatedAsset,
                TryReadLog::WarningChineseTextureFields,
            ),
            ObjectTailOutcome::IncorrectByteCount => (
                TryReadOutcome::UseCreatedAsset,
                TryReadLog::ErrorIncorrectByteCount,
            ),
        }
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

/// The base field ending the generated MonoBehaviour header in a TypeTree node list.
pub const MONO_BEHAVIOUR_NAME_FIELD: &str = "m_Name";
/// The editor-only base field following `m_Name` on newer generations.
pub const MONO_BEHAVIOUR_EDITOR_CLASS_ID_FIELD: &str = "m_EditorClassIdentifier";

/// `SerializableTreeType.FindStartingIndexForMonoBehaviour` over caller facts: a
/// MonoBehaviour structure starts after the later of `m_Name` and
/// `m_EditorClassIdentifier`, skipping the base fields the generated
/// `monoBehaviour.Read` already consumed (reading them again would misalign the
/// remainder and surface as a layout mismatch). Either marker may be absent, which
/// reads as index -1 like the source `IndexOf`; both absent starts at zero. The
/// caller supplies the ordered node field names, so no tree model crosses here.
pub fn monobehaviour_structure_start_index(field_names: &[&str]) -> usize {
    let position = |marker: &str| {
        field_names
            .iter()
            .position(|name| *name == marker)
            .map(|index| index as i64)
            .unwrap_or(-1)
    };
    let start =
        position(MONO_BEHAVIOUR_NAME_FIELD).max(position(MONO_BEHAVIOUR_EDITOR_CLASS_ID_FIELD)) + 1;
    usize::try_from(start).unwrap_or(0)
}

/// Which type resolver a freshly created asset reads its TypeTree structure with.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum StructureResolverKind {
    /// Engine `TypeTreeObject`s from Tpk roots: engine assets cannot have
    /// `[SerializeReference]` fields, so they never need (and never get) a resolver.
    Null,
    /// MonoBehaviour structures from a file TypeTree: resolved against the file's
    /// reference types.
    FromReferenceTypes,
}

/// `GameAssetFactory` resolver selection over caller facts: the `CreateAsset` Tpk
/// fallback always reads with `ITypeResolver.Null`, while a MonoBehaviour TypeTree
/// structure reads with `TypeTreeResolver.Create(referenceTypes)`. The fact is true
/// exactly for Tpk-backed engine `TypeTreeObject`s; generated reads take no resolver
/// at all and never reach this gate. Resolver construction needs live assemblies;
/// only the selection is pinned here.
pub const fn select_structure_resolver(is_engine_typetree_object: bool) -> StructureResolverKind {
    if is_engine_typetree_object {
        StructureResolverKind::Null
    } else {
        StructureResolverKind::FromReferenceTypes
    }
}

/// Which field tree a `TypeTreeObject` read consumes and where it converts
/// (`TypeTreeObject.ReadRelease`/`ReadEditor` without the byte reads, which need
/// live structures).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum TypeTreeReadPlan {
    /// Single-tree objects read their only fields in either mode; nothing converts.
    SingleFields,
    /// Double-tree release reads consume release fields, then convert into editor fields.
    ReleaseThenConvertToEditor,
    /// Double-tree editor reads consume editor fields, then convert into release fields.
    EditorThenConvertToRelease,
}

/// `TypeTreeObject` read routing over caller facts: single-tree objects always read
/// their shared fields, while double-tree objects read the mode-matching tree and
/// convert it into the other one (same-collection `PPtrConverter`, which stays
/// caller-side with the value copies). Reversing the conversion direction would
/// corrupt the target tree, so the plan pins it alongside the read side.
pub const fn select_typetree_read_plan(
    is_double_tree_object: bool,
    release_mode: bool,
) -> TypeTreeReadPlan {
    if !is_double_tree_object {
        TypeTreeReadPlan::SingleFields
    } else if release_mode {
        TypeTreeReadPlan::ReleaseThenConvertToEditor
    } else {
        TypeTreeReadPlan::EditorThenConvertToRelease
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

// ---------------------------------------------------------------------------
// GameAssetFactory engine: the dispatch and fallback chains over real
// SerializedFile bytes.
//
// The kernels above decide *what* the source does; this engine drives them from a
// parsed `SerializedFile` and materializes the same fallback objects the source
// creates: a generated class identity, a TPK-backed `TypeTreeObject`, an
// `UnknownObject`, or an `UnreadableObject`, with the raw payload always preserved.
//
// Class identity and class layouts arrive through `ClassLayoutSource`, the Rust
// equivalent of the generated `AssetFactory.CreateSerialized` table plus
// `TypeTreeNodeStruct.TryMakeFromTpk`: AssetRipper's generated classes are themselves
// generated from the TPK, so the TPK trees are the class layouts.
// ---------------------------------------------------------------------------

use std::borrow::Cow;
use std::fmt::{self, Display, Formatter};
use std::sync::{Arc, OnceLock};

use crate::io::binary::Endian;
use crate::serialized_file::file::SerializedFile;
use crate::serialized_file::metadata::{
    unity_version_aligns_arrays, ObjectInfo, TypeTree, TypeTreeNode,
};
use crate::serialized_file::object::{
    classify_type_tree_usability, decode_typetree_payload, raw_data_class_name,
    version_is_unsupported, ManagedPayloadDecodeContext, ObjectDecodeError, ObjectDecodeLimits,
    ParsedObjectPayload, SerializedValue, TypeTreeUsability,
};

/// The release-type letter of a Unity version (`UnityVersionType`).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FactoryVersionType {
    Alpha,
    Beta,
    China,
    Release,
    Patch,
    Experimental,
    Unknown,
}

/// Unity version facts the factory dispatch reads from the SerializedFile signature.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FactoryUnityVersion {
    pub major: u32,
    pub minor: u32,
    pub build: u16,
    pub version_type: FactoryVersionType,
    pub type_number: u8,
    pub text: String,
}

impl FactoryUnityVersion {
    /// Parse the `{major}.{minor}.{build}{type}{increment}` Unity version grammar.
    ///
    /// A stripped or malformed signature returns `None`; callers then use the
    /// configured default version, which is `0.0.0` and therefore below the 3.5 gate.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let mut parts = text.split('.');
        let major = parts.next()?.parse::<u32>().ok()?;
        let minor = parts.next()?.parse::<u32>().ok()?;
        let rest = parts.next()?;
        let build_digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if build_digits.is_empty() {
            return None;
        }
        let build = build_digits.parse::<u16>().ok()?;
        let suffix = rest.get(build_digits.len()..)?;
        let (version_type, type_number_text) =
            if let Some(china_suffix) = suffix.strip_prefix("f1c") {
                (FactoryVersionType::China, china_suffix)
            } else {
                let mut chars = suffix.chars();
                let version_type = match chars.next()? {
                    'a' => FactoryVersionType::Alpha,
                    'b' => FactoryVersionType::Beta,
                    'c' => FactoryVersionType::China,
                    'f' => FactoryVersionType::Release,
                    'p' => FactoryVersionType::Patch,
                    'x' => FactoryVersionType::Experimental,
                    _ => FactoryVersionType::Unknown,
                };
                let type_number_start = suffix.len() - chars.as_str().len();
                (version_type, suffix.get(type_number_start..)?)
            };
        let type_number_digits: String = type_number_text
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        let type_number = if type_number_digits.is_empty() {
            0
        } else {
            type_number_digits.parse::<u8>().ok()?
        };
        Some(Self {
            major,
            minor,
            build,
            version_type,
            type_number,
            text: text.to_owned(),
        })
    }

    /// The default configured version for a stripped signature (`UnityVersion(0,0,0)`).
    pub fn stripped() -> Self {
        Self {
            major: 0,
            minor: 0,
            build: 0,
            version_type: FactoryVersionType::Release,
            type_number: 0,
            text: String::new(),
        }
    }

    /// `SerializedAssetCollection.Version.LessThan(3, 5)`.
    pub const fn is_unsupported(&self) -> bool {
        version_is_unsupported(self.major, self.minor)
    }

    /// `UnityVersion.Type == UnityVersionType.Patch`.
    pub const fn is_patch(&self) -> bool {
        matches!(self.version_type, FactoryVersionType::Patch)
    }

    /// `new UnityVersion(major, minor, unchecked((ushort)(build + 1)))`: the release
    /// constructor keeps the release type and wraps the build counter.
    pub fn with_bumped_build(&self) -> Self {
        let mut bumped = self.clone();
        bumped.build = self.build.wrapping_add(1);
        bumped.version_type = FactoryVersionType::Release;
        bumped.type_number = 0;
        bumped.text = format!("{}.{}.{}f0", bumped.major, bumped.minor, bumped.build);
        bumped
    }

    /// The packed ordering used by the frozen source's `UnityVersion` comparison.
    /// `UnityVersion.GetBits()` stores components from most to least significant as
    /// major, minor, build, type, and type number.
    pub fn source_order_bits(&self) -> Option<u64> {
        let type_code: u64 = match self.version_type {
            FactoryVersionType::Alpha => 0,
            FactoryVersionType::Beta => 1,
            FactoryVersionType::China => 2,
            FactoryVersionType::Release => 3,
            FactoryVersionType::Patch => 4,
            FactoryVersionType::Experimental => 5,
            FactoryVersionType::Unknown => return None,
        };
        if self.major > u16::MAX as u32 || self.minor > u16::MAX as u32 {
            return None;
        }
        Some(
            (u64::from(self.major) << 48)
                | (u64::from(self.minor) << 32)
                | (u64::from(self.build) << 16)
                | (type_code << 8)
                | u64::from(self.type_number),
        )
    }
}

/// Collection/file facts the source reads from the owning asset info.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FactoryFileFacts<'a> {
    /// `FileBase.NameFixed` (the owning collection name).
    pub collection_name: &'a str,
    /// `FileBase.FilePath`.
    pub file_path: &'a str,
    /// The platform display name (`BuildTarget.ToString()`), used by diagnostics.
    pub platform: &'a str,
}

/// Class identity and class layout behind `CreateAsset`.
pub trait ClassLayoutSource {
    /// `AssetFactory.CreateSerialized(info, version)`: the generated class name, or
    /// `None` when the class id has no generated class at this version.
    fn generated_class_name(&self, class_id: i32, version: &FactoryUnityVersion) -> Option<&str>;

    /// `TypeTreeNodeStruct.TryMakeFromTpk`: the class's release and editor trees.
    fn tpk_trees(
        &self,
        class_id: i32,
        version: &FactoryUnityVersion,
    ) -> Result<Option<SourceTpkTrees>, SourceTpkLookupError>;
}

/// Which `CreateAsset` branch produced an asset.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FactoryAssetKind {
    /// `AssetFactory.CreateSerialized` produced a generated class.
    Generated(String),
    /// The TPK fallback produced a `TypeTreeObject`.
    TypeTreeObject,
    /// No generated class and no TPK entry: an `UnknownObject`.
    Unknown,
    /// A pre-3.5/stripped version, or a normal read the caller refused: an `UnreadableObject`.
    Unreadable,
}

impl FactoryAssetKind {
    /// Whether the branch is one of the two recoverable engine-object kinds.
    pub const fn is_recoverable(&self) -> bool {
        matches!(self, Self::Generated(_) | Self::TypeTreeObject)
    }
}

/// The non-standard Chinese texture tail (`ReadExtraTextureFields`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChineseTextureFields {
    pub original_width: i32,
    pub original_height: i32,
    pub original_asset_guid: [u32; 4],
}

/// Severity of the log line a factory branch emits.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FactoryLogLevel {
    Silent,
    Info,
    Warning,
    Error,
}

/// Where a MonoBehaviour structure came from, with the parsed fields when available.
#[derive(Clone, Debug, PartialEq)]
pub struct FactoryStructure {
    pub outcome: MonoBehaviourStructureOutcome,
    /// The parsed script fields for `UseParsedStructure`.
    pub fields: Option<SerializedValue>,
    /// The unread remainder backing `UseUnloadedRemainder`.
    pub unloaded_remainder: Vec<u8>,
}

/// One materialized asset from the factory dispatch.
#[derive(Clone, Debug, PartialEq)]
pub struct FactoryAsset {
    pub object_index: usize,
    pub path_id: i64,
    pub class_id: i32,
    pub kind: FactoryAssetKind,
    /// `RawDataObject.ClassName`: the class-id type name or its decimal form.
    pub class_name: Cow<'static, str>,
    /// `INamed.Name`: the stored name, or the synthetic Unknown/Unreadable form.
    pub name: String,
    /// The preserved raw payload (`RawDataObject.RawData`).
    pub raw: Vec<u8>,
    /// The decoded fields when a read consumed any of the payload.
    pub payload: Option<ParsedObjectPayload>,
    /// The tail classification for a completed normal read.
    pub tail: Option<ObjectTailOutcome>,
    /// The Chinese texture tail when it was consumed.
    pub chinese_texture_fields: Option<ChineseTextureFields>,
    /// The MonoBehaviour structure decision and value.
    pub structure: Option<FactoryStructure>,
    pub log_level: FactoryLogLevel,
    pub log_message: Option<String>,
}

/// Structural engine failures (not classification outcomes).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FactoryEngineError {
    ObjectIndexOutOfBounds {
        object_index: usize,
        object_count: usize,
    },
    InvalidObjectPayloadRange {
        object_index: usize,
    },
    ObjectTypeUnresolved {
        object_index: usize,
    },
}

impl Display for FactoryEngineError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ObjectIndexOutOfBounds {
                object_index,
                object_count,
            } => write!(
                f,
                "object index {object_index} is outside the {object_count}-object table"
            ),
            Self::InvalidObjectPayloadRange { object_index } => write!(
                f,
                "object {object_index} payload range overflows or leaves the input file"
            ),
            Self::ObjectTypeUnresolved { object_index } => {
                write!(f, "object {object_index} has no resolved serialized type")
            }
        }
    }
}

impl std::error::Error for FactoryEngineError {}

/// The read class id the source collection uses: the file's resolved class id with
/// the stripped (negative) fallback to MonoBehaviour.
pub fn object_read_class_id(object: &ObjectInfo) -> i32 {
    let class_id = object.class_id.map(i32::from).unwrap_or(object.raw_type_id);
    resolve_read_class_id(class_id)
}

/// `GameAssetFactory.ReadAsset`: dispatch one file object through the factory.
pub fn materialize_object(
    file: &SerializedFile<'_>,
    facts: &FactoryFileFacts<'_>,
    source: &dyn ClassLayoutSource,
    object_index: usize,
    limits: &ObjectDecodeLimits,
) -> Result<FactoryAsset, FactoryEngineError> {
    let object = file.metadata.objects.get(object_index).ok_or(
        FactoryEngineError::ObjectIndexOutOfBounds {
            object_index,
            object_count: file.metadata.objects.len(),
        },
    )?;
    let raw = object_payload_bytes(file, object_index)?;
    let version = file
        .metadata
        .prefix
        .unity_version_text
        .as_deref()
        .and_then(FactoryUnityVersion::parse)
        .unwrap_or_else(FactoryUnityVersion::stripped);
    let class_id = object_read_class_id(object);
    let class_name = raw_data_class_name(class_id);

    let mut asset = FactoryAsset {
        object_index,
        path_id: object.path_id,
        class_id,
        kind: FactoryAssetKind::Unreadable,
        class_name,
        name: String::new(),
        raw,
        payload: None,
        tail: None,
        chinese_texture_fields: None,
        structure: None,
        log_level: FactoryLogLevel::Silent,
        log_message: None,
    };

    if version.is_unsupported() {
        // `ReadAsset` returns an UnreadableObject without reading or logging.
        asset.kind = FactoryAssetKind::Unreadable;
        asset.name = unreadable_asset_name(
            asset.class_name.as_ref(),
            named_field(&None),
            raw_data_hash(&asset.raw),
        );
        return Ok(asset);
    }

    if class_id == MONO_BEHAVIOUR_CLASS_ID {
        read_monobehaviour(
            file,
            facts,
            source,
            object_index,
            limits,
            &version,
            &mut asset,
        );
        return Ok(asset);
    }

    read_normal_object(
        file,
        facts,
        source,
        object_index,
        limits,
        &version,
        &mut asset,
    );
    Ok(asset)
}

fn object_payload_bytes(
    file: &SerializedFile<'_>,
    object_index: usize,
) -> Result<Vec<u8>, FactoryEngineError> {
    let object = &file.metadata.objects[object_index];
    let range = object
        .data_range(&file.header)
        .ok_or(FactoryEngineError::InvalidObjectPayloadRange { object_index })?;
    let start = usize::try_from(range.start)
        .map_err(|_| FactoryEngineError::InvalidObjectPayloadRange { object_index })?;
    let end = usize::try_from(range.end)
        .map_err(|_| FactoryEngineError::InvalidObjectPayloadRange { object_index })?;
    file.input()
        .get(start..end)
        .map(<[u8]>::to_vec)
        .ok_or(FactoryEngineError::InvalidObjectPayloadRange { object_index })
}

fn serialized_type_tree<'a>(
    file: &'a SerializedFile<'_>,
    object_index: usize,
) -> Option<&'a TypeTree> {
    let object = file.metadata.objects.get(object_index)?;
    let type_index = object.resolved_type_index?;
    file.metadata.types.get(type_index)?.type_tree.as_ref()
}

/// The `INamed` name behind a decoded payload's `m_Name` field.
fn named_field(payload: &Option<ParsedObjectPayload>) -> Option<&str> {
    payload
        .as_ref()
        .and_then(|payload| named_field_of(&payload.root))
}

fn named_field_of(root: &SerializedValue) -> Option<&str> {
    let SerializedValue::Struct(_) = root else {
        return None;
    };
    let name = root.unique_field_in_base_chain("m_Name").ok().flatten()?;
    match name {
        SerializedValue::StringBytes(bytes) => std::str::from_utf8(bytes).ok(),
        _ => None,
    }
}

fn named_field_owned(payload: &Option<ParsedObjectPayload>) -> Option<String> {
    let bytes = payload.as_ref().and_then(|payload| {
        let SerializedValue::Struct(_) = payload.root else {
            return None;
        };
        let name = payload
            .root
            .unique_field_in_base_chain("m_Name")
            .ok()
            .flatten()?;
        match name {
            SerializedValue::StringBytes(bytes) => Some(bytes.clone()),
            _ => None,
        }
    })?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Which asset `CreateAsset` produces.
enum CreatedAsset {
    Generated(String),
    TypeTreeObject,
    Unknown,
}

fn create_asset(
    source: &dyn ClassLayoutSource,
    class_id: i32,
    version: &FactoryUnityVersion,
) -> Result<CreatedAsset, SourceTpkLookupError> {
    if let Some(name) = source.generated_class_name(class_id, version) {
        Ok(CreatedAsset::Generated(name.to_owned()))
    } else if source.tpk_trees(class_id, version)?.is_some() {
        Ok(CreatedAsset::TypeTreeObject)
    } else {
        Ok(CreatedAsset::Unknown)
    }
}

/// The layout a created asset reads its fields with: TPK class layout first (the
/// generated classes are generated from it), then the file's own TypeTree.
fn class_layout(
    file: &SerializedFile<'_>,
    source: &dyn ClassLayoutSource,
    class_id: i32,
    version: &FactoryUnityVersion,
    object_index: usize,
    is_release: bool,
) -> Result<Option<TypeTree>, SourceTpkLookupError> {
    if let Some((release, editor)) = source.tpk_trees(class_id, version)? {
        let selected = if is_release { release } else { editor };
        return Ok(Some(selected.as_ref().clone()));
    }
    let Some(tree) = serialized_type_tree(file, object_index) else {
        return Ok(None);
    };
    Ok(match classify_type_tree_usability(Some(tree)) {
        TypeTreeUsability::Present => Some(tree.clone()),
        TypeTreeUsability::Empty | TypeTreeUsability::Missing => None,
    })
}

/// A partial or complete normal-read attempt.
struct NormalAttempt {
    kind: FactoryAssetKind,
    payload: Option<ParsedObjectPayload>,
    tail: Option<ObjectTailOutcome>,
    chinese: Option<ChineseTextureFields>,
    log_level: FactoryLogLevel,
    log_message: Option<String>,
}

fn read_normal_object(
    file: &SerializedFile<'_>,
    facts: &FactoryFileFacts<'_>,
    source: &dyn ClassLayoutSource,
    object_index: usize,
    limits: &ObjectDecodeLimits,
    version: &FactoryUnityVersion,
    asset: &mut FactoryAsset,
) {
    match try_read_normal_object(file, facts, source, object_index, limits, version) {
        Ok(attempt) => apply_attempt(asset, attempt),
        Err(failed) => {
            if is_engine_collection_name(Some(facts.collection_name)) {
                // Engine collections tolerate failed reads: shaders log info, others warn.
                let (log_level, log_message) = if asset.class_id == SHADER_CLASS_ID {
                    (FactoryLogLevel::Info, None)
                } else {
                    (FactoryLogLevel::Warning, failed.log_message.clone())
                };
                apply_attempt(asset, *failed);
                asset.log_level = log_level;
                asset.log_message = log_message;
            } else if version.is_patch() {
                let retry_version = version.with_bumped_build();
                match try_read_normal_object(
                    file,
                    facts,
                    source,
                    object_index,
                    limits,
                    &retry_version,
                ) {
                    Ok(attempt) => apply_attempt(asset, attempt),
                    Err(failed_retry) => unreadable_from_failed(asset, *failed_retry),
                }
            } else {
                unreadable_from_failed(asset, *failed);
            }
        }
    }
}

/// The class id of `Shader`, whose engine-collection failure logs at info level.
const SHADER_CLASS_ID: i32 = 48;

fn unreadable_from_failed(asset: &mut FactoryAsset, failed: NormalAttempt) {
    let stored_name = named_field_owned(&failed.payload);
    let message = failed.log_message.clone();
    apply_attempt(asset, failed);
    asset.kind = FactoryAssetKind::Unreadable;
    asset.name = unreadable_asset_name(
        asset.class_name.as_ref(),
        stored_name.as_deref(),
        raw_data_hash(&asset.raw),
    );
    asset.log_level = FactoryLogLevel::Error;
    asset.log_message = message;
}

fn apply_attempt(asset: &mut FactoryAsset, attempt: NormalAttempt) {
    asset.kind = attempt.kind;
    asset.payload = attempt.payload;
    asset.tail = attempt.tail;
    asset.chinese_texture_fields = attempt.chinese;
    asset.log_level = attempt.log_level;
    asset.log_message = attempt.log_message;
    match &asset.kind {
        FactoryAssetKind::Unknown => {
            asset.name = unknown_asset_name(asset.class_name.as_ref(), raw_data_hash(&asset.raw));
        }
        FactoryAssetKind::Generated(_) | FactoryAssetKind::TypeTreeObject => {
            // Only named classes (`INamed`) expose a name; TPK TypeTreeObjects do not.
            asset.name = match asset.kind {
                FactoryAssetKind::Generated(_) => {
                    named_field_owned(&asset.payload).unwrap_or_default()
                }
                _ => String::new(),
            };
        }
        FactoryAssetKind::Unreadable => {}
    }
}

fn try_read_normal_object(
    file: &SerializedFile<'_>,
    facts: &FactoryFileFacts<'_>,
    source: &dyn ClassLayoutSource,
    object_index: usize,
    limits: &ObjectDecodeLimits,
    version: &FactoryUnityVersion,
) -> Result<NormalAttempt, Box<NormalAttempt>> {
    let class_id = object_read_class_id(&file.metadata.objects[object_index]);
    let created_asset = match create_asset(source, class_id, version) {
        Ok(created_asset) => created_asset,
        Err(error) => {
            return Err(Box::new(NormalAttempt {
                kind: FactoryAssetKind::Unreadable,
                payload: None,
                tail: None,
                chinese: None,
                log_level: FactoryLogLevel::Error,
                log_message: Some(error.to_string()),
            }));
        }
    };
    let kind = match created_asset {
        CreatedAsset::Unknown => {
            // A creation miss wraps the raw bytes silently, without reading.
            return Ok(NormalAttempt {
                kind: FactoryAssetKind::Unknown,
                payload: None,
                tail: None,
                chinese: None,
                log_level: FactoryLogLevel::Silent,
                log_message: None,
            });
        }
        CreatedAsset::Generated(name) => FactoryAssetKind::Generated(name),
        CreatedAsset::TypeTreeObject => FactoryAssetKind::TypeTreeObject,
    };
    let created = NormalAttempt {
        kind,
        payload: None,
        tail: None,
        chinese: None,
        log_level: FactoryLogLevel::Silent,
        log_message: None,
    };
    let payload_bytes = object_payload_bytes(file, object_index)
        .map_err(|_| Box::new(created.clone_for_failure()))?;
    let read_flags = file.object_read_flags(facts.file_path, facts.collection_name);
    let layout = match class_layout(
        file,
        source,
        class_id,
        version,
        object_index,
        read_flags.is_release(),
    ) {
        Ok(Some(layout)) => layout,
        Ok(None) => {
            return Err(Box::new(NormalAttempt {
                log_level: FactoryLogLevel::Error,
                log_message: None,
                ..created
            }));
        }
        Err(error) => {
            return Err(Box::new(NormalAttempt {
                log_level: FactoryLogLevel::Error,
                log_message: Some(error.to_string()),
                ..created
            }));
        }
    };
    let arrays_align =
        unity_version_aligns_arrays(file.metadata.prefix.unity_version_text.as_deref());
    let decoded = decode_with_tree(
        &layout,
        &payload_bytes,
        file.metadata.prefix.body_endian,
        arrays_align,
        &file.metadata.reference_types,
        object_index,
        file.metadata.objects[object_index].path_id,
        limits,
    );
    match decoded {
        Ok(payload) => {
            let tail =
                classify_object_tail(&payload.trailing_bytes, class_id == TEXTURE2D_CLASS_ID);
            match tail {
                ObjectTailOutcome::Exact | ObjectTailOutcome::ZeroPadding => Ok(NormalAttempt {
                    payload: Some(payload),
                    tail: Some(tail),
                    ..created
                }),
                ObjectTailOutcome::ChineseTextureFields => {
                    let name = named_field_of_payload(&created.payload).unwrap_or_default();
                    let chinese = parse_chinese_texture_fields(&payload.trailing_bytes);
                    Ok(NormalAttempt {
                        payload: Some(payload),
                        tail: Some(tail),
                        chinese,
                        log_level: FactoryLogLevel::Warning,
                        log_message: Some(chinese_texture_warning_message(&name)),
                        ..created
                    })
                }
                ObjectTailOutcome::IncorrectByteCount => {
                    let message = incorrect_byte_count_message(
                        i32::try_from(payload.consumed_bytes).unwrap_or(i32::MAX),
                        i32::try_from(payload_bytes.len()).unwrap_or(i32::MAX),
                        &raw_data_class_name(class_id),
                        version.text_or_default(),
                        facts.platform,
                        facts.collection_name,
                        facts.file_path,
                    );
                    Err(Box::new(NormalAttempt {
                        payload: Some(payload),
                        tail: Some(tail),
                        log_level: FactoryLogLevel::Error,
                        log_message: Some(message),
                        ..created
                    }))
                }
            }
        }
        Err(error) => Err(Box::new(NormalAttempt {
            log_level: FactoryLogLevel::Error,
            log_message: Some(read_exception_message(
                &raw_data_class_name(class_id),
                version.text_or_default(),
                facts.platform,
                facts.collection_name,
                facts.file_path,
                &error.to_string(),
            )),
            ..created
        })),
    }
}

impl NormalAttempt {
    fn clone_for_failure(&self) -> Self {
        Self {
            kind: self.kind.clone(),
            payload: None,
            tail: None,
            chinese: None,
            log_level: FactoryLogLevel::Error,
            log_message: None,
        }
    }
}

/// The class id of `Texture2D`, which may carry the 24-byte Chinese tail.
const TEXTURE2D_CLASS_ID: i32 = 28;

fn named_field_of_payload(payload: &Option<ParsedObjectPayload>) -> Option<String> {
    named_field_owned(payload)
}

fn parse_chinese_texture_fields(trailing: &[u8]) -> Option<ChineseTextureFields> {
    if trailing.len() != CHINESE_TEXTURE_EXTRA_BYTES {
        return None;
    }
    let word = |offset: usize| -> i32 {
        i32::from_le_bytes([
            trailing[offset],
            trailing[offset + 1],
            trailing[offset + 2],
            trailing[offset + 3],
        ])
    };
    let mut guid = [0u32; 4];
    for (index, slot) in guid.iter_mut().enumerate() {
        let offset = CHINESE_TEXTURE_ORIGINAL_GUID_OFFSET + index * 4;
        *slot = u32::from_le_bytes([
            trailing[offset],
            trailing[offset + 1],
            trailing[offset + 2],
            trailing[offset + 3],
        ]);
    }
    Some(ChineseTextureFields {
        original_width: word(CHINESE_TEXTURE_ORIGINAL_WIDTH_OFFSET),
        original_height: word(CHINESE_TEXTURE_ORIGINAL_HEIGHT_OFFSET),
        original_asset_guid: guid,
    })
}

/// Decode a payload against a caller-supplied layout (TPK class tree or file TypeTree).
#[allow(clippy::too_many_arguments)]
fn decode_with_tree(
    tree: &TypeTree,
    payload: &[u8],
    endian: Endian,
    arrays_align: bool,
    reference_types: &[crate::serialized_file::metadata::SerializedTypeReference],
    object_index: usize,
    path_id: i64,
    limits: &ObjectDecodeLimits,
) -> Result<ParsedObjectPayload, ObjectDecodeError> {
    let context = ManagedPayloadDecodeContext {
        entry_index: object_index,
        stream_offset: 0,
        managed_registry_depth: 0,
    };
    let (root, consumed_bytes, allocated_bytes) = decode_typetree_payload(
        tree,
        payload,
        endian,
        arrays_align,
        reference_types,
        context,
        limits,
    )?;
    let trailing_bytes = payload.get(consumed_bytes..).unwrap_or_default().to_vec();
    Ok(ParsedObjectPayload {
        object_index,
        path_id,
        type_name: tree
            .nodes
            .first()
            .and_then(|node| node.type_name.clone())
            .unwrap_or_default(),
        root,
        consumed_bytes,
        trailing_bytes,
        allocated_bytes,
    })
}

/// `ReadMonoBehaviour`: the generated header, then the structure source decision.
#[allow(clippy::too_many_arguments)]
fn read_monobehaviour(
    file: &SerializedFile<'_>,
    facts: &FactoryFileFacts<'_>,
    source: &dyn ClassLayoutSource,
    object_index: usize,
    limits: &ObjectDecodeLimits,
    version: &FactoryUnityVersion,
    asset: &mut FactoryAsset,
) {
    let payload_bytes = match object_payload_bytes(file, object_index) {
        Ok(bytes) => bytes,
        Err(_) => {
            asset.kind = FactoryAssetKind::Unreadable;
            asset.log_level = FactoryLogLevel::Error;
            asset.log_message = Some(read_exception_message(
                asset.class_name.as_ref(),
                version.text_or_default(),
                facts.platform,
                facts.collection_name,
                facts.file_path,
                "InvalidObjectPayloadRange",
            ));
            return;
        }
    };
    asset.kind = FactoryAssetKind::Generated(
        source
            .generated_class_name(MONO_BEHAVIOUR_CLASS_ID, version)
            .unwrap_or("MonoBehaviour")
            .to_owned(),
    );
    let arrays_align =
        unity_version_aligns_arrays(file.metadata.prefix.unity_version_text.as_deref());
    let read_flags = file.object_read_flags(facts.file_path, facts.collection_name);
    let header = class_layout(
        file,
        source,
        MONO_BEHAVIOUR_CLASS_ID,
        version,
        object_index,
        read_flags.is_release(),
    )
    .map_err(|error| {
        ObjectDecodeError::source_type_tree_lookup(
            object_index,
            MONO_BEHAVIOUR_CLASS_ID,
            version.text.clone(),
            error.to_string(),
        )
    })
    .and_then(|layout| {
        layout
            .map(|tree| {
                decode_with_tree(
                    &tree,
                    &payload_bytes,
                    file.metadata.prefix.body_endian,
                    arrays_align,
                    &file.metadata.reference_types,
                    object_index,
                    file.metadata.objects[object_index].path_id,
                    limits,
                )
            })
            .transpose()
    });

    let (header_ok, header_payload) = match header {
        Ok(Some(payload)) => (true, Some(payload)),
        Ok(None) => (false, None),
        Err(_) => (false, None),
    };
    let consumed = header_payload
        .as_ref()
        .map(|payload| payload.consumed_bytes)
        .unwrap_or(0);
    let remainder = payload_bytes.get(consumed..).unwrap_or_default().to_vec();

    let file_tree = serialized_type_tree(file, object_index);
    let has_type_tree = matches!(
        classify_type_tree_usability(file_tree),
        TypeTreeUsability::Present
    );
    let start_index = file_tree
        .map(|tree| {
            let names: Vec<&str> = tree
                .nodes
                .iter()
                .skip(1)
                .filter(|node| node.level == 1)
                .map(|node| node.name.as_deref().unwrap_or(""))
                .collect();
            monobehaviour_structure_start_index(&names)
        })
        .unwrap_or(0);
    let structure_tree = file_tree.and_then(|tree| structure_subtree(tree, start_index));
    let structure_read = structure_tree.as_ref().map(|tree| {
        decode_with_tree(
            tree,
            &remainder,
            file.metadata.prefix.body_endian,
            arrays_align,
            &file.metadata.reference_types,
            object_index,
            file.metadata.objects[object_index].path_id,
            limits,
        )
    });
    let structure_try_read_ok = matches!(
        structure_read.as_ref(),
        Some(Ok(payload)) if payload.consumed_bytes == remainder.len()
    );
    let structure_facts = MonoBehaviourReadFacts {
        read_threw: !header_ok,
        has_type_tree,
        type_tree_parse_ok: structure_tree.is_some(),
        structure_try_read_ok,
    };
    let (outcome, log) = decide_monobehaviour_structure(&structure_facts);
    let fields = match (&outcome, structure_read) {
        (MonoBehaviourStructureOutcome::UseParsedStructure, Some(Ok(payload))) => {
            Some(payload.root)
        }
        _ => None,
    };
    asset.payload = header_payload;
    asset.name = named_field_owned(&asset.payload).unwrap_or_default();
    asset.structure = Some(FactoryStructure {
        outcome,
        fields,
        unloaded_remainder: match outcome {
            MonoBehaviourStructureOutcome::UseUnloadedRemainder => remainder,
            _ => Vec::new(),
        },
    });
    match log {
        MonoBehaviourReadLog::Silent => {
            asset.log_level = FactoryLogLevel::Silent;
            asset.log_message = None;
        }
        MonoBehaviourReadLog::ErrorLayoutMismatch => {
            asset.log_level = FactoryLogLevel::Error;
            asset.log_message = Some(monobehaviour_layout_mismatch_message(
                asset.kind.generated_name().unwrap_or("MonoBehaviour"),
                "",
                "ArgumentOutOfRangeException",
            ));
        }
    }
}

/// A TypeTree rooted at `start_index`, with levels rebased so the sub-root is level 0.
/// This is the field list `SerializableTreeType.FromRootNode(root, true)` builds for a
/// MonoBehaviour structure: the root plus the fields after the base MonoBehaviour fields.
fn structure_subtree(tree: &TypeTree, start_index: usize) -> Option<TypeTree> {
    let root = tree.nodes.first()?;
    let mut child_indices = Vec::new();
    for (index, node) in tree.nodes.iter().enumerate().skip(1) {
        if node.level == 1 {
            child_indices.push(index);
        }
    }
    let first = *child_indices.get(start_index)?;
    let base_level = tree.nodes[first].level;
    let mut nodes = Vec::new();
    nodes.push(root.clone());
    for node in tree.nodes.iter().skip(first) {
        let mut node = node.clone();
        node.level = node.level.checked_sub(base_level)?.checked_add(1)?;
        nodes.push(node);
    }
    nodes[0].level = 0;
    // The synthesized nodes use direct names, so no string buffer is required.
    Some(TypeTree {
        nodes,
        string_buffer: Vec::new(),
    })
}

impl FactoryAssetKind {
    fn generated_name(&self) -> Option<&str> {
        match self {
            Self::Generated(name) => Some(name),
            _ => None,
        }
    }
}

trait VersionText {
    fn text_or_default(&self) -> &str;
}

impl VersionText for FactoryUnityVersion {
    fn text_or_default(&self) -> &str {
        if self.text.is_empty() {
            "0.0.0"
        } else {
            &self.text
        }
    }
}

/// The committed reference probe output: the `GEN` generated-class identities, the
/// `TPK`/`NODE` real TypeTrees, and the authored case files.
pub const PROBE_FIXTURE: &str = include_str!("asset_factory_fixture.txt");

/// Class identity and class layouts pinned by the committed reference probe output
/// (`tools/probes/ser-factory`, SDK 10.0.401). The `GEN` rows reproduce
/// `AssetFactory.CreateSerialized` and the `TPK` rows reproduce
/// `TypeTreeNodeStruct.TryMakeFromTpk`, so this is the data layer of `CreateAsset`.
///
/// The tables cover the classes the probe exercised; classes outside them fall through
/// to `Unknown`, exactly like a generated table that does not know the class.
#[derive(Clone, Debug, Default)]
pub struct ReferenceClassLayouts {
    generated: std::collections::BTreeMap<i32, String>,
    tpk: std::collections::BTreeMap<(i32, String), (Arc<TypeTree>, Arc<TypeTree>)>,
    source_tpk: Option<&'static SourceTpkDatabase>,
}

impl ReferenceClassLayouts {
    /// Parse the probe output rows.
    pub fn parse(fixture: &str) -> Self {
        let mut generated = std::collections::BTreeMap::new();
        let mut trees: std::collections::BTreeMap<(i32, String, bool), Vec<TypeTreeNode>> =
            std::collections::BTreeMap::new();
        let mut current: Option<(i32, String, bool)> = None;
        for line in fixture.lines() {
            let mut parts = line.split('|');
            match parts.next() {
                Some("GEN") => {
                    let Ok(class_id) = parts.next().unwrap_or_default().parse::<i32>() else {
                        continue;
                    };
                    let name = parts.next().unwrap_or_default();
                    if !name.is_empty() && name != "MISSING" {
                        generated.insert(class_id, name.to_owned());
                    }
                }
                Some("TPK") => {
                    let Ok(class_id) = parts.next().unwrap_or_default().parse::<i32>() else {
                        continue;
                    };
                    let third = parts.next().unwrap_or_default();
                    if third == "yes" || third == "no" {
                        current = None;
                        continue;
                    }
                    let version = third.to_owned();
                    let is_editor = parts.next() == Some("editor");
                    current = Some((class_id, version.clone(), is_editor));
                    trees.entry((class_id, version, is_editor)).or_default();
                }
                Some("NODE") => {
                    let Some((class_id, version, is_editor)) = current.clone() else {
                        continue;
                    };
                    let level = parts
                        .next()
                        .and_then(|value| value.parse::<u8>().ok())
                        .unwrap_or(0);
                    let type_name = parts.next().unwrap_or("").to_owned();
                    let name = parts.next().unwrap_or("").to_owned();
                    let node_version = parts
                        .next()
                        .and_then(|value| value.trim_start_matches('v').parse::<i32>().ok())
                        .unwrap_or(1);
                    let meta = parts
                        .next()
                        .and_then(|value| value.trim_start_matches("meta").parse::<u32>().ok())
                        .unwrap_or(0);
                    let nodes = trees.entry((class_id, version, is_editor)).or_default();
                    let index = i32::try_from(nodes.len()).unwrap_or(i32::MAX);
                    nodes.push(TypeTreeNode {
                        level,
                        version: node_version,
                        type_flags: 0,
                        type_string_offset: None,
                        name_string_offset: None,
                        type_name: Some(type_name),
                        name: Some(name),
                        byte_size: -1,
                        index,
                        meta_flags: meta,
                        reference_type_hash: None,
                    });
                }
                _ => {}
            }
        }
        let mut tpk = std::collections::BTreeMap::new();
        for (class_id, version, _) in trees.keys() {
            let release = trees.get(&(*class_id, version.clone(), false));
            let editor = trees.get(&(*class_id, version.clone(), true));
            if let (Some(release), Some(editor)) = (release, editor) {
                tpk.insert(
                    (*class_id, version.clone()),
                    (
                        Arc::new(TypeTree {
                            nodes: release.clone(),
                            string_buffer: Vec::new(),
                        }),
                        Arc::new(TypeTree {
                            nodes: editor.clone(),
                            string_buffer: Vec::new(),
                        }),
                    ),
                );
            }
        }
        Self {
            generated,
            tpk,
            source_tpk: None,
        }
    }

    /// The tables of the committed probe fixture.
    pub fn from_probe_fixture() -> Self {
        Self::parse(PROBE_FIXTURE)
    }

    /// The probed versions (all reference rows were produced at one version).
    pub fn probed_version(&self) -> Option<&str> {
        self.tpk.keys().next().map(|(_, version)| version.as_str())
    }
}

impl ClassLayoutSource for ReferenceClassLayouts {
    fn generated_class_name(&self, class_id: i32, version: &FactoryUnityVersion) -> Option<&str> {
        // Every GEN row was probed at one version; other versions are unprobed.
        if self.probed_version() != Some(version.text.as_str()) {
            return None;
        }
        self.generated.get(&class_id).map(String::as_str)
    }

    fn tpk_trees(
        &self,
        class_id: i32,
        version: &FactoryUnityVersion,
    ) -> Result<Option<(Arc<TypeTree>, Arc<TypeTree>)>, SourceTpkLookupError> {
        if let Some(trees) = self.tpk.get(&(class_id, version.text.clone())).cloned() {
            return Ok(Some(trees));
        }
        match self.source_tpk {
            Some(database) => database.trees(class_id, version),
            None => Ok(None),
        }
    }
}

/// The source-pinned generated-class and TPK layouts available to the Rust product.
///
/// Generated class names remain limited to explicit factory probes. TPK layouts use
/// the complete source `TpkTypeTreeBlob` class/version/node/string tables.
pub fn reference_class_layouts() -> &'static ReferenceClassLayouts {
    static LAYOUTS: OnceLock<ReferenceClassLayouts> = OnceLock::new();
    LAYOUTS.get_or_init(|| {
        let mut layouts = ReferenceClassLayouts::parse(PROBE_FIXTURE);
        layouts.source_tpk = Some(SourceTpkDatabase::embedded());
        layouts
    })
}

/// A payload decoded with its source TPK TypeTree because the file contains no usable
/// TypeTree for that object.
#[derive(Clone, Debug, PartialEq)]
pub struct TypeTreePayloadRecovery {
    pub payload: ParsedObjectPayload,
    pub type_tree: Arc<TypeTree>,
}

/// Recover one missing file-TypeTree payload through its source TPK class layout.
///
/// Generated classes and fallback `TypeTreeObject`s share the TPK layout data. MonoBehaviour
/// remains separate because its serialized tail depends on assembly/script resolution.
pub fn decode_no_usable_type_tree_payload(
    file: &SerializedFile<'_>,
    object_index: usize,
    collection_name: &str,
    file_path: &str,
    source: &dyn ClassLayoutSource,
    limits: &ObjectDecodeLimits,
) -> Result<Option<TypeTreePayloadRecovery>, ObjectDecodeError> {
    let Some(object) = file.metadata.objects.get(object_index) else {
        return Ok(None);
    };
    let version = file
        .metadata
        .prefix
        .unity_version_text
        .as_deref()
        .and_then(FactoryUnityVersion::parse)
        .unwrap_or_else(FactoryUnityVersion::stripped);
    if version.is_unsupported() {
        return Ok(None);
    }
    let class_id = object_read_class_id(object);
    if class_id == MONO_BEHAVIOUR_CLASS_ID {
        return Ok(None);
    }
    let Some((release_tree, editor_tree)) =
        source.tpk_trees(class_id, &version).map_err(|error| {
            ObjectDecodeError::source_type_tree_lookup(
                object_index,
                class_id,
                version.text.clone(),
                error.to_string(),
            )
        })?
    else {
        return Ok(None);
    };
    let flags = file.object_read_flags(file_path, collection_name);
    let type_tree = if flags.is_release() {
        release_tree
    } else {
        editor_tree
    };
    let decoded = file
        .metadata
        .decode_object_payload_using_type_tree_with_limits(
            file.input(),
            &file.header,
            object_index,
            &type_tree,
            limits,
        )?;
    if matches!(
        classify_object_tail(&decoded.trailing_bytes, class_id == TEXTURE2D_CLASS_ID),
        ObjectTailOutcome::Exact | ObjectTailOutcome::ZeroPadding
    ) {
        Ok(Some(TypeTreePayloadRecovery {
            payload: decoded,
            type_tree,
        }))
    } else {
        Ok(None)
    }
}

/// Materialize every object of one parsed SerializedFile through the ported
/// `GameAssetFactory` engine. This is the product entry point: the CLI/inspect and
/// export paths call it instead of the reader-side per-object decode when they need
/// the source factory's recovery/fallback objects.
pub fn materialize_file_objects(
    file: &SerializedFile<'_>,
    collection_name: &str,
    file_path: &str,
    platform: &str,
    source: &dyn ClassLayoutSource,
    limits: &ObjectDecodeLimits,
) -> Result<Vec<FactoryAsset>, FactoryEngineError> {
    let facts = FactoryFileFacts {
        collection_name,
        file_path,
        platform,
    };
    let mut assets = Vec::new();
    assets
        .try_reserve_exact(file.metadata.objects.len())
        .map_err(|_| FactoryEngineError::ObjectIndexOutOfBounds {
            object_index: 0,
            object_count: file.metadata.objects.len(),
        })?;
    for object_index in 0..file.metadata.objects.len() {
        assets.push(materialize_object(
            file,
            &facts,
            source,
            object_index,
            limits,
        )?);
    }
    Ok(assets)
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
    fn try_read_composition_matches_the_source_control_flow() {
        // A creation miss wraps silently without reading: later facts are ignored.
        for facts in [
            TryReadFacts {
                created: CreatedAssetKind::Unknown,
                read_threw: false,
                tail: ObjectTailOutcome::Exact,
            },
            TryReadFacts {
                created: CreatedAssetKind::Unknown,
                read_threw: true,
                tail: ObjectTailOutcome::IncorrectByteCount,
            },
        ] {
            assert_eq!(
                decide_try_read_object(&facts),
                (TryReadOutcome::UseUnknownObject, TryReadLog::Silent)
            );
        }
        // A thrown read keeps the asset with the exception error, whatever the tail.
        let thrown = TryReadFacts {
            created: CreatedAssetKind::Serialized,
            read_threw: true,
            tail: ObjectTailOutcome::Exact,
        };
        assert_eq!(
            decide_try_read_object(&thrown),
            (
                TryReadOutcome::UseCreatedAsset,
                TryReadLog::ErrorReadException
            )
        );
        // Clean and padded tails stay silent; the Chinese tail warns; leftovers error.
        let base = TryReadFacts {
            created: CreatedAssetKind::TypeTreeObject,
            read_threw: false,
            tail: ObjectTailOutcome::Exact,
        };
        assert_eq!(
            decide_try_read_object(&base),
            (TryReadOutcome::UseCreatedAsset, TryReadLog::Silent)
        );
        assert_eq!(
            decide_try_read_object(&TryReadFacts {
                tail: ObjectTailOutcome::ZeroPadding,
                ..base
            }),
            (TryReadOutcome::UseCreatedAsset, TryReadLog::Silent)
        );
        assert_eq!(
            decide_try_read_object(&TryReadFacts {
                tail: ObjectTailOutcome::ChineseTextureFields,
                ..base
            }),
            (
                TryReadOutcome::UseCreatedAsset,
                TryReadLog::WarningChineseTextureFields
            )
        );
        assert_eq!(
            decide_try_read_object(&TryReadFacts {
                tail: ObjectTailOutcome::IncorrectByteCount,
                ..base
            }),
            (
                TryReadOutcome::UseCreatedAsset,
                TryReadLog::ErrorIncorrectByteCount
            )
        );
    }

    #[test]
    fn player_settings_class_matches_the_typetree_predicate() {
        assert!(super::is_player_settings_class(
            super::PLAYER_SETTINGS_CLASS_ID
        ));
        for class_id in [0, 28, 48, 114, 115, 128, 130, -1, i32::MAX] {
            assert!(!super::is_player_settings_class(class_id), "{class_id}");
        }
    }

    #[test]
    fn monobehaviour_structures_skip_the_generated_base_fields() {
        use super::{MONO_BEHAVIOUR_EDITOR_CLASS_ID_FIELD, MONO_BEHAVIOUR_NAME_FIELD};
        assert_eq!(MONO_BEHAVIOUR_NAME_FIELD, "m_Name");
        assert_eq!(
            MONO_BEHAVIOUR_EDITOR_CLASS_ID_FIELD,
            "m_EditorClassIdentifier"
        );
        // Newer generations carry both markers: fields start after the later one.
        assert_eq!(
            super::monobehaviour_structure_start_index(&[
                "m_GameObject",
                "m_Enabled",
                "m_EditorHideFlags",
                "m_Name",
                "m_EditorClassIdentifier",
                "m_Material",
            ]),
            5
        );
        // Legacy generations without the editor marker start after `m_Name`.
        assert_eq!(
            super::monobehaviour_structure_start_index(&[
                "m_GameObject",
                "m_Enabled",
                "m_Name",
                "health",
            ]),
            3
        );
        // The maximum wins when the markers arrive out of order.
        assert_eq!(
            super::monobehaviour_structure_start_index(
                &["m_EditorClassIdentifier", "m_Name", "x",]
            ),
            2
        );
        // The first match wins, like the source `IndexOf`.
        assert_eq!(
            super::monobehaviour_structure_start_index(&["m_Name", "m_Name", "x"]),
            1
        );
        // Both markers absent starts at zero.
        assert_eq!(
            super::monobehaviour_structure_start_index(&["health", "speed"]),
            0
        );
        assert_eq!(super::monobehaviour_structure_start_index(&[]), 0);
    }

    #[test]
    fn structure_resolvers_follow_the_creation_path() {
        // Engine TypeTreeObjects from Tpk roots never get a resolver, so
        // SerializeReference fields can never resolve inside them.
        assert_eq!(
            super::select_structure_resolver(true),
            super::StructureResolverKind::Null
        );
        // MonoBehaviour TypeTree structures resolve against the reference types.
        assert_eq!(
            super::select_structure_resolver(false),
            super::StructureResolverKind::FromReferenceTypes
        );
    }

    #[test]
    fn typetree_reads_consume_the_matching_tree_and_convert_into_the_other() {
        use super::TypeTreeReadPlan;
        // Single-tree objects read their only fields in either mode.
        assert_eq!(
            super::select_typetree_read_plan(false, true),
            TypeTreeReadPlan::SingleFields
        );
        assert_eq!(
            super::select_typetree_read_plan(false, false),
            TypeTreeReadPlan::SingleFields
        );
        // Double-tree release reads consume release fields, then convert to editor.
        assert_eq!(
            super::select_typetree_read_plan(true, true),
            TypeTreeReadPlan::ReleaseThenConvertToEditor
        );
        // Double-tree editor reads consume editor fields, then convert to release.
        assert_eq!(
            super::select_typetree_read_plan(true, false),
            TypeTreeReadPlan::EditorThenConvertToRelease
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
    fn stripped_type_ids_read_as_monobehaviours_before_dispatch() {
        // The `ReadData` fallback: negatives become 114, everything else is identity.
        assert_eq!(super::resolve_read_class_id(-1), MONO_BEHAVIOUR_CLASS_ID);
        assert_eq!(super::resolve_read_class_id(-2), MONO_BEHAVIOUR_CLASS_ID);
        assert_eq!(
            super::resolve_read_class_id(i32::MIN),
            MONO_BEHAVIOUR_CLASS_ID
        );
        for type_id in [0, 1, 28, 113, 114, 115, 129, i32::MAX] {
            assert_eq!(super::resolve_read_class_id(type_id), type_id);
        }
        // The handoff routes a stripped object down the script path, while the
        // version gate still wins over the fallback like the source call order.
        assert_eq!(
            super::classify_read_data_path(false, -1),
            AssetReadPath::MonoBehaviour
        );
        assert_eq!(
            super::classify_read_data_path(false, 28),
            AssetReadPath::Normal
        );
        assert_eq!(
            super::classify_read_data_path(true, -1),
            AssetReadPath::UnreadableUnsupportedVersion
        );
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

    // -----------------------------------------------------------------------
    // Fixture-driven factory engine differential
    // -----------------------------------------------------------------------

    use crate::serialized_file::file::SerializedFile;

    struct FixtureCase {
        name: String,
        bytes: Vec<u8>,
        collection_name: String,
        rows: Vec<String>,
    }

    fn fixture_cases(fixture: &str) -> Vec<FixtureCase> {
        let mut cases: Vec<FixtureCase> = Vec::new();
        for line in fixture.lines() {
            if let Some(rest) = line.strip_prefix("SF_HEX|") {
                let (name, hex) = rest.split_once('|').expect("SF_HEX name|hex");
                cases.push(FixtureCase {
                    name: name.to_owned(),
                    bytes: hex_bytes(hex),
                    collection_name: String::new(),
                    rows: Vec::new(),
                });
            } else if let Some(rest) = line.strip_prefix("SF_READ|") {
                let (name, row) = rest.split_once('|').expect("SF_READ name|row");
                if let Some(case) = cases.iter_mut().find(|case| case.name == name) {
                    if let Some(file_name) = row
                        .strip_prefix("FILE|")
                        .and_then(|fields| fields.split('|').next())
                    {
                        case.collection_name = file_name.to_owned();
                    }
                    case.rows.push(row.to_owned());
                }
            }
        }
        cases
    }

    fn hex_bytes(hex: &str) -> Vec<u8> {
        assert!(hex.len().is_multiple_of(2), "odd hex length");
        let bytes = hex.as_bytes();
        let mut out = Vec::with_capacity(hex.len() / 2);
        let mut index = 0;
        while index < bytes.len() {
            let digit = |byte: u8| match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                other => panic!("invalid hex byte {other}"),
            };
            out.push((digit(bytes[index]) << 4) | digit(bytes[index + 1]));
            index += 2;
        }
        out
    }

    fn row_value<'a>(case: &'a FixtureCase, prefix: &str) -> Option<&'a str> {
        case.rows.iter().find_map(|row| row.strip_prefix(prefix))
    }

    fn assert_case(case: &FixtureCase, layouts: &ReferenceClassLayouts) {
        let file = SerializedFile::parse(&case.bytes)
            .unwrap_or_else(|error| panic!("{} parse: {error}", case.name));
        let facts = FactoryFileFacts {
            collection_name: &case.collection_name,
            file_path: if case.collection_name.ends_with(".unity") {
                &case.collection_name
            } else {
                ""
            },
            platform: "StandaloneWin64Player",
        };
        let asset = materialize_object(&file, &facts, layouts, 0, &ObjectDecodeLimits::default())
            .unwrap_or_else(|error| panic!("{} materialize: {error}", case.name));

        let reference_type = row_value(case, "ASSET|42|class=")
            .and_then(|row| row.split("|type=").nth(1))
            .and_then(|row| row.split('|').next())
            .expect("an ASSET row");
        let reference_classname = row_value(case, "ASSET|42|class=")
            .and_then(|row| row.split("|classname=").nth(1))
            .and_then(|row| row.split('|').next())
            .expect("a classname");
        let reference_name = row_value(case, "ASSET|42|class=")
            .and_then(|row| row.split("|name=").nth(1))
            .expect("a name");

        let engine_type = match &asset.kind {
            FactoryAssetKind::Generated(name) => name.as_str(),
            FactoryAssetKind::TypeTreeObject => "DoubleTypeTreeObject",
            FactoryAssetKind::Unknown => "UnknownObject",
            FactoryAssetKind::Unreadable => "UnreadableObject",
        };
        assert_eq!(engine_type, reference_type, "{} type", case.name);
        match &asset.kind {
            FactoryAssetKind::TypeTreeObject => {
                let type_name = asset
                    .payload
                    .as_ref()
                    .map(|payload| payload.type_name.as_str());
                assert_eq!(
                    type_name,
                    Some(reference_classname),
                    "{} TPK class name",
                    case.name
                );
            }
            _ => assert_eq!(
                asset.class_name.as_ref(),
                reference_classname,
                "{} class name",
                case.name
            ),
        }
        let expected_name = if reference_name == "NULL" {
            ""
        } else {
            reference_name
        };
        assert_eq!(asset.name, expected_name, "{} name", case.name);

        // The reference error rows must match the engine log exactly.
        let reference_logs: Vec<&str> = case
            .rows
            .iter()
            .filter_map(|row| row.strip_prefix("LOG|Error|Import|"))
            .collect();
        let reference_warnings: Vec<&str> = case
            .rows
            .iter()
            .filter_map(|row| row.strip_prefix("LOG|Warning|Import|"))
            .collect();
        if let Some(expected) = reference_logs.first() {
            let expected = expected.replace('␠', " ");
            assert_eq!(asset.log_level, FactoryLogLevel::Error, "{} log", case.name);
            assert_eq!(
                asset.log_message.as_deref(),
                Some(expected.as_str()),
                "{} log text",
                case.name
            );
        } else if let Some(expected) = reference_warnings.first() {
            let expected = expected.replace('␠', " ");
            assert_eq!(
                asset.log_level,
                FactoryLogLevel::Warning,
                "{} log",
                case.name
            );
            assert_eq!(
                asset.log_message.as_deref(),
                Some(expected.as_str()),
                "{} log text",
                case.name
            );
        } else {
            assert_eq!(
                asset.log_level,
                FactoryLogLevel::Silent,
                "{} log",
                case.name
            );
            assert_eq!(asset.log_message, None, "{} log text", case.name);
        }

        // Case-specific tail, structure, and field fidelity.
        match case.name.as_str() {
            "textasset_notree" => {
                assert_eq!(
                    classify_type_tree_usability(file.metadata.types[0].type_tree.as_ref()),
                    TypeTreeUsability::Empty
                );
                assert_eq!(asset.tail, Some(ObjectTailOutcome::Exact));
                assert_eq!(asset.payload.unwrap().consumed_bytes, 20);
            }
            "textasset_tree" => {
                assert_eq!(
                    classify_type_tree_usability(file.metadata.types[0].type_tree.as_ref()),
                    TypeTreeUsability::Present
                );
                assert_eq!(asset.tail, Some(ObjectTailOutcome::Exact));
            }
            "textasset_padding" => {
                assert_eq!(asset.tail, Some(ObjectTailOutcome::ZeroPadding));
                assert_eq!(asset.payload.as_ref().unwrap().trailing_bytes, [0, 0, 0, 0]);
            }
            "textasset_bad_tail" => {
                assert_eq!(asset.tail, Some(ObjectTailOutcome::IncorrectByteCount));
                assert_eq!(
                    asset.payload.as_ref().unwrap().consumed_bytes,
                    20,
                    "partially read fields stay available"
                );
                assert_eq!(asset.raw.len(), 25, "raw payload preserved");
            }
            "unknown_class" => {
                assert_eq!(asset.class_name.as_ref(), "7");
                assert_eq!(asset.raw, [1, 2, 3, 4, 5, 6, 7, 8]);
                assert!(asset.payload.is_none());
            }
            "tpk_navmesh" => {
                assert_eq!(asset.tail, Some(ObjectTailOutcome::Exact));
                assert_eq!(
                    asset
                        .payload
                        .as_ref()
                        .unwrap()
                        .root
                        .unique_field("m_Name")
                        .unwrap()
                        .unwrap(),
                    &SerializedValue::StringBytes(b"NavMeshData".to_vec())
                );
            }
            "tpk_navmesh_editor" => {
                assert!(
                    !file
                        .object_read_flags(&case.collection_name, &case.collection_name)
                        .is_release(),
                    "the source fixture is a NoTarget .unity file"
                );
                assert_eq!(asset.kind, FactoryAssetKind::TypeTreeObject);
                assert_eq!(asset.tail, Some(ObjectTailOutcome::Exact));
                assert_eq!(
                    asset.raw,
                    hex_bytes(
                        row_value(case, "EDITOR_HEX|")
                            .expect("the source factory records editor serialization")
                    )
                );
                assert_eq!(
                    row_value(case, "RELEASE_HEX|"),
                    Some("0C0000005363656E654E61764D657368")
                );
                let root = &asset.payload.as_ref().unwrap().root;
                assert_eq!(
                    root.unique_field("m_ObjectHideFlags").unwrap().unwrap(),
                    &SerializedValue::U32(3)
                );
                assert_eq!(
                    root.unique_field("m_PrefabParentObject").unwrap().unwrap(),
                    &SerializedValue::PPtr {
                        file_id: 0,
                        path_id: 0,
                    }
                );
                assert_eq!(
                    root.unique_field("m_PrefabInternal").unwrap().unwrap(),
                    &SerializedValue::PPtr {
                        file_id: 0,
                        path_id: 0,
                    }
                );
                assert_eq!(
                    root.unique_field("m_Name").unwrap().unwrap(),
                    &SerializedValue::StringBytes(b"SceneNavMesh".to_vec())
                );
            }
            "unreadable_version" => {
                assert!(asset.payload.is_none());
                assert_eq!(asset.raw.len(), 20);
            }
            "engine_default_resources" => {
                assert!(asset.kind.is_recoverable());
                assert_eq!(asset.tail, Some(ObjectTailOutcome::IncorrectByteCount));
            }
            "mono_unloaded" => {
                let structure = asset.structure.as_ref().expect("a structure decision");
                assert_eq!(
                    structure.outcome,
                    MonoBehaviourStructureOutcome::UseUnloadedRemainder
                );
                assert_eq!(structure.unloaded_remainder, [0xDE, 0xAD, 0xBE, 0xEF]);
            }
            "mono_with_structure" => {
                let structure = asset.structure.as_ref().expect("a structure decision");
                assert_eq!(
                    structure.outcome,
                    MonoBehaviourStructureOutcome::UseParsedStructure,
                    "{} source structure outcome",
                    case.name
                );
                let fields = structure.fields.as_ref().expect("parsed structure fields");
                assert_eq!(
                    fields.unique_field("health").unwrap().unwrap(),
                    &SerializedValue::I32(100)
                );
                assert!(row_value(case, "STRUCTURE|SerializableStructure").is_some());
            }
            "monoscript_notree" => {
                let root = &asset.payload.as_ref().unwrap().root;
                assert_eq!(
                    root.unique_field("m_ClassName").unwrap().unwrap(),
                    &SerializedValue::StringBytes(b"HeroController".to_vec())
                );
                assert_eq!(
                    root.unique_field("m_AssemblyName").unwrap().unwrap(),
                    &SerializedValue::StringBytes(b"Assembly-CSharp".to_vec())
                );
                assert_eq!(
                    root.unique_field("m_ExecutionOrder").unwrap().unwrap(),
                    &SerializedValue::I32(200)
                );
            }
            other => panic!("unexpected case {other}"),
        }
    }

    /// Every reference-authored case in `asset_factory_fixture.txt` is replayed through
    /// the production engine and compared row by row with the shipped factory.
    #[test]
    fn factory_differential_replays_every_reference_case() {
        let fixture = include_str!("asset_factory_fixture.txt");
        let layouts = ReferenceClassLayouts::parse(fixture);
        let cases = fixture_cases(fixture);
        assert_eq!(cases.len(), 12, "all probe cases are replayed");
        for case in &cases {
            assert_case(case, &layouts);
        }
    }
}
