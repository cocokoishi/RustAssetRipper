use super::error::{object_workspace_error, ObjectWorkspaceError, ObjectWorkspaceErrorKind};
use super::{
    ManagedBehaviourRecovery, ManagedBehaviourRecoveryOutcome, ManagedBehaviourRecoveryReport,
    ObjectWorkspace, ObjectWorkspaceLimits,
};
use crate::io::filesystem::VirtualFileSystem;
use crate::processing::asset_factory::reference_class_layouts;
use crate::processing::asset_factory::{FactoryObjectState, MonoBehaviourStructureOutcome};
use crate::processing::managed_assembly::{
    resolve_mono_script_object, ManagedAssemblyCatalog, ManagedScriptResolutionOutcome,
};
use crate::processing::managed_assembly_schema::{
    build_managed_reference_type_tree, build_mono_behaviour_schema, ManagedSchemaLimits,
};
use crate::processing::script_identity::ScriptIdentifier;
use crate::serialized_file::file::SerializedFile;
use crate::serialized_file::metadata;
use crate::serialized_file::object::{
    decode_typetree_payload, ManagedPayloadDecodeContext, ParsedObjectPayload,
    SerializedFieldValue, SerializedValue,
};
use crate::serialized_file::reference_graph::{
    MultiFilePPtrResolution, MultiFileReferenceGraphLimits, ObjectReferenceGraph,
    ReferenceGraphLimits,
};
use crate::serialized_file::workspace::SerializedFileWorkspace;

struct ManagedBehaviourCandidate {
    file_index: usize,
    object_index: usize,
    path_id: i64,
    script_file_index: Option<usize>,
    script: Result<ScriptIdentifier, String>,
}

struct PendingManagedBehaviourRecovery {
    object_index: usize,
    payload: ParsedObjectPayload,
    factory: FactoryObjectState,
    script: ScriptIdentifier,
    script_version: String,
    decoded_bytes: usize,
    skipped_fields: Vec<String>,
}

impl ObjectWorkspace {
    /// Resolve source-generated MonoBehaviour headers through their `m_Script` PPtr, build the
    /// assembly-backed schema with the MonoScript file's Unity version, decode each saved
    /// remainder, and rebuild affected file reference scans from the recovered payloads.
    /// Failed or unavailable scripts remain explicit `Unresolved` outcomes and keep the object
    /// incomplete.
    pub fn recover_mono_behaviour_structures(
        &mut self,
        filesystem: &VirtualFileSystem,
        workspace: &mut SerializedFileWorkspace,
        catalog: &ManagedAssemblyCatalog,
        limits: ObjectWorkspaceLimits,
        schema_limits: ManagedSchemaLimits,
    ) -> Result<ManagedBehaviourRecoveryReport, Box<ObjectWorkspaceError>> {
        let candidates = {
            let graph = workspace
                .reference_graph(MultiFileReferenceGraphLimits::default())
                .map_err(|source| {
                    object_workspace_error(
                        0,
                        "<workspace>",
                        None,
                        ObjectWorkspaceErrorKind::MultiFileReferenceGraphBuild(source),
                    )
                })?;
            let mut candidates = Vec::new();
            for object in self.objects.iter().filter(|object| {
                object.factory.class_id == crate::processing::asset_factory::MONO_BEHAVIOUR_CLASS_ID
                    && object.factory.structure.as_ref().is_some_and(|structure| {
                        structure.outcome == MonoBehaviourStructureOutcome::UseUnloadedRemainder
                    })
            }) {
                let script_reference = graph
                    .references_from_object(object.file_index, object.object_index)
                    .find(|reference| reference.field_path == "m_Script");
                let mut script_file_index = None;
                let resolved = match script_reference.map(|reference| &reference.resolution) {
                    Some(MultiFilePPtrResolution::Object {
                        target_file_index,
                        target_object_index,
                    }) => {
                        script_file_index = Some(*target_file_index);
                        match self
                            .get(*target_file_index, *target_object_index)
                            .filter(|target| target.factory.class_id == 115)
                        {
                            Some(target) => {
                                let resolution = resolve_mono_script_object(target, catalog);
                                if matches!(
                                    resolution.outcome,
                                    ManagedScriptResolutionOutcome::Resolved { is_valid: true }
                                ) {
                                    resolution.script.ok_or_else(|| {
                                        "resolved MonoScript has no class identity".to_owned()
                                    })
                                } else {
                                    Err(format!(
                                        "MonoScript lookup returned {:?}",
                                        resolution.outcome
                                    ))
                                }
                            }
                            None => Err("m_Script does not resolve to a decoded MonoScript object"
                                .to_owned()),
                        }
                    }
                    Some(outcome) => Err(format!("m_Script reference is {outcome:?}")),
                    None => {
                        Err("m_Script reference is absent from the decoded header graph".to_owned())
                    }
                };
                candidates.push(ManagedBehaviourCandidate {
                    file_index: object.file_index,
                    object_index: object.object_index,
                    path_id: object.path_id,
                    script_file_index,
                    script: resolved,
                });
            }
            candidates
        };

        let mut report = ManagedBehaviourRecoveryReport::default();
        let mut candidate_index = 0;
        while candidate_index < candidates.len() {
            let file_index = candidates[candidate_index].file_index;
            let (file_name, virtual_path) = workspace
                .files()
                .get(file_index)
                .map(|file| (file.file_name.clone(), file.virtual_path.clone()))
                .ok_or_else(|| {
                    object_workspace_error(
                        file_index,
                        "<workspace>",
                        None,
                        ObjectWorkspaceErrorKind::WorkspaceFileIndexOutOfBounds,
                    )
                })?;
            let file_stream = filesystem.open_file(&virtual_path).map_err(|source| {
                object_workspace_error(
                    file_index,
                    &virtual_path,
                    None,
                    ObjectWorkspaceErrorKind::FileSystem(source),
                )
            })?;
            let file_length = file_stream.length().map_err(|source| {
                object_workspace_error(
                    file_index,
                    &virtual_path,
                    None,
                    ObjectWorkspaceErrorKind::FileSystem(source),
                )
            })?;
            if file_length > limits.max_file_bytes {
                return Err(object_workspace_error(
                    file_index,
                    &virtual_path,
                    None,
                    ObjectWorkspaceErrorKind::FileSizeLimitExceeded {
                        file_bytes: file_length,
                        limit: limits.max_file_bytes,
                    },
                ));
            }
            let maximum_bytes = usize::try_from(limits.max_file_bytes).unwrap_or(usize::MAX);
            let file_bytes = filesystem
                .read_all_bytes(&virtual_path, maximum_bytes)
                .map_err(|source| {
                    object_workspace_error(
                        file_index,
                        &virtual_path,
                        None,
                        ObjectWorkspaceErrorKind::FileSystem(source),
                    )
                })?;
            let parsed_file = SerializedFile::parse(&file_bytes).map_err(|source| {
                object_workspace_error(
                    file_index,
                    &virtual_path,
                    None,
                    ObjectWorkspaceErrorKind::SerializedFile(source),
                )
            })?;
            let identity_matches = workspace.files().get(file_index).is_some_and(|file| {
                parsed_file.header == file.header && parsed_file.metadata == file.metadata
            });
            if !identity_matches {
                return Err(object_workspace_error(
                    file_index,
                    &virtual_path,
                    None,
                    ObjectWorkspaceErrorKind::FileChangedSinceWorkspaceLoad,
                ));
            }

            let mut pending = Vec::new();
            let mut overrides = Vec::new();
            let mut pending_allocation_bytes = 0usize;
            while candidate_index < candidates.len()
                && candidates[candidate_index].file_index == file_index
            {
                let candidate = &candidates[candidate_index];
                candidate_index += 1;
                let mut result = ManagedBehaviourRecovery {
                    file_index,
                    object_index: candidate.object_index,
                    path_id: candidate.path_id,
                    script: candidate.script.as_ref().ok().cloned(),
                    outcome: ManagedBehaviourRecoveryOutcome::Unresolved(
                        candidate.script.as_ref().err().cloned().unwrap_or_default(),
                    ),
                };
                let script = match &candidate.script {
                    Ok(script) => script,
                    Err(_) => {
                        report.behaviours.push(result);
                        continue;
                    }
                };
                let script_version = candidate
                    .script_file_index
                    .and_then(|script_file_index| workspace.files().get(script_file_index))
                    .and_then(|script_file| {
                        script_file.metadata.prefix.unity_version_text.as_deref()
                    })
                    .unwrap_or("0.0.0")
                    .to_owned();
                let schema = match build_mono_behaviour_schema(
                    catalog,
                    script,
                    &script_version,
                    schema_limits,
                ) {
                    Ok(schema) => schema,
                    Err(error) => {
                        result.outcome =
                            ManagedBehaviourRecoveryOutcome::Unresolved(error.to_string());
                        report.behaviours.push(result);
                        continue;
                    }
                };
                let Some(object) = self.get(file_index, candidate.object_index) else {
                    result.outcome = ManagedBehaviourRecoveryOutcome::Unresolved(
                        "MonoBehaviour object is absent from ObjectWorkspace".to_owned(),
                    );
                    report.behaviours.push(result);
                    continue;
                };
                let Some(payload) = object.payload.as_ref().ok() else {
                    result.outcome = ManagedBehaviourRecoveryOutcome::Unresolved(
                        "MonoBehaviour generated header is unavailable".to_owned(),
                    );
                    report.behaviours.push(result);
                    continue;
                };
                let Some(structure) = object.factory.structure.as_ref() else {
                    result.outcome = ManagedBehaviourRecoveryOutcome::Unresolved(
                        "MonoBehaviour factory structure decision is unavailable".to_owned(),
                    );
                    report.behaviours.push(result);
                    continue;
                };
                let Some(raw) = file_bytes.get(
                    usize::try_from(object.raw_payload_range.start).unwrap_or(usize::MAX)
                        ..usize::try_from(object.raw_payload_range.end).unwrap_or(usize::MAX),
                ) else {
                    result.outcome = ManagedBehaviourRecoveryOutcome::Unresolved(
                        "MonoBehaviour raw payload range is outside the loaded file".to_owned(),
                    );
                    report.behaviours.push(result);
                    continue;
                };
                let Some(raw_remainder) = raw.get(payload.consumed_bytes..) else {
                    result.outcome = ManagedBehaviourRecoveryOutcome::Unresolved(
                        "generated header consumed more bytes than the object payload".to_owned(),
                    );
                    report.behaviours.push(result);
                    continue;
                };
                if raw_remainder != structure.unloaded_remainder.as_slice() {
                    result.outcome = ManagedBehaviourRecoveryOutcome::Unresolved(
                        "saved UnloadedStructure bytes differ from the source object suffix"
                            .to_owned(),
                    );
                    report.behaviours.push(result);
                    continue;
                }
                let mut decode_limits = limits.object_decode;
                let allocated_before = self
                    .report
                    .total_decoded_allocation_bytes
                    .checked_add(pending_allocation_bytes)
                    .ok_or_else(|| {
                        object_workspace_error(
                            file_index,
                            &virtual_path,
                            Some(candidate.object_index),
                            ObjectWorkspaceErrorKind::DecodedAllocationByteCountOverflow,
                        )
                    })?;
                decode_limits.max_decoded_allocation_bytes =
                    decode_limits.max_decoded_allocation_bytes.min(
                        limits
                            .max_total_decoded_allocation_bytes
                            .saturating_sub(allocated_before),
                    );
                let stream_offset = usize::try_from(object.raw_payload_range.start)
                    .ok()
                    .and_then(|start| start.checked_add(payload.consumed_bytes));
                let Some(stream_offset) = stream_offset else {
                    result.outcome = ManagedBehaviourRecoveryOutcome::Unresolved(
                        "MonoBehaviour script byte offset overflows address space".to_owned(),
                    );
                    report.behaviours.push(result);
                    continue;
                };
                let arrays_align = metadata::unity_version_aligns_arrays(
                    parsed_file.metadata.prefix.unity_version_text.as_deref(),
                );
                let managed_reference_type_resolver = |managed_script: &ScriptIdentifier| {
                    if catalog.find_type(managed_script).is_none() {
                        return Ok(None);
                    }
                    build_managed_reference_type_tree(
                        catalog,
                        managed_script,
                        &script_version,
                        schema_limits,
                    )
                    .map(Some)
                    .map_err(|error| error.to_string())
                };
                let decoded = decode_typetree_payload(
                    &schema.type_tree,
                    raw_remainder,
                    parsed_file.metadata.prefix.body_endian,
                    arrays_align,
                    &parsed_file.metadata.reference_types,
                    ManagedPayloadDecodeContext {
                        entry_index: candidate.object_index,
                        stream_offset,
                        managed_registry_depth: 0,
                        managed_reference_type_resolver: Some(&managed_reference_type_resolver),
                    },
                    &decode_limits,
                );
                let (structure_value, consumed_bytes, allocated_bytes) = match decoded {
                    Ok(decoded) => decoded,
                    Err(error) => {
                        result.outcome =
                            ManagedBehaviourRecoveryOutcome::Unresolved(error.to_string());
                        report.behaviours.push(result);
                        continue;
                    }
                };
                if consumed_bytes != raw_remainder.len() {
                    result.outcome = ManagedBehaviourRecoveryOutcome::Unresolved(format!(
                        "schema consumed {consumed_bytes} of {} UnloadedStructure bytes",
                        raw_remainder.len()
                    ));
                    report.behaviours.push(result);
                    continue;
                }
                if !matches!(&structure_value, SerializedValue::Struct(_)) {
                    result.outcome = ManagedBehaviourRecoveryOutcome::Unresolved(
                        "script schema did not decode to a structure".to_owned(),
                    );
                    report.behaviours.push(result);
                    continue;
                }
                let Some(root_fields) = payload.root.struct_fields() else {
                    result.outcome = ManagedBehaviourRecoveryOutcome::Unresolved(
                        "MonoBehaviour generated header root is not a structure".to_owned(),
                    );
                    report.behaviours.push(result);
                    continue;
                };
                if payload
                    .root
                    .unique_field("m_Structure")
                    .ok()
                    .flatten()
                    .is_some()
                {
                    result.outcome = ManagedBehaviourRecoveryOutcome::Unresolved(
                        "generated MonoBehaviour header already contains m_Structure".to_owned(),
                    );
                    report.behaviours.push(result);
                    continue;
                }
                let mut updated_payload = payload.clone();
                let Some(updated_field_capacity) = root_fields.len().checked_add(1) else {
                    return Err(object_workspace_error(
                        file_index,
                        &virtual_path,
                        Some(candidate.object_index),
                        ObjectWorkspaceErrorKind::AllocationFailed {
                            resource: "recovered MonoBehaviour fields",
                            requested: usize::MAX,
                        },
                    ));
                };
                let mut updated_fields = Vec::new();
                if updated_fields
                    .try_reserve_exact(updated_field_capacity)
                    .is_err()
                {
                    return Err(object_workspace_error(
                        file_index,
                        &virtual_path,
                        Some(candidate.object_index),
                        ObjectWorkspaceErrorKind::AllocationFailed {
                            resource: "recovered MonoBehaviour fields",
                            requested: updated_field_capacity,
                        },
                    ));
                }
                updated_fields.extend(root_fields.iter().cloned());
                updated_fields.push(SerializedFieldValue {
                    name: "m_Structure".to_owned(),
                    value: structure_value.clone(),
                });
                updated_payload.root = SerializedValue::Struct(updated_fields);
                updated_payload.consumed_bytes = updated_payload
                    .consumed_bytes
                    .checked_add(consumed_bytes)
                    .ok_or_else(|| {
                        object_workspace_error(
                            file_index,
                            &virtual_path,
                            Some(candidate.object_index),
                            ObjectWorkspaceErrorKind::DecodedAllocationByteCountOverflow,
                        )
                    })?;
                updated_payload.trailing_bytes.clear();
                updated_payload.allocated_bytes = updated_payload
                    .allocated_bytes
                    .checked_add(allocated_bytes)
                    .ok_or_else(|| {
                        object_workspace_error(
                            file_index,
                            &virtual_path,
                            Some(candidate.object_index),
                            ObjectWorkspaceErrorKind::DecodedAllocationByteCountOverflow,
                        )
                    })?;
                let mut updated_factory = object.factory.clone();
                let Some(updated_structure) = updated_factory.structure.as_mut() else {
                    result.outcome = ManagedBehaviourRecoveryOutcome::Unresolved(
                        "MonoBehaviour factory structure disappeared during recovery".to_owned(),
                    );
                    report.behaviours.push(result);
                    continue;
                };
                updated_structure.outcome = MonoBehaviourStructureOutcome::UseParsedStructure;
                updated_structure.fields = Some(structure_value);
                updated_structure.unloaded_remainder.clear();
                if !updated_factory.is_fully_materialized(Some(&updated_payload)) {
                    result.outcome = ManagedBehaviourRecoveryOutcome::Unresolved(
                        "recovered payload still fails the source materialization invariant"
                            .to_owned(),
                    );
                    report.behaviours.push(result);
                    continue;
                }
                pending_allocation_bytes = pending_allocation_bytes
                    .checked_add(allocated_bytes)
                    .ok_or_else(|| {
                        object_workspace_error(
                            file_index,
                            &virtual_path,
                            Some(candidate.object_index),
                            ObjectWorkspaceErrorKind::DecodedAllocationByteCountOverflow,
                        )
                    })?;
                overrides.push((candidate.object_index, updated_payload.clone()));
                pending.push(PendingManagedBehaviourRecovery {
                    object_index: candidate.object_index,
                    payload: updated_payload,
                    factory: updated_factory,
                    script: script.clone(),
                    script_version,
                    decoded_bytes: consumed_bytes,
                    skipped_fields: schema.skipped_fields,
                });
            }

            if !pending.is_empty() {
                let graph = ObjectReferenceGraph::build_with_source_type_tree_fallback_and_payload_overrides(
                    &parsed_file,
                    ReferenceGraphLimits::default(),
                    &file_name,
                    &virtual_path,
                    reference_class_layouts(),
                    &overrides,
                )
                .map_err(|source| {
                    object_workspace_error(
                        file_index,
                        &virtual_path,
                        None,
                        ObjectWorkspaceErrorKind::ReferenceGraphBuild(source),
                    )
                })?;
                if !workspace.replace_reference_graph(file_index, graph) {
                    return Err(object_workspace_error(
                        file_index,
                        &virtual_path,
                        None,
                        ObjectWorkspaceErrorKind::WorkspaceFileIndexOutOfBounds,
                    ));
                }
                for recovery in pending {
                    let Some(object) = self.objects.iter_mut().find(|object| {
                        object.file_index == file_index
                            && object.object_index == recovery.object_index
                    }) else {
                        return Err(object_workspace_error(
                            file_index,
                            &virtual_path,
                            Some(recovery.object_index),
                            ObjectWorkspaceErrorKind::WorkspaceObjectIndexOutOfBounds,
                        ));
                    };
                    object.payload = Ok(recovery.payload);
                    object.factory = recovery.factory;
                    report.behaviours.push(ManagedBehaviourRecovery {
                        file_index,
                        object_index: recovery.object_index,
                        path_id: object.path_id,
                        script: Some(recovery.script),
                        outcome: ManagedBehaviourRecoveryOutcome::Recovered {
                            script_version: recovery.script_version,
                            decoded_bytes: recovery.decoded_bytes,
                            skipped_fields: recovery.skipped_fields,
                        },
                    });
                }
            }
        }
        self.recount_report(limits)?;
        report
            .behaviours
            .sort_by_key(|record| (record.file_index, record.object_index));
        Ok(report)
    }
}
