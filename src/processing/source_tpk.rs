//! Offline Rust reader for the frozen AssetRipper source TPK class layouts.
//!
//! `source_tpk_fixture.txt` contains the complete class histories, shared node buffer,
//! and string buffer projected by the isolated `tools/probes/ser-factory` probe from
//! `SourceTpk.GetStream()` and `TpkTypeTreeBlob`. The product has no .NET or AssetRipper
//! runtime dependency: it parses the committed data and performs source version
//! selection in Rust.

use super::asset_factory::FactoryUnityVersion;
use crate::serialized_file::metadata::{TypeTree, TypeTreeNode};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, OnceLock, RwLock};

pub type SourceTpkTrees = (Arc<TypeTree>, Arc<TypeTree>);

const SOURCE_TPK_FIXTURE: &str = include_str!("source_tpk_fixture.txt");
#[cfg(test)]
const SOURCE_TPK_SELECTION_FIXTURE: &str = include_str!("source_tpk_selection_fixture.txt");

#[derive(Clone, Debug, Eq, PartialEq)]
struct ClassRoots {
    name: u16,
    base: u16,
    flags: u8,
    release_root: Option<u16>,
    editor_root: Option<u16>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ClassVersion {
    version: FactoryUnityVersion,
    bits: u64,
    value: Option<ClassRoots>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceNode {
    type_name: u16,
    name: u16,
    byte_size: i32,
    version: i16,
    type_flags: u8,
    meta_flags: u32,
    children: Vec<u16>,
}

#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceTpkCounts {
    raw_length: usize,
    raw_sha256: String,
    class_count: usize,
    version_count: usize,
    boundary_count: usize,
    null_class_count: usize,
    node_count: usize,
    string_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SourceTpkDataError {
    line: usize,
    message: String,
}

impl std::fmt::Display for SourceTpkDataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "source TPK fixture line {}: {}", self.line, self.message)
    }
}

#[derive(Debug)]
pub(crate) struct SourceTpkDatabase {
    class_histories: BTreeMap<i32, Vec<ClassVersion>>,
    nodes: Vec<SourceNode>,
    strings: Vec<String>,
    #[cfg(test)]
    source_versions: Vec<(FactoryUnityVersion, u64)>,
    #[cfg(test)]
    counts: SourceTpkCounts,
    tree_cache: RwLock<HashMap<u16, Arc<TypeTree>>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceTpkLookupError {
    class_id: i32,
    version: String,
    message: String,
}

impl std::fmt::Display for SourceTpkLookupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "source TPK layout lookup for class {} at {} failed: {}",
            self.class_id, self.version, self.message
        )
    }
}

impl SourceTpkDatabase {
    pub(crate) fn embedded() -> &'static Self {
        static DATABASE: OnceLock<SourceTpkDatabase> = OnceLock::new();
        DATABASE.get_or_init(|| {
            Self::parse(SOURCE_TPK_FIXTURE)
                .expect("the embedded frozen-source TPK layout fixture must be valid")
        })
    }

    fn parse(fixture: &str) -> Result<Self, SourceTpkDataError> {
        let mut header: Option<(usize, String, usize, usize, usize)> = None;
        let mut expected_histories = BTreeMap::<i32, usize>::new();
        let mut class_histories = BTreeMap::<i32, Vec<ClassVersion>>::new();
        let mut source_versions = Vec::new();
        let mut nodes = Vec::new();
        let mut strings = Vec::new();
        let mut null_class_count = 0usize;

        for (line_index, line) in fixture.lines().enumerate() {
            let line_number = line_index + 1;
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<&str> = line.split('|').collect();
            match fields.first().copied() {
                Some("TPK-DATA") => {
                    if fields.len() != 7 || header.is_some() {
                        return Err(data_error(
                            line_number,
                            "invalid or duplicate TPK-DATA header",
                        ));
                    }
                    header = Some((
                        parse_field(fields[2], line_number, "raw TPK length")?,
                        fields[3].to_owned(),
                        parse_field(fields[4], line_number, "class count")?,
                        parse_field(fields[5], line_number, "node count")?,
                        parse_field(fields[6], line_number, "string count")?,
                    ));
                    if fields[1] != "1" {
                        return Err(data_error(
                            line_number,
                            "unsupported TPK fixture format version",
                        ));
                    }
                }
                Some("VERSION") => {
                    if fields.len() != 8 {
                        return Err(data_error(line_number, "invalid VERSION record"));
                    }
                    let version = parse_source_version(&fields, line_number)?;
                    let bits = parse_hex_bits(fields[2], line_number)?;
                    if version.source_order_bits() != Some(bits) {
                        return Err(data_error(
                            line_number,
                            "VERSION fields disagree with UnityVersion.GetBits",
                        ));
                    }
                    if source_versions
                        .last()
                        .is_some_and(|(_, previous_bits)| *previous_bits >= bits)
                    {
                        return Err(data_error(
                            line_number,
                            "source Unity versions are not strictly ordered",
                        ));
                    }
                    source_versions.push((version, bits));
                }
                Some("CLASSSET") => {
                    if fields.len() != 3 {
                        return Err(data_error(line_number, "invalid CLASSSET record"));
                    }
                    let class_id = parse_field(fields[1], line_number, "class ID")?;
                    let boundary_count =
                        parse_field(fields[2], line_number, "class boundary count")?;
                    if expected_histories
                        .insert(class_id, boundary_count)
                        .is_some()
                    {
                        return Err(data_error(line_number, "duplicate class ID"));
                    }
                    class_histories.insert(class_id, Vec::with_capacity(boundary_count));
                }
                Some("CLASS") => {
                    if fields.len() < 10 {
                        return Err(data_error(line_number, "short CLASS record"));
                    }
                    let class_id = parse_field(fields[1], line_number, "class ID")?;
                    let version = parse_source_version(&fields, line_number)?;
                    let bits = parse_hex_bits(fields[3], line_number)?;
                    if version.source_order_bits() != Some(bits) {
                        return Err(data_error(
                            line_number,
                            "CLASS version disagrees with UnityVersion.GetBits",
                        ));
                    }
                    let history = class_histories
                        .get_mut(&class_id)
                        .ok_or_else(|| data_error(line_number, "CLASS record has no CLASSSET"))?;
                    if history.last().is_some_and(|previous| previous.bits >= bits) {
                        return Err(data_error(
                            line_number,
                            "class version boundaries are not strictly ordered",
                        ));
                    }
                    let value = if fields[9] == "NULL" {
                        if fields.len() != 10 {
                            return Err(data_error(
                                line_number,
                                "NULL CLASS record has trailing fields",
                            ));
                        }
                        null_class_count += 1;
                        None
                    } else {
                        if fields.len() != 14 {
                            return Err(data_error(line_number, "invalid populated CLASS record"));
                        }
                        Some(ClassRoots {
                            name: parse_field(fields[9], line_number, "class name string ID")?,
                            base: parse_field(fields[10], line_number, "base string ID")?,
                            flags: parse_field(fields[11], line_number, "class flags")?,
                            release_root: parse_root(fields[12], line_number, "release root node")?,
                            editor_root: parse_root(fields[13], line_number, "editor root node")?,
                        })
                    };
                    history.push(ClassVersion {
                        version,
                        bits,
                        value,
                    });
                }
                Some("NODE") => {
                    if fields.len() != 9 {
                        return Err(data_error(line_number, "invalid NODE record"));
                    }
                    let index: usize = parse_field(fields[1], line_number, "node index")?;
                    if index != nodes.len() {
                        return Err(data_error(line_number, "node indices are not contiguous"));
                    }
                    let children = if fields[8].is_empty() {
                        Vec::new()
                    } else {
                        fields[8]
                            .split(',')
                            .map(|value| parse_field(value, line_number, "child node index"))
                            .collect::<Result<Vec<u16>, _>>()?
                    };
                    nodes.push(SourceNode {
                        type_name: parse_field(fields[2], line_number, "type string ID")?,
                        name: parse_field(fields[3], line_number, "name string ID")?,
                        byte_size: parse_field(fields[4], line_number, "node byte size")?,
                        version: parse_field(fields[5], line_number, "node version")?,
                        type_flags: parse_field(fields[6], line_number, "node type flags")?,
                        meta_flags: parse_field(fields[7], line_number, "node metadata flags")?,
                        children,
                    });
                }
                Some("STRING") => {
                    if fields.len() != 3 {
                        return Err(data_error(line_number, "invalid STRING record"));
                    }
                    let index: usize = parse_field(fields[1], line_number, "string index")?;
                    if index != strings.len() {
                        return Err(data_error(line_number, "string indices are not contiguous"));
                    }
                    let bytes = decode_base64(fields[2])
                        .ok_or_else(|| data_error(line_number, "invalid Base64 UTF-8 string"))?;
                    let value = String::from_utf8(bytes)
                        .map_err(|_| data_error(line_number, "TPK string is not valid UTF-8"))?;
                    strings.push(value);
                }
                Some(other) => {
                    return Err(data_error(
                        line_number,
                        format!("unknown record kind {other}"),
                    ));
                }
                None => continue,
            }
        }

        let (raw_length, raw_sha256, class_count, node_count, string_count) =
            header.ok_or_else(|| data_error(0, "missing TPK-DATA header"))?;
        if raw_length == 0
            || raw_sha256.len() != 64
            || !raw_sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(data_error(0, "invalid raw TPK identity in header"));
        }
        if source_versions.len() != 1420 {
            return Err(data_error(
                0,
                format!(
                    "expected 1,420 source versions, found {}",
                    source_versions.len()
                ),
            ));
        }
        if class_histories.len() != class_count || expected_histories.len() != class_count {
            return Err(data_error(
                0,
                format!(
                    "expected {class_count} class IDs, found {}",
                    class_histories.len()
                ),
            ));
        }
        let mut boundary_count = 0usize;
        for (class_id, history) in &class_histories {
            let expected = expected_histories.get(class_id).copied().unwrap_or(0);
            if history.len() != expected {
                return Err(data_error(
                    0,
                    format!(
                        "class {class_id} has {} boundaries, expected {expected}",
                        history.len()
                    ),
                ));
            }
            boundary_count = boundary_count
                .checked_add(history.len())
                .ok_or_else(|| data_error(0, "class boundary count overflow"))?;
        }
        if boundary_count == 0 || null_class_count > boundary_count {
            return Err(data_error(0, "invalid class boundary/null-entry totals"));
        }
        if nodes.len() != node_count || strings.len() != string_count {
            return Err(data_error(0, format!("node/string counts disagree with header ({} / {}, expected {node_count} / {string_count})", nodes.len(), strings.len())));
        }
        for (index, node) in nodes.iter().enumerate() {
            if usize::from(node.type_name) >= strings.len()
                || usize::from(node.name) >= strings.len()
            {
                return Err(data_error(
                    0,
                    format!("node {index} has an invalid string reference"),
                ));
            }
            if node
                .children
                .iter()
                .any(|child| usize::from(*child) >= nodes.len())
            {
                return Err(data_error(
                    0,
                    format!("node {index} has an invalid child reference"),
                ));
            }
        }
        for (class_id, history) in &class_histories {
            for entry in history {
                if let Some(value) = &entry.value {
                    if usize::from(value.name) >= strings.len()
                        || usize::from(value.base) >= strings.len()
                        || value
                            .release_root
                            .is_some_and(|root| usize::from(root) >= nodes.len())
                        || value
                            .editor_root
                            .is_some_and(|root| usize::from(root) >= nodes.len())
                    {
                        return Err(data_error(
                            0,
                            format!("class {class_id} has an invalid TPK reference"),
                        ));
                    }
                }
            }
        }

        Ok(Self {
            class_histories,
            nodes,
            strings,
            #[cfg(test)]
            source_versions,
            #[cfg(test)]
            counts: SourceTpkCounts {
                raw_length,
                raw_sha256,
                class_count,
                version_count: 1420,
                boundary_count,
                null_class_count,
                node_count,
                string_count,
            },
            tree_cache: RwLock::new(HashMap::new()),
        })
    }

    #[cfg(test)]
    fn counts(&self) -> &SourceTpkCounts {
        &self.counts
    }

    #[cfg(test)]
    pub(crate) fn source_versions(&self) -> &[(FactoryUnityVersion, u64)] {
        &self.source_versions
    }

    pub(crate) fn trees(
        &self,
        class_id: i32,
        version: &FactoryUnityVersion,
    ) -> Result<Option<SourceTpkTrees>, SourceTpkLookupError> {
        let failure = |message: &str| SourceTpkLookupError {
            class_id,
            version: version.text.clone(),
            message: message.to_owned(),
        };
        let version_bits = version
            .source_order_bits()
            .ok_or_else(|| failure("version cannot be ordered by the source UnityVersion rules"))?;
        let Some(history) = self.class_histories.get(&class_id) else {
            return Ok(None);
        };
        let Some(entry) = select_class_version(history, version_bits).map_err(failure)? else {
            return Ok(None);
        };
        let roots = entry
            .value
            .as_ref()
            .ok_or_else(|| failure("version selection resolved to a null class"))?;
        let _source_class_identity = (roots.name, roots.base, roots.flags);
        let release_root = roots
            .release_root
            .ok_or_else(|| failure("selected class has no release root node"))?;
        let editor_root = roots
            .editor_root
            .ok_or_else(|| failure("selected class has no editor root node"))?;
        let release = self
            .tree(release_root)
            .ok_or_else(|| failure("release tree contains an invalid or cyclic node graph"))?;
        let editor = self
            .tree(editor_root)
            .ok_or_else(|| failure("editor tree contains an invalid or cyclic node graph"))?;
        Ok(Some((release, editor)))
    }

    fn tree(&self, root: u16) -> Option<Arc<TypeTree>> {
        if let Ok(cache) = self.tree_cache.read() {
            if let Some(tree) = cache.get(&root) {
                return Some(Arc::clone(tree));
            }
        }
        let tree = Arc::new(self.build_tree(root)?);
        if let Ok(mut cache) = self.tree_cache.write() {
            return Some(Arc::clone(cache.entry(root).or_insert(tree)));
        }
        Some(tree)
    }

    fn build_tree(&self, root: u16) -> Option<TypeTree> {
        let mut flattened = Vec::new();
        let mut path = vec![false; self.nodes.len()];
        self.append_node(root, 0, &mut path, &mut flattened)?;
        Some(TypeTree {
            nodes: flattened,
            string_buffer: Vec::new(),
        })
    }

    fn append_node(
        &self,
        node_index: u16,
        level: usize,
        path: &mut [bool],
        flattened: &mut Vec<TypeTreeNode>,
    ) -> Option<()> {
        if level > usize::from(u8::MAX) {
            return None;
        }
        let index = usize::from(node_index);
        if *path.get(index)? {
            return None;
        }
        let source = self.nodes.get(index)?.clone();
        let type_name = self.strings.get(usize::from(source.type_name))?.clone();
        let name = self.strings.get(usize::from(source.name))?.clone();
        let output_index = i32::try_from(flattened.len()).ok()?;
        path[index] = true;
        flattened.push(TypeTreeNode {
            level: u8::try_from(level).ok()?,
            version: i32::from(source.version),
            type_flags: i32::from(source.type_flags),
            type_string_offset: None,
            name_string_offset: None,
            type_name: Some(type_name),
            name: Some(name),
            byte_size: source.byte_size,
            index: output_index,
            meta_flags: source.meta_flags,
            reference_type_hash: None,
        });
        for child in source.children {
            self.append_node(child, level + 1, path, flattened)?;
        }
        path[index] = false;
        Some(())
    }
}

fn select_class_version(
    history: &[ClassVersion],
    version_bits: u64,
) -> Result<Option<&ClassVersion>, &'static str> {
    let first = history.first().ok_or("class version history is empty")?;
    if first.bits > version_bits {
        return if first.value.is_some() {
            Ok(Some(first))
        } else {
            Err("first class version entry is null")
        };
    }
    for index in 0..history.len().saturating_sub(1) {
        let current = &history[index];
        let next = &history[index + 1];
        if current.bits <= version_bits && version_bits < next.bits {
            if current.value.is_some() {
                return Ok(Some(current));
            }
            return index
                .checked_sub(1)
                .and_then(|previous| history.get(previous))
                .filter(|previous| previous.value.is_some())
                .map(Some)
                .ok_or("null class version entry has no preceding class");
        }
    }
    let last = history.last().ok_or("class version history is empty")?;
    if last.value.is_some() {
        Ok(Some(last))
    } else {
        let previous_index = history
            .len()
            .checked_sub(2)
            .ok_or("final null class entry has no preceding class")?;
        history
            .get(previous_index)
            .filter(|previous| previous.value.is_some())
            .map(Some)
            .ok_or("final null class entry has no preceding class")
    }
}

fn parse_source_version(
    fields: &[&str],
    line: usize,
) -> Result<FactoryUnityVersion, SourceTpkDataError> {
    let (version_index, offset) = match fields.first().copied() {
        Some("VERSION") => (1, 3),
        Some("CLASS") => (2, 4),
        _ => {
            return Err(data_error(
                line,
                "version used outside VERSION or CLASS record",
            ))
        }
    };
    let version =
        FactoryUnityVersion::parse(fields.get(version_index).copied().unwrap_or_default())
            .ok_or_else(|| data_error(line, "invalid Unity version"))?;
    let expected_major: u32 = parse_field(
        fields.get(offset).unwrap_or(&""),
        line,
        "Unity major version",
    )?;
    let expected_minor: u32 = parse_field(
        fields.get(offset + 1).unwrap_or(&""),
        line,
        "Unity minor version",
    )?;
    let expected_build: u16 = parse_field(
        fields.get(offset + 2).unwrap_or(&""),
        line,
        "Unity build version",
    )?;
    let expected_type: u8 = parse_field(
        fields.get(offset + 3).unwrap_or(&""),
        line,
        "Unity version type",
    )?;
    let expected_type_number: u8 = parse_field(
        fields.get(offset + 4).unwrap_or(&""),
        line,
        "Unity version type number",
    )?;
    if version.major != expected_major
        || version.minor != expected_minor
        || version.build != expected_build
        || version_type_code(version.version_type) != Some(expected_type)
        || version.type_number != expected_type_number
    {
        return Err(data_error(line, "Unity version text and fields disagree"));
    }
    Ok(version)
}

fn version_type_code(version_type: super::asset_factory::FactoryVersionType) -> Option<u8> {
    use super::asset_factory::FactoryVersionType;
    Some(match version_type {
        FactoryVersionType::Alpha => 0,
        FactoryVersionType::Beta => 1,
        FactoryVersionType::China => 2,
        FactoryVersionType::Release => 3,
        FactoryVersionType::Patch => 4,
        FactoryVersionType::Experimental => 5,
        FactoryVersionType::Unknown => return None,
    })
}

fn parse_hex_bits(value: &str, line: usize) -> Result<u64, SourceTpkDataError> {
    u64::from_str_radix(value, 16).map_err(|_| data_error(line, "invalid packed Unity version"))
}

fn parse_root(value: &str, line: usize, name: &str) -> Result<Option<u16>, SourceTpkDataError> {
    let root: u16 = parse_field(value, line, name)?;
    Ok((root != u16::MAX).then_some(root))
}

fn parse_field<T: std::str::FromStr>(
    value: &str,
    line: usize,
    name: &str,
) -> Result<T, SourceTpkDataError> {
    value
        .parse::<T>()
        .map_err(|_| data_error(line, format!("invalid {name}")))
}

fn data_error(line: usize, message: impl Into<String>) -> SourceTpkDataError {
    SourceTpkDataError {
        line,
        message: message.into(),
    }
}

fn decode_base64(input: &str) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity(input.len().saturating_mul(3) / 4);
    let mut buffer = 0u32;
    let mut bits = 0u8;
    let mut padding = false;
    for byte in input.bytes() {
        let value = match byte {
            b'A'..=b'Z' => u32::from(byte - b'A'),
            b'a'..=b'z' => u32::from(byte - b'a' + 26),
            b'0'..=b'9' => u32::from(byte - b'0' + 52),
            b'+' => 62,
            b'/' => 63,
            b'=' => {
                padding = true;
                continue;
            }
            _ => return None,
        };
        if padding {
            return None;
        }
        buffer = (buffer << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push(((buffer >> bits) & 0xff) as u8);
            buffer &= (1u32 << bits).wrapping_sub(1);
        }
    }
    if bits == 6 || (bits != 0 && buffer != 0) {
        return None;
    }
    Some(output)
}

#[cfg(test)]
fn tree_signature(tree: &TypeTree) -> (usize, u64) {
    const OFFSET_BASIS: u64 = 14_695_981_039_346_656_037;
    const PRIME: u64 = 1_099_511_628_211;
    let mut hash = OFFSET_BASIS;
    for node in &tree.nodes {
        let line = format!(
            "{}|{}|{}|{}|{}\n",
            node.level,
            node.type_name.as_deref().unwrap_or_default(),
            node.name.as_deref().unwrap_or_default(),
            node.version,
            node.meta_flags
        );
        for byte in line.as_bytes() {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(PRIME);
        }
    }
    (tree.nodes.len(), hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processing::asset_factory::ClassLayoutSource;

    #[test]
    fn embedded_tpk_projection_has_all_source_classes_and_nodes() {
        let database = SourceTpkDatabase::embedded();
        assert_eq!(database.counts().raw_length, 207_864);
        assert_eq!(
            database.counts().raw_sha256,
            "B5BD478FD590B76362D09349EBC9BA28DC126A6A8460BD19B4CC193CCE1CB894"
        );
        assert_eq!(database.counts().class_count, 403);
        assert_eq!(database.counts().version_count, 1420);
        assert_eq!(database.counts().boundary_count, 5098);
        assert_eq!(database.counts().null_class_count, 98);
        assert_eq!(database.counts().node_count, 26_516);
        assert_eq!(database.counts().string_count, 7363);
        assert_eq!(database.source_versions().len(), 1420);
    }

    #[test]
    fn base64_decoder_preserves_empty_ascii_and_unicode_strings() {
        assert_eq!(decode_base64(""), Some(Vec::new()));
        assert_eq!(decode_base64("SGVsbG8="), Some(b"Hello".to_vec()));
        assert_eq!(decode_base64("5LiW55WM"), Some("世界".as_bytes().to_vec()));
        assert_eq!(decode_base64("SGVsbG8!"), None);
    }

    #[test]
    fn tpk_version_selection_matches_source_range_and_null_fallback_rules() {
        let version = |text: &str| FactoryUnityVersion::parse(text).unwrap();
        let tree = |root| ClassRoots {
            name: 0,
            base: 0,
            flags: 0,
            release_root: Some(root),
            editor_root: Some(root),
        };
        let history = vec![
            ClassVersion {
                version: version("2019.1.0a0"),
                bits: version("2019.1.0a0").source_order_bits().unwrap(),
                value: Some(tree(10)),
            },
            ClassVersion {
                version: version("2019.2.0a0"),
                bits: version("2019.2.0a0").source_order_bits().unwrap(),
                value: None,
            },
            ClassVersion {
                version: version("2020.1.0a0"),
                bits: version("2020.1.0a0").source_order_bits().unwrap(),
                value: Some(tree(20)),
            },
        ];
        let selected_root = |bits| {
            select_class_version(&history, bits)
                .ok()
                .flatten()
                .and_then(|entry| entry.value.as_ref()?.release_root)
        };
        assert_eq!(selected_root(0), Some(10));
        assert_eq!(selected_root(history[0].bits), Some(10));
        assert_eq!(selected_root(history[1].bits), Some(10));
        assert_eq!(selected_root(history[2].bits), Some(20));
        assert_eq!(selected_root(u64::MAX), Some(20));
    }

    #[test]
    fn full_tpk_tree_for_2019_material_matches_source_probe_rows() {
        let database = SourceTpkDatabase::embedded();
        let version = FactoryUnityVersion::parse("2019.4.40f1").unwrap();
        for class_id in [49, 114, 115, 194] {
            let actual = database.trees(class_id, &version).unwrap().unwrap();
            let expected =
                crate::processing::asset_factory::ReferenceClassLayouts::from_probe_fixture()
                    .tpk_trees(class_id, &version)
                    .unwrap()
                    .unwrap();
            for (actual_tree, expected_tree) in [(&actual.0, &expected.0), (&actual.1, &expected.1)]
            {
                let actual_nodes: Vec<_> = actual_tree
                    .nodes
                    .iter()
                    .map(|node| {
                        (
                            node.level,
                            node.type_name.as_deref(),
                            node.name.as_deref(),
                            node.version,
                            node.meta_flags,
                        )
                    })
                    .collect();
                let expected_nodes: Vec<_> = expected_tree
                    .nodes
                    .iter()
                    .map(|node| {
                        (
                            node.level,
                            node.type_name.as_deref(),
                            node.name.as_deref(),
                            node.version,
                            node.meta_flags,
                        )
                    })
                    .collect();
                assert_eq!(actual_nodes, expected_nodes, "class {class_id}");
            }
        }
    }

    #[test]
    fn production_class_layout_source_uses_the_complete_tpk_database() {
        let layouts = crate::processing::asset_factory::reference_class_layouts();
        let version = FactoryUnityVersion::parse("0.0.0f0").unwrap();
        let (release, editor) = layouts.tpk_trees(1, &version).unwrap().unwrap();
        assert_eq!(tree_signature(&release), (16, 0x02D8_6FCD_8685_B47D));
        assert_eq!(tree_signature(&editor), (31, 0x7AD4_629B_5892_824D));

        let rootless = layouts.tpk_trees(0, &version).unwrap_err();
        assert!(rootless.to_string().contains("release root node"));
    }

    #[test]
    fn every_source_tpk_class_boundary_matches_reference_tree_selection() {
        let database = SourceTpkDatabase::embedded();
        let mut rows = 0usize;
        let mut successful = 0usize;
        let mut thrown = 0usize;
        for (line_number, line) in SOURCE_TPK_SELECTION_FIXTURE.lines().enumerate() {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let fields: Vec<&str> = line.split('|').collect();
            assert_eq!(fields.first(), Some(&"SELECT"), "line {}", line_number + 1);
            let class_id = fields[1].parse::<i32>().unwrap();
            let version = FactoryUnityVersion::parse(fields[2]).unwrap();
            let actual = database.trees(class_id, &version);
            match fields[3] {
                "OK" => {
                    assert_eq!(fields.len(), 8, "line {}", line_number + 1);
                    let (release, editor) = actual
                        .unwrap_or_else(|error| panic!("line {}: {error}", line_number + 1))
                        .unwrap_or_else(|| {
                            panic!(
                                "line {} unexpectedly missed class {class_id}",
                                line_number + 1
                            )
                        });
                    assert_eq!(
                        tree_signature(&release),
                        (
                            fields[4].parse::<usize>().unwrap(),
                            u64::from_str_radix(fields[5], 16).unwrap(),
                        ),
                        "release class {class_id} at {}",
                        fields[2]
                    );
                    assert_eq!(
                        tree_signature(&editor),
                        (
                            fields[6].parse::<usize>().unwrap(),
                            u64::from_str_radix(fields[7], 16).unwrap(),
                        ),
                        "editor class {class_id} at {}",
                        fields[2]
                    );
                    successful += 1;
                }
                "THROW" => {
                    assert_eq!(fields.len(), 5, "line {}", line_number + 1);
                    assert!(
                        actual.is_err(),
                        "source throws {} for class {class_id} at {}, Rust returned {actual:?}",
                        fields[4],
                        fields[2]
                    );
                    thrown += 1;
                }
                "MISSING" => {
                    assert_eq!(fields.len(), 4, "line {}", line_number + 1);
                    assert_eq!(actual.unwrap(), None, "class {class_id} at {}", fields[2]);
                }
                other => panic!(
                    "line {} has unknown source outcome {other}",
                    line_number + 1
                ),
            }
            rows += 1;
        }
        assert_eq!(rows, 7304);
        assert_eq!(successful, 7240);
        assert_eq!(thrown, 64);
    }
}
