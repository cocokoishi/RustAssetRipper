//! Project export ordering ported from `ProjectExporter.Export`.
//!
//! The baseline project export runs in a fixed order:
//!
//! 1. `CreateCollections` walks `GameBundle.FetchAssets()` in order. Each asset not already
//!    queued creates one collection through the exporter stack, and every asset of that
//!    collection joins the queued set, so later duplicates never open a second collection.
//! 2. The export loop sets `CurrentCollection` to each collection in turn and calls `Export`
//!    only when `Exportable` is true. A `false` result logs a `Failed to export` warning while a
//!    skipped collection stays silent.
//! 3. `EventExportProgressUpdated` fires once per collection with its index and the total count,
//!    including skipped collections. The preparation/export started/finished events bracket
//!    planning and the loop in source order.
//! 4. `CreateCollection` throws when no exporter in the handler stack handles an asset, which
//!    aborts planning with no partial result; the fallible creator models that throw.
//!
//! Asset content, the exporter stack, and the event subscribers stay outside this module; only
//! the order, the queued-set rule, the export gate, the progress steps, the lifecycle events,
//! the unhandled-asset abort, the export-start version listing, and the settings-identity
//! guard are ported, over caller-supplied asset and collection identities.

use std::collections::HashSet;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::hash::Hash;

/// Error returned when the project export order cannot be planned within bounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectOrderError {
    /// More distinct queued assets were collected than the caller allows.
    QueuedAssetLimitExceeded { limit: usize },
    /// More collections were created than the caller allows.
    CollectionLimitExceeded { limit: usize },
    /// The `Exportable` gate and the `Export` result disagree, which the baseline never produces.
    InconsistentGateAndResult,
    /// No exporter in the handler stack handles an asset, so `CreateCollection` throws and
    /// planning aborts. The source message names the asset; the generic key cannot render
    /// here, so callers attach the asset identity to this error.
    UnhandledAsset,
    /// An export handler was built from different settings than the loader holds, so
    /// `ThrowIfSettingsDontMatch` rejects it. Reference identity is caller-observed.
    SettingsMismatch,
    /// A bounded allocation for the plan could not be reserved.
    AllocationFailed { resource: &'static str },
}

impl Display for ProjectOrderError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::QueuedAssetLimitExceeded { limit } => write!(
                formatter,
                "project export queued more assets than the limit of {limit}"
            ),
            Self::CollectionLimitExceeded { limit } => write!(
                formatter,
                "project export created more collections than the limit of {limit}"
            ),
            Self::InconsistentGateAndResult => write!(
                formatter,
                "project export gate and result disagree, which the baseline never produces"
            ),
            Self::UnhandledAsset => {
                write!(formatter, "there is no exporter that can handle the asset")
            }
            Self::SettingsMismatch => {
                write!(formatter, "export handler settings don't match")
            }
            Self::AllocationFailed { resource } => {
                write!(formatter, "could not allocate {resource}")
            }
        }
    }
}

impl Error for ProjectOrderError {}

/// Bounds applied to the unbounded source loops in `CreateCollections`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProjectOrderLimits {
    /// Maximum distinct queued assets across all collections.
    pub max_queued_assets: usize,
    /// Maximum created collections.
    pub max_collections: usize,
}

impl Default for ProjectOrderLimits {
    fn default() -> Self {
        Self {
            max_queued_assets: 10_000_000,
            max_collections: 10_000_000,
        }
    }
}

/// The outcome of one collection in the `ProjectExporter.Export` loop.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CollectionExportOutcome {
    /// `Exportable` is false: the baseline skips `Export` silently.
    SkippedNotExportable,
    /// `Export` returned true.
    Succeeded,
    /// `Export` returned false: the baseline logs a `Failed to export` warning.
    Failed,
}

/// Classify one loop step from the `Exportable` gate and the `Export` result.
///
/// `None` means the baseline never called `Export` for this collection.
pub fn classify_collection_outcome(
    is_exportable: bool,
    export_result: Option<bool>,
) -> Result<CollectionExportOutcome, ProjectOrderError> {
    match (is_exportable, export_result) {
        (false, None) => Ok(CollectionExportOutcome::SkippedNotExportable),
        (true, Some(true)) => Ok(CollectionExportOutcome::Succeeded),
        (true, Some(false)) => Ok(CollectionExportOutcome::Failed),
        _ => Err(ProjectOrderError::InconsistentGateAndResult),
    }
}

/// The collection order assembled by `CreateCollections`, plus the exportable count the
/// baseline logs as its progress denominator.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedProjectExport<C> {
    pub collections: Vec<C>,
    pub exportable_count: usize,
}

impl<C> PlannedProjectExport<C> {
    /// One `(index, total)` progress step per collection, mirroring
    /// `EventExportProgressUpdated(i, collections.Count)`.
    pub fn progress_steps(&self) -> Vec<(usize, usize)> {
        (0..self.collections.len())
            .map(|index| (index, self.collections.len()))
            .collect()
    }
}

/// One `ProjectExporter` lifecycle event, in source order.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ProjectExportEvent {
    /// `EventExportPreparationStarted`, before `CreateCollections`.
    PreparationStarted,
    /// `EventExportPreparationFinished`, after `CreateCollections`.
    PreparationFinished,
    /// `EventExportStarted`, before the export loop.
    ExportStarted,
    /// `EventExportProgressUpdated(index, total)` after each collection.
    ProgressUpdated { index: usize, total: usize },
    /// `EventExportFinished`, after the export loop.
    ExportFinished,
}

/// The recorded outcome of the export loop: its lifecycle events plus the indexes of
/// collections whose `Export` returned false (the `Failed to export` warnings).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectExportRun {
    /// Every lifecycle event in source order.
    pub events: Vec<ProjectExportEvent>,
    /// Indexes of failed collections, in loop order.
    pub failed_collections: Vec<usize>,
}

/// Mirror `CreateCollections`: walk fetched assets in order, open one collection per unqueued
/// asset, and queue every asset of each new collection.
///
/// The fallible creator mirrors the source throw: when no exporter handles an asset,
/// planning aborts with `UnhandledAsset` and no partial plan is returned.
pub fn plan_project_export_order<A, C>(
    fetch_assets: &[A],
    is_exportable: impl Fn(&C) -> bool,
    create_collection: impl Fn(&A) -> Result<C, ProjectOrderError>,
    assets_of: impl Fn(&C) -> Vec<A>,
    limits: ProjectOrderLimits,
) -> Result<PlannedProjectExport<C>, ProjectOrderError>
where
    A: Clone + Eq + Hash,
{
    let mut queued: HashSet<A> = HashSet::new();
    let mut collections: Vec<C> = Vec::new();
    let mut exportable_count = 0usize;
    for asset in fetch_assets {
        if queued.contains(asset) {
            continue;
        }
        if collections.len() >= limits.max_collections {
            return Err(ProjectOrderError::CollectionLimitExceeded {
                limit: limits.max_collections,
            });
        }
        let collection = create_collection(asset)?;
        if is_exportable(&collection) {
            exportable_count += 1;
        }
        for element in assets_of(&collection) {
            if queued.len() >= limits.max_queued_assets && !queued.contains(&element) {
                return Err(ProjectOrderError::QueuedAssetLimitExceeded {
                    limit: limits.max_queued_assets,
                });
            }
            queued.insert(element);
        }
        collections.push(collection);
    }
    Ok(PlannedProjectExport {
        collections,
        exportable_count,
    })
}

/// Plan with the preparation lifecycle events: `PreparationStarted` always fires first;
/// `PreparationFinished` fires only when planning succeeds, mirroring the source throw
/// out of `CreateCollections`.
pub fn plan_project_export_with_events<A, C>(
    fetch_assets: &[A],
    is_exportable: impl Fn(&C) -> bool,
    create_collection: impl Fn(&A) -> Result<C, ProjectOrderError>,
    assets_of: impl Fn(&C) -> Vec<A>,
    limits: ProjectOrderLimits,
) -> (
    Vec<ProjectExportEvent>,
    Result<PlannedProjectExport<C>, ProjectOrderError>,
)
where
    A: Clone + Eq + Hash,
{
    let mut events = vec![ProjectExportEvent::PreparationStarted];
    let planned = plan_project_export_order(
        fetch_assets,
        is_exportable,
        create_collection,
        assets_of,
        limits,
    );
    if planned.is_ok() {
        events.push(ProjectExportEvent::PreparationFinished);
    }
    (events, planned)
}

/// Mirror the `ProjectExporter.Export` loop over an already planned export: `ExportStarted`
/// fires, each collection exports only when exportable (a false result records a `Failed to
/// export` warning index), `ProgressUpdated` fires per collection including skipped ones,
/// and `ExportFinished` closes the run.
pub fn run_project_export_loop<C>(
    planned: &PlannedProjectExport<C>,
    is_exportable: impl Fn(&C) -> bool,
    export_collection: impl Fn(usize, &C) -> bool,
) -> ProjectExportRun {
    let mut events = vec![ProjectExportEvent::ExportStarted];
    let mut failed_collections = Vec::new();
    for (index, collection) in planned.collections.iter().enumerate() {
        if is_exportable(collection) && !export_collection(index, collection) {
            failed_collections.push(index);
        }
        events.push(ProjectExportEvent::ProgressUpdated {
            index,
            total: planned.collections.len(),
        });
    }
    events.push(ProjectExportEvent::ExportFinished);
    ProjectExportRun {
        events,
        failed_collections,
    }
}

/// Mirror the `Failed to export` warning text of the `ProjectExporter.Export` loop:
/// the collection name plus its runtime type name.
///
/// Both arrive as caller facts; the port only fixes the text shape.
pub fn format_export_failure(collection_name: &str, collection_type_name: &str) -> String {
    format!("Failed to export '{collection_name}' ({collection_type_name})")
}

/// `ExportHandler.Export` logs the distinct Unity versions across the fetched asset
/// collections (`GetListOfVersions`): first-seen order, single-space separated.
///
/// Versions arrive as caller facts (the port carries them opaquely); only the
/// `Distinct` plus `string.Join(' ', ...)` shape is ported.
pub fn distinct_collection_versions(versions: &[&str]) -> String {
    let mut seen: Vec<&str> = Vec::new();
    for version in versions {
        if !seen.contains(version) {
            seen.push(version);
        }
    }
    seen.join(" ")
}

/// `ExportHandler.ThrowIfSettingsDontMatch`: the loader (`GameFileLoader.ExportHandler`
/// setter) only accepts a handler built from its own settings instance.
///
/// Reference identity is caller-observed, so the check takes the comparison outcome: a
/// mismatch is an explicit error carrying the source message, never a silent accept.
pub const fn throw_if_settings_dont_match(same_settings: bool) -> Result<(), ProjectOrderError> {
    if same_settings {
        Ok(())
    } else {
        Err(ProjectOrderError::SettingsMismatch)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        classify_collection_outcome, distinct_collection_versions, format_export_failure,
        plan_project_export_order, plan_project_export_with_events, run_project_export_loop,
        throw_if_settings_dont_match, CollectionExportOutcome, ProjectExportEvent,
        ProjectOrderError, ProjectOrderLimits,
    };
    use std::collections::HashMap;
    use std::rc::Rc;

    struct Harness {
        exportable: AssetPredicate,
        create: AssetProjector,
        assets: CollectionMembers,
    }

    type AssetPredicate = Box<dyn Fn(&char) -> bool>;
    type AssetProjector = Box<dyn Fn(&char) -> Result<char, ProjectOrderError>>;
    type CollectionMembers = Box<dyn Fn(&char) -> Vec<char>>;

    fn harness(groups: HashMap<char, (Vec<char>, bool)>) -> Harness {
        let groups = Rc::new(groups);
        let exportable_groups = Rc::clone(&groups);
        let assets_groups = Rc::clone(&groups);
        Harness {
            exportable: Box::new(move |collection: &char| exportable_groups[collection].1),
            create: Box::new(|asset: &char| Ok(*asset)),
            assets: Box::new(move |collection: &char| assets_groups[collection].0.clone()),
        }
    }

    #[test]
    fn collection_order_follows_fetch_order_with_queued_dedup() {
        let groups: HashMap<char, (Vec<char>, bool)> =
            [('a', (vec!['a', 'b'], true)), ('c', (vec!['c'], true))]
                .into_iter()
                .collect();
        let h = harness(groups);
        let planned = plan_project_export_order(
            &['a', 'b', 'c'],
            h.exportable,
            h.create,
            h.assets,
            ProjectOrderLimits::default(),
        )
        .unwrap();
        // 'b' joins the queued set through 'a' and never opens its own collection.
        assert_eq!(planned.collections, vec!['a', 'c']);
        assert_eq!(planned.exportable_count, 2);
        assert_eq!(planned.progress_steps(), vec![(0, 2), (1, 2)]);
    }

    #[test]
    fn non_exportable_collections_keep_order_and_progress_steps() {
        let groups: HashMap<char, (Vec<char>, bool)> =
            [('a', (vec!['a'], false)), ('b', (vec!['b'], true))]
                .into_iter()
                .collect();
        let h = harness(groups);
        let planned = plan_project_export_order(
            &['a', 'b'],
            h.exportable,
            h.create,
            h.assets,
            ProjectOrderLimits::default(),
        )
        .unwrap();
        assert_eq!(planned.collections, vec!['a', 'b']);
        assert_eq!(planned.exportable_count, 1);
        assert_eq!(planned.progress_steps(), vec![(0, 2), (1, 2)]);
    }

    #[test]
    fn outcome_classification_matches_the_warning_behavior() {
        assert_eq!(
            classify_collection_outcome(false, None),
            Ok(CollectionExportOutcome::SkippedNotExportable)
        );
        assert_eq!(
            classify_collection_outcome(true, Some(true)),
            Ok(CollectionExportOutcome::Succeeded)
        );
        assert_eq!(
            classify_collection_outcome(true, Some(false)),
            Ok(CollectionExportOutcome::Failed)
        );
        assert_eq!(
            classify_collection_outcome(false, Some(true)),
            Err(ProjectOrderError::InconsistentGateAndResult)
        );
        assert_eq!(
            classify_collection_outcome(true, None),
            Err(ProjectOrderError::InconsistentGateAndResult)
        );
    }

    #[test]
    fn planner_enforces_collection_and_queued_asset_bounds() {
        let groups: HashMap<char, (Vec<char>, bool)> =
            [('a', (vec!['a'], true)), ('b', (vec!['b'], true))]
                .into_iter()
                .collect();
        let h = harness(groups);
        assert_eq!(
            plan_project_export_order(
                &['a', 'b'],
                h.exportable,
                h.create,
                h.assets,
                ProjectOrderLimits {
                    max_queued_assets: 10_000_000,
                    max_collections: 1,
                }
            ),
            Err(ProjectOrderError::CollectionLimitExceeded { limit: 1 })
        );

        let groups: HashMap<char, (Vec<char>, bool)> =
            [('a', (vec!['a', 'b'], true))].into_iter().collect();
        let h = harness(groups);
        assert_eq!(
            plan_project_export_order(
                &['a'],
                h.exportable,
                h.create,
                h.assets,
                ProjectOrderLimits {
                    max_queued_assets: 1,
                    max_collections: 10_000_000,
                }
            ),
            Err(ProjectOrderError::QueuedAssetLimitExceeded { limit: 1 })
        );
    }

    #[test]
    fn unhandled_assets_abort_planning_without_a_partial_plan() {
        use std::cell::Cell;
        let created = Rc::new(Cell::new(0));
        let counted = Rc::clone(&created);
        let planned = plan_project_export_order(
            &['a', 'b', 'c'],
            |_: &char| true,
            move |asset: &char| {
                counted.set(counted.get() + 1);
                if *asset == 'b' {
                    return Err(ProjectOrderError::UnhandledAsset);
                }
                Ok(*asset)
            },
            |asset: &char| vec![*asset],
            ProjectOrderLimits::default(),
        );
        assert_eq!(planned, Err(ProjectOrderError::UnhandledAsset));
        // 'a' created its collection, 'b' threw, and 'c' was never attempted.
        assert_eq!(created.get(), 2);
        assert_eq!(
            ProjectOrderError::UnhandledAsset.to_string(),
            "there is no exporter that can handle the asset"
        );
    }

    #[test]
    fn preparation_events_bracket_successful_planning_only() {
        let groups: HashMap<char, (Vec<char>, bool)> =
            [('a', (vec!['a'], true))].into_iter().collect();
        let h = harness(groups);
        let (events, planned) = plan_project_export_with_events(
            &['a'],
            h.exportable,
            h.create,
            h.assets,
            ProjectOrderLimits::default(),
        );
        assert_eq!(
            events,
            vec![
                ProjectExportEvent::PreparationStarted,
                ProjectExportEvent::PreparationFinished,
            ]
        );
        assert_eq!(planned.unwrap().collections, vec!['a']);

        let (events, planned) = plan_project_export_with_events(
            &['a', 'b'],
            |_: &char| true,
            |asset: &char| {
                if *asset == 'b' {
                    return Err(ProjectOrderError::UnhandledAsset);
                }
                Ok(*asset)
            },
            |asset: &char| vec![*asset],
            ProjectOrderLimits::default(),
        );
        // The source throws out of CreateCollections before PreparationFinished.
        assert_eq!(events, vec![ProjectExportEvent::PreparationStarted]);
        assert_eq!(planned, Err(ProjectOrderError::UnhandledAsset));
    }

    #[test]
    fn export_loop_events_warnings_and_skips_follow_the_source() {
        let groups: HashMap<char, (Vec<char>, bool)> = [
            ('a', (vec!['a'], true)),
            ('b', (vec!['b'], false)),
            ('c', (vec!['c'], true)),
        ]
        .into_iter()
        .collect();
        let h = harness(groups);
        let planned = plan_project_export_order(
            &['a', 'b', 'c'],
            h.exportable,
            h.create,
            h.assets,
            ProjectOrderLimits::default(),
        )
        .unwrap();
        let run = run_project_export_loop(
            &planned,
            |collection: &char| *collection != 'b',
            |index: usize, _: &char| index != 2,
        );
        // 'a' exports, 'b' is skipped silently, 'c' fails with a warning index.
        assert_eq!(run.failed_collections, vec![2]);
        assert_eq!(
            run.events,
            vec![
                ProjectExportEvent::ExportStarted,
                ProjectExportEvent::ProgressUpdated { index: 0, total: 3 },
                ProjectExportEvent::ProgressUpdated { index: 1, total: 3 },
                ProjectExportEvent::ProgressUpdated { index: 2, total: 3 },
                ProjectExportEvent::ExportFinished,
            ]
        );
    }

    #[test]
    fn version_listing_dedups_in_first_seen_order() {
        assert_eq!(distinct_collection_versions(&[]), "");
        assert_eq!(
            distinct_collection_versions(&["2021.3.0f1", "2019.4.31f1", "2021.3.0f1"]),
            "2021.3.0f1 2019.4.31f1"
        );
        assert_eq!(
            distinct_collection_versions(&["5.6.0f1", "5.6.0f1"]),
            "5.6.0f1"
        );
    }

    #[test]
    fn settings_identity_mismatches_are_explicit_errors() {
        assert_eq!(throw_if_settings_dont_match(true), Ok(()));
        assert_eq!(
            throw_if_settings_dont_match(false),
            Err(ProjectOrderError::SettingsMismatch)
        );
        assert_eq!(
            ProjectOrderError::SettingsMismatch.to_string(),
            "export handler settings don't match"
        );
    }

    #[test]
    fn failure_warnings_name_the_collection_and_its_type() {
        assert_eq!(
            format_export_failure("Main", "TextureExportCollection"),
            "Failed to export 'Main' (TextureExportCollection)"
        );
    }

    /// End-to-end project-export execution over synthetic collections: the planned
    /// loop drives the real asset-plus-`.meta` step into a fresh temporary project
    /// root, proving the plan/loop/write composition writes exactly the exportable
    /// successes, records the failure warning index, never invokes the skipped
    /// collection, and emits the full lifecycle event order. Collection creation
    /// runs through the real handler trial order (last registration wins).
    #[test]
    fn export_loop_writes_a_real_project_tree_through_asset_and_meta_steps() {
        use crate::export::meta::UnityMetaFileOptions;
        use crate::export::project_output::{
            export_asset_with_meta, AssetExportStep, AssetExportWithMetaPlan,
            UnityProjectOutputLimits,
        };
        use crate::export::unity_yaml::{
            UnityYamlMappingStyle, UnityYamlNode, UnityYamlWriteLimits,
        };
        use std::cell::RefCell;
        use std::fs;
        use std::sync::atomic::{AtomicU64, Ordering};

        struct FakeCollection {
            name: &'static str,
            exportable: bool,
            relative_path: &'static str,
            guid: [u8; 16],
            bytes: &'static [u8],
            fail: bool,
        }

        struct TestRoot(std::path::PathBuf);
        impl TestRoot {
            fn new() -> Self {
                static NEXT_ID: AtomicU64 = AtomicU64::new(0);
                let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
                let path = std::env::temp_dir().join(format!(
                    "assetripper-project-loop-{}-{id}",
                    std::process::id()
                ));
                fs::create_dir(&path).unwrap();
                Self(path)
            }
        }
        impl Drop for TestRoot {
            fn drop(&mut self) {
                fs::remove_dir_all(&self.0).unwrap();
            }
        }

        fn plan_for(collection: &FakeCollection) -> AssetExportWithMetaPlan {
            AssetExportWithMetaPlan {
                relative_asset_path: collection.relative_path.to_owned(),
                guid: collection.guid,
                importer_class: "DefaultImporter".to_owned(),
                importer: UnityYamlNode::mapping(UnityYamlMappingStyle::Block),
                meta_options: UnityMetaFileOptions {
                    has_license_data: false,
                    ..Default::default()
                },
            }
        }

        let collections = [
            FakeCollection {
                name: "First",
                exportable: true,
                relative_path: "Assets/First.txt",
                guid: [17; 16],
                bytes: b"first-bytes",
                fail: false,
            },
            FakeCollection {
                name: "Second",
                exportable: true,
                relative_path: "Assets/Second.txt",
                guid: [34; 16],
                bytes: b"second-bytes",
                fail: true,
            },
            FakeCollection {
                name: "Skipped",
                exportable: false,
                relative_path: "Assets/Skipped.txt",
                guid: [51; 16],
                bytes: b"skipped-bytes",
                fail: false,
            },
        ];
        let assets = [0usize, 1, 2];
        // Two handlers stay registered for asset 0; the real trial order tries the
        // last registration first, so the winning exporter is `new-handler`.
        let winners: Rc<RefCell<Vec<&'static str>>> = Rc::new(RefCell::new(Vec::new()));
        let planned = plan_project_export_order(
            &assets,
            |asset: &usize| collections[*asset].exportable,
            |asset: &usize| {
                use crate::export::exporter_match::handler_try_order;
                let registered: &[&'static str] = match asset {
                    0 => &["old-handler", "new-handler"],
                    _ => &["only-handler"],
                };
                let winner = handler_try_order(registered)[0];
                winners.borrow_mut().push(winner);
                Ok(*asset)
            },
            |asset: &usize| vec![*asset],
            ProjectOrderLimits::default(),
        )
        .unwrap();
        assert_eq!(planned.collections, vec![0, 1, 2]);
        assert_eq!(planned.exportable_count, 2);
        assert_eq!(
            *winners.borrow(),
            vec!["new-handler", "only-handler", "only-handler"]
        );

        let root = TestRoot::new();
        let called: Rc<RefCell<Vec<usize>>> = Rc::new(RefCell::new(Vec::new()));
        let run = run_project_export_loop(
            &planned,
            |asset: &usize| collections[*asset].exportable,
            |index: usize, asset: &usize| {
                called.borrow_mut().push(index);
                let collection = &collections[*asset];
                let bytes = collection.bytes;
                match export_asset_with_meta(
                    &root.0,
                    plan_for(collection),
                    !collection.fail,
                    |path| {
                        fs::write(path, bytes)
                            .map(|()| bytes.len())
                            .map_err(|error| {
                                std::io::Error::new(error.kind(), "synthetic asset writer")
                            })
                    },
                    UnityProjectOutputLimits::default(),
                    UnityYamlWriteLimits::default(),
                ) {
                    Ok(AssetExportStep::Exported { .. }) => true,
                    Ok(AssetExportStep::SkippedFailedExport) => false,
                    Err(error) => panic!("synthetic export step failed: {error}"),
                }
            },
        );
        // The skipped collection is never invoked; the failed one records its warning index.
        assert_eq!(*called.borrow(), vec![0, 1]);
        assert_eq!(run.failed_collections, vec![1]);
        assert_eq!(
            run.events,
            vec![
                ProjectExportEvent::ExportStarted,
                ProjectExportEvent::ProgressUpdated { index: 0, total: 3 },
                ProjectExportEvent::ProgressUpdated { index: 1, total: 3 },
                ProjectExportEvent::ProgressUpdated { index: 2, total: 3 },
                ProjectExportEvent::ExportFinished,
            ]
        );
        // The success wrote asset bytes plus the GUID sidecar; the failure wrote
        // neither (its empty directory stays behind like the source order); the
        // skipped collection left no trace at all.
        assert_eq!(
            fs::read(root.0.join("Assets/First.txt")).unwrap(),
            b"first-bytes"
        );
        let sidecar = fs::read_to_string(root.0.join("Assets/First.txt.meta")).unwrap();
        assert!(
            sidecar.contains("guid: 11111111111111111111111111111111"),
            "{sidecar}"
        );
        assert!(!root.0.join("Assets/Second.txt").exists());
        assert!(!root.0.join("Assets/Second.txt.meta").exists());
        assert!(root.0.join("Assets").exists());
        assert!(!root.0.join("Assets/Skipped.txt").exists());
        assert!(!root.0.join("Assets/Skipped.txt.meta").exists());
        let _ = collections[0].name;
    }
}
