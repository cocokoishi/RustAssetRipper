use super::error::{
    factory_outcome_error, object_workspace_error, target_platform_label, ObjectWorkspaceError,
    ObjectWorkspaceErrorKind,
};
use super::{ObjectWorkspace, ObjectWorkspaceLimits, ObjectWorkspaceReport, WorkspaceObject};
use crate::io::filesystem::VirtualFileSystem;
use crate::processing::asset_factory::{
    reference_class_layouts, ClassLayoutSource, FactoryFileFacts, FactoryObjectMaterialization,
    FactoryUnityVersion,
};
use crate::serialized_file::file::SerializedFile;
use crate::serialized_file::workspace::SerializedFileWorkspace;
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
}
