//! A read-only bundle-tree snapshot behind the loaded game.
//!
//! The frozen product browses the loaded `GameBundle` directly: every `/X/View` page walks
//! `Bundle.Bundles`/`Collections`/`Resources`/`FailedFiles` and links siblings through the
//! `Paths/` values. The port keeps no live asset graph behind the web surface (the load
//! pipeline decodes straight into the export workspaces), so the state builds this owned
//! snapshot once per load from the `SerializedFileWorkspace` inventory plus the explicit
//! root resource catalog and renders the tree pages from it.
//!
//! The snapshot mirrors the frozen `GameBundle.FromPaths` shape: index 0 is the root bundle
//! (named `GameBundle`, like the frozen root), directly loaded SerializedFiles become root
//! collections in workspace order, containers become child bundles in mount order carrying
//! the fixed container name, mounted resources and failed entries attach to their owning
//! bundle, and explicitly passed root resources list on the root in catalog order (the
//! catalog already keeps the frozen file-stack pop order). The `Paths/` index trails
//! address positions in these source-order lists; the pages sort copies for display exactly
//! like the frozen `ViewPage` classes do, so the URLs never depend on display order.
//!
//! Names follow the frozen `Name`/`NameFixed` rules: collections use the identifier-fixed
//! file name (like `SerializedAssetCollection.Name = file.NameFixed`), bundles use the
//! fixed container name, resources use the cached fixed name (like `FileBase.NameFixed`),
//! and failed entries use the source entry name (the workspace retains no fixed form for
//! those). A container bundle without a fixed name falls back to its scope's file name,
//! then to `Bundle`; the frozen side always has a container name, so the fallback only
//! covers data the workspace did not retain a name for.
//!
//! Two deliberate boundaries are recorded here, not hidden:
//!
//! * The snapshot carries names and counts only. Asset identities below the collection
//!   level are the files' object path IDs; anything needing decoded asset fields (asset
//!   names, scene graphs, resource bytes, failure stack traces) stays on the explicit
//!   pending pages until its own port lands.
//! * A workspace index that points outside its list is a projection error: the tree
//!   reports it instead of dropping the entry silently.

use assetripper::export::project_exporter::object_class_name;
use assetripper::export::project_workflow::{object_asset_name, text_asset_script_bytes};
use assetripper::io::special_names::fix_file_identifier;
use assetripper::serialized_file::object_workspace::ObjectWorkspace;
use assetripper::serialized_file::reference_graph::{
    MultiFilePPtrResolution, MultiFileReferenceGraphLimits,
};
use assetripper::serialized_file::workspace::{SerializedFileWorkspace, WorkspaceRootResourceFile};
use assetripper::web_paths::{AssetPath, BundlePath, CollectionPath, FailedFilePath, ResourcePath};

/// Failure to project the workspace inventory into a bundle tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeError {
    message: String,
}

impl TreeError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for TreeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for TreeError {}

/// The name of the snapshot root, matching the frozen `GameBundle.Name`.
pub const ROOT_BUNDLE_NAME: &str = "GameBundle";

/// The Unity class ID of `TextAsset`, the only class whose text the port serves.
const TEXT_ASSET_CLASS_ID: i32 = 49;

/// The display name when a container bundle carries neither a fixed name nor a scope.
pub const NAMELESS_BUNDLE_NAME: &str = "Bundle";

/// One collection in the snapshot: a workspace SerializedFile with its asset rows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeCollection {
    /// The identifier-fixed file name, mirroring `AssetCollection.Name`.
    pub name: String,
    /// The number of objects in the file, mirroring `AssetCollection.Count`.
    pub asset_count: usize,
    /// One row per object in file order, mirroring the collection's asset list.
    pub assets: Vec<TreeAsset>,
}

/// One asset row: the identity the collection page's table shows.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeAsset {
    /// The object path ID, mirroring `IUnityObjectBase.PathID`.
    pub path_id: i64,
    /// The numeric class ID, mirroring `IUnityObjectBase.ClassID`.
    pub class_id: i32,
    /// The source `ClassName` for the object.
    pub class_name: String,
    /// The display name, mirroring `IUnityObjectBase.GetBestName`.
    pub best_name: String,
    /// The workspace file index holding the object.
    pub file_index: usize,
    /// The object index within the file's object table.
    pub object_index: usize,
    /// Whether the asset serves text, mirroring `AssetAPI.HasText` for the ported
    /// TextAsset case (non-empty script bytes).
    pub has_text: bool,
    /// The non-null reference edges in field traversal order.
    pub dependencies: Vec<TreeDependency>,
}

/// One reference edge: the field path with its resolved target.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeDependency {
    /// The field path within the decoded object value.
    pub field_path: String,
    /// The linked asset, or `Missing` when the target has no tree address.
    pub target: TreeDependencyTarget,
}

/// Where a dependency edge points.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TreeDependencyTarget {
    /// A snapshot asset address: bundle, collection position, asset row.
    Asset {
        bundle: usize,
        position: usize,
        row: usize,
    },
    /// The target resolves to nothing the tree can link, mirroring the frozen
    /// `Missing` text (this covers dangling, ambiguous and engine-asset targets,
    /// none of which the frozen page links either).
    Missing,
}

/// One bundle in the snapshot. Lists keep source order; pages sort copies for display.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeBundle {
    /// The bundle display name.
    pub name: String,
    /// The parent bundle index, or `None` for the snapshot root.
    pub parent: Option<usize>,
    /// Child bundle indexes in mount order.
    pub children: Vec<usize>,
    /// Direct collections in workspace order.
    pub collections: Vec<TreeCollection>,
    /// Direct resources in workspace order.
    pub resources: Vec<TreeResource>,
    /// Direct failed-entry names in workspace order.
    pub failed: Vec<String>,
}

/// One resource in the snapshot: a mounted bundle resource or an explicit root input.
///
/// The name mirrors `ResourceFile.Name` for display; the virtual path locates the bytes
/// in the source virtual filesystem, mirroring `ResourceFile.Stream`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TreeResource {
    /// The source entry name.
    pub name: String,
    /// The virtual path of the bytes in the source filesystem.
    pub virtual_path: String,
}

/// The whole projected tree. Index 0 is always the root bundle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BundleTree {
    bundles: Vec<TreeBundle>,
}

impl BundleTree {
    /// Project the workspace inventory plus the explicit root resources.
    ///
    /// Asset rows read the decoded object workspace for class and display names; the
    /// export flow's own helpers decide both, so the pages list what the export names.
    pub fn from_loaded(
        workspace: &SerializedFileWorkspace,
        objects: &ObjectWorkspace,
        root_resources: &[WorkspaceRootResourceFile],
    ) -> Result<Self, TreeError> {
        let files = workspace.files();
        let bundles = workspace.container_bundles();
        let resources = workspace.resource_files();
        let failed = workspace.failed_container_entries();

        let mut tree_bundles: Vec<TreeBundle> = Vec::with_capacity(bundles.len() + 1);
        tree_bundles.push(TreeBundle {
            name: ROOT_BUNDLE_NAME.to_owned(),
            parent: None,
            children: Vec::new(),
            collections: Vec::new(),
            resources: root_resources
                .iter()
                .map(|resource| TreeResource {
                    name: resource.name.clone(),
                    virtual_path: resource.virtual_path.clone(),
                })
                .collect(),
            failed: Vec::new(),
        });
        for bundle in bundles {
            let name = bundle
                .name_fixed
                .clone()
                .unwrap_or_else(|| scope_file_name(bundle.scope_path.as_deref()));
            tree_bundles.push(TreeBundle {
                name,
                parent: None,
                children: Vec::new(),
                collections: Vec::new(),
                resources: Vec::new(),
                failed: Vec::new(),
            });
        }

        // Parent links first, so a later out-of-range parent fails before any entry moves.
        for (workspace_index, bundle) in bundles.iter().enumerate() {
            let snapshot_index = workspace_index + 1;
            match bundle.parent_bundle_index {
                None => {
                    tree_bundles[0].children.push(snapshot_index);
                    tree_bundles[snapshot_index].parent = Some(0);
                }
                Some(parent) => {
                    if parent == workspace_index {
                        return Err(TreeError::new(format!(
                            "bundle {workspace_index} names itself as its own parent"
                        )));
                    }
                    let parent_snapshot = parent.checked_add(1).ok_or_else(|| {
                        TreeError::new(format!(
                            "bundle {workspace_index} has a parent index that overflows"
                        ))
                    })?;
                    if parent_snapshot >= tree_bundles.len() {
                        return Err(TreeError::new(format!(
                            "bundle {workspace_index} names bundle {parent}, which does not exist"
                        )));
                    }
                    tree_bundles[parent_snapshot].children.push(snapshot_index);
                    tree_bundles[snapshot_index].parent = Some(parent_snapshot);
                }
            }
        }

        // Collections follow each bundle's direct list, which is the source of order;
        // every container file must be claimed by exactly one of them.
        let mut claimed = vec![false; files.len()];
        // Every file's collection address, for dependency targets below.
        let mut file_address: Vec<Option<(usize, usize)>> = vec![None; files.len()];
        for (workspace_index, bundle) in bundles.iter().enumerate() {
            let snapshot_index = workspace_index + 1;
            for &file_index in &bundle.direct_collection_file_indices {
                let file = files.get(file_index).ok_or_else(|| {
                    TreeError::new(format!(
                        "bundle {workspace_index} names file {file_index}, which does not exist"
                    ))
                })?;
                if file.container_bundle_index != Some(workspace_index) {
                    return Err(TreeError::new(format!(
                        "file {file_index} does not belong to bundle {workspace_index}"
                    )));
                }
                if claimed[file_index] {
                    return Err(TreeError::new(format!(
                        "file {file_index} is claimed by more than one bundle"
                    )));
                }
                claimed[file_index] = true;
                tree_bundles[snapshot_index]
                    .collections
                    .push(collection_from_file(file_index, file, objects));
                file_address[file_index] = Some((
                    snapshot_index,
                    tree_bundles[snapshot_index].collections.len() - 1,
                ));
            }
        }
        for (file_index, file) in files.iter().enumerate() {
            match file.container_bundle_index {
                None => {
                    tree_bundles[0]
                        .collections
                        .push(collection_from_file(file_index, file, objects));
                    file_address[file_index] = Some((0, tree_bundles[0].collections.len() - 1));
                }
                Some(bundle_index) => {
                    if bundle_index >= bundles.len() {
                        return Err(TreeError::new(format!(
                            "file {file_index} names bundle {bundle_index}, which does not exist"
                        )));
                    }
                    if !claimed[file_index] {
                        return Err(TreeError::new(format!(
                            "file {file_index} names bundle {bundle_index}, which does not list it"
                        )));
                    }
                }
            }
        }

        resolve_dependency_edges(workspace, &file_address, &mut tree_bundles)?;

        // Mounted resources follow the same direct-list discipline.
        let mut resource_claimed = vec![false; resources.len()];
        for (workspace_index, bundle) in bundles.iter().enumerate() {
            let snapshot_index = workspace_index + 1;
            for &resource_index in &bundle.direct_resource_file_indices {
                let resource = resources.get(resource_index).ok_or_else(|| {
                    TreeError::new(format!(
                        "bundle {workspace_index} names resource {resource_index}, which does not exist"
                    ))
                })?;
                if resource.container_bundle_index != workspace_index {
                    return Err(TreeError::new(format!(
                        "resource {resource_index} does not belong to bundle {workspace_index}"
                    )));
                }
                if resource_claimed[resource_index] {
                    return Err(TreeError::new(format!(
                        "resource {resource_index} is claimed by more than one bundle"
                    )));
                }
                resource_claimed[resource_index] = true;
                tree_bundles[snapshot_index].resources.push(TreeResource {
                    name: resource.name.clone(),
                    virtual_path: resource.virtual_path.clone(),
                });
            }
        }
        for (resource_index, resource) in resources.iter().enumerate() {
            if !resource_claimed[resource_index] {
                return Err(TreeError::new(format!(
                    "resource {resource_index} in bundle {} is listed by no bundle",
                    resource.container_bundle_index
                )));
            }
        }

        // Failed entries attach through their owning container, if any.
        for entry in failed {
            match entry.entry.container_bundle_index {
                None => tree_bundles[0].failed.push(entry.entry.source_name.clone()),
                Some(bundle_index) => {
                    let snapshot_index = bundle_index.checked_add(1).ok_or_else(|| {
                        TreeError::new(
                            "a failed entry has a bundle index that overflows".to_owned(),
                        )
                    })?;
                    if snapshot_index >= tree_bundles.len() {
                        return Err(TreeError::new(format!(
                            "a failed entry names bundle {bundle_index}, which does not exist"
                        )));
                    }
                    tree_bundles[snapshot_index]
                        .failed
                        .push(entry.entry.source_name.clone());
                }
            }
        }

        Ok(Self {
            bundles: tree_bundles,
        })
    }

    /// The bundle at `index`, or `None` when out of range.
    pub fn bundle(&self, index: usize) -> Option<&TreeBundle> {
        self.bundles.get(index)
    }

    /// Walk one index trail from the root, mirroring `TryGetBundle`.
    pub fn resolve_bundle(&self, path: &BundlePath) -> Option<usize> {
        let mut index = 0;
        for &child in path.indices() {
            let position = usize::try_from(child).ok()?;
            index = *self.bundles.get(index)?.children.get(position)?;
        }
        Some(index)
    }

    /// Resolve a collection to its bundle and position, mirroring `TryGetCollection`.
    pub fn resolve_collection(&self, path: &CollectionPath) -> Option<(usize, usize)> {
        let bundle = self.resolve_bundle(&path.bundle)?;
        let position = usize::try_from(path.index).ok()?;
        self.bundles
            .get(bundle)?
            .collections
            .get(position)
            .map(|_| (bundle, position))
    }

    /// Resolve a resource to its bundle and position, mirroring `TryGetResource`.
    pub fn resolve_resource(&self, path: &ResourcePath) -> Option<(usize, usize)> {
        let bundle = self.resolve_bundle(&path.bundle)?;
        let position = usize::try_from(path.index).ok()?;
        self.bundles
            .get(bundle)?
            .resources
            .get(position)
            .map(|_| (bundle, position))
    }

    /// Resolve a failed file to its bundle and position, mirroring `TryGetFailedFile`.
    pub fn resolve_failed(&self, path: &FailedFilePath) -> Option<(usize, usize)> {
        let bundle = self.resolve_bundle(&path.bundle)?;
        let position = usize::try_from(path.index).ok()?;
        self.bundles
            .get(bundle)?
            .failed
            .get(position)
            .map(|_| (bundle, position))
    }

    /// Resolve an asset to its bundle, collection and row, mirroring `TryGetAsset`.
    pub fn resolve_asset(&self, path: &AssetPath) -> Option<(usize, usize, usize)> {
        let (bundle, position) = self.resolve_collection(&path.collection)?;
        let row = self
            .bundles
            .get(bundle)?
            .collections
            .get(position)?
            .assets
            .iter()
            .position(|asset| asset.path_id == path.path_id)?;
        Some((bundle, position, row))
    }

    /// The `BundlePath` of the bundle at `index`, or `None` when out of range.
    pub fn bundle_path(&self, index: usize) -> Option<BundlePath> {
        let mut steps = Vec::new();
        let mut current = index;
        loop {
            let bundle = self.bundles.get(current)?;
            match bundle.parent {
                None => {
                    if current != 0 {
                        return None;
                    }
                    break;
                }
                Some(parent) => {
                    let position =
                        self.bundles
                            .get(parent)?
                            .children
                            .iter()
                            .position(|&child| child == current)? as i32;
                    steps.push(position);
                    current = parent;
                }
            }
        }
        steps.reverse();
        let mut path = BundlePath::root();
        for step in steps {
            path = path.child(step);
        }
        Some(path)
    }

    /// Search every asset by display and class name, mirroring the frozen `Match`.
    ///
    /// Both fields match case-insensitively by containment, and results keep collection
    /// order: the frozen search never sorts them. Case folding uses Unicode lowercase,
    /// which agrees with the frozen ordinal-ignore-case comparison on ASCII text.
    pub fn search(&self, query: &str) -> Vec<SearchMatch> {
        let folded = query.to_lowercase();
        let mut matches = Vec::new();
        for (bundle_index, bundle) in self.bundles.iter().enumerate() {
            let Some(bundle_path) = self.bundle_path(bundle_index) else {
                continue;
            };
            for (position, collection) in bundle.collections.iter().enumerate() {
                let collection_path = CollectionPath {
                    bundle: bundle_path.clone(),
                    index: position as i32,
                };
                for asset in &collection.assets {
                    if asset.best_name.to_lowercase().contains(&folded)
                        || asset.class_name.to_lowercase().contains(&folded)
                    {
                        matches.push(SearchMatch {
                            collection_path: collection_path.clone(),
                            collection_name: collection.name.clone(),
                            asset: asset.clone(),
                        });
                    }
                }
            }
        }
        matches
    }
}

/// One search hit: the asset row with its collection address.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchMatch {
    /// The address of the collection holding the asset.
    pub collection_path: CollectionPath,
    /// The collection display name.
    pub collection_name: String,
    /// The matching asset row.
    pub asset: TreeAsset,
}

/// Build one [`TreeCollection`] from a workspace file.
fn collection_from_file(
    file_index: usize,
    file: &assetripper::serialized_file::workspace::LoadedSerializedFile,
    objects: &ObjectWorkspace,
) -> TreeCollection {
    let assets = file
        .metadata
        .objects
        .iter()
        .enumerate()
        .map(|(object_index, info)| asset_row(file_index, object_index, info, objects))
        .collect::<Vec<_>>();
    TreeCollection {
        name: fix_file_identifier(&file.file_name),
        asset_count: assets.len(),
        assets,
    }
}

/// Build one asset row: the export flow's class and display names for the object.
///
/// A decoded payload names the object through its `m_Name`/`Name` field with the class
/// name as the fallback, exactly like the export identity; an undecodable object falls
/// back to the factory result, and an object absent from the object workspace falls back
/// to the file table's own class fields.
fn asset_row(
    file_index: usize,
    object_index: usize,
    info: &assetripper::serialized_file::metadata::ObjectInfo,
    objects: &ObjectWorkspace,
) -> TreeAsset {
    let path_id = info.path_id;
    let Some(object) = objects.get(file_index, object_index) else {
        let class_id = info.class_id.map(i32::from).unwrap_or(info.raw_type_id);
        let class_name = object_class_name(class_id, None);
        return TreeAsset {
            path_id,
            class_id,
            class_name: class_name.clone(),
            best_name: class_name,
            file_index,
            object_index,
            has_text: false,
            dependencies: Vec::new(),
        };
    };
    let class_id = object.factory.class_id;
    let payload = object.payload.as_ref().ok();
    let class_name = object_class_name(class_id, payload.map(|value| value.type_name.as_str()));
    let best_name = payload.and_then(object_asset_name).unwrap_or_else(|| {
        if object.factory.name.is_empty() {
            class_name.clone()
        } else {
            object.factory.name.clone()
        }
    });
    // Only TextAsset scripts are served: the frozen `HasText` also covers shaders and
    // MonoScripts, whose text ports are still open.
    let has_text = class_id == TEXT_ASSET_CLASS_ID
        && payload
            .map(|value| {
                text_asset_script_bytes(value)
                    .ok()
                    .flatten()
                    .is_some_and(|script| !script.is_empty())
            })
            .unwrap_or(false);
    TreeAsset {
        path_id,
        class_id,
        class_name,
        best_name,
        file_index,
        object_index,
        has_text,
        dependencies: Vec::new(),
    }
}

/// Resolve every recorded reference edge through the workspace's own joined
/// reference graph, so cross-file targets link exactly like the export's own
/// resolution does (source bundle-collection lookup order, first match wins).
///
/// Null edges are skipped: the frozen page lists no row for them and the section shows
/// only when a non-null edge exists. Every other resolution that names no snapshot
/// asset (dangling, ambiguous, invalid and predefined-engine targets) becomes `Missing`,
/// matching the frozen page, which links only targets its own lookup resolves.
fn resolve_dependency_edges(
    workspace: &SerializedFileWorkspace,
    file_address: &[Option<(usize, usize)>],
    tree_bundles: &mut [TreeBundle],
) -> Result<(), TreeError> {
    let joined = workspace
        .reference_graph(MultiFileReferenceGraphLimits::default())
        .map_err(|error| TreeError::new(format!("reference join failed: {error}")))?;
    for reference in joined.references() {
        if matches!(reference.resolution, MultiFilePPtrResolution::Null) {
            continue;
        }
        let target = match reference.resolution {
            MultiFilePPtrResolution::Object {
                target_file_index,
                target_object_index,
            } => file_address
                .get(target_file_index)
                .copied()
                .flatten()
                .and_then(|(bundle, position)| {
                    tree_bundles
                        .get(bundle)?
                        .collections
                        .get(position)
                        .filter(|collection| collection.assets.get(target_object_index).is_some())
                        .map(|_| TreeDependencyTarget::Asset {
                            bundle,
                            position,
                            row: target_object_index,
                        })
                })
                .unwrap_or(TreeDependencyTarget::Missing),
            _ => TreeDependencyTarget::Missing,
        };
        let dependency = TreeDependency {
            field_path: reference.field_path.to_owned(),
            target,
        };
        let row = file_address
            .get(reference.source_file_index)
            .copied()
            .flatten()
            .and_then(|(bundle, position)| {
                tree_bundles
                    .get_mut(bundle)?
                    .collections
                    .get_mut(position)?
                    .assets
                    .get_mut(reference.source_object_index)
            });
        match row {
            Some(row) => row.dependencies.push(dependency),
            None => {
                return Err(TreeError::new(format!(
                    "reference source file {} object {} has no snapshot row",
                    reference.source_file_index, reference.source_object_index
                )));
            }
        }
    }
    Ok(())
}

/// The display name for a container bundle without a fixed name.
fn scope_file_name(scope_path: Option<&str>) -> String {
    scope_path
        .and_then(|scope| scope.rsplit('/').next())
        .filter(|name| !name.is_empty())
        .unwrap_or(NAMELESS_BUNDLE_NAME)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_workspace_projects_to_a_lonely_root() {
        let workspace = SerializedFileWorkspace::default();
        let objects = ObjectWorkspace::default();
        let tree =
            BundleTree::from_loaded(&workspace, &objects, &[]).expect("empty workspace projects");
        let root = tree.bundle(0).expect("the root always exists");
        assert_eq!(root.name, ROOT_BUNDLE_NAME);
        assert_eq!(root.parent, None);
        assert!(tree.resolve_bundle(&BundlePath::root().child(0)).is_none());
    }

    #[test]
    fn bundle_path_resolution_rejects_negative_and_out_of_range_steps() {
        let workspace = SerializedFileWorkspace::default();
        let objects = ObjectWorkspace::default();
        let tree =
            BundleTree::from_loaded(&workspace, &objects, &[]).expect("empty workspace projects");
        assert_eq!(tree.resolve_bundle(&BundlePath::root()), Some(0));
        assert!(tree.resolve_bundle(&BundlePath::root().child(-1)).is_none());
        assert!(tree.resolve_bundle(&BundlePath::root().child(4)).is_none());
    }

    #[test]
    fn bundle_paths_round_trip_through_parent_links() {
        let workspace = SerializedFileWorkspace::default();
        let objects = ObjectWorkspace::default();
        let tree =
            BundleTree::from_loaded(&workspace, &objects, &[]).expect("empty workspace projects");
        assert_eq!(tree.bundle_path(0), Some(BundlePath::root()));
        assert_eq!(tree.bundle_path(1), None);
    }

    #[test]
    fn an_empty_tree_has_no_search_hits() {
        let workspace = SerializedFileWorkspace::default();
        let objects = ObjectWorkspace::default();
        let tree =
            BundleTree::from_loaded(&workspace, &objects, &[]).expect("empty workspace projects");
        assert!(tree.search("MonoBehaviour").is_empty());
    }
}
