//! Primary-content export pipeline ported from `PrimaryContentExporter`.
//!
//! The second export pipeline extracts raw primary content (JSON, GLB models, binaries, audio,
//! textures, scripts) instead of Unity project collections. Its observable differences from the
//! Unity-project pipeline are:
//!
//! - The handler stack holds `IContentExtractor` values behind `RegisterHandler`, starting with
//!   a `JsonContentExtractor` catch-all for `IUnityObjectBase` and ending with the
//!   `DeletedAssetsExporter` for `IUnityObjectBase`. In between come 20 empty handlers, the GLB
//!   model/mesh/navmesh/terrain handlers, the shared binary handler for text/font/movie/video
//!   assets, and the audio, texture (image-format driven), and script (C# language driven)
//!   handlers. Registration order below is the source order.
//! - Collection assembly adds each fetched asset to the queued set *first* and drops
//!   `EmptyExportCollection` results without queuing their remaining members, unlike the
//!   Unity-project pipeline which queues every member of every created collection.
//! - The export loop counts progress as one-based `(i + 1)/collections.Count` over *all*
//!   collections (the Unity-project loop counts exportable collections only) and exports to
//!   `ExportRootPath` rather than the derived project root.
//! - `SingleExportCollection.Export` reuses the best-directory plus unique-name flow but with
//!   the full `MaxFileNameLength` budget (no `.meta` sidecar is reserved) and writes no sidecar.
//! - `ExportCollectionBase.ExportAsset` also resolves unique names against the full
//!   `MaxFileNameLength` budget.
//!
//! Extractor internals, script language resolution, and actual file writes stay outside this
//! module; only the handler table, the assembly/export-loop deltas, the progress format, and
//! the single-asset destination planner are ported.

use super::asset_path::{
    fix_invalid_file_name_characters, get_unique_name, join_path, remove_clone_suffixes,
    remove_instance_suffixes, AssetPathError, ExportAssetPathIdentity, ExportFileSystem,
    HostPathPlatform, MAX_FILE_NAME_LENGTH,
};

/// One `RegisterHandler` call in `RegisterDefaultHandlers` source order.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct PrimaryHandlerRegistration {
    /// The asset interface or class the extractor is registered for.
    pub target: &'static str,
    /// The content extractor type name.
    pub extractor: &'static str,
    /// True for the twenty `RegisterEmptyHandler` entries.
    pub is_empty_handler: bool,
}

/// Every primary-content handler registration in source order.
pub const PRIMARY_HANDLERS: &[PrimaryHandlerRegistration] = &[
    PrimaryHandlerRegistration {
        target: "IUnityObjectBase",
        extractor: "JsonContentExtractor",
        is_empty_handler: false,
    },
    PrimaryHandlerRegistration {
        target: "IAnimation",
        extractor: "EmptyContentExtractor",
        is_empty_handler: true,
    },
    PrimaryHandlerRegistration {
        target: "IAnimationClip",
        extractor: "EmptyContentExtractor",
        is_empty_handler: true,
    },
    PrimaryHandlerRegistration {
        target: "IAnimator",
        extractor: "EmptyContentExtractor",
        is_empty_handler: true,
    },
    PrimaryHandlerRegistration {
        target: "IAnimatorController",
        extractor: "EmptyContentExtractor",
        is_empty_handler: true,
    },
    PrimaryHandlerRegistration {
        target: "IAnimatorOverrideController",
        extractor: "EmptyContentExtractor",
        is_empty_handler: true,
    },
    PrimaryHandlerRegistration {
        target: "IAnimatorState",
        extractor: "EmptyContentExtractor",
        is_empty_handler: true,
    },
    PrimaryHandlerRegistration {
        target: "IAnimatorStateMachine",
        extractor: "EmptyContentExtractor",
        is_empty_handler: true,
    },
    PrimaryHandlerRegistration {
        target: "IAnimatorStateTransition",
        extractor: "EmptyContentExtractor",
        is_empty_handler: true,
    },
    PrimaryHandlerRegistration {
        target: "IAnimatorTransition",
        extractor: "EmptyContentExtractor",
        is_empty_handler: true,
    },
    PrimaryHandlerRegistration {
        target: "IAnimatorTransitionBase",
        extractor: "EmptyContentExtractor",
        is_empty_handler: true,
    },
    PrimaryHandlerRegistration {
        target: "IAvatar",
        extractor: "EmptyContentExtractor",
        is_empty_handler: true,
    },
    PrimaryHandlerRegistration {
        target: "IBlendTree",
        extractor: "EmptyContentExtractor",
        is_empty_handler: true,
    },
    PrimaryHandlerRegistration {
        target: "IComponent",
        extractor: "EmptyContentExtractor",
        is_empty_handler: true,
    },
    PrimaryHandlerRegistration {
        target: "IComputeShader",
        extractor: "EmptyContentExtractor",
        is_empty_handler: true,
    },
    PrimaryHandlerRegistration {
        target: "ILightingDataAsset",
        extractor: "EmptyContentExtractor",
        is_empty_handler: true,
    },
    PrimaryHandlerRegistration {
        target: "IMaterial",
        extractor: "EmptyContentExtractor",
        is_empty_handler: true,
    },
    PrimaryHandlerRegistration {
        target: "IPreloadData",
        extractor: "EmptyContentExtractor",
        is_empty_handler: true,
    },
    PrimaryHandlerRegistration {
        target: "IRuntimeAnimatorController",
        extractor: "EmptyContentExtractor",
        is_empty_handler: true,
    },
    PrimaryHandlerRegistration {
        target: "ISceneAsset",
        extractor: "EmptyContentExtractor",
        is_empty_handler: true,
    },
    PrimaryHandlerRegistration {
        target: "SpriteInformationObject",
        extractor: "EmptyContentExtractor",
        is_empty_handler: true,
    },
    PrimaryHandlerRegistration {
        target: "GameObjectHierarchyObject",
        extractor: "GlbModelExporter",
        is_empty_handler: false,
    },
    PrimaryHandlerRegistration {
        target: "IGameObject",
        extractor: "GlbModelExporter",
        is_empty_handler: false,
    },
    PrimaryHandlerRegistration {
        target: "IComponent",
        extractor: "GlbModelExporter",
        is_empty_handler: false,
    },
    PrimaryHandlerRegistration {
        target: "ILevelGameManager",
        extractor: "GlbModelExporter",
        is_empty_handler: false,
    },
    PrimaryHandlerRegistration {
        target: "IMesh",
        extractor: "GlbMeshExporter",
        is_empty_handler: false,
    },
    PrimaryHandlerRegistration {
        target: "INavMeshData",
        extractor: "GlbNavMeshExporter",
        is_empty_handler: false,
    },
    PrimaryHandlerRegistration {
        target: "ITerrainData",
        extractor: "GlbTerrainExporter",
        is_empty_handler: false,
    },
    PrimaryHandlerRegistration {
        target: "ITextAsset",
        extractor: "BinaryAssetContentExtractor",
        is_empty_handler: false,
    },
    PrimaryHandlerRegistration {
        target: "IFont",
        extractor: "BinaryAssetContentExtractor",
        is_empty_handler: false,
    },
    PrimaryHandlerRegistration {
        target: "IMovieTexture",
        extractor: "BinaryAssetContentExtractor",
        is_empty_handler: false,
    },
    PrimaryHandlerRegistration {
        target: "IVideoClip",
        extractor: "BinaryAssetContentExtractor",
        is_empty_handler: false,
    },
    PrimaryHandlerRegistration {
        target: "IAudioClip",
        extractor: "AudioContentExtractor",
        is_empty_handler: false,
    },
    PrimaryHandlerRegistration {
        target: "IImageTexture",
        extractor: "TextureExporter",
        is_empty_handler: false,
    },
    PrimaryHandlerRegistration {
        target: "IMonoScript",
        extractor: "ScriptContentExtractor",
        is_empty_handler: false,
    },
    PrimaryHandlerRegistration {
        target: "IUnityObjectBase",
        extractor: "DeletedAssetsExporter",
        is_empty_handler: false,
    },
];

/// All registrations for one target, in source order.
pub fn handlers_for_target(
    target: &str,
) -> impl Iterator<Item = &'static PrimaryHandlerRegistration> + '_ {
    PRIMARY_HANDLERS
        .iter()
        .filter(move |registration| registration.target == target)
}

/// Format one export progress line: `(current/total) Exporting 'name'`.
///
/// The primary-content loop passes the one-based index over all collections; the
/// Unity-project loop passes the exportable-only counter instead.
pub fn format_export_progress(one_based_current: usize, total: usize, name: &str) -> String {
    format!("({one_based_current}/{total}) Exporting '{name}'")
}

/// One asset as `CreateCollections` sees it.
///
/// The caller resolves the two questions the source answers by type tests: which extractor claims
/// the asset, and which assets that extractor's collection would also claim. Keeping both as
/// caller facts lets the assembly loop be exercised without building a whole game.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrimaryContentAsset<'a> {
    /// A stable identity for the `queued` set, which the source keys by asset reference.
    pub identity: u64,
    /// The extractor the handler stack selects, or `None` when no handler claims the asset.
    ///
    /// `RegisterHandler<IUnityObjectBase>(JsonContentExtractor)` is registered first and the
    /// deleted-asset handler last, so in practice a fetched asset always resolves; `None` models
    /// the caller passing an asset the stack declined.
    pub extractor: Option<&'static str>,
    /// True when the selected extractor produced an **empty** export collection.
    ///
    /// `EmptyExportCollection` results are dropped *without* queuing their remaining members,
    /// which is the documented difference from the Unity-project pipeline.
    pub is_empty_collection: bool,
    /// Every asset the selected collection would claim, including this one.
    ///
    /// `MultipleExportCollection` reports the primary asset first and then its secondaries; the
    /// caller supplies the same list the extractor's collection would expose.
    pub collection_assets: &'a [u64],
    /// `ExportCollectionBase.Name` for the collection this asset produces.
    pub collection_name: &'a str,
}

/// One collection `CreateCollections` produced.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrimaryContentCollection {
    /// The extractor that owns the collection.
    pub extractor: &'static str,
    /// The primary asset's identity: the one the loop visited when the collection was created.
    pub primary: u64,
    /// Every asset the collection claims, primary first, in the order the extractor reported.
    pub assets: Vec<u64>,
    /// `ExportCollectionBase.Name`.
    ///
    /// The source declares this abstract and each collection derives it differently - a single
    /// asset collection uses the asset's best name, `EmptyExportCollection` uses its own type
    /// name - so the caller supplies the resolved name rather than this module guessing a rule.
    pub name: String,
}

impl PrimaryContentCollection {
    /// `ExportCollectionBase.Name`.
    pub fn collection_name(&self) -> String {
        self.name.clone()
    }
}

/// `PrimaryContentExporter.CreateCollections` over caller-resolved assets.
///
/// The loop, in source order:
///
/// ```text
/// for asset in FetchAssets():
///     if !queued.Add(asset):  continue          # already claimed, including by an earlier collection
///     collection = CreateCollection(asset)
///     if collection is EmptyExportCollection:  continue   # note: members are NOT queued
///     foreach element in collection.Assets: queued.Add(element)
///     collections.Add(collection)
/// ```
///
/// Two details are load-bearing and easy to get backwards. The asset is added to `queued`
/// **before** the empty check, so an asset whose collection turned out empty is still marked and
/// will not be reconsidered. And a non-empty collection queues **all** its members, so an asset
/// claimed by an earlier collection is never exported twice even if a later handler would also
/// accept it.
pub fn create_primary_content_collections(
    assets: &[PrimaryContentAsset],
) -> Vec<PrimaryContentCollection> {
    let mut queued: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut collections = Vec::new();
    for asset in assets {
        // `queued.Add` runs first: a duplicate is skipped even when the earlier visit produced an
        // empty collection.
        if !queued.insert(asset.identity) {
            continue;
        }
        let Some(extractor) = asset.extractor else {
            continue;
        };
        if asset.is_empty_collection {
            // The empty check comes after the queue insert and skips the member queueing.
            continue;
        }
        let members: Vec<u64> = asset.collection_assets.to_vec();
        for member in &members {
            queued.insert(*member);
        }
        collections.push(PrimaryContentCollection {
            extractor,
            primary: asset.identity,
            assets: members,
            name: asset.collection_name.to_owned(),
        });
    }
    collections
}

/// The number of collections the export loop reports progress against.
///
/// `format_export_progress` is called with the one-based index over **all** collections, not only
/// the exportable ones, which is the second documented difference from the project pipeline.
pub fn primary_content_progress_total(collections: &[PrimaryContentCollection]) -> usize {
    collections.len()
}

/// One collection as the export loop sees it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrimaryContentExportStep<'a> {
    pub collection: &'a PrimaryContentCollection,
    /// `ExportCollectionBase.Exportable`: false collections are skipped without a progress line.
    pub exportable: bool,
}

/// One line the export loop emits, in the order it emits it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PrimaryContentExportEvent {
    /// `(i + 1)/count` over **all** collections, then the collection name.
    Progress {
        /// One-based index over all collections, including the skipped ones.
        one_based_index: usize,
        /// The total number of collections, including the skipped ones.
        total: usize,
        /// The collection name.
        name: String,
        /// The exact line the source logs, produced by [`format_export_progress`].
        line: String,
    },
    /// `collection.Export` returned false.
    Failed { name: String },
}

/// `PrimaryContentExporter.Export` over already-assembled collections.
///
/// The loop, in source order:
///
/// ```text
/// for i in 0..collections.Count:
///     collection = collections[i]
///     if collection.Exportable:
///         log((i + 1)/collections.Count, collection.Name)
///         if !collection.Export(root, fileSystem):
///             log warning "Failed to export '<name>'"
/// ```
///
/// Note what the index means: it counts **every** collection produced by `CreateCollections`,
/// including ones this loop skips, so a run whose first three collections are not exportable
/// still starts reporting at `(4/N)`. The project pipeline instead counts only the collections it
/// exports, which is why the two loops cannot share a counter.
///
/// The caller supplies `export_one`, which performs the write and reports whether it succeeded.
/// Returns the events in order so the loop's logging is testable without a file system.
pub fn run_primary_content_export_loop<F>(
    collections: &[PrimaryContentCollection],
    steps: &[PrimaryContentExportStep<'_>],
    mut export_one: F,
) -> Vec<PrimaryContentExportEvent>
where
    F: FnMut(&PrimaryContentCollection) -> bool,
{
    let total = collections.len();
    let mut events = Vec::new();
    for (index, step) in steps.iter().enumerate() {
        if !step.exportable {
            continue;
        }
        let name = step.collection.collection_name();
        let line = format_export_progress(index + 1, total, &name);
        events.push(PrimaryContentExportEvent::Progress {
            one_based_index: index + 1,
            total,
            name: name.clone(),
            line,
        });
        if !export_one(step.collection) {
            events.push(PrimaryContentExportEvent::Failed { name });
        }
    }
    events
}

/// Destinations selected by `SingleExportCollection.Export`, which writes no `.meta` sidecar.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedSingleAssetPaths {
    pub subdirectory: String,
    pub file_name: String,
    pub file_path: String,
}

/// Mirror the destination selection of `SingleExportCollection.Export` without touching the disk.
///
/// The flow matches the asset unique-name flow except the name budget is the full
/// `MaxFileNameLength`: no `.meta` extension is reserved.
pub fn plan_single_export_paths(
    identity: &ExportAssetPathIdentity,
    extension: &str,
    platform: HostPathPlatform,
    state: &mut super::asset_path::UniqueNameState,
    file_system: &impl ExportFileSystem,
    project_directory: &str,
) -> Result<PlannedSingleAssetPaths, AssetPathError> {
    let stem = remove_clone_suffixes(identity.best_name());
    let stem = remove_instance_suffixes(&stem);
    let trimmed = stem.trim();
    let mut file_name = if trimmed.is_empty() {
        identity.class_name.clone()
    } else {
        fix_invalid_file_name_characters(trimmed, platform)?
    };
    file_name.reserve(1 + extension.len());
    file_name.push('.');
    file_name.push_str(extension);
    let fixed_directory =
        super::asset_path::fix_invalid_path_characters(&identity.best_directory(), platform)?;
    if fixed_directory.is_empty() {
        return Err(AssetPathError::AllocationFailed {
            resource: "export subdirectory",
        });
    }
    let subdirectory = join_path(project_directory, &fixed_directory, platform);
    let unique_name = get_unique_name(
        state,
        platform,
        file_system,
        &subdirectory,
        &file_name,
        MAX_FILE_NAME_LENGTH,
    )?;
    let file_path = join_path(&subdirectory, &unique_name, platform);
    Ok(PlannedSingleAssetPaths {
        subdirectory,
        file_name: unique_name,
        file_path,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        create_primary_content_collections, format_export_progress, handlers_for_target,
        plan_single_export_paths, primary_content_progress_total, run_primary_content_export_loop,
        PrimaryContentAsset, PrimaryContentExportEvent, PrimaryContentExportStep, PRIMARY_HANDLERS,
    };
    use crate::export::asset_path::{
        join_path, ExportAssetPathIdentity, ExportFileSystem, HostPathPlatform, UniqueNameState,
        MAX_FILE_NAME_LENGTH,
    };

    /// A collection that claims only `identity`, which is what the single-asset extractors do.
    ///
    /// The name is explicit so a test that asserts on a progress line says what it expects instead
    /// of depending on a naming rule hidden in this helper.
    fn solo(
        identity: u64,
        extractor: &'static str,
        name: &'static str,
    ) -> PrimaryContentAsset<'static> {
        const ONE_2: [u64; 1] = [2];
        const ONE_3: [u64; 1] = [3];
        const ONE_6: [u64; 1] = [6];
        const ONE_7: [u64; 1] = [7];
        let members: &'static [u64] = match identity {
            2 => &ONE_2,
            3 => &ONE_3,
            6 => &ONE_6,
            7 => &ONE_7,
            _ => &[],
        };
        PrimaryContentAsset {
            identity,
            extractor: Some(extractor),
            is_empty_collection: false,
            collection_assets: members,
            collection_name: name,
        }
    }

    /// The assembly loop's two ordering rules, which are easy to get backwards.
    #[test]
    fn primary_content_assembly_queues_before_the_empty_check_and_queues_members() {
        // 1. A duplicate asset is skipped even though the earlier visit produced an empty
        //    collection, because `queued.Add` runs *before* the empty check.
        let empty_then_retry = [
            PrimaryContentAsset {
                identity: 7,
                extractor: Some("GlbModelExporter"),
                is_empty_collection: true,
                collection_assets: &[7],
                collection_name: "Empty",
            },
            solo(7, "JsonContentExtractor", "Seven"),
        ];
        assert!(
            create_primary_content_collections(&empty_then_retry).is_empty(),
            "an asset queued by an empty collection is not reconsidered"
        );

        // 2. A non-empty collection queues ALL its members, so a later asset already claimed is
        //    not exported a second time.
        let members = [1_u64, 2, 3];
        let claiming = [
            PrimaryContentAsset {
                identity: 1,
                extractor: Some("GlbModelExporter"),
                is_empty_collection: false,
                collection_assets: &members,
                collection_name: "Model",
            },
            solo(2, "JsonContentExtractor", "Two"),
            solo(3, "JsonContentExtractor", "Three"),
            solo(4, "JsonContentExtractor", "Four"),
        ];
        let collections = create_primary_content_collections(&claiming);
        let primaries: Vec<u64> = collections
            .iter()
            .map(|collection| collection.primary)
            .collect();
        assert_eq!(primaries, [1, 4], "members 2 and 3 were already claimed");
        assert_eq!(collections[0].assets, members.to_vec());
        assert_eq!(collections[0].extractor, "GlbModelExporter");
        // The progress total counts all collections, which is the primary-content delta.
        assert_eq!(primary_content_progress_total(&collections), 2);
    }

    /// An asset the handler stack declined is skipped without becoming a collection.
    #[test]
    fn primary_content_assembly_skips_an_unclaimed_asset() {
        let assets = [PrimaryContentAsset {
            identity: 9,
            extractor: None,
            is_empty_collection: false,
            collection_assets: &[9],
            collection_name: "Unclaimed",
        }];
        assert!(create_primary_content_collections(&assets).is_empty());
    }

    /// A primary asset is claimed by its own collection even when the member list omits it.
    #[test]
    fn primary_content_assembly_keeps_the_primary_asset() {
        let assets = [PrimaryContentAsset {
            identity: 5,
            extractor: Some("AudioContentExtractor"),
            is_empty_collection: false,
            collection_assets: &[5, 6],
            collection_name: "Audio",
        }];
        let collections = create_primary_content_collections(&assets);
        assert_eq!(collections.len(), 1);
        assert_eq!(collections[0].primary, 5);
        assert_eq!(collections[0].assets, [5, 6]);
        assert_eq!(collections[0].collection_name(), "Audio");
    }

    /// The export loop's progress counter spans **all** collections, not the exportable ones.
    #[test]
    fn primary_content_export_loop_indexes_over_every_collection() {
        let collections = create_primary_content_collections(&[
            solo(1, "JsonContentExtractor", "One"),
            solo(2, "JsonContentExtractor", "Two"),
            solo(3, "JsonContentExtractor", "Three"),
            solo(4, "JsonContentExtractor", "Four"),
        ]);
        assert_eq!(collections.len(), 4);
        // Skip the first two: the loop still reports the third as (3/4), not (1/2).
        let steps: Vec<PrimaryContentExportStep<'_>> = collections
            .iter()
            .enumerate()
            .map(|(index, collection)| PrimaryContentExportStep {
                collection,
                exportable: index >= 2,
            })
            .collect();
        let mut exported = 0;
        let events = run_primary_content_export_loop(&collections, &steps, |_| {
            exported += 1;
            true
        });
        assert_eq!(exported, 2, "only the exportable collections were written");
        let lines: Vec<(usize, usize, &str)> = events
            .iter()
            .map(|event| match event {
                PrimaryContentExportEvent::Progress {
                    one_based_index,
                    total,
                    line,
                    ..
                } => (*one_based_index, *total, line.as_str()),
                PrimaryContentExportEvent::Failed { .. } => panic!("no failures expected"),
            })
            .collect();
        assert_eq!(
            lines,
            [
                (3, 4, "(3/4) Exporting 'Three'"),
                (4, 4, "(4/4) Exporting 'Four'"),
            ],
            "the index counts skipped collections, and the line is the source format"
        );
    }

    /// A failed write emits the progress line and then the failure warning, in that order.
    #[test]
    fn primary_content_export_loop_reports_a_failed_collection() {
        let collections =
            create_primary_content_collections(&[solo(7, "JsonContentExtractor", "Seven")]);
        let steps = [PrimaryContentExportStep {
            collection: &collections[0],
            exportable: true,
        }];
        let events = run_primary_content_export_loop(&collections, &steps, |_| false);
        assert_eq!(
            events,
            [
                PrimaryContentExportEvent::Progress {
                    one_based_index: 1,
                    total: 1,
                    name: "Seven".into(),
                    line: "(1/1) Exporting 'Seven'".into(),
                },
                PrimaryContentExportEvent::Failed {
                    name: "Seven".into(),
                },
            ],
            "progress is logged first, then the failure warning"
        );
    }

    /// A non-exportable collection produces no events at all, not even a skip notice.
    #[test]
    fn primary_content_export_loop_is_silent_for_skipped_collections() {
        let collections =
            create_primary_content_collections(&[solo(7, "JsonContentExtractor", "Seven")]);
        let steps = [PrimaryContentExportStep {
            collection: &collections[0],
            exportable: false,
        }];
        let events = run_primary_content_export_loop(&collections, &steps, |_| {
            panic!("a non-exportable collection must not be written")
        });
        assert!(events.is_empty());
    }
    use std::collections::BTreeSet;

    #[derive(Default)]
    struct TestFileSystem {
        directories: BTreeSet<String>,
        files: BTreeSet<String>,
    }

    impl TestFileSystem {
        fn joined(directory: &str, name: &str) -> String {
            join_path(directory, name, HostPathPlatform::Windows)
        }
    }

    impl ExportFileSystem for TestFileSystem {
        fn directory_exists(&self, directory: &str) -> bool {
            self.directories.contains(directory)
        }

        fn file_exists(&self, directory: &str, name: &str) -> bool {
            self.files.contains(&Self::joined(directory, name))
        }
    }

    #[test]
    fn handler_table_opens_json_first_and_closes_deleted_last() {
        // 1 catch-all plus 20 empty handlers plus 14 content handlers plus the deleted tail.
        assert_eq!(PRIMARY_HANDLERS.len(), 36);
        assert_eq!(PRIMARY_HANDLERS.first().unwrap().target, "IUnityObjectBase");
        assert_eq!(
            PRIMARY_HANDLERS.first().unwrap().extractor,
            "JsonContentExtractor"
        );
        assert_eq!(
            PRIMARY_HANDLERS.last().unwrap().extractor,
            "DeletedAssetsExporter"
        );
        assert_eq!(
            PRIMARY_HANDLERS
                .iter()
                .filter(|registration| registration.is_empty_handler)
                .count(),
            20
        );
        // The shared binary extractor covers all four binary asset kinds.
        for target in ["ITextAsset", "IFont", "IMovieTexture", "IVideoClip"] {
            let extractors: Vec<&str> = handlers_for_target(target)
                .map(|registration| registration.extractor)
                .collect();
            assert_eq!(extractors, ["BinaryAssetContentExtractor"], "{target}");
        }
        // The model extractor shadows the empty component handler.
        let component: Vec<&str> = handlers_for_target("IComponent")
            .map(|registration| registration.extractor)
            .collect();
        assert_eq!(component, ["EmptyContentExtractor", "GlbModelExporter"]);
    }

    #[test]
    fn progress_lines_use_one_based_positions_over_all_collections() {
        assert_eq!(
            format_export_progress(1, 3, "Cube"),
            "(1/3) Exporting 'Cube'"
        );
        assert_eq!(
            format_export_progress(3, 3, "Scene"),
            "(3/3) Exporting 'Scene'"
        );
    }

    #[test]
    fn single_export_uses_the_full_name_budget_without_a_sidecar() {
        let file_system = TestFileSystem::default();
        let mut state = UniqueNameState::default();
        let identity = ExportAssetPathIdentity {
            class_name: "Mesh".to_owned(),
            named_name: Some("Cube".to_owned()),
            ..Default::default()
        };
        let planned = plan_single_export_paths(
            &identity,
            "glb",
            HostPathPlatform::Windows,
            &mut state,
            &file_system,
            "Project",
        )
        .unwrap();
        assert_eq!(planned.subdirectory, "Project\\Assets/Mesh");
        assert_eq!(planned.file_name, "Cube.glb");
        assert_eq!(planned.file_path, "Project\\Assets/Mesh\\Cube.glb");
        assert_eq!(MAX_FILE_NAME_LENGTH, 245);
    }
}
