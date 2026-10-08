//! Bounded decoding of all object records in a loaded SerializedFile workspace.

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;
use std::ops::Range;
use std::sync::Arc;

use crate::io::filesystem::VirtualFileSystem;

use super::file::{SerializedFile, SerializedFileError, SerializedFileErrorKind};
use super::metadata::TypeTree;
use super::object::{ObjectDecodeErrorKind, ObjectDecodeLimits, ParsedObjectPayload};
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
    /// Source TPK tree used for this object when its file has no usable tree.
    pub recovered_type_tree: Option<Arc<TypeTree>>,
    pub payload: Result<ParsedObjectPayload, SerializedFileError>,
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
                let mut recovered_type_tree = None;
                let payload = match parsed_file
                    .decode_object_payload_with_limits(object_index, &decode_limits)
                {
                    Ok(payload) => Ok(payload),
                    Err(original_error) if missing_type_tree_error(&original_error) => {
                        match crate::processing::asset_factory::decode_no_usable_type_tree_payload(
                            &parsed_file,
                            object_index,
                            &file.file_name,
                            &file.virtual_path,
                            crate::processing::asset_factory::reference_class_layouts(),
                            &decode_limits,
                        ) {
                            Ok(Some(recovery)) => {
                                recovered_type_tree = Some(recovery.type_tree);
                                Ok(recovery.payload)
                            }
                            Ok(None) => Err(original_error),
                            Err(fallback_error) => {
                                Err(SerializedFileError::from_object(fallback_error))
                            }
                        }
                    }
                    Err(error) => Err(error),
                };
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
                        report.decoded_objects =
                            report.decoded_objects.checked_add(1).ok_or_else(|| {
                                object_workspace_error(
                                    file_index,
                                    &file.virtual_path,
                                    Some(object_index),
                                    ObjectWorkspaceErrorKind::ObjectCountOverflow,
                                )
                            })?;
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
                    recovered_type_tree,
                    payload,
                });
            }
        }

        Ok(Self { objects, report })
    }

    pub fn objects(&self) -> &[WorkspaceObject] {
        &self.objects
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
    DecodedAllocationByteCountOverflow,
    TotalDecodedAllocationLimitExceeded {
        byte_count: usize,
        limit: usize,
    },
    UndecodedRawByteCountOverflow,
    FileSystem(io::Error),
    SerializedFile(SerializedFileError),
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
        }
    }
}

impl Error for ObjectWorkspaceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            ObjectWorkspaceErrorKind::FileSystem(error) => Some(error),
            ObjectWorkspaceErrorKind::SerializedFile(error) => Some(error),
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

fn missing_type_tree_error(error: &SerializedFileError) -> bool {
    matches!(
        error.kind(),
        SerializedFileErrorKind::Object(object_error)
            if matches!(
                object_error.kind(),
                ObjectDecodeErrorKind::MissingTypeTree { .. }
                    | ObjectDecodeErrorKind::EmptyTypeTree { .. }
            )
    )
}
