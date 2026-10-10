use super::*;

impl SceneGraph {
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
pub(super) fn reference_field_path_matches(
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

pub(super) fn reference_field_path_matches_array_element(
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

pub(super) fn scene_target_from_multi_file_resolution(
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
