//! SceneDefinition-owned workspace views of recovered GameObject and Transform data.

include!("scene_hierarchy_types_and_roots.rs");
include!("scene_hierarchy_membership_graph.rs");
include!("scene_hierarchy_builders.rs");
include!("scene_hierarchy_transform_support.rs");

#[cfg(test)]
mod tests {
    include!("scene_hierarchy_tests_01.rs");
    include!("scene_hierarchy_tests_02.rs");
}
