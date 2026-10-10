use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SceneGraphBuildError {
    ObjectCountLimitExceeded {
        object_count: usize,
        limit: usize,
    },
    RelationshipLimitExceeded {
        requested_count: usize,
        limit: usize,
    },
    IssueLimitExceeded {
        requested_count: usize,
        limit: usize,
    },
    ResourceLimitExceeded {
        resource: &'static str,
        requested: usize,
        limit: usize,
    },
    CountOverflow {
        resource: &'static str,
    },
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    InvalidObjectIndex {
        object_index: usize,
        object_count: usize,
    },
    DecodedRootCountMismatch {
        expected: usize,
        actual: usize,
    },
    DuplicateDecodedRoot {
        object_index: usize,
    },
    UnexpectedDecodedRoot {
        object_index: usize,
        class_id: Option<i32>,
    },
    MaterializedObjectCountMismatch {
        expected: usize,
        actual: usize,
    },
    MaterializedObjectIdentityMismatch {
        object_index: usize,
        expected_file_index: usize,
        actual_file_index: usize,
        actual_object_index: usize,
        expected_path_id: i64,
        actual_path_id: i64,
    },
    MaterializedPayloadIdentityMismatch {
        object_index: usize,
        expected_path_id: i64,
        payload_object_index: usize,
        payload_path_id: i64,
    },
}

impl Display for SceneGraphBuildError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ObjectCountLimitExceeded { object_count, limit } => write!(
                f,
                "scene graph object count {object_count} exceeds configured limit {limit}"
            ),
            Self::RelationshipLimitExceeded {
                requested_count,
                limit,
            } => write!(
                f,
                "scene graph relationship count {requested_count} exceeds configured limit {limit}"
            ),
            Self::IssueLimitExceeded {
                requested_count,
                limit,
            } => write!(
                f,
                "scene graph issue count {requested_count} exceeds configured limit {limit}"
            ),
            Self::ResourceLimitExceeded {
                resource,
                requested,
                limit,
            } => write!(
                f,
                "scene graph requires {requested} units for {resource}, above configured limit {limit}"
            ),
            Self::CountOverflow { resource } => {
                write!(f, "scene graph count overflows for {resource}")
            }
            Self::AllocationFailed {
                resource,
                requested,
            } => write!(
                f,
                "could not reserve {requested} entries for scene graph {resource}"
            ),
            Self::InvalidObjectIndex {
                object_index,
                object_count,
            } => write!(
                f,
                "decoded scene root refers to object index {object_index}, but the table has {object_count} objects"
            ),
            Self::DecodedRootCountMismatch { expected, actual } => write!(
                f,
                "decoded scene roots contain {actual} entries, but the object table has {expected} scene objects"
            ),
            Self::DuplicateDecodedRoot { object_index } => write!(
                f,
                "decoded scene roots contain object index {object_index} more than once"
            ),
            Self::UnexpectedDecodedRoot {
                object_index,
                class_id,
            } => write!(
                f,
                "decoded scene root at object index {object_index} has non-scene class ID {class_id:?}"
            ),
            Self::MaterializedObjectCountMismatch { expected, actual } => write!(
                f,
                "materialized workspace contains {actual} objects, but the source file has {expected} object records"
            ),
            Self::MaterializedObjectIdentityMismatch {
                object_index,
                expected_file_index,
                actual_file_index,
                actual_object_index,
                expected_path_id,
                actual_path_id,
            } => write!(
                f,
                "materialized object at file {actual_file_index}, object {actual_object_index}, PathID {actual_path_id} does not match file {expected_file_index}, object {object_index}, PathID {expected_path_id}"
            ),
            Self::MaterializedPayloadIdentityMismatch {
                object_index,
                expected_path_id,
                payload_object_index,
                payload_path_id,
            } => write!(
                f,
                "materialized payload at object {payload_object_index}, PathID {payload_path_id} does not match object {object_index}, PathID {expected_path_id}"
            ),
        }
    }
}

impl Error for SceneGraphBuildError {}
