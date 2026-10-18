use super::reference_mapping::WorkspaceObjectKey;
use std::collections::HashMap;

use super::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceSceneHierarchyMembershipIssue {
    IncompleteCollectionGraph {
        file_index: usize,
    },
    IncompleteReferenceScans {
        file_index: usize,
    },
    InvalidSceneAssetIdentity {
        identity: WorkspaceSceneObjectIdentity,
        collection_file_index: usize,
        expected_class_id: Option<i32>,
    },
    DuplicateSceneAsset {
        identity: WorkspaceSceneObjectIdentity,
    },
    ConflictingSceneAssetIdentity {
        existing: WorkspaceSceneObjectIdentity,
        conflicting: WorkspaceSceneObjectIdentity,
    },
    ConflictingComponentIdentity {
        existing: WorkspaceSceneObjectIdentity,
        conflicting: WorkspaceSceneObjectIdentity,
    },
    ConflictingComponentGameObjectEvidence {
        identity: WorkspaceSceneObjectIdentity,
        existing: WorkspaceSceneComponentEvidence,
        conflicting: WorkspaceSceneComponentEvidence,
    },
    ComponentOutsideSceneCollections {
        identity: WorkspaceSceneObjectIdentity,
        evidence: WorkspaceSceneComponentEvidence,
    },
    ComponentClassIdUnavailable {
        identity: WorkspaceSceneObjectIdentity,
    },
    MonoBehaviourGameObjectReferenceUnavailable {
        identity: WorkspaceSceneObjectIdentity,
    },
    UnclassifiedComponentCandidate {
        identity: WorkspaceSceneObjectIdentity,
        evidence: WorkspaceSceneComponentEvidence,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceSceneHierarchyMembership {
    game_objects: Vec<WorkspaceSceneObjectIdentity>,
    components: Vec<WorkspaceSceneObjectIdentity>,
    prefab_instances: Vec<WorkspaceSceneObjectIdentity>,
    managers: Vec<WorkspaceSceneObjectIdentity>,
    scene_roots: Option<WorkspaceSceneObjectIdentity>,
    issues: Vec<WorkspaceSceneHierarchyMembershipIssue>,
}

impl WorkspaceSceneHierarchyMembership {
    pub fn game_objects(&self) -> &[WorkspaceSceneObjectIdentity] {
        &self.game_objects
    }

    pub fn components(&self) -> &[WorkspaceSceneObjectIdentity] {
        &self.components
    }

    pub fn prefab_instances(&self) -> &[WorkspaceSceneObjectIdentity] {
        &self.prefab_instances
    }

    pub fn managers(&self) -> &[WorkspaceSceneObjectIdentity] {
        &self.managers
    }

    pub fn scene_roots(&self) -> Option<WorkspaceSceneObjectIdentity> {
        self.scene_roots
    }

    pub fn issues(&self) -> &[WorkspaceSceneHierarchyMembershipIssue] {
        &self.issues
    }

    pub fn is_complete(&self) -> bool {
        self.issues.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceSceneHierarchyMembershipError {
    AssetLimitExceeded {
        requested: usize,
        limit: usize,
    },
    IssueLimitExceeded {
        requested: usize,
        limit: usize,
    },
    CountOverflow {
        resource: &'static str,
    },
    DuplicateCollectionFileIndex {
        file_index: usize,
    },
    AllocationFailed {
        resource: &'static str,
        requested: usize,
    },
}

impl Display for WorkspaceSceneHierarchyMembershipError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AssetLimitExceeded { requested, limit } => write!(
                f,
                "Scene hierarchy membership needs {requested} assets/candidates, exceeding limit {limit}"
            ),
            Self::IssueLimitExceeded { requested, limit } => write!(
                f,
                "Scene hierarchy membership has {requested} issues, exceeding limit {limit}"
            ),
            Self::CountOverflow { resource } => {
                write!(f, "Scene hierarchy membership count overflowed for {resource}")
            }
            Self::DuplicateCollectionFileIndex { file_index } => write!(
                f,
                "SceneDefinition repeats workspace collection file index {file_index}"
            ),
            Self::AllocationFailed { resource, requested } => {
                write!(f, "could not allocate {requested} entries for {resource}")
            }
        }
    }
}

impl Error for WorkspaceSceneHierarchyMembershipError {}

fn charge_membership_asset(
    count: &mut usize,
    limit: usize,
) -> Result<(), WorkspaceSceneHierarchyMembershipError> {
    let requested =
        count
            .checked_add(1)
            .ok_or(WorkspaceSceneHierarchyMembershipError::CountOverflow {
                resource: "ordered asset membership",
            })?;
    if requested > limit {
        return Err(WorkspaceSceneHierarchyMembershipError::AssetLimitExceeded {
            requested,
            limit,
        });
    }
    *count = requested;
    Ok(())
}

fn push_membership_asset(
    count: &mut usize,
    assets: &mut Vec<WorkspaceSceneObjectIdentity>,
    identity: WorkspaceSceneObjectIdentity,
    limit: usize,
) -> Result<(), WorkspaceSceneHierarchyMembershipError> {
    let requested =
        count
            .checked_add(1)
            .ok_or(WorkspaceSceneHierarchyMembershipError::CountOverflow {
                resource: "ordered asset membership",
            })?;
    if requested > limit {
        return Err(WorkspaceSceneHierarchyMembershipError::AssetLimitExceeded {
            requested,
            limit,
        });
    }
    assets.try_reserve(1).map_err(|_| {
        WorkspaceSceneHierarchyMembershipError::AllocationFailed {
            resource: "ordered Scene hierarchy category",
            requested,
        }
    })?;
    assets.push(identity);
    *count = requested;
    Ok(())
}

fn push_membership_issue(
    issues: &mut Vec<WorkspaceSceneHierarchyMembershipIssue>,
    issue: WorkspaceSceneHierarchyMembershipIssue,
    limit: usize,
) -> Result<(), WorkspaceSceneHierarchyMembershipError> {
    let requested = issues.len().checked_add(1).ok_or(
        WorkspaceSceneHierarchyMembershipError::CountOverflow {
            resource: "membership diagnostics",
        },
    )?;
    if requested > limit {
        return Err(WorkspaceSceneHierarchyMembershipError::IssueLimitExceeded {
            requested,
            limit,
        });
    }
    issues.try_reserve(1).map_err(|_| {
        WorkspaceSceneHierarchyMembershipError::AllocationFailed {
            resource: "Scene hierarchy membership diagnostics",
            requested,
        }
    })?;
    issues.push(issue);
    Ok(())
}

impl WorkspaceSceneHierarchyScene {
    pub fn collections(&self) -> &[WorkspaceSceneCollectionHierarchy] {
        &self.collections
    }

    /// Reconstructs the category membership exposed by the baseline
    /// `SceneHierarchyObject.Assets`, retaining source collection and object order.
    ///
    /// Components are included when backed by a Transform class ID, a decoded GameObject
    /// `m_Component` reference, or the baseline MonoBehaviour class with a non-null
    /// `m_GameObject` field. A null MonoBehaviour pointer excludes it; other `m_GameObject`-only
    /// candidates remain ambiguous. Callers must not treat partial membership as export-ready.
    pub fn asset_membership(
        &self,
        limits: WorkspaceSceneHierarchyMembershipLimits,
    ) -> Result<WorkspaceSceneHierarchyMembership, WorkspaceSceneHierarchyMembershipError> {
        if self.collections.len() > limits.max_assets {
            return Err(WorkspaceSceneHierarchyMembershipError::AssetLimitExceeded {
                requested: self.collections.len(),
                limit: limits.max_assets,
            });
        }

        let mut collection_ranks = HashMap::new();
        collection_ranks
            .try_reserve(self.collections.len())
            .map_err(
                |_| WorkspaceSceneHierarchyMembershipError::AllocationFailed {
                    resource: "SceneDefinition collection-order index",
                    requested: self.collections.len(),
                },
            )?;
        for (rank, collection) in self.collections.iter().enumerate() {
            if collection_ranks
                .insert(collection.collection_file_index(), rank)
                .is_some()
            {
                return Err(
                    WorkspaceSceneHierarchyMembershipError::DuplicateCollectionFileIndex {
                        file_index: collection.collection_file_index(),
                    },
                );
            }
        }

        let scene_asset_count = self
            .collections
            .iter()
            .try_fold(0usize, |count, collection| {
                count.checked_add(collection.scene_assets().len())
            })
            .ok_or(WorkspaceSceneHierarchyMembershipError::CountOverflow {
                resource: "SceneDefinition scene assets",
            })?;
        if scene_asset_count > limits.max_assets {
            return Err(WorkspaceSceneHierarchyMembershipError::AssetLimitExceeded {
                requested: scene_asset_count,
                limit: limits.max_assets,
            });
        }
        let component_candidate_count = self
            .collections
            .iter()
            .try_fold(0usize, |count, collection| {
                count.checked_add(collection.component_candidates().len())
            })
            .ok_or(WorkspaceSceneHierarchyMembershipError::CountOverflow {
                resource: "SceneDefinition component candidates",
            })?;
        if component_candidate_count > limits.max_assets {
            return Err(WorkspaceSceneHierarchyMembershipError::AssetLimitExceeded {
                requested: component_candidate_count,
                limit: limits.max_assets,
            });
        }

        let mut membership = WorkspaceSceneHierarchyMembership {
            game_objects: Vec::new(),
            components: Vec::new(),
            prefab_instances: Vec::new(),
            managers: Vec::new(),
            scene_roots: None,
            issues: Vec::new(),
        };
        let mut membership_asset_count = 0usize;
        let mut seen_scene_assets = HashMap::new();
        seen_scene_assets
            .try_reserve(scene_asset_count)
            .map_err(
                |_| WorkspaceSceneHierarchyMembershipError::AllocationFailed {
                    resource: "SceneDefinition asset identity index",
                    requested: scene_asset_count,
                },
            )?;
        let mut seen_components = HashMap::new();
        seen_components
            .try_reserve(component_candidate_count)
            .map_err(
                |_| WorkspaceSceneHierarchyMembershipError::AllocationFailed {
                    resource: "SceneDefinition component identity index",
                    requested: component_candidate_count,
                },
            )?;
        let mut weak_components = Vec::new();
        weak_components
            .try_reserve_exact(component_candidate_count)
            .map_err(
                |_| WorkspaceSceneHierarchyMembershipError::AllocationFailed {
                    resource: "ambiguous SceneDefinition component candidates",
                    requested: component_candidate_count,
                },
            )?;
        let mut seen_weak_components = HashMap::new();
        seen_weak_components
            .try_reserve(component_candidate_count)
            .map_err(
                |_| WorkspaceSceneHierarchyMembershipError::AllocationFailed {
                    resource: "ambiguous SceneDefinition component identity index",
                    requested: component_candidate_count,
                },
            )?;
        let mut ordered_components = Vec::new();
        ordered_components
            .try_reserve(component_candidate_count)
            .map_err(
                |_| WorkspaceSceneHierarchyMembershipError::AllocationFailed {
                    resource: "ordered SceneDefinition components",
                    requested: component_candidate_count,
                },
            )?;

        for collection in &self.collections {
            let file_index = collection.collection_file_index();
            if !collection.is_scene_membership_graph_complete() {
                push_membership_issue(
                    &mut membership.issues,
                    WorkspaceSceneHierarchyMembershipIssue::IncompleteCollectionGraph {
                        file_index,
                    },
                    limits.max_issues,
                )?;
            }
            if !collection.source_scene_membership_reference_scans_complete() {
                push_membership_issue(
                    &mut membership.issues,
                    WorkspaceSceneHierarchyMembershipIssue::IncompleteReferenceScans { file_index },
                    limits.max_issues,
                )?;
            }

            for asset in collection.scene_assets() {
                let expected_class_id = match asset.category {
                    WorkspaceSceneAssetCategory::GameObject => Some(GAME_OBJECT_CLASS_ID),
                    WorkspaceSceneAssetCategory::PrefabInstance => Some(PREFAB_INSTANCE_CLASS_ID),
                    WorkspaceSceneAssetCategory::SceneRoots => Some(SCENE_ROOTS_CLASS_ID),
                    WorkspaceSceneAssetCategory::LevelGameManager(kind) => Some(kind.class_id()),
                };
                if asset.identity.file_index != file_index
                    || asset.identity.class_id != expected_class_id
                {
                    push_membership_issue(
                        &mut membership.issues,
                        WorkspaceSceneHierarchyMembershipIssue::InvalidSceneAssetIdentity {
                            identity: asset.identity,
                            collection_file_index: file_index,
                            expected_class_id,
                        },
                        limits.max_issues,
                    )?;
                    continue;
                }
                let asset_key = WorkspaceObjectKey::from(asset.identity);
                if let Some(&existing) = seen_scene_assets.get(&asset_key) {
                    let issue = if existing == asset.identity {
                        WorkspaceSceneHierarchyMembershipIssue::DuplicateSceneAsset {
                            identity: asset.identity,
                        }
                    } else {
                        WorkspaceSceneHierarchyMembershipIssue::ConflictingSceneAssetIdentity {
                            existing,
                            conflicting: asset.identity,
                        }
                    };
                    push_membership_issue(&mut membership.issues, issue, limits.max_issues)?;
                    continue;
                }
                seen_scene_assets.insert(asset_key, asset.identity);
                match asset.category {
                    WorkspaceSceneAssetCategory::GameObject => push_membership_asset(
                        &mut membership_asset_count,
                        &mut membership.game_objects,
                        asset.identity,
                        limits.max_assets,
                    )?,
                    WorkspaceSceneAssetCategory::PrefabInstance => push_membership_asset(
                        &mut membership_asset_count,
                        &mut membership.prefab_instances,
                        asset.identity,
                        limits.max_assets,
                    )?,
                    WorkspaceSceneAssetCategory::LevelGameManager(_) => push_membership_asset(
                        &mut membership_asset_count,
                        &mut membership.managers,
                        asset.identity,
                        limits.max_assets,
                    )?,
                    WorkspaceSceneAssetCategory::SceneRoots => {
                        if membership.scene_roots.is_none() {
                            charge_membership_asset(
                                &mut membership_asset_count,
                                limits.max_assets,
                            )?;
                        }
                        // SceneHierarchyObject.Create overwrites its SceneRoots property for
                        // every matching source asset, so the last one in traversal order wins.
                        membership.scene_roots = Some(asset.identity);
                    }
                }
            }

            for candidate in collection.component_candidates() {
                match candidate.evidence {
                    WorkspaceSceneComponentEvidence::TransformClassId
                    | WorkspaceSceneComponentEvidence::TypeTreeComponentBase
                    | WorkspaceSceneComponentEvidence::GameObjectComponentReference => {
                        let Some(&rank) = collection_ranks.get(&candidate.identity.file_index)
                        else {
                            push_membership_issue(
                                &mut membership.issues,
                                WorkspaceSceneHierarchyMembershipIssue::ComponentOutsideSceneCollections {
                                    identity: candidate.identity,
                                    evidence: candidate.evidence,
                                },
                                limits.max_issues,
                            )?;
                            continue;
                        };
                        let component_key = WorkspaceObjectKey::from(candidate.identity);
                        if let Some(&existing) = seen_components.get(&component_key) {
                            if existing != candidate.identity {
                                push_membership_issue(
                                    &mut membership.issues,
                                    WorkspaceSceneHierarchyMembershipIssue::ConflictingComponentIdentity {
                                        existing,
                                        conflicting: candidate.identity,
                                    },
                                    limits.max_issues,
                                )?;
                            }
                        } else {
                            seen_components.insert(component_key, candidate.identity);
                            ordered_components.try_reserve(1).map_err(|_| {
                                WorkspaceSceneHierarchyMembershipError::AllocationFailed {
                                    resource: "ordered SceneDefinition components",
                                    requested: ordered_components.len().saturating_add(1),
                                }
                            })?;
                            ordered_components.push((rank, candidate.identity));
                        }
                    }
                    WorkspaceSceneComponentEvidence::GameObjectPointerFieldNull
                    | WorkspaceSceneComponentEvidence::GameObjectPointerField => {
                        let weak_key = WorkspaceObjectKey::from(candidate.identity);
                        if let Some(&(existing, existing_evidence)) =
                            seen_weak_components.get(&weak_key)
                        {
                            if existing != candidate.identity {
                                push_membership_issue(
                                    &mut membership.issues,
                                    WorkspaceSceneHierarchyMembershipIssue::ConflictingComponentIdentity {
                                        existing,
                                        conflicting: candidate.identity,
                                    },
                                    limits.max_issues,
                                )?;
                            } else if existing_evidence != candidate.evidence {
                                push_membership_issue(
                                    &mut membership.issues,
                                    WorkspaceSceneHierarchyMembershipIssue::ConflictingComponentGameObjectEvidence {
                                        identity: candidate.identity,
                                        existing: existing_evidence,
                                        conflicting: candidate.evidence,
                                    },
                                    limits.max_issues,
                                )?;
                            }
                        } else {
                            seen_weak_components
                                .insert(weak_key, (candidate.identity, candidate.evidence));
                            weak_components.try_reserve(1).map_err(|_| {
                                WorkspaceSceneHierarchyMembershipError::AllocationFailed {
                                    resource: "ambiguous SceneDefinition component candidates",
                                    requested: weak_components.len().saturating_add(1),
                                }
                            })?;
                            weak_components.push((candidate.identity, candidate.evidence));
                        }
                    }
                    WorkspaceSceneComponentEvidence::AmbiguousTypeTreeBase => {
                        push_membership_issue(
                            &mut membership.issues,
                            WorkspaceSceneHierarchyMembershipIssue::UnclassifiedComponentCandidate {
                                identity: candidate.identity,
                                evidence: candidate.evidence,
                            },
                            limits.max_issues,
                        )?;
                    }
                }
            }
        }

        for (identity, evidence) in weak_components {
            if let Some(&existing) = seen_components.get(&WorkspaceObjectKey::from(identity)) {
                if existing != identity {
                    push_membership_issue(
                        &mut membership.issues,
                        WorkspaceSceneHierarchyMembershipIssue::ConflictingComponentIdentity {
                            existing,
                            conflicting: identity,
                        },
                        limits.max_issues,
                    )?;
                }
                continue;
            }
            if identity.class_id == Some(MONO_BEHAVIOUR_CLASS_ID) {
                if evidence == WorkspaceSceneComponentEvidence::GameObjectPointerFieldNull {
                    continue;
                }
                let Some(&rank) = collection_ranks.get(&identity.file_index) else {
                    push_membership_issue(
                        &mut membership.issues,
                        WorkspaceSceneHierarchyMembershipIssue::ComponentOutsideSceneCollections {
                            identity,
                            evidence,
                        },
                        limits.max_issues,
                    )?;
                    continue;
                };
                seen_components.insert(WorkspaceObjectKey::from(identity), identity);
                ordered_components.try_reserve(1).map_err(|_| {
                    WorkspaceSceneHierarchyMembershipError::AllocationFailed {
                        resource: "ordered SceneDefinition components",
                        requested: ordered_components.len().saturating_add(1),
                    }
                })?;
                ordered_components.push((rank, identity));
                continue;
            }
            push_membership_issue(
                &mut membership.issues,
                WorkspaceSceneHierarchyMembershipIssue::UnclassifiedComponentCandidate {
                    identity,
                    evidence,
                },
                limits.max_issues,
            )?;
        }
        ordered_components.sort_unstable_by_key(|(rank, identity)| (*rank, identity.object_index));
        for (_, identity) in ordered_components {
            if identity.class_id == Some(MONO_BEHAVIOUR_CLASS_ID) {
                match seen_weak_components.get(&WorkspaceObjectKey::from(identity)) {
                    Some(&(
                        weak_identity,
                        WorkspaceSceneComponentEvidence::GameObjectPointerField,
                    )) if weak_identity == identity => {}
                    Some(&(
                        weak_identity,
                        WorkspaceSceneComponentEvidence::GameObjectPointerFieldNull,
                    )) if weak_identity == identity => continue,
                    Some(&(weak_identity, _)) if weak_identity != identity => {
                        push_membership_issue(
                            &mut membership.issues,
                            WorkspaceSceneHierarchyMembershipIssue::ConflictingComponentIdentity {
                                existing: weak_identity,
                                conflicting: identity,
                            },
                            limits.max_issues,
                        )?;
                        continue;
                    }
                    _ => {
                        push_membership_issue(
                            &mut membership.issues,
                            WorkspaceSceneHierarchyMembershipIssue::MonoBehaviourGameObjectReferenceUnavailable {
                                identity,
                            },
                            limits.max_issues,
                        )?;
                        continue;
                    }
                }
            }
            if identity.class_id.is_none() {
                push_membership_issue(
                    &mut membership.issues,
                    WorkspaceSceneHierarchyMembershipIssue::ComponentClassIdUnavailable {
                        identity,
                    },
                    limits.max_issues,
                )?;
            }
            membership_asset_count = membership_asset_count.checked_add(1).ok_or(
                WorkspaceSceneHierarchyMembershipError::CountOverflow {
                    resource: "ordered component membership",
                },
            )?;
            if membership_asset_count > limits.max_assets {
                return Err(WorkspaceSceneHierarchyMembershipError::AssetLimitExceeded {
                    requested: membership_asset_count,
                    limit: limits.max_assets,
                });
            }
            membership.components.try_reserve(1).map_err(|_| {
                WorkspaceSceneHierarchyMembershipError::AllocationFailed {
                    resource: "SceneDefinition component identities",
                    requested: membership_asset_count,
                }
            })?;
            membership.components.push(identity);
        }
        Ok(membership)
    }

    /// Returns the first LightmapSettings object selected by LightingDataProcessor when it has
    /// the versioned LightingDataAsset field and the scene also contains RenderSettings.
    pub fn lighting_data_dependency_owner(&self) -> Option<WorkspaceSceneObjectIdentity> {
        let mut has_render_settings = false;
        let mut first_lightmap_settings = None;
        for collection in &self.collections {
            for asset in collection.scene_assets() {
                match asset.category {
                    WorkspaceSceneAssetCategory::LevelGameManager(
                        SceneManagerKind::RenderSettings,
                    ) => has_render_settings = true,
                    WorkspaceSceneAssetCategory::LevelGameManager(
                        SceneManagerKind::LightmapSettings,
                    ) if first_lightmap_settings.is_none() => {
                        first_lightmap_settings = Some((
                            asset.identity,
                            collection.has_lighting_data_asset_field(asset.identity.object_index),
                        ));
                    }
                    _ => {}
                }
            }
        }
        if has_render_settings {
            first_lightmap_settings
                .filter(|(_, has_field)| *has_field)
                .map(|(identity, _)| identity)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorkspaceSceneHierarchyMembershipLimits {
    pub max_assets: usize,
    pub max_issues: usize,
}

impl Default for WorkspaceSceneHierarchyMembershipLimits {
    fn default() -> Self {
        Self {
            max_assets: 20_000_000,
            max_issues: 100_000,
        }
    }
}
