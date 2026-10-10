use super::*;
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceSceneHierarchyError {
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
    CountOverflow {
        resource: &'static str,
    },
    ResourceLimitExceeded {
        resource: &'static str,
        requested: usize,
        limit: usize,
    },
    WorkspaceFileCountMismatch {
        count: usize,
        limit: usize,
    },
    WorkspaceDefinitionCollectionCountMismatch {
        expected: usize,
        actual: usize,
    },
    WorkspaceCollectionGraphIndexCountMismatch {
        expected: usize,
        actual: usize,
    },
    SceneCountLimitExceeded {
        count: usize,
        limit: usize,
    },
    CollectionFileIndexOutOfBounds {
        file_index: usize,
        file_count: usize,
    },
    DuplicateCollectionGraph {
        file_index: usize,
    },
    DuplicateGeneratedTransformSource {
        file_index: usize,
    },
    MissingCollectionGraph {
        scene_index: usize,
        file_index: usize,
    },
    InvalidSourceIdentity {
        file_index: usize,
        object_index: usize,
        path_id: i64,
    },
    InvalidRelationshipSource {
        file_index: usize,
        relationship_index: usize,
        object_index: usize,
    },
    PrefabRootRelationshipSourceMissing {
        file_index: usize,
        relationship_index: usize,
        object_index: usize,
    },
    DuplicateGameObjectHierarchy {
        file_index: usize,
        object_index: usize,
    },
    GameObjectComponentRelationshipSourceMissing {
        file_index: usize,
        relationship_index: usize,
        object_index: usize,
    },
    GameObjectHierarchyIndexOutOfBounds {
        hierarchy_index: usize,
        hierarchy_count: usize,
    },
    MissingGameObjectHierarchy {
        file_index: usize,
        object_index: usize,
    },
    InvalidGameObjectComponentRelationshipRange {
        hierarchy_index: usize,
        start: usize,
        end: usize,
        index_count: usize,
    },
    InvalidGameObjectComponentRelationshipIndex {
        hierarchy_index: usize,
        relationship_index: usize,
        relationship_count: usize,
    },
    DuplicateTransformHierarchy {
        file_index: usize,
        object_index: usize,
    },
    InvalidTransformHierarchySource {
        file_index: usize,
        relationship_index: usize,
        object_index: usize,
    },
    TransformHierarchyIndexOutOfBounds {
        hierarchy_index: usize,
        hierarchy_count: usize,
    },
    InvalidTransformParentRelationshipRange {
        hierarchy_index: usize,
        start: usize,
        end: usize,
        index_count: usize,
    },
    InvalidTransformChildRelationshipRange {
        hierarchy_index: usize,
        start: usize,
        end: usize,
        index_count: usize,
    },
    InvalidTransformGameObjectRelationshipRange {
        hierarchy_index: usize,
        start: usize,
        end: usize,
        index_count: usize,
    },
    InvalidTransformRelationshipIndex {
        hierarchy_index: usize,
        relationship_index: usize,
        relationship_count: usize,
    },
    InvalidComponentTarget {
        source_file_index: usize,
        target_file_index: usize,
        object_index: usize,
    },
    InvalidRootIdentity {
        file_index: usize,
        object_index: usize,
    },
    InvalidRootTransformIdentity {
        file_index: usize,
        transform_file_index: usize,
        transform_index: usize,
    },
    RootSourceFileMismatch {
        expected: usize,
        actual: usize,
    },
    AuditSourceFileMismatch {
        expected: usize,
        actual: usize,
    },
}

impl Display for WorkspaceSceneHierarchyError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AllocationFailed { resource, requested } => {
                write!(f, "could not allocate {requested} entries for {resource}")
            }
            Self::CountOverflow { resource } => write!(f, "{resource} count overflows usize"),
            Self::ResourceLimitExceeded {
                resource,
                requested,
                limit,
            } => write!(f, "{resource} count {requested} exceeds limit {limit}"),
            Self::WorkspaceFileCountMismatch { count, limit } => {
                write!(f, "workspace file count {count} exceeds limit {limit}")
            }
            Self::WorkspaceDefinitionCollectionCountMismatch { expected, actual } => write!(
                f,
                "generated-transform planning expected {expected} workspace collections, definitions contain {actual}"
            ),
            Self::WorkspaceCollectionGraphIndexCountMismatch { expected, actual } => write!(
                f,
                "workspace collection graph index expected {expected} entries, contains {actual}"
            ),
            Self::SceneCountLimitExceeded { count, limit } => {
                write!(f, "SceneDefinition count {count} exceeds limit {limit}")
            }
            Self::CollectionFileIndexOutOfBounds {
                file_index,
                file_count,
            } => write!(f, "scene collection file index {file_index} is outside {file_count} files"),
            Self::DuplicateCollectionGraph { file_index } => {
                write!(f, "scene collection file {file_index} has more than one graph")
            }
            Self::DuplicateGeneratedTransformSource { file_index } => write!(
                f,
                "workspace file {file_index} supplied generated-transform candidates more than once"
            ),
            Self::MissingCollectionGraph {
                scene_index,
                file_index,
            } => write!(
                f,
                "SceneDefinition {scene_index} has no recovered graph for collection file {file_index}"
            ),
            Self::InvalidSourceIdentity {
                file_index,
                object_index,
                path_id,
            } => write!(
                f,
                "scene graph object {file_index}:{object_index} PathID {path_id} does not match the joined workspace"
            ),
            Self::InvalidRelationshipSource {
                file_index,
                relationship_index,
                object_index,
            } => write!(
                f,
                "scene relationship {relationship_index} source object {file_index}:{object_index} is invalid"
            ),
            Self::PrefabRootRelationshipSourceMissing {
                file_index,
                relationship_index,
                object_index,
            } => write!(
                f,
                "PrefabInstance root relationship {relationship_index} in collection {file_index} has no matching PrefabInstance at object {object_index}"
            ),
            Self::DuplicateGameObjectHierarchy {
                file_index,
                object_index,
            } => write!(
                f,
                "scene collection {file_index} contains GameObject {object_index} more than once"
            ),
            Self::GameObjectComponentRelationshipSourceMissing {
                file_index,
                relationship_index,
                object_index,
            } => write!(
                f,
                "GameObject component relationship {relationship_index} in collection {file_index} has no indexed GameObject source at object {object_index}"
            ),
            Self::GameObjectHierarchyIndexOutOfBounds {
                hierarchy_index,
                hierarchy_count,
            } => write!(
                f,
                "GameObject hierarchy index {hierarchy_index} is outside {hierarchy_count} entries"
            ),
            Self::MissingGameObjectHierarchy {
                file_index,
                object_index,
            } => write!(
                f,
                "collection {file_index} has no component list for GameObject {object_index}"
            ),
            Self::InvalidGameObjectComponentRelationshipRange {
                hierarchy_index,
                start,
                end,
                index_count,
            } => write!(
                f,
                "GameObject hierarchy {hierarchy_index} component relationship range {start}..{end} is outside {index_count} indexes"
            ),
            Self::InvalidGameObjectComponentRelationshipIndex {
                hierarchy_index,
                relationship_index,
                relationship_count,
            } => write!(
                f,
                "GameObject hierarchy {hierarchy_index} references relationship {relationship_index}, but only {relationship_count} relationships exist"
            ),
            Self::DuplicateTransformHierarchy {
                file_index,
                object_index,
            } => write!(
                f,
                "scene collection {file_index} contains Transform {object_index} more than once"
            ),
            Self::InvalidTransformHierarchySource {
                file_index,
                relationship_index,
                object_index,
            } => write!(
                f,
                "Transform hierarchy relationship {relationship_index} in collection {file_index} has no indexed Transform source at object {object_index}"
            ),
            Self::TransformHierarchyIndexOutOfBounds {
                hierarchy_index,
                hierarchy_count,
            } => write!(
                f,
                "Transform hierarchy index {hierarchy_index} is outside {hierarchy_count} entries"
            ),
            Self::InvalidTransformParentRelationshipRange {
                hierarchy_index,
                start,
                end,
                index_count,
            } => write!(
                f,
                "Transform hierarchy {hierarchy_index} parent relationship range {start}..{end} is outside {index_count} indexes"
            ),
            Self::InvalidTransformChildRelationshipRange {
                hierarchy_index,
                start,
                end,
                index_count,
            } => write!(
                f,
                "Transform hierarchy {hierarchy_index} child relationship range {start}..{end} is outside {index_count} indexes"
            ),
            Self::InvalidTransformGameObjectRelationshipRange {
                hierarchy_index,
                start,
                end,
                index_count,
            } => write!(
                f,
                "Transform hierarchy {hierarchy_index} GameObject relationship range {start}..{end} is outside {index_count} indexes"
            ),
            Self::InvalidTransformRelationshipIndex {
                hierarchy_index,
                relationship_index,
                relationship_count,
            } => write!(
                f,
                "Transform hierarchy {hierarchy_index} references relationship {relationship_index}, but only {relationship_count} relationships exist"
            ),
            Self::InvalidComponentTarget {
                source_file_index,
                target_file_index,
                object_index,
            } => write!(
                f,
                "scene collection {source_file_index} component edge targets invalid object {target_file_index}:{object_index}"
            ),
            Self::InvalidRootIdentity {
                file_index,
                object_index,
            } => write!(
                f,
                "scene root candidate GameObject {file_index}:{object_index} is invalid"
            ),
            Self::InvalidRootTransformIdentity {
                file_index,
                transform_file_index,
                transform_index,
            } => write!(
                f,
                "scene root candidate Transform {transform_file_index}:{transform_index} from file {file_index} is invalid"
            ),
            Self::RootSourceFileMismatch { expected, actual } => write!(
                f,
                "root discovery belongs to source file {actual}, expected {expected}"
            ),
            Self::AuditSourceFileMismatch { expected, actual } => write!(
                f,
                "hierarchy audit contains source file {actual}, expected {expected}"
            ),
        }
    }
}

impl Error for WorkspaceSceneHierarchyError {}
