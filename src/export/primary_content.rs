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
use super::project_exporter::{class_satisfies_target, ObjectClassFacts};

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

/// `ObjectHandlerStack.GetHandlerStack`: the handlers to try for one asset type, in the order the
/// exporter tries them.
///
/// **The order is the reverse of the registration order**, and that is the single most important
/// fact about this table. `OverrideHandler` appends to a list, `CalculateAssetHandlerStack` pushes
/// each match onto a `Stack<T>` while walking that list forwards, and a stack enumerates
/// most-recently-pushed first - so the **last** matching registration is tried **first**. Measured,
/// not read: `tools/probes/handler-stack` registers `string` handlers on the public,
/// asset-independent `ObjectHandlerStack<string>` and observes `three,two,one` for three
/// registrations of the same type. The rows are replayed by `handler_stack_matches_the_runtime_rows`
/// below.
///
/// Consequences for the default stack, both of which are easy to invert by accident:
///
/// - `DeletedAssetsExporter` is registered **last** for `IUnityObjectBase`, so it is tried **first**
///   for every asset, and `JsonContentExtractor` - registered first - is the **final fallback**. That
///   is why JSON covers everything the others decline.
/// - `GlbModelExporter` is registered after the empty `IComponent` handler, so it shadows it.
///
/// `applies` is the caller's `type == baseType || allowInheritance && type.IsAssignableTo(baseType)`
/// test. It is a predicate rather than a type relation because the port models asset types as names,
/// and every registration in the default stack uses the default `allowInheritance = true`, so the
/// inheritance branch is the only one the default stack exercises - which is also why the row type
/// carries no such flag.
pub fn handler_stack_from(
    registrations: &[PrimaryHandlerRegistration],
    mut applies: impl FnMut(&PrimaryHandlerRegistration) -> bool,
) -> Vec<&PrimaryHandlerRegistration> {
    let mut stack: Vec<&PrimaryHandlerRegistration> =
        registrations.iter().filter(|r| applies(r)).collect();
    // `Stack<T>.Push` in registration order, enumerated - so reverse the filtered list.
    stack.reverse();
    stack
}

/// [`handler_stack_from`] over the default table, for a queried type name and an assignability test.
///
/// `is_assignable(target)` answers `queried.IsAssignableTo(target)`. A registration always applies to
/// its own exact target, which is the source's first disjunct.
pub fn handler_stack_for(
    queried: &str,
    mut is_assignable: impl FnMut(&str) -> bool,
) -> Vec<&'static PrimaryHandlerRegistration> {
    handler_stack_from(PRIMARY_HANDLERS, |registration| {
        registration.target == queried || is_assignable(registration.target)
    })
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
/// The caller resolves the two questions the source answers by type tests: which collection the
/// selected extractor would build, and what that collection is called. Keeping both as caller
/// facts lets the assembly loop be exercised without building a whole game. *Which* extractor
/// claims the asset is not a caller fact - [`create_primary_content_collection`] decides that by
/// walking the handler stack, and its failure is the source's thrown exception.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrimaryContentAsset<'a> {
    /// A stable identity for the `queued` set, which the source keys by asset reference.
    pub identity: u64,
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
    ///
    /// This is deliberately the **unsanitized** name, matching `SingleExportCollection.Name`,
    /// which returns `Asset.GetBestName()` raw while the *filename* goes through
    /// `FixInvalidFileNameCharacters`. Checked rather than assumed benign: the only consumers of
    /// `Name` are the progress and warning log lines in `PrimaryContentExporter.Export`, so a raw
    /// name can appear in a log message but can never reach a path. That asymmetry is therefore
    /// **not** a defect, unlike the class-name one recorded in
    /// `DIFF-PRIMARY-CONTENT-CLASSNAME-001` - do not "fix" this one.
    pub name: String,
}

impl PrimaryContentCollection {
    /// `ExportCollectionBase.Name`.
    pub fn collection_name(&self) -> String {
        self.name.clone()
    }

    /// `MultipleExportCollection.Contains` / `SingleExportCollection.Contains`.
    ///
    /// Membership is by **asset identity** (`AssetInfo` is `(Collection, PathID, ClassID)`, a
    /// record struct, so equality is structural), never by name. Two assets in different
    /// collections can share a name and remain distinct, and a multi-asset collection claims its
    /// primary plus every secondary it was given.
    pub fn contains(&self, identity: u64) -> bool {
        self.assets.contains(&identity)
    }

    /// `MultipleExportCollection.AddAsset`.
    ///
    /// The primary asset is never added as a secondary: the source tests
    /// `!ReferenceEquals(asset, Asset)` and skips it, because `Assets` already yields the primary
    /// first. Without that guard the primary would be written twice. Returns true when the asset
    /// was actually appended.
    pub fn add_asset(&mut self, identity: u64) -> bool {
        if identity == self.primary || self.assets.contains(&identity) {
            return false;
        }
        self.assets.push(identity);
        true
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
///
/// `resolve` is the caller's `CreateCollection`: given an asset it returns the extractor that
/// claimed it, or the error the source throws when the stack is exhausted. Passing it in keeps the
/// type tests and the collection construction - which need a whole game - outside this module,
/// while the ordering rules above stay testable.
pub fn create_primary_content_collections<'a>(
    assets: &[PrimaryContentAsset<'_>],
    mut resolve: impl FnMut(
        &PrimaryContentAsset<'_>,
    ) -> Result<&'a PrimaryHandlerRegistration, PrimaryContentNoHandler>,
) -> Result<Vec<PrimaryContentCollection>, PrimaryContentNoHandler> {
    let mut queued: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut collections = Vec::new();
    for asset in assets {
        // `queued.Add` runs first: a duplicate is skipped even when the earlier visit produced an
        // empty collection.
        if !queued.insert(asset.identity) {
            continue;
        }
        let registration = resolve(asset)?;
        if asset.is_empty_collection {
            // The empty check comes after the queue insert and skips the member queueing.
            continue;
        }
        let members: Vec<u64> = asset.collection_assets.to_vec();
        for member in &members {
            queued.insert(*member);
        }
        collections.push(PrimaryContentCollection {
            extractor: registration.extractor,
            primary: asset.identity,
            assets: members,
            name: asset.collection_name.to_owned(),
        });
    }
    Ok(collections)
}

/// `CreateCollection` failed to find a handler for the asset.
///
/// The source throws `There is no exporter that can handle '<asset>'`. It is unreachable through
/// the default registration because `JsonContentExtractor` is the **last** entry in stack order -
/// it is registered first, and the stack runs in reverse registration order - so it is the final
/// fallback after every other handler has declined. The port keeps the failure loud rather than
/// inventing a fallback: a silently-dropped asset would be a lost resource.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrimaryContentNoHandler {
    /// A stable identity for the asset, matching [`PrimaryContentAsset::identity`].
    pub identity: u64,
}

/// What one extractor's `TryCreateCollection` decided for an object.
///
/// The reference separates two facts that are easy to conflate: whether an extractor **claimed** the
/// asset, and whether the collection it built is the shared empty one. The empty handlers claim
/// every asset they are reached for, so "claimed" alone cannot decide whether the asset is dropped.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrimaryContentClaim {
    /// The extractor declined; the stack continues to the next handler.
    Declined,
    /// The extractor claimed into `EmptyExportCollection`, so the asset is queued and then dropped.
    Empty,
    /// The extractor claimed into a real collection.
    Claimed,
}

/// One workspace object presented to the primary-content pipeline.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrimaryContentObject<'a> {
    /// A stable identity for the `queued` set.
    pub identity: u64,
    /// The generated-type facts that decide which registration targets this object satisfies.
    pub facts: ObjectClassFacts,
    /// `ExportCollectionBase.Name` for the collection this object creates.
    pub collection_name: &'a str,
    /// Every asset the created collection would claim, this object first.
    pub collection_assets: &'a [u64],
}

/// `ObjectHandlerStack.GetHandlerStack` for one workspace object, in stack order.
///
/// The reference matches on the runtime type with `type == baseType || allowInheritance &&
/// type.IsAssignableTo(baseType)`. [`class_satisfies_target`] already answers both disjuncts for a
/// class id, including the `TypeTreeObject`, `UnknownObject` and `UnreadableObject` fallbacks that a
/// generated-interface check alone would miss, so it *is* the predicate - there is no second rule to
/// write here.
pub fn handler_stack_for_object(
    facts: ObjectClassFacts,
) -> Vec<&'static PrimaryHandlerRegistration> {
    handler_stack_from(PRIMARY_HANDLERS, |registration| {
        class_satisfies_target(facts, registration.target)
    })
}

/// `PrimaryContentExporter.CreateCollections` over workspace objects.
///
/// This is [`create_primary_content_collections`] with the stack derived per object instead of
/// supplied: the caller answers only the extractor's own `TryCreateCollection`, which is the part
/// that needs decoded asset data. The ordering rules are the same ones that function documents - the
/// asset enters `queued` **before** the empty check, a non-empty collection queues all its members,
/// and an exhausted stack is an error rather than a silent skip.
pub fn create_primary_content_collections_for_objects(
    objects: &[PrimaryContentObject<'_>],
    mut claim: impl FnMut(&PrimaryHandlerRegistration, &PrimaryContentObject<'_>) -> PrimaryContentClaim,
) -> Result<Vec<PrimaryContentCollection>, PrimaryContentNoHandler> {
    let mut queued: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut collections = Vec::new();
    for object in objects {
        if !queued.insert(object.identity) {
            continue;
        }
        let stack = handler_stack_for_object(object.facts);
        let mut selected = None;
        let mut is_empty = false;
        for registration in &stack {
            match claim(registration, object) {
                PrimaryContentClaim::Declined => continue,
                PrimaryContentClaim::Empty => {
                    selected = Some(*registration);
                    is_empty = true;
                    break;
                }
                PrimaryContentClaim::Claimed => {
                    selected = Some(*registration);
                    break;
                }
            }
        }
        let Some(registration) = selected else {
            return Err(PrimaryContentNoHandler {
                identity: object.identity,
            });
        };
        if is_empty {
            // Queued above, members deliberately not queued.
            continue;
        }
        for member in object.collection_assets {
            queued.insert(*member);
        }
        collections.push(PrimaryContentCollection {
            extractor: registration.extractor,
            primary: object.identity,
            assets: object.collection_assets.to_vec(),
            name: object.collection_name.to_owned(),
        });
    }
    Ok(collections)
}

impl core::fmt::Display for PrimaryContentNoHandler {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            formatter,
            "There is no exporter that can handle asset {}",
            self.identity
        )
    }
}

impl std::error::Error for PrimaryContentNoHandler {}

/// The handler stack's answer for one asset: the first extractor that claims it.
///
/// `candidates` must already be in **stack order** - the reverse of the registration order, which
/// [`handler_stack_for`] produces. Passing the table in source order inverts every priority: the
/// catch-all JSON extractor would claim every asset and the deleted-assets and model extractors would
/// never be reached. This function cannot detect that, because a caller may legitimately hand it a
/// narrowed stack, so the requirement is stated here and pinned by
/// `default_stack_puts_deleted_first_and_json_last`.
///
/// `CreateCollection` takes the first candidate whose `TryCreateCollection` succeeds.
pub fn select_primary_content_extractor<'a>(
    candidates: &[&'a PrimaryHandlerRegistration],
    mut accepts: impl FnMut(&PrimaryHandlerRegistration) -> bool,
) -> Option<&'a PrimaryHandlerRegistration> {
    candidates
        .iter()
        .copied()
        .find(|registration| accepts(registration))
}

/// `PrimaryContentExporter.CreateCollection` for one asset.
///
/// Walks `candidates` in stack order - see [`select_primary_content_extractor`], which is where the
/// reverse-registration requirement is explained - and returns the first handler whose
/// `TryCreateCollection` reports success, or the error the source throws when every handler declined.
///
/// The failure is deliberately an error rather than a fallback. Through the default registration the
/// stack can never be exhausted, because `JsonContentExtractor` accepts every asset and is the last
/// entry in stack order - but a port that substituted a default collection here would silently drop a
/// resource the moment a caller narrowed the stack, which is the class of bug this whole row is meant
/// to rule out.
pub fn create_primary_content_collection<'a>(
    identity: u64,
    candidates: &[&'a PrimaryHandlerRegistration],
    mut try_create_collection: impl FnMut(&PrimaryHandlerRegistration) -> bool,
) -> Result<&'a PrimaryHandlerRegistration, PrimaryContentNoHandler> {
    select_primary_content_extractor(candidates, &mut try_create_collection)
        .ok_or(PrimaryContentNoHandler { identity })
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
    // `ClassName` is sanitized on **both** branches, which is a deliberate divergence from the
    // frozen source. The source applies `FixInvalidFileNameCharacters` only when the best name was
    // non-empty, and falls back to `asset.ClassName` verbatim otherwise. That asymmetry is
    // reachable and unsafe: for a `TypeTreeObject` the class name is `EditorFields.Type.Name`,
    // i.e. the TypeTree's own `m_Type` string, and the baseline's own `ValidNameGenerator`
    // demonstrates the shape of those names with the regex `[<>\[\]\s&\(\):\.-]` - so they can
    // contain `[`, `]`, `:`, or spaces, each of which is an invalid file-name character. Unity
    // assets whose class name contains a path separator would escape the export directory.
    // Sanitizing here cannot reject a valid name: the transform is idempotent over names that
    // contain no invalid character. See `DIFF-PRIMARY-CONTENT-CLASSNAME-001`.
    let mut file_name = if trimmed.is_empty() {
        fix_invalid_file_name_characters(&identity.class_name, platform)?
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
        create_primary_content_collection, create_primary_content_collections,
        create_primary_content_collections_for_objects, format_export_progress, handler_stack_for,
        handler_stack_for_object, handler_stack_from, handlers_for_target,
        plan_single_export_paths, primary_content_progress_total, run_primary_content_export_loop,
        select_primary_content_extractor, PrimaryContentAsset, PrimaryContentClaim,
        PrimaryContentExportEvent, PrimaryContentExportStep, PrimaryContentNoHandler,
        PrimaryContentObject, PrimaryHandlerRegistration, PRIMARY_HANDLERS,
    };
    use crate::export::asset_path::{
        join_path, ExportAssetPathIdentity, ExportFileSystem, HostPathPlatform, UniqueNameState,
        MAX_FILE_NAME_LENGTH,
    };
    use crate::export::project_exporter::ObjectClassFacts;

    /// A collection that claims only `identity`, which is what the single-asset extractors do.
    ///
    /// The name is explicit so a test that asserts on a progress line says what it expects instead
    /// of depending on a naming rule hidden in this helper.
    fn solo(identity: u64, name: &'static str) -> PrimaryContentAsset<'static> {
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
            is_empty_collection: false,
            collection_assets: members,
            collection_name: name,
        }
    }

    /// A resolver that always claims with `JsonContentExtractor`, which is the catch-all the
    /// default registration puts first.
    fn catch_all(
        _asset: &PrimaryContentAsset<'_>,
    ) -> Result<&'static PrimaryHandlerRegistration, PrimaryContentNoHandler> {
        handlers_for_target("IUnityObjectBase")
            .next()
            .ok_or(PrimaryContentNoHandler { identity: 0 })
    }

    /// The assembly loop's two ordering rules, which are easy to get backwards.
    #[test]
    fn primary_content_assembly_queues_before_the_empty_check_and_queues_members() {
        // 1. A duplicate asset is skipped even though the earlier visit produced an empty
        //    collection, because `queued.Add` runs *before* the empty check.
        let empty_then_retry = [
            PrimaryContentAsset {
                identity: 7,
                is_empty_collection: true,
                collection_assets: &[7],
                collection_name: "Empty",
            },
            solo(7, "Seven"),
        ];
        assert!(
            create_primary_content_collections(&empty_then_retry, catch_all)
                .unwrap()
                .is_empty(),
            "an asset queued by an empty collection is not reconsidered"
        );

        // 2. A non-empty collection queues ALL its members, so a later asset already claimed is
        //    not exported a second time.
        let members = [1_u64, 2, 3];
        let claiming = [
            PrimaryContentAsset {
                identity: 1,
                is_empty_collection: false,
                collection_assets: &members,
                collection_name: "Model",
            },
            solo(2, "Two"),
            solo(3, "Three"),
            solo(4, "Four"),
        ];
        let collections = create_primary_content_collections(&claiming, catch_all).unwrap();
        let primaries: Vec<u64> = collections
            .iter()
            .map(|collection| collection.primary)
            .collect();
        assert_eq!(primaries, [1, 4], "members 2 and 3 were already claimed");
        assert_eq!(collections[0].assets, members.to_vec());
        assert_eq!(collections[0].extractor, "JsonContentExtractor");
        // The progress total counts all collections, which is the primary-content delta.
        assert_eq!(primary_content_progress_total(&collections), 2);
    }

    /// `CreateCollection` reports the source's exception when the stack is exhausted.
    #[test]
    fn primary_content_create_collection_reports_the_source_exception() {
        let stack: Vec<&PrimaryHandlerRegistration> =
            handlers_for_target("IUnityObjectBase").collect();
        // A handler that claims the asset.
        let selected = create_primary_content_collection(4, &stack, |registration| {
            registration.extractor == "DeletedAssetsExporter"
        })
        .expect("the last handler accepts");
        assert_eq!(selected.extractor, "DeletedAssetsExporter");
        // A stack where nothing claims it: the message is the source's own text.
        let error = create_primary_content_collection(4, &stack, |_| false).unwrap_err();
        assert_eq!(error.identity, 4);
        assert_eq!(
            error.to_string(),
            "There is no exporter that can handle asset 4"
        );
        // An empty stack is the same failure, not a panic.
        let empty: [&PrimaryHandlerRegistration; 0] = [];
        assert!(create_primary_content_collection(4, &empty, |_| true).is_err());
    }

    /// An exhausted handler stack is the source's thrown exception, not a skipped asset.
    #[test]
    fn primary_content_assembly_fails_loudly_when_no_handler_claims_an_asset() {
        let assets = [PrimaryContentAsset {
            identity: 9,
            is_empty_collection: false,
            collection_assets: &[9],
            collection_name: "Unclaimed",
        }];
        let error = create_primary_content_collections(&assets, |_| {
            Err(PrimaryContentNoHandler { identity: 9 })
        })
        .unwrap_err();
        assert_eq!(error.identity, 9);
        assert_eq!(
            error.to_string(),
            "There is no exporter that can handle asset 9"
        );
        // A duplicate is skipped before dispatch, so a stack that would decline is never consulted
        // for an asset an earlier collection already claimed.
        let duplicate_then_unclaimed = [
            PrimaryContentAsset {
                identity: 9,
                is_empty_collection: false,
                collection_assets: &[9],
                collection_name: "First",
            },
            PrimaryContentAsset {
                identity: 9,
                is_empty_collection: false,
                collection_assets: &[9],
                collection_name: "Second",
            },
        ];
        let resolved = create_primary_content_collections(&duplicate_then_unclaimed, catch_all)
            .expect("the duplicate never reaches the resolver twice");
        assert_eq!(resolved.len(), 1);
    }

    /// `CreateCollection` takes the **first** handler that accepts, in stack order.
    #[test]
    fn primary_content_dispatch_takes_the_first_accepting_handler() {
        let stack: Vec<&PrimaryHandlerRegistration> =
            handlers_for_target("IUnityObjectBase").collect();
        // Exactly two handlers claim the root interface: the JSON catch-all registered first and
        // `DeletedAssetsExporter` registered last.
        assert_eq!(
            stack
                .iter()
                .map(|registration| registration.extractor)
                .collect::<Vec<_>>(),
            ["JsonContentExtractor", "DeletedAssetsExporter"],
            "the IUnityObjectBase stack is the source's first and last registration"
        );

        // Accepting the first yields the catch-all, and the walk stops there.
        let mut seen = 0;
        let selected = select_primary_content_extractor(&stack, |_| {
            seen += 1;
            true
        })
        .expect("the first handler accepts");
        assert_eq!(selected.extractor, "JsonContentExtractor");
        assert_eq!(seen, 1, "the walk stops at the first accepting handler");

        // Declining the first falls through to the second.
        let selected = select_primary_content_extractor(&stack, |registration| {
            registration.extractor == "DeletedAssetsExporter"
        })
        .expect("the last handler accepts");
        assert_eq!(selected.extractor, "DeletedAssetsExporter");

        // An entirely declining stack yields no selection rather than a default.
        assert!(select_primary_content_extractor(&stack, |_| false).is_none());
    }

    /// An identity whose only name is its class name, matching an asset with no name of its own.
    fn class_named(class_name: &str) -> ExportAssetPathIdentity {
        ExportAssetPathIdentity {
            class_name: class_name.to_owned(),
            ..Default::default()
        }
    }

    /// Membership is by identity, and the primary is never appended as a secondary.
    #[test]
    fn primary_content_collection_membership_is_by_identity() {
        let collections =
            create_primary_content_collections(&[solo(7, "Seven")], catch_all).unwrap();
        let mut collection = collections.into_iter().next().expect("one collection");
        assert_eq!(collection.primary, 7);
        assert_eq!(collection.assets, [7]);
        assert!(collection.contains(7));
        assert!(!collection.contains(8));

        // A secondary is appended exactly once, and the primary is never appended at all - it is
        // already the first member, so letting it in would export it twice.
        assert!(!collection.add_asset(7), "the primary is not a secondary");
        assert!(collection.add_asset(8));
        assert!(
            !collection.add_asset(8),
            "an existing secondary is not re-added"
        );
        assert_eq!(collection.assets, [7, 8], "the primary stays first");
        assert!(collection.contains(8));
        // Membership is by identity even when every collection shares a display name: two assets
        // in different collections stay distinct.
        let same_name =
            create_primary_content_collections(&[solo(2, "Shared"), solo(3, "Shared")], catch_all)
                .unwrap();
        assert_eq!(same_name.len(), 2, "a shared name does not merge assets");
        assert_eq!(
            same_name[0].collection_name(),
            same_name[1].collection_name()
        );
        assert!(same_name[0].contains(2));
        assert!(!same_name[0].contains(3));
        assert!(same_name[1].contains(3));
    }

    /// A class-name fallback is sanitized, closing a path-escape the frozen source allows.
    ///
    /// The fallback branch is narrower than it first appears: `GetBestName` itself falls back to
    /// `ClassName` when no name is set, so the ordinary "asset has no name" case already takes the
    /// **sanitizing** branch with the class name as its input. The unsanitized branch is reachable
    /// only when a name is *present but whitespace-only* (`OverrideName` or `OriginalName`), since
    /// only then does `GetBestName` return that value and the trim leave nothing behind. That is
    /// still a real input, so the divergence is kept and pinned here.
    #[test]
    fn primary_content_class_name_fallback_is_sanitized() {
        let file_system = TestFileSystem::default();
        let mut state = UniqueNameState::default();
        // The class name takes the shape a TypeTree type name can take: the baseline's own
        // `ValidNameGenerator` regex `[<>\[\]\s&\(\):\.-]` shows that `[`, `]`, `:`, and spaces
        // are characters a type name may contain, and each is an invalid file-name character.
        let mut identity = class_named("Foo[Bar]:Baz");
        identity.override_name = Some("   ".to_owned());
        let planned = plan_single_export_paths(
            &identity,
            "glb",
            HostPathPlatform::Windows,
            &mut state,
            &file_system,
            "Export",
        )
        .expect("the sanitized name is still a valid path");
        assert_eq!(planned.file_name, "Foo_Bar__Baz.glb");
        assert_eq!(planned.file_path, "Export\\Assets\\Foo_Bar__Baz.glb");
        assert!(
            !planned.file_name.contains(':') && !planned.file_name.contains('['),
            "the fallback name carries no invalid file-name character: {}",
            planned.file_name
        );

        // A class name that is already valid is unaffected, so sanitizing cannot rename a
        // legitimate asset.
        let mut identity = class_named("MonoBehaviour");
        identity.override_name = Some("   ".to_owned());
        let planned = plan_single_export_paths(
            &identity,
            "glb",
            HostPathPlatform::Windows,
            &mut state,
            &file_system,
            "Export",
        )
        .expect("a plain class name is unchanged");
        assert_eq!(planned.file_name, "MonoBehaviour.glb");

        // And the ordinary unnamed asset already sanitizes, via `GetBestName`'s own fallback.
        let identity = class_named("Foo[Bar]:Baz");
        assert_eq!(identity.best_name(), "Foo[Bar]:Baz");
        let planned = plan_single_export_paths(
            &identity,
            "glb",
            HostPathPlatform::Windows,
            &mut state,
            &file_system,
            "Export",
        )
        .expect("the best-name branch sanitizes");
        assert_eq!(planned.file_name, "Foo_Bar__Baz.glb");
    }

    /// A primary asset is claimed by its own collection even when the member list omits it.
    #[test]
    fn primary_content_assembly_keeps_the_primary_asset() {
        let assets = [PrimaryContentAsset {
            identity: 5,
            is_empty_collection: false,
            collection_assets: &[5, 6],
            collection_name: "Audio",
        }];
        let collections = create_primary_content_collections(&assets, catch_all).unwrap();
        assert_eq!(collections.len(), 1);
        assert_eq!(collections[0].primary, 5);
        assert_eq!(collections[0].assets, [5, 6]);
        assert_eq!(collections[0].collection_name(), "Audio");
    }

    /// The export loop's progress counter spans **all** collections, not the exportable ones.
    #[test]
    fn primary_content_export_loop_indexes_over_every_collection() {
        let collections = create_primary_content_collections(
            &[
                solo(1, "One"),
                solo(2, "Two"),
                solo(3, "Three"),
                solo(4, "Four"),
            ],
            catch_all,
        )
        .unwrap();
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
            create_primary_content_collections(&[solo(7, "Seven")], catch_all).unwrap();
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
            create_primary_content_collections(&[solo(7, "Seven")], catch_all).unwrap();
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

    /// Replay the runtime's handler-stack order.
    ///
    /// The rows come from `tools/probes/handler-stack`, which measures the real
    /// `ObjectHandlerStack<string>`. The port's own documentation claimed the stack arrived in
    /// **registration** order; it arrives in the reverse, and the difference decides every asset's
    /// extractor. The fixture is what makes that reversal evidence rather than a reading of
    /// `Stack<T>`.
    #[test]
    fn handler_stack_matches_the_runtime_rows() {
        let fixture = include_str!("handler_stack_fixture.txt");
        let mut checked = 0usize;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let rest = line.strip_prefix("STACK|").expect("STACK row");
            let mut parts = rest.splitn(5, '|');
            let case = parts.next().expect("case");
            let queried = parts.next().expect("queried type");
            let registrations = parts.next().expect("registrations");
            let assignable = parts.next().expect("assignable targets");
            let expected = parts.next().expect("expected order");
            let assignable: Vec<&str> = if assignable.is_empty() {
                Vec::new()
            } else {
                assignable.split(',').collect()
            };

            // The probe registered `string` handlers on a real stack; this replay builds the same
            // registration list and runs it through the production filter-and-reverse.
            let mut no_inherit: Vec<&str> = Vec::new();
            let owned: Vec<PrimaryHandlerRegistration> = registrations
                .split(',')
                .map(|entry| {
                    let (target, handler) = entry.split_once(':').expect("target:handler");
                    // `@false` marks `allowInheritance = false`, which the default table never sets,
                    // so the replay carries it as a test-local set instead of a production field.
                    let target = match target.strip_suffix("@false") {
                        Some(target) => {
                            no_inherit.push(target);
                            target
                        }
                        None => target,
                    };
                    PrimaryHandlerRegistration {
                        target: Box::leak(target.to_owned().into_boxed_str()),
                        extractor: Box::leak(handler.to_owned().into_boxed_str()),
                        is_empty_handler: false,
                    }
                })
                .collect();

            let stack = handler_stack_from(&owned, |registration| {
                registration.target == queried
                    || (!no_inherit.contains(&registration.target)
                        && assignable.contains(&registration.target))
            });
            let actual: Vec<&str> = stack.iter().map(|r| r.extractor).collect();
            assert_eq!(
                actual,
                expected.split(',').collect::<Vec<_>>(),
                "case {case}"
            );
            checked += 1;
        }
        assert_eq!(checked, 9, "every recorded row is replayed");
    }

    /// The default stack's two priority facts, asserted directly.
    ///
    /// An asset assignable to every registered interface is the worst case for getting this
    /// backwards, so it is the case pinned: the deleted-assets tail is tried first and the JSON
    /// catch-all last.
    #[test]
    fn default_stack_puts_deleted_first_and_json_last() {
        let assignable_to_everything = |_target: &str| true;
        let stack = handler_stack_for("IUnityObjectBase", assignable_to_everything);
        assert_eq!(stack.len(), PRIMARY_HANDLERS.len(), "everything applies");
        assert_eq!(
            stack.first().unwrap().extractor,
            "DeletedAssetsExporter",
            "the last registration is tried first"
        );
        assert_eq!(
            stack.last().unwrap().extractor,
            "JsonContentExtractor",
            "the first registration is the final fallback"
        );

        // A component that is not a hierarchy member: the model exporter shadows the empty handler,
        // so it comes first even though it was registered second.
        let component = handler_stack_for("IComponent", |_| false);
        let extractors: Vec<&str> = component.iter().map(|r| r.extractor).collect();
        assert_eq!(extractors, ["GlbModelExporter", "EmptyContentExtractor"]);
    }

    /// The per-object stack, asserted against the real interface table.
    ///
    /// Four classes with different interface sets pin the reverse-registration rule and the
    /// inheritance filter together: a class only sees the handlers whose registered target it
    /// satisfies, and among those the last registered is first.
    #[test]
    fn object_stack_runs_in_reverse_registration_order() {
        fn extractors(facts: ObjectClassFacts) -> Vec<&'static str> {
            handler_stack_for_object(facts)
                .iter()
                .map(|registration| registration.extractor)
                .collect()
        }

        // Class 43 is `IMesh,IUnityObjectBase`.
        assert_eq!(
            extractors(ObjectClassFacts::new(43, false)),
            [
                "DeletedAssetsExporter",
                "GlbMeshExporter",
                "JsonContentExtractor"
            ]
        );
        // Class 1 is `IGameObject,IUnityObjectBase`, so the model exporter appears between them.
        assert_eq!(
            extractors(ObjectClassFacts::new(1, false)),
            [
                "DeletedAssetsExporter",
                "GlbModelExporter",
                "JsonContentExtractor"
            ]
        );
        // Class 115 is `IMonoScript,ITextAsset,IUnityObjectBase`: three different extractors, and
        // the binary handler is reached through `ITextAsset` rather than through the script one.
        assert_eq!(
            extractors(ObjectClassFacts::new(115, false)),
            [
                "DeletedAssetsExporter",
                "ScriptContentExtractor",
                "BinaryAssetContentExtractor",
                "JsonContentExtractor"
            ]
        );
        // Class 28 is `IImageTexture,ITexture,ITexture2D,IUnityObjectBase`.
        assert_eq!(
            extractors(ObjectClassFacts::new(28, false)),
            [
                "DeletedAssetsExporter",
                "TextureExporter",
                "JsonContentExtractor"
            ]
        );
    }

    /// Selection walks the stack and takes the first claimant, so a declining tail falls to JSON.
    #[test]
    fn selection_takes_the_first_claimant_in_stack_order() {
        let objects = [PrimaryContentObject {
            identity: 1,
            facts: ObjectClassFacts::new(43, false),
            collection_name: "Tri",
            collection_assets: &[1],
        }];

        // Every handler declines except the JSON fallback.
        let collections =
            create_primary_content_collections_for_objects(&objects, |registration, _| {
                if registration.extractor == "JsonContentExtractor" {
                    PrimaryContentClaim::Claimed
                } else {
                    PrimaryContentClaim::Declined
                }
            })
            .unwrap();
        assert_eq!(collections.len(), 1);
        assert_eq!(collections[0].extractor, "JsonContentExtractor");

        // The mesh handler claims first when it accepts, and JSON is never reached.
        let collections =
            create_primary_content_collections_for_objects(&objects, |registration, _| {
                match registration.extractor {
                    "GlbMeshExporter" => PrimaryContentClaim::Claimed,
                    "JsonContentExtractor" => PrimaryContentClaim::Claimed,
                    _ => PrimaryContentClaim::Declined,
                }
            })
            .unwrap();
        assert_eq!(collections[0].extractor, "GlbMeshExporter");
    }

    /// The empty-versus-non-empty queueing rule, contrasted on the same two objects.
    ///
    /// This is the documented difference from the Unity-project pipeline, and the two halves are the
    /// only way to see it: the *same* member list either queues its members or does not, purely
    /// depending on whether the claiming collection was the empty one.
    #[test]
    fn an_empty_claim_drops_the_collection_but_keeps_the_asset_queued() {
        let objects = [
            PrimaryContentObject {
                identity: 7,
                facts: ObjectClassFacts::new(115, false),
                collection_name: "Script",
                collection_assets: &[7, 8],
            },
            PrimaryContentObject {
                identity: 8,
                facts: ObjectClassFacts::new(43, false),
                collection_name: "Tri",
                collection_assets: &[8],
            },
        ];
        let json_claims = |registration: &PrimaryHandlerRegistration| {
            registration.extractor == "JsonContentExtractor"
        };

        // The script claim is the empty one: no collection is added, and member 8 is NOT queued,
        // so asset 8 is still visited and produces its own JSON collection.
        let collections =
            create_primary_content_collections_for_objects(&objects, |registration, _| {
                if registration.extractor == "ScriptContentExtractor" {
                    PrimaryContentClaim::Empty
                } else if json_claims(registration) {
                    PrimaryContentClaim::Claimed
                } else {
                    PrimaryContentClaim::Declined
                }
            })
            .unwrap();
        assert_eq!(collections.len(), 1, "the empty claim added no collection");
        assert_eq!(
            collections[0].primary, 8,
            "member 8 was not queued, so it was visited on its own"
        );

        // The same claim as a real collection: member 8 IS queued, so it is skipped entirely.
        let collections =
            create_primary_content_collections_for_objects(&objects, |registration, _| {
                if registration.extractor == "ScriptContentExtractor" || json_claims(registration) {
                    PrimaryContentClaim::Claimed
                } else {
                    PrimaryContentClaim::Declined
                }
            })
            .unwrap();
        assert_eq!(collections.len(), 1);
        assert_eq!(collections[0].primary, 7);
        assert_eq!(collections[0].assets, vec![7, 8]);
    }

    /// A non-empty collection queues every member, so a later visit is skipped.
    #[test]
    fn a_non_empty_collection_queues_all_of_its_members() {
        let objects = [
            PrimaryContentObject {
                identity: 1,
                facts: ObjectClassFacts::new(1, false),
                collection_name: "Scene",
                collection_assets: &[1, 2, 3],
            },
            PrimaryContentObject {
                identity: 2,
                facts: ObjectClassFacts::new(43, false),
                collection_name: "Tri",
                collection_assets: &[2],
            },
        ];
        let collections =
            create_primary_content_collections_for_objects(&objects, |registration, object| {
                if (object.identity == 1 && registration.extractor == "GlbModelExporter")
                    || registration.extractor == "JsonContentExtractor"
                {
                    PrimaryContentClaim::Claimed
                } else {
                    PrimaryContentClaim::Declined
                }
            })
            .unwrap();
        assert_eq!(
            collections.len(),
            1,
            "asset 2 was queued by asset 1's collection"
        );
        assert_eq!(collections[0].extractor, "GlbModelExporter");
        assert_eq!(collections[0].assets, vec![1, 2, 3]);
    }

    /// An exhausted stack is the source's thrown exception, not a skipped asset.
    #[test]
    fn an_exhausted_object_stack_is_an_error() {
        let objects = [PrimaryContentObject {
            identity: 42,
            facts: ObjectClassFacts::new(43, false),
            collection_name: "Tri",
            collection_assets: &[42],
        }];
        let error = create_primary_content_collections_for_objects(&objects, |_, _| {
            PrimaryContentClaim::Declined
        })
        .unwrap_err();
        assert_eq!(error.identity, 42);
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
