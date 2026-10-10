use super::{
    SerializedFileWorkspace, WorkspaceResolvedResourceFile, WorkspaceResourceFile,
    WorkspaceResourceLookupError, WorkspaceRootResourceFile, WorkspaceSerializedBundle,
};
use crate::io::special_names::fix_file_identifier;

pub(super) struct WorkspaceResourceLookup<'a> {
    container_bundles: &'a [WorkspaceSerializedBundle],
    resource_files: &'a [WorkspaceResourceFile],
}

impl<'a> WorkspaceResourceLookup<'a> {
    pub(super) fn new(workspace: &'a SerializedFileWorkspace) -> Self {
        Self {
            container_bundles: &workspace.container_bundles,
            resource_files: &workspace.resource_files,
        }
    }
    /// Finds a ResourceFile in the source bundle hierarchy using `Bundle.ResolveResource`
    /// precedence. `source_bundle_index == None` represents the root GameBundle. This searches
    /// mounted bundle resources only; a GameBundle external provider is a separate step.
    pub(super) fn resolve_bundled_resource_file(
        &self,
        source_bundle_index: Option<usize>,
        requested_name: &str,
    ) -> Result<Option<usize>, WorkspaceResourceLookupError> {
        match self.resolve_bundled_or_root_resource_file(
            source_bundle_index,
            requested_name,
            &[],
        )? {
            Some(WorkspaceResolvedResourceFile::MountedBundle { resource_index }) => {
                Ok(Some(resource_index))
            }
            Some(WorkspaceResolvedResourceFile::ExplicitRoot { resource_index }) => Err(
                WorkspaceResourceLookupError::RootResourceFileIndexOutOfBounds {
                    index: resource_index,
                    count: 0,
                },
            ),
            None => Ok(None),
        }
    }

    /// Resolves mounted bundle resources together with direct GameBundle ResourceFile inputs.
    /// Explicit root resources are checked at the GameBundle scope before its child Bundles,
    /// matching `Bundle.ResolveResource`'s current-scope resource-before-child order.
    pub(super) fn resolve_bundled_or_root_resource_file(
        &self,
        source_bundle_index: Option<usize>,
        requested_name: &str,
        root_resources: &[WorkspaceRootResourceFile],
    ) -> Result<Option<WorkspaceResolvedResourceFile>, WorkspaceResourceLookupError> {
        if requested_name.is_empty() {
            return Ok(None);
        }
        if let Some(index) = source_bundle_index {
            if index >= self.container_bundles.len() {
                return Err(WorkspaceResourceLookupError::BundleIndexOutOfBounds {
                    index,
                    count: self.container_bundles.len(),
                });
            }
        }

        let fixed_name = fix_file_identifier(requested_name);
        let mut current_bundle_index = source_bundle_index;
        let mut excluded_child_bundle = None;
        for _ in 0..=bundle_walk_step_limit(self.container_bundles.len()) {
            if let Some(bundle_index) = current_bundle_index {
                if let Some(resource_index) =
                    self.find_resource_in_bundle(bundle_index, &fixed_name)?
                {
                    return Ok(Some(WorkspaceResolvedResourceFile::MountedBundle {
                        resource_index,
                    }));
                }
                for (child_index, child) in self.container_bundles.iter().enumerate() {
                    if child.parent_bundle_index != Some(bundle_index)
                        || Some(child_index) == excluded_child_bundle
                    {
                        continue;
                    }
                    if let Some(resource_index) =
                        self.find_resource_in_bundle(child_index, &fixed_name)?
                    {
                        return Ok(Some(WorkspaceResolvedResourceFile::MountedBundle {
                            resource_index,
                        }));
                    }
                }
            } else {
                if let Some((resource_index, _)) = root_resources
                    .iter()
                    .enumerate()
                    .find(|(_, resource)| fix_file_identifier(&resource.name) == fixed_name)
                {
                    return Ok(Some(WorkspaceResolvedResourceFile::ExplicitRoot {
                        resource_index,
                    }));
                }
                for (child_index, child) in self.container_bundles.iter().enumerate() {
                    if child.parent_bundle_index.is_some()
                        || Some(child_index) == excluded_child_bundle
                    {
                        continue;
                    }
                    if let Some(resource_index) =
                        self.find_resource_in_bundle(child_index, &fixed_name)?
                    {
                        return Ok(Some(WorkspaceResolvedResourceFile::MountedBundle {
                            resource_index,
                        }));
                    }
                }
                return Ok(None);
            }

            let Some(bundle_index) = current_bundle_index else {
                return Ok(None);
            };
            let bundle = self.container_bundles.get(bundle_index).ok_or(
                WorkspaceResourceLookupError::BundleIndexOutOfBounds {
                    index: bundle_index,
                    count: self.container_bundles.len(),
                },
            )?;
            if let Some(parent_index) = bundle.parent_bundle_index {
                if parent_index >= self.container_bundles.len() {
                    return Err(WorkspaceResourceLookupError::ParentBundleIndexOutOfBounds {
                        index: parent_index,
                        count: self.container_bundles.len(),
                    });
                }
            }
            excluded_child_bundle = Some(bundle_index);
            current_bundle_index = bundle.parent_bundle_index;
        }
        Err(WorkspaceResourceLookupError::BundleParentCycle)
    }

    fn find_resource_in_bundle(
        &self,
        bundle_index: usize,
        fixed_name: &str,
    ) -> Result<Option<usize>, WorkspaceResourceLookupError> {
        let bundle = self.container_bundles.get(bundle_index).ok_or(
            WorkspaceResourceLookupError::BundleIndexOutOfBounds {
                index: bundle_index,
                count: self.container_bundles.len(),
            },
        )?;
        for &resource_index in &bundle.direct_resource_file_indices {
            let resource = self.resource_files.get(resource_index).ok_or(
                WorkspaceResourceLookupError::ResourceFileIndexOutOfBounds {
                    index: resource_index,
                    count: self.resource_files.len(),
                },
            )?;
            if resource.name_fixed.as_str() == fixed_name {
                return Ok(Some(resource_index));
            }
        }
        Ok(None)
    }
}

pub(super) fn bundle_walk_step_limit(bundle_count: usize) -> usize {
    bundle_count.saturating_add(1)
}
