//! Script identity ported from `AssetRipper.Import.Structure.Assembly`.
//!
//! Covers the first audited slice of `TYPE-SCRIPT-001`: `ScriptIdentifier`
//! (construction validation, `ToUniqueName` overloads, `ToString`, `IsDefault`,
//! `UniqueName`), the `ScriptingBackend` discriminants, and the pure string kernels
//! behind `MonoScriptExtensions.GetFullName` and `GetValidAssemblyName` (the
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

use crate::io::special_names::fix_assembly_name;

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
}
