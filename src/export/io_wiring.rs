//! Lane integration surface for the import, container, compression, and resource-loading workstream.
//!
//! The implementation of the decoded-object resource wiring lives in
//! [`crate::io::resource_wiring`]; this module re-exports it so the export/project path can depend
//! on the `crate::export` surface without reaching into `crate::io` directly.
//!
//! `src/export/project_workflow.rs` should call [`stage_workspace_streamed_yaml_asset`] for Mesh
//! and image-texture objects before writing their YAML documents, passing the workspace, the
//! explicit root resource catalog, and the platform provider already used by the import path.

pub use crate::io::resource_wiring::{
    read_object_streamed_resource_bytes, select_streamed_asset_info,
    stage_workspace_streamed_yaml_asset, streamed_asset_stream_field_names,
    WorkspaceStreamedAssetError, WorkspaceStreamedAssetLimits, WorkspaceStreamedAssetReport,
};
