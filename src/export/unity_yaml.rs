//! Bounded Unity YAML document, node, and pointer formatting primitives.
//!
//! Mapping entries also cover the `serializedVersion` append/prepend gates
//! (`YamlSerializedVersionExtensions`); typeless-data mapping assembly
//! (`AddTypelessData`) stays with the value converter, which owns byte emission.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::processing::scene_naming::SceneUnityVersion;

use super::pointer_map::{
    ExportPointerGuid, ProjectPointerResolution, UnityMetaPointer, UnityProjectAssetType,
    UnresolvedProjectPointer,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnityYamlMappingStyle {
    Block,
    Flow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnityYamlSequenceStyle {
    Block,
    BlockCurve,
    Flow,
}

#[derive(Clone, Debug, PartialEq)]
pub enum UnityYamlScalar {
    Null,
    Boolean(bool),
    Signed(i64),
    Unsigned(u64),
    Float32(f32),
    Float64(f64),
    Float32Hex(f32),
    Float64Hex(f64),
    String(String),
    PlainString(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum UnityYamlNodeKind {
    Scalar(UnityYamlScalar),
    Mapping {
        style: UnityYamlMappingStyle,
        entries: Vec<(UnityYamlNode, UnityYamlNode)>,
    },
    Sequence {
        style: UnityYamlSequenceStyle,
        items: Vec<UnityYamlNode>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct UnityYamlNode {
    pub kind: UnityYamlNodeKind,
    pub tag: Option<String>,
    pub anchor: Option<String>,
    pub stripped: bool,
}

impl UnityYamlNode {
    pub fn scalar(value: UnityYamlScalar) -> Self {
        Self {
            kind: UnityYamlNodeKind::Scalar(value),
            tag: None,
            anchor: None,
            stripped: false,
        }
    }

    pub fn string(value: impl Into<String>) -> Self {
        Self::scalar(UnityYamlScalar::String(value.into()))
    }

    pub fn plain_string(value: impl Into<String>) -> Self {
        Self::scalar(UnityYamlScalar::PlainString(value.into()))
    }

    pub fn mapping(style: UnityYamlMappingStyle) -> Self {
        Self {
            kind: UnityYamlNodeKind::Mapping {
                style,
                entries: Vec::new(),
            },
            tag: None,
            anchor: None,
            stripped: false,
        }
    }

    pub fn sequence(style: UnityYamlSequenceStyle) -> Self {
        Self {
            kind: UnityYamlNodeKind::Sequence {
                style,
                items: Vec::new(),
            },
            tag: None,
            anchor: None,
            stripped: false,
        }
    }

    pub fn insert(
        &mut self,
        key: UnityYamlNode,
        value: UnityYamlNode,
    ) -> Result<(), UnityYamlNodeBuildError> {
        let UnityYamlNodeKind::Mapping { entries, .. } = &mut self.kind else {
            return Err(UnityYamlNodeBuildError::NotAMapping);
        };
        entries
            .try_reserve(1)
            .map_err(|_| UnityYamlNodeBuildError::AllocationFailed)?;
        entries.push((key, value));
        Ok(())
    }

    pub fn insert_field(
        &mut self,
        key: impl Into<String>,
        value: UnityYamlNode,
    ) -> Result<(), UnityYamlNodeBuildError> {
        self.insert(Self::plain_string(key), value)
    }

    /// Prepends a key/value pair to a mapping, mirroring `YamlMappingNode.InsertBegin`.
    pub fn insert_begin(
        &mut self,
        key: UnityYamlNode,
        value: UnityYamlNode,
    ) -> Result<(), UnityYamlNodeBuildError> {
        let UnityYamlNodeKind::Mapping { entries, .. } = &mut self.kind else {
            return Err(UnityYamlNodeBuildError::NotAMapping);
        };
        entries
            .try_reserve(1)
            .map_err(|_| UnityYamlNodeBuildError::AllocationFailed)?;
        entries.insert(0, (key, value));
        Ok(())
    }

    /// Prepends a plain-string field to a mapping, mirroring
    /// `YamlMappingNode.InsertBegin(string, YamlNode)` (keys are plain scalars,
    /// like [`Self::insert_field`]).
    pub fn insert_field_begin(
        &mut self,
        key: impl Into<String>,
        value: UnityYamlNode,
    ) -> Result<(), UnityYamlNodeBuildError> {
        self.insert_begin(Self::plain_string(key), value)
    }

    pub fn push(&mut self, value: UnityYamlNode) -> Result<(), UnityYamlNodeBuildError> {
        let UnityYamlNodeKind::Sequence { items, .. } = &mut self.kind else {
            return Err(UnityYamlNodeBuildError::NotASequence);
        };
        items
            .try_reserve(1)
            .map_err(|_| UnityYamlNodeBuildError::AllocationFailed)?;
        items.push(value);
        Ok(())
    }

    fn node_type(&self) -> UnityYamlNodeType {
        match self.kind {
            UnityYamlNodeKind::Scalar(_) => UnityYamlNodeType::Scalar,
            UnityYamlNodeKind::Mapping { .. } => UnityYamlNodeType::Mapping,
            UnityYamlNodeKind::Sequence { .. } => UnityYamlNodeType::Sequence,
        }
    }

    fn is_multiline(&self) -> bool {
        match &self.kind {
            UnityYamlNodeKind::Scalar(_) => false,
            UnityYamlNodeKind::Mapping { style, entries } => {
                *style == UnityYamlMappingStyle::Block && !entries.is_empty()
            }
            UnityYamlNodeKind::Sequence { style, items } => {
                *style != UnityYamlSequenceStyle::Flow && !items.is_empty()
            }
        }
    }

    fn is_indent(&self) -> bool {
        matches!(
            self.kind,
            UnityYamlNodeKind::Mapping {
                style: UnityYamlMappingStyle::Block,
                ..
            }
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UnityYamlNodeType {
    Scalar,
    Mapping,
    Sequence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnityYamlNodeBuildError {
    NotAMapping,
    NotASequence,
    AllocationFailed,
}

impl Display for UnityYamlNodeBuildError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAMapping => {
                f.write_str("cannot insert a mapping entry into a non-mapping YAML node")
            }
            Self::NotASequence => f.write_str("cannot append an item to a non-sequence YAML node"),
            Self::AllocationFailed => f.write_str("could not allocate Unity YAML node children"),
        }
    }
}

impl Error for UnityYamlNodeBuildError {}

/// Key for the `serializedVersion` mapping entry
/// (`YamlSerializedVersionExtensions.SerializedVersionName`).
pub const SERIALIZED_VERSION_KEY: &str = "serializedVersion";

/// `AddSerializedVersion` gate: the entry is appended only when `version > 1`.
pub fn serialized_version_is_appended(version: i32) -> bool {
    version > 1
}

/// `ForceAddSerializedVersion` gate: the entry is appended when `version > 0`.
pub fn serialized_version_is_forced(version: i32) -> bool {
    version > 0
}

/// `InsertSerializedVersion` gate: the entry is prepended (see
/// [`UnityYamlNode::insert_field_begin`]) only when `version > 1`.
pub fn serialized_version_is_prepended(version: i32) -> bool {
    version > 1
}

#[derive(Clone, Debug, PartialEq)]
pub struct UnityYamlDocument {
    pub root: UnityYamlNode,
}

impl UnityYamlDocument {
    pub fn object(
        class_id: i32,
        file_id: i64,
        class_name: impl Into<String>,
        body: UnityYamlNode,
        stripped: bool,
    ) -> Result<Self, UnityYamlNodeBuildError> {
        let mut root = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        root.tag = Some(format!("!u!{class_id}"));
        root.anchor = Some(file_id.to_string());
        root.stripped = stripped;
        root.insert_field(class_name, body)?;
        Ok(Self { root })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnityYamlWriteLimits {
    pub max_documents: usize,
    pub max_nodes: usize,
    pub max_depth: usize,
    pub max_output_bytes: usize,
}

impl Default for UnityYamlWriteLimits {
    fn default() -> Self {
        Self {
            max_documents: 1_000_000,
            max_nodes: 100_000_000,
            max_depth: 256,
            max_output_bytes: 2 * 1024 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum UnityYamlWriteError {
    DocumentLimitExceeded { requested: usize, limit: usize },
    RootMustBeMapping,
    NodeLimitExceeded { requested: usize, limit: usize },
    DepthLimitExceeded { requested: usize, limit: usize },
    IndentLimitExceeded { requested: usize, limit: usize },
    OutputLimitExceeded { requested: usize, limit: usize },
    AllocationFailed { requested: usize },
    InvalidMappingKey,
    MissingReferenceGuidRequired,
    UnresolvedInputReference(Box<UnresolvedProjectPointer>),
}

impl Display for UnityYamlWriteError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::DocumentLimitExceeded { requested, limit } => write!(
                f,
                "Unity YAML stream has {requested} documents, exceeding limit {limit}"
            ),
            Self::RootMustBeMapping => {
                f.write_str("a standalone Unity YAML mapping requires a mapping root")
            }
            Self::NodeLimitExceeded { requested, limit } => {
                write!(f, "Unity YAML stream has {requested} nodes, exceeding limit {limit}")
            }
            Self::DepthLimitExceeded { requested, limit } => write!(
                f,
                "Unity YAML node depth {requested} exceeds limit {limit}"
            ),
            Self::IndentLimitExceeded { requested, limit } => write!(
                f,
                "Unity YAML indentation {requested} exceeds limit {limit}"
            ),
            Self::OutputLimitExceeded { requested, limit } => write!(
                f,
                "Unity YAML output would reach {requested} bytes, exceeding limit {limit}"
            ),
            Self::AllocationFailed { requested } => {
                write!(f, "could not allocate Unity YAML output for {requested} bytes")
            }
            Self::InvalidMappingKey => f.write_str("Unity YAML mapping keys must be scalar nodes"),
            Self::MissingReferenceGuidRequired => f.write_str(
                "serializing a missing-reference pointer requires the baseline MissingReference GUID",
            ),
            Self::UnresolvedInputReference(reference) => write!(
                f,
                "cannot emit unresolved PPtr from file {} object {} PathID {} field {:?} (FileID {}, PathID {}): {:?}",
                reference.source_file_index,
                reference.source_object_index,
                reference.source_path_id,
                reference.field_path,
                reference.serialized_file_id,
                reference.serialized_path_id,
                reference.resolution
            ),
        }
    }
}

impl Error for UnityYamlWriteError {}

/// Write one or more Unity YAML documents using the baseline header, tags, anchors, and separators.
pub fn write_unity_yaml_documents(
    documents: &[UnityYamlDocument],
    limits: UnityYamlWriteLimits,
) -> Result<String, UnityYamlWriteError> {
    if documents.len() > limits.max_documents {
        return Err(UnityYamlWriteError::DocumentLimitExceeded {
            requested: documents.len(),
            limit: limits.max_documents,
        });
    }

    let mut emitter = UnityYamlEmitter {
        output: String::new(),
        pending_whitespace: false,
        pending_separator: false,
        pending_line_break: false,
        indent: 0,
        node_count: 0,
        limits,
        format_keys: false,
        is_key: false,
    };
    emitter.raw_str("%YAML 1.1\n%TAG !u! tag:unity3d.com,2011:\n")?;
    let mut separator = true;
    for document in documents {
        if separator {
            emitter.write_str("---")?;
            emitter.write_whitespace();
        }
        emitter.emit_node(&document.root, 0)?;
        separator = true;
    }
    // The baseline writes one final line feed directly, while a pending node newline is not flushed.
    emitter.raw_str("\n")?;
    Ok(emitter.output)
}

/// Write one standalone YAML mapping without Unity document headers or separators.
///
/// This is used for Unity sidecar files such as `.meta`; the node, depth, indentation, and output
/// byte limits are the same as for serialized-object YAML.
pub fn write_unity_yaml_mapping(
    root: &UnityYamlNode,
    limits: UnityYamlWriteLimits,
) -> Result<String, UnityYamlWriteError> {
    if root.node_type() != UnityYamlNodeType::Mapping {
        return Err(UnityYamlWriteError::RootMustBeMapping);
    }
    let mut emitter = UnityYamlEmitter {
        output: String::new(),
        pending_whitespace: false,
        pending_separator: false,
        pending_line_break: false,
        indent: 0,
        node_count: 0,
        limits,
        format_keys: true,
        is_key: false,
    };
    emitter.emit_node(root, 0)?;
    emitter.raw_str("\n")?;
    Ok(emitter.output)
}

/// Convert the baseline project-export `MetaPtr` representation into a flow-style YAML mapping.
///
/// `missing_reference_guid` must be the `UnityGuid.MissingReference` value from the frozen baseline;
/// it is required only for that sentinel and is deliberately not guessed here.
pub fn meta_pointer_to_unity_yaml(
    pointer: UnityMetaPointer,
    export_version: SceneUnityVersion,
    missing_reference_guid: Option<[u8; 16]>,
) -> Result<UnityYamlNode, UnityYamlWriteError> {
    let mut mapping = UnityYamlNode::mapping(UnityYamlMappingStyle::Flow);
    mapping
        .insert_field(
            "fileID",
            UnityYamlNode::scalar(UnityYamlScalar::Signed(pointer.file_id)),
        )
        .map_err(|_| UnityYamlWriteError::AllocationFailed { requested: 1 })?;

    let guid = match pointer.guid {
        ExportPointerGuid::None => None,
        ExportPointerGuid::Collection(guid) => Some(guid),
        ExportPointerGuid::MissingReference => {
            let guid =
                missing_reference_guid.ok_or(UnityYamlWriteError::MissingReferenceGuidRequired)?;
            if guid == [0; 16] {
                return Err(UnityYamlWriteError::MissingReferenceGuidRequired);
            }
            Some(guid)
        }
    };
    if let Some(guid) = guid.filter(|guid| *guid != [0; 16]) {
        mapping
            .insert_field("guid", UnityYamlNode::string(format_unity_guid(guid)))
            .map_err(|_| UnityYamlWriteError::AllocationFailed { requested: 1 })?;
        let asset_type =
            if export_version.major < 4 && pointer.asset_type == UnityProjectAssetType::Meta {
                UnityProjectAssetType::Cached
            } else {
                pointer.asset_type
            };
        mapping
            .insert_field(
                "type",
                UnityYamlNode::scalar(UnityYamlScalar::Signed(i64::from(
                    asset_type.as_unity_value(),
                ))),
            )
            .map_err(|_| UnityYamlWriteError::AllocationFailed { requested: 1 })?;
    }
    Ok(mapping)
}

/// Convert a resolved project pointer to YAML.
///
/// References into external files that were not supplied export as
/// missing-reference pointers (matching the reference exporter): the fileID
/// keeps the target's serialized PathID so the dangling edge stays
/// identifiable. Every other unresolvable input cause remains an explicit
/// error.
pub fn project_pointer_to_unity_yaml(
    resolution: &ProjectPointerResolution,
    export_version: SceneUnityVersion,
    missing_reference_guid: Option<[u8; 16]>,
) -> Result<UnityYamlNode, UnityYamlWriteError> {
    let pointer = match resolution {
        ProjectPointerResolution::Null => UnityMetaPointer {
            file_id: 0,
            guid: ExportPointerGuid::None,
            asset_type: UnityProjectAssetType::Serialized,
        },
        ProjectPointerResolution::Mapped(pointer)
        | ProjectPointerResolution::MissingExportedTarget { pointer, .. } => *pointer,
        ProjectPointerResolution::UnresolvedInput(reference) => {
            if !matches!(
                reference.resolution,
                crate::serialized_file::reference_graph::MultiFilePPtrResolution::MissingDependency { .. }
            ) {
                return Err(UnityYamlWriteError::UnresolvedInputReference(Box::new(
                    reference.clone(),
                )));
            }
            UnityMetaPointer {
                file_id: reference.serialized_path_id,
                guid: ExportPointerGuid::MissingReference,
                asset_type: UnityProjectAssetType::Internal,
            }
        }
    };
    meta_pointer_to_unity_yaml(pointer, export_version, missing_reference_guid)
}

pub(crate) fn format_unity_guid(bytes: [u8; 16]) -> String {
    // `UnityGuid.ToString` emits each 32-bit word's nibbles from least significant to most
    // significant, which is the canonical Unity GUID layout (e.g. the `unity default resources`
    // GUID `0000000000000000e000000000000000`).
    let mut value = String::with_capacity(32);
    for word in bytes.chunks_exact(4) {
        let word = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
        for shift in 0..8 {
            let digit = ((word >> (shift * 4)) & 0x0f) as usize;
            value.push(b"0123456789abcdef"[digit] as char);
        }
    }
    value
}

/// A failure while formatting a `Hash128`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Hash128Error {
    /// The baseline's `HashHelper.ToString` uses checked 32-bit arithmetic, so a word whose most
    /// significant byte has the high bit set overflows instead of formatting.
    Overflow,
}

impl core::fmt::Display for Hash128Error {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Overflow => {
                formatter.write_str("Hash128 word overflows the baseline's checked int")
            }
        }
    }
}

impl std::error::Error for Hash128Error {}

/// `HashHelper.ToString`: each 32-bit word's big-endian hex, the opposite nibble order from a GUID.
/// The baseline throws on a word whose most significant byte has the high bit set.
pub fn format_hash128(bytes: [u8; 16]) -> Result<String, Hash128Error> {
    for word in bytes.chunks_exact(4) {
        if word[3] >= 0x80 {
            return Err(Hash128Error::Overflow);
        }
    }
    let mut value = String::with_capacity(32);
    for word in bytes.chunks_exact(4) {
        let word = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
        for shift in (0..8).rev() {
            let digit = ((word >> (shift * 4)) & 0x0f) as usize;
            value.push(b"0123456789abcdef"[digit] as char);
        }
    }
    Ok(value)
}

struct UnityYamlEmitter {
    output: String,
    pending_whitespace: bool,
    pending_separator: bool,
    pending_line_break: bool,
    indent: usize,
    node_count: usize,
    limits: UnityYamlWriteLimits,
    format_keys: bool,
    is_key: bool,
}

impl UnityYamlEmitter {
    fn emit_node(&mut self, node: &UnityYamlNode, depth: usize) -> Result<(), UnityYamlWriteError> {
        const MAX_SAFE_RECURSION_DEPTH: usize = 512;
        let depth_limit = self.limits.max_depth.min(MAX_SAFE_RECURSION_DEPTH);
        if depth > depth_limit {
            return Err(UnityYamlWriteError::DepthLimitExceeded {
                requested: depth,
                limit: depth_limit,
            });
        }
        self.node_count =
            self.node_count
                .checked_add(1)
                .ok_or(UnityYamlWriteError::NodeLimitExceeded {
                    requested: usize::MAX,
                    limit: self.limits.max_nodes,
                })?;
        if self.node_count > self.limits.max_nodes {
            return Err(UnityYamlWriteError::NodeLimitExceeded {
                requested: self.node_count,
                limit: self.limits.max_nodes,
            });
        }

        let mut wrote_decoration = false;
        if let Some(tag) = &node.tag {
            self.write_str(tag)?;
            self.write_whitespace();
            wrote_decoration = true;
        }
        if let Some(anchor) = &node.anchor {
            self.write_char('&')?;
            self.write_str(anchor)?;
            self.write_whitespace();
            wrote_decoration = true;
        }
        if node.stripped {
            self.write_str("stripped")?;
            self.write_whitespace();
            wrote_decoration = true;
        }
        if wrote_decoration && node.is_multiline() {
            self.write_line();
        }

        match &node.kind {
            UnityYamlNodeKind::Scalar(scalar) => self.emit_scalar(scalar),
            UnityYamlNodeKind::Mapping { style, entries } => {
                self.emit_mapping(*style, entries, depth)
            }
            UnityYamlNodeKind::Sequence { style, items } => {
                self.emit_sequence(*style, items, depth)
            }
        }
    }

    fn emit_scalar(&mut self, scalar: &UnityYamlScalar) -> Result<(), UnityYamlWriteError> {
        match scalar {
            UnityYamlScalar::Null => self.write_str("null"),
            UnityYamlScalar::Boolean(value) => self.write_str(if *value { "1" } else { "0" }),
            UnityYamlScalar::Signed(value) => self.write_str(&value.to_string()),
            UnityYamlScalar::Unsigned(value) => self.write_str(&value.to_string()),
            UnityYamlScalar::Float32(value) => self.write_str(&format_dotnet_f32(*value)),
            UnityYamlScalar::Float64(value) => self.write_str(&format_dotnet_f64(*value)),
            UnityYamlScalar::Float32Hex(value) => {
                let text = format!("0x{:08x}({})", value.to_bits(), format_dotnet_f32(*value));
                self.write_str(&text)
            }
            UnityYamlScalar::Float64Hex(value) => {
                let text = format!("0x{:016x}({})", value.to_bits(), format_dotnet_f64(*value));
                self.write_str(&text)
            }
            UnityYamlScalar::String(value) => self.emit_string(value, false),
            UnityYamlScalar::PlainString(value) => self.emit_string(value, true),
        }
    }

    fn emit_string(&mut self, value: &str, force_plain: bool) -> Result<(), UnityYamlWriteError> {
        if force_plain {
            return self.write_key_or_string(value);
        }
        if value.chars().any(char::is_control) {
            self.write_char('"')?;
            for ch in value.chars() {
                match ch {
                    '\\' => self.write_str("\\\\")?,
                    '\n' => self.write_str("\\n")?,
                    '\r' => self.write_str("\\r")?,
                    '\t' => self.write_str("\\t")?,
                    '"' => self.write_str("\\\"")?,
                    control if control.is_control() => {
                        self.write_str(&format!("\\u{:04X}", control as u32))?
                    }
                    ch => self.write_char(ch)?,
                }
            }
            return self.write_char('"');
        }
        if !string_needs_quotes(value) {
            return self.write_key_or_string(value);
        }
        self.write_char('\'')?;
        for ch in value.chars() {
            self.write_char(ch)?;
            if ch == '\'' {
                self.write_char('\'')?;
            }
        }
        self.write_char('\'')
    }

    fn write_key_or_string(&mut self, value: &str) -> Result<(), UnityYamlWriteError> {
        if self.format_keys && self.is_key && value.starts_with("m_") && value.len() > 2 {
            let mut formatted = value.to_owned();
            if let Some(ch) = formatted[2..].chars().next() {
                if ch.is_ascii_uppercase() {
                    formatted.replace_range(2..3, &ch.to_ascii_lowercase().to_string());
                }
            }
            self.write_str(&formatted)
        } else {
            self.write_str(value)
        }
    }

    fn emit_mapping(
        &mut self,
        style: UnityYamlMappingStyle,
        entries: &[(UnityYamlNode, UnityYamlNode)],
        depth: usize,
    ) -> Result<(), UnityYamlWriteError> {
        if style == UnityYamlMappingStyle::Flow || entries.is_empty() {
            self.write_char('{')?;
        }
        for (key, value) in entries {
            if key.node_type() != UnityYamlNodeType::Scalar {
                return Err(UnityYamlWriteError::InvalidMappingKey);
            }
            let previous_key_state = self.is_key;
            self.is_key = true;
            self.emit_node(key, depth + 1)?;
            self.is_key = false;
            self.write_char(':')?;
            self.write_whitespace();
            if style == UnityYamlMappingStyle::Block && value.is_multiline() {
                self.write_line();
            }
            if value.is_indent() {
                self.increase_indent()?;
            }
            self.emit_node(value, depth + 1)?;
            if style == UnityYamlMappingStyle::Block {
                self.write_line();
            } else {
                self.write_separator();
                self.write_whitespace();
            }
            if value.is_indent() {
                self.decrease_indent();
            }
            self.is_key = previous_key_state;
        }
        match style {
            UnityYamlMappingStyle::Block if entries.is_empty() => self.write_char('}')?,
            UnityYamlMappingStyle::Block => {}
            UnityYamlMappingStyle::Flow => self.write_close_char('}')?,
        }
        if style == UnityYamlMappingStyle::Block {
            self.write_line();
        }
        Ok(())
    }

    fn emit_sequence(
        &mut self,
        style: UnityYamlSequenceStyle,
        items: &[UnityYamlNode],
        depth: usize,
    ) -> Result<(), UnityYamlWriteError> {
        match style {
            UnityYamlSequenceStyle::Block if items.is_empty() => self.write_char('[')?,
            UnityYamlSequenceStyle::BlockCurve if items.is_empty() => self.write_char('{')?,
            UnityYamlSequenceStyle::Flow => self.write_char('[')?,
            _ => {}
        }
        for item in items {
            if style != UnityYamlSequenceStyle::Flow {
                self.write_char('-')?;
                self.write_whitespace();
                if item.node_type() == UnityYamlNodeType::Sequence {
                    self.increase_indent()?;
                }
            }
            if item.is_indent() {
                self.increase_indent()?;
            }
            self.emit_node(item, depth + 1)?;
            if style != UnityYamlSequenceStyle::Flow {
                self.write_line();
                if item.node_type() == UnityYamlNodeType::Sequence {
                    self.decrease_indent();
                }
            } else {
                self.write_separator();
                self.write_whitespace();
            }
            if item.is_indent() {
                self.decrease_indent();
            }
        }
        match style {
            UnityYamlSequenceStyle::Block if items.is_empty() => self.write_char(']')?,
            UnityYamlSequenceStyle::BlockCurve if items.is_empty() => self.write_close_char('}')?,
            UnityYamlSequenceStyle::Flow => self.write_close_char(']')?,
            _ => {}
        }
        if style != UnityYamlSequenceStyle::Flow {
            self.write_line();
        }
        Ok(())
    }

    fn increase_indent(&mut self) -> Result<(), UnityYamlWriteError> {
        let requested = self.indent + 1;
        if requested.saturating_mul(2) > 1000 {
            return Err(UnityYamlWriteError::IndentLimitExceeded {
                requested: requested.saturating_mul(2),
                limit: 1000,
            });
        }
        self.indent = requested;
        Ok(())
    }

    fn decrease_indent(&mut self) {
        self.indent = self.indent.saturating_sub(1);
    }

    fn write_str(&mut self, value: &str) -> Result<(), UnityYamlWriteError> {
        // The baseline's `Emitter.Write(ReadOnlySpan<char>)` skips `WriteDelayed` for an empty
        // span, so a pending whitespace or newline before an empty plain string is dropped
        // rather than flushed. This is observable as `k:\n` (no trailing space) for an empty
        // value.
        if value.is_empty() {
            return Ok(());
        }
        self.write_delayed()?;
        self.append(value)
    }

    fn write_char(&mut self, value: char) -> Result<(), UnityYamlWriteError> {
        self.write_delayed()?;
        let needed = value.len_utf8();
        self.reserve_output(needed)?;
        self.output.push(value);
        Ok(())
    }

    fn write_close_char(&mut self, value: char) -> Result<(), UnityYamlWriteError> {
        self.pending_separator = false;
        self.pending_whitespace = false;
        self.pending_line_break = false;
        self.write_char(value)
    }

    fn write_whitespace(&mut self) {
        self.pending_whitespace = true;
    }

    fn write_separator(&mut self) {
        self.pending_separator = true;
    }

    fn write_line(&mut self) {
        self.pending_line_break = true;
    }

    fn write_delayed(&mut self) -> Result<(), UnityYamlWriteError> {
        if self.pending_line_break {
            self.append("\n")?;
            self.pending_separator = false;
            self.pending_whitespace = false;
            self.pending_line_break = false;
            let spaces = self.indent.saturating_mul(2);
            if spaces > 1000 {
                return Err(UnityYamlWriteError::IndentLimitExceeded {
                    requested: spaces,
                    limit: 1000,
                });
            }
            for _ in 0..spaces {
                self.append(" ")?;
            }
        }
        if self.pending_separator {
            self.append(",")?;
            self.pending_separator = false;
        }
        if self.pending_whitespace {
            self.append(" ")?;
            self.pending_whitespace = false;
        }
        Ok(())
    }

    fn raw_str(&mut self, value: &str) -> Result<(), UnityYamlWriteError> {
        self.append(value)
    }

    fn append(&mut self, value: &str) -> Result<(), UnityYamlWriteError> {
        self.reserve_output(value.len())?;
        self.output.push_str(value);
        Ok(())
    }

    fn reserve_output(&mut self, additional: usize) -> Result<(), UnityYamlWriteError> {
        let requested = self.output.len().checked_add(additional).ok_or(
            UnityYamlWriteError::OutputLimitExceeded {
                requested: usize::MAX,
                limit: self.limits.max_output_bytes,
            },
        )?;
        if requested > self.limits.max_output_bytes {
            return Err(UnityYamlWriteError::OutputLimitExceeded {
                requested,
                limit: self.limits.max_output_bytes,
            });
        }
        self.output
            .try_reserve(additional)
            .map_err(|_| UnityYamlWriteError::AllocationFailed { requested })
    }
}

/// Format an `f32` exactly like .NET's `Single.ToString(CultureInfo.InvariantCulture)`.
///
/// `NumericNode<float>` writes through `TextWriter.Write(float)` under the invariant culture
/// used by the baseline's `InvariantStreamWriter`, so Unity YAML floats follow .NET's default
/// shortest-round-trip "G" formatting rather than Rust's `Display`. The notation threshold and
/// special-value spellings were measured against the reference runtime; see the module docs and
/// [`tests::float_formatting_matches_the_reference_runtime`].
pub fn format_dotnet_f32(value: f32) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-Infinity" } else { "Infinity" }.to_owned();
    }
    let negative = value.is_sign_negative();
    if value == 0.0 {
        return if negative {
            "-0".to_owned()
        } else {
            "0".to_owned()
        };
    }
    format_dotnet_shortest(
        &round_shortest_tie_to_even_f32(&format!("{value:e}"), value),
        negative,
        9,
    )
}

/// Resolve shortest-representation ties the way .NET's `Single.ToString` does.
///
/// Rust's `LowerExp` already yields *a* shortest round-tripping decimal, but when several
/// same-length decimals round-trip to `value` it may pick a different one than .NET, which
/// resolves the tie toward the even last digit (observed: `-280.890625f32` prints
/// `-280.89062` under .NET and `-280.89063` under Rust, while both spellings parse to
/// bits `0xC38C7200`). This only adjusts the digit string when a same-length neighbor
/// parses back to the identical bits; otherwise the input is returned unchanged.
///
/// The winner among the round-tripping neighbors is the closest to the exact value with
/// exact ties broken toward even. Closeness is decided by comparing `2*value` against
/// the neighbor sum as exact terminating decimals (binary fractions always terminate),
/// so no floating-point rounding noise can flip an exact tie. The full-workspace
/// real-sample replay differentially guards the outcome.
fn round_shortest_tie_to_even_f32(sci: &str, value: f32) -> String {
    let (mantissa, exponent_text) = match sci.split_once('e') {
        Some(parts) => parts,
        None => return sci.to_owned(),
    };
    let negative = mantissa.starts_with('-');
    let digits: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).collect();
    if digits.is_empty() {
        return sci.to_owned();
    }
    let bits = value.to_bits();
    if !sci_parses_to_f32_bits(negative, &digits, exponent_text, bits) {
        return sci.to_owned();
    }
    // Same-length decimals that round-trip form a contiguous run around `value`; walk it
    // in both directions. The cap is far above any physically possible run length for
    // binary32 shortest forms (the rounding interval spans only a few decimal ulps).
    let mut candidates: Vec<Vec<u8>> = vec![digits.clone()];
    for delta in [-1i32, 1i32] {
        let mut current = digits.clone();
        for _ in 0..8 {
            let Some(next) = step_decimal_digits(&current, delta) else {
                break;
            };
            if next.len() != digits.len() || next[0] == b'0' {
                // Length or magnitude rollover: not the same shortest form.
                break;
            }
            if !sci_parses_to_f32_bits(negative, &next, exponent_text, bits) {
                break;
            }
            current = next.clone();
            candidates.push(next);
        }
    }
    if candidates.len() < 2 {
        return sci.to_owned();
    }
    // All candidates share the digit length and the exponent, so closeness to `value`
    // is decided exactly: `2*value` below/above/equal to the pairwise digit sum means
    // the smaller/larger/either neighbor wins. Exact ties resolve toward even.
    let doubled = exact_doubled_decimal_expansion_f32(bits);
    let Some(scale) = decimal_last_place_scale(exponent_text, digits.len()) else {
        return sci.to_owned();
    };
    let mut best = 0usize;
    for index in 1..candidates.len() {
        let sum = digit_string_sum(&candidates[best], &candidates[index]);
        match cmp_doubled_value_against_sum(&doubled, &sum, scale) {
            std::cmp::Ordering::Less => {
                if candidates[index] < candidates[best] {
                    best = index;
                }
            }
            std::cmp::Ordering::Greater => {
                if candidates[index] > candidates[best] {
                    best = index;
                }
            }
            std::cmp::Ordering::Equal => {
                let best_even =
                    (candidates[best][candidates[best].len() - 1] - b'0').is_multiple_of(2);
                let challenger_even =
                    (candidates[index][candidates[index].len() - 1] - b'0').is_multiple_of(2);
                if challenger_even && !best_even {
                    best = index;
                }
            }
        }
    }
    if best == 0 {
        return sci.to_owned();
    }
    rebuild_sci_string(negative, &candidates[best], exponent_text)
}

/// Decimal scale (`10^scale`) of the shared last digit place for a shortest-form digit
/// string with the given `LowerExp` exponent text. Returns `None` for malformed exponents.
fn decimal_last_place_scale(exponent_text: &str, digit_len: usize) -> Option<i32> {
    let exponent: i32 = exponent_text.parse().ok()?;
    let len = i32::try_from(digit_len).ok()?;
    exponent.checked_sub(len)?.checked_add(1)
}

/// Digit-string sum of two same-length decimal digit strings.
fn digit_string_sum(left: &[u8], right: &[u8]) -> Vec<u8> {
    debug_assert_eq!(left.len(), right.len());
    let mut sum = Vec::with_capacity(left.len() + 1);
    let mut carry = 0u8;
    for (left_digit, right_digit) in left.iter().rev().zip(right.iter().rev()) {
        let total = (left_digit - b'0') + (right_digit - b'0') + carry;
        sum.push(b'0' + total % 10);
        carry = total / 10;
    }
    if carry != 0 {
        sum.push(b'0' + carry);
    }
    sum.reverse();
    sum
}

/// Exact decimal expansion of `2*value` for a nonzero finite `f32` magnitude, as
/// `(integer_digits, frac_digits)`. Binary fractions terminate in decimal, so the
/// expansion is complete; doubling/halving digit strings is exact and bounded
/// (at most ~150 steps for binary32 extremes).
fn exact_doubled_decimal_expansion_f32(bits: u32) -> (Vec<u8>, Vec<u8>) {
    let exponent_field = bits >> 23 & 0xFF;
    let fraction_field = bits & 0x7F_FFFF;
    let (mantissa, shift) = if exponent_field == 0 {
        (fraction_field, -148i32)
    } else {
        // `2*value = mantissa * 2^(exponent_field - 149)`.
        (
            fraction_field | 0x80_0000,
            i32::try_from(exponent_field).unwrap_or(0) - 149,
        )
    };
    // Decimal digits of the mantissa, then `2*value = mantissa * 2^shift`.
    let mut parts = DecimalParts {
        integer: mantissa.to_string().into_bytes(),
        frac: Vec::new(),
    };
    if shift >= 0 {
        for _ in 0..shift {
            parts.double();
        }
    } else {
        for _ in 0..(-shift) {
            parts.halve();
        }
    }
    parts.trim();
    (parts.integer, parts.frac)
}

/// Exact terminating decimal split into integer and frac digit strings.
struct DecimalParts {
    integer: Vec<u8>,
    frac: Vec<u8>,
}

impl DecimalParts {
    fn double(&mut self) {
        let mut carry = 0u8;
        for digit in self
            .frac
            .iter_mut()
            .rev()
            .chain(self.integer.iter_mut().rev())
        {
            let total = (*digit - b'0') * 2 + carry;
            *digit = b'0' + total % 10;
            carry = total / 10;
        }
        if carry != 0 {
            self.integer.insert(0, b'0' + carry);
        }
    }

    fn halve(&mut self) {
        let mut remainder = 0u8;
        for digit in self.integer.iter_mut().chain(self.frac.iter_mut()) {
            let total = remainder * 10 + (*digit - b'0');
            *digit = b'0' + total / 2;
            remainder = total % 2;
        }
        if remainder != 0 {
            self.frac.push(b'5');
        }
    }

    fn trim(&mut self) {
        while self.integer.len() > 1 && self.integer[0] == b'0' {
            self.integer.remove(0);
        }
        while self.frac.last() == Some(&b'0') {
            self.frac.pop();
        }
    }
}

/// Compare exact `2*value` (as an expansion from
/// [`exact_doubled_decimal_expansion_f32`]) against a digit-string sum at decimal
/// scale `10^scale`. Both sides terminate, so padding the shorter frac part with
/// zeros is exact.
fn cmp_doubled_value_against_sum(
    doubled: &(Vec<u8>, Vec<u8>),
    sum_digits: &[u8],
    scale: i32,
) -> std::cmp::Ordering {
    let (sum_integer, sum_frac) = split_scaled_digits(sum_digits, scale);
    let (value_integer, value_frac) = (&doubled.0, &doubled.1);
    let stripped_value: &[u8] = trim_leading_zeros(value_integer);
    let stripped_sum: &[u8] = trim_leading_zeros(&sum_integer);
    if stripped_value.len() != stripped_sum.len() {
        return stripped_value.len().cmp(&stripped_sum.len());
    }
    match stripped_value.cmp(stripped_sum) {
        std::cmp::Ordering::Equal => {}
        ordering => return ordering,
    }
    let width = value_frac.len().max(sum_frac.len());
    for index in 0..width {
        let left = value_frac.get(index).copied().unwrap_or(b'0');
        let right = sum_frac.get(index).copied().unwrap_or(b'0');
        match left.cmp(&right) {
            std::cmp::Ordering::Equal => {}
            ordering => return ordering,
        }
    }
    std::cmp::Ordering::Equal
}

fn trim_leading_zeros(digits: &[u8]) -> &[u8] {
    let mut start = 0;
    while start + 1 < digits.len() && digits[start] == b'0' {
        start += 1;
    }
    &digits[start..]
}

/// Place a digit string at decimal scale `10^scale`, splitting into
/// `(integer_digits, frac_digits)`.
fn split_scaled_digits(digits: &[u8], scale: i32) -> (Vec<u8>, Vec<u8>) {
    let mut trimmed: &[u8] = digits;
    while trimmed.len() > 1 && trimmed[0] == b'0' {
        trimmed = &trimmed[1..];
    }
    if scale >= 0 {
        let zeros = usize::try_from(scale).unwrap_or(usize::MAX).min(1024);
        let mut integer = Vec::with_capacity(trimmed.len() + zeros);
        integer.extend_from_slice(trimmed);
        integer.extend(std::iter::repeat_n(b'0', zeros));
        (integer, Vec::new())
    } else {
        let point = usize::try_from(-scale).unwrap_or(usize::MAX).min(2048);
        if trimmed.len() > point {
            let split = trimmed.len() - point;
            (trimmed[..split].to_vec(), trimmed[split..].to_vec())
        } else {
            let mut frac = Vec::with_capacity(point);
            frac.extend(std::iter::repeat_n(b'0', point - trimmed.len()));
            frac.extend_from_slice(trimmed);
            (vec![b'0'], frac)
        }
    }
}

/// Steps a decimal digit string by `delta` (±1) with borrow/carry propagation.
/// Returns `None` when the step overflows past the most significant digit.
fn step_decimal_digits(digits: &[u8], delta: i32) -> Option<Vec<u8>> {
    let mut next = digits.to_vec();
    let mut carry = delta;
    let mut index = next.len();
    while index > 0 && carry != 0 {
        index -= 1;
        let digit = next[index] as i32 - b'0' as i32 + carry;
        if digit < 0 {
            next[index] = b'9';
            carry = -1;
        } else if digit > 9 {
            next[index] = b'0';
            carry = 1;
        } else {
            next[index] = b'0' + digit as u8;
            carry = 0;
        }
    }
    if carry != 0 {
        return None;
    }
    Some(next)
}

/// Rebuilds a `[-]d[.ddd]e{exponent}` string from a digit string after tie resolution.
fn rebuild_sci_string(negative: bool, digits: &[u8], exponent_text: &str) -> String {
    let mut text = String::with_capacity(digits.len() + exponent_text.len() + 3);
    if negative {
        text.push('-');
    }
    text.push(digits[0] as char);
    if digits.len() > 1 {
        text.push('.');
        text.extend(digits[1..].iter().map(|digit| *digit as char));
    }
    text.push('e');
    text.push_str(exponent_text);
    text
}

/// Checks that a digit string with the given exponent parses back to `bits` as `f32`.
fn sci_parses_to_f32_bits(negative: bool, digits: &[u8], exponent_text: &str, bits: u32) -> bool {
    rebuild_sci_string(negative, digits, exponent_text)
        .parse::<f32>()
        .is_ok_and(|parsed| parsed.to_bits() == bits)
}

/// Format an `f64` exactly like .NET's `Double.ToString(CultureInfo.InvariantCulture)`.
pub fn format_dotnet_f64(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-Infinity" } else { "Infinity" }.to_owned();
    }
    let negative = value.is_sign_negative();
    if value == 0.0 {
        return if negative {
            "-0".to_owned()
        } else {
            "0".to_owned()
        };
    }
    format_dotnet_shortest(&format!("{value:e}"), negative, 17)
}

/// Reformat Rust's shortest-round-trip scientific form to .NET's fixed/scientific choice.
///
/// `sci` is the output of Rust's `LowerExp` for a finite nonzero value (for example `1.5e-7`).
/// `threshold` is the largest positive power-of-ten exponent rendered in fixed notation: 9 for
/// `Single`, 17 for `Double`. .NET uses scientific notation when the exponent is below -4 or at
/// least `threshold`, always signs the exponent, and zero-pads it to two digits.
fn format_dotnet_shortest(sci: &str, negative: bool, threshold: i32) -> String {
    let (mantissa, exponent_text) = sci
        .split_once('e')
        .expect("Rust LowerExp always contains an exponent separator");
    let exponent: i32 = exponent_text
        .parse()
        .expect("Rust LowerExp exponent is an integer");
    let digits: String = mantissa.chars().filter(|c| c.is_ascii_digit()).collect();

    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if exponent < -4 || exponent >= threshold {
        out.push_str(&digits[..1]);
        if digits.len() > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('E');
        out.push(if exponent < 0 { '-' } else { '+' });
        let magnitude = exponent.unsigned_abs();
        if magnitude < 10 {
            out.push('0');
        }
        out.push_str(&magnitude.to_string());
    } else if exponent >= 0 {
        let point = (exponent + 1) as usize;
        if digits.len() <= point {
            out.push_str(&digits);
            for _ in 0..(point - digits.len()) {
                out.push('0');
            }
        } else {
            out.push_str(&digits[..point]);
            out.push('.');
            out.push_str(&digits[point..]);
        }
    } else {
        out.push_str("0.");
        for _ in 0..(-exponent - 1) {
            out.push('0');
        }
        out.push_str(&digits);
    }
    out
}

fn string_needs_quotes(value: &str) -> bool {
    let Some(first) = value.chars().next() else {
        return false;
    };
    if first.is_whitespace()
        || (value.starts_with('-') && value[1..].chars().next().is_some_and(char::is_whitespace))
        || value == "-"
        || ":[]\"'*&!@#%{}?<>,`".contains(first)
        || value.ends_with(':')
        || value.chars().last().is_some_and(char::is_whitespace)
        || value.contains(['\n', '\r'])
    {
        return true;
    }
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if (ch == ':' || ch == '@') && chars.peek().is_some_and(|next| next.is_whitespace()) {
            return true;
        }
    }
    false
}

/// Format UnityGuid's four serialized little-endian words as the baseline's 32-character string.
pub fn format_unity_guid_words(bytes: [u8; 16]) -> String {
    format_unity_guid(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mapping(entries: &[(&str, UnityYamlNode)]) -> UnityYamlNode {
        let mut map = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        for (key, value) in entries {
            map.insert_field(*key, value.clone()).unwrap();
        }
        map
    }

    fn version(value: &str) -> SceneUnityVersion {
        SceneUnityVersion::parse(value).unwrap()
    }

    /// Differential against the external `UnityGuid.ToString` (probe `Temp/opencode/guidprobe2`).
    /// The reference emits each 32-bit word's nibbles from least significant to most significant,
    /// so a word tuple's string is the per-word reversal of its big-endian hex.
    #[test]
    fn unity_guid_string_matches_the_reference_runtime() {
        let fixture = include_str!("guid_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            let mut bytes = [0u8; 16];
            match parts[0] {
                "GUID" => {
                    for (index, word) in parts[1..5].iter().enumerate() {
                        bytes[index * 4..index * 4 + 4]
                            .copy_from_slice(&word.parse::<u32>().unwrap().to_le_bytes());
                    }
                }
                "SPAN" => {
                    for (index, chunk) in parts[1].as_bytes().chunks_exact(2).enumerate() {
                        let text = std::str::from_utf8(chunk).unwrap();
                        bytes[index] = u8::from_str_radix(text, 16).unwrap();
                    }
                }
                other => panic!("unknown guid fixture tag {other} in {line}"),
            }
            assert_eq!(format_unity_guid(bytes), parts[parts.len() - 1], "{line}");
            rows += 1;
        }
        assert_eq!(rows, 4);
        // The canonical `unity default resources` GUID.
        let mut engine_bytes = [0u8; 16];
        engine_bytes[8..12].copy_from_slice(&0x0Eu32.to_le_bytes());
        assert_eq!(
            format_unity_guid(engine_bytes),
            "0000000000000000e000000000000000"
        );
    }

    /// Differential against the external `HashHelper.ToString` (probe `Temp/opencode/hashprobe2`).
    /// A word whose most significant byte has the high bit set overflows the baseline's checked
    /// arithmetic, so the formatter returns an error for it.
    #[test]
    fn hash128_string_matches_the_reference_runtime() {
        let fixture = include_str!("hash128_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "H", "{line}");
            let mut bytes = [0u8; 16];
            for (index, chunk) in parts[1].as_bytes().chunks_exact(2).enumerate() {
                let text = std::str::from_utf8(chunk).unwrap();
                bytes[index] = u8::from_str_radix(text, 16).unwrap();
            }
            let result = format_hash128(bytes);
            if parts[2] == "OverflowException" {
                assert_eq!(result, Err(Hash128Error::Overflow), "{line}");
            } else {
                assert_eq!(result.unwrap(), parts[2], "{line}");
            }
            rows += 1;
        }
        assert_eq!(rows, 7);
    }

    #[test]
    fn plain_string_edges_match_the_source_predicate_exactly() {
        // Arm-for-arm with the frozen `IllegalStringsRegex` plus its
        // null-or-empty-plain rule. The shared holes (empty, numeric-looking,
        // bool-looking, and ` #` strings stay plain in both) are pinned as
        // parity, not endorsed as ideal YAML.
        for value in [
            "", "123", "1.5", "true", "null", "~", "0x10", "a # b", ".inf", "yes", "a'b",
        ] {
            assert!(!string_needs_quotes(value), "{value:?} stays plain");
        }
        for value in [
            " leading",
            "trailing ",
            "-",
            "- x",
            ":lead",
            "a:",
            "a: b",
            "@ b",
            "a@ b",
            "#c",
            "'lead",
        ] {
            assert!(string_needs_quotes(value), "{value:?} needs quotes");
        }
        // An empty string emits bare with no trailing space (the frozen
        // `Write(ReadOnlySpan)` skips empty values before the delayed space),
        // and the key takes the baseline `m_` formatting. It reads back as
        // null in both.
        let body = mapping(&[("m_Name", UnityYamlNode::string(""))]);
        assert_eq!(
            write_unity_yaml_mapping(&body, UnityYamlWriteLimits::default()).unwrap(),
            "m_name:\n"
        );
    }

    #[test]
    fn writes_baseline_object_document_header_block_fields_and_quoted_strings() {
        let body = mapping(&[
            (
                "serializedVersion",
                UnityYamlNode::scalar(UnityYamlScalar::Signed(6)),
            ),
            ("m_Name", UnityYamlNode::string("name: test")),
            (
                "m_Enabled",
                UnityYamlNode::scalar(UnityYamlScalar::Boolean(true)),
            ),
        ]);
        let document = UnityYamlDocument::object(1, 100001, "GameObject", body, false).unwrap();

        assert_eq!(
            write_unity_yaml_documents(&[document], UnityYamlWriteLimits::default()).unwrap(),
            "%YAML 1.1\n%TAG !u! tag:unity3d.com,2011:\n--- !u!1 &100001\nGameObject:\n  serializedVersion: 6\n  m_Name: 'name: test'\n  m_Enabled: 1\n"
        );
    }

    #[test]
    fn writes_standalone_mapping_without_unity_stream_header() {
        let root = mapping(&[(
            "m_AlphaIsTransparency",
            UnityYamlNode::scalar(UnityYamlScalar::Boolean(true)),
        )]);
        assert_eq!(
            write_unity_yaml_mapping(&root, UnityYamlWriteLimits::default()).unwrap(),
            "m_alphaIsTransparency: 1\n"
        );
        assert_eq!(
            write_unity_yaml_mapping(
                &UnityYamlNode::scalar(UnityYamlScalar::Signed(1)),
                UnityYamlWriteLimits::default(),
            ),
            Err(UnityYamlWriteError::RootMustBeMapping)
        );
    }

    #[test]
    fn writes_multiple_documents_and_flow_sequences_with_baseline_spacing() {
        let mut pointers = UnityYamlNode::sequence(UnityYamlSequenceStyle::Flow);
        pointers
            .push(UnityYamlNode::scalar(UnityYamlScalar::Signed(1)))
            .unwrap();
        pointers
            .push(UnityYamlNode::scalar(UnityYamlScalar::Signed(2)))
            .unwrap();
        let first =
            UnityYamlDocument::object(1, 11, "First", mapping(&[("items", pointers)]), false)
                .unwrap();
        let second = UnityYamlDocument::object(
            4,
            22,
            "Second",
            UnityYamlNode::mapping(UnityYamlMappingStyle::Block),
            true,
        )
        .unwrap();

        assert_eq!(
            write_unity_yaml_documents(&[first, second], UnityYamlWriteLimits::default()).unwrap(),
            "%YAML 1.1\n%TAG !u! tag:unity3d.com,2011:\n--- !u!1 &11\nFirst:\n  items: [1, 2]\n--- !u!4 &22 stripped\nSecond: {}\n"
        );
    }

    #[test]
    fn writes_block_sequences_of_mappings_with_baseline_indentation() {
        let mut pointer = UnityYamlNode::mapping(UnityYamlMappingStyle::Flow);
        pointer
            .insert_field("fileID", UnityYamlNode::scalar(UnityYamlScalar::Signed(45)))
            .unwrap();
        let mut component = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        component.insert_field("component", pointer).unwrap();
        let mut components = UnityYamlNode::sequence(UnityYamlSequenceStyle::Block);
        components.push(component).unwrap();
        let document = UnityYamlDocument::object(
            1,
            10,
            "GameObject",
            mapping(&[("m_Component", components)]),
            false,
        )
        .unwrap();

        assert_eq!(
            write_unity_yaml_documents(&[document], UnityYamlWriteLimits::default()).unwrap(),
            "%YAML 1.1\n%TAG !u! tag:unity3d.com,2011:\n--- !u!1 &10\nGameObject:\n  m_Component:\n  - component: {fileID: 45}\n"
        );
    }

    #[test]
    fn meta_pointer_yaml_matches_unity_flow_mapping_and_unity_three_meta_type() {
        let pointer = UnityMetaPointer {
            file_id: 42,
            guid: ExportPointerGuid::Collection([
                0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
                0x0f, 0x10,
            ]),
            asset_type: UnityProjectAssetType::Meta,
        };
        let node = meta_pointer_to_unity_yaml(pointer, version("3.5.7f1"), None).unwrap();
        let document =
            UnityYamlDocument::object(1, 1, "Pointer", mapping(&[("m_Ptr", node)]), false).unwrap();

        assert_eq!(
            write_unity_yaml_documents(&[document], UnityYamlWriteLimits::default()).unwrap(),
            "%YAML 1.1\n%TAG !u! tag:unity3d.com,2011:\n--- !u!1 &1\nPointer:\n  m_Ptr: {fileID: 42, guid: 102030405060708090a0b0c0d0e0f001, type: 1}\n"
        );
    }

    #[test]
    fn pointer_writer_requires_real_missing_reference_guid_and_omits_null_guid_fields() {
        let missing = UnityMetaPointer {
            file_id: 4_200_000,
            guid: ExportPointerGuid::MissingReference,
            asset_type: UnityProjectAssetType::Meta,
        };
        assert_eq!(
            meta_pointer_to_unity_yaml(missing, version("2022.3.1f1"), None),
            Err(UnityYamlWriteError::MissingReferenceGuidRequired)
        );
        let null_guid = UnityMetaPointer {
            file_id: 0,
            guid: ExportPointerGuid::None,
            asset_type: UnityProjectAssetType::Serialized,
        };
        let node = meta_pointer_to_unity_yaml(null_guid, version("2022.3.1f1"), None).unwrap();
        let document =
            UnityYamlDocument::object(1, 1, "Pointer", mapping(&[("m_Ptr", node)]), false).unwrap();
        assert!(
            write_unity_yaml_documents(&[document], UnityYamlWriteLimits::default())
                .unwrap()
                .contains("m_Ptr: {fileID: 0}")
        );

        let projected_null = project_pointer_to_unity_yaml(
            &ProjectPointerResolution::Null,
            version("2022.3.1f1"),
            None,
        )
        .unwrap();
        let document = UnityYamlDocument::object(
            1,
            1,
            "Pointer",
            mapping(&[("m_Ptr", projected_null)]),
            false,
        )
        .unwrap();
        assert!(
            write_unity_yaml_documents(&[document], UnityYamlWriteLimits::default())
                .unwrap()
                .contains("m_Ptr: {fileID: 0}")
        );

        let unresolved = UnresolvedProjectPointer {
            source_file_index: 2,
            source_object_index: 3,
            source_path_id: 77,
            field_path: "m_Material".to_owned(),
            serialized_file_id: 1,
            serialized_path_id: 88,
            expected_class_id: Some(21),
            resolution: crate::serialized_file::reference_graph::MultiFilePPtrResolution::MissingDependency {
                external_index: 0,
                path_id: 88,
            },
        };
        assert_eq!(
            project_pointer_to_unity_yaml(
                &ProjectPointerResolution::UnresolvedInput(unresolved.clone()),
                version("2022.3.1f1"),
                None,
            ),
            Err(UnityYamlWriteError::MissingReferenceGuidRequired)
        );
        // With the baseline MissingReference GUID, a missing external
        // dependency exports as a missing-reference pointer that keeps the
        // target's serialized PathID, matching the reference exporter.
        let missing_guid =
            crate::export::script_hashing::UnityGuid::MISSING_REFERENCE.to_byte_array();
        let node = project_pointer_to_unity_yaml(
            &ProjectPointerResolution::UnresolvedInput(unresolved),
            version("2022.3.1f1"),
            Some(missing_guid),
        )
        .unwrap();
        let document =
            UnityYamlDocument::object(1, 1, "Pointer", mapping(&[("m_Ptr", node)]), false).unwrap();
        assert!(
            write_unity_yaml_documents(&[document], UnityYamlWriteLimits::default())
                .unwrap()
                .contains("m_Ptr: {fileID: 88, guid: 0000000deadbeef15deadf00d0000000, type: 0}")
        );
    }

    #[test]
    fn yaml_writer_enforces_document_node_depth_and_output_limits() {
        let document = UnityYamlDocument::object(
            1,
            1,
            "Object",
            mapping(&[("value", UnityYamlNode::scalar(UnityYamlScalar::Signed(1)))]),
            false,
        )
        .unwrap();
        assert!(matches!(
            write_unity_yaml_documents(
                &[document.clone(), document.clone()],
                UnityYamlWriteLimits {
                    max_documents: 1,
                    ..UnityYamlWriteLimits::default()
                }
            ),
            Err(UnityYamlWriteError::DocumentLimitExceeded {
                requested: 2,
                limit: 1
            })
        ));
        assert!(matches!(
            write_unity_yaml_documents(
                std::slice::from_ref(&document),
                UnityYamlWriteLimits {
                    max_nodes: 1,
                    ..UnityYamlWriteLimits::default()
                }
            ),
            Err(UnityYamlWriteError::NodeLimitExceeded { .. })
        ));
        assert!(matches!(
            write_unity_yaml_documents(
                std::slice::from_ref(&document),
                UnityYamlWriteLimits {
                    max_depth: 1,
                    ..UnityYamlWriteLimits::default()
                }
            ),
            Err(UnityYamlWriteError::DepthLimitExceeded { .. })
        ));
        assert!(matches!(
            write_unity_yaml_documents(
                &[document],
                UnityYamlWriteLimits {
                    max_output_bytes: 8,
                    ..UnityYamlWriteLimits::default()
                }
            ),
            Err(UnityYamlWriteError::OutputLimitExceeded { .. })
        ));
    }

    /// Emit `{k: <value>}` as a standalone mapping, the way `ExportMeta` writes a sidecar.
    fn emit_scalar_document(value: &str) -> String {
        let mut map = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        map.insert_field("k", UnityYamlNode::string(value)).unwrap();
        write_unity_yaml_mapping(&map, UnityYamlWriteLimits::default()).unwrap()
    }

    /// Byte-for-byte differential against the reference `AssetRipper.Yaml` runtime.
    ///
    /// Each expected document is the exact output of a probe that referenced the isolated
    /// `AssetRipper.Yaml.dll` build, created `YamlScalarNode.Create(value)`, and wrote it under
    /// a `k` key with `IsWriteDefaultTag = false`, `IsWriteVersion = false`,
    /// `IsFormatKeys = true`, and a `\n` newline (the same configuration as
    /// `write_unity_yaml_mapping`). The probe and its command are recorded in
    /// `docs/validation-log.md`. It caught the `-` + non-space whitespace divergence.
    #[test]
    fn scalar_quoting_matches_the_reference_yaml_runtime() {
        let cases: &[(&str, &str)] = &[
            ("", "k:\n"),
            ("abc", "k: abc\n"),
            (" hello", "k: ' hello'\n"),
            ("hello ", "k: 'hello '\n"),
            ("\thello", "k: \"\\thello\"\n"),
            ("- foo", "k: '- foo'\n"),
            ("-foo", "k: -foo\n"),
            ("-", "k: '-'\n"),
            ("-\u{A0}foo", "k: '-\u{A0}foo'\n"),
            ("-\u{2003}foo", "k: '-\u{2003}foo'\n"),
            ("-\u{200B}foo", "k: -\u{200B}foo\n"),
            (":foo", "k: ':foo'\n"),
            ("[foo", "k: '[foo'\n"),
            ("foo: bar", "k: 'foo: bar'\n"),
            ("foo:", "k: 'foo:'\n"),
            ("foo:\u{A0}bar", "k: 'foo:\u{A0}bar'\n"),
            ("foo@\tbar", "k: \"foo@\\tbar\"\n"),
            ("caf\u{E9}", "k: caf\u{E9}\n"),
            ("line\nbreak", "k: \"line\\nbreak\"\n"),
            ("quote'inside", "k: quote'inside\n"),
            ("double\"inside", "k: double\"inside\n"),
            ("back\\slash", "k: back\\slash\n"),
            ("m_Name", "k: m_Name\n"),
            ("a\u{2028}b", "k: a\u{2028}b\n"),
            ("\u{A0}foo", "k: '\u{A0}foo'\n"),
            ("foo\u{A0}", "k: 'foo\u{A0}'\n"),
            ("!foo", "k: '!foo'\n"),
            ("#foo", "k: '#foo'\n"),
            ("%foo", "k: '%foo'\n"),
            ("&foo", "k: '&foo'\n"),
            ("*foo", "k: '*foo'\n"),
            ("@foo", "k: '@foo'\n"),
            ("`foo", "k: '`foo'\n"),
            (">foo", "k: '>foo'\n"),
            ("<foo", "k: '<foo'\n"),
            (",foo", "k: ',foo'\n"),
            ("'foo", "k: '''foo'\n"),
            ("\"foo", "k: '\"foo'\n"),
            ("{foo", "k: '{foo'\n"),
            ("}foo", "k: '}foo'\n"),
            ("?foo", "k: '?foo'\n"),
            ("  ", "k: '  '\n"),
            ("a:b", "k: a:b\n"),
            ("a:\tb", "k: \"a:\\tb\"\n"),
            ("\u{85}foo", "k: \"\\u0085foo\"\n"),
            ("foo\rbar", "k: \"foo\\rbar\"\n"),
            ("\u{0}", "k: \"\\u0000\"\n"),
            ("tabs\there", "k: \"tabs\\there\"\n"),
        ];
        for (input, expected) in cases {
            assert_eq!(
                &emit_scalar_document(input),
                expected,
                "input {input:?} diverged from the reference runtime"
            );
        }
    }
    /// Byte-for-byte differential against the reference `AssetRipper.Yaml` runtime.
    ///
    /// The expected strings are `Single.ToString(CultureInfo.InvariantCulture)` and
    /// `Double.ToString(CultureInfo.InvariantCulture)` values captured by an isolated net10.0
    /// probe (`Temp/opencode/numericprobe`, recorded in `docs/validation-log.md`). The set mixes
    /// curated boundaries with deterministic pseudo-random finite bit patterns and found the
    /// Rust-versus-.NET divergence in the notation switch.
    #[test]
    fn float_formatting_matches_the_reference_runtime() {
        for (bits, expected) in F32_CASES {
            assert_eq!(
                format_dotnet_f32(f32::from_bits(*bits)),
                *expected,
                "f32 {bits:#010X}"
            );
        }
        for (bits, expected) in F64_CASES {
            assert_eq!(
                format_dotnet_f64(f64::from_bits(*bits)),
                *expected,
                "f64 {bits:#018X}"
            );
        }
    }

    /// Differential against `float.ToString(CultureInfo.InvariantCulture)` (probe
    /// `Temp/opencode/floatprobe`): the exact call behind `SerializedProperty`
    /// float rendering in shader dummy exports.
    #[test]
    fn float_tostring_matches_the_reference_runtime() {
        let fixture = include_str!("float_tostring_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.splitn(3, '|');
            assert_eq!(parts.next(), Some("FLOAT"), "{line}");
            let bits = u32::from_str_radix(parts.next().expect("a bits word"), 16).unwrap();
            let expected = parts.next().expect("a text word");
            assert!(parts.next().is_none(), "{line}");
            assert_eq!(
                format_dotnet_f32(f32::from_bits(bits)),
                expected,
                "f32 {bits:#010X}"
            );
            rows += 1;
        }
        assert_eq!(rows, 28, "all probed float rows are asserted");
    }

    /// Shortest-representation ties resolve toward the even last digit like .NET.
    ///
    /// Both spellings of each pair parse to identical `f32` bits (checked with a
    /// scratch parse: `280.89062` and `280.89063` are both bits `0x438C7200`;
    /// `477.89062` and `477.89063` are both `0x43EEF200`). The expected strings are
    /// the frozen reference's YAML spellings from 松风竹庭` class-4
    /// `m_LocalPosition.z` documents (local fileID 34284 decodes the exact midpoint
    /// `-280.890625`), while Rust's own formatter picks the odd neighbor.
    #[test]
    fn format_dotnet_f32_rounds_exact_midpoint_ties_to_even() {
        for (bits, expected) in [(0xC38C7200u32, "-280.89062"), (0xC3EEF200u32, "-477.89062")] {
            assert_eq!(
                format_dotnet_f32(f32::from_bits(bits)),
                expected,
                "f32 {bits:#010X}"
            );
        }
        // Non-tie values keep Rust's shortest choice.
        assert_eq!(format_dotnet_f32(51.55f32), "51.55");
        assert_eq!(format_dotnet_f32(-153.0f32), "-153");
    }

    /// Exact ties at negative powers of two resolve toward even through exact
    /// decimal comparison, not `f64` distance (whose rounding noise flipped these
    /// cases before). Bits `0x39800000`/`0xB9800000` are exactly `2^-12`; both
    /// `...462` and `...463` spellings parse back, and the frozen reference's
    /// `Face (merged).baked.asset` mesh vertices print the even neighbor.
    #[test]
    fn format_dotnet_f32_rounds_power_of_two_ties_to_even() {
        for (bits, expected) in [
            (0x39800000u32, "0.00024414062"),
            (0xB9800000u32, "-0.00024414062"),
        ] {
            assert_eq!(
                format_dotnet_f32(f32::from_bits(bits)),
                expected,
                "f32 {bits:#010X}"
            );
        }
    }

    /// `format_dotnet_f64` agrees with the .NET oracle on ordinary values
    /// (3,675-row sweep: powers of two, odd multiples, boundary decimals, and
    /// 3,000 pseudo-random bit patterns — only 4 E-notation subnormal-adjacent
    /// cases diverge, where .NET prints a shorter non-round-tripping string;
    /// no decoded F64 reaches real-sample YAML, so that quirk stays latent).
    /// Expectations below are oracle-pinned (`tools/probes/tostring-oracle`).
    #[test]
    fn format_dotnet_f64_matches_oracle_on_ordinary_values() {
        for (bits, expected) in [
            (0x3FF8000000000000u64, "1.5"),
            (0x400921FB54442D18u64, "3.141592653589793"),
            (0x8000000000000000u64, "-0"),
            (0x0019D438AE9B4CFEu64, "3.591963138571574E-308"),
        ] {
            assert_eq!(
                format_dotnet_f64(f64::from_bits(bits)),
                expected,
                "f64 {bits:#018X}"
            );
        }
    }
    /// Byte-for-byte differential against the reference `AssetRipper.Yaml` runtime layout.
    ///
    /// Expected documents come from the isolated net10.0 probe `Temp/opencode/layoutprobe`
    /// (recorded in `docs/validation-log.md`), which built the same node trees through
    /// `YamlMappingNode`/`YamlSequenceNode` and emitted with `IsWriteDefaultTag = false`,
    /// `IsWriteVersion = false`, `IsFormatKeys = true`, and a `\n` newline.
    #[test]
    fn layout_matches_the_reference_yaml_runtime() {
        let cases: &[(&str, &str)] = &[
            ("nested_block", "a:\n  b: 1\n  c: x\nd: 1\n"),
            ("block_seq", "s:\n- -1\n- 2\n"),
            ("flow_seq", "s: [1, 2]\n"),
            ("blockcurve_seq", "s:\n- 1\n"),
            ("empty_block_seq", "s: []\n"),
            ("empty_flow_seq", "s: []\n"),
            ("empty_blockcurve_seq", "s: {}\n"),
            ("seq_of_maps", "s:\n- x: 1\n- y: 2\n"),
            ("flow_map", "f: {k: 1}\n"),
            ("empty_block_map", "e: {}\n"),
            ("anchored", "anchored: &a1 5\n"),
            ("tagged", "tagged: !u!myTag 5\n"),
            ("stripped", "stripped: stripped 5\n"),
            ("anchored_multiline_map", "a:\n  &m1\n  b: 1\n"),
            ("seq_nested_seq", "s:\n- [1]\n- 2\n"),
        ];
        for (name, expected) in cases {
            assert_eq!(
                &build_layout_case(name),
                expected,
                "layout case {name} diverged from the reference runtime"
            );
        }
    }

    fn render_layout(build: impl FnOnce(&mut UnityYamlNode)) -> String {
        let mut root = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        build(&mut root);
        write_unity_yaml_mapping(&root, UnityYamlWriteLimits::default()).unwrap()
    }

    fn signed(value: i64) -> UnityYamlNode {
        UnityYamlNode::scalar(UnityYamlScalar::Signed(value))
    }

    fn build_layout_case(name: &str) -> String {
        match name {
            "nested_block" => render_layout(|root| {
                let mut inner = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
                inner.insert_field("b", signed(1)).unwrap();
                inner.insert_field("c", UnityYamlNode::string("x")).unwrap();
                root.insert_field("a", inner).unwrap();
                root.insert_field("d", UnityYamlNode::scalar(UnityYamlScalar::Boolean(true)))
                    .unwrap();
            }),
            "block_seq" => render_layout(|root| {
                let mut seq = UnityYamlNode::sequence(UnityYamlSequenceStyle::Block);
                seq.push(signed(-1)).unwrap();
                seq.push(signed(2)).unwrap();
                root.insert_field("s", seq).unwrap();
            }),
            "flow_seq" => render_layout(|root| {
                let mut seq = UnityYamlNode::sequence(UnityYamlSequenceStyle::Flow);
                seq.push(signed(1)).unwrap();
                seq.push(signed(2)).unwrap();
                root.insert_field("s", seq).unwrap();
            }),
            "blockcurve_seq" => render_layout(|root| {
                let mut seq = UnityYamlNode::sequence(UnityYamlSequenceStyle::BlockCurve);
                seq.push(signed(1)).unwrap();
                root.insert_field("s", seq).unwrap();
            }),
            "empty_block_seq" => render_layout(|root| {
                root.insert_field("s", UnityYamlNode::sequence(UnityYamlSequenceStyle::Block))
                    .unwrap();
            }),
            "empty_flow_seq" => render_layout(|root| {
                root.insert_field("s", UnityYamlNode::sequence(UnityYamlSequenceStyle::Flow))
                    .unwrap();
            }),
            "empty_blockcurve_seq" => render_layout(|root| {
                root.insert_field(
                    "s",
                    UnityYamlNode::sequence(UnityYamlSequenceStyle::BlockCurve),
                )
                .unwrap();
            }),
            "seq_of_maps" => render_layout(|root| {
                let mut seq = UnityYamlNode::sequence(UnityYamlSequenceStyle::Block);
                let mut first = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
                first.insert_field("x", signed(1)).unwrap();
                let mut second = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
                second.insert_field("y", signed(2)).unwrap();
                seq.push(first).unwrap();
                seq.push(second).unwrap();
                root.insert_field("s", seq).unwrap();
            }),
            "flow_map" => render_layout(|root| {
                let mut inner = UnityYamlNode::mapping(UnityYamlMappingStyle::Flow);
                inner.insert_field("k", signed(1)).unwrap();
                root.insert_field("f", inner).unwrap();
            }),
            "empty_block_map" => render_layout(|root| {
                root.insert_field("e", UnityYamlNode::mapping(UnityYamlMappingStyle::Block))
                    .unwrap();
            }),
            "anchored" => render_layout(|root| {
                let mut node = signed(5);
                node.anchor = Some("a1".to_owned());
                root.insert_field("anchored", node).unwrap();
            }),
            "tagged" => render_layout(|root| {
                let mut node = signed(5);
                node.tag = Some("!u!myTag".to_owned());
                root.insert_field("tagged", node).unwrap();
            }),
            "stripped" => render_layout(|root| {
                let mut node = signed(5);
                node.stripped = true;
                root.insert_field("stripped", node).unwrap();
            }),
            "anchored_multiline_map" => render_layout(|root| {
                let mut inner = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
                inner.insert_field("b", signed(1)).unwrap();
                inner.anchor = Some("m1".to_owned());
                root.insert_field("a", inner).unwrap();
            }),
            "seq_nested_seq" => render_layout(|root| {
                let mut outer = UnityYamlNode::sequence(UnityYamlSequenceStyle::Block);
                let mut inner = UnityYamlNode::sequence(UnityYamlSequenceStyle::Flow);
                inner.push(signed(1)).unwrap();
                outer.push(inner).unwrap();
                outer.push(signed(2)).unwrap();
                root.insert_field("s", outer).unwrap();
            }),
            other => panic!("unknown layout case {other}"),
        }
    }

    const F32_CASES: &[(u32, &str)] = &[
        (0x00000000, "0"),
        (0x80000000, "-0"),
        (0x3F800000, "1"),
        (0xBF800000, "-1"),
        (0x3F000000, "0.5"),
        (0x3DCCCCCD, "0.1"),
        (0x3E4CCCCD, "0.2"),
        (0x3E99999A, "0.3"),
        (0x40200000, "2.5"),
        (0x40100000, "2.25"),
        (0x3EAAAAAB, "0.33333334"),
        (0x40490FD0, "3.14159"),
        (0x3727C5AC, "1E-05"),
        (0x38D1B717, "0.0001"),
        (0x3A83126F, "0.001"),
        (0x47C35000, "100000"),
        (0x49742400, "1000000"),
        (0x4B189680, "10000000"),
        (0x4CBEBC20, "100000000"),
        (0x58635FA9, "1E+15"),
        (0x5A0E1BCA, "1E+16"),
        (0x60AD78EC, "1E+20"),
        (0x1E3CE508, "1E-20"),
        (0x0DA24260, "1E-30"),
        (0x000116C2, "1E-40"),
        (0x7F7FFFFF, "3.4028235E+38"),
        (0xFF7FFFFF, "-3.4028235E+38"),
        (0x00000001, "1E-45"),
        (0xFFC00000, "NaN"),
        (0xFFC00000, "NaN"),
        (0x7F800000, "Infinity"),
        (0xFF800000, "-Infinity"),
        (0x47F12065, "123456.79"),
        (0x4996B438, "1234567"),
        (0x4B3C614E, "12345678"),
        (0x497423F0, "999999"),
        (0x49742400, "1000000"),
        (0x4B800000, "16777216"),
        (0x42C80000, "100"),
        (0x447A0000, "1000"),
        (0x390173F8, "0.000123456"),
        (0x66FF0AA8, "6.022E+23"),
        (0xC2F6E979, "-123.456"),
        (0x34210FB0, "1.5E-07"),
        (0x38D1B716, "9.999999E-05"),
        (0x3A83126E, "0.0009999999"),
        (0x4B18967F, "9999999"),
        (0x4CBEBC20, "100000000"),
        (0x4E6E6B28, "1E+09"),
        (0x4CEB79A3, "123456790"),
        (0x4E932C06, "1.234568E+09"),
        (0x0BF34DAD, "9.371707E-32"),
        (0x026E6076, "1.7513161E-37"),
        (0xE5906136, "-8.522675E+22"),
        (0x368DCC74, "4.225929E-06"),
        (0xA1C54AEC, "-1.3369071E-18"),
        (0x4E7BFB79, "1.05689043E+09"),
        (0xD6E8F2CF, "-1.2806478E+14"),
        (0x1AF470EA, "1.0109846E-22"),
        (0xB0D6E28B, "-1.5634937E-09"),
        (0x49428D8E, "796888.9"),
        (0xB8E8AB15, "-0.000110944886"),
        (0x56D71F03, "1.182641E+14"),
        (0xADA711FD, "-1.899369E-11"),
        (0xD63B809E, "-5.154027E+13"),
        (0x3F5A101F, "0.8518085"),
        (0xFC5D43FF, "-4.595507E+36"),
        (0x7DFAE6B8, "4.168812E+37"),
        (0x5585BD75, "1.8381095E+13"),
        (0x6B714B4F, "2.9170676E+26"),
        (0xC8AE9A19, "-357584.78"),
        (0x6D00616D, "2.4832414E+27"),
        (0x9A319AEF, "-3.6727896E-23"),
        (0x9C0AA21A, "-4.586988E-22"),
        (0x826571DE, "-1.6856941E-37"),
        (0xED7C0CBD, "-4.8753514E+27"),
        (0xC1FA7BE4, "-31.310493"),
        (0xCDBDF613, "-398377570"),
        (0x69E7693F, "3.496986E+25"),
        (0x6993082D, "2.2218838E+25"),
        (0x987D857D, "-3.2766878E-24"),
        (0x6CED8137, "2.297005E+27"),
        (0x3498F7F5, "2.849262E-07"),
        (0x78869B5A, "2.184121E+34"),
        (0x7F6200EC, "3.0041031E+38"),
        (0xD0DB7FED, "-2.9460752E+10"),
        (0xC0FC9252, "-7.8928614"),
        (0x73FFBFF6, "4.052518E+31"),
        (0xE71B7D09, "-7.342733E+23"),
        (0x26CD29B3, "1.4236035E-15"),
        (0x0CBCBF20, "2.9081016E-31"),
        (0x56D20E5E, "1.15479574E+14"),
        (0x48B3FDC2, "368622.06"),
        (0x9417AAB9, "-7.657221E-27"),
        (0xE0DA0BAC, "-1.2569473E+20"),
        (0x2F9AD4BB, "2.816359E-10"),
        (0x9A43AFD2, "-4.046708E-23"),
        (0xC47C9C0D, "-1010.4383"),
        (0xEDB8C675, "-7.148145E+27"),
        (0x41EE8AB9, "29.817736"),
        (0xA840D9EC, "-1.0705395E-14"),
        (0x1EA5A35F, "1.7537623E-20"),
        (0xE029DFD9, "-4.8962964E+19"),
        (0x38295A26, "4.0376697E-05"),
        (0x496F4112, "979985.1"),
        (0x2A2F9B10, "1.559692E-13"),
        (0x4EB30026, "1.5015657E+09"),
        (0xFDA6AFA6, "-2.7695483E+37"),
        (0x2AF0CB79, "4.2773752E-13"),
        (0x10A6D4AF, "6.5803146E-29"),
        (0x302B52C6, "6.2327066E-10"),
    ];
    const F64_CASES: &[(u64, &str)] = &[
        (0x0000000000000000, "0"),
        (0x8000000000000000, "-0"),
        (0x3FF0000000000000, "1"),
        (0xBFF0000000000000, "-1"),
        (0x3FE0000000000000, "0.5"),
        (0x3FB999999999999A, "0.1"),
        (0x3FD5555555555555, "0.3333333333333333"),
        (0x400921FB54442D18, "3.141592653589793"),
        (0x3EE4F8B588E368F1, "1E-05"),
        (0x3F1A36E2EB1C432D, "0.0001"),
        (0x3F50624DD2F1A9FC, "0.001"),
        (0x40F86A0000000000, "100000"),
        (0x430C6BF526340000, "1000000000000000"),
        (0x4341C37937E08000, "10000000000000000"),
        (0x4376345785D8A000, "1E+17"),
        (0x4415AF1D78B58C40, "1E+20"),
        (0x3BC79CA10C924223, "1E-20"),
        (0x01A56E1FC2F8F359, "1E-300"),
        (0x7FEFFFFFFFFFFFFF, "1.7976931348623157E+308"),
        (0xFFEFFFFFFFFFFFFF, "-1.7976931348623157E+308"),
        (0x0000000000000001, "5E-324"),
        (0xFFF8000000000000, "NaN"),
        (0xFFF8000000000000, "NaN"),
        (0x7FF0000000000000, "Infinity"),
        (0xFFF0000000000000, "-Infinity"),
        (0x41D26580B487E69B, "1234567890.12345"),
        (0x7FE1CCF385EBC8A0, "1E+308"),
        (0x0000000000000001, "5E-324"),
        (0x0010000000000000, "2.2250738585072014E-308"),
        (0x4341C37937E08000, "10000000000000000"),
        (0x4376345785D8A000, "1E+17"),
        (0x437B69B4BA630F35, "1.2345678901234568E+17"),
        (0xDC1B77AE0BF34DAD, "-4.9911105725155504E+135"),
        (0x64F0EEB9026E6076, "1.715373926431966E+178"),
        (0x7B07CE91E5906136, "4.4251604027143375E+284"),
        (0x305F050C368DCC74, "1.0715660391465826E-75"),
        (0x2CEB16E0A1C54AEC, "2.5973481493288907E-92"),
        (0x97101DCE4E7BFB79, "-1.3475090132806154E-197"),
        (0x9AD2E144D6E8F2CF, "-1.81996730402717E-179"),
        (0xD9AA792E1AF470EA, "-8.750186241947517E+123"),
        (0xDDAA4E85B0D6E28B, "-1.603964615428183E+143"),
        (0x8F8EA9D349428D8E, "-9.643915712060552E-234"),
        (0x08F474FFB8E8AB15, "1.5860846119992697E-265"),
        (0x2EAD854756D71F03, "7.597954524892738E-84"),
        (0x55BC79F8ADA711FD, "1.0204796945730538E+105"),
        (0x0E1FC49BD63B809E, "1.1910593176322935E-240"),
        (0xB92199E83F5A101F, "-1.694933913128696E-33"),
        (0xC5765079FC5D43FF, "-4.3162252145519944E+26"),
        (0x353CFC387DFAE6B8, "3.0261999441573203E-52"),
        (0xA32EDABF5585BD75, "-3.238696681491661E-139"),
        (0xFC5639B16B714B4F, "-8.663725837306241E+290"),
        (0x92FB2DCFC8AE9A19, "-3.0797526843217145E-217"),
        (0x544B0EC76D00616D, "1.15589923121874E+98"),
        (0xBCBB9B7E9A319AEF, "-3.831296467316889E-16"),
        (0x0F1A50D59C0AA21A, "6.466053826432699E-236"),
        (0x80AE2120826571DE, "-2.1452846540129615E-305"),
        (0x0E1ECD02ED7C0CBD, "1.1547975125429853E-240"),
        (0x0D0981E8C1FA7BE4, "7.296267179458751E-246"),
        (0xDC86B3D3CDBDF613, "-5.2803264785067434E+137"),
        (0x6D0844C269E7693F, "1.6732138965686944E+217"),
        (0x3681DA7F6993082D, "3.909076791246025E-46"),
        (0x5B928E2C987D857D, "1.3170664051174186E+133"),
        (0x6C716E1E6CED8137, "2.347135155778617E+214"),
        (0xCCB870213498F7F5, "-3.92703626520675E+61"),
        (0x9F07B27A78869B5A, "-3.3710794864173703E-159"),
        (0xB0BA91E47F6200EC, "-5.874229071700385E-74"),
        (0xC72B4C36D0DB7FED, "-7.086891063466728E+34"),
        (0x35F305B0C0FC9252, "8.134710481112139E-49"),
        (0x7AC78FB373FFBFF6, "2.7371952383912E+283"),
        (0x8E39B81DE71B7D09, "-3.857101171397437E-240"),
        (0x59A69BA126CD29B3, "7.472552783215012E+123"),
        (0xBFCBFFFF0CBCBF20, "-0.21874988672197393"),
        (0x2D1CE28856D20E5E, "2.215603731797175E-91"),
        (0x572A15ED48B3FDC2, "7.841697833625344E+111"),
        (0x32B911499417AAB9, "2.3803041954013153E-64"),
        (0x17A8EC88E0DA0BAC, "1.0669643833474769E-194"),
        (0x0CC88EAB2F9AD4BB, "4.3902997915267284E-247"),
        (0xBC3A5B419A43AFD2, "-1.4287870889761718E-18"),
        (0x6F0F3414C47C9C0D, "9.239942182940311E+226"),
        (0xE71A7567EDB8C675, "-4.6049449625872434E+188"),
        (0x63674CF841EE8AB9, "7.034866163903114E+170"),
        (0xE375837DA840D9EC, "-1.2990664698521692E+171"),
        (0x578EB22E1EA5A35F, "5.905686629582816E+113"),
        (0x4EFA582EE029DFD9, "2.9091650704676072E+72"),
        (0xF3B363E938295A26, "-2.1692042628035588E+249"),
        (0x2E3A4EF3496F4112, "5.2900001119406624E-86"),
        (0xB809D78E2A2F9B10, "-9.49285634690637E-39"),
        (0x46217F104EB30026, "6.93101536724741E+29"),
        (0x8947B5FEFDA6AFA6, "-5.882775229507853E-264"),
        (0x4A3AE29B2AF0CB79, "3.9292734840969675E+49"),
        (0xBFD0273B10A6D4AF, "-0.2523944532980051"),
        (0xED0A00EB302B52C6, "-1.7928344098460697E+217"),
    ];

    #[test]
    fn serialized_version_gates_match_the_reference_thresholds() {
        assert_eq!(SERIALIZED_VERSION_KEY, "serializedVersion");
        assert!(!serialized_version_is_appended(0));
        assert!(!serialized_version_is_appended(1));
        assert!(serialized_version_is_appended(2));
        assert!(!serialized_version_is_forced(0));
        assert!(serialized_version_is_forced(1));
        assert!(serialized_version_is_forced(2));
        assert!(!serialized_version_is_prepended(1));
        assert!(serialized_version_is_prepended(2));
    }

    #[test]
    fn insert_begin_prepends_mapping_entries_in_order() {
        let mut mapping = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        mapping
            .insert_field("m_Name", UnityYamlNode::string("body"))
            .unwrap();
        mapping
            .insert_field_begin(
                SERIALIZED_VERSION_KEY,
                UnityYamlNode::scalar(UnityYamlScalar::Signed(2)),
            )
            .unwrap();
        let text = write_unity_yaml_mapping(&mapping, UnityYamlWriteLimits::default()).unwrap();
        assert_eq!(text, "serializedVersion: 2\nm_name: body\n");
        // A second prepend lands before the first.
        mapping
            .insert_field_begin("first", UnityYamlNode::scalar(UnityYamlScalar::Signed(1)))
            .unwrap();
        let text = write_unity_yaml_mapping(&mapping, UnityYamlWriteLimits::default()).unwrap();
        assert_eq!(text, "first: 1\nserializedVersion: 2\nm_name: body\n");
        // Prepending to a non-mapping is an explicit error.
        let mut scalar = UnityYamlNode::scalar(UnityYamlScalar::Signed(0));
        assert_eq!(
            scalar.insert_field_begin("k", UnityYamlNode::scalar(UnityYamlScalar::Signed(0))),
            Err(UnityYamlNodeBuildError::NotAMapping)
        );
    }

    #[test]
    fn serialized_version_entry_placement_follows_the_gates_end_to_end() {
        // Version 2 is prepended ahead of the body, like InsertSerializedVersion.
        let mut mapping = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        mapping
            .insert_field("m_Name", UnityYamlNode::string("v2"))
            .unwrap();
        let version = 2;
        if serialized_version_is_prepended(version) {
            mapping
                .insert_field_begin(
                    SERIALIZED_VERSION_KEY,
                    UnityYamlNode::scalar(UnityYamlScalar::Signed(version as i64)),
                )
                .unwrap();
        }
        let text = write_unity_yaml_mapping(&mapping, UnityYamlWriteLimits::default()).unwrap();
        assert_eq!(text, "serializedVersion: 2\nm_name: v2\n");
        // Version 1 is omitted entirely, like AddSerializedVersion.
        let mut mapping = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        mapping
            .insert_field("m_Name", UnityYamlNode::string("v1"))
            .unwrap();
        let version = 1;
        if serialized_version_is_appended(version) {
            mapping
                .insert_field(
                    SERIALIZED_VERSION_KEY,
                    UnityYamlNode::scalar(UnityYamlScalar::Signed(version as i64)),
                )
                .unwrap();
        }
        let text = write_unity_yaml_mapping(&mapping, UnityYamlWriteLimits::default()).unwrap();
        assert_eq!(text, "m_name: v1\n");
    }
}
