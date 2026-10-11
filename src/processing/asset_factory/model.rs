use std::borrow::Cow;
use std::fmt::{self, Display, Formatter};

use crate::processing::source_factory_dispatch::SourceFactoryLookupError;
use crate::processing::source_tpk::{SourceTpkLookupError, SourceTpkTrees};
use crate::serialized_file::object::{
    version_is_unsupported, ParsedObjectPayload, SerializedValue,
};

use super::decisions::{MonoBehaviourStructureOutcome, ObjectTailOutcome};
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

    /// Eligibility for the processing step that resolves a source UnloadedStructure
    /// to an empty field set. Materialization itself retains the original lazy state.
    pub(crate) fn has_complete_null_script_header(&self, payload: &ParsedObjectPayload) -> bool {
        self.class_id == super::decisions::MONO_BEHAVIOUR_CLASS_ID
            && self.kind.is_recoverable()
            && self.log_level != FactoryLogLevel::Error
            && matches!(self.tail, None | Some(ObjectTailOutcome::Exact))
            && payload.trailing_bytes.is_empty()
            && self.structure.as_ref().is_some_and(|structure| {
                structure.outcome == MonoBehaviourStructureOutcome::UseUnloadedRemainder
                    && structure.unloaded_remainder.is_empty()
                    && structure.fields.is_none()
            })
            && matches!(
                payload.root.unique_field_in_base_chain("m_Script"),
                Ok(Some(SerializedValue::PPtr { path_id: 0, .. }))
            )
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
    pub(super) fn into_workspace_materialization(self) -> FactoryObjectMaterialization {
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
