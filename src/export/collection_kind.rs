//! Per-collection file-writing behavior of AssetRipper's Unity project exporter.
//!
//! Every rule here is ported from the frozen baseline's `Export` overrides and the
//! `ProjectExporter.Export` loop:
//!
//! - `AssetExportCollection<T>.Export` writes the asset through `ExportInner`, then writes a
//!   `.meta` sidecar for `Meta(GUID, CreateImporter(container))`. The file extension comes from
//!   the virtual `GetExportExtension`, which each subclass may override.
//! - `SceneExportCollection.ExportScene` writes the scene, then a `.meta` sidecar for
//!   `Meta(Scene.GUID, DefaultImporter)`. The extension is always `unity`.
//! - `PrefabExportCollection` overrides the extension to `prefab` and creates either a
//!   `NativeFormatImporter` (legacy prefab asset) or a `PrefabImporter`.
//! - `ManagerExportCollection.Export` writes `$"{name}.asset"` at a fixed path with no
//!   unique-name resolution and no `.meta` sidecar.
//! - `ScriptExportCollectionBase.OnScriptExported` writes a `.meta` sidecar for
//!   `Meta(ScriptHashing.CalculateScriptGuid(script), MonoImporter)` with a `.cs` file name.
//!   `ScriptExportCollection` skips the asset write when the file exists, while
//!   `EmptyScriptExportCollection` always overwrites it; both log an error instead of writing
//!   the sidecar when the `.meta` file already exists.
//! - `UnknownExportCollection.Export` and `UnreadableExportCollection.Export` write `.unknown`
//!   and `.unreadable` payloads under `AssetRipper/UnknownAssets` and
//!   `AssetRipper/UnreadableAssets` with unique-name resolution and no `.meta` sidecar.
//! - `FailExportCollection.Export` logs a warning and returns `false` without writing files.
//! - `SkipExportCollection`, `EmptyExportCollection`, `DeletedAssetsExportCollection`,
//!   `RedirectExportCollection`, and `SingleRedirectExportCollection` are not exportable:
//!   `ProjectExporter` skips them without calling `Export`.
//!
//! Class-specific importer field values and `GetExportExtension` mappings that depend on
//! generated marker interfaces are not ported here; they need the generated class schemas this
//! crate does not have yet. This module only records which collections write files, which write
//! sidecars, which extensions are fixed in source, and how file-name collisions are handled.

/// One family of `IExportCollection` implementations with shared file-writing behavior.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ExportCollectionKind {
    /// `AssetExportCollection<T>` and subclasses that keep its `Export` flow.
    Asset,
    /// `SceneExportCollection`.
    Scene,
    /// `PrefabExportCollection`.
    Prefab,
    /// `ManagerExportCollection`.
    Manager,
    /// `ScriptExportCollection`, which skips the `.cs` write when the file exists.
    Script,
    /// `EmptyScriptExportCollection`, which always overwrites the `.cs` file.
    EmptyScript,
    /// `UnknownExportCollection`.
    Unknown,
    /// `UnreadableExportCollection`.
    Unreadable,
    /// `FailExportCollection`.
    Fail,
    /// `SkipExportCollection`.
    Skip,
    /// `EmptyExportCollection`.
    Empty,
    /// `DeletedAssetsExportCollection`.
    Deleted,
    /// `RedirectExportCollection` and `SingleRedirectExportCollection`.
    Redirect,
}

/// How one collection family resolves the destination file name.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CollectionCollisionRule {
    /// `FileSystem.GetUniqueName` in the destination subdirectory.
    UniqueName,
    /// A fixed `Path.Join` destination with no unique-name resolution.
    FixedPath,
    /// The asset write is skipped when the file exists; the `.meta` write logs an error when
    /// the sidecar exists instead of overwriting it.
    SkipAssetIfExists,
    /// The asset file is always overwritten; the `.meta` write logs an error when the sidecar
    /// exists instead of overwriting it.
    OverwriteAsset,
    /// No files are written by this collection.
    NotExported,
}

impl ExportCollectionKind {
    /// All collection families known to this audit.
    pub const ALL: [Self; 13] = [
        Self::Asset,
        Self::Scene,
        Self::Prefab,
        Self::Manager,
        Self::Script,
        Self::EmptyScript,
        Self::Unknown,
        Self::Unreadable,
        Self::Fail,
        Self::Skip,
        Self::Empty,
        Self::Deleted,
        Self::Redirect,
    ];

    /// The baseline class name or names behind this family.
    pub fn class_names(self) -> &'static [&'static str] {
        match self {
            Self::Asset => &["AssetExportCollection<T>", "AssetsExportCollection<T>"],
            Self::Scene => &["SceneExportCollection"],
            Self::Prefab => &["PrefabExportCollection"],
            Self::Manager => &["ManagerExportCollection"],
            Self::Script => &["ScriptExportCollection"],
            Self::EmptyScript => &["EmptyScriptExportCollection"],
            Self::Unknown => &["UnknownExportCollection"],
            Self::Unreadable => &["UnreadableExportCollection"],
            Self::Fail => &["FailExportCollection"],
            Self::Skip => &["SkipExportCollection"],
            Self::Empty => &["EmptyExportCollection"],
            Self::Deleted => &["DeletedAssetsExportCollection"],
            Self::Redirect => &["RedirectExportCollection", "SingleRedirectExportCollection"],
        }
    }

    /// The `IExportCollection.Exportable` gate checked by `ProjectExporter.Export`.
    ///
    /// Non-exportable collections are skipped without calling `Export` and without a warning.
    pub fn is_exportable(self) -> bool {
        match self {
            Self::Skip | Self::Empty | Self::Deleted | Self::Redirect => false,
            Self::Asset
            | Self::Scene
            | Self::Prefab
            | Self::Manager
            | Self::Script
            | Self::EmptyScript
            | Self::Unknown
            | Self::Unreadable
            | Self::Fail => true,
        }
    }

    /// Whether a successful `Export` writes a `.meta` sidecar beside the asset.
    ///
    /// `AssetExportCollection` only writes the sidecar when `ExportInner` succeeds;
    /// `Fail` never succeeds, so it never writes one.
    pub fn emits_meta(self) -> bool {
        match self {
            Self::Asset | Self::Scene | Self::Prefab | Self::Script | Self::EmptyScript => true,
            Self::Manager
            | Self::Unknown
            | Self::Unreadable
            | Self::Fail
            | Self::Skip
            | Self::Empty
            | Self::Deleted
            | Self::Redirect => false,
        }
    }

    /// The file extension when the baseline fixes it for the whole family.
    ///
    /// `Asset` returns `None` because `GetExportExtension` is virtual and dispatches on
    /// generated marker interfaces; the remaining `None` families write no files.
    pub fn fixed_extension(self) -> Option<&'static str> {
        match self {
            Self::Scene => Some("unity"),
            Self::Prefab => Some("prefab"),
            Self::Manager => Some("asset"),
            Self::Script | Self::EmptyScript => Some("cs"),
            Self::Unknown => Some("unknown"),
            Self::Unreadable => Some("unreadable"),
            Self::Asset
            | Self::Fail
            | Self::Skip
            | Self::Empty
            | Self::Deleted
            | Self::Redirect => None,
        }
    }

    /// How the family resolves its destination file name.
    pub fn collision_rule(self) -> CollectionCollisionRule {
        match self {
            // `AssetExportCollection` and `PrefabExportCollection` go through
            // `GetUniqueFileName`; `Unknown`/`Unreadable` go through `GetUniqueName` directly.
            Self::Asset | Self::Prefab | Self::Unknown | Self::Unreadable => {
                CollectionCollisionRule::UniqueName
            }
            // `SceneExportCollection` joins `Scene.Path` with no unique-name resolution, but a
            // duplicate scene index skips the export with a warning when the file exists.
            // `ManagerExportCollection` joins its fixed name with no resolution at all.
            Self::Scene | Self::Manager => CollectionCollisionRule::FixedPath,
            Self::Script => CollectionCollisionRule::SkipAssetIfExists,
            Self::EmptyScript => CollectionCollisionRule::OverwriteAsset,
            Self::Fail | Self::Skip | Self::Empty | Self::Deleted | Self::Redirect => {
                CollectionCollisionRule::NotExported
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CollectionCollisionRule, ExportCollectionKind};

    #[test]
    fn every_audited_family_has_a_source_class_name() {
        assert_eq!(ExportCollectionKind::ALL.len(), 13);
        for kind in ExportCollectionKind::ALL {
            assert!(
                !kind.class_names().is_empty(),
                "{kind:?} must name its baseline class"
            );
        }
        assert_eq!(
            ExportCollectionKind::Redirect.class_names(),
            &["RedirectExportCollection", "SingleRedirectExportCollection"]
        );
    }

    #[test]
    fn exportable_gate_matches_the_project_exporter_loop() {
        // `ProjectExporter.Export` calls `Export` only when `Exportable` is true.
        for kind in [
            ExportCollectionKind::Asset,
            ExportCollectionKind::Scene,
            ExportCollectionKind::Prefab,
            ExportCollectionKind::Manager,
            ExportCollectionKind::Script,
            ExportCollectionKind::EmptyScript,
            ExportCollectionKind::Unknown,
            ExportCollectionKind::Unreadable,
            ExportCollectionKind::Fail,
        ] {
            assert!(kind.is_exportable(), "{kind:?} must be exportable");
        }
        for kind in [
            ExportCollectionKind::Skip,
            ExportCollectionKind::Empty,
            ExportCollectionKind::Deleted,
            ExportCollectionKind::Redirect,
        ] {
            assert!(!kind.is_exportable(), "{kind:?} must not be exportable");
        }
    }

    #[test]
    fn meta_emission_matches_each_export_override() {
        for kind in [
            ExportCollectionKind::Asset,
            ExportCollectionKind::Scene,
            ExportCollectionKind::Prefab,
            ExportCollectionKind::Script,
            ExportCollectionKind::EmptyScript,
        ] {
            assert!(kind.emits_meta(), "{kind:?} must emit a .meta sidecar");
        }
        for kind in [
            ExportCollectionKind::Manager,
            ExportCollectionKind::Unknown,
            ExportCollectionKind::Unreadable,
            ExportCollectionKind::Fail,
            ExportCollectionKind::Skip,
            ExportCollectionKind::Empty,
            ExportCollectionKind::Deleted,
            ExportCollectionKind::Redirect,
        ] {
            assert!(!kind.emits_meta(), "{kind:?} must not emit a .meta sidecar");
        }
    }

    #[test]
    fn fixed_extensions_match_the_baseline_format_strings() {
        assert_eq!(ExportCollectionKind::Asset.fixed_extension(), None);
        assert_eq!(ExportCollectionKind::Scene.fixed_extension(), Some("unity"));
        assert_eq!(
            ExportCollectionKind::Prefab.fixed_extension(),
            Some("prefab")
        );
        assert_eq!(
            ExportCollectionKind::Manager.fixed_extension(),
            Some("asset")
        );
        assert_eq!(ExportCollectionKind::Script.fixed_extension(), Some("cs"));
        assert_eq!(
            ExportCollectionKind::EmptyScript.fixed_extension(),
            Some("cs")
        );
        assert_eq!(
            ExportCollectionKind::Unknown.fixed_extension(),
            Some("unknown")
        );
        assert_eq!(
            ExportCollectionKind::Unreadable.fixed_extension(),
            Some("unreadable")
        );
        for kind in [
            ExportCollectionKind::Fail,
            ExportCollectionKind::Skip,
            ExportCollectionKind::Empty,
            ExportCollectionKind::Deleted,
            ExportCollectionKind::Redirect,
        ] {
            assert_eq!(kind.fixed_extension(), None, "{kind:?} writes no files");
        }
    }

    #[test]
    fn collision_rules_match_each_export_override() {
        assert_eq!(
            ExportCollectionKind::Asset.collision_rule(),
            CollectionCollisionRule::UniqueName
        );
        assert_eq!(
            ExportCollectionKind::Prefab.collision_rule(),
            CollectionCollisionRule::UniqueName
        );
        assert_eq!(
            ExportCollectionKind::Unknown.collision_rule(),
            CollectionCollisionRule::UniqueName
        );
        assert_eq!(
            ExportCollectionKind::Unreadable.collision_rule(),
            CollectionCollisionRule::UniqueName
        );
        assert_eq!(
            ExportCollectionKind::Scene.collision_rule(),
            CollectionCollisionRule::FixedPath
        );
        assert_eq!(
            ExportCollectionKind::Manager.collision_rule(),
            CollectionCollisionRule::FixedPath
        );
        assert_eq!(
            ExportCollectionKind::Script.collision_rule(),
            CollectionCollisionRule::SkipAssetIfExists
        );
        assert_eq!(
            ExportCollectionKind::EmptyScript.collision_rule(),
            CollectionCollisionRule::OverwriteAsset
        );
        for kind in [
            ExportCollectionKind::Fail,
            ExportCollectionKind::Skip,
            ExportCollectionKind::Empty,
            ExportCollectionKind::Deleted,
            ExportCollectionKind::Redirect,
        ] {
            assert_eq!(
                kind.collision_rule(),
                CollectionCollisionRule::NotExported,
                "{kind:?} writes no files"
            );
        }
    }
}
