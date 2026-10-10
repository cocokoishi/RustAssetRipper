use crate::processing::source_factory_dispatch::SourceFactoryDispatch;
pub use crate::processing::source_factory_dispatch::SourceFactoryLookupError;
use crate::processing::source_tpk::SourceTpkDatabase;
pub use crate::processing::source_tpk::{SourceTpkLookupError, SourceTpkTrees};
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

/// Byte offset of `m_OriginalWidth` (body-endian Int32) in the non-standard Chinese
/// texture tail (`GameAssetFactory.ReadExtraTextureFields`).
pub const CHINESE_TEXTURE_ORIGINAL_WIDTH_OFFSET: usize = 0;
/// Byte offset of `m_OriginalHeight` (body-endian Int32) in the tail.
pub const CHINESE_TEXTURE_ORIGINAL_HEIGHT_OFFSET: usize = 4;
/// Byte offset of `m_OriginalAssetGuid` (body-endian GUID release read, 16 bytes) in the tail.
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
    fn generated_class_name(
        &self,
        class_id: i32,
        version: &FactoryUnityVersion,
    ) -> Result<Option<&str>, SourceFactoryLookupError>;

    /// `TypeTreeNodeStruct.TryMakeFromTpk`: the class's release and editor trees.
    fn tpk_trees(
        &self,
        class_id: i32,
        version: &FactoryUnityVersion,
    ) -> Result<Option<SourceTpkTrees>, SourceTpkLookupError>;
}

/// A schema source for synthetic module fixtures that intentionally exercise arbitrary file
/// TypeTrees. Production entry points use `reference_class_layouts`; this test source makes no
/// claim about AssetRipper compatibility and must not be used as parity evidence.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct FileTypeTreeTestLayouts;

#[cfg(test)]
impl ClassLayoutSource for FileTypeTreeTestLayouts {
    fn generated_class_name(
        &self,
        _class_id: i32,
        _version: &FactoryUnityVersion,
    ) -> Result<Option<&str>, SourceFactoryLookupError> {
        Ok(Some("SyntheticTypeTreeTestObject"))
    }

    fn tpk_trees(
        &self,
        _class_id: i32,
        _version: &FactoryUnityVersion,
    ) -> Result<Option<SourceTpkTrees>, SourceTpkLookupError> {
        Ok(None)
    }
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

/// The source-factory result retained by `ObjectWorkspace`. Raw object bytes stay in the VFS and
/// are addressed by the workspace object's byte range, so this view does not duplicate them.
#[derive(Clone, Debug, PartialEq)]
pub struct FactoryObjectState {
    pub object_index: usize,
    pub path_id: i64,
    pub class_id: i32,
    pub kind: FactoryAssetKind,
    pub class_name: Cow<'static, str>,
    pub name: String,
    pub tail: Option<ObjectTailOutcome>,
    pub chinese_texture_fields: Option<ChineseTextureFields>,
    pub structure: Option<FactoryStructure>,
    pub log_level: FactoryLogLevel,
    pub log_message: Option<String>,
}

impl FactoryObjectState {
    /// Whether this source-factory result and its decoded value represent the full serialized
    /// object rather than a raw or partially read fallback.
    pub fn is_fully_materialized(&self, payload: Option<&ParsedObjectPayload>) -> bool {
        let Some(payload) = payload else {
            return false;
        };
        if !self.kind.is_recoverable()
            || self.log_level == FactoryLogLevel::Error
            || self.tail == Some(ObjectTailOutcome::IncorrectByteCount)
        {
            return false;
        }
        let tail_is_complete = match self.tail {
            Some(ObjectTailOutcome::Exact) => payload.trailing_bytes.is_empty(),
            Some(ObjectTailOutcome::ZeroPadding | ObjectTailOutcome::ChineseTextureFields) => true,
            Some(ObjectTailOutcome::IncorrectByteCount) => false,
            None => payload.trailing_bytes.is_empty(),
        };
        if !tail_is_complete {
            return false;
        }
        match self.structure.as_ref().map(|structure| structure.outcome) {
            Some(MonoBehaviourStructureOutcome::UseParsedStructure) | None => true,
            Some(
                MonoBehaviourStructureOutcome::ClearStructure
                | MonoBehaviourStructureOutcome::UseUnloadedRemainder
                | MonoBehaviourStructureOutcome::KeepExisting,
            ) => false,
        }
    }
}

/// Typed source-factory values for one workspace object, without a second raw-byte copy.
#[derive(Clone, Debug, PartialEq)]
pub struct FactoryObjectMaterialization {
    pub state: FactoryObjectState,
    pub payload: Option<ParsedObjectPayload>,
}

impl FactoryObjectMaterialization {
    /// Whether the factory recovered all serialized values needed to represent this object.
    /// An unloaded MonoBehaviour remainder and a failed normal-object tail remain incomplete even
    /// when the source factory retained a partially decoded header.
    pub fn is_fully_materialized(&self) -> bool {
        self.state.is_fully_materialized(self.payload.as_ref())
    }
}

impl FactoryAsset {
    fn into_workspace_materialization(self) -> FactoryObjectMaterialization {
        let Self {
            object_index,
            path_id,
            class_id,
            kind,
            class_name,
            name,
            raw: _,
            payload,
            tail,
            chinese_texture_fields,
            structure,
            log_level,
            log_message,
        } = self;
        FactoryObjectMaterialization {
            state: FactoryObjectState {
                object_index,
                path_id,
                class_id,
                kind,
                class_name,
                name,
                tail,
                chinese_texture_fields,
                structure,
                log_level,
                log_message,
            },
            payload,
        }
    }
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

/// The read class id the source collection uses: `ObjectInfo.TypeID` is retained as
/// `raw_type_id`, with the stripped (negative) fallback to MonoBehaviour. The legacy
/// object-table `class_id` field is not the value passed to `AssetInfo` by the source.
pub fn object_read_class_id(object: &ObjectInfo) -> i32 {
    resolve_read_class_id(object.raw_type_id)
}

/// `GameAssetFactory.ReadAsset`: dispatch one file object through the factory.
pub fn materialize_object(
    file: &SerializedFile<'_>,
    facts: &FactoryFileFacts<'_>,
    source: &dyn ClassLayoutSource,
    object_index: usize,
    limits: &ObjectDecodeLimits,
) -> Result<FactoryAsset, FactoryEngineError> {
    materialize_object_inner(file, facts, source, object_index, limits, true)
}

/// Materialize one object for `ObjectWorkspace` without retaining a duplicate of its raw bytes.
/// The source VFS remains the owner of those bytes and the workspace stores their validated range.
pub fn materialize_object_for_workspace(
    file: &SerializedFile<'_>,
    facts: &FactoryFileFacts<'_>,
    source: &dyn ClassLayoutSource,
    object_index: usize,
    limits: &ObjectDecodeLimits,
) -> Result<FactoryObjectMaterialization, FactoryEngineError> {
    materialize_object_inner(file, facts, source, object_index, limits, false)
        .map(FactoryAsset::into_workspace_materialization)
}

fn materialize_object_inner(
    file: &SerializedFile<'_>,
    facts: &FactoryFileFacts<'_>,
    source: &dyn ClassLayoutSource,
    object_index: usize,
    limits: &ObjectDecodeLimits,
    retain_raw_bytes: bool,
) -> Result<FactoryAsset, FactoryEngineError> {
    let object = file.metadata.objects.get(object_index).ok_or(
        FactoryEngineError::ObjectIndexOutOfBounds {
            object_index,
            object_count: file.metadata.objects.len(),
        },
    )?;
    let raw_bytes = object_payload_slice(file, object_index)?;
    let raw = if retain_raw_bytes {
        raw_bytes.to_vec()
    } else {
        Vec::new()
    };
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
            raw_data_hash(raw_bytes),
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
            raw_bytes,
            &mut asset,
        );
        return Ok(asset);
    }

    read_normal_object(
        NormalObjectReadContext {
            file,
            facts,
            source,
            limits,
            version: &version,
        },
        object_index,
        raw_bytes,
        &mut asset,
    );
    Ok(asset)
}

fn object_payload_slice<'a>(
    file: &'a SerializedFile<'_>,
    object_index: usize,
) -> Result<&'a [u8], FactoryEngineError> {
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
) -> Result<CreatedAsset, String> {
    let generated_name = source
        .generated_class_name(class_id, version)
        .map_err(|error| error.to_string())?;
    if let Some(name) = generated_name {
        Ok(CreatedAsset::Generated(name.to_owned()))
    } else if source
        .tpk_trees(class_id, version)
        .map_err(|error| error.to_string())?
        .is_some()
    {
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

#[derive(Clone, Copy)]
struct NormalObjectReadContext<'a, 'file, 'facts> {
    file: &'a SerializedFile<'file>,
    facts: &'a FactoryFileFacts<'facts>,
    source: &'a dyn ClassLayoutSource,
    limits: &'a ObjectDecodeLimits,
    version: &'a FactoryUnityVersion,
}

fn read_normal_object(
    context: NormalObjectReadContext<'_, '_, '_>,
    object_index: usize,
    raw_bytes: &[u8],
    asset: &mut FactoryAsset,
) {
    match try_read_normal_object(
        context.file,
        context.facts,
        context.source,
        object_index,
        context.limits,
        context.version,
        raw_bytes,
    ) {
        Ok(attempt) => apply_attempt(asset, attempt, raw_bytes),
        Err(failed) => {
            if is_engine_collection_name(Some(context.facts.collection_name)) {
                // Engine collections tolerate failed reads: shaders log info, others warn.
                let (log_level, log_message) = if asset.class_id == SHADER_CLASS_ID {
                    (FactoryLogLevel::Info, None)
                } else {
                    (FactoryLogLevel::Warning, failed.log_message.clone())
                };
                apply_attempt(asset, *failed, raw_bytes);
                asset.log_level = log_level;
                asset.log_message = log_message;
            } else if context.version.is_patch() {
                let retry_version = context.version.with_bumped_build();
                match try_read_normal_object(
                    context.file,
                    context.facts,
                    context.source,
                    object_index,
                    context.limits,
                    &retry_version,
                    raw_bytes,
                ) {
                    Ok(attempt) => apply_attempt(asset, attempt, raw_bytes),
                    Err(_failed_retry) => unreadable_from_failed(asset, *failed, raw_bytes),
                }
            } else {
                unreadable_from_failed(asset, *failed, raw_bytes);
            }
        }
    }
}

/// The class id of `Shader`, whose engine-collection failure logs at info level.
const SHADER_CLASS_ID: i32 = 48;

fn unreadable_from_failed(asset: &mut FactoryAsset, failed: NormalAttempt, raw_bytes: &[u8]) {
    let stored_name = named_field_owned(&failed.payload);
    let message = failed.log_message.clone();
    apply_attempt(asset, failed, raw_bytes);
    asset.kind = FactoryAssetKind::Unreadable;
    asset.name = unreadable_asset_name(
        asset.class_name.as_ref(),
        stored_name.as_deref(),
        raw_data_hash(raw_bytes),
    );
    asset.log_level = FactoryLogLevel::Error;
    asset.log_message = message;
}

fn apply_attempt(asset: &mut FactoryAsset, attempt: NormalAttempt, raw_bytes: &[u8]) {
    asset.kind = attempt.kind;
    asset.payload = attempt.payload;
    asset.tail = attempt.tail;
    asset.chinese_texture_fields = attempt.chinese;
    asset.log_level = attempt.log_level;
    asset.log_message = attempt.log_message;
    match &asset.kind {
        FactoryAssetKind::Unknown => {
            asset.name = unknown_asset_name(asset.class_name.as_ref(), raw_data_hash(raw_bytes));
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
    payload_bytes: &[u8],
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
                log_message: Some(error),
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
        payload_bytes,
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
                    let name = named_field_of(&payload.root).unwrap_or_default().to_owned();
                    let chinese = parse_chinese_texture_fields(
                        &payload.trailing_bytes,
                        file.metadata.prefix.body_endian,
                    );
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

/// The class id of `Texture2D`, which may carry the 24-byte Chinese tail.
const TEXTURE2D_CLASS_ID: i32 = 28;

fn parse_chinese_texture_fields(trailing: &[u8], endian: Endian) -> Option<ChineseTextureFields> {
    if trailing.len() != CHINESE_TEXTURE_EXTRA_BYTES {
        return None;
    }
    let read_u32 = |offset: usize| -> Option<u32> {
        let end = offset.checked_add(4)?;
        let bytes: [u8; 4] = trailing.get(offset..end)?.try_into().ok()?;
        Some(match endian {
            Endian::Little => u32::from_le_bytes(bytes),
            Endian::Big => u32::from_be_bytes(bytes),
        })
    };
    let original_width = read_u32(CHINESE_TEXTURE_ORIGINAL_WIDTH_OFFSET)? as i32;
    let original_height = read_u32(CHINESE_TEXTURE_ORIGINAL_HEIGHT_OFFSET)? as i32;
    let mut guid = [0u32; 4];
    for (index, slot) in guid.iter_mut().enumerate() {
        let offset = CHINESE_TEXTURE_ORIGINAL_GUID_OFFSET + index * 4;
        *slot = read_u32(offset)?;
    }
    Some(ChineseTextureFields {
        original_width,
        original_height,
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
        managed_reference_type_resolver: None,
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
