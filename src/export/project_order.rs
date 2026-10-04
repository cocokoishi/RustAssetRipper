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
//!    including skipped collections.
//!
//! Asset content, the exporter stack, and the event subscribers stay outside this module; only
//! the order, the queued-set rule, the export gate, and the progress steps are ported, over
//! caller-supplied asset and collection identities.

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

/// Mirror `CreateCollections`: walk fetched assets in order, open one collection per unqueued
/// asset, and queue every asset of each new collection.
pub fn plan_project_export_order<A, C>(
    fetch_assets: &[A],
    is_exportable: impl Fn(&C) -> bool,
    create_collection: impl Fn(&A) -> C,
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
        let collection = create_collection(asset);
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

#[cfg(test)]
mod tests {
    use super::{
        classify_collection_outcome, plan_project_export_order, CollectionExportOutcome,
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
    type AssetProjector = Box<dyn Fn(&char) -> char>;
    type CollectionMembers = Box<dyn Fn(&char) -> Vec<char>>;

    fn harness(groups: HashMap<char, (Vec<char>, bool)>) -> Harness {
        let groups = Rc::new(groups);
        let exportable_groups = Rc::clone(&groups);
        let assets_groups = Rc::clone(&groups);
        Harness {
            exportable: Box::new(move |collection: &char| exportable_groups[collection].1),
            create: Box::new(|asset: &char| *asset),
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
}
