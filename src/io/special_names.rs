//! Source-compatible normalization for serialized file and resource names.

use std::path::Path;

pub const DEFAULT_RESOURCE_NAME_1: &str = "unity default resources";
pub const DEFAULT_RESOURCE_NAME_2: &str = "unity_default_resources";
pub const BUILTIN_EXTRA_NAME_1: &str = "unity builtin extra";
pub const BUILTIN_EXTRA_NAME_2: &str = "unity_builtin_extra";

pub fn is_default_resource(name: &str) -> bool {
    matches!(name, DEFAULT_RESOURCE_NAME_1 | DEFAULT_RESOURCE_NAME_2)
}

pub fn is_builtin_extra(name: &str) -> bool {
    matches!(name, BUILTIN_EXTRA_NAME_1 | BUILTIN_EXTRA_NAME_2)
}

/// Lowercase and normalize a serialized-file external identifier as the source does.
pub fn fix_file_identifier(name: &str) -> String {
    let lowercase = name.to_lowercase();
    let dependency = fix_dependency_name(&lowercase);
    fix_resource_path(&dependency)
}

/// Remove the source's `library/` or `resources/` prefix when present.
pub fn fix_dependency_name(dependency: &str) -> String {
    dependency
        .strip_prefix("library/")
        .or_else(|| dependency.strip_prefix("resources/"))
        .unwrap_or(dependency)
        .to_owned()
}

/// Collapse `archive:/...` virtual paths to their host-platform basename.
pub fn fix_resource_path(resource_path: &str) -> String {
    let Some(path) = resource_path.strip_prefix("archive:/") else {
        return resource_path.to_owned();
    };
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::{fix_dependency_name, fix_file_identifier, fix_resource_path};
    use crate::serialized_file::metadata::FileIdentifier;

    #[test]
    fn fixes_source_dependency_prefixes_and_archive_paths() {
        assert_eq!(
            fix_file_identifier("LIBRARY/Shared.Assets"),
            "shared.assets"
        );
        assert_eq!(
            fix_file_identifier("resources/archive:/Bundles/MainData"),
            "maindata"
        );
        assert_eq!(
            fix_file_identifier("Scene/LEVEL0.ASSETS"),
            "scene/level0.assets"
        );
    }

    #[test]
    fn prefix_helpers_keep_their_source_case_and_fallback_behavior() {
        assert_eq!(fix_dependency_name("library/path"), "path");
        assert_eq!(fix_dependency_name("Library/path"), "Library/path");
        assert_eq!(fix_dependency_name("resources/path"), "path");
        assert_eq!(fix_resource_path("archive:/dir/file.assets"), "file.assets");
        assert_eq!(fix_resource_path("dir/file.assets"), "dir/file.assets");
    }

    #[test]
    fn recognizes_source_engine_resource_aliases() {
        assert!(super::is_default_resource(super::DEFAULT_RESOURCE_NAME_1));
        assert!(super::is_default_resource(super::DEFAULT_RESOURCE_NAME_2));
        assert!(super::is_builtin_extra(super::BUILTIN_EXTRA_NAME_1));
        assert!(super::is_builtin_extra(super::BUILTIN_EXTRA_NAME_2));
        assert!(!super::is_default_resource("unity editor resources"));
        assert!(!super::is_builtin_extra("unity default resources"));
    }

    #[test]
    fn external_identifier_exposes_its_fixed_dependency_path_without_losing_origin() {
        let identifier = FileIdentifier {
            asset_path: Some(String::new()),
            guid: Some([1, 2, 3, 4]),
            file_type: Some(2),
            path_name_origin: "Library/Shared.Assets".to_owned(),
        };

        assert_eq!(identifier.fixed_path_name(), "shared.assets");
        assert_eq!(identifier.path_name_origin, "Library/Shared.Assets");
    }
}
