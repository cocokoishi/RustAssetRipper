use crate::processing::asset_factory::{FactoryAssetKind, FactoryEngineError, FactoryObjectState};
use crate::serialized_file::file::{SerializedFileError, SerializedFileStreamError};
use crate::serialized_file::object::ObjectDecodeError;
use crate::serialized_file::reference_graph::{
    MultiFileReferenceGraphBuildError, ReferenceGraphBuildError,
};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;
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
    StreamSerializedFile(SerializedFileStreamError),
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
            Self::StreamSerializedFile(error) => {
                write!(f, "could not parse file-backed source object file: {error}")
            }
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
            ObjectWorkspaceErrorKind::StreamSerializedFile(error) => Some(error),
            ObjectWorkspaceErrorKind::MultiFileReferenceGraphBuild(error) => Some(error),
            ObjectWorkspaceErrorKind::ReferenceGraphBuild(error) => Some(error),
            _ => None,
        }
    }
}

pub(super) fn object_workspace_error(
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

pub(super) fn factory_outcome_error(state: &FactoryObjectState) -> SerializedFileError {
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

pub(super) fn target_platform_label(platform: Option<u32>) -> String {
    match platform {
        Some(crate::serialized_file::object::BUILD_TARGET_NO_TARGET) => "NoTarget".to_owned(),
        Some(19) => "StandaloneWin64Player".to_owned(),
        Some(value) => format!("BuildTarget({value})"),
        None => "Unknown".to_owned(),
    }
}
