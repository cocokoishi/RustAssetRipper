//! File-scoped GameObject and Transform relationship recovery.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::serialized_file::file::SerializedFile;
use crate::serialized_file::metadata::{ObjectInfo, SerializedFileMetadata};
use crate::serialized_file::object::{
    ObjectDecodeLimits, ParsedObjectPayload, SerializedValue, SerializedValueLookupError,
};

const GAME_OBJECT_CLASS_ID: i32 = 1;
const TRANSFORM_CLASS_ID: i32 = 4;
const RECT_TRANSFORM_CLASS_ID: i32 = 224;

/// Bounds for building one file-scoped scene relationship graph.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SceneGraphLimits {
    pub max_objects: usize,
    pub max_relationships: usize,
    pub max_issues: usize,
    pub max_total_decoded_bytes: usize,
    pub max_total_name_bytes: usize,
    pub max_total_field_path_bytes: usize,
    pub object_decode: ObjectDecodeLimits,
}

impl Default for SceneGraphLimits {
    fn default() -> Self {
        Self {
            max_objects: 4_000_000,
            max_relationships: 2_000_000,
            max_issues: 100_000,
            max_total_decoded_bytes: 2 * 1024 * 1024 * 1024,
            max_total_name_bytes: 64 * 1024 * 1024,
            max_total_field_path_bytes: 64 * 1024 * 1024,
            object_decode: ObjectDecodeLimits::default(),
        }
    }
}

/// One object-table identity, retaining the source PathID and table index.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SceneObjectRecord {
    pub object_index: usize,
    pub path_id: i64,
    pub class_id: Option<i32>,
    /// Raw serialized UTF-8 bytes are kept so malformed input is not silently rewritten.
    pub name_bytes: Option<Vec<u8>>,
    pub is_active: Option<bool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SceneRelationshipKind {
    GameObjectComponent,
    ComponentGameObject,
    TransformParent,
    TransformChild,
}

/// Explicit result of resolving one recovered scene pointer within its source file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SceneRelationshipTarget {
    Null,
    LocalObject {
        object_index: usize,
    },
    MissingLocalObject {
        path_id: i64,
    },
    AmbiguousLocalObject {
        path_id: i64,
        match_count: usize,
    },
    ExternalObject {
        file_id: i32,
        external_index: usize,
        path_id: i64,
    },
    InvalidFileId {
        file_id: i32,
        external_count: usize,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SceneRelationship {
    pub source_object_index: usize,
    pub source_path_id: i64,
    pub kind: SceneRelationshipKind,
    /// Field path includes zero-based array indexes where the source value was an array.
    pub field_path: String,
    pub file_id: i32,
    pub path_id: i64,
    pub target: SceneRelationshipTarget,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SceneGraphIssue {
    pub object_index: usize,
    pub path_id: i64,
    pub field_path: Option<String>,
    pub kind: SceneGraphIssueKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SceneGraphIssueKind {
    ObjectDecodeFailed {
        message: String,
    },
    MissingField {
        field_name: String,
    },
    DuplicateField {
        field_name: String,
        match_count: usize,
    },
    UnexpectedValueType {
        expected: &'static str,
        actual: &'static str,
    },
}

/// File-scoped scene nodes and relationships recovered from GameObject/Transform TypeTrees.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SceneGraph {
    objects: Vec<SceneObjectRecord>,
    relationships: Vec<SceneRelationship>,
    issues: Vec<SceneGraphIssue>,
}

impl SceneGraph {
    /// Decode GameObject, Transform, and RectTransform payloads from one parsed SerializedFile.
    ///
    /// Other object records remain in `objects` as potential relationship targets. This stage
    /// does not load external files or synthesize reciprocal relationships.
    pub fn build(
        file: &SerializedFile<'_>,
        limits: SceneGraphLimits,
    ) -> Result<Self, SceneGraphBuildError> {
        let mut builder = SceneGraphBuilder::new(&file.metadata, limits)?;
        for (object_index, object) in file.metadata.objects.iter().enumerate() {
            if !is_scene_object_class(object_scene_class_id(object)) {
                continue;
            }
            match file.decode_object_payload_with_limits(object_index, &limits.object_decode) {
                Ok(payload) => builder.add_payload(&payload)?,
                Err(error) => builder.push_issue(SceneGraphIssue {
                    object_index,
                    path_id: object.path_id,
                    field_path: None,
                    kind: SceneGraphIssueKind::ObjectDecodeFailed {
                        message: error.to_string(),
                    },
                })?,
            }
        }
        builder.finish()
    }

    /// Build from already decoded roots, useful to processors that share an object decode pass.
    /// Every supplied root must correspond to the matching object-table index.
    pub fn from_decoded_roots(
        metadata: &SerializedFileMetadata,
        roots: &[(usize, SerializedValue)],
        limits: SceneGraphLimits,
    ) -> Result<Self, SceneGraphBuildError> {
        let mut builder = SceneGraphBuilder::new(metadata, limits)?;
        let expected_root_count = metadata
            .objects
            .iter()
            .filter(|object| is_scene_object_class(object_scene_class_id(object)))
            .count();
        if roots.len() != expected_root_count {
            return Err(SceneGraphBuildError::DecodedRootCountMismatch {
                expected: expected_root_count,
                actual: roots.len(),
            });
        }
        let mut seen = Vec::new();
        seen.try_reserve_exact(metadata.objects.len())
            .map_err(|_| SceneGraphBuildError::AllocationFailed {
                resource: "decoded scene root index",
                requested: metadata.objects.len(),
            })?;
        seen.resize(metadata.objects.len(), false);
        for (object_index, root) in roots {
            let object = metadata.objects.get(*object_index).ok_or(
                SceneGraphBuildError::InvalidObjectIndex {
                    object_index: *object_index,
                    object_count: metadata.objects.len(),
                },
            )?;
            if !is_scene_object_class(object_scene_class_id(object)) {
                return Err(SceneGraphBuildError::UnexpectedDecodedRoot {
                    object_index: *object_index,
                    class_id: object_scene_class_id(object),
                });
            }
            if std::mem::replace(&mut seen[*object_index], true) {
                return Err(SceneGraphBuildError::DuplicateDecodedRoot {
                    object_index: *object_index,
                });
            }
            builder.add_root(
                *object_index,
                object.path_id,
                object_scene_class_id(object),
                root,
            )?;
        }
        builder.finish()
    }

    pub fn objects(&self) -> &[SceneObjectRecord] {
        &self.objects
    }

    pub fn relationships(&self) -> &[SceneRelationship] {
        &self.relationships
    }

    pub fn issues(&self) -> &[SceneGraphIssue] {
        &self.issues
    }

    /// True when relevant TypeTrees decoded and every recovered pointer is null or local/unique.
    pub fn is_complete(&self) -> bool {
        self.issues.is_empty()
            && self.relationships.iter().all(|relationship| {
                matches!(
                    &relationship.target,
                    SceneRelationshipTarget::Null | SceneRelationshipTarget::LocalObject { .. }
                )
            })
    }
}

struct SceneGraphBuilder<'a> {
    metadata: &'a SerializedFileMetadata,
    limits: SceneGraphLimits,
    objects: Vec<SceneObjectRecord>,
    path_id_index: Vec<(i64, usize)>,
    relationships: Vec<SceneRelationship>,
    issues: Vec<SceneGraphIssue>,
    total_decoded_bytes: usize,
    total_name_bytes: usize,
    total_field_path_bytes: usize,
}

impl<'a> SceneGraphBuilder<'a> {
    fn new(
        metadata: &'a SerializedFileMetadata,
        limits: SceneGraphLimits,
    ) -> Result<Self, SceneGraphBuildError> {
        let object_count = metadata.objects.len();
        if object_count > limits.max_objects {
            return Err(SceneGraphBuildError::ObjectCountLimitExceeded {
                object_count,
                limit: limits.max_objects,
            });
        }
        let mut objects = Vec::new();
        objects.try_reserve_exact(object_count).map_err(|_| {
            SceneGraphBuildError::AllocationFailed {
                resource: "scene object records",
                requested: object_count,
            }
        })?;
        let mut path_id_index = Vec::new();
        path_id_index.try_reserve_exact(object_count).map_err(|_| {
            SceneGraphBuildError::AllocationFailed {
                resource: "scene PathID index",
                requested: object_count,
            }
        })?;
        for (object_index, object) in metadata.objects.iter().enumerate() {
            objects.push(SceneObjectRecord {
                object_index,
                path_id: object.path_id,
                class_id: object_scene_class_id(object),
                name_bytes: None,
                is_active: None,
            });
            path_id_index.push((object.path_id, object_index));
        }
        path_id_index.sort_unstable();
        Ok(Self {
            metadata,
            limits,
            objects,
            path_id_index,
            relationships: Vec::new(),
            issues: Vec::new(),
            total_decoded_bytes: 0,
            total_name_bytes: 0,
            total_field_path_bytes: 0,
        })
    }

    fn add_payload(&mut self, payload: &ParsedObjectPayload) -> Result<(), SceneGraphBuildError> {
        self.total_decoded_bytes = self.charge(
            self.total_decoded_bytes,
            payload.allocated_bytes,
            self.limits.max_total_decoded_bytes,
            "decoded scene payload bytes",
        )?;
        let object = self.metadata.objects.get(payload.object_index).ok_or(
            SceneGraphBuildError::InvalidObjectIndex {
                object_index: payload.object_index,
                object_count: self.metadata.objects.len(),
            },
        )?;
        self.add_root(
            payload.object_index,
            object.path_id,
            object_scene_class_id(object),
            &payload.root,
        )
    }

    fn add_root(
        &mut self,
        object_index: usize,
        path_id: i64,
        class_id: Option<i32>,
        root: &SerializedValue,
    ) -> Result<(), SceneGraphBuildError> {
        match class_id {
            Some(GAME_OBJECT_CLASS_ID) => self.read_game_object(object_index, path_id, root),
            Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID) => {
                self.read_transform(object_index, path_id, root)
            }
            _ => Ok(()),
        }
    }

    fn read_game_object(
        &mut self,
        object_index: usize,
        path_id: i64,
        root: &SerializedValue,
    ) -> Result<(), SceneGraphBuildError> {
        if let Some(value) = self.unique_field(object_index, path_id, root, "m_Name")? {
            match value {
                SerializedValue::StringBytes(bytes) => {
                    self.total_name_bytes = self.charge(
                        self.total_name_bytes,
                        bytes.len(),
                        self.limits.max_total_name_bytes,
                        "scene object name bytes",
                    )?;
                    self.objects[object_index].name_bytes = Some(bytes.clone());
                }
                other => self.unexpected_type(
                    object_index,
                    path_id,
                    "m_Name",
                    "string",
                    value_type_name(other),
                )?,
            }
        }
        if let Some(value) = self.unique_field(object_index, path_id, root, "m_IsActive")? {
            match value {
                SerializedValue::Bool(is_active) => {
                    self.objects[object_index].is_active = Some(*is_active);
                }
                other => self.unexpected_type(
                    object_index,
                    path_id,
                    "m_IsActive",
                    "bool",
                    value_type_name(other),
                )?,
            }
        }
        let Some(value) = self.unique_field(object_index, path_id, root, "m_Component")? else {
            return Ok(());
        };
        let Some(components) = value.array_elements() else {
            self.unexpected_type(
                object_index,
                path_id,
                "m_Component",
                "array",
                value_type_name(value),
            )?;
            return Ok(());
        };
        for (index, component) in components.iter().enumerate() {
            let field_path = format!("m_Component[{index}]");
            if let Some(pointer) =
                self.component_pointer(object_index, path_id, &field_path, component)?
            {
                self.push_relationship(
                    object_index,
                    path_id,
                    SceneRelationshipKind::GameObjectComponent,
                    field_path,
                    pointer,
                )?;
            }
        }
        Ok(())
    }

    fn read_transform(
        &mut self,
        object_index: usize,
        path_id: i64,
        root: &SerializedValue,
    ) -> Result<(), SceneGraphBuildError> {
        for (field_name, kind) in [
            ("m_GameObject", SceneRelationshipKind::ComponentGameObject),
            ("m_Father", SceneRelationshipKind::TransformParent),
        ] {
            let Some(value) = self.unique_field(object_index, path_id, root, field_name)? else {
                continue;
            };
            if let Some(pointer) = self.pointer_value(object_index, path_id, field_name, value)? {
                self.push_relationship(
                    object_index,
                    path_id,
                    kind,
                    field_name.to_owned(),
                    pointer,
                )?;
            }
        }
        let Some(value) = self.unique_field(object_index, path_id, root, "m_Children")? else {
            return Ok(());
        };
        let Some(children) = value.array_elements() else {
            self.unexpected_type(
                object_index,
                path_id,
                "m_Children",
                "array",
                value_type_name(value),
            )?;
            return Ok(());
        };
        for (index, child) in children.iter().enumerate() {
            let field_path = format!("m_Children[{index}]");
            if let Some(pointer) = self.pointer_value(object_index, path_id, &field_path, child)? {
                self.push_relationship(
                    object_index,
                    path_id,
                    SceneRelationshipKind::TransformChild,
                    field_path,
                    pointer,
                )?;
            }
        }
        Ok(())
    }

    fn component_pointer(
        &mut self,
        object_index: usize,
        path_id: i64,
        field_path: &str,
        value: &SerializedValue,
    ) -> Result<Option<(i32, i64)>, SceneGraphBuildError> {
        if let Some(pointer) = value.pptr() {
            return Ok(Some(pointer));
        }
        let value = match value.unique_field("component") {
            Ok(Some(value)) => value,
            Ok(None) => {
                self.push_issue(SceneGraphIssue {
                    object_index,
                    path_id,
                    field_path: Some(field_path.to_owned()),
                    kind: SceneGraphIssueKind::MissingField {
                        field_name: "component".to_owned(),
                    },
                })?;
                return Ok(None);
            }
            Err(error) => {
                self.push_lookup_issue(object_index, path_id, field_path, error)?;
                return Ok(None);
            }
        };
        self.pointer_value(object_index, path_id, field_path, value)
    }

    fn pointer_value(
        &mut self,
        object_index: usize,
        path_id: i64,
        field_path: &str,
        value: &SerializedValue,
    ) -> Result<Option<(i32, i64)>, SceneGraphBuildError> {
        if let Some(pointer) = value.pptr() {
            Ok(Some(pointer))
        } else {
            self.unexpected_type(
                object_index,
                path_id,
                field_path,
                "PPtr",
                value_type_name(value),
            )?;
            Ok(None)
        }
    }

    fn unique_field<'b>(
        &mut self,
        object_index: usize,
        path_id: i64,
        root: &'b SerializedValue,
        field_name: &str,
    ) -> Result<Option<&'b SerializedValue>, SceneGraphBuildError> {
        match root.unique_field_in_base_chain(field_name) {
            Ok(Some(value)) => Ok(Some(value)),
            Ok(None) => {
                self.push_issue(SceneGraphIssue {
                    object_index,
                    path_id,
                    field_path: Some(field_name.to_owned()),
                    kind: SceneGraphIssueKind::MissingField {
                        field_name: field_name.to_owned(),
                    },
                })?;
                Ok(None)
            }
            Err(error) => {
                self.push_lookup_issue(object_index, path_id, field_name, error)?;
                Ok(None)
            }
        }
    }

    fn push_lookup_issue(
        &mut self,
        object_index: usize,
        path_id: i64,
        field_path: &str,
        error: SerializedValueLookupError,
    ) -> Result<(), SceneGraphBuildError> {
        let kind = match error {
            SerializedValueLookupError::NotStruct => SceneGraphIssueKind::UnexpectedValueType {
                expected: "struct",
                actual: "non-struct",
            },
            SerializedValueLookupError::DuplicateField { name, match_count } => {
                SceneGraphIssueKind::DuplicateField {
                    field_name: name,
                    match_count,
                }
            }
        };
        self.push_issue(SceneGraphIssue {
            object_index,
            path_id,
            field_path: Some(field_path.to_owned()),
            kind,
        })
    }

    fn unexpected_type(
        &mut self,
        object_index: usize,
        path_id: i64,
        field_path: &str,
        expected: &'static str,
        actual: &'static str,
    ) -> Result<(), SceneGraphBuildError> {
        self.push_issue(SceneGraphIssue {
            object_index,
            path_id,
            field_path: Some(field_path.to_owned()),
            kind: SceneGraphIssueKind::UnexpectedValueType { expected, actual },
        })
    }

    fn push_relationship(
        &mut self,
        source_object_index: usize,
        source_path_id: i64,
        kind: SceneRelationshipKind,
        field_path: String,
        (file_id, path_id): (i32, i64),
    ) -> Result<(), SceneGraphBuildError> {
        let requested_count =
            self.relationships
                .len()
                .checked_add(1)
                .ok_or(SceneGraphBuildError::CountOverflow {
                    resource: "scene relationships",
                })?;
        if requested_count > self.limits.max_relationships {
            return Err(SceneGraphBuildError::RelationshipLimitExceeded {
                requested_count,
                limit: self.limits.max_relationships,
            });
        }
        self.total_field_path_bytes = self.charge(
            self.total_field_path_bytes,
            field_path.len(),
            self.limits.max_total_field_path_bytes,
            "scene relationship field paths",
        )?;
        self.relationships
            .try_reserve(1)
            .map_err(|_| SceneGraphBuildError::AllocationFailed {
                resource: "scene relationships",
                requested: requested_count,
            })?;
        let target = self.resolve_pointer(file_id, path_id);
        self.relationships.push(SceneRelationship {
            source_object_index,
            source_path_id,
            kind,
            field_path,
            file_id,
            path_id,
            target,
        });
        Ok(())
    }

    fn resolve_pointer(&self, file_id: i32, path_id: i64) -> SceneRelationshipTarget {
        if path_id == 0 {
            return SceneRelationshipTarget::Null;
        }
        if file_id == 0 {
            let first = self
                .path_id_index
                .partition_point(|(candidate, _)| *candidate < path_id);
            let end = self
                .path_id_index
                .partition_point(|(candidate, _)| *candidate <= path_id);
            return match end - first {
                0 => SceneRelationshipTarget::MissingLocalObject { path_id },
                1 => SceneRelationshipTarget::LocalObject {
                    object_index: self.path_id_index[first].1,
                },
                match_count => SceneRelationshipTarget::AmbiguousLocalObject {
                    path_id,
                    match_count,
                },
            };
        }
        let Some(external_index) = file_id
            .checked_sub(1)
            .and_then(|index| usize::try_from(index).ok())
        else {
            return SceneRelationshipTarget::InvalidFileId {
                file_id,
                external_count: self.metadata.externals.len(),
            };
        };
        if external_index >= self.metadata.externals.len() {
            SceneRelationshipTarget::InvalidFileId {
                file_id,
                external_count: self.metadata.externals.len(),
            }
        } else {
            SceneRelationshipTarget::ExternalObject {
                file_id,
                external_index,
                path_id,
            }
        }
    }

    fn push_issue(&mut self, issue: SceneGraphIssue) -> Result<(), SceneGraphBuildError> {
        let requested_count =
            self.issues
                .len()
                .checked_add(1)
                .ok_or(SceneGraphBuildError::CountOverflow {
                    resource: "scene graph issues",
                })?;
        if requested_count > self.limits.max_issues {
            return Err(SceneGraphBuildError::IssueLimitExceeded {
                requested_count,
                limit: self.limits.max_issues,
            });
        }
        self.issues
            .try_reserve(1)
            .map_err(|_| SceneGraphBuildError::AllocationFailed {
                resource: "scene graph issues",
                requested: requested_count,
            })?;
        self.issues.push(issue);
        Ok(())
    }

    fn charge(
        &self,
        current: usize,
        amount: usize,
        limit: usize,
        resource: &'static str,
    ) -> Result<usize, SceneGraphBuildError> {
        let requested = current
            .checked_add(amount)
            .ok_or(SceneGraphBuildError::CountOverflow { resource })?;
        if requested > limit {
            return Err(SceneGraphBuildError::ResourceLimitExceeded {
                resource,
                requested,
                limit,
            });
        }
        Ok(requested)
    }

    fn finish(self) -> Result<SceneGraph, SceneGraphBuildError> {
        Ok(SceneGraph {
            objects: self.objects,
            relationships: self.relationships,
            issues: self.issues,
        })
    }
}

fn object_scene_class_id(object: &ObjectInfo) -> Option<i32> {
    object
        .class_id
        .map(i32::from)
        .or_else(|| object.resolved_type_index.map(|_| object.raw_type_id))
}

fn is_scene_object_class(class_id: Option<i32>) -> bool {
    matches!(
        class_id,
        Some(GAME_OBJECT_CLASS_ID | TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)
    )
}

fn value_type_name(value: &SerializedValue) -> &'static str {
    match value {
        SerializedValue::Bool(_) => "bool",
        SerializedValue::I8(_) => "i8",
        SerializedValue::U8(_) => "u8",
        SerializedValue::I16(_) => "i16",
        SerializedValue::U16(_) => "u16",
        SerializedValue::Char(_) => "char",
        SerializedValue::I32(_) => "i32",
        SerializedValue::U32(_) => "u32",
        SerializedValue::I64(_) => "i64",
        SerializedValue::U64(_) => "u64",
        SerializedValue::F32(_) => "f32",
        SerializedValue::F64(_) => "f64",
        SerializedValue::StringBytes(_) => "string",
        SerializedValue::Bytes(_) => "bytes",
        SerializedValue::Array(_) => "array",
        SerializedValue::Struct(_) => "struct",
        SerializedValue::PPtr { .. } => "PPtr",
        SerializedValue::Raw { .. } => "raw",
    }
}

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
        }
    }
}

impl Error for SceneGraphBuildError {}

#[cfg(test)]
mod tests {
    use super::{
        SceneGraph, SceneGraphIssueKind, SceneGraphLimits, SceneRelationshipKind,
        SceneRelationshipTarget,
    };
    use crate::io::binary::Endian;
    use crate::serialized_file::file::SerializedFile;
    use crate::serialized_file::format_version::FormatVersion;
    use crate::serialized_file::metadata::{FileIdentifier, ObjectInfo, SerializedFileMetadata};
    use crate::serialized_file::metadata_prefix::SerializedFileMetadataPrefix;
    use crate::serialized_file::object::{SerializedFieldValue as Field, SerializedValue as Value};

    fn metadata(path_ids: &[(i64, i16)]) -> SerializedFileMetadata {
        SerializedFileMetadata {
            prefix: SerializedFileMetadataPrefix {
                metadata_offset: 0,
                metadata_size: 0,
                body_endian: Endian::Little,
                unity_version_text: None,
                target_platform: None,
                has_type_tree: false,
                consumed_bytes: 0,
            },
            types: Vec::new(),
            long_file_id: None,
            objects: path_ids
                .iter()
                .map(|(path_id, class_id)| ObjectInfo {
                    path_id: *path_id,
                    byte_start: 0,
                    byte_size: 0,
                    serialized_type_index: None,
                    raw_type_id: i32::from(*class_id),
                    class_id: Some(*class_id),
                    script_type_index: None,
                    is_destroyed: None,
                    is_stripped: None,
                    resolved_type_index: None,
                })
                .collect(),
            script_types: Vec::new(),
            externals: vec![FileIdentifier {
                asset_path: Some("shared.assets".to_owned()),
                guid: None,
                file_type: None,
                path_name_origin: "shared.assets".to_owned(),
            }],
            reference_types: Vec::new(),
            user_information: None,
            trailing_metadata: Vec::new(),
            consumed_bytes: 0,
        }
    }

    fn field(name: &str, value: Value) -> Field {
        Field {
            name: name.to_owned(),
            value,
        }
    }

    fn pptr(file_id: i32, path_id: i64) -> Value {
        Value::PPtr { file_id, path_id }
    }

    fn put_u16_le(output: &mut Vec<u8>, value: u16) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn put_i32_le(output: &mut Vec<u8>, value: i32) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn put_u32_le(output: &mut Vec<u8>, value: u32) {
        output.extend_from_slice(&value.to_le_bytes());
    }

    fn push_tree_string(buffer: &mut Vec<u8>, value: &str) -> u32 {
        let offset = u32::try_from(buffer.len()).unwrap();
        buffer.extend_from_slice(value.as_bytes());
        buffer.push(0);
        offset
    }

    #[allow(clippy::too_many_arguments)]
    fn put_tree_node(
        output: &mut Vec<u8>,
        level: u8,
        type_offset: u32,
        name_offset: u32,
        byte_size: i32,
        index: i32,
    ) {
        put_u16_le(output, 1);
        output.push(level);
        output.push(0);
        put_u32_le(output, type_offset);
        put_u32_le(output, name_offset);
        put_i32_le(output, byte_size);
        put_i32_le(output, index);
        put_u32_le(output, 0);
    }

    fn make_game_object_file() -> Vec<u8> {
        let mut strings = Vec::new();
        let root_type = push_tree_string(&mut strings, "GameObject");
        let empty = push_tree_string(&mut strings, "");
        let name_field = push_tree_string(&mut strings, "m_Name");
        let string_type = push_tree_string(&mut strings, "string");
        let array = push_tree_string(&mut strings, "Array");
        let size = push_tree_string(&mut strings, "size");
        let int_type = push_tree_string(&mut strings, "int");
        let data = push_tree_string(&mut strings, "data");
        let char_type = push_tree_string(&mut strings, "char");
        let active_field = push_tree_string(&mut strings, "m_IsActive");
        let bool_type = push_tree_string(&mut strings, "bool");
        let component_field = push_tree_string(&mut strings, "m_Component");
        let vector_type = push_tree_string(&mut strings, "vector");
        let pptr_type = push_tree_string(&mut strings, "PPtr<Component>");
        let file_id_field = push_tree_string(&mut strings, "m_FileID");
        let path_id_field = push_tree_string(&mut strings, "m_PathID");
        let int32_type = push_tree_string(&mut strings, "SInt32");

        let mut metadata = Vec::new();
        metadata.extend_from_slice(b"2019.4.0f1\0");
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 12);
        put_i32_le(&mut metadata, i32::try_from(strings.len()).unwrap());
        for (level, type_offset, name_offset, byte_size, index) in [
            (0, root_type, empty, -1, 0),
            (1, string_type, name_field, -1, 1),
            (2, array, array, -1, 2),
            (3, int_type, size, 4, 3),
            (3, char_type, data, 1, 4),
            (1, bool_type, active_field, 1, 5),
            (1, vector_type, component_field, -1, 6),
            (2, array, array, -1, 7),
            (3, int_type, size, 4, 8),
            (3, pptr_type, data, -1, 9),
            (4, int32_type, file_id_field, 4, 10),
            (4, int32_type, path_id_field, 4, 11),
        ] {
            put_tree_node(
                &mut metadata,
                level,
                type_offset,
                name_offset,
                byte_size,
                index,
            );
        }
        metadata.extend_from_slice(&strings);
        put_u32_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 1);
        put_i32_le(&mut metadata, 42);
        put_u32_le(&mut metadata, 0);

        let mut payload = Vec::new();
        put_i32_le(&mut payload, 4);
        payload.extend_from_slice(b"Root");
        payload.push(1);
        put_i32_le(&mut payload, 0);
        put_i32_le(&mut metadata, i32::try_from(payload.len()).unwrap());
        put_i32_le(&mut metadata, 1);
        put_u16_le(&mut metadata, 1);
        put_u16_le(&mut metadata, 0);
        put_i32_le(&mut metadata, 0);
        metadata.push(0);

        let header_size = 20usize;
        let data_offset = header_size + metadata.len();
        let file_size = data_offset + payload.len();
        let mut file = Vec::with_capacity(file_size);
        file.extend_from_slice(&i32::try_from(metadata.len()).unwrap().to_be_bytes());
        file.extend_from_slice(&u32::try_from(file_size).unwrap().to_be_bytes());
        file.extend_from_slice(&FormatVersion::Unknown10.raw().to_be_bytes());
        file.extend_from_slice(&u32::try_from(data_offset).unwrap().to_be_bytes());
        file.extend_from_slice(&[0, 0, 0, 0]);
        file.extend_from_slice(&metadata);
        file.extend_from_slice(&payload);
        file
    }

    #[test]
    fn reconstructs_game_object_component_and_transform_edges_without_merging_order() {
        let metadata = metadata(&[(10, 1), (20, 4), (30, 4), (40, 23)]);
        let roots = vec![
            (
                0,
                Value::Struct(vec![
                    field(
                        "Base",
                        Value::Struct(vec![
                            field("m_Name", Value::StringBytes(b"Avatar".to_vec())),
                            field("m_IsActive", Value::Bool(true)),
                        ]),
                    ),
                    field(
                        "m_Component",
                        Value::Array(vec![
                            Value::Struct(vec![field("component", pptr(0, 20))]),
                            Value::Struct(vec![field("component", pptr(0, 40))]),
                        ]),
                    ),
                ]),
            ),
            (
                1,
                Value::Struct(vec![
                    field(
                        "Base",
                        Value::Struct(vec![field("m_GameObject", pptr(0, 10))]),
                    ),
                    field("m_Father", pptr(0, 0)),
                    field("m_Children", Value::Array(vec![pptr(0, 30)])),
                ]),
            ),
            (
                2,
                Value::Struct(vec![
                    field(
                        "Base",
                        Value::Struct(vec![field("m_GameObject", pptr(0, 10))]),
                    ),
                    field("m_Father", pptr(0, 20)),
                    field("m_Children", Value::Array(Vec::new())),
                ]),
            ),
        ];

        let graph =
            SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()).unwrap();
        assert_eq!(graph.objects().len(), 4);
        assert_eq!(
            graph.objects()[0].name_bytes.as_deref(),
            Some(&b"Avatar"[..])
        );
        assert_eq!(graph.objects()[0].is_active, Some(true));
        assert_eq!(graph.relationships().len(), 7);
        assert_eq!(
            graph
                .relationships()
                .iter()
                .map(|relationship| relationship.kind)
                .collect::<Vec<_>>(),
            [
                SceneRelationshipKind::GameObjectComponent,
                SceneRelationshipKind::GameObjectComponent,
                SceneRelationshipKind::ComponentGameObject,
                SceneRelationshipKind::TransformParent,
                SceneRelationshipKind::TransformChild,
                SceneRelationshipKind::ComponentGameObject,
                SceneRelationshipKind::TransformParent,
            ]
        );
        assert_eq!(
            graph.relationships()[0].target,
            SceneRelationshipTarget::LocalObject { object_index: 1 }
        );
        assert_eq!(
            graph.relationships()[3].target,
            SceneRelationshipTarget::Null
        );
        assert_eq!(
            graph.relationships()[6].target,
            SceneRelationshipTarget::LocalObject { object_index: 1 }
        );
        assert!(graph.issues().is_empty());
        assert!(graph.is_complete());
    }

    #[test]
    fn builds_a_scene_graph_from_a_complete_serialized_file() {
        let bytes = make_game_object_file();
        let file = SerializedFile::parse(&bytes).unwrap();
        let graph = SceneGraph::build(&file, SceneGraphLimits::default()).unwrap();

        assert_eq!(graph.objects().len(), 1);
        assert_eq!(graph.objects()[0].path_id, 42);
        assert_eq!(graph.objects()[0].class_id, Some(1));
        assert_eq!(graph.objects()[0].name_bytes.as_deref(), Some(&b"Root"[..]));
        assert_eq!(graph.objects()[0].is_active, Some(true));
        assert!(graph.relationships().is_empty());
        assert!(graph.issues().is_empty());
        assert!(graph.is_complete());
    }

    #[test]
    fn uses_resolved_serialized_types_for_modern_object_class_ids() {
        let mut metadata = metadata(&[(42, 1)]);
        metadata.objects[0].class_id = None;
        metadata.objects[0].resolved_type_index = Some(0);
        let roots = vec![(
            0,
            Value::Struct(vec![
                field("m_Name", Value::StringBytes(b"Modern".to_vec())),
                field("m_IsActive", Value::Bool(true)),
                field("m_Component", Value::Array(Vec::new())),
            ]),
        )];

        let graph =
            SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()).unwrap();
        assert_eq!(graph.objects()[0].class_id, Some(1));
        assert!(graph.issues().is_empty());
    }

    #[test]
    fn keeps_external_missing_duplicate_and_malformed_relationships_explicit() {
        let metadata = metadata(&[(10, 1), (20, 4), (20, 23)]);
        let roots = vec![
            (
                0,
                Value::Struct(vec![
                    field("m_Name", Value::StringBytes(vec![0xff])),
                    field("m_IsActive", Value::Bool(false)),
                    field(
                        "m_Component",
                        Value::Array(vec![
                            Value::Struct(vec![field("component", pptr(1, 99))]),
                            Value::Struct(vec![field("component", pptr(0, 20))]),
                            Value::Struct(vec![field("component", pptr(0, 88))]),
                        ]),
                    ),
                ]),
            ),
            (
                1,
                Value::Struct(vec![
                    field("m_GameObject", pptr(-1, 10)),
                    field("m_Father", pptr(0, 0)),
                    field("m_Father", pptr(0, 20)),
                    field("m_Children", Value::Bool(true)),
                ]),
            ),
        ];

        let graph =
            SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()).unwrap();
        assert_eq!(graph.objects()[0].name_bytes.as_deref(), Some(&[0xff][..]));
        assert!(matches!(
            graph.relationships()[0].target,
            SceneRelationshipTarget::ExternalObject {
                external_index: 0,
                path_id: 99,
                ..
            }
        ));
        assert_eq!(
            graph.relationships()[1].target,
            SceneRelationshipTarget::AmbiguousLocalObject {
                path_id: 20,
                match_count: 2,
            }
        );
        assert_eq!(
            graph.relationships()[2].target,
            SceneRelationshipTarget::MissingLocalObject { path_id: 88 }
        );
        assert_eq!(
            graph.relationships()[3].target,
            SceneRelationshipTarget::InvalidFileId {
                file_id: -1,
                external_count: 1,
            }
        );
        assert!(graph.issues().iter().any(|issue| matches!(
            issue.kind,
            SceneGraphIssueKind::DuplicateField {
                field_name: ref name,
                match_count: 2,
            } if name == "m_Father"
        )));
        assert!(graph.issues().iter().any(|issue| matches!(
            issue.kind,
            SceneGraphIssueKind::UnexpectedValueType {
                expected: "array",
                actual: "bool",
            }
        )));
        assert!(!graph.is_complete());
    }

    #[test]
    fn enforces_object_and_relationship_limits() {
        let metadata = metadata(&[(10, 1)]);
        let roots = vec![(
            0,
            Value::Struct(vec![
                field("m_Name", Value::StringBytes(Vec::new())),
                field("m_IsActive", Value::Bool(true)),
                field("m_Component", Value::Array(vec![pptr(0, 0)])),
            ]),
        )];
        let limits = SceneGraphLimits {
            max_objects: 0,
            ..SceneGraphLimits::default()
        };
        assert!(SceneGraph::from_decoded_roots(&metadata, &roots, limits).is_err());

        let limits = SceneGraphLimits {
            max_relationships: 0,
            ..SceneGraphLimits::default()
        };
        assert!(SceneGraph::from_decoded_roots(&metadata, &roots, limits).is_err());
    }

    #[test]
    fn rejects_incomplete_or_duplicate_decoded_root_sets() {
        let metadata = metadata(&[(10, 1), (20, 4)]);
        assert!(
            SceneGraph::from_decoded_roots(&metadata, &[], SceneGraphLimits::default()).is_err()
        );

        let root = Value::Struct(Vec::new());
        let roots = vec![(0, root.clone()), (0, root)];
        assert!(matches!(
            SceneGraph::from_decoded_roots(&metadata, &roots, SceneGraphLimits::default()),
            Err(super::SceneGraphBuildError::DuplicateDecodedRoot { object_index: 0 })
        ));
    }
}
