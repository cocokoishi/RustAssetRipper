//! Unity YAML export bridge for objects decoded from a SerializedFile workspace.

mod assignment;
mod error;
mod generated_assets;
mod model;
mod object_conversion;
pub(crate) mod sprite_atlas;

pub use assignment::{
    workspace_assignments_to_unity_yaml,
    workspace_assignments_to_unity_yaml_with_streamed_resources,
    workspace_prefab_hierarchy_to_unity_yaml,
    workspace_prefab_hierarchy_to_unity_yaml_with_generated_transforms,
    workspace_scene_assignments_to_unity_yaml,
    workspace_scene_assignments_to_unity_yaml_with_generated_transforms,
    workspace_scene_assignments_to_unity_yaml_with_project_generated_assets,
    workspace_scene_assignments_to_unity_yaml_with_streamed_resources,
    workspace_scene_assignments_to_unity_yaml_with_streamed_resources_and_generated_transforms,
    UnityYamlCollectionAssignment,
};
pub use error::{UnityYamlWorkspaceCollectionError, UnityYamlWorkspaceExportError};
pub use generated_assets::{
    generated_transform_payload_to_unity_yaml_document, is_stripped_document,
    WorkspaceGeneratedTransformYamlContext,
};
pub use model::{
    UnityYamlWorkspaceCollectionLimits, WorkspaceGeneratedLightingDataYamlPointer,
    WorkspaceStreamedYamlResourceContext,
};
pub(crate) use object_conversion::workspace_object_to_unity_yaml_document_with_atlas_overlay;
pub use object_conversion::{
    workspace_object_to_unity_yaml_document,
    workspace_object_to_unity_yaml_document_with_processed_animation_root,
    workspace_object_to_unity_yaml_document_with_streamed_resources,
};

#[cfg(test)]
pub(crate) mod tests;
