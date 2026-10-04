//! Bounded Unity YAML document, node, and pointer formatting primitives.

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

/// Convert a resolved project pointer to YAML; unresolved input pointers remain explicit errors.
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
            return Err(UnityYamlWriteError::UnresolvedInputReference(Box::new(
                reference.clone(),
            )));
        }
    };
    meta_pointer_to_unity_yaml(pointer, export_version, missing_reference_guid)
}

fn format_unity_guid(bytes: [u8; 16]) -> String {
    let mut value = String::with_capacity(32);
    for word in bytes.chunks_exact(4) {
        let word = u32::from_le_bytes([word[0], word[1], word[2], word[3]]);
        for shift in (0..8).rev() {
            let digit = ((word >> (shift * 4)) & 0x0f) as usize;
            value.push(b"0123456789abcdef"[digit] as char);
        }
    }
    value
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
            UnityYamlScalar::Float32(value) => self.write_str(&value.to_string()),
            UnityYamlScalar::Float64(value) => self.write_str(&value.to_string()),
            UnityYamlScalar::Float32Hex(value) => {
                self.write_str(&format!("0x{:08x}({})", value.to_bits(), value))
            }
            UnityYamlScalar::Float64Hex(value) => {
                self.write_str(&format!("0x{:016x}({})", value.to_bits(), value))
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
            "%YAML 1.1\n%TAG !u! tag:unity3d.com,2011:\n--- !u!1 &1\nPointer:\n  m_Ptr: {fileID: 42, guid: 04030201080706050c0b0a09100f0e0d, type: 1}\n"
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
            Err(UnityYamlWriteError::UnresolvedInputReference(Box::new(
                unresolved
            )))
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
}
