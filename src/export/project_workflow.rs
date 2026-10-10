//! Executable Unity project export composed from collection dispatch, staged planning, and
//! the `ExportHandler.Export` write sequence.
//!
//! This is the product-level project path: every decoded workspace object is routed through the
//! frozen `ProjectExporter.CreateCollection` stack walk ([`crate::export::project_exporter`]),
//! every route is materialized into a staged [`ProjectExportPlan`], and only then does
//! `execute_project_export` touch the disk. Objects whose baseline collection is not exportable
//! are reported as explicit notices. Some unported families retain an explicit generic YAML
//! fallback; AudioClip uses the source `YamlAudioExportCollection` path only when that collection
//! wins and no unresolved audio data would be dropped. TextAssets with non-empty `m_Script` bytes
//! use their binary exporter, settings-based extension, and TextScriptImporter.
//!
//! Scene definitions are processed into workspace hierarchies and emitted as `.unity` files
//! through the scene collection route, with scene IDs, pointers, and `DefaultImporter` metadata.
//! Modern PrefabProcessor hierarchies are emitted as `.prefab` collections with planned IDs,
//! generated Transforms, pointer mapping, and `PrefabImporter` metadata. A source-matched legacy
//! generated Prefab route covers a synthetic Unity 2017.4.40f1 hierarchy; other legacy branches,
//! full AssetBundle-derived importer metadata, and the remaining class-specific exporters remain
//! required integrations. A manager-backed `DllExportWithoutRenaming` route saves catalog-backed
//! assemblies to `Assets/Plugins` with source GUID/fileID mapping and a source-pinned Unity 2019.4
//! `PluginImporter`; assembly reserialization and additional manager-resolved assemblies remain
//! unresolved.

include!("project_workflow_entrypoints.rs");
include!("project_workflow_export.rs");
include!("project_workflow_asset_routing.rs");
include!("project_workflow_lighting_data.rs");

#[cfg(test)]
mod tests {
    include!("project_workflow_tests_01.rs");
}
