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
