//! Bounded decoding of object records in a loaded SerializedFile workspace.

use crate::processing::asset_factory::FactoryObjectState;
use crate::serialized_file::file::SerializedFileError;
use crate::serialized_file::metadata::TypeTree;
use crate::serialized_file::object::{ObjectDecodeLimits, ParsedObjectPayload};
use std::ops::Range;
use std::sync::Arc;

mod error;
mod loading;
mod recovery;
mod report;

pub use error::{ObjectWorkspaceError, ObjectWorkspaceErrorKind};

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
