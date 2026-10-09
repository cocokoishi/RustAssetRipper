//! Bounded decoding of all object records in a loaded SerializedFile workspace.

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;
use std::ops::Range;
use std::sync::Arc;

use crate::io::filesystem::VirtualFileSystem;
use crate::processing::asset_factory::{
    reference_class_layouts, ClassLayoutSource, FactoryAssetKind, FactoryEngineError,
    FactoryFileFacts, FactoryObjectMaterialization, FactoryObjectState, FactoryUnityVersion,
    MonoBehaviourStructureOutcome,
};
use crate::processing::managed_assembly::{
    resolve_mono_script_object, ManagedAssemblyCatalog, ManagedScriptResolutionOutcome,
};
use crate::processing::managed_assembly_schema::{
    build_managed_reference_type_tree, build_mono_behaviour_schema, ManagedSchemaLimits,
};
use crate::processing::script_identity::ScriptIdentifier;

use super::file::{SerializedFile, SerializedFileError};
use super::metadata::TypeTree;
use super::object::{
    decode_typetree_payload, ManagedPayloadDecodeContext, ObjectDecodeError, ObjectDecodeLimits,
    ParsedObjectPayload, SerializedFieldValue, SerializedValue,
};
use super::reference_graph::{
    MultiFilePPtrResolution, MultiFileReferenceGraphBuildError, MultiFileReferenceGraphLimits,
    ObjectReferenceGraph, ReferenceGraphBuildError, ReferenceGraphLimits,
};
use super::workspace::SerializedFileWorkspace;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ObjectWorkspaceLimits {
    pub max_objects: usize,
    pub max_file_bytes: u64,
    pub max_total_decoded_allocation_bytes: usize,
    pub object_decode: ObjectDecodeLimits,
}

impl Default for ObjectWorkspaceLimits {
    fn default() -> Self {
        Self {
            max_objects: 20_000_000,
            max_file_bytes: 2 * 1024 * 1024 * 1024,
            max_total_decoded_allocation_bytes: 2 * 1024 * 1024 * 1024,
            object_decode: ObjectDecodeLimits::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ObjectWorkspaceReport {
    pub object_count: usize,
    pub decoded_objects: usize,
    pub undecoded_objects: usize,
    pub total_decoded_allocation_bytes: usize,
    pub total_undecoded_raw_bytes: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ManagedBehaviourRecoveryReport {
    pub behaviours: Vec<ManagedBehaviourRecovery>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManagedBehaviourRecovery {
    pub file_index: usize,
    pub object_index: usize,
    pub path_id: i64,
    pub script: Option<crate::processing::script_identity::ScriptIdentifier>,
    pub outcome: ManagedBehaviourRecoveryOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ManagedBehaviourRecoveryOutcome {
    Recovered {
        script_version: String,
        decoded_bytes: usize,
        skipped_fields: Vec<String>,
    },
    Unresolved(String),
}

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

impl ObjectWorkspaceReport {
    pub fn is_complete(self) -> bool {
        self.undecoded_objects == 0 && self.decoded_objects == self.object_count
    }
}

/// One object identity and its TypeTree-decoded value or explicit decode failure.
///
/// `raw_payload_range` points back into the owning file in the source VFS. Failed objects retain
/// their original range without copying bytes or being silently replaced with an empty value.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceObject {
    pub file_index: usize,
    pub object_index: usize,
    pub path_id: i64,
    pub serialized_type_index: Option<i32>,
    pub raw_type_id: i32,
    pub resolved_type_index: Option<usize>,
    pub class_id: Option<i16>,
    pub script_type_index: Option<i16>,
    pub is_destroyed: Option<u16>,
    pub is_stripped: Option<bool>,
    pub raw_payload_range: Range<u64>,
    /// `GameAssetFactory` classification and class-specific read outcome. Raw bytes stay in the
    /// VFS and are available through `raw_payload_range`.
    pub factory: FactoryObjectState,
    /// Source TPK tree used for this object when its file has no usable tree.
    pub recovered_type_tree: Option<Arc<TypeTree>>,
    pub payload: Result<ParsedObjectPayload, SerializedFileError>,
}

impl WorkspaceObject {
    pub fn is_fully_materialized(&self) -> bool {
        self.factory
            .is_fully_materialized(self.payload.as_ref().ok())
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ObjectWorkspace {
    objects: Vec<WorkspaceObject>,
    report: ObjectWorkspaceReport,
}

impl ObjectWorkspace {
    /// Decode every object in workspace order. Unsupported or malformed TypeTrees remain as
    /// explicit per-object errors with raw VFS byte ranges; file-level identity changes are fatal.
    pub fn load_from_vfs(
        filesystem: &VirtualFileSystem,
        workspace: &SerializedFileWorkspace,
        limits: ObjectWorkspaceLimits,
    ) -> Result<Self, Box<ObjectWorkspaceError>> {
        Self::load_from_vfs_inner(filesystem, workspace, limits, reference_class_layouts())
    }

    pub fn load_from_vfs_with_layout_source(
        filesystem: &VirtualFileSystem,
        workspace: &SerializedFileWorkspace,
        limits: ObjectWorkspaceLimits,
        layouts: &dyn ClassLayoutSource,
    ) -> Result<Self, Box<ObjectWorkspaceError>> {
        Self::load_from_vfs_inner(filesystem, workspace, limits, layouts)
    }

    fn load_from_vfs_inner(
        filesystem: &VirtualFileSystem,
        workspace: &SerializedFileWorkspace,
        limits: ObjectWorkspaceLimits,
        layouts: &dyn ClassLayoutSource,
    ) -> Result<Self, Box<ObjectWorkspaceError>> {
        let mut report = ObjectWorkspaceReport::default();
        for (file_index, file) in workspace.files().iter().enumerate() {
            report.object_count = report
                .object_count
                .checked_add(file.metadata.objects.len())
                .ok_or_else(|| {
                    object_workspace_error(
                        file_index,
                        &file.virtual_path,
                        None,
                        ObjectWorkspaceErrorKind::ObjectCountOverflow,
                    )
                })?;
        }
        if report.object_count > limits.max_objects {
            let virtual_path = workspace
                .files()
                .first()
                .map(|file| file.virtual_path.as_str())
                .unwrap_or_default();
            return Err(object_workspace_error(
                0,
                virtual_path,
                None,
                ObjectWorkspaceErrorKind::ObjectCountLimitExceeded {
                    count: report.object_count,
                    limit: limits.max_objects,
                },
            ));
        }

        let mut objects = Vec::new();
        objects
            .try_reserve_exact(report.object_count)
            .map_err(|_| {
                object_workspace_error(
                    0,
                    "",
                    None,
                    ObjectWorkspaceErrorKind::AllocationFailed {
                        resource: "workspace decoded object records",
                        requested: report.object_count,
                    },
                )
            })?;

        for (file_index, file) in workspace.files().iter().enumerate() {
            let stream = filesystem.open_file(&file.virtual_path).map_err(|source| {
                object_workspace_error(
                    file_index,
                    &file.virtual_path,
                    None,
                    ObjectWorkspaceErrorKind::FileSystem(source),
                )
            })?;
            let file_bytes = stream.length().map_err(|source| {
                object_workspace_error(
                    file_index,
                    &file.virtual_path,
                    None,
                    ObjectWorkspaceErrorKind::FileSystem(source),
                )
            })?;
            if file_bytes > limits.max_file_bytes {
                return Err(object_workspace_error(
                    file_index,
                    &file.virtual_path,
                    None,
                    ObjectWorkspaceErrorKind::FileSizeLimitExceeded {
                        file_bytes,
                        limit: limits.max_file_bytes,
                    },
                ));
            }
            let maximum_bytes = usize::try_from(limits.max_file_bytes).unwrap_or(usize::MAX);
            let file_bytes = filesystem
                .read_all_bytes(&file.virtual_path, maximum_bytes)
                .map_err(|source| {
                    object_workspace_error(
                        file_index,
                        &file.virtual_path,
                        None,
                        ObjectWorkspaceErrorKind::FileSystem(source),
                    )
                })?;
            let parsed_file = SerializedFile::parse(&file_bytes).map_err(|source| {
                object_workspace_error(
                    file_index,
                    &file.virtual_path,
                    None,
                    ObjectWorkspaceErrorKind::SerializedFile(source),
                )
            })?;
            if parsed_file.header != file.header || parsed_file.metadata != file.metadata {
                return Err(object_workspace_error(
                    file_index,
                    &file.virtual_path,
                    None,
                    ObjectWorkspaceErrorKind::FileChangedSinceWorkspaceLoad,
                ));
            }

            for (object_index, object_info) in file.metadata.objects.iter().enumerate() {
                let raw_payload_range = object_info
                    .data_range(&file.header)
                    .filter(|range| {
                        range.end <= u64::try_from(file_bytes.len()).unwrap_or(u64::MAX)
                    })
                    .ok_or_else(|| {
                        object_workspace_error(
                            file_index,
                            &file.virtual_path,
                            Some(object_index),
                            ObjectWorkspaceErrorKind::InvalidObjectPayloadRange,
                        )
                    })?;
                let mut decode_limits = limits.object_decode;
                decode_limits.max_decoded_allocation_bytes =
                    decode_limits.max_decoded_allocation_bytes.min(
                        limits
                            .max_total_decoded_allocation_bytes
                            .saturating_sub(report.total_decoded_allocation_bytes),
                    );
                let platform = target_platform_label(parsed_file.metadata.prefix.target_platform);
                let materialization =
                    crate::processing::asset_factory::materialize_object_for_workspace(
                        &parsed_file,
                        &FactoryFileFacts {
                            collection_name: &file.file_name,
                            file_path: &file.virtual_path,
                            platform: &platform,
                        },
                        layouts,
                        object_index,
                        &decode_limits,
                    )
                    .map_err(|source| {
                        object_workspace_error(
                            file_index,
                            &file.virtual_path,
                            Some(object_index),
                            ObjectWorkspaceErrorKind::FactoryMaterialization(source),
                        )
                    })?;
                let FactoryObjectMaterialization {
                    state,
                    payload: factory_payload,
                } = materialization;
                let object_class_id = state.class_id;
                let is_mono_behaviour =
                    object_class_id == crate::processing::asset_factory::MONO_BEHAVIOUR_CLASS_ID;
                let has_file_type_tree = parsed_file
                    .metadata
                    .objects
                    .get(object_index)
                    .and_then(|object| object.resolved_type_index)
                    .and_then(|type_index| parsed_file.metadata.types.get(type_index))
                    .and_then(|serialized_type| serialized_type.type_tree.as_ref())
                    .is_some_and(|tree| !tree.nodes.is_empty());
                let mut recovered_type_tree = None;
                let payload = if is_mono_behaviour && has_file_type_tree {
                    parsed_file.decode_object_payload_with_limits(object_index, &decode_limits)
                } else if state.kind.is_recoverable() {
                    factory_payload.ok_or_else(|| factory_outcome_error(&state))
                } else {
                    Err(factory_outcome_error(&state))
                };
                if state.kind.is_recoverable() && !has_file_type_tree {
                    let version = parsed_file
                        .metadata
                        .prefix
                        .unity_version_text
                        .as_deref()
                        .and_then(FactoryUnityVersion::parse)
                        .unwrap_or_else(FactoryUnityVersion::stripped);
                    recovered_type_tree = layouts
                        .tpk_trees(object_class_id, &version)
                        .ok()
                        .flatten()
                        .map(|(release, editor)| {
                            if parsed_file
                                .object_read_flags(&file.virtual_path, &file.file_name)
                                .is_release()
                            {
                                release
                            } else {
                                editor
                            }
                        });
                }
                let fully_materialized = state.is_fully_materialized(payload.as_ref().ok());
                match &payload {
                    Ok(decoded) => {
                        report.total_decoded_allocation_bytes = report
                            .total_decoded_allocation_bytes
                            .checked_add(decoded.allocated_bytes)
                            .ok_or_else(|| {
                                object_workspace_error(
                                    file_index,
                                    &file.virtual_path,
                                    Some(object_index),
                                    ObjectWorkspaceErrorKind::DecodedAllocationByteCountOverflow,
                                )
                            })?;
                        if report.total_decoded_allocation_bytes
                            > limits.max_total_decoded_allocation_bytes
                        {
                            return Err(object_workspace_error(
                                file_index,
                                &file.virtual_path,
                                Some(object_index),
                                ObjectWorkspaceErrorKind::TotalDecodedAllocationLimitExceeded {
                                    byte_count: report.total_decoded_allocation_bytes,
                                    limit: limits.max_total_decoded_allocation_bytes,
                                },
                            ));
                        }
                        if fully_materialized {
                            report.decoded_objects =
                                report.decoded_objects.checked_add(1).ok_or_else(|| {
                                    object_workspace_error(
                                        file_index,
                                        &file.virtual_path,
                                        Some(object_index),
                                        ObjectWorkspaceErrorKind::ObjectCountOverflow,
                                    )
                                })?;
                        } else {
                            let raw_length =
                                usize::try_from(raw_payload_range.end - raw_payload_range.start)
                                    .map_err(|_| {
                                        object_workspace_error(
                                            file_index,
                                            &file.virtual_path,
                                            Some(object_index),
                                            ObjectWorkspaceErrorKind::UndecodedRawByteCountOverflow,
                                        )
                                    })?;
                            report.total_undecoded_raw_bytes = report
                                .total_undecoded_raw_bytes
                                .checked_add(raw_length)
                                .ok_or_else(|| {
                                    object_workspace_error(
                                        file_index,
                                        &file.virtual_path,
                                        Some(object_index),
                                        ObjectWorkspaceErrorKind::UndecodedRawByteCountOverflow,
                                    )
                                })?;
                            report.undecoded_objects =
                                report.undecoded_objects.checked_add(1).ok_or_else(|| {
                                    object_workspace_error(
                                        file_index,
                                        &file.virtual_path,
                                        Some(object_index),
                                        ObjectWorkspaceErrorKind::ObjectCountOverflow,
                                    )
                                })?;
                        }
                    }
                    Err(_) => {
                        let raw_length =
                            usize::try_from(raw_payload_range.end - raw_payload_range.start)
                                .map_err(|_| {
                                    object_workspace_error(
                                        file_index,
                                        &file.virtual_path,
                                        Some(object_index),
                                        ObjectWorkspaceErrorKind::UndecodedRawByteCountOverflow,
                                    )
                                })?;
                        report.total_undecoded_raw_bytes = report
                            .total_undecoded_raw_bytes
                            .checked_add(raw_length)
                            .ok_or_else(|| {
                                object_workspace_error(
                                    file_index,
                                    &file.virtual_path,
                                    Some(object_index),
                                    ObjectWorkspaceErrorKind::UndecodedRawByteCountOverflow,
                                )
                            })?;
                        report.undecoded_objects =
                            report.undecoded_objects.checked_add(1).ok_or_else(|| {
                                object_workspace_error(
                                    file_index,
                                    &file.virtual_path,
                                    Some(object_index),
                                    ObjectWorkspaceErrorKind::ObjectCountOverflow,
                                )
                            })?;
                    }
                }
                objects.push(WorkspaceObject {
                    file_index,
                    object_index,
                    path_id: object_info.path_id,
                    serialized_type_index: object_info.serialized_type_index,
                    raw_type_id: object_info.raw_type_id,
                    resolved_type_index: object_info.resolved_type_index,
                    class_id: object_info.class_id,
                    script_type_index: object_info.script_type_index,
                    is_destroyed: object_info.is_destroyed,
                    is_stripped: object_info.is_stripped,
                    raw_payload_range,
                    factory: state,
                    recovered_type_tree,
                    payload,
                });
            }
        }

        Ok(Self { objects, report })
    }

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
                let arrays_align = super::metadata::unity_version_aligns_arrays(
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

    fn recount_report(
        &mut self,
        limits: ObjectWorkspaceLimits,
    ) -> Result<(), Box<ObjectWorkspaceError>> {
        let mut report = ObjectWorkspaceReport::default();
        for object in &self.objects {
            report.object_count = report.object_count.checked_add(1).ok_or_else(|| {
                object_workspace_error(
                    object.file_index,
                    "<workspace>",
                    Some(object.object_index),
                    ObjectWorkspaceErrorKind::ObjectCountOverflow,
                )
            })?;
            match &object.payload {
                Ok(payload) => {
                    report.total_decoded_allocation_bytes = report
                        .total_decoded_allocation_bytes
                        .checked_add(payload.allocated_bytes)
                        .ok_or_else(|| {
                            object_workspace_error(
                                object.file_index,
                                "<workspace>",
                                Some(object.object_index),
                                ObjectWorkspaceErrorKind::DecodedAllocationByteCountOverflow,
                            )
                        })?;
                    if report.total_decoded_allocation_bytes
                        > limits.max_total_decoded_allocation_bytes
                    {
                        return Err(object_workspace_error(
                            object.file_index,
                            "<workspace>",
                            Some(object.object_index),
                            ObjectWorkspaceErrorKind::TotalDecodedAllocationLimitExceeded {
                                byte_count: report.total_decoded_allocation_bytes,
                                limit: limits.max_total_decoded_allocation_bytes,
                            },
                        ));
                    }
                    if object.is_fully_materialized() {
                        report.decoded_objects =
                            report.decoded_objects.checked_add(1).ok_or_else(|| {
                                object_workspace_error(
                                    object.file_index,
                                    "<workspace>",
                                    Some(object.object_index),
                                    ObjectWorkspaceErrorKind::ObjectCountOverflow,
                                )
                            })?;
                    } else {
                        report.undecoded_objects =
                            report.undecoded_objects.checked_add(1).ok_or_else(|| {
                                object_workspace_error(
                                    object.file_index,
                                    "<workspace>",
                                    Some(object.object_index),
                                    ObjectWorkspaceErrorKind::ObjectCountOverflow,
                                )
                            })?;
                        add_undecoded_raw_bytes(&mut report, object)?;
                    }
                }
                Err(_) => {
                    report.undecoded_objects =
                        report.undecoded_objects.checked_add(1).ok_or_else(|| {
                            object_workspace_error(
                                object.file_index,
                                "<workspace>",
                                Some(object.object_index),
                                ObjectWorkspaceErrorKind::ObjectCountOverflow,
                            )
                        })?;
                    add_undecoded_raw_bytes(&mut report, object)?;
                }
            }
        }
        self.report = report;
        Ok(())
    }

    pub fn objects(&self) -> &[WorkspaceObject] {
        &self.objects
    }

    /// Return the contiguous object records for one workspace file.
    ///
    /// `load_from_vfs` stores these records by ascending `(file_index, object_index)`, so this
    /// avoids rescanning unrelated files when a caller processes scene data file by file.
    pub fn objects_for_file(&self, file_index: usize) -> &[WorkspaceObject] {
        let start = self
            .objects
            .partition_point(|object| object.file_index < file_index);
        let end =
            start + self.objects[start..].partition_point(|object| object.file_index == file_index);
        &self.objects[start..end]
    }

    /// Find an object by its stable workspace file/object-table indexes.
    ///
    /// Objects are stored in ascending file index and then object index order by
    /// `load_from_vfs`, so this lookup does not depend on PathID uniqueness.
    pub fn get(&self, file_index: usize, object_index: usize) -> Option<&WorkspaceObject> {
        let position = self
            .objects
            .binary_search_by_key(&(file_index, object_index), |object| {
                (object.file_index, object.object_index)
            })
            .ok()?;
        self.objects.get(position)
    }

    pub fn report(&self) -> ObjectWorkspaceReport {
        self.report
    }
}

#[derive(Debug)]
pub struct ObjectWorkspaceError {
    pub file_index: usize,
    pub virtual_path: String,
    pub object_index: Option<usize>,
    pub kind: ObjectWorkspaceErrorKind,
}

#[derive(Debug)]
pub enum ObjectWorkspaceErrorKind {
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    ObjectCountOverflow,
    ObjectCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    FileSizeLimitExceeded {
        file_bytes: u64,
        limit: u64,
    },
    FileChangedSinceWorkspaceLoad,
    InvalidObjectPayloadRange,
    FactoryMaterialization(FactoryEngineError),
    DecodedAllocationByteCountOverflow,
    TotalDecodedAllocationLimitExceeded {
        byte_count: usize,
        limit: usize,
    },
    UndecodedRawByteCountOverflow,
    FileSystem(io::Error),
    SerializedFile(SerializedFileError),
    MultiFileReferenceGraphBuild(MultiFileReferenceGraphBuildError),
    ReferenceGraphBuild(ReferenceGraphBuildError),
    WorkspaceFileIndexOutOfBounds,
    WorkspaceObjectIndexOutOfBounds,
}

impl Display for ObjectWorkspaceError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "object workspace file {} {:?}",
            self.file_index, self.virtual_path
        )?;
        if let Some(object_index) = self.object_index {
            write!(f, " object {object_index}")?;
        }
        write!(f, ": {}", self.kind)
    }
}

impl Display for ObjectWorkspaceErrorKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AllocationFailed {
                resource,
                requested,
            } => write!(f, "could not allocate {requested} entries for {resource}"),
            Self::ObjectCountOverflow => f.write_str("workspace object count overflows"),
            Self::ObjectCountLimitExceeded { count, limit } => {
                write!(
                    f,
                    "workspace contains {count} objects, exceeding limit {limit}"
                )
            }
            Self::FileSizeLimitExceeded { file_bytes, limit } => {
                write!(f, "input uses {file_bytes} bytes, exceeding limit {limit}")
            }
            Self::FileChangedSinceWorkspaceLoad => {
                f.write_str("input metadata changed after SerializedFileWorkspace loading")
            }
            Self::InvalidObjectPayloadRange => {
                f.write_str("object payload range overflows or is outside the input file")
            }
            Self::FactoryMaterialization(error) => {
                write!(f, "source asset-factory materialization failed: {error}")
            }
            Self::DecodedAllocationByteCountOverflow => {
                f.write_str("aggregate decoded allocation byte count overflows")
            }
            Self::TotalDecodedAllocationLimitExceeded { byte_count, limit } => write!(
                f,
                "decoded objects use {byte_count} bytes, exceeding aggregate limit {limit}"
            ),
            Self::UndecodedRawByteCountOverflow => {
                f.write_str("aggregate undecoded raw payload byte count overflows")
            }
            Self::FileSystem(error) => write!(f, "VFS error: {error}"),
            Self::SerializedFile(error) => write!(f, "could not parse source object file: {error}"),
            Self::MultiFileReferenceGraphBuild(error) => {
                write!(f, "could not build joined reference graph: {error}")
            }
            Self::ReferenceGraphBuild(error) => {
                write!(f, "could not rebuild recovered reference graph: {error}")
            }
            Self::WorkspaceFileIndexOutOfBounds => {
                f.write_str("SerializedFile workspace index is out of bounds")
            }
            Self::WorkspaceObjectIndexOutOfBounds => {
                f.write_str("ObjectWorkspace index is out of bounds")
            }
        }
    }
}

impl Error for ObjectWorkspaceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            ObjectWorkspaceErrorKind::FileSystem(error) => Some(error),
            ObjectWorkspaceErrorKind::FactoryMaterialization(error) => Some(error),
            ObjectWorkspaceErrorKind::SerializedFile(error) => Some(error),
            ObjectWorkspaceErrorKind::MultiFileReferenceGraphBuild(error) => Some(error),
            ObjectWorkspaceErrorKind::ReferenceGraphBuild(error) => Some(error),
            _ => None,
        }
    }
}

fn object_workspace_error(
    file_index: usize,
    virtual_path: &str,
    object_index: Option<usize>,
    kind: ObjectWorkspaceErrorKind,
) -> Box<ObjectWorkspaceError> {
    Box::new(ObjectWorkspaceError {
        file_index,
        virtual_path: virtual_path.to_owned(),
        object_index,
        kind,
    })
}

fn add_undecoded_raw_bytes(
    report: &mut ObjectWorkspaceReport,
    object: &WorkspaceObject,
) -> Result<(), Box<ObjectWorkspaceError>> {
    let raw_byte_count = object
        .raw_payload_range
        .end
        .checked_sub(object.raw_payload_range.start)
        .and_then(|byte_count| usize::try_from(byte_count).ok())
        .ok_or_else(|| {
            object_workspace_error(
                object.file_index,
                "<workspace>",
                Some(object.object_index),
                ObjectWorkspaceErrorKind::UndecodedRawByteCountOverflow,
            )
        })?;
    report.total_undecoded_raw_bytes = report
        .total_undecoded_raw_bytes
        .checked_add(raw_byte_count)
        .ok_or_else(|| {
            object_workspace_error(
                object.file_index,
                "<workspace>",
                Some(object.object_index),
                ObjectWorkspaceErrorKind::UndecodedRawByteCountOverflow,
            )
        })?;
    Ok(())
}

fn factory_outcome_error(state: &FactoryObjectState) -> SerializedFileError {
    let outcome = match &state.kind {
        FactoryAssetKind::Generated(name) => {
            format!("generated class {name} without decoded values")
        }
        FactoryAssetKind::TypeTreeObject => "TypeTreeObject without decoded values".to_owned(),
        FactoryAssetKind::Unknown => "UnknownObject raw fallback".to_owned(),
        FactoryAssetKind::Unreadable => "UnreadableObject raw fallback".to_owned(),
    };
    SerializedFileError::from_object(ObjectDecodeError::factory_object_outcome(
        state.object_index,
        state.class_id,
        outcome,
        state.log_message.clone(),
    ))
}

fn target_platform_label(platform: Option<u32>) -> String {
    match platform {
        Some(crate::serialized_file::object::BUILD_TARGET_NO_TARGET) => "NoTarget".to_owned(),
        Some(19) => "StandaloneWin64Player".to_owned(),
        Some(value) => format!("BuildTarget({value})"),
        None => "Unknown".to_owned(),
    }
}
