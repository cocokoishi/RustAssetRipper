//! `.meta` emission sites ported from the frozen baseline's `ExportMeta` flow.
//!
//! `ExportCollection.ExportMeta` derives the sidecar path as `{filePath}.meta`, opens it with
//! `File.Create` (an unconditional overwrite), and writes UTF-8 without a BOM through a
//! `YamlWriter` with default-tag and version emission off and key formatting on. Every call site
//! writes the exported asset bytes first and the sidecar second; no site writes the sidecar for
//! a failed asset export.
//!
//! A whole-`Source` search for `new Meta(` finds exactly five product emission sites (the two
//! remaining hits construct an unrelated HTML `Meta` in `AssetRipper.GUI.Web`). Their collision
//! rules differ: four sites overwrite freely through `File.Create`, while the per-script path in
//! `ScriptExportCollection.Export` logs an error and skips the sidecar when `{filePath}.meta`
//! already exists. No snapshot caller passes `isFolder: true`, so the `folderAsset` envelope
//! branch in `Meta.cs` is never emitted by this baseline; folder sidecars are recorded as
//! unwritten rather than invented.
//!
//! The Rust add-only writer (`write_unity_meta_sidecar` in `project_output.rs`) refuses to
//! replace an existing sidecar, which is stricter than the four overwriting sites and exactly
//! matches the script skip rule. `plan_meta_write` connects the two: it turns each site's
//! collision rule plus sidecar existence into an explicit decision, so overwrite sites stay
//! visibly unwired instead of being silently overwritten or silently skipped.

use super::guid_policy::ExportGuidSource;
use super::importer_fields::CreateImporterKind;

/// How one emission site reacts when its sidecar path already exists.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MetaCollision {
    /// `File.Create`: the existing sidecar is replaced.
    Overwrite,
    /// The export logs an error and writes nothing (the per-script path only).
    SkipWithError,
}

/// One `.meta` emission site found by the whole-`Source` `new Meta(` search.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MetaEmissionSite {
    /// The exporting method that constructs the `Meta` value.
    pub site: &'static str,
    /// Which GUID ownership rule supplies the sidecar GUID.
    pub guid: ExportGuidSource,
    /// The importer object the site wraps.
    pub importer: CreateImporterKind,
    /// Where that importer comes from.
    pub importer_source: &'static str,
    /// Every site writes the asset bytes before the sidecar.
    pub writes_asset_first: bool,
    /// The site's collision rule when the sidecar already exists.
    pub collision: MetaCollision,
    /// No snapshot caller passes `isFolder: true`; every row is false.
    pub is_folder: bool,
}

/// All five product `.meta` emission sites in source-discovery order.
pub const META_EMISSION_SITES: &[MetaEmissionSite] = &[
    MetaEmissionSite {
        site: "ExportCollection.ExportAsset",
        guid: ExportGuidSource::RandomPerCollection,
        importer: CreateImporterKind::NativeFormat,
        importer_source: "caller-supplied importer argument",
        writes_asset_first: true,
        collision: MetaCollision::Overwrite,
        is_folder: false,
    },
    MetaEmissionSite {
        site: "AssetExportCollection<T>.Export",
        guid: ExportGuidSource::RandomPerCollection,
        importer: CreateImporterKind::NativeFormat,
        importer_source: "CreateImporter dispatch table",
        writes_asset_first: true,
        collision: MetaCollision::Overwrite,
        is_folder: false,
    },
    MetaEmissionSite {
        site: "SceneExportCollection.ExportScene",
        guid: ExportGuidSource::PreservedScene,
        importer: CreateImporterKind::Default,
        importer_source: "DefaultImporter.Create with bundle propagation",
        writes_asset_first: true,
        collision: MetaCollision::Overwrite,
        is_folder: false,
    },
    MetaEmissionSite {
        site: "ScriptExportCollectionBase.OnScriptExported",
        guid: ExportGuidSource::DeterministicScript,
        importer: CreateImporterKind::Mono,
        importer_source: "MonoImporter.Create with truncated execution order",
        writes_asset_first: true,
        collision: MetaCollision::SkipWithError,
        is_folder: false,
    },
    MetaEmissionSite {
        site: "ScriptExportCollection.OnAssemblyExported",
        guid: ExportGuidSource::DeterministicScript,
        importer: CreateImporterKind::Plugin,
        importer_source: "PluginImporter.Create with Any/Editor platform entries",
        writes_asset_first: true,
        collision: MetaCollision::Overwrite,
        is_folder: false,
    },
];

/// The `ExportMeta` sidecar mechanics shared by every site.
pub mod export_meta_mechanics {
    /// `ExportCollection.MetaExtension`: the sidecar suffix appended to the asset path.
    pub const META_EXTENSION: &str = ".meta";
    /// `new UTF8Encoding(false)`: no byte-order mark is written.
    pub const EMITS_BYTE_ORDER_MARK: bool = false;
    /// `YamlWriter.IsWriteDefaultTag = false`.
    pub const WRITES_DEFAULT_TAG: bool = false;
    /// `YamlWriter.IsWriteVersion = false`.
    pub const WRITES_VERSION_DIRECTIVE: bool = false;
    /// `YamlWriter.IsFormatKeys = true`.
    pub const FORMATS_MAPPING_KEYS: bool = true;
}

/// The export-owned decision for one planned sidecar write.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MetaWriteDecision {
    /// No sidecar exists (or the script rule is satisfied): the caller may invoke the
    /// add-only sidecar writer.
    Proceed,
    /// The per-script rule: a sidecar already exists, so log an error and write nothing.
    SkipWithError,
    /// An overwriting site found an existing sidecar. The source would replace it via
    /// `File.Create`, but the add-only writer refuses; the caller must surface this
    /// explicitly instead of overwriting or reporting success.
    BlockedOverwrite,
}

/// Turn one site's collision rule plus sidecar existence into an explicit write decision.
///
/// This never authorizes an overwrite: `BlockedOverwrite` keeps the four overwriting sites
/// visibly unwired until an explicit overwrite path exists.
pub fn plan_meta_write(collision: MetaCollision, sidecar_exists: bool) -> MetaWriteDecision {
    if !sidecar_exists {
        return MetaWriteDecision::Proceed;
    }
    match collision {
        MetaCollision::SkipWithError => MetaWriteDecision::SkipWithError,
        MetaCollision::Overwrite => MetaWriteDecision::BlockedOverwrite,
    }
}

#[cfg(test)]
mod tests {
    use super::export_meta_mechanics;
    use super::{plan_meta_write, MetaCollision, MetaWriteDecision, META_EMISSION_SITES};

    #[test]
    fn emission_table_covers_all_five_product_sites() {
        let sites: Vec<&str> = META_EMISSION_SITES.iter().map(|site| site.site).collect();
        assert_eq!(
            sites,
            [
                "ExportCollection.ExportAsset",
                "AssetExportCollection<T>.Export",
                "SceneExportCollection.ExportScene",
                "ScriptExportCollectionBase.OnScriptExported",
                "ScriptExportCollection.OnAssemblyExported",
            ]
        );
        // Four sites overwrite through File.Create; only the per-script path skips with an error.
        let skips = META_EMISSION_SITES
            .iter()
            .filter(|site| site.collision == MetaCollision::SkipWithError)
            .count();
        assert_eq!(skips, 1);
        assert_eq!(
            META_EMISSION_SITES
                .iter()
                .find(|site| site.collision == MetaCollision::SkipWithError)
                .unwrap()
                .site,
            "ScriptExportCollectionBase.OnScriptExported"
        );
    }

    #[test]
    fn every_site_writes_asset_bytes_first_and_never_a_folder_sidecar() {
        assert!(META_EMISSION_SITES
            .iter()
            .all(|site| site.writes_asset_first));
        // A whole-Source search for `new Meta(` finds no caller passing `isFolder: true`,
        // so the folderAsset envelope branch stays unwritten rather than invented.
        assert!(META_EMISSION_SITES.iter().all(|site| !site.is_folder));
    }

    #[test]
    fn collision_planner_matches_source_rules_without_authorizing_overwrites() {
        // Absent sidecars always proceed, whatever the site rule.
        assert_eq!(
            plan_meta_write(MetaCollision::Overwrite, false),
            MetaWriteDecision::Proceed
        );
        assert_eq!(
            plan_meta_write(MetaCollision::SkipWithError, false),
            MetaWriteDecision::Proceed
        );
        // The per-script rule skips with an error when the sidecar exists.
        assert_eq!(
            plan_meta_write(MetaCollision::SkipWithError, true),
            MetaWriteDecision::SkipWithError
        );
        // Overwriting sites stay explicitly blocked: the add-only writer refuses, and this
        // decision keeps that refusal visible instead of reporting success.
        assert_eq!(
            plan_meta_write(MetaCollision::Overwrite, true),
            MetaWriteDecision::BlockedOverwrite
        );
    }

    #[test]
    fn export_meta_mechanics_match_the_source_writer() {
        assert_eq!(export_meta_mechanics::META_EXTENSION, ".meta");
        let emits_bom = export_meta_mechanics::EMITS_BYTE_ORDER_MARK;
        assert!(!emits_bom);
        let writes_tag = export_meta_mechanics::WRITES_DEFAULT_TAG;
        assert!(!writes_tag);
        let writes_version = export_meta_mechanics::WRITES_VERSION_DIRECTIVE;
        assert!(!writes_version);
        let formats_keys = export_meta_mechanics::FORMATS_MAPPING_KEYS;
        assert!(formats_keys);
    }
}
