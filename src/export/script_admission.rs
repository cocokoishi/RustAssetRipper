//! Script exporter admission state and per-script write discipline.
//!
//! The decompiler, assembly manager, and script contents stay outside; only the
//! caller-fact decisions are pinned here:
//!
//! - `ScriptExporter.TryCreateCollection` is stateful: the first script admitted sets the
//!   `HasDecompiled` flag and yields a full collection when assemblies are available or
//!   an empty-script collection otherwise; every later script yields a single redirect to
//!   the shared export pointer; non-scripts are refused. Callers must drive admissions
//!   in encounter order through [`ScriptExporterState`].
//! - `ScriptExportCollection` registers every bundle script first-wins by identity for
//!   duplicate redirection, but only scripts whose assembly decompiles and whose content
//!   the manager holds enter the export set.
//! - Write discipline differs by collection: the empty flow overwrites every `.cs` file
//!   and always writes the sidecar, while the full flow skips existing files and logs an
//!   error instead of writing a sidecar whose `.meta` already exists.

use super::script_export_flow::AssemblyExportType;

/// What `ScriptExporter.TryCreateCollection` builds for one asset.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ScriptTryCreate {
    /// First script with assemblies: decompile the project and emit empty stubs.
    ScriptCollection,
    /// First script without assemblies: emit empty stubs only.
    EmptyScriptCollection,
    /// Any later script: redirect to the shared export pointer.
    SingleRedirect,
    /// Not a script: no collection.
    Refused,
}

/// The stateful `ScriptExporter` admission flag.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct ScriptExporterState {
    /// Whether a first script was already admitted.
    has_decompiled: bool,
}

impl ScriptExporterState {
    /// A fresh exporter, before any script is seen.
    pub const fn new() -> Self {
        Self {
            has_decompiled: false,
        }
    }

    /// Mirror one `TryCreateCollection` call: non-scripts refuse without touching the
    /// flag; the first script sets it and selects the collection by assembly
    /// availability; later scripts redirect.
    pub fn try_create(&mut self, is_script: bool, assembly_set: bool) -> ScriptTryCreate {
        if !is_script {
            return ScriptTryCreate::Refused;
        }
        if self.has_decompiled {
            return ScriptTryCreate::SingleRedirect;
        }
        self.has_decompiled = true;
        if assembly_set {
            ScriptTryCreate::ScriptCollection
        } else {
            ScriptTryCreate::EmptyScriptCollection
        }
    }
}

/// `ScriptExportCollection.ShouldExport`: only scripts in decompiling assemblies whose
/// content the manager holds enter the export set.
pub const fn should_export_script(
    export_type_decompiles: bool,
    script_present_in_manager: bool,
) -> bool {
    export_type_decompiles && script_present_in_manager
}

/// Whether an assembly export type decompiles, for [`should_export_script`] callers.
pub const fn export_type_decompiles(export_type: AssemblyExportType) -> bool {
    matches!(export_type, AssemblyExportType::Decompile)
}

/// First-wins script registry mirroring the constructor dedup dictionaries.
///
/// Both collection constructors keep the first script seen per identity for duplicate
/// redirection; only the full collection additionally gates the export set.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScriptDedupRegistry {
    /// Identities in first-seen order.
    seen: Vec<(String, String, String)>,
}

impl ScriptDedupRegistry {
    /// Register one script identity; reports whether it was first seen.
    pub fn register(&mut self, class: &str, namespace: &str, assembly: &str) -> bool {
        if self
            .seen
            .iter()
            .any(|entry| entry.0 == class && entry.1 == namespace && entry.2 == assembly)
        {
            return false;
        }
        self.seen
            .push((class.to_owned(), namespace.to_owned(), assembly.to_owned()));
        true
    }

    /// How many distinct identities are registered.
    pub fn distinct_count(&self) -> usize {
        self.seen.len()
    }
}

/// The empty flow overwrites unconditionally; the full flow skips existing files.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ScriptFileWrite {
    /// `WriteAllText` regardless of existence (empty flow).
    Overwrite,
    /// Write only when absent (full flow).
    WriteIfAbsent,
    /// Already present: skip silently (full flow).
    SkipExists,
}

/// Plan one full-flow `.cs` write from file existence.
pub const fn plan_full_script_file_write(file_exists: bool) -> ScriptFileWrite {
    if file_exists {
        ScriptFileWrite::SkipExists
    } else {
        ScriptFileWrite::WriteIfAbsent
    }
}

#[cfg(test)]
mod tests {
    use super::{
        export_type_decompiles, plan_full_script_file_write, should_export_script,
        ScriptDedupRegistry, ScriptExporterState, ScriptFileWrite, ScriptTryCreate,
    };
    use crate::export::script_export_flow::AssemblyExportType;

    #[test]
    fn try_create_state_machine_matches_exporter_flag_order() {
        let mut state = ScriptExporterState::new();
        // Non-scripts refuse without touching the flag.
        assert_eq!(state.try_create(false, true), ScriptTryCreate::Refused);
        // The first script selects by assembly availability and sets the flag.
        assert_eq!(
            state.try_create(true, true),
            ScriptTryCreate::ScriptCollection
        );
        // Every later script redirects, whatever the assemblies offer.
        assert_eq!(
            state.try_create(true, true),
            ScriptTryCreate::SingleRedirect
        );
        assert_eq!(
            state.try_create(true, false),
            ScriptTryCreate::SingleRedirect
        );
        assert_eq!(state.try_create(false, true), ScriptTryCreate::Refused);
        // Without assemblies the first script takes the empty-script collection.
        let mut unset = ScriptExporterState::new();
        assert_eq!(
            unset.try_create(true, false),
            ScriptTryCreate::EmptyScriptCollection
        );
        assert_eq!(
            unset.try_create(true, true),
            ScriptTryCreate::SingleRedirect
        );
    }

    #[test]
    fn export_set_gates_on_decompile_and_presence() {
        assert!(should_export_script(true, true));
        assert!(!should_export_script(true, false));
        assert!(!should_export_script(false, true));
        assert!(!should_export_script(false, false));
        assert!(export_type_decompiles(AssemblyExportType::Decompile));
        assert!(!export_type_decompiles(AssemblyExportType::Save));
        assert!(!export_type_decompiles(AssemblyExportType::Skip));
    }

    #[test]
    fn dedup_registers_first_wins_in_encounter_order() {
        let mut registry = ScriptDedupRegistry::default();
        assert!(registry.register("Player", "Game", "Assembly-CSharp"));
        assert!(!registry.register("Player", "Game", "Assembly-CSharp"));
        assert!(registry.register("Player", "Other", "Assembly-CSharp"));
        assert_eq!(registry.distinct_count(), 2);
    }

    #[test]
    fn file_discipline_differs_between_empty_and_full_flows() {
        // The full flow skips existing files; the empty flow has no such check and
        // always overwrites through WriteAllText.
        assert_eq!(
            plan_full_script_file_write(true),
            ScriptFileWrite::SkipExists
        );
        assert_eq!(
            plan_full_script_file_write(false),
            ScriptFileWrite::WriteIfAbsent
        );
        assert_ne!(ScriptFileWrite::Overwrite, ScriptFileWrite::SkipExists);
    }
}
