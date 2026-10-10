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

fn hierarchy_kind_rank(kind: SceneRelationshipKind) -> u8 {
    match kind {
        SceneRelationshipKind::GameObjectComponent => 0,
        SceneRelationshipKind::ComponentGameObject => 1,
        SceneRelationshipKind::TransformParent => 2,
        SceneRelationshipKind::TransformChild => 3,
        SceneRelationshipKind::PrefabInstanceRootGameObject => 4,
    }
}

fn hierarchy_relationship_range(
    index: &[IndexedHierarchyRelationship],
    source_object_index: usize,
    kind: SceneRelationshipKind,
) -> std::ops::Range<usize> {
    let key = (source_object_index, hierarchy_kind_rank(kind));
    let start = index.partition_point(|entry| {
        (entry.source_object_index, hierarchy_kind_rank(entry.kind)) < key
    });
    let end = index.partition_point(|entry| {
        (entry.source_object_index, hierarchy_kind_rank(entry.kind)) <= key
    });
    start..end
}

fn scene_relationship_range(
    index: &[IndexedSceneRelationship],
    source_object_index: usize,
    kind: SceneRelationshipKind,
) -> std::ops::Range<usize> {
    let key = (source_object_index, hierarchy_kind_rank(kind));
    let start = index.partition_point(|entry| {
        (entry.source_object_index, hierarchy_kind_rank(entry.kind)) < key
    });
    let end = index.partition_point(|entry| {
        (entry.source_object_index, hierarchy_kind_rank(entry.kind)) <= key
    });
    start..end
}

fn push_scene_root_issue(
    issues: &mut Vec<SceneRootDiscoveryIssue>,
    limit: usize,
    issue: SceneRootDiscoveryIssue,
) -> Result<(), SceneRootDiscoveryError> {
    let requested = issues
        .len()
        .checked_add(1)
        .ok_or(SceneRootDiscoveryError::CountOverflow { resource: "issues" })?;
    if requested > limit {
        return Err(SceneRootDiscoveryError::IssueCountLimitExceeded { requested, limit });
    }
    issues
        .try_reserve(1)
        .map_err(|_| SceneRootDiscoveryError::AllocationFailed {
            resource: "issues",
            requested,
        })?;
    issues.push(issue);
    Ok(())
}

fn push_scene_root_candidate(
    roots: &mut Vec<SceneRootCandidate>,
    limit: usize,
    root: SceneRootCandidate,
) -> Result<(), SceneRootDiscoveryError> {
    let requested = roots
        .len()
        .checked_add(1)
        .ok_or(SceneRootDiscoveryError::CountOverflow { resource: "roots" })?;
    if requested > limit {
        return Err(SceneRootDiscoveryError::RootCountLimitExceeded { requested, limit });
    }
    roots
        .try_reserve(1)
        .map_err(|_| SceneRootDiscoveryError::AllocationFailed {
            resource: "roots",
            requested,
        })?;
    roots.push(root);
    Ok(())
}

fn push_workspace_scene_root_issue(
    discovery: &mut WorkspaceSceneRootDiscovery,
    limit: usize,
    issue: WorkspaceSceneRootDiscoveryIssue,
) -> Result<(), SceneRootDiscoveryError> {
    let requested = discovery
        .issues
        .len()
        .checked_add(1)
        .ok_or(SceneRootDiscoveryError::CountOverflow { resource: "issues" })?;
    if requested > limit {
        return Err(SceneRootDiscoveryError::IssueCountLimitExceeded { requested, limit });
    }
    discovery
        .issues
        .try_reserve(1)
        .map_err(|_| SceneRootDiscoveryError::AllocationFailed {
            resource: "workspace root issues",
            requested,
        })?;
    discovery.issues.push(issue);
    Ok(())
}

fn push_workspace_scene_root_candidate(
    discovery: &mut WorkspaceSceneRootDiscovery,
    limit: usize,
    root: WorkspaceSceneRootCandidate,
) -> Result<(), SceneRootDiscoveryError> {
    let requested = discovery
        .roots
        .len()
        .checked_add(1)
        .ok_or(SceneRootDiscoveryError::CountOverflow { resource: "roots" })?;
    if requested > limit {
        return Err(SceneRootDiscoveryError::RootCountLimitExceeded { requested, limit });
    }
    discovery
        .roots
        .try_reserve(1)
        .map_err(|_| SceneRootDiscoveryError::AllocationFailed {
            resource: "workspace root candidates",
            requested,
        })?;
    discovery.roots.push(root);
    Ok(())
}

fn push_workspace_hierarchy_issue(
    audit: &mut WorkspaceSceneHierarchyAudit,
    limit: usize,
    issue: WorkspaceSceneHierarchyIssue,
) -> Result<(), SceneGraphAuditError> {
    let requested =
        audit
            .issues
            .len()
            .checked_add(1)
            .ok_or(SceneGraphAuditError::CountOverflow {
                resource: "workspace hierarchy issues",
            })?;
    if requested > limit {
        return Err(SceneGraphAuditError::IssueLimitExceeded {
            count: requested,
            limit,
        });
    }
    audit
        .issues
        .try_reserve(1)
        .map_err(|_| SceneGraphAuditError::AllocationFailed {
            resource: "workspace hierarchy issues",
            requested,
        })?;
    audit.issues.push(issue);
    Ok(())
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

fn reference_field_path_matches(
    reference: &MultiFileObjectReference<'_>,
    relationship_field_path: &str,
) -> bool {
    reference.field_path == relationship_field_path
        || reference
            .field_path
            .strip_suffix(relationship_field_path)
            .and_then(|prefix| prefix.strip_suffix('.'))
            .is_some_and(|prefix| prefix.split('.').all(|segment| segment == "Base"))
}

fn reference_field_path_matches_array_element(
    reference: &MultiFileObjectReference<'_>,
    field_name: &str,
) -> bool {
    let mut path = reference.field_path;
    while let Some(base_path) = path.strip_prefix("Base.") {
        path = base_path;
    }
    let Some(element) = path
        .strip_prefix(field_name)
        .and_then(|value| value.strip_prefix('['))
    else {
        return false;
    };
    let Some((index, suffix)) = element.split_once(']') else {
        return false;
    };
    !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit()) && suffix.is_empty()
}

fn scene_target_from_multi_file_resolution(
    resolution: &MultiFilePPtrResolution,
    source_file_index: usize,
) -> SceneRelationshipTarget {
    match resolution {
        MultiFilePPtrResolution::Null => SceneRelationshipTarget::Null,
        MultiFilePPtrResolution::Object {
            target_file_index,
            target_object_index,
        } if *target_file_index == source_file_index => SceneRelationshipTarget::LocalObject {
            object_index: *target_object_index,
        },
        MultiFilePPtrResolution::Object {
            target_file_index,
            target_object_index,
        } => SceneRelationshipTarget::WorkspaceObject {
            file_index: *target_file_index,
            object_index: *target_object_index,
        },
        MultiFilePPtrResolution::MissingLocalObject { path_id } => {
            SceneRelationshipTarget::MissingLocalObject { path_id: *path_id }
        }
        MultiFilePPtrResolution::AmbiguousLocalObject {
            path_id,
            match_count,
        } => SceneRelationshipTarget::AmbiguousLocalObject {
            path_id: *path_id,
            match_count: *match_count,
        },
        MultiFilePPtrResolution::MissingDependency {
            external_index,
            path_id,
        } => SceneRelationshipTarget::MissingDependency {
            external_index: *external_index,
            path_id: *path_id,
        },
        MultiFilePPtrResolution::AmbiguousDependency {
            external_index,
            path_id,
            candidate_count,
        } => SceneRelationshipTarget::AmbiguousDependency {
            external_index: *external_index,
            path_id: *path_id,
            candidate_count: *candidate_count,
        },
        MultiFilePPtrResolution::MissingExternalObject {
            target_file_index,
            path_id,
        } => SceneRelationshipTarget::MissingExternalObject {
            file_index: *target_file_index,
            path_id: *path_id,
        },
        MultiFilePPtrResolution::AmbiguousExternalObject {
            target_file_index,
            path_id,
            match_count,
        } => SceneRelationshipTarget::AmbiguousExternalObject {
            file_index: *target_file_index,
            path_id: *path_id,
            match_count: *match_count,
        },
        MultiFilePPtrResolution::InvalidFileId {
            file_id,
            external_count,
        } => SceneRelationshipTarget::InvalidFileId {
            file_id: *file_id,
            external_count: *external_count,
        },
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SceneGraphResolveError {
    SourceFileIndexOutOfBounds {
        file_index: usize,
        file_count: usize,
    },
    SourceFileNameMismatch {
        file_index: usize,
        expected: String,
        actual: String,
    },
    ObjectTableMismatch {
        file_index: usize,
        object_index: usize,
    },
    ObjectIdentityMismatch {
        file_index: usize,
        object_index: usize,
        scene_path_id: i64,
        graph_path_id: i64,
    },
    ObjectCountMismatch {
        file_index: usize,
        scene_graph_object_count: usize,
        reference_object_count: usize,
    },
    ReferenceFieldNotFound {
        relationship_index: usize,
        object_index: usize,
        field_path: String,
    },
    ReferenceFieldAmbiguous {
        relationship_index: usize,
        object_index: usize,
        field_path: String,
    },
    ReferenceValueMismatch {
        relationship_index: usize,
        object_index: usize,
        field_path: String,
        scene_file_id: i32,
        scene_path_id: i64,
        graph_file_id: i32,
        graph_path_id: i64,
    },
    AllocationFailed {
        requested: usize,
    },
}

impl Display for SceneGraphResolveError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceFileIndexOutOfBounds {
                file_index,
                file_count,
            } => write!(
                f,
                "scene source file index {file_index} is outside the {file_count}-file reference graph"
            ),
            Self::SourceFileNameMismatch {
                file_index,
                expected,
                actual,
            } => write!(
                f,
                "scene source file {file_index} has normalized name '{actual}', but the reference graph has '{expected}'"
            ),
            Self::ObjectTableMismatch {
                file_index,
                object_index,
            } => write!(
                f,
                "scene object {file_index}:{object_index} is absent from the reference graph"
            ),
            Self::ObjectIdentityMismatch {
                file_index,
                object_index,
                scene_path_id,
                graph_path_id,
            } => write!(
                f,
                "scene object {file_index}:{object_index} has PathID {scene_path_id}, but the reference graph has PathID {graph_path_id}"
            ),
            Self::ObjectCountMismatch {
                file_index,
                scene_graph_object_count,
                reference_object_count,
            } => write!(
                f,
                "scene graph file {file_index} has {scene_graph_object_count} object records, but the reference graph has {reference_object_count}"
            ),
            Self::ReferenceFieldNotFound {
                relationship_index,
                object_index,
                field_path,
            } => write!(
                f,
                "scene relationship {relationship_index} for object {object_index} has no matching decoded reference at '{field_path}'"
            ),
            Self::ReferenceFieldAmbiguous {
                relationship_index,
                object_index,
                field_path,
            } => write!(
                f,
                "scene relationship {relationship_index} for object {object_index} matches multiple decoded references at '{field_path}'"
            ),
            Self::ReferenceValueMismatch {
                relationship_index,
                object_index,
                field_path,
                scene_file_id,
                scene_path_id,
                graph_file_id,
                graph_path_id,
            } => write!(
                f,
                "scene relationship {relationship_index} for object {object_index} at '{field_path}' has PPtr ({scene_file_id}, {scene_path_id}), but the reference graph has ({graph_file_id}, {graph_path_id})"
            ),
            Self::AllocationFailed { requested } => write!(
                f,
                "could not reserve {requested} resolved scene targets"
            ),
        }
    }
}

impl Error for SceneGraphResolveError {}

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

/// The asset kinds `SceneHierarchyObject.Create` sorts on, in source arm order.
///
/// Interface checks (`IGameObject`, `ILevelGameManager`, `ISceneRoots`, …) and the
/// `IsComponentOnGameObject` pointer test need live objects and stay caller-side;
/// the caller reports the winning arm, including which side of the MonoBehaviour
/// gate the asset falls on.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SceneHierarchyAssetKind {
    /// `IGameObject`.
    GameObject,
    /// `IMonoBehaviour` with `IsComponentOnGameObject`.
    AttachedMonoBehaviour,
    /// `IMonoBehaviour` without a GameObject: matched by no arm, ignored.
    DetachedMonoBehaviour,
    /// Any other `IComponent`.
    Component,
    /// `ILevelGameManager`.
    LevelGameManager,
    /// `IPrefabInstance`.
    PrefabInstance,
    /// `ISceneRoots`.
    SceneRoots,
    /// Anything else: matched by no arm, ignored.
    Other,
}

/// Which list `SceneHierarchyObject.Create` files the asset into.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SceneHierarchySlot {
    /// `GameObjects`.
    GameObjects,
    /// `Components`.
    Components,
    /// `Managers`.
    Managers,
    /// `PrefabInstances`.
    PrefabInstances,
    /// `SceneRoots` (a single assignment: repeated roots overwrite, caller keeps last).
    SceneRoots,
    /// Matched by no arm; the asset lands in no list.
    Ignored,
}

/// `SceneHierarchyObject.Create` (`AssetRipper/Source/AssetRipper.Processing/Prefabs/SceneHierarchyObject.cs`
/// lines 82-108): GameObjects, attached MonoBehaviours, and other components file into
/// their lists; managers, prefab instances, and scene roots file into theirs; detached
/// MonoBehaviours and anything else are ignored.
pub const fn classify_scene_hierarchy_asset(kind: SceneHierarchyAssetKind) -> SceneHierarchySlot {
    match kind {
        SceneHierarchyAssetKind::GameObject => SceneHierarchySlot::GameObjects,
        SceneHierarchyAssetKind::AttachedMonoBehaviour | SceneHierarchyAssetKind::Component => {
            SceneHierarchySlot::Components
        }
        SceneHierarchyAssetKind::LevelGameManager => SceneHierarchySlot::Managers,
        SceneHierarchyAssetKind::PrefabInstance => SceneHierarchySlot::PrefabInstances,
        SceneHierarchyAssetKind::SceneRoots => SceneHierarchySlot::SceneRoots,
        SceneHierarchyAssetKind::DetachedMonoBehaviour | SceneHierarchyAssetKind::Other => {
            SceneHierarchySlot::Ignored
        }
    }
}

/// `SceneHierarchyObject.FetchDependencies` edge labels in walk order
/// (`AssetRipper/Source/AssetRipper.Processing/Prefabs/SceneHierarchyObject.cs` lines 43-54
/// with `GameObjectHierarchyObject.FetchDependencies` first): one `{List}[]` label per
/// member of `GameObjects`, `Components`, `PrefabInstances`, and `Managers`, then a single
/// `SceneRoots` label when a root block applies. Counts arrive as caller facts; pointer
/// resolution stays caller-side.
pub fn scene_hierarchy_dependency_fields(
    game_objects: usize,
    components: usize,
    prefab_instances: usize,
    managers: usize,
    has_scene_roots: bool,
) -> Vec<String> {
    let mut fields = Vec::new();
    for _ in 0..game_objects {
        fields.push("GameObjects[]".to_owned());
    }
    for _ in 0..components {
        fields.push("Components[]".to_owned());
    }
    for _ in 0..prefab_instances {
        fields.push("PrefabInstances[]".to_owned());
    }
    for _ in 0..managers {
        fields.push("Managers[]".to_owned());
    }
    if has_scene_roots {
        fields.push("SceneRoots".to_owned());
    }
    fields
}

/// Caller-reported `GameObjectExtensions.IsRoot` facts for one hierarchy GameObject.
///
/// Facts arrive in `GameObjects` order; the component lookup and the father-pointer
/// resolution (including cross-file targets) stay caller-side.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SceneHierarchyRootFact {
    /// Whether the GameObject carries a Transform component.
    pub has_transform: bool,
    /// Whether that Transform's father pointer resolved to null.
    /// Ignored when there is no Transform.
    pub father_is_null: bool,
}

/// `GameObjectExtensions.IsRoot` (`AssetRipper/Source/AssetRipper.SourceGenerated.Extensions/GameObjectExtensions.cs`
/// lines 117-120) over caller facts: a GameObject is a root when it has no Transform
/// (the source `?.` yields null) or its Transform father is null.
pub const fn is_scene_hierarchy_root(fact: SceneHierarchyRootFact) -> bool {
    !fact.has_transform || fact.father_is_null
}

/// `SceneHierarchyObject.GetRoots` (`AssetRipper/Source/AssetRipper.Processing/Prefabs/SceneHierarchyObject.cs`
/// lines 73-76): hierarchy-order indexes of the GameObjects that are roots.
///
/// This is the export-time filter over the assembled hierarchy; `discover_local_roots`
/// runs the same rule during graph discovery with diagnostics for unresolved links.
pub fn scene_hierarchy_root_indexes(facts: &[SceneHierarchyRootFact]) -> Vec<usize> {
    let mut roots = Vec::new();
    for (index, fact) in facts.iter().enumerate() {
        if is_scene_hierarchy_root(*fact) {
            roots.push(index);
        }
    }
    roots
}
