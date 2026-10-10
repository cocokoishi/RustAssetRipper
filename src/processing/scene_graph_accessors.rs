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

    pub fn objects(&self) -> &[SceneObjectRecord] {
        &self.objects
    }

    pub fn relationships(&self) -> &[SceneRelationship] {
        &self.relationships
    }

    pub fn issues(&self) -> &[SceneGraphIssue] {
        &self.issues
    }

    /// True when relevant TypeTrees decoded and every pointer uniquely resolves in the joined set.
    pub fn is_complete(&self) -> bool {
        self.issues.is_empty()
            && self.relationships.iter().all(|relationship| {
                matches!(
                    &relationship.target,
                    SceneRelationshipTarget::Null
                        | SceneRelationshipTarget::LocalObject { .. }
                        | SceneRelationshipTarget::WorkspaceObject { .. }
                )
            })
    }

    /// Find local GameObject root candidates using the source SceneHierarchyObject rule.
    ///
    /// A GameObject is a root when it has no resolved Transform component or its first resolved
    /// Transform component has a null father. Missing, ambiguous, or cross-file component and
    /// father pointers are reported as issues and do not become roots by assumption. This returns
    /// file-local candidates; assigning them to a SceneDefinition still requires collection and
    /// scene metadata that this graph does not hold.
    pub fn discover_local_roots(
        &self,
        limits: SceneRootDiscoveryLimits,
    ) -> Result<SceneRootDiscovery, SceneRootDiscoveryError> {
        let indexed_relationship_count = self
            .relationships
            .iter()
            .filter(|relationship| {
                matches!(
                    relationship.kind,
                    SceneRelationshipKind::GameObjectComponent
                        | SceneRelationshipKind::TransformParent
                )
            })
            .count();
        let mut relation_index = Vec::new();
        relation_index
            .try_reserve_exact(indexed_relationship_count)
            .map_err(|_| SceneRootDiscoveryError::AllocationFailed {
                resource: "relationship lookup index",
                requested: indexed_relationship_count,
            })?;
        for (relationship_index, relationship) in self.relationships.iter().enumerate() {
            if matches!(
                relationship.kind,
                SceneRelationshipKind::GameObjectComponent | SceneRelationshipKind::TransformParent
            ) {
                relation_index.push(IndexedSceneRelationship {
                    source_object_index: relationship.source_object_index,
                    kind: relationship.kind,
                    relationship_index,
                });
            }
        }
        relation_index.sort_unstable_by_key(|entry| {
            (
                entry.source_object_index,
                hierarchy_kind_rank(entry.kind),
                entry.relationship_index,
            )
        });

        let mut component_issue_objects = Vec::new();
        component_issue_objects
            .try_reserve_exact(self.issues.len())
            .map_err(|_| SceneRootDiscoveryError::AllocationFailed {
                resource: "GameObject component issue index",
                requested: self.issues.len(),
            })?;
        for issue in &self.issues {
            if issue.field_path.as_deref().is_some_and(|field_path| {
                field_path == "m_Component" || field_path.starts_with("m_Component[")
            }) {
                component_issue_objects.push(issue.object_index);
            }
        }
        component_issue_objects.sort_unstable();
        component_issue_objects.dedup();

        let mut discovery = SceneRootDiscovery {
            roots: Vec::new(),
            issues: Vec::new(),
        };
        for (game_object_index, object) in self.objects.iter().enumerate() {
            if object.class_id != Some(GAME_OBJECT_CLASS_ID) {
                continue;
            }
            let mut component_list_incomplete = component_issue_objects
                .binary_search(&game_object_index)
                .is_ok();
            if component_list_incomplete {
                push_scene_root_issue(
                    &mut discovery.issues,
                    limits.max_issues,
                    SceneRootDiscoveryIssue {
                        game_object_index,
                        transform_index: None,
                        kind: SceneRootDiscoveryIssueKind::ComponentListIncomplete,
                    },
                )?;
            }

            let mut selected_transform: Option<usize> = None;
            let mut transform_match_count = 0usize;
            let component_range = scene_relationship_range(
                &relation_index,
                game_object_index,
                SceneRelationshipKind::GameObjectComponent,
            );
            for entry in &relation_index[component_range] {
                let relationship = self.relationships.get(entry.relationship_index).ok_or(
                    SceneRootDiscoveryError::InvalidRelationshipIndex {
                        relationship_index: entry.relationship_index,
                        relationship_count: self.relationships.len(),
                    },
                )?;
                match &relationship.target {
                    SceneRelationshipTarget::Null => {}
                    SceneRelationshipTarget::LocalObject { object_index } => {
                        let Some(component) = self.objects.get(*object_index) else {
                            component_list_incomplete = true;
                            push_scene_root_issue(
                                &mut discovery.issues,
                                limits.max_issues,
                                SceneRootDiscoveryIssue {
                                    game_object_index,
                                    transform_index: selected_transform,
                                    kind: SceneRootDiscoveryIssueKind::UnresolvedComponent {
                                        target: relationship.target.clone(),
                                    },
                                },
                            )?;
                            continue;
                        };
                        match component.class_id {
                            Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID) => {
                                transform_match_count = transform_match_count
                                    .checked_add(1)
                                    .ok_or(SceneRootDiscoveryError::CountOverflow {
                                        resource: "Transform components",
                                    })?;
                                if selected_transform.is_none() {
                                    selected_transform = Some(*object_index);
                                }
                            }
                            Some(_) => {}
                            None => {
                                component_list_incomplete = true;
                                push_scene_root_issue(
                                    &mut discovery.issues,
                                    limits.max_issues,
                                    SceneRootDiscoveryIssue {
                                        game_object_index,
                                        transform_index: selected_transform,
                                        kind: SceneRootDiscoveryIssueKind::UnresolvedComponent {
                                            target: relationship.target.clone(),
                                        },
                                    },
                                )?;
                            }
                        }
                    }
                    target => {
                        component_list_incomplete = true;
                        push_scene_root_issue(
                            &mut discovery.issues,
                            limits.max_issues,
                            SceneRootDiscoveryIssue {
                                game_object_index,
                                transform_index: selected_transform,
                                kind: SceneRootDiscoveryIssueKind::UnresolvedComponent {
                                    target: target.clone(),
                                },
                            },
                        )?;
                    }
                }
            }
            if transform_match_count > 1 {
                if let Some(selected_transform_index) = selected_transform {
                    push_scene_root_issue(
                        &mut discovery.issues,
                        limits.max_issues,
                        SceneRootDiscoveryIssue {
                            game_object_index,
                            transform_index: Some(selected_transform_index),
                            kind: SceneRootDiscoveryIssueKind::MultipleTransformComponents {
                                selected_transform_index,
                                match_count: transform_match_count,
                            },
                        },
                    )?;
                }
            }

            if component_list_incomplete {
                continue;
            }

            let Some(transform_index) = selected_transform else {
                push_scene_root_candidate(
                    &mut discovery.roots,
                    limits.max_roots,
                    SceneRootCandidate {
                        game_object_index,
                        transform_index: None,
                        reason: SceneRootReason::NoTransformComponent,
                    },
                )?;
                continue;
            };

            let father_range = scene_relationship_range(
                &relation_index,
                transform_index,
                SceneRelationshipKind::TransformParent,
            );
            let father_entries = &relation_index[father_range];
            match father_entries {
                [] => push_scene_root_issue(
                    &mut discovery.issues,
                    limits.max_issues,
                    SceneRootDiscoveryIssue {
                        game_object_index,
                        transform_index: Some(transform_index),
                        kind: SceneRootDiscoveryIssueKind::MissingTransformParent,
                    },
                )?,
                [entry] => {
                    let father = self.relationships.get(entry.relationship_index).ok_or(
                        SceneRootDiscoveryError::InvalidRelationshipIndex {
                            relationship_index: entry.relationship_index,
                            relationship_count: self.relationships.len(),
                        },
                    )?;
                    match &father.target {
                        SceneRelationshipTarget::Null => push_scene_root_candidate(
                            &mut discovery.roots,
                            limits.max_roots,
                            SceneRootCandidate {
                                game_object_index,
                                transform_index: Some(transform_index),
                                reason: SceneRootReason::NullFather,
                            },
                        )?,
                        SceneRelationshipTarget::LocalObject { object_index } => {
                            let parent_is_transform =
                                self.objects.get(*object_index).is_some_and(|parent| {
                                    matches!(
                                        parent.class_id,
                                        Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)
                                    )
                                });
                            if !parent_is_transform {
                                push_scene_root_issue(
                                    &mut discovery.issues,
                                    limits.max_issues,
                                    SceneRootDiscoveryIssue {
                                        game_object_index,
                                        transform_index: Some(transform_index),
                                        kind:
                                            SceneRootDiscoveryIssueKind::UnresolvedTransformParent {
                                                target: father.target.clone(),
                                            },
                                    },
                                )?;
                            }
                        }
                        target => push_scene_root_issue(
                            &mut discovery.issues,
                            limits.max_issues,
                            SceneRootDiscoveryIssue {
                                game_object_index,
                                transform_index: Some(transform_index),
                                kind: SceneRootDiscoveryIssueKind::UnresolvedTransformParent {
                                    target: target.clone(),
                                },
                            },
                        )?,
                    }
                }
                entries => push_scene_root_issue(
                    &mut discovery.issues,
                    limits.max_issues,
                    SceneRootDiscoveryIssue {
                        game_object_index,
                        transform_index: Some(transform_index),
                        kind: SceneRootDiscoveryIssueKind::DuplicateTransformParent {
                            match_count: entries.len(),
                        },
                    },
                )?,
            }
        }
        Ok(discovery)
    }

    /// Discover roots while resolving component and father targets across loaded files.
    ///
    /// The scene graph is resolved against the supplied graph before discovery; source file and
    /// object identities are checked by `resolve_with_multi_file_graph`. The graph supplies target
    /// class IDs and the `m_Father` edge for external Transforms. Missing or undecodable targets
    /// remain issues, never roots.
    pub fn discover_workspace_roots(
        &mut self,
        source_file_index: usize,
        source_file_name: &str,
        graph: &MultiFileReferenceGraph<'_>,
        limits: SceneRootDiscoveryLimits,
    ) -> Result<WorkspaceSceneRootDiscovery, SceneRootDiscoveryError> {
        self.resolve_with_multi_file_graph(source_file_index, source_file_name, graph)
            .map_err(|source| SceneRootDiscoveryError::ReferenceGraphMismatch {
                message: source.to_string(),
            })?;
        let indexed_relationship_count = self
            .relationships
            .iter()
            .filter(|relationship| {
                matches!(
                    relationship.kind,
                    SceneRelationshipKind::GameObjectComponent
                        | SceneRelationshipKind::TransformParent
                )
            })
            .count();
        let mut relation_index = Vec::new();
        relation_index
            .try_reserve_exact(indexed_relationship_count)
            .map_err(|_| SceneRootDiscoveryError::AllocationFailed {
                resource: "workspace root relationship lookup index",
                requested: indexed_relationship_count,
            })?;
        for (relationship_index, relationship) in self.relationships.iter().enumerate() {
            if matches!(
                relationship.kind,
                SceneRelationshipKind::GameObjectComponent | SceneRelationshipKind::TransformParent
            ) {
                relation_index.push(IndexedSceneRelationship {
                    source_object_index: relationship.source_object_index,
                    kind: relationship.kind,
                    relationship_index,
                });
            }
        }
        relation_index.sort_unstable_by_key(|entry| {
            (
                entry.source_object_index,
                hierarchy_kind_rank(entry.kind),
                entry.relationship_index,
            )
        });

        let mut component_issue_objects = Vec::new();
        component_issue_objects
            .try_reserve_exact(self.issues.len())
            .map_err(|_| SceneRootDiscoveryError::AllocationFailed {
                resource: "workspace GameObject component issue index",
                requested: self.issues.len(),
            })?;
        for issue in &self.issues {
            if issue.field_path.as_deref().is_some_and(|field_path| {
                field_path == "m_Component" || field_path.starts_with("m_Component[")
            }) {
                component_issue_objects.push(issue.object_index);
            }
        }
        component_issue_objects.sort_unstable();
        component_issue_objects.dedup();

        let mut discovery = WorkspaceSceneRootDiscovery {
            roots: Vec::new(),
            issues: Vec::new(),
        };
        for (game_object_index, game_object) in self.objects.iter().enumerate() {
            if game_object.class_id != Some(GAME_OBJECT_CLASS_ID) {
                continue;
            }
            let mut component_list_incomplete = component_issue_objects
                .binary_search(&game_object_index)
                .is_ok();
            if component_list_incomplete {
                push_workspace_scene_root_issue(
                    &mut discovery,
                    limits.max_issues,
                    WorkspaceSceneRootDiscoveryIssue {
                        source_file_index,
                        game_object_index,
                        transform_file_index: None,
                        transform_index: None,
                        kind: WorkspaceSceneRootDiscoveryIssueKind::ComponentListIncomplete,
                    },
                )?;
            }

            let mut selected_transform: Option<(usize, usize)> = None;
            let mut transform_match_count = 0usize;
            let component_range = scene_relationship_range(
                &relation_index,
                game_object_index,
                SceneRelationshipKind::GameObjectComponent,
            );
            for entry in &relation_index[component_range] {
                let relationship = self.relationships.get(entry.relationship_index).ok_or(
                    SceneRootDiscoveryError::InvalidRelationshipIndex {
                        relationship_index: entry.relationship_index,
                        relationship_count: self.relationships.len(),
                    },
                )?;
                let target_identity = match &relationship.target {
                    SceneRelationshipTarget::Null => continue,
                    SceneRelationshipTarget::LocalObject { object_index } => {
                        Some((source_file_index, *object_index))
                    }
                    SceneRelationshipTarget::WorkspaceObject {
                        file_index,
                        object_index,
                    } => Some((*file_index, *object_index)),
                    target => {
                        component_list_incomplete = true;
                        push_workspace_scene_root_issue(
                            &mut discovery,
                            limits.max_issues,
                            WorkspaceSceneRootDiscoveryIssue {
                                source_file_index,
                                game_object_index,
                                transform_file_index: selected_transform.map(|identity| identity.0),
                                transform_index: selected_transform.map(|identity| identity.1),
                                kind: WorkspaceSceneRootDiscoveryIssueKind::UnresolvedComponent {
                                    target: target.clone(),
                                },
                            },
                        )?;
                        None
                    }
                };
                let Some((target_file_index, target_object_index)) = target_identity else {
                    continue;
                };
                match graph.object_class_id(target_file_index, target_object_index) {
                    Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID) => {
                        transform_match_count = transform_match_count.checked_add(1).ok_or(
                            SceneRootDiscoveryError::CountOverflow {
                                resource: "workspace Transform components",
                            },
                        )?;
                        if selected_transform.is_none() {
                            selected_transform = Some((target_file_index, target_object_index));
                        }
                    }
                    Some(_) => {}
                    None => {
                        component_list_incomplete = true;
                        push_workspace_scene_root_issue(
                            &mut discovery,
                            limits.max_issues,
                            WorkspaceSceneRootDiscoveryIssue {
                                source_file_index,
                                game_object_index,
                                transform_file_index: selected_transform.map(|identity| identity.0),
                                transform_index: selected_transform.map(|identity| identity.1),
                                kind: WorkspaceSceneRootDiscoveryIssueKind::UnresolvedComponent {
                                    target: relationship.target.clone(),
                                },
                            },
                        )?;
                    }
                }
            }
            if transform_match_count > 1 {
                if let Some((transform_file_index, selected_transform_index)) = selected_transform {
                    push_workspace_scene_root_issue(
                        &mut discovery,
                        limits.max_issues,
                        WorkspaceSceneRootDiscoveryIssue {
                            source_file_index,
                            game_object_index,
                            transform_file_index: Some(transform_file_index),
                            transform_index: Some(selected_transform_index),
                            kind:
                                WorkspaceSceneRootDiscoveryIssueKind::MultipleTransformComponents {
                                    selected_transform_file_index: transform_file_index,
                                    selected_transform_index,
                                    match_count: transform_match_count,
                                },
                        },
                    )?;
                }
            }
            if component_list_incomplete {
                continue;
            }

            let Some((transform_file_index, transform_index)) = selected_transform else {
                push_workspace_scene_root_candidate(
                    &mut discovery,
                    limits.max_roots,
                    WorkspaceSceneRootCandidate {
                        source_file_index,
                        game_object_index,
                        transform_file_index: None,
                        transform_index: None,
                        reason: SceneRootReason::NoTransformComponent,
                    },
                )?;
                continue;
            };
            let Some(transform_identity) =
                graph.object_identity(transform_file_index, transform_index)
            else {
                push_workspace_scene_root_issue(
                    &mut discovery,
                    limits.max_issues,
                    WorkspaceSceneRootDiscoveryIssue {
                        source_file_index,
                        game_object_index,
                        transform_file_index: Some(transform_file_index),
                        transform_index: Some(transform_index),
                        kind: WorkspaceSceneRootDiscoveryIssueKind::UnresolvedComponent {
                            target: SceneRelationshipTarget::WorkspaceObject {
                                file_index: transform_file_index,
                                object_index: transform_index,
                            },
                        },
                    },
                )?;
                continue;
            };
            let mut father_references = graph
                .references_from_object(transform_file_index, transform_index)
                .filter(|reference| {
                    reference.source_path_id == transform_identity.path_id
                        && reference_field_path_matches(reference, "m_Father")
                });
            let Some(father_reference) = father_references.next() else {
                push_workspace_scene_root_issue(
                    &mut discovery,
                    limits.max_issues,
                    WorkspaceSceneRootDiscoveryIssue {
                        source_file_index,
                        game_object_index,
                        transform_file_index: Some(transform_file_index),
                        transform_index: Some(transform_index),
                        kind: WorkspaceSceneRootDiscoveryIssueKind::MissingTransformParent,
                    },
                )?;
                continue;
            };
            if father_references.next().is_some() {
                let match_count = graph
                    .references_from_object(transform_file_index, transform_index)
                    .filter(|reference| {
                        reference.source_path_id == transform_identity.path_id
                            && reference_field_path_matches(reference, "m_Father")
                    })
                    .count();
                push_workspace_scene_root_issue(
                    &mut discovery,
                    limits.max_issues,
                    WorkspaceSceneRootDiscoveryIssue {
                        source_file_index,
                        game_object_index,
                        transform_file_index: Some(transform_file_index),
                        transform_index: Some(transform_index),
                        kind: WorkspaceSceneRootDiscoveryIssueKind::DuplicateTransformParent {
                            match_count,
                        },
                    },
                )?;
                continue;
            }
            match &father_reference.resolution {
                MultiFilePPtrResolution::Null => push_workspace_scene_root_candidate(
                    &mut discovery,
                    limits.max_roots,
                    WorkspaceSceneRootCandidate {
                        source_file_index,
                        game_object_index,
                        transform_file_index: Some(transform_file_index),
                        transform_index: Some(transform_index),
                        reason: SceneRootReason::NullFather,
                    },
                )?,
                MultiFilePPtrResolution::Object {
                    target_file_index,
                    target_object_index,
                } if matches!(
                    graph.object_class_id(*target_file_index, *target_object_index),
                    Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID)
                ) => {}
                resolution => push_workspace_scene_root_issue(
                    &mut discovery,
                    limits.max_issues,
                    WorkspaceSceneRootDiscoveryIssue {
                        source_file_index,
                        game_object_index,
                        transform_file_index: Some(transform_file_index),
                        transform_index: Some(transform_index),
                        kind: WorkspaceSceneRootDiscoveryIssueKind::UnresolvedTransformParent {
                            target: scene_target_from_multi_file_resolution(
                                resolution,
                                transform_file_index,
                            ),
                        },
                    },
                )?,
            }
        }
        Ok(discovery)
    }

    /// Audit reciprocal parent/child pointers whose targets resolve inside this scene graph.
    ///
    /// Cross-file targets are not checked here because this graph contains scene values for only
    /// one file. The source editor conversion requires each non-root Transform to appear in its
    /// father's child array when it calculates root order; this method reports that and related
    /// contradictions without panicking or repairing the serialized values.
    pub fn audit_local_transform_links(
        &self,
    ) -> Result<Vec<SceneHierarchyIssue>, SceneGraphAuditError> {
        let relationship_count = self
            .relationships
            .iter()
            .filter(|relationship| {
                matches!(
                    relationship.kind,
                    SceneRelationshipKind::TransformParent | SceneRelationshipKind::TransformChild
                )
            })
            .count();
        let mut index = Vec::new();
        index.try_reserve_exact(relationship_count).map_err(|_| {
            SceneGraphAuditError::AllocationFailed {
                resource: "local Transform relationship index",
                requested: relationship_count,
            }
        })?;
        for (relationship_index, relationship) in self.relationships.iter().enumerate() {
            if !matches!(
                relationship.kind,
                SceneRelationshipKind::TransformParent | SceneRelationshipKind::TransformChild
            ) {
                continue;
            }
            index.push(IndexedHierarchyRelationship {
                source_object_index: relationship.source_object_index,
                kind: relationship.kind,
                target_local_object_index: match relationship.target {
                    SceneRelationshipTarget::LocalObject { object_index } => Some(object_index),
                    _ => None,
                },
                relationship_index,
            });
        }
        index.sort_unstable_by_key(|entry| {
            (
                entry.source_object_index,
                hierarchy_kind_rank(entry.kind),
                entry.target_local_object_index.unwrap_or(usize::MAX),
                entry.relationship_index,
            )
        });

        let mut issues = Vec::new();
        issues.try_reserve_exact(relationship_count).map_err(|_| {
            SceneGraphAuditError::AllocationFailed {
                resource: "local Transform consistency issues",
                requested: relationship_count,
            }
        })?;
        for (relationship_index, relationship) in self.relationships.iter().enumerate() {
            let counterpart_kind = match relationship.kind {
                SceneRelationshipKind::TransformParent => SceneRelationshipKind::TransformChild,
                SceneRelationshipKind::TransformChild => SceneRelationshipKind::TransformParent,
                _ => continue,
            };
            let SceneRelationshipTarget::LocalObject {
                object_index: target_index,
            } = &relationship.target
            else {
                continue;
            };
            let target_index = *target_index;
            let Some(target_object) = self.objects.get(target_index) else {
                continue;
            };
            match target_object.class_id {
                Some(TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID) => {}
                Some(class_id) => {
                    issues.push(SceneHierarchyIssue {
                        relationship_index,
                        source_object_index: relationship.source_object_index,
                        target_object_index: target_index,
                        field_path: relationship.field_path.clone(),
                        kind: SceneHierarchyIssueKind::TargetIsNotTransform { class_id },
                    });
                    continue;
                }
                None => {
                    issues.push(SceneHierarchyIssue {
                        relationship_index,
                        source_object_index: relationship.source_object_index,
                        target_object_index: target_index,
                        field_path: relationship.field_path.clone(),
                        kind: SceneHierarchyIssueKind::TargetClassUnknown,
                    });
                    continue;
                }
            }

            let counterpart_source_index = target_index;
            let expected_target_index = relationship.source_object_index;
            let range =
                hierarchy_relationship_range(&index, counterpart_source_index, counterpart_kind);
            let entries = &index[range];
            match relationship.kind {
                SceneRelationshipKind::TransformParent => {
                    let match_count = entries
                        .iter()
                        .filter(|entry| {
                            entry.target_local_object_index == Some(expected_target_index)
                        })
                        .count();
                    let kind = match match_count {
                        0 => Some(SceneHierarchyIssueKind::MissingReciprocal { counterpart_kind }),
                        1 => None,
                        match_count => Some(SceneHierarchyIssueKind::DuplicateReciprocal {
                            counterpart_kind,
                            match_count,
                        }),
                    };
                    if let Some(kind) = kind {
                        issues.push(SceneHierarchyIssue {
                            relationship_index,
                            source_object_index: relationship.source_object_index,
                            target_object_index: target_index,
                            field_path: relationship.field_path.clone(),
                            kind,
                        });
                    }
                }
                SceneRelationshipKind::TransformChild => match entries {
                    [] => issues.push(SceneHierarchyIssue {
                        relationship_index,
                        source_object_index: relationship.source_object_index,
                        target_object_index: target_index,
                        field_path: relationship.field_path.clone(),
                        kind: SceneHierarchyIssueKind::MissingReciprocal { counterpart_kind },
                    }),
                    [entry] if entry.target_local_object_index == Some(expected_target_index) => {}
                    [entry] => {
                        let counterpart = self.relationships.get(entry.relationship_index).ok_or(
                            SceneGraphAuditError::InvalidRelationshipIndex {
                                relationship_index: entry.relationship_index,
                                relationship_count: self.relationships.len(),
                            },
                        )?;
                        issues.push(SceneHierarchyIssue {
                            relationship_index,
                            source_object_index: relationship.source_object_index,
                            target_object_index: target_index,
                            field_path: relationship.field_path.clone(),
                            kind: SceneHierarchyIssueKind::MismatchedReciprocalTarget {
                                counterpart_kind,
                                actual_target: counterpart.target.clone(),
                            },
                        });
                    }
                    entries => issues.push(SceneHierarchyIssue {
                        relationship_index,
                        source_object_index: relationship.source_object_index,
                        target_object_index: target_index,
                        field_path: relationship.field_path.clone(),
                        kind: SceneHierarchyIssueKind::DuplicateReciprocal {
                            counterpart_kind,
                            match_count: entries.len(),
                        },
                    }),
                },
                _ => {}
            }
        }
        Ok(issues)
    }

    /// Audit reciprocal Transform parent/child pointers across every loaded SerializedFile.
    ///
    /// A missing reciprocal edge is reported only when the target object's reference scan is
    /// complete. If its TypeTree payload or one of its fields could not be scanned, the result is
    /// marked incomplete instead of claiming that the edge is absent.
    pub fn audit_workspace_transform_links(
        &mut self,
        source_file_index: usize,
        source_file_name: &str,
        graph: &MultiFileReferenceGraph<'_>,
        limits: SceneHierarchyAuditLimits,
    ) -> Result<WorkspaceSceneHierarchyAudit, SceneGraphAuditError> {
        self.resolve_with_multi_file_graph(source_file_index, source_file_name, graph)
            .map_err(|source| SceneGraphAuditError::ReferenceGraphMismatch {
                message: source.to_string(),
            })?;

        let relationship_count = self
            .relationships
            .iter()
            .filter(|relationship| {
                matches!(
                    relationship.kind,
                    SceneRelationshipKind::TransformParent | SceneRelationshipKind::TransformChild
                )
            })
            .count();
        if relationship_count > limits.max_relationships {
            return Err(SceneGraphAuditError::RelationshipLimitExceeded {
                count: relationship_count,
                limit: limits.max_relationships,
            });
        }

        let mut audit = WorkspaceSceneHierarchyAudit {
            relationships_checked: 0,
            issues: Vec::new(),
        };
        for (relationship_index, relationship) in self.relationships.iter().enumerate() {
            let counterpart_kind = match relationship.kind {
                SceneRelationshipKind::TransformParent => SceneRelationshipKind::TransformChild,
                SceneRelationshipKind::TransformChild => SceneRelationshipKind::TransformParent,
                _ => continue,
            };
            audit.relationships_checked = audit.relationships_checked.checked_add(1).ok_or(
                SceneGraphAuditError::CountOverflow {
                    resource: "workspace Transform relationships",
                },
            )?;

            if graph.object_scan_is_complete(source_file_index, relationship.source_object_index)
                != Some(true)
            {
                push_workspace_hierarchy_issue(
                    &mut audit,
                    limits.max_issues,
                    WorkspaceSceneHierarchyIssue {
                        source_file_index,
                        relationship_index,
                        source_object_index: relationship.source_object_index,
                        source_path_id: relationship.source_path_id,
                        target_file_index: None,
                        target_object_index: None,
                        field_path: relationship.field_path.clone(),
                        kind: SceneHierarchyIssueKind::SourceScanIncomplete,
                    },
                )?;
                continue;
            }

            if relationship.kind == SceneRelationshipKind::TransformParent
                && relationship.target == SceneRelationshipTarget::Null
            {
                continue;
            }

            let (target_file_index, target_object_index) = match &relationship.target {
                SceneRelationshipTarget::LocalObject { object_index } => {
                    (source_file_index, *object_index)
                }
                SceneRelationshipTarget::WorkspaceObject {
                    file_index,
                    object_index,
                } => (*file_index, *object_index),
                target => {
                    push_workspace_hierarchy_issue(
                        &mut audit,
                        limits.max_issues,
                        WorkspaceSceneHierarchyIssue {
                            source_file_index,
                            relationship_index,
                            source_object_index: relationship.source_object_index,
                            source_path_id: relationship.source_path_id,
                            target_file_index: None,
                            target_object_index: None,
                            field_path: relationship.field_path.clone(),
                            kind: SceneHierarchyIssueKind::UnresolvedTarget {
                                target: target.clone(),
                            },
                        },
                    )?;
                    continue;
                }
            };

            let Some(target_class_id) =
                graph.object_class_id(target_file_index, target_object_index)
            else {
                push_workspace_hierarchy_issue(
                    &mut audit,
                    limits.max_issues,
                    WorkspaceSceneHierarchyIssue {
                        source_file_index,
                        relationship_index,
                        source_object_index: relationship.source_object_index,
                        source_path_id: relationship.source_path_id,
                        target_file_index: Some(target_file_index),
                        target_object_index: Some(target_object_index),
                        field_path: relationship.field_path.clone(),
                        kind: SceneHierarchyIssueKind::TargetClassUnknown,
                    },
                )?;
                continue;
            };
            if !matches!(
                target_class_id,
                TRANSFORM_CLASS_ID | RECT_TRANSFORM_CLASS_ID
            ) {
                push_workspace_hierarchy_issue(
                    &mut audit,
                    limits.max_issues,
                    WorkspaceSceneHierarchyIssue {
                        source_file_index,
                        relationship_index,
                        source_object_index: relationship.source_object_index,
                        source_path_id: relationship.source_path_id,
                        target_file_index: Some(target_file_index),
                        target_object_index: Some(target_object_index),
                        field_path: relationship.field_path.clone(),
                        kind: SceneHierarchyIssueKind::TargetIsNotTransform {
                            class_id: target_class_id,
                        },
                    },
                )?;
                continue;
            }

            if graph.object_scan_is_complete(target_file_index, target_object_index) != Some(true) {
                push_workspace_hierarchy_issue(
                    &mut audit,
                    limits.max_issues,
                    WorkspaceSceneHierarchyIssue {
                        source_file_index,
                        relationship_index,
                        source_object_index: relationship.source_object_index,
                        source_path_id: relationship.source_path_id,
                        target_file_index: Some(target_file_index),
                        target_object_index: Some(target_object_index),
                        field_path: relationship.field_path.clone(),
                        kind: SceneHierarchyIssueKind::CounterpartScanIncomplete,
                    },
                )?;
                continue;
            }

            let Some(target_identity) =
                graph.object_identity(target_file_index, target_object_index)
            else {
                push_workspace_hierarchy_issue(
                    &mut audit,
                    limits.max_issues,
                    WorkspaceSceneHierarchyIssue {
                        source_file_index,
                        relationship_index,
                        source_object_index: relationship.source_object_index,
                        source_path_id: relationship.source_path_id,
                        target_file_index: Some(target_file_index),
                        target_object_index: Some(target_object_index),
                        field_path: relationship.field_path.clone(),
                        kind: SceneHierarchyIssueKind::TargetClassUnknown,
                    },
                )?;
                continue;
            };
            let mut counterpart_edges = graph
                .references_from_object(target_file_index, target_object_index)
                .filter(|reference| {
                    if reference.source_path_id != target_identity.path_id {
                        return false;
                    }
                    match counterpart_kind {
                        SceneRelationshipKind::TransformParent => {
                            reference_field_path_matches(reference, "m_Father")
                        }
                        SceneRelationshipKind::TransformChild => {
                            reference_field_path_matches_array_element(reference, "m_Children")
                        }
                        _ => false,
                    }
                });
            let Some(counterpart_edge) = counterpart_edges.next() else {
                push_workspace_hierarchy_issue(
                    &mut audit,
                    limits.max_issues,
                    WorkspaceSceneHierarchyIssue {
                        source_file_index,
                        relationship_index,
                        source_object_index: relationship.source_object_index,
                        source_path_id: relationship.source_path_id,
                        target_file_index: Some(target_file_index),
                        target_object_index: Some(target_object_index),
                        field_path: relationship.field_path.clone(),
                        kind: SceneHierarchyIssueKind::MissingReciprocal { counterpart_kind },
                    },
                )?;
                continue;
            };

            let Some(_second_edge) = counterpart_edges.next() else {
                if !matches!(
                    counterpart_edge.resolution,
                    MultiFilePPtrResolution::Object {
                        target_file_index: actual_file_index,
                        target_object_index: actual_object_index,
                    } if actual_file_index == source_file_index
                        && actual_object_index == relationship.source_object_index
                ) {
                    push_workspace_hierarchy_issue(
                        &mut audit,
                        limits.max_issues,
                        WorkspaceSceneHierarchyIssue {
                            source_file_index,
                            relationship_index,
                            source_object_index: relationship.source_object_index,
                            source_path_id: relationship.source_path_id,
                            target_file_index: Some(target_file_index),
                            target_object_index: Some(target_object_index),
                            field_path: relationship.field_path.clone(),
                            kind: SceneHierarchyIssueKind::MismatchedReciprocalTarget {
                                counterpart_kind,
                                actual_target: scene_target_from_multi_file_resolution(
                                    &counterpart_edge.resolution,
                                    target_file_index,
                                ),
                            },
                        },
                    )?;
                }
                continue;
            };
            let match_count = counterpart_edges.count().checked_add(2).ok_or(
                SceneGraphAuditError::CountOverflow {
                    resource: "workspace reciprocal edge matches",
                },
            )?;
            push_workspace_hierarchy_issue(
                &mut audit,
                limits.max_issues,
                WorkspaceSceneHierarchyIssue {
                    source_file_index,
                    relationship_index,
                    source_object_index: relationship.source_object_index,
                    source_path_id: relationship.source_path_id,
                    target_file_index: Some(target_file_index),
                    target_object_index: Some(target_object_index),
                    field_path: relationship.field_path.clone(),
                    kind: SceneHierarchyIssueKind::DuplicateReciprocal {
                        counterpart_kind,
                        match_count,
                    },
                },
            )?;
        }
        Ok(audit)
    }

    /// Resolve every scene edge through a cross-file graph built from the same ordered workspace.
    ///
    /// The source file name and every object-table PathID are checked before any target is
    /// changed. The update is transactional: a missing or ambiguous field-path match leaves the
    /// original per-file targets intact.
    pub fn resolve_with_multi_file_graph(
        &mut self,
        source_file_index: usize,
        source_file_name: &str,
        graph: &MultiFileReferenceGraph<'_>,
    ) -> Result<(), SceneGraphResolveError> {
        let Some(graph_file_name) = graph.normalized_file_names().get(source_file_index) else {
            return Err(SceneGraphResolveError::SourceFileIndexOutOfBounds {
                file_index: source_file_index,
                file_count: graph.file_count(),
            });
        };
        let normalized_source_name =
            crate::io::special_names::fix_file_identifier(source_file_name);
        if normalized_source_name != *graph_file_name {
            return Err(SceneGraphResolveError::SourceFileNameMismatch {
                file_index: source_file_index,
                expected: graph_file_name.clone(),
                actual: normalized_source_name,
            });
        }
        for object in &self.objects {
            let identity = graph
                .object_identity(source_file_index, object.object_index)
                .ok_or(SceneGraphResolveError::ObjectTableMismatch {
                    file_index: source_file_index,
                    object_index: object.object_index,
                })?;
            if identity.path_id != object.path_id {
                return Err(SceneGraphResolveError::ObjectIdentityMismatch {
                    file_index: source_file_index,
                    object_index: object.object_index,
                    scene_path_id: object.path_id,
                    graph_path_id: identity.path_id,
                });
            }
        }
        let reference_object_count = graph.file_object_count(source_file_index).ok_or(
            SceneGraphResolveError::SourceFileIndexOutOfBounds {
                file_index: source_file_index,
                file_count: graph.file_count(),
            },
        )?;
        if reference_object_count != self.objects.len() {
            return Err(SceneGraphResolveError::ObjectCountMismatch {
                file_index: source_file_index,
                scene_graph_object_count: self.objects.len(),
                reference_object_count,
            });
        }

        let mut resolved_targets = Vec::new();
        resolved_targets
            .try_reserve_exact(self.relationships.len())
            .map_err(|_| SceneGraphResolveError::AllocationFailed {
                requested: self.relationships.len(),
            })?;
        for (relationship_index, relationship) in self.relationships.iter().enumerate() {
            let mut matches = graph
                .references_from_object(source_file_index, relationship.source_object_index)
                .filter(|reference| {
                    reference.source_path_id == relationship.source_path_id
                        && reference_field_path_matches(reference, &relationship.field_path)
                });
            let Some(reference) = matches.next() else {
                return Err(SceneGraphResolveError::ReferenceFieldNotFound {
                    relationship_index,
                    object_index: relationship.source_object_index,
                    field_path: relationship.field_path.clone(),
                });
            };
            if matches.next().is_some() {
                return Err(SceneGraphResolveError::ReferenceFieldAmbiguous {
                    relationship_index,
                    object_index: relationship.source_object_index,
                    field_path: relationship.field_path.clone(),
                });
            }
            if reference.file_id != relationship.file_id
                || reference.path_id != relationship.path_id
            {
                return Err(SceneGraphResolveError::ReferenceValueMismatch {
                    relationship_index,
                    object_index: relationship.source_object_index,
                    field_path: relationship.field_path.clone(),
                    scene_file_id: relationship.file_id,
                    scene_path_id: relationship.path_id,
                    graph_file_id: reference.file_id,
                    graph_path_id: reference.path_id,
                });
            }
            resolved_targets.push(scene_target_from_multi_file_resolution(
                &reference.resolution,
                source_file_index,
            ));
        }
        for (relationship, target) in self.relationships.iter_mut().zip(resolved_targets) {
            relationship.target = target;
        }
        Ok(())
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
