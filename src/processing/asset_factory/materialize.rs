use crate::io::binary::Endian;
use crate::processing::source_tpk::SourceTpkLookupError;
use crate::serialized_file::file::SerializedFile;
use crate::serialized_file::header::SerializedFileHeader;
use crate::serialized_file::metadata::{
    unity_version_aligns_arrays, ObjectInfo, SerializedFileMetadata, TypeTree,
};
use crate::serialized_file::object::{
    classify_type_tree_usability, decode_typetree_payload, raw_data_class_name,
    serialized_file_object_read_flags, ManagedPayloadDecodeContext, ObjectDecodeError,
    ObjectDecodeLimits, ParsedObjectPayload, SerializedValue, TypeTreeUsability,
};

use super::decisions::*;
use super::model::*;
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
    let raw_bytes = object_payload_slice(file, object_index)?;
    materialize_object_inner(
        MaterializationFile::from(file),
        facts,
        source,
        object_index,
        limits,
        raw_bytes,
        true,
    )
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
    let raw_bytes = object_payload_slice(file, object_index)?;
    materialize_object_inner(
        MaterializationFile::from(file),
        facts,
        source,
        object_index,
        limits,
        raw_bytes,
        false,
    )
    .map(FactoryAsset::into_workspace_materialization)
}

/// Materialize an object whose exact SerializedFile payload range was read from a file-backed
/// stream. The metadata and offsets remain those of the original file.
pub(crate) fn materialize_object_for_workspace_from_payload(
    header: &SerializedFileHeader,
    metadata: &SerializedFileMetadata,
    facts: &FactoryFileFacts<'_>,
    source: &dyn ClassLayoutSource,
    object_index: usize,
    raw_bytes: &[u8],
    limits: &ObjectDecodeLimits,
) -> Result<FactoryObjectMaterialization, FactoryEngineError> {
    materialize_object_inner(
        MaterializationFile { header, metadata },
        facts,
        source,
        object_index,
        limits,
        raw_bytes,
        false,
    )
    .map(FactoryAsset::into_workspace_materialization)
}

fn materialize_object_inner(
    file: MaterializationFile<'_>,
    facts: &FactoryFileFacts<'_>,
    source: &dyn ClassLayoutSource,
    object_index: usize,
    limits: &ObjectDecodeLimits,
    raw_bytes: &[u8],
    retain_raw_bytes: bool,
) -> Result<FactoryAsset, FactoryEngineError> {
    let object = file.metadata.objects.get(object_index).ok_or(
        FactoryEngineError::ObjectIndexOutOfBounds {
            object_index,
            object_count: file.metadata.objects.len(),
        },
    )?;
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
            &file,
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
            file: &file,
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

#[derive(Clone, Copy)]
struct MaterializationFile<'a> {
    header: &'a SerializedFileHeader,
    metadata: &'a SerializedFileMetadata,
}

impl<'a> From<&'a SerializedFile<'_>> for MaterializationFile<'a> {
    fn from(file: &'a SerializedFile<'_>) -> Self {
        Self {
            header: &file.header,
            metadata: &file.metadata,
        }
    }
}

impl MaterializationFile<'_> {
    fn object_read_flags(
        &self,
        file_path: &str,
        file_name: &str,
    ) -> crate::serialized_file::transfer_flags::TransferInstructionFlags {
        serialized_file_object_read_flags(
            self.header.format_version,
            self.metadata.prefix.target_platform,
            file_path,
            file_name,
            self.metadata.prefix.body_endian,
        )
    }
}

fn serialized_type_tree<'metadata>(
    file: &MaterializationFile<'metadata>,
    object_index: usize,
) -> Option<&'metadata TypeTree> {
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
    file: &MaterializationFile<'_>,
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
    file: &'a MaterializationFile<'file>,
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
pub(super) const SHADER_CLASS_ID: i32 = 48;

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
    file: &MaterializationFile<'_>,
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
pub(super) const TEXTURE2D_CLASS_ID: i32 = 28;

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
pub(super) fn decode_with_tree(
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

/// `ReadMonoBehaviour`: the generated header, then the structure source decision.
#[allow(clippy::too_many_arguments)]
fn read_monobehaviour(
    file: &MaterializationFile<'_>,
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
        if tree.nodes.len() == 1 && remainder.is_empty() {
            Ok(empty_structure_payload(
                tree,
                object_index,
                file.metadata.objects[object_index].path_id,
            ))
        } else {
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
        }
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
    let Some(first) = child_indices.get(start_index).copied() else {
        if start_index == child_indices.len() {
            return Some(TypeTree {
                nodes: vec![root.clone()],
                string_buffer: Vec::new(),
            });
        }
        return None;
    };
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

fn empty_structure_payload(
    tree: &TypeTree,
    object_index: usize,
    path_id: i64,
) -> ParsedObjectPayload {
    ParsedObjectPayload {
        object_index,
        path_id,
        type_name: tree
            .nodes
            .first()
            .and_then(|node| node.type_name.clone())
            .unwrap_or_default(),
        root: SerializedValue::Struct(Vec::new()),
        consumed_bytes: 0,
        trailing_bytes: Vec::new(),
        allocated_bytes: 0,
    }
}

impl FactoryAssetKind {
    pub(super) fn generated_name(&self) -> Option<&str> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::serialized_file::metadata::TypeTreeNode;

    fn node(level: u8, type_name: &str, name: &str) -> TypeTreeNode {
        TypeTreeNode {
            level,
            version: 1,
            type_flags: 0,
            type_string_offset: None,
            name_string_offset: None,
            type_name: Some(type_name.to_owned()),
            name: Some(name.to_owned()),
            byte_size: -1,
            index: 0,
            meta_flags: 0,
            reference_type_hash: None,
        }
    }

    #[test]
    fn mono_behaviour_tree_with_only_base_fields_has_a_valid_empty_structure() {
        let tree = TypeTree {
            nodes: vec![
                node(0, "MonoBehaviour", "Base"),
                node(1, "PPtr<GameObject>", "m_GameObject"),
                node(1, "UInt8", "m_Enabled"),
                node(1, "PPtr<MonoScript>", "m_Script"),
                node(1, "string", "m_Name"),
            ],
            string_buffer: Vec::new(),
        };
        let field_names = tree
            .nodes
            .iter()
            .skip(1)
            .filter(|node| node.level == 1)
            .map(|node| node.name.as_deref().unwrap_or(""))
            .collect::<Vec<_>>();
        let start_index = monobehaviour_structure_start_index(&field_names);
        assert_eq!(start_index, field_names.len());

        let structure = structure_subtree(&tree, start_index)
            .expect("a valid TypeTree with no script fields builds an empty structure");
        assert_eq!(structure.nodes, [tree.nodes[0].clone()]);
        let payload = empty_structure_payload(&structure, 894, 895);
        assert_eq!(payload.consumed_bytes, 0);
        assert_eq!(payload.trailing_bytes, []);
        assert_eq!(payload.root, SerializedValue::Struct(Vec::new()));
    }
}
