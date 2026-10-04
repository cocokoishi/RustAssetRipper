//! Empty-script export ported from `EmptyScript` and `EmptyScriptExportCollection`.
//!
//! When no assembly manager is available, scripts export as compilable dummy classes instead
//! of decompiled sources. The collection gathers every MonoScript in the bundle hierarchy,
//! deduplicates by the `(Class, Namespace, Assembly)` triple (first registration wins),
//! skips injected scripts (all three parts empty), and unconditionally overwrites each `.cs`
//! file with dummy content; assembly-definition details accumulate per assembly name and emit
//! only when details exist, the export version reaches Unity 2017.3, and the assembly is not
//! one of the six predefined ones. The export always reports success.
//!
//! Dummy content comes in two verbatim templates (global versus namespaced). Generic class
//! names of the form `Name` + backtick + arity (for example `Box`3`) expand to
//! `Name<T1, T2, T3>`; the arity pattern is `^(\w+)` + backtick + `([1-9][0-9]*)$` with an
//! `int.TryParse` tail, so leading zeros, missing digits, and overflowing arities are not
//! generic. The word class is approximated as alphanumeric-or-underscore: ASCII behavior is
//! identical, while exotic Unicode connector punctuation and marks may differ from .NET
//! `\w` and stay an open edge.
//!
//! Template bytes join with CRLF because the frozen snapshot is uniformly CRLF on disk and
//! C# raw strings embed file newlines verbatim; whitespace-only template lines become empty
//! lines (verified against the .NET raw-string rules with an isolated `dotnet run` probe).
//! A reference-runtime differential of the rendered bytes stays open.

use crate::processing::scene_naming::SceneUnityVersion;

/// The fixed `Name` of the empty-script collection.
pub const EMPTY_SCRIPT_COLLECTION_NAME: &str = "EmptyScriptExportCollection";

/// A script identity for empty-script deduplication: `(Class, Namespace, Assembly)`.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct MonoScriptIdentity {
    pub class: String,
    pub namespace: String,
    pub assembly: String,
}

impl MonoScriptIdentity {
    /// `MonoScriptInfo.IsInjected`: all three parts are empty.
    pub fn is_injected(&self) -> bool {
        self.class.is_empty() && self.namespace.is_empty() && self.assembly.is_empty()
    }
}

/// Plan empty-script writes: deduplicate by identity (first registration wins, including
/// injected scripts occupying their slot), then drop injected scripts, preserving order.
pub fn plan_empty_script_writes(scripts: Vec<MonoScriptIdentity>) -> Vec<MonoScriptIdentity> {
    let mut unique: Vec<MonoScriptIdentity> = Vec::new();
    for script in scripts {
        if !unique.contains(&script) {
            unique.push(script);
        }
    }
    unique
        .into_iter()
        .filter(|script| !script.is_injected())
        .collect()
}

/// Upper bound for generic arity expansion; larger arities fail instead of allocating.
pub const MAX_GENERIC_PARAMETERS: i32 = 1024;

/// Why dummy content cannot be rendered.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EmptyScriptError {
    /// The generic arity exceeds [`MAX_GENERIC_PARAMETERS`].
    TooManyGenericParameters { count: i32 },
}

impl core::fmt::Display for EmptyScriptError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::TooManyGenericParameters { count } => {
                write!(
                    formatter,
                    "generic arity {count} exceeds the expansion bound"
                )
            }
        }
    }
}

impl std::error::Error for EmptyScriptError {}

/// One `\w` character for the generic-name pattern, approximated as alphanumeric or `_`.
///
/// ASCII behavior matches .NET `\w` exactly; exotic Unicode connector punctuation and marks
/// accepted by .NET stay an open edge and are rejected here.
pub fn is_generic_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Split `Name` + backtick + arity into its base name and arity.
///
/// Returns `None` for non-generic names, leading-zero arities, and overflowing arities,
/// mirroring the regex plus `int.TryParse` combination.
pub fn split_generic_name(class_name: &str) -> Option<(&str, i32)> {
    let (base, arity_text) = class_name.split_once('`')?;
    if base.is_empty() || !base.chars().all(is_generic_word_char) {
        return None;
    }
    let mut digits = arity_text.chars();
    match digits.next() {
        Some(first) if ('1'..='9').contains(&first) => {}
        _ => return None,
    }
    if !digits.all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some((base, arity_text.parse::<i32>().ok()?))
}

/// Expand a generic class name to `Name<T1, T2>` display form, or keep it verbatim.
pub fn expand_generic_class_name(class_name: &str) -> Result<String, EmptyScriptError> {
    match split_generic_name(class_name) {
        None => Ok(class_name.to_owned()),
        Some((base, count)) => {
            if count > MAX_GENERIC_PARAMETERS {
                return Err(EmptyScriptError::TooManyGenericParameters { count });
            }
            let parameters = (1..=count)
                .map(|index| format!("T{index}"))
                .collect::<Vec<_>>()
                .join(", ");
            Ok(format!("{base}<{parameters}>"))
        }
    }
}

/// Global-scope dummy template lines; `{{name}}` marks the class slot.
const GLOBAL_TEMPLATE_LINES: &[&str] = &[
        "using UnityEngine;",
        "",
        "public class {{name}} : MonoBehaviour",
        "{",
        "\t/*",
        "\tDummy class. This could have happened for several reasons:",
        "",
        "\t1. No dll files were provided to AssetRipper.",
        "",
        "\t\tUnity asset bundles and serialized files do not contain script information to decompile.",
        "\t\t\t* For Mono games, that information is contained in .NET dll files.",
        "\t\t\t* For Il2Cpp games, that information is contained in compiled C++ assemblies and the global metadata.",
        "",
        "\t\tAssetRipper usually expects games to conform to a normal file structure for Unity games of that platform.",
        "\t\tA unexpected file structure could cause AssetRipper to not find the required files.",
        "",
        "\t2. Incorrect dll files were provided to AssetRipper.",
        "",
        "\t\tAny of the following could cause this:",
        "\t\t\t* Il2CppInterop assemblies",
        "\t\t\t* Deobfuscated assemblies",
        "\t\t\t* Older assemblies (compared to when the bundle was built)",
        "\t\t\t* Newer assemblies (compared to when the bundle was built)",
        "",
        "\t\tNote: Although assembly publicizing is bad, it alone cannot cause empty scripts. See: https://github.com/AssetRipper/AssetRipper/issues/653",
        "",
        "\t3. Assembly Reconstruction has not been implemented.",
        "",
        "\t\tAsset bundles contain a small amount of information about the script content.",
        "\t\tThis information can be used to recover the serializable fields of a script.",
        "",
        "\t\tSee: https://github.com/AssetRipper/AssetRipper/issues/655",
        "",
        "\t4. This script is unnecessary.",
        "",
        "\t\tIf this script has no asset or script references, it can be deleted.",
        "\t\tBe sure to resolve any compile errors before deleting because they can hide references.",
        "",
        "\t5. Script Content Level 0",
        "",
        "\t\tAssetRipper was set to not load any script information.",
        "",
        "\t6. Cpp2IL failed to decompile Il2Cpp data",
        "",
        "\t\tIf this happened, there will be errors in the AssetRipper.log indicating that it happened.",
        "\t\tThis is an upstream problem, and the AssetRipper developer has very little control over it.",
        "\t\tPlease post a GitHub issue at: https://github.com/SamboyCoding/Cpp2IL/issues",
        "",
        "\t7. An incorrect path was provided to AssetRipper.",
        "",
        "\t\tThis is characterized by \"Mixed game structure has been found at\" in the AssetRipper.log file.",
        "\t\tAssetRipper expects games to conform to a normal file structure for Unity games of that platform.",
        "\t\tAn unexpected file structure could cause AssetRipper to not find the required files for script decompilation.",
        "\t\tGenerally, AssetRipper expects users to provide the root folder of the game. For example:",
        "\t\t\t* Windows: the folder containing the game's .exe file",
        "\t\t\t* Mac: the .app file/folder",
        "\t\t\t* Linux: the folder containing the game's executable file",
        "\t\t\t* Android: the apk file",
        "\t\t\t* iOS: the ipa file",
        "\t\t\t* Switch: the folder containing exefs and romfs",
        "",
        "\t*/",
        "}",
];

/// Namespaced dummy template lines; `{{@namespace}}` and `{{name}}` mark the slots.
const NAMESPACED_TEMPLATE_LINES: &[&str] = &[
        "using UnityEngine;",
        "",
        "namespace {{@namespace}}",
        "{",
        "\tpublic class {{name}} : MonoBehaviour",
        "\t{",
        "\t\t/*",
        "\t\tDummy class. This could have happened for several reasons:",
        "",
        "\t\t1. No dll files were provided to AssetRipper.",
        "",
        "\t\t\tUnity asset bundles and serialized files do not contain script information to decompile.",
        "\t\t\t\t* For Mono games, that information is contained in .NET dll files.",
        "\t\t\t\t* For Il2Cpp games, that information is contained in compiled C++ assemblies and the global metadata.",
        "",
        "\t\t\tAssetRipper usually expects games to conform to a normal file structure for Unity games of that platform.",
        "\t\t\tA unexpected file structure could cause AssetRipper to not find the required files.",
        "",
        "\t\t2. Incorrect dll files were provided to AssetRipper.",
        "",
        "\t\t\tAny of the following could cause this:",
        "\t\t\t\t* Il2CppInterop assemblies",
        "\t\t\t\t* Deobfuscated assemblies",
        "\t\t\t\t* Older assemblies (compared to when the bundle was built)",
        "\t\t\t\t* Newer assemblies (compared to when the bundle was built)",
        "",
        "\t\t\tNote: Although assembly publicizing is bad, it alone cannot cause empty scripts. See: https://github.com/AssetRipper/AssetRipper/issues/653",
        "",
        "\t\t3. Assembly Reconstruction has not been implemented.",
        "",
        "\t\t\tAsset bundles contain a small amount of information about the script content.",
        "\t\t\tThis information can be used to recover the serializable fields of a script.",
        "",
        "\t\t\tSee: https://github.com/AssetRipper/AssetRipper/issues/655",
        "",
        "\t\t4. This script is unnecessary.",
        "",
        "\t\t\tIf this script has no asset or script references, it can be deleted.",
        "\t\t\tBe sure to resolve any compile errors before deleting because they can hide references.",
        "",
        "\t\t5. Script Content Level 0",
        "",
        "\t\t\tAssetRipper was set to not load any script information.",
        "",
        "\t\t6. Cpp2IL failed to decompile Il2Cpp data",
        "",
        "\t\t\tIf this happened, there will be errors in the AssetRipper.log indicating that it happened.",
        "\t\t\tThis is an upstream problem, and the AssetRipper developer has very little control over it.",
        "\t\t\tPlease post a GitHub issue at: https://github.com/SamboyCoding/Cpp2IL/issues",
        "",
        "\t\t7. An incorrect path was provided to AssetRipper.",
        "",
        "\t\t\tThis is characterized by \"Mixed game structure has been found at\" in the AssetRipper.log file.",
        "\t\t\tAssetRipper expects games to conform to a normal file structure for Unity games of that platform.",
        "\t\t\tAn unexpected file structure could cause AssetRipper to not find the required files for script decompilation.",
        "\t\t\tGenerally, AssetRipper expects users to provide the root folder of the game. For example:",
        "\t\t\t\t* Windows: the folder containing the game's .exe file",
        "\t\t\t\t* Mac: the .app file/folder",
        "\t\t\t\t* Linux: the folder containing the game's executable file",
        "\t\t\t\t* Android: the apk file",
        "\t\t\t\t* iOS: the ipa file",
        "\t\t\t\t* Switch: the folder containing exefs and romfs",
        "",
        "\t\t*/",
        "\t}",
        "}",
];

/// Render the template lines with CRLF endings and a trailing newline, like the source.
fn render_template(lines: &[&str], namespace: &str, display_name: &str) -> String {
    let mut rendered = String::new();
    for line in lines {
        rendered.push_str(
            &line
                .replace("{{@namespace}}", namespace)
                .replace("{{name}}", display_name),
        );
        rendered.push_str("\r\n");
    }
    rendered
}

/// `EmptyScript.GetContent`: the dummy class text for one namespace/class pair.
///
/// An empty namespace selects the global template. Only absurd generic arities fail.
pub fn empty_script_content(namespace: &str, class_name: &str) -> Result<String, EmptyScriptError> {
    let display_name = expand_generic_class_name(class_name)?;
    if namespace.is_empty() {
        Ok(render_template(
            GLOBAL_TEMPLATE_LINES,
            namespace,
            &display_name,
        ))
    } else {
        Ok(render_template(
            NAMESPACED_TEMPLATE_LINES,
            namespace,
            &display_name,
        ))
    }
}

/// `ReferenceAssemblies.IsPredefinedAssembly`: the six assemblies excluded from definition
/// emission, matched ordinally.
pub fn is_predefined_assembly(assembly_name: &str) -> bool {
    matches!(
        assembly_name,
        "Assembly-CSharp"
            | "Assembly-CSharp-firstpass"
            | "Assembly-CSharp-Editor"
            | "Assembly-CSharp-Editor-firstpass"
            | "Assembly-UnityScript"
            | "Assembly-UnityScript-firstpass"
    )
}

/// The assembly-definition emission gate: details exist and the version reaches 2017.3.
pub fn emit_assembly_definitions(has_details: bool, version: SceneUnityVersion) -> bool {
    has_details && (version.major > 2017 || (version.major == 2017 && version.minor >= 3))
}

#[cfg(test)]
mod tests {
    use super::{
        emit_assembly_definitions, empty_script_content, expand_generic_class_name,
        is_predefined_assembly, plan_empty_script_writes, split_generic_name, MonoScriptIdentity,
        EMPTY_SCRIPT_COLLECTION_NAME,
    };
    use crate::processing::scene_naming::SceneUnityVersion;

    fn identity(class: &str, namespace: &str, assembly: &str) -> MonoScriptIdentity {
        MonoScriptIdentity {
            class: class.to_owned(),
            namespace: namespace.to_owned(),
            assembly: assembly.to_owned(),
        }
    }

    fn version(text: &str) -> SceneUnityVersion {
        SceneUnityVersion::parse(text).unwrap()
    }

    #[test]
    fn generic_names_follow_arity_pattern_and_int_parsing() {
        assert_eq!(split_generic_name("Box`3"), Some(("Box", 3)));
        assert_eq!(split_generic_name("A`1"), Some(("A", 1)));
        assert_eq!(split_generic_name("Plain"), None);
        assert_eq!(split_generic_name("Box`0"), None);
        assert_eq!(split_generic_name("Box`01"), None);
        assert_eq!(split_generic_name("`2"), None);
        assert_eq!(split_generic_name("A`B`2"), None);
        assert_eq!(split_generic_name("Huge`9999999999"), None);
        assert_eq!(split_generic_name("类型`2"), Some(("类型", 2)));
        assert_eq!(
            expand_generic_class_name("Box`3").unwrap(),
            "Box<T1, T2, T3>"
        );
        assert_eq!(expand_generic_class_name("Plain").unwrap(), "Plain");
        assert!(expand_generic_class_name("Box`2000").is_err());
    }

    #[test]
    fn dummy_content_selects_templates_and_expands_names() {
        let global = empty_script_content("", "Brick").unwrap();
        assert!(global.starts_with("using UnityEngine;\r\n\r\npublic class Brick"));
        assert!(!global.contains("namespace"));
        assert!(global.ends_with("}\r\n"));
        assert!(!global.replace("\r\n", "").contains('\n'));

        let namespaced = empty_script_content("Game.Play", "Brick").unwrap();
        assert!(namespaced.contains("namespace Game.Play\r\n"));
        assert!(namespaced.contains("\tpublic class Brick : MonoBehaviour\r\n"));
        assert!(namespaced.ends_with("\t}\r\n}\r\n"));

        let generic = empty_script_content("Game.Play", "Box`2").unwrap();
        assert!(generic.contains("\tpublic class Box<T1, T2> : MonoBehaviour\r\n"));
    }

    #[test]
    fn template_bodies_carry_comment_markers_and_issue_links() {
        let global = empty_script_content("", "Brick").unwrap();
        assert!(global.contains("\t/*\r\n"));
        assert!(global.contains("\t*/\r\n"));
        assert!(global.contains("https://github.com/AssetRipper/AssetRipper/issues/653"));
        assert!(global.contains("https://github.com/AssetRipper/AssetRipper/issues/655"));
        // The whitespace-only template lines render as empty lines.
        assert!(
            global.contains("assemblies and the global metadata.\r\n\r\n\t\tAssetRipper usually")
        );
        let namespaced = empty_script_content("N", "C").unwrap();
        assert!(namespaced.contains("namespace N\r\n{\r\n"));
        assert_eq!(EMPTY_SCRIPT_COLLECTION_NAME, "EmptyScriptExportCollection");
    }

    #[test]
    fn write_plans_deduplicate_first_wins_and_skip_injected() {
        let injected = identity("", "", "");
        assert!(injected.is_injected());
        let scripts = [
            identity("A", "N", "Asm"),
            identity("A", "N", "Asm"),
            injected.clone(),
            identity("B", "", "Asm"),
            identity("A", "N", "Other"),
        ];
        assert_eq!(
            plan_empty_script_writes(scripts.into_iter().collect()),
            [
                identity("A", "N", "Asm"),
                identity("B", "", "Asm"),
                identity("A", "N", "Other")
            ]
        );
    }

    #[test]
    fn assembly_definition_gates_follow_names_and_versions() {
        for name in [
            "Assembly-CSharp",
            "Assembly-CSharp-firstpass",
            "Assembly-CSharp-Editor",
            "Assembly-CSharp-Editor-firstpass",
            "Assembly-UnityScript",
            "Assembly-UnityScript-firstpass",
        ] {
            assert!(is_predefined_assembly(name), "{name}");
        }
        assert!(!is_predefined_assembly(
            "Assembly-CSharp-Editor-firstpass-extra"
        ));
        assert!(!is_predefined_assembly("MyGame"));
        assert!(emit_assembly_definitions(true, version("2017.3.0f1")));
        assert!(emit_assembly_definitions(true, version("2022.3.1f1")));
        assert!(!emit_assembly_definitions(true, version("2017.2.5f1")));
        assert!(!emit_assembly_definitions(false, version("2022.3.1f1")));
    }
}
