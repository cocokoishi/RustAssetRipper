//! `StateConstantExtensions`.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/StateConstantExtensions.cs`. State ids come
//! from the full-path, path, or plain id field depending on the version; names are either a
//! dictionary lookup by `NameID` or the state path with any parent-machine prefix stripped.
//!
//! Also covers the caller-independent grouping math of
//! `AssetRipper.Processing/AnimatorControllers/AnimatorStateContext.cs`: the default-state
//! index, the parent-machine path derivation, and the state-index selection behind
//! `StateIndicesForStateMachine`. Asset creation and the path↔id registry stay caller-side;
//! the registry-backed overload takes its entries as caller facts.

use std::collections::HashMap;

/// A state-constant view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StateConstantView {
    /// `FullPathID`: present only from a certain version.
    pub full_path_id: Option<u32>,
    /// `PathID`: present only from a certain version.
    pub path_id: Option<u32>,
    /// `ID`.
    pub id: u32,
    /// `NameID`: present only from a certain version.
    pub name_id: Option<u32>,
    /// `WriteDefaultValues`: present only from a certain version.
    pub write_default_values: Option<bool>,
    /// The node count of each `BlendTreeConstantArray` entry.
    pub blend_tree_node_counts: Vec<usize>,
}

/// A failure while resolving a state constant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StateConstantError {
    /// The `NameID`/`ID` was not present in the string dictionary.
    MissingName {
        /// The missing key.
        key: u32,
    },
}

impl core::fmt::Display for StateConstantError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MissingName { key } => write!(formatter, "state name key {key} is missing"),
        }
    }
}

impl std::error::Error for StateConstantError {}

impl StateConstantView {
    /// `StateConstantExtensions.IsBlendTree`.
    pub fn is_blend_tree(&self) -> bool {
        match self.blend_tree_node_counts.first() {
            Some(count) => *count > 1,
            None => false,
        }
    }

    /// `StateConstantExtensions.GetWriteDefaultValues`.
    pub fn get_write_default_values(&self) -> bool {
        self.write_default_values.unwrap_or(true)
    }

    /// `StateConstantExtensions.GetId`.
    pub fn get_id(&self) -> u32 {
        match self.full_path_id {
            Some(value) => value,
            None => match self.path_id {
                Some(value) => value,
                None => self.id,
            },
        }
    }

    /// `StateConstantExtensions.GetName`.
    pub fn get_name(&self, tos: &HashMap<u32, String>) -> Result<String, StateConstantError> {
        if let Some(name_id) = self.name_id {
            return tos
                .get(&name_id)
                .cloned()
                .ok_or(StateConstantError::MissingName { key: name_id });
        }
        let state_path = tos
            .get(&self.id)
            .cloned()
            .ok_or(StateConstantError::MissingName { key: self.id })?;
        let characters: Vec<char> = state_path.chars().collect();
        if let Some(position) = characters.iter().position(|character| *character == '.') {
            if position + 1 < characters.len() {
                return Ok(characters[position + 1..].iter().collect());
            }
        }
        Ok(state_path)
    }
}

/// `AnimatorStateContext` default-state index: `DefaultState` unless it is `uint.MaxValue`,
/// which selects index 0. Downstream bounds stay caller-side.
pub fn default_state_index(default_state: u32) -> usize {
    if default_state == u32::MAX {
        0
    } else {
        default_state as usize
    }
}

/// `AnimatorStateContext.StateIndicesForStateMachine(fullPathID)`: the state indices whose
/// parent machine id matches, in order.
pub fn state_indices_for_state_machine(
    parent_machine_ids: &[u32],
    full_path_id: u32,
) -> Vec<usize> {
    parent_machine_ids
        .iter()
        .enumerate()
        .filter_map(|(index, parent)| (*parent == full_path_id).then_some(index))
        .collect()
}

/// `AnimatorStateContext.StateIndicesForStateMachine(fullPath)`: resolves the path through
/// the caller-held path↔id entries, then selects like the id overload. An unregistered
/// path selects nothing.
pub fn state_indices_for_state_machine_path(
    parent_machine_ids: &[u32],
    path_entries: &[(String, u32)],
    full_path: &str,
) -> Vec<usize> {
    match path_entries
        .iter()
        .find(|(path, _)| path == full_path)
        .map(|(_, id)| *id)
    {
        Some(full_path_id) => state_indices_for_state_machine(parent_machine_ids, full_path_id),
        None => Vec::new(),
    }
}

/// `AnimatorStateContext.MakeStateMachinePath`: strips the trailing `.StateName` from a
/// `[stateMachinePath.]StateName` full path.
///
/// Paths too short to hold the separator read `None` where the source throws, as do
/// suffix cuts that land inside a UTF-8 sequence (the source slices UTF-16 units). A
/// suffix that is not `.StateName` also reads `None`; the source slices blindly there,
/// but valid call-site pairs always match, so only malformed inputs differ.
pub fn make_state_machine_path(full_path: &str, state_name: &str) -> Option<String> {
    let end = full_path
        .len()
        .checked_sub(state_name.len())?
        .checked_sub(1)?;
    if !full_path.is_char_boundary(end) {
        return None;
    }
    let (machine_path, suffix) = full_path.split_at(end);
    if !suffix.starts_with('.') || &suffix[1..] != state_name {
        return None;
    }
    Some(machine_path.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tos() -> HashMap<u32, String> {
        HashMap::from([
            (0, "Zero".to_string()),
            (4, "Default".to_string()),
            (7, "Parent.State".to_string()),
            (8, "NoDot".to_string()),
            (9, "Trailing.".to_string()),
            (10, ".".to_string()),
        ])
    }

    #[allow(clippy::too_many_arguments)]
    fn view(
        has_full: bool,
        has_path: bool,
        has_name: bool,
        has_wdv: bool,
        full: u32,
        path: u32,
        id: u32,
        name_id: u32,
        wdv: bool,
        node_counts: Vec<usize>,
    ) -> StateConstantView {
        StateConstantView {
            full_path_id: has_full.then_some(full),
            path_id: has_path.then_some(path),
            id,
            name_id: has_name.then_some(name_id),
            write_default_values: has_wdv.then_some(wdv),
            blend_tree_node_counts: node_counts,
        }
    }

    /// Byte-for-byte differential against the reference `StateConstantExtensions` (probe
    /// `Temp/opencode/scprobe`).
    #[test]
    fn state_constant_matches_the_reference_runtime() {
        let fixture = include_str!("state_constant_fixture.txt");
        let strings = tos();
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "SC", "{line}");
            let has_full = parts[2] == "1";
            let has_path = parts[3] == "1";
            let has_name = parts[4] == "1";
            let has_wdv = parts[5] == "1";

            let make =
                |full: u32, path: u32, id: u32, name_id: u32, wdv: bool, counts: Vec<usize>| {
                    view(
                        has_full, has_path, has_name, has_wdv, full, path, id, name_id, wdv, counts,
                    )
                };

            assert_eq!(
                make(5, 6, 4, 8, false, Vec::new()).get_id(),
                parts[6].parse::<u32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                make(0, 6, 4, 8, false, Vec::new()).get_id(),
                parts[7].parse::<u32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                make(0, 0, 4, 8, false, Vec::new()).get_id(),
                parts[8].parse::<u32>().unwrap(),
                "{line}"
            );
            assert_eq!(
                make(0, 0, 4, 8, true, Vec::new()).get_write_default_values(),
                parts[9] == "1",
                "{line}"
            );
            assert_eq!(
                make(0, 0, 4, 8, false, Vec::new()).get_write_default_values(),
                parts[10] == "1",
                "{line}"
            );
            assert_eq!(
                make(0, 0, 4, 8, false, Vec::new()).is_blend_tree(),
                parts[11] == "1",
                "{line}"
            );
            assert_eq!(
                make(0, 0, 4, 8, false, vec![1]).is_blend_tree(),
                parts[12] == "1",
                "{line}"
            );
            assert_eq!(
                make(0, 0, 4, 8, false, vec![2]).is_blend_tree(),
                parts[13] == "1",
                "{line}"
            );
            assert_eq!(
                make(0, 0, 4, 8, false, Vec::new())
                    .get_name(&strings)
                    .unwrap(),
                parts[14],
                "{line}"
            );
            assert_eq!(
                make(0, 0, 7, 8, false, Vec::new())
                    .get_name(&strings)
                    .unwrap(),
                parts[15],
                "{line}"
            );
            assert_eq!(
                make(0, 0, 9, 8, false, Vec::new())
                    .get_name(&strings)
                    .unwrap(),
                parts[16],
                "{line}"
            );
            let dot_only = make(0, 0, 10, 8, false, Vec::new())
                .get_name(&strings)
                .unwrap();
            let expected_dot_only = if parts[17] == "DOT" { "." } else { parts[17] };
            assert_eq!(dot_only, expected_dot_only, "{line}");
            rows += 1;
        }
        assert_eq!(rows, 7);
    }

    #[test]
    fn animator_state_grouping_math_matches_the_source_context() {
        // The default-state sentinel selects index 0; anything else passes through.
        assert_eq!(default_state_index(u32::MAX), 0);
        assert_eq!(default_state_index(0), 0);
        assert_eq!(default_state_index(9), 9);
        // Parent-machine selection keeps order and duplicates, and misses cleanly.
        let parents = [7u32, 3, 7, 9];
        assert_eq!(state_indices_for_state_machine(&parents, 7), vec![0, 2]);
        assert_eq!(state_indices_for_state_machine(&parents, 9), vec![3]);
        assert_eq!(
            state_indices_for_state_machine(&parents, 1),
            Vec::<usize>::new()
        );
        assert_eq!(state_indices_for_state_machine(&[], 7), Vec::<usize>::new());
        // The path overload resolves through the registry entries first.
        let entries = [(String::from("A.B"), 7u32), (String::from("C"), 9u32)];
        assert_eq!(
            state_indices_for_state_machine_path(&parents, &entries, "A.B"),
            vec![0, 2]
        );
        assert_eq!(
            state_indices_for_state_machine_path(&parents, &entries, "Missing"),
            Vec::<usize>::new()
        );
        // Path derivation strips one trailing `.StateName` segment.
        assert_eq!(
            make_state_machine_path("Parent.State", "State").as_deref(),
            Some("Parent")
        );
        assert_eq!(
            make_state_machine_path("A.B.State", "State").as_deref(),
            Some("A.B")
        );
        // Too-short paths, mismatched suffixes, and empty names read None.
        assert_eq!(make_state_machine_path("NoDot", "NoDot"), None);
        assert_eq!(make_state_machine_path("Parent.Other", "State"), None);
        assert_eq!(make_state_machine_path("", "State"), None);
    }
}
