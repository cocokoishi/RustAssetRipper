/// `ReadMonoBehaviour`: the generated header, then the structure source decision.
#[allow(clippy::too_many_arguments)]
fn read_monobehaviour(
    file: &SerializedFile<'_>,
    facts: &FactoryFileFacts<'_>,
    source: &dyn ClassLayoutSource,
    object_index: usize,
    limits: &ObjectDecodeLimits,
    version: &FactoryUnityVersion,
    payload_bytes: &[u8],
    asset: &mut FactoryAsset,
) {
    let generated_name = match source.generated_class_name(MONO_BEHAVIOUR_CLASS_ID, version) {
        Ok(name) => name.unwrap_or("MonoBehaviour").to_owned(),
        Err(error) => {
            asset.kind = FactoryAssetKind::Unreadable;
            asset.log_level = FactoryLogLevel::Error;
            asset.log_message = Some(error.to_string());
            return;
        }
    };
    asset.kind = FactoryAssetKind::Generated(generated_name);
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
                    payload_bytes,
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

/// Class identity and class layouts pinned by isolated reference probes. The explicit
/// fixture parsed by `parse` is bounded to its authored cases; the production accessor
/// overlays the complete source `AssetFactory.CreateSerialized` dispatch matrix and
/// complete TPK class/version database.
#[derive(Clone, Debug, Default)]
pub struct ReferenceClassLayouts {
    generated: std::collections::BTreeMap<i32, String>,
    tpk: std::collections::BTreeMap<(i32, String), (Arc<TypeTree>, Arc<TypeTree>)>,
    source_tpk: Option<&'static SourceTpkDatabase>,
    source_factory_dispatch: Option<&'static SourceFactoryDispatch>,
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
            source_factory_dispatch: None,
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
    fn generated_class_name(
        &self,
        class_id: i32,
        version: &FactoryUnityVersion,
    ) -> Result<Option<&str>, SourceFactoryLookupError> {
        if let Some(dispatch) = self.source_factory_dispatch {
            return dispatch.generated_class_name(class_id, version);
        }
        if self.probed_version() != Some(version.text.as_str()) {
            return Ok(None);
        }
        Ok(self.generated.get(&class_id).map(String::as_str))
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
/// Generated class names use the source-probed `AssetFactory.CreateSerialized`
/// class/version matrix. TPK layouts use the complete source class/version/node/string
/// tables.
pub fn reference_class_layouts() -> &'static ReferenceClassLayouts {
    static LAYOUTS: OnceLock<ReferenceClassLayouts> = OnceLock::new();
    LAYOUTS.get_or_init(|| {
        let mut layouts = ReferenceClassLayouts::parse(PROBE_FIXTURE);
        layouts.source_tpk = Some(SourceTpkDatabase::embedded());
        layouts.source_factory_dispatch = Some(SourceFactoryDispatch::embedded());
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
