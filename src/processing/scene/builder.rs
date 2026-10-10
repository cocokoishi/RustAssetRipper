use super::errors::SceneGraphBuildError;
use super::*;

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
                transform: None,
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
            Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID) => self.read_transform(
                object_index,
                path_id,
                root,
                class_id == Some(RECT_TRANSFORM_CLASS_ID),
            ),
            Some(PREFAB_INSTANCE_CLASS_ID) => {
                self.read_prefab_instance(object_index, path_id, root)
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
                    pointer.field_path,
                    (pointer.file_id, pointer.path_id),
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
        is_rect_transform: bool,
    ) -> Result<(), SceneGraphBuildError> {
        let local_position =
            self.read_vector3_field(object_index, path_id, root, "m_LocalPosition", true)?;
        let local_rotation =
            self.read_quaternion_field(object_index, path_id, root, "m_LocalRotation", true)?;
        let local_scale =
            self.read_vector3_field(object_index, path_id, root, "m_LocalScale", true)?;
        let root_order = self.read_i32_field(object_index, path_id, root, "m_RootOrder")?;
        let local_euler_angles_hint =
            self.read_vector3_field(object_index, path_id, root, "m_LocalEulerAnglesHint", false)?;
        let rect_transform = if is_rect_transform {
            Some(SceneRectTransformValues {
                anchor_min: self.read_vector2_field(
                    object_index,
                    path_id,
                    root,
                    "m_AnchorMin",
                    true,
                )?,
                anchor_max: self.read_vector2_field(
                    object_index,
                    path_id,
                    root,
                    "m_AnchorMax",
                    true,
                )?,
                anchored_position: self.read_vector2_field(
                    object_index,
                    path_id,
                    root,
                    "m_AnchoredPosition",
                    true,
                )?,
                size_delta: self.read_vector2_field(
                    object_index,
                    path_id,
                    root,
                    "m_SizeDelta",
                    true,
                )?,
                pivot: self.read_vector2_field(object_index, path_id, root, "m_Pivot", true)?,
            })
        } else {
            None
        };
        self.objects[object_index].transform = Some(SceneTransformValues {
            local_position,
            local_rotation,
            local_scale,
            root_order,
            local_euler_angles_hint,
            rect_transform,
        });

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

    fn read_prefab_instance(
        &mut self,
        object_index: usize,
        path_id: i64,
        root: &SerializedValue,
    ) -> Result<(), SceneGraphBuildError> {
        let Some(value) = self.unique_field(object_index, path_id, root, "m_RootGameObject")?
        else {
            return Ok(());
        };
        if let Some(pointer) =
            self.pointer_value(object_index, path_id, "m_RootGameObject", value)?
        {
            self.push_relationship(
                object_index,
                path_id,
                SceneRelationshipKind::PrefabInstanceRootGameObject,
                "m_RootGameObject".to_owned(),
                pointer,
            )?;
        }
        Ok(())
    }

    fn read_vector2_field(
        &mut self,
        object_index: usize,
        path_id: i64,
        root: &SerializedValue,
        field_name: &str,
        required: bool,
    ) -> Result<Option<SceneVector2>, SceneGraphBuildError> {
        let value = if required {
            self.unique_field(object_index, path_id, root, field_name)?
        } else {
            self.optional_field(object_index, path_id, root, field_name)?
        };
        let Some(value) = value else {
            return Ok(None);
        };
        if !matches!(value, SerializedValue::Struct(_)) {
            self.unexpected_type(
                object_index,
                path_id,
                field_name,
                "Vector2",
                value_type_name(value),
            )?;
            return Ok(None);
        }
        let Some(x) = self.read_f32_component(object_index, path_id, value, field_name, "x")?
        else {
            return Ok(None);
        };
        let Some(y) = self.read_f32_component(object_index, path_id, value, field_name, "y")?
        else {
            return Ok(None);
        };
        Ok(Some(SceneVector2 { x, y }))
    }

    fn read_vector3_field(
        &mut self,
        object_index: usize,
        path_id: i64,
        root: &SerializedValue,
        field_name: &str,
        required: bool,
    ) -> Result<Option<SceneVector3>, SceneGraphBuildError> {
        let value = if required {
            self.unique_field(object_index, path_id, root, field_name)?
        } else {
            self.optional_field(object_index, path_id, root, field_name)?
        };
        let Some(value) = value else {
            return Ok(None);
        };
        if !matches!(value, SerializedValue::Struct(_)) {
            self.unexpected_type(
                object_index,
                path_id,
                field_name,
                "Vector3",
                value_type_name(value),
            )?;
            return Ok(None);
        }
        let Some(x) = self.read_f32_component(object_index, path_id, value, field_name, "x")?
        else {
            return Ok(None);
        };
        let Some(y) = self.read_f32_component(object_index, path_id, value, field_name, "y")?
        else {
            return Ok(None);
        };
        let Some(z) = self.read_f32_component(object_index, path_id, value, field_name, "z")?
        else {
            return Ok(None);
        };
        Ok(Some(SceneVector3 { x, y, z }))
    }

    fn read_quaternion_field(
        &mut self,
        object_index: usize,
        path_id: i64,
        root: &SerializedValue,
        field_name: &str,
        required: bool,
    ) -> Result<Option<SceneQuaternion>, SceneGraphBuildError> {
        let value = if required {
            self.unique_field(object_index, path_id, root, field_name)?
        } else {
            self.optional_field(object_index, path_id, root, field_name)?
        };
        let Some(value) = value else {
            return Ok(None);
        };
        if !matches!(value, SerializedValue::Struct(_)) {
            self.unexpected_type(
                object_index,
                path_id,
                field_name,
                "Quaternion",
                value_type_name(value),
            )?;
            return Ok(None);
        }
        let Some(x) = self.read_f32_component(object_index, path_id, value, field_name, "x")?
        else {
            return Ok(None);
        };
        let Some(y) = self.read_f32_component(object_index, path_id, value, field_name, "y")?
        else {
            return Ok(None);
        };
        let Some(z) = self.read_f32_component(object_index, path_id, value, field_name, "z")?
        else {
            return Ok(None);
        };
        let Some(w) = self.read_f32_component(object_index, path_id, value, field_name, "w")?
        else {
            return Ok(None);
        };
        Ok(Some(SceneQuaternion { x, y, z, w }))
    }

    fn read_i32_field(
        &mut self,
        object_index: usize,
        path_id: i64,
        root: &SerializedValue,
        field_name: &str,
    ) -> Result<Option<i32>, SceneGraphBuildError> {
        let Some(value) = self.optional_field(object_index, path_id, root, field_name)? else {
            return Ok(None);
        };
        match value {
            SerializedValue::I32(value) => Ok(Some(*value)),
            other => {
                self.unexpected_type(
                    object_index,
                    path_id,
                    field_name,
                    "i32",
                    value_type_name(other),
                )?;
                Ok(None)
            }
        }
    }

    fn read_f32_component(
        &mut self,
        object_index: usize,
        path_id: i64,
        vector: &SerializedValue,
        vector_path: &str,
        component_name: &str,
    ) -> Result<Option<f32>, SceneGraphBuildError> {
        let component_path = format!("{vector_path}.{component_name}");
        match vector.unique_field(component_name) {
            Ok(Some(SerializedValue::F32(value))) => Ok(Some(*value)),
            Ok(Some(other)) => {
                self.unexpected_type(
                    object_index,
                    path_id,
                    &component_path,
                    "f32",
                    value_type_name(other),
                )?;
                Ok(None)
            }
            Ok(None) => {
                self.push_issue(SceneGraphIssue {
                    object_index,
                    path_id,
                    field_path: Some(component_path),
                    kind: SceneGraphIssueKind::MissingField {
                        field_name: component_name.to_owned(),
                    },
                })?;
                Ok(None)
            }
            Err(error) => {
                self.push_lookup_issue(object_index, path_id, &component_path, error)?;
                Ok(None)
            }
        }
    }

    fn component_pointer(
        &mut self,
        object_index: usize,
        path_id: i64,
        field_path: &str,
        value: &SerializedValue,
    ) -> Result<Option<ScenePointerField>, SceneGraphBuildError> {
        if let Some(pointer) = value.pptr() {
            return Ok(Some(ScenePointerField {
                file_id: pointer.0,
                path_id: pointer.1,
                field_path: field_path.to_owned(),
            }));
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
        let pointer_field_path = format!("{field_path}.component");
        Ok(self
            .pointer_value(object_index, path_id, &pointer_field_path, value)?
            .map(|pointer| ScenePointerField {
                file_id: pointer.0,
                path_id: pointer.1,
                field_path: pointer_field_path,
            }))
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

    fn optional_field<'b>(
        &mut self,
        object_index: usize,
        path_id: i64,
        root: &'b SerializedValue,
        field_name: &str,
    ) -> Result<Option<&'b SerializedValue>, SceneGraphBuildError> {
        match root.unique_field_in_base_chain(field_name) {
            Ok(value) => Ok(value),
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
        Some(
            GAME_OBJECT_CLASS_ID
                | TRANSFORM_CLASS_ID
                | RECT_TRANSFORM_CLASS_ID
                | PREFAB_INSTANCE_CLASS_ID
        )
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
        SerializedValue::ManagedReferenceId { .. } => "managed reference ID",
        SerializedValue::ManagedReferenceRegistry(_) => "managed reference registry",
        SerializedValue::Raw { .. } => "raw",
    }
}

impl SceneGraph {
    /// Decode GameObject, Transform, RectTransform, and PrefabInstance payloads from one file.
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

    /// Build from the payloads already materialized by the workspace object factory.
    ///
    /// This preserves generated TPK TypeTree decoding for files that do not carry a TypeTree,
    /// while retaining decode failures as explicit graph issues.
    pub fn from_materialized_workspace(
        metadata: &SerializedFileMetadata,
        file_index: usize,
        workspace_objects: &[WorkspaceObject],
        limits: SceneGraphLimits,
    ) -> Result<Self, SceneGraphBuildError> {
        if workspace_objects.len() != metadata.objects.len() {
            return Err(SceneGraphBuildError::MaterializedObjectCountMismatch {
                expected: metadata.objects.len(),
                actual: workspace_objects.len(),
            });
        }

        let mut builder = SceneGraphBuilder::new(metadata, limits)?;
        for (object_index, (metadata_object, workspace_object)) in
            metadata.objects.iter().zip(workspace_objects).enumerate()
        {
            if workspace_object.file_index != file_index
                || workspace_object.object_index != object_index
                || workspace_object.path_id != metadata_object.path_id
            {
                return Err(SceneGraphBuildError::MaterializedObjectIdentityMismatch {
                    object_index,
                    expected_file_index: file_index,
                    actual_file_index: workspace_object.file_index,
                    actual_object_index: workspace_object.object_index,
                    expected_path_id: metadata_object.path_id,
                    actual_path_id: workspace_object.path_id,
                });
            }
            if !is_scene_object_class(object_scene_class_id(metadata_object)) {
                continue;
            }
            match &workspace_object.payload {
                Ok(payload)
                    if payload.object_index == object_index
                        && payload.path_id == metadata_object.path_id =>
                {
                    builder.add_payload(payload)?
                }
                Ok(payload) => {
                    return Err(SceneGraphBuildError::MaterializedPayloadIdentityMismatch {
                        object_index,
                        expected_path_id: metadata_object.path_id,
                        payload_object_index: payload.object_index,
                        payload_path_id: payload.path_id,
                    });
                }
                Err(error) => builder.push_issue(SceneGraphIssue {
                    object_index,
                    path_id: metadata_object.path_id,
                    field_path: None,
                    kind: SceneGraphIssueKind::ObjectDecodeFailed {
                        message: error.to_string(),
                    },
                })?,
            }
        }
        builder.finish()
    }
}
