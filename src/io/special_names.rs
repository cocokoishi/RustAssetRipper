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

/// The managed assembly file extension the source appends and strips.
pub const ASSEMBLY_EXTENSION: &str = ".dll";

/// `SpecialFileNames.IsAssemblyIdentifier`: the legacy UnityScript/Boo module names.
fn is_assembly_identifier(assembly: &str) -> bool {
    matches!(
        assembly,
        "Boo"
            | "Boo - first pass"
            | "CSharp"
            | "CSharp - first pass"
            | "UnityScript"
            | "UnityScript - first pass"
    )
}

/// `SpecialFileNames.FixAssemblyName`: prefix legacy module names with `Assembly - `
/// (checked against the original spelling, case-sensitively) and strip one `.dll`
/// suffix (also case-sensitive).
pub fn fix_assembly_name(assembly: &str) -> String {
    let prefixed = if is_assembly_identifier(assembly) {
        format!("Assembly - {assembly}")
    } else {
        assembly.to_owned()
    };
    remove_assembly_file_extension(&prefixed)
}

/// `SpecialFileNames.RemoveAssemblyFileExtension`: strip one case-sensitive `.dll`.
pub fn remove_assembly_file_extension(assembly: &str) -> String {
    assembly
        .strip_suffix(ASSEMBLY_EXTENSION)
        .unwrap_or(assembly)
        .to_owned()
}

/// `SpecialFileNames.AddAssemblyFileExtension`: append `.dll` unless already present.
pub fn add_assembly_file_extension(assembly: &str) -> String {
    if assembly.ends_with(ASSEMBLY_EXTENSION) {
        assembly.to_owned()
    } else {
        format!("{assembly}{ASSEMBLY_EXTENSION}")
    }
}

/// `SpecialFileNames.IsProjectAssembly`: the `Assembly - ` / `Assembly-` prefixes,
/// matched case-sensitively.
pub fn is_project_assembly(assembly: &str) -> bool {
    assembly.starts_with("Assembly - ") || assembly.starts_with("Assembly-")
}

#[cfg(test)]
mod tests {
    use super::{
        add_assembly_file_extension, fix_assembly_name, fix_dependency_name, fix_file_identifier,
        fix_resource_path, is_project_assembly, remove_assembly_file_extension,
    };
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

    /// Differential against the shipped `AssetRipper.IO.Files.dll` rows committed in
    /// `script_identity_fixture.txt` (probe `Temp/opencode/scriptidprobe`, SDK 10.0.401):
    /// assembly-name fixing, extension add/remove, and project-assembly detection,
    /// including the case-sensitive edges (`csharp`, `CSHARP.DLL`, `A.DLL`).
    #[test]
    fn assembly_name_helpers_match_the_reference_runtime() {
        let fixture = include_str!("script_identity_fixture.txt");
        let mut fixed = 0;
        let mut removed = 0;
        let mut added = 0;
        let mut detected = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // Inputs never contain pipes or whitespace runs; outputs are single
            // tokens, so a plain split is exact.
            let mut parts = line.split('|');
            match parts.next() {
                Some("FIX") => {
                    let input = parts.next().expect("an input");
                    let expected = parts.next().expect("an output");
                    assert!(parts.next().is_none(), "{line}");
                    assert_eq!(fix_assembly_name(input), expected, "{line}");
                    fixed += 1;
                }
                Some("RMEXT") => {
                    let input = parts.next().expect("an input");
                    let expected = parts.next().expect("an output");
                    assert!(parts.next().is_none(), "{line}");
                    assert_eq!(remove_assembly_file_extension(input), expected, "{line}");
                    removed += 1;
                }
                Some("ADDEXT") => {
                    let input = parts.next().expect("an input");
                    let expected = parts.next().expect("an output");
                    assert!(parts.next().is_none(), "{line}");
                    assert_eq!(add_assembly_file_extension(input), expected, "{line}");
                    added += 1;
                }
                Some("ISPROJ") => {
                    let input = parts.next().expect("an input");
                    let expected = parts.next().expect("a flag");
                    assert!(parts.next().is_none(), "{line}");
                    assert_eq!(is_project_assembly(input), expected == "1", "{line}");
                    detected += 1;
                }
                _ => {}
            }
        }
        assert_eq!(fixed, 16, "all probed fix rows are asserted");
        assert_eq!(removed, 5, "all probed remove rows are asserted");
        assert_eq!(added, 5, "all probed add rows are asserted");
        assert_eq!(detected, 9, "all probed project rows are asserted");
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
