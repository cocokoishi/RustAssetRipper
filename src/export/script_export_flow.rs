//! Managed script export flow ported from `ScriptExporter` and `ScriptExportCollection`.
//!
//! Assemblies take one of three paths selected by `GetExportType`, checked in order: a
//! reference assembly is skipped, an unset assembly manager decompiles everything, the
//! `Decompiled` mode decompiles, `Hybrid` decompiles only the six predefined assemblies and
//! saves the rest as DLLs, and every other mode saves. Decompiled assemblies land in
//! `Assets/<Scripts|Plugins>/<Name>`, saved ones in `Assets/Plugins/<Name>.dll` (appending
//! the ordinal `.dll` only when missing) with a plugin importer and an assembly-GUID sidecar.
//!
//! MonoScripts deduplicate by script identity with first registration winning; only scripts
//! whose assembly decompiles and whose content the manager holds enter the export set. Each
//! exported script writes its `.cs` file only when absent (unlike the empty-script flow,
//! which overwrites) and writes its sidecar only when no `.meta` exists, logging an error
//! otherwise. Assembly-definition details accumulate per assembly name and emit `{name}.asmdef`
//! files only with details present, version 2017.3+, and a non-predefined assembly.
//!
//! Script pointers resolve by the same export type: decompiled scripts use the MonoScript
//! main export ID with the script GUID, skipped ones the script file ID with the reference
//! GUID, and saved ones the script file ID with the assembly GUID, all as `Meta` pointers.
//! Collection pointer requests reject local pointers and non-first-script assets before
//! routing to the first script. The MonoScript class ID is 115: every MonoScript-touching
//! file imports the generated `ClassID_115` namespace, so the decompiled file ID is
//! `GetMainExportID(115, 0)`.
//!
//! The `.asmdef` body is `{"name","references","allowUnsafeCode":true}` with references in
//! module order minus null and reference-known entries. String escaping covers ASCII
//! controls, quotes, and backslashes; non-ASCII encoder details follow System.Text.Json and
//! stay open. Decompilation, assembly saving, and reference-dictionary construction need the
//! assembly manager and stay outside; all selections below run on caller-supplied facts.

use super::empty_script::MonoScriptIdentity;
use super::ids::{get_main_export_id, ExportIdError};
use super::settings::ScriptExportMode;
use crate::processing::scene_naming::SceneUnityVersion;

use std::error::Error;
use std::fmt::{self, Display, Formatter};

/// The MonoScript class ID: every MonoScript-touching file imports `ClassID_115`.
pub const MONO_SCRIPT_CLASS_ID: i32 = 115;

/// `ScriptExporter.MonoScriptDecompiledFileID`: `GetMainExportID(115, 0)`.
pub const MONO_SCRIPT_DECOMPILED_FILE_ID: i64 = 11_500_000;

/// The ordinal assembly file extension appended by the save path.
pub const ASSEMBLY_EXTENSION: &str = ".dll";

/// `AssemblyExportType` in source declaration order.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AssemblyExportType {
    /// The assembly should be decompiled.
    Decompile,
    /// The assembly should be saved as is.
    Save,
    /// The assembly is a framework assembly and should be skipped.
    Skip,
}

/// `ScriptExporter.GetExportType` for one assembly name.
///
/// The checks run in source order: reference assemblies skip before anything else.
pub fn assembly_export_type(
    is_reference_assembly: bool,
    assembly_manager_is_set: bool,
    mode: ScriptExportMode,
    assembly_name: &str,
) -> AssemblyExportType {
    use super::empty_script::is_predefined_assembly;
    if is_reference_assembly {
        return AssemblyExportType::Skip;
    }
    if !assembly_manager_is_set {
        return AssemblyExportType::Decompile;
    }
    match mode {
        ScriptExportMode::Decompiled => AssemblyExportType::Decompile,
        ScriptExportMode::Hybrid => {
            if is_predefined_assembly(assembly_name) {
                AssemblyExportType::Decompile
            } else {
                AssemblyExportType::Save
            }
        }
        _ => AssemblyExportType::Save,
    }
}

/// Check `ScriptExportMode` coverage: which Rust variants exist for the mode switch.
pub fn script_export_mode_name(mode: ScriptExportMode) -> &'static str {
    match mode {
        ScriptExportMode::Decompiled => "Decompiled",
        ScriptExportMode::Hybrid => "Hybrid",
        ScriptExportMode::DllExportWithRenaming => "DllExportWithRenaming",
        ScriptExportMode::DllExportWithoutRenaming => "DllExportWithoutRenaming",
    }
}

/// Which GUID source a script pointer uses; ID and file layout stay with the caller.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ScriptPointerGuid {
    /// `CalculateScriptGuid(script)` with the decompiled file ID.
    ScriptGuid,
    /// The reference dictionary GUID with the script file ID.
    ReferenceGuid,
    /// `CalculateAssemblyGuid` with the script file ID.
    AssemblyGuid,
}

/// `ScriptExporter.CreateExportPointer` routing by export type.
pub const fn script_pointer_guid(export_type: AssemblyExportType) -> ScriptPointerGuid {
    match export_type {
        AssemblyExportType::Decompile => ScriptPointerGuid::ScriptGuid,
        AssemblyExportType::Skip => ScriptPointerGuid::ReferenceGuid,
        AssemblyExportType::Save => ScriptPointerGuid::AssemblyGuid,
    }
}

/// The decompiled file ID behind `Decompile` pointers.
pub fn decompiled_file_id() -> Result<i64, ExportIdError> {
    get_main_export_id(MONO_SCRIPT_CLASS_ID, 0)
}

/// Rejection of a script-collection pointer request, mirroring the source throws.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ScriptCollectionPointerError {
    /// `ThrowIfLocal`: local pointers throw `NotSupportedException`.
    LocalPointerNotSupported,
    /// `ThrowIfNotAsset`: only the reference-identical first script is accepted.
    NotCollectionAsset,
}

impl Display for ScriptCollectionPointerError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::LocalPointerNotSupported => {
                formatter.write_str("local script collection pointers are not supported")
            }
            Self::NotCollectionAsset => {
                formatter.write_str("script collection pointers require the first script asset")
            }
        }
    }
}

impl Error for ScriptCollectionPointerError {}

/// Mirror `ScriptExportCollectionBase.CreateExportPointer` target selection.
///
/// Local requests and non-first-script assets are rejected in source order; every
/// accepted asset resolves to the exporter's first-script pointer, which stays
/// caller-built alongside its GUID facts.
pub const fn script_collection_pointer_target(
    is_local: bool,
    is_first_script: bool,
) -> Result<(), ScriptCollectionPointerError> {
    if is_local {
        return Err(ScriptCollectionPointerError::LocalPointerNotSupported);
    }
    if !is_first_script {
        return Err(ScriptCollectionPointerError::NotCollectionAsset);
    }
    Ok(())
}

/// One dedup input: identity plus the two `ShouldExport` facts.
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptExportCandidate {
    pub identity: MonoScriptIdentity,
    /// `GetExportType` already resolved for this script's assembly.
    pub export_type: AssemblyExportType,
    /// `IsScriptPresents`: the manager holds the script content.
    pub present: bool,
}

/// Mirror the constructor: dedup by identity first-wins, then keep decompilable presents.
pub fn plan_script_collection_scripts(
    candidates: Vec<ScriptExportCandidate>,
) -> (Vec<MonoScriptIdentity>, Vec<MonoScriptIdentity>) {
    let mut unique: Vec<&ScriptExportCandidate> = Vec::new();
    for candidate in &candidates {
        if !unique
            .iter()
            .any(|known| known.identity == candidate.identity)
        {
            unique.push(candidate);
        }
    }
    let export = unique
        .iter()
        .filter(|candidate| {
            candidate.export_type == AssemblyExportType::Decompile && candidate.present
        })
        .map(|candidate| candidate.identity.clone())
        .collect();
    let all = unique
        .iter()
        .map(|candidate| candidate.identity.clone())
        .collect();
    (all, export)
}

/// What one exported script's file/sidecar guards decide.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ScriptFileActions {
    /// No `.cs` exists yet: write the decompiled (or empty) content.
    pub write_file: bool,
    /// No `.meta` exists yet: export the sidecar.
    pub write_meta: bool,
    /// A `.meta` already exists: log the metafile error instead of exporting it.
    pub log_meta_error: bool,
}

/// The per-script existence guards: write-if-absent for the file, error-if-present for the
/// sidecar.
pub const fn script_file_actions(file_exists: bool, meta_exists: bool) -> ScriptFileActions {
    ScriptFileActions {
        write_file: !file_exists,
        write_meta: !meta_exists,
        log_meta_error: meta_exists,
    }
}

/// `SpecialFileNames.AddAssemblyFileExtension`: append `.dll` unless ordinally suffixed.
pub fn add_assembly_file_extension(assembly: &str) -> String {
    if assembly.ends_with(ASSEMBLY_EXTENSION) {
        assembly.to_owned()
    } else {
        format!("{assembly}{ASSEMBLY_EXTENSION}")
    }
}

/// The `{name}.asmdef` file name for one assembly.
pub fn assembly_definition_file_name(assembly_name: &str) -> String {
    format!("{assembly_name}.asmdef")
}

/// Escape one string the way the definition serializer does for ASCII payloads.
///
/// Non-ASCII encoder details follow System.Text.Json and stay open; assembly and reference
/// names are ASCII identifiers in practice.
pub fn escape_asmdef_string(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            '\u{08}' => escaped.push_str("\\b"),
            '\u{0C}' => escaped.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                escaped.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => escaped.push(c),
        }
    }
    escaped
}

/// `AssemblyDefinitionExporter` reference filter: drop null and reference-known entries,
/// keep module order with duplicates as the source does.
pub fn filter_assembly_references<'a>(
    references: &[Option<&'a str>],
    known: &[&str],
) -> Vec<&'a str> {
    references
        .iter()
        .filter_map(|reference| *reference)
        .filter(|reference| !known.contains(reference))
        .collect()
}

/// The `.asmdef` body for one assembly name and its filtered references, using the source
/// `System.Text.Json` indented layout and Windows line endings.
pub fn assembly_definition_json(assembly_name: &str, references: &[&str]) -> String {
    let body = if references.is_empty() {
        "[]".to_owned()
    } else {
        format!(
            "[\r\n    {}\r\n  ]",
            references
                .iter()
                .map(|reference| format!("\"{}\"", escape_asmdef_string(reference)))
                .collect::<Vec<_>>()
                .join(",\r\n    ")
        )
    };
    format!(
        "{{\r\n  \"name\": \"{}\",\r\n  \"references\": {},\r\n  \"allowUnsafeCode\": true\r\n}}",
        escape_asmdef_string(assembly_name),
        body
    )
}

/// The assembly-definition emission gate shared with the empty-script flow.
pub fn emit_assembly_definitions(has_details: bool, version: SceneUnityVersion) -> bool {
    super::empty_script::emit_assembly_definitions(has_details, version)
}

#[cfg(test)]
mod tests {
    use super::super::settings::ScriptExportMode;
    use super::{
        add_assembly_file_extension, assembly_definition_file_name, assembly_definition_json,
        assembly_export_type, decompiled_file_id, emit_assembly_definitions,
        filter_assembly_references, plan_script_collection_scripts,
        script_collection_pointer_target, script_export_mode_name, script_file_actions,
        script_pointer_guid, AssemblyExportType, ScriptCollectionPointerError,
        ScriptExportCandidate, ScriptFileActions, ScriptPointerGuid, ASSEMBLY_EXTENSION,
        MONO_SCRIPT_CLASS_ID, MONO_SCRIPT_DECOMPILED_FILE_ID,
    };
    use crate::export::empty_script::MonoScriptIdentity;
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
    fn export_types_follow_the_source_check_order() {
        use AssemblyExportType::{Decompile, Save, Skip};
        use ScriptExportMode::{Decompiled, DllExportWithoutRenaming, Hybrid};
        // Reference assemblies skip before anything else, even without a manager.
        assert_eq!(
            assembly_export_type(true, false, Decompiled, "Assembly-CSharp"),
            Skip
        );
        // An unset manager decompiles everything non-reference.
        assert_eq!(
            assembly_export_type(false, false, DllExportWithoutRenaming, "MyGame"),
            Decompile
        );
        assert_eq!(
            assembly_export_type(false, true, Decompiled, "MyGame"),
            Decompile
        );
        // Hybrid splits on the predefined list; other modes save.
        assert_eq!(
            assembly_export_type(false, true, Hybrid, "Assembly-CSharp"),
            Decompile
        );
        assert_eq!(assembly_export_type(false, true, Hybrid, "MyGame"), Save);
        assert_eq!(
            assembly_export_type(false, true, DllExportWithoutRenaming, "Assembly-CSharp"),
            Save
        );
        assert_eq!(script_export_mode_name(Hybrid), "Hybrid");
        assert_eq!(ASSEMBLY_EXTENSION, ".dll");
    }

    #[test]
    fn pointers_route_by_export_type_with_checked_ids() {
        assert_eq!(
            script_pointer_guid(AssemblyExportType::Decompile),
            ScriptPointerGuid::ScriptGuid
        );
        assert_eq!(
            script_pointer_guid(AssemblyExportType::Skip),
            ScriptPointerGuid::ReferenceGuid
        );
        assert_eq!(
            script_pointer_guid(AssemblyExportType::Save),
            ScriptPointerGuid::AssemblyGuid
        );
        // The MonoScript class ID comes from the ClassID_115 imports; the file ID is the
        // main export ID of that class.
        assert_eq!(MONO_SCRIPT_CLASS_ID, 115);
        assert_eq!(MONO_SCRIPT_DECOMPILED_FILE_ID, 11_500_000);
        assert_eq!(decompiled_file_id(), Ok(11_500_000));
    }

    #[test]
    fn collection_pointer_requests_reject_local_and_foreign_assets() {
        // The first script routes to the exporter's pointer.
        assert_eq!(script_collection_pointer_target(false, true), Ok(()));
        // Local requests throw before the asset check, even for foreign assets.
        assert_eq!(
            script_collection_pointer_target(true, true),
            Err(ScriptCollectionPointerError::LocalPointerNotSupported)
        );
        assert_eq!(
            script_collection_pointer_target(true, false),
            Err(ScriptCollectionPointerError::LocalPointerNotSupported)
        );
        assert_eq!(
            script_collection_pointer_target(false, false),
            Err(ScriptCollectionPointerError::NotCollectionAsset)
        );
        assert_eq!(
            ScriptCollectionPointerError::LocalPointerNotSupported.to_string(),
            "local script collection pointers are not supported"
        );
        assert_eq!(
            ScriptCollectionPointerError::NotCollectionAsset.to_string(),
            "script collection pointers require the first script asset"
        );
    }

    #[test]
    fn collection_scripts_deduplicate_then_filter_exports() {
        use AssemblyExportType::{Decompile, Save};
        let candidates = [
            ScriptExportCandidate {
                identity: identity("A", "N", "Asm"),
                export_type: Decompile,
                present: true,
            },
            ScriptExportCandidate {
                identity: identity("A", "N", "Asm"),
                export_type: Decompile,
                present: true,
            },
            ScriptExportCandidate {
                identity: identity("B", "", "Asm"),
                export_type: Save,
                present: true,
            },
            ScriptExportCandidate {
                identity: identity("C", "", "Asm"),
                export_type: Decompile,
                present: false,
            },
        ];
        let (all, export) = plan_script_collection_scripts(candidates.into_iter().collect());
        assert_eq!(
            all,
            [
                identity("A", "N", "Asm"),
                identity("B", "", "Asm"),
                identity("C", "", "Asm")
            ]
        );
        assert_eq!(export, [identity("A", "N", "Asm")]);
    }

    #[test]
    fn per_script_guards_write_once_and_error_on_meta_collisions() {
        // Fresh script: write both file and sidecar.
        assert_eq!(
            script_file_actions(false, false),
            ScriptFileActions {
                write_file: true,
                write_meta: true,
                log_meta_error: false,
            }
        );
        // Existing file with a fresh sidecar: sidecar only.
        assert_eq!(
            script_file_actions(true, false),
            ScriptFileActions {
                write_file: false,
                write_meta: true,
                log_meta_error: false,
            }
        );
        // Any existing sidecar logs the metafile error instead of exporting it.
        assert_eq!(
            script_file_actions(false, true),
            ScriptFileActions {
                write_file: true,
                write_meta: false,
                log_meta_error: true,
            }
        );
        assert_eq!(
            script_file_actions(true, true),
            ScriptFileActions {
                write_file: false,
                write_meta: false,
                log_meta_error: true,
            }
        );
        // The save path appends .dll only when ordinally missing.
        assert_eq!(
            add_assembly_file_extension("Assembly-CSharp"),
            "Assembly-CSharp.dll"
        );
        assert_eq!(
            add_assembly_file_extension("Assembly-CSharp.dll"),
            "Assembly-CSharp.dll"
        );
        assert_eq!(add_assembly_file_extension("Plugin.DLL"), "Plugin.DLL.dll");
    }

    #[test]
    fn assembly_definition_bodies_follow_name_references_and_shape() {
        assert_eq!(assembly_definition_file_name("MyGame"), "MyGame.asmdef");
        // Null and reference-known entries drop; order and duplicates stay.
        let references = filter_assembly_references(
            &[Some("UnityEngine"), Some("Shared"), None, Some("Shared")],
            &["UnityEngine"],
        );
        assert_eq!(references, ["Shared", "Shared"]);
        assert_eq!(
            assembly_definition_json("MyGame", &references),
            "{\r\n  \"name\": \"MyGame\",\r\n  \"references\": [\r\n    \"Shared\",\r\n    \"Shared\"\r\n  ],\r\n  \"allowUnsafeCode\": true\r\n}"
        );
        assert_eq!(
            assembly_definition_json("Empty", &[]),
            "{\r\n  \"name\": \"Empty\",\r\n  \"references\": [],\r\n  \"allowUnsafeCode\": true\r\n}"
        );
        // The empty-script flow shares the 2017.3 gate.
        assert!(emit_assembly_definitions(true, version("2017.3.0f1")));
        assert!(!emit_assembly_definitions(true, version("2017.2.5f1")));
        assert!(!emit_assembly_definitions(false, version("2022.3.1f1")));
    }
}
