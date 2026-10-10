//! Unity YAML export bridge for objects decoded from a SerializedFile workspace.

include!("workspace_yaml_assignments.rs");
include!("workspace_yaml_object_conversion.rs");
include!("workspace_yaml_generated_assets.rs");

#[cfg(test)]
pub(crate) mod tests {
    include!("workspace_yaml_tests_01.rs");
    include!("workspace_yaml_tests_02.rs");
}
