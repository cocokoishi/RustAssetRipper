//! `StateConstantExtensions`.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/StateConstantExtensions.cs`. State ids come
//! from the full-path, path, or plain id field depending on the version; names are either a
//! dictionary lookup by `NameID` or the state path with any parent-machine prefix stripped.

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
}
