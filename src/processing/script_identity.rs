//! Script identity ported from `AssetRipper.Import.Structure.Assembly`.
//!
//! Covers the first audited slice of `TYPE-SCRIPT-001`: `ScriptIdentifier`
//! (construction validation, `ToUniqueName` overloads, `ToString`, `IsDefault`,
//! `UniqueName`), the `ScriptingBackend` discriminants, the `MonoManager.IsMonoAssembly`
//! file-name predicate and mscorlib discovery (`TryGetMscorlibPath`), the pure string kernels
//! behind `MonoScriptExtensions.GetFullName` and `GetValidAssemblyName`, and the
//! `PropertiesHash` fallback bytes behind `GetPropertiesHash` (the
//! `IMonoScript`/assembly-manager lookups themselves stay outside: they need live
//! managed assemblies).
//!
//! Differential evidence: `src/io/script_identity_fixture.txt` (probe
//! `Temp/opencode/scriptidprobe` against the shipped `AssetRipper.Import.dll` and
//! `AssetRipper.IO.Files.dll`, SDK 10.0.401). The null-namespace rejection arm has
//! no Rust counterpart: a Rust `&str` cannot be null, so only the empty-assembly
//! and empty-name rejections are modeled.

use std::error::Error;
use std::fmt::{self, Display};

use crate::io::special_names::{fix_assembly_name, remove_assembly_file_extension};
use crate::serialized_file::metadata::SerializedTypeReference;

/// `ScriptingBackend`: the source scripting backend discriminants.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(i32)]
pub enum ScriptingBackend {
    /// Backend could not be determined.
    Unknown = 0,
    /// Mono scripting backend.
    Mono = 1,
    /// IL2Cpp scripting backend.
    IL2Cpp = 2,
}

impl ScriptingBackend {
    /// The inverse of the implicit enum-to-int conversion; unknown values yield `None`.
    pub fn from_discriminant(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Unknown),
            1 => Some(Self::Mono),
            2 => Some(Self::IL2Cpp),
            _ => None,
        }
    }
}

/// Why a script identifier cannot be constructed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScriptIdError {
    /// The assembly name is empty (`ArgumentException` in the source).
    EmptyAssembly,
    /// The type name is empty (`ArgumentException` in the source).
    EmptyName,
}

impl Display for ScriptIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyAssembly => write!(formatter, "script assembly name is empty"),
            Self::EmptyName => write!(formatter, "script type name is empty"),
        }
    }
}

impl Error for ScriptIdError {}

/// The `.NET` fallback `ScriptIdentifier.ToString` renders for the default value
/// (`ValueType.ToString()`), pinned by the probe.
pub const DEFAULT_SCRIPT_IDENTIFIER_DISPLAY: &str =
    "AssetRipper.Import.Structure.Assembly.ScriptIdentifier";

/// `MonoManager.AssemblyExtension`, the file-name suffix that marks a Mono assembly.
pub const ASSEMBLY_EXTENSION: &str = ".dll";

/// `MonoManager.IsMonoAssembly`: a file is a Mono assembly candidate exactly when its
/// name ends with `.dll` under an ordinal (case-sensitive) comparison.
pub fn is_mono_assembly(file_name: &str) -> bool {
    file_name.ends_with(ASSEMBLY_EXTENSION)
}

/// `MonoManager` mscorlib discovery over caller facts (`TryGetMscorlibPath`).
///
/// The exact `mscorlib.dll` entry wins first; otherwise the first
/// case-insensitive match in iteration order wins; otherwise there is none.
/// Callers pass the discovered assemblies in enumeration order. The scan uses
/// ASCII case folding like the tree's other ordinal-ignorecase renderings.
pub fn find_mscorlib_path(assemblies: &[(String, String)]) -> Option<&str> {
    if let Some((_, path)) = assemblies.iter().find(|(name, _)| name == "mscorlib.dll") {
        return Some(path);
    }
    assemblies
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("mscorlib.dll"))
        .map(|(_, path)| path.as_str())
}

/// `MonoScriptExtensions.GetPropertiesHash` fallback over caller facts.
///
/// When the `PropertiesHash_Hash128_5` field is absent, the source expands the legacy
/// `PropertiesHash_UInt32` into the first four hash bytes in collection byte order and
/// leaves the remaining twelve bytes at the struct default (zero). `hash` is the legacy
/// value; `big_endian` is whether the owning collection reads big-endian (the source
/// checks `monoScript.Collection.EndianType`, noting most platforms are little-endian).
pub fn properties_hash_fallback_bytes(hash: u32, big_endian: bool) -> [u8; 16] {
    let words = if big_endian {
        hash.to_be_bytes()
    } else {
        hash.to_le_bytes()
    };
    [
        words[0], words[1], words[2], words[3], 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ]
}

/// `ScriptIdentifier`: an assembly-qualified script type identity.
///
/// `name` is `None` exactly for the default value (the source's null `Name`), which
/// is also the only state where `assembly`/`namespace` are empty.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ScriptIdentifier {
    /// The assembly name.
    pub assembly: String,
    /// The namespace, possibly empty.
    pub namespace: String,
    /// The type name; `None` is the default value.
    pub name: Option<String>,
}

impl ScriptIdentifier {
    /// The source constructor: assembly and name must be non-empty; the namespace
    /// must be non-null (unrepresentable, and therefore always satisfied, in Rust).
    pub fn try_new(assembly: &str, namespace: &str, name: &str) -> Result<Self, ScriptIdError> {
        if assembly.is_empty() {
            return Err(ScriptIdError::EmptyAssembly);
        }
        if name.is_empty() {
            return Err(ScriptIdError::EmptyName);
        }
        Ok(Self {
            assembly: assembly.to_owned(),
            namespace: namespace.to_owned(),
            name: Some(name.to_owned()),
        })
    }

    /// `IsDefault`: the name is absent.
    pub fn is_default(&self) -> bool {
        self.name.is_none()
    }

    /// `UniqueName` for this identifier.
    pub fn unique_name(&self) -> String {
        to_unique_name(
            &self.assembly,
            &self.namespace,
            self.name.as_deref().unwrap_or_default(),
        )
    }

    /// `ToString`: the default value renders as the .NET type name; otherwise the
    /// bare name, qualified by the namespace when it is non-empty.
    pub fn to_display(&self) -> String {
        match &self.name {
            None => DEFAULT_SCRIPT_IDENTIFIER_DISPLAY.to_owned(),
            Some(name) if self.namespace.is_empty() => name.clone(),
            Some(name) => format!("{}.{}", self.namespace, name),
        }
    }
}

/// `ScriptIdentifier.ToUniqueName(assembly, namespace, name)`.
pub fn to_unique_name(assembly: &str, namespace: &str, name: &str) -> String {
    if namespace.is_empty() {
        format!("[{assembly}]{name}")
    } else {
        format!("[{assembly}]{namespace}.{name}")
    }
}

/// `ScriptIdentifier.ToUniqueName(assembly, fullName)`.
pub fn to_unique_name_full(assembly: &str, full_name: &str) -> String {
    format!("[{assembly}]{full_name}")
}

/// `MonoScriptExtensions.GetFullName` over caller facts: the bare class name, or
/// `namespace.class` when a namespace is present.
pub fn get_full_name(namespace: &str, class_name: &str) -> String {
    if namespace.is_empty() {
        class_name.to_owned()
    } else {
        format!("{namespace}.{class_name}")
    }
}

/// `MonoScriptExtensions.GetValidAssemblyName` over caller facts: the fixed
/// assembly name, defaulting an empty result to `Assembly-CSharp`.
pub fn get_valid_assembly_name(assembly: &str) -> String {
    let fixed = fix_assembly_name(assembly);
    if fixed.is_empty() {
        "Assembly-CSharp".to_owned()
    } else {
        fixed
    }
}

/// The `TypeTreeResolver.TryGetSerializableType` failure when no reference matches.
pub const TYPE_REFERENCE_NOT_FOUND: &str =
    "The type reference was not found in the set of available type trees.";

/// The `TypeTreeResolver.TryGetSerializableType` failure when the matched `OldType`
/// cannot build nodes. Node building itself stays with the TypeTree reader.
pub const TYPE_TREE_NODE_BUILD_FAILED: &str = "Failed to create nodes from the type tree.";

/// The `TypeTreeResolver` three-way match predicate: class name, namespace, and the
/// extension-stripped assembly name, all compared ordinally.
///
/// Absent Rust fields read as empty, matching the source `Utf8String.Empty` defaults.
/// A default identifier (absent name) never matches: the source compares against a null
/// name, which no stored class name equals.
pub fn type_reference_matches_script(
    reference: &SerializedTypeReference,
    script: &ScriptIdentifier,
) -> bool {
    let Some(name) = script.name.as_deref() else {
        return false;
    };
    reference.class_name.as_deref().unwrap_or_default() == name
        && reference.namespace.as_deref().unwrap_or_default() == script.namespace
        && remove_assembly_file_extension(reference.assembly_name.as_deref().unwrap_or_default())
            == script.assembly
}

/// The `TypeTreeResolver` linear scan: the index of the first matching reference, or
/// `None` with `TYPE_REFERENCE_NOT_FOUND` as the failure reason.
pub fn find_type_reference_index(
    references: &[SerializedTypeReference],
    script: &ScriptIdentifier,
) -> Option<usize> {
    references
        .iter()
        .position(|reference| type_reference_matches_script(reference, script))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_rows(kind: &str) -> Vec<Vec<String>> {
        let fixture = include_str!("../io/script_identity_fixture.txt");
        let mut rows = Vec::new();
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<String> = line.split('|').map(str::to_owned).collect();
            if parts.first().is_some_and(|first| first == kind) {
                rows.push(parts);
            }
        }
        rows
    }

    #[test]
    fn backend_discriminants_match_the_reference_runtime() {
        let mut rows = 0;
        for parts in fixture_rows("BACKEND") {
            assert_eq!(parts.len(), 3, "{parts:?}");
            let discriminant: i32 = parts[2].parse().unwrap();
            let backend = ScriptingBackend::from_discriminant(discriminant)
                .expect("a known backend discriminant");
            assert_eq!(
                match backend {
                    ScriptingBackend::Unknown => "Unknown",
                    ScriptingBackend::Mono => "Mono",
                    ScriptingBackend::IL2Cpp => "IL2Cpp",
                },
                parts[1],
                "{parts:?}"
            );
            rows += 1;
        }
        assert_eq!(rows, 3, "all probed backend rows are asserted");
        assert_eq!(ScriptingBackend::from_discriminant(3), None);
        assert_eq!(ScriptingBackend::Unknown as i32, 0);
        assert_eq!(ScriptingBackend::Mono as i32, 1);
        assert_eq!(ScriptingBackend::IL2Cpp as i32, 2);
    }

    #[test]
    fn mono_assembly_predicate_matches_the_source_rule() {
        assert_eq!(ASSEMBLY_EXTENSION, ".dll");
        assert!(is_mono_assembly("Assembly-CSharp.dll"));
        assert!(is_mono_assembly("mscorlib.dll"));
        // Ordinal comparison: case, trailing dots, and other extensions do not match.
        assert!(!is_mono_assembly("Assembly-CSharp.DLL"));
        assert!(!is_mono_assembly("Assembly-CSharp.dll "));
        assert!(!is_mono_assembly("GameAssembly.so"));
        assert!(!is_mono_assembly("global-metadata.dat"));
        assert!(!is_mono_assembly(""));
        assert!(!is_mono_assembly(".dll.bak"));
    }

    #[test]
    fn mscorlib_discovery_prefers_the_exact_entry() {
        fn assemblies(names: &[(&str, &str)]) -> Vec<(String, String)> {
            names
                .iter()
                .map(|(name, path)| ((*name).to_owned(), (*path).to_owned()))
                .collect()
        }
        // The exact entry wins even when a case-variant comes first.
        let listed = assemblies(&[
            ("MSCorLib.DLL", "/lib/upper.dll"),
            ("mscorlib.dll", "/lib/exact.dll"),
        ]);
        assert_eq!(find_mscorlib_path(&listed), Some("/lib/exact.dll"));
        // Otherwise the first case-insensitive match in order wins.
        let listed = assemblies(&[
            ("MSCorLib.DLL", "/lib/first.dll"),
            ("Mscorlib.dll", "/lib/second.dll"),
        ]);
        assert_eq!(find_mscorlib_path(&listed), Some("/lib/first.dll"));
        // A lone exact entry matches, and an empty table matches nothing.
        let listed = assemblies(&[("mscorlib.dll", "/lib/only.dll")]);
        assert_eq!(find_mscorlib_path(&listed), Some("/lib/only.dll"));
        assert_eq!(find_mscorlib_path(&[]), None);
        // Near misses are not matches.
        let listed = assemblies(&[
            ("mscorlib.dll.bak", "/lib/bak.dll"),
            ("Assembly-CSharp.dll", "/lib/game.dll"),
        ]);
        assert_eq!(find_mscorlib_path(&listed), None);
    }

    #[test]
    fn identifier_names_match_the_reference_runtime() {
        let mut sid_rows = 0;
        for parts in fixture_rows("SID") {
            assert_eq!(parts.len(), 6, "{parts:?}");
            let id = ScriptIdentifier::try_new(&parts[1], &parts[2], &parts[3]).unwrap();
            assert!(!id.is_default(), "{parts:?}");
            assert_eq!(id.to_display(), parts[4], "{parts:?}");
            assert_eq!(id.unique_name(), parts[5], "{parts:?}");
            sid_rows += 1;
        }
        assert_eq!(sid_rows, 3, "all probed identifier rows are asserted");
        let mut unique3 = Vec::new();
        for parts in fixture_rows("UNIQ3") {
            assert_eq!(parts.len(), 2, "{parts:?}");
            unique3.push(parts[1].clone());
        }
        assert_eq!(unique3, vec!["[A]N", "[A]G.N"], "probed overload outputs");
        assert_eq!(to_unique_name("A", "", "N"), "[A]N");
        assert_eq!(to_unique_name("A", "G", "N"), "[A]G.N");
        for parts in fixture_rows("UNIQ2") {
            assert_eq!(parts.len(), 2, "{parts:?}");
            assert_eq!(parts[1], "[A]G.N", "{parts:?}");
        }
        assert_eq!(to_unique_name_full("A", "G.N"), "[A]G.N");
    }

    #[test]
    fn default_identifier_matches_the_reference_runtime() {
        for parts in fixture_rows("ISDEF") {
            assert_eq!(parts.len(), 2, "{parts:?}");
            assert_eq!(parts[1], "1", "{parts:?}");
        }
        let def = ScriptIdentifier::default();
        assert!(def.is_default(), "the default value is default");
        for parts in fixture_rows("DEFTOSTRING") {
            assert_eq!(parts.len(), 2, "{parts:?}");
            assert_eq!(def.to_display(), parts[1], "{parts:?}");
        }
        assert_eq!(def.to_display(), DEFAULT_SCRIPT_IDENTIFIER_DISPLAY);
    }

    #[test]
    fn constructor_rejections_match_the_reference_runtime() {
        let mut rows = 0;
        for parts in fixture_rows("THROW") {
            assert_eq!(parts.len(), 5, "{parts:?}");
            let namespace = if parts[2] == "<null>" {
                // Rust strings cannot be null; the source rejects a null
                // namespace with ArgumentNullException before any other check.
                // An empty namespace is the closest representable input and is
                // accepted, which is exactly the behavioral gap this documents.
                String::new()
            } else {
                parts[2].clone()
            };
            let result = ScriptIdentifier::try_new(&parts[1], &namespace, &parts[3]);
            match parts[1].is_empty() {
                true => assert_eq!(result, Err(ScriptIdError::EmptyAssembly), "{parts:?}"),
                false if parts[3].is_empty() => {
                    assert_eq!(result, Err(ScriptIdError::EmptyName), "{parts:?}")
                }
                false => assert!(result.is_ok(), "{parts:?}"),
            }
            rows += 1;
        }
        assert_eq!(rows, 3, "all probed rejection rows are asserted");
    }

    #[test]
    fn full_and_assembly_names_follow_the_source_rules() {
        assert_eq!(get_full_name("", "Player"), "Player");
        assert_eq!(get_full_name("Game", "Player"), "Game.Player");
        assert_eq!(get_valid_assembly_name(""), "Assembly-CSharp");
        assert_eq!(get_valid_assembly_name("MyLib.dll"), "MyLib");
        assert_eq!(get_valid_assembly_name("CSharp"), "Assembly - CSharp");
    }

    fn script_reference(
        class: Option<&str>,
        namespace: Option<&str>,
        assembly: Option<&str>,
    ) -> SerializedTypeReference {
        SerializedTypeReference {
            raw_type_id: 0,
            type_id_for_hash: 0,
            is_stripped_type: false,
            script_type_index: 0,
            script_id: None,
            old_type_hash: None,
            type_tree: None,
            class_name: class.map(str::to_owned),
            namespace: namespace.map(str::to_owned),
            assembly_name: assembly.map(str::to_owned),
        }
    }

    /// Differential against the shipped `AssetRipper.Import.dll` resolver
    /// (probe `Temp/opencode/scriptidprobe`, SDK 10.0.401): the `RESOLVE` rows pin
    /// both failure reasons. The `broken-tree` row proves the three-way match fired
    /// (a miss would report not-found), so the first test replays that match.
    #[test]
    fn type_reference_scan_matches_the_reference_runtime() {
        let mut rows = 0;
        for parts in fixture_rows("RESOLVE") {
            assert_eq!(parts.len(), 4, "{parts:?}");
            match parts[1].as_str() {
                "empty" => {
                    assert_eq!(parts[2], "0", "{parts:?}");
                    assert_eq!(parts[3], TYPE_REFERENCE_NOT_FOUND, "{parts:?}");
                }
                "broken-tree" => {
                    assert_eq!(parts[2], "0", "{parts:?}");
                    assert_eq!(parts[3], TYPE_TREE_NODE_BUILD_FAILED, "{parts:?}");
                }
                other => panic!("unexpected resolve case {other}"),
            }
            rows += 1;
        }
        assert_eq!(rows, 2, "both probed resolve rows are asserted");
        // The probed broken-tree reference: Player/""/Assembly-CSharp.
        let script = ScriptIdentifier::try_new("Assembly-CSharp", "", "Player").unwrap();
        let references = vec![script_reference(
            Some("Player"),
            Some(""),
            Some("Assembly-CSharp"),
        )];
        assert_eq!(find_type_reference_index(&references, &script), Some(0));
        assert!(find_type_reference_index(&[], &script).is_none());
    }

    #[test]
    fn type_reference_match_covers_assembly_stripping_case_and_absence() {
        let script = ScriptIdentifier::try_new("Assembly-CSharp", "Game", "Player").unwrap();
        // The resolver strips the stored extension before comparing.
        assert!(type_reference_matches_script(
            &script_reference(Some("Player"), Some("Game"), Some("Assembly-CSharp.dll")),
            &script
        ));
        // Ordinal comparison: case differs, no match.
        assert!(!type_reference_matches_script(
            &script_reference(Some("player"), Some("Game"), Some("Assembly-CSharp")),
            &script
        ));
        // Absent Rust fields read as empty like Utf8String.Empty.
        let empty_ns = ScriptIdentifier::try_new("A", "", "N").unwrap();
        assert!(type_reference_matches_script(
            &script_reference(Some("N"), None, Some("A")),
            &empty_ns
        ));
        // A default identifier never matches, even against empty fields.
        assert!(!type_reference_matches_script(
            &script_reference(Some(""), Some(""), Some("")),
            &ScriptIdentifier::default()
        ));
        // First match wins in scan order.
        let first = script_reference(Some("Player"), Some("Game"), Some("Assembly-CSharp"));
        let references = vec![script_reference(Some("Other"), None, None), first];
        assert_eq!(find_type_reference_index(&references, &script), Some(1));
    }

    #[test]
    fn properties_hash_fallback_expands_in_collection_byte_order() {
        assert_eq!(
            properties_hash_fallback_bytes(0x1234_5678, false),
            [0x78, 0x56, 0x34, 0x12, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            properties_hash_fallback_bytes(0x1234_5678, true),
            [0x12, 0x34, 0x56, 0x78, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(properties_hash_fallback_bytes(0, true), [0; 16]);
        assert_eq!(
            properties_hash_fallback_bytes(u32::MAX, false),
            [0xFF, 0xFF, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        );
    }
}
