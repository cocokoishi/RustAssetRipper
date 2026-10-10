use super::error::{object_workspace_error, ObjectWorkspaceError, ObjectWorkspaceErrorKind};
use super::{ObjectWorkspace, ObjectWorkspaceLimits, ObjectWorkspaceReport, WorkspaceObject};

impl ObjectWorkspace {
    pub(super) fn recount_report(
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

pub(super) fn add_undecoded_raw_bytes(
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
