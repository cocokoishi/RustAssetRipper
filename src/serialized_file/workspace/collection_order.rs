use super::{
    workspace_error, LoadedSerializedFile, SerializedFileWorkspace, SerializedFileWorkspaceError,
    SerializedFileWorkspaceErrorKind, SerializedFileWorkspaceReport, WorkspaceSerializedBundle,
};

pub(super) struct WorkspaceCollectionOrder<'a> {
    files: &'a [LoadedSerializedFile],
    container_bundles: &'a [WorkspaceSerializedBundle],
    report: SerializedFileWorkspaceReport,
}

impl<'a> WorkspaceCollectionOrder<'a> {
    pub(super) fn new(workspace: &'a SerializedFileWorkspace) -> Self {
        Self {
            files: &workspace.files,
            container_bundles: &workspace.container_bundles,
            report: workspace.report,
        }
    }
    /// Returns workspace indexes for direct collections in one mounted container, ordered as
    /// `SerializedBundle.Collections` is ordered by the source baseline.
    pub(super) fn ordered_collection_file_indices(
        &self,
        container_scope: &str,
    ) -> Result<Vec<usize>, Box<SerializedFileWorkspaceError>> {
        let matching_count = self
            .files
            .iter()
            .filter(|file| file.container_scope.as_deref() == Some(container_scope))
            .count();
        let mut ordered_files = Vec::new();
        ordered_files
            .try_reserve_exact(matching_count)
            .map_err(|_| {
                workspace_error(
                    self.report,
                    Some(container_scope),
                    SerializedFileWorkspaceErrorKind::AllocationFailed {
                        resource: "ordered container collection indexes",
                        requested: matching_count,
                    },
                )
            })?;
        for (file_index, file) in self.files.iter().enumerate() {
            if file.container_scope.as_deref() != Some(container_scope) {
                continue;
            }
            let Some(collection_index) = file.container_collection_index else {
                return Err(workspace_error(
                    self.report,
                    Some(&file.virtual_path),
                    SerializedFileWorkspaceErrorKind::MissingContainerCollectionOrder,
                ));
            };
            ordered_files.push((collection_index, file_index, file.virtual_path.as_str()));
        }
        ordered_files.sort_unstable_by_key(|(collection_index, _, _)| *collection_index);
        for (expected_index, (actual_index, _, path)) in ordered_files.iter().enumerate() {
            if *actual_index != expected_index {
                return Err(workspace_error(
                    self.report,
                    Some(path),
                    SerializedFileWorkspaceErrorKind::InvalidContainerCollectionSequence {
                        expected: expected_index,
                        actual: *actual_index,
                    },
                ));
            }
        }
        let mut file_indices = Vec::new();
        file_indices
            .try_reserve_exact(ordered_files.len())
            .map_err(|_| {
                workspace_error(
                    self.report,
                    Some(container_scope),
                    SerializedFileWorkspaceErrorKind::AllocationFailed {
                        resource: "ordered container collection file indexes",
                        requested: ordered_files.len(),
                    },
                )
            })?;
        file_indices.extend(
            ordered_files
                .into_iter()
                .map(|(_, file_index, _)| file_index),
        );
        Ok(file_indices)
    }

    /// Returns all collections in the order used by source `GameBundle.FetchAssetCollections`.
    /// Root standalone collections are emitted first in reverse file-stack order, followed by
    /// root containers in reverse insertion order; each container emits direct collections then
    /// child containers ordered by their containing source entry. These rules are applied to the
    /// deterministic file and bundle insertion order retained by this workspace.
    pub(super) fn source_collection_file_indices(
        &self,
    ) -> Result<Vec<usize>, Box<SerializedFileWorkspaceError>> {
        let bundle_count = self.container_bundles.len();
        let mut children = Vec::<Vec<(usize, usize)>>::new();
        children.try_reserve_exact(bundle_count).map_err(|_| {
            workspace_error(
                self.report,
                None,
                SerializedFileWorkspaceErrorKind::AllocationFailed {
                    resource: "source collection bundle adjacency",
                    requested: bundle_count,
                },
            )
        })?;
        children.resize_with(bundle_count, Vec::new);

        let mut root_bundles = Vec::new();
        root_bundles.try_reserve_exact(bundle_count).map_err(|_| {
            workspace_error(
                self.report,
                None,
                SerializedFileWorkspaceErrorKind::AllocationFailed {
                    resource: "source collection root bundle indexes",
                    requested: bundle_count,
                },
            )
        })?;
        for (bundle_index, bundle) in self.container_bundles.iter().enumerate() {
            match (bundle.parent_bundle_index, bundle.parent_entry_index) {
                (None, None) => root_bundles.push(bundle_index),
                (Some(parent), Some(entry_index)) if parent < bundle_index => {
                    let Some(parent_children) = children.get_mut(parent) else {
                        return Err(workspace_error(
                            self.report,
                            bundle.scope_path.as_deref(),
                            SerializedFileWorkspaceErrorKind::InvalidMountedContainerHierarchy {
                                bundle_index,
                            },
                        ));
                    };
                    parent_children.try_reserve(1).map_err(|_| {
                        workspace_error(
                            self.report,
                            bundle.scope_path.as_deref(),
                            SerializedFileWorkspaceErrorKind::AllocationFailed {
                                resource: "source collection child bundle indexes",
                                requested: parent_children.len().saturating_add(1),
                            },
                        )
                    })?;
                    parent_children.push((entry_index, bundle_index));
                }
                _ => {
                    return Err(workspace_error(
                        self.report,
                        bundle.scope_path.as_deref(),
                        SerializedFileWorkspaceErrorKind::InvalidMountedContainerHierarchy {
                            bundle_index,
                        },
                    ));
                }
            }
        }
        for (parent_index, parent_children) in children.iter_mut().enumerate() {
            parent_children.sort_unstable_by_key(|(entry_index, _)| *entry_index);
            if parent_children
                .windows(2)
                .any(|pair| pair[0].0 == pair[1].0)
            {
                return Err(workspace_error(
                    self.report,
                    self.container_bundles[parent_index].scope_path.as_deref(),
                    SerializedFileWorkspaceErrorKind::InvalidMountedContainerHierarchy {
                        bundle_index: parent_index,
                    },
                ));
            }
        }

        let mut ordered_files = Vec::new();
        ordered_files
            .try_reserve_exact(self.files.len())
            .map_err(|_| {
                workspace_error(
                    self.report,
                    None,
                    SerializedFileWorkspaceErrorKind::AllocationFailed {
                        resource: "source collection file indexes",
                        requested: self.files.len(),
                    },
                )
            })?;
        let mut seen_files = Vec::new();
        seen_files
            .try_reserve_exact(self.files.len())
            .map_err(|_| {
                workspace_error(
                    self.report,
                    None,
                    SerializedFileWorkspaceErrorKind::AllocationFailed {
                        resource: "source collection file visitation state",
                        requested: self.files.len(),
                    },
                )
            })?;
        seen_files.resize(self.files.len(), false);

        // GameBundle pops its loaded file stack from the end and stores standalone collections
        // separately from root bundles, so all standalone collections are fetched first.
        for (file_index, file) in self.files.iter().enumerate().rev() {
            if file.container_bundle_index.is_some() {
                continue;
            }
            if file.container_scope.is_some() || file.container_collection_index.is_some() {
                return Err(workspace_error(
                    self.report,
                    Some(&file.virtual_path),
                    SerializedFileWorkspaceErrorKind::InvalidWorkspaceCollectionTraversal {
                        file_index,
                    },
                ));
            }
            seen_files[file_index] = true;
            ordered_files.push(file_index);
        }

        // Bundle insertion follows the same reversed file-stack walk. Push roots in insertion
        // order so the stack visits the final inserted root first, as FetchAssetCollections does.
        let mut stack = Vec::new();
        stack.try_reserve_exact(bundle_count).map_err(|_| {
            workspace_error(
                self.report,
                None,
                SerializedFileWorkspaceErrorKind::AllocationFailed {
                    resource: "source collection bundle traversal stack",
                    requested: bundle_count,
                },
            )
        })?;
        stack.extend(root_bundles);
        let mut seen_bundles = Vec::new();
        seen_bundles.try_reserve_exact(bundle_count).map_err(|_| {
            workspace_error(
                self.report,
                None,
                SerializedFileWorkspaceErrorKind::AllocationFailed {
                    resource: "source collection bundle visitation state",
                    requested: bundle_count,
                },
            )
        })?;
        seen_bundles.resize(bundle_count, false);

        while let Some(bundle_index) = stack.pop() {
            let Some(bundle) = self.container_bundles.get(bundle_index) else {
                return Err(workspace_error(
                    self.report,
                    None,
                    SerializedFileWorkspaceErrorKind::InvalidMountedContainerHierarchy {
                        bundle_index,
                    },
                ));
            };
            if seen_bundles[bundle_index] {
                return Err(workspace_error(
                    self.report,
                    bundle.scope_path.as_deref(),
                    SerializedFileWorkspaceErrorKind::InvalidMountedContainerHierarchy {
                        bundle_index,
                    },
                ));
            }
            seen_bundles[bundle_index] = true;
            for (collection_index, &file_index) in
                bundle.direct_collection_file_indices.iter().enumerate()
            {
                let Some(file) = self.files.get(file_index) else {
                    return Err(workspace_error(
                        self.report,
                        bundle.scope_path.as_deref(),
                        SerializedFileWorkspaceErrorKind::InvalidWorkspaceCollectionTraversal {
                            file_index,
                        },
                    ));
                };
                if file.container_bundle_index != Some(bundle_index)
                    || file.container_collection_index != Some(collection_index)
                    || file.container_scope.as_deref() != bundle.scope_path.as_deref()
                    || seen_files[file_index]
                {
                    return Err(workspace_error(
                        self.report,
                        Some(&file.virtual_path),
                        SerializedFileWorkspaceErrorKind::InvalidWorkspaceCollectionTraversal {
                            file_index,
                        },
                    ));
                }
                seen_files[file_index] = true;
                ordered_files.push(file_index);
            }

            let bundle_children = &children[bundle_index];
            stack.try_reserve(bundle_children.len()).map_err(|_| {
                workspace_error(
                    self.report,
                    bundle.scope_path.as_deref(),
                    SerializedFileWorkspaceErrorKind::AllocationFailed {
                        resource: "source collection bundle traversal stack",
                        requested: stack.len().saturating_add(bundle_children.len()),
                    },
                )
            })?;
            for &(_, child_index) in bundle_children.iter().rev() {
                stack.push(child_index);
            }
        }

        if seen_bundles.iter().any(|seen| !seen)
            || seen_files.iter().any(|seen| !seen)
            || ordered_files.len() != self.files.len()
        {
            let bundle_index = seen_bundles.iter().position(|seen| !seen).unwrap_or(0);
            return Err(workspace_error(
                self.report,
                self.container_bundles
                    .get(bundle_index)
                    .and_then(|bundle| bundle.scope_path.as_deref()),
                SerializedFileWorkspaceErrorKind::IncompleteWorkspaceCollectionTraversal,
            ));
        }
        Ok(ordered_files)
    }
}
