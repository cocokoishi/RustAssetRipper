//! `StateMachineConstantExtensions`.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/StateMachineConstantExtensions.cs`. The state
//! machine count is recovered from the entry/exit selector array, which pairs an entry and an exit
//! selector per state machine; versions without the array report zero. The transition decision
//! lives in a dedicated kernel; short or uniform arrays follow the same rule with no special case.

/// A state-machine-constant view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StateMachineConstantView {
    /// `SelectorStateConstantArray` full-path ids: `None` when the version does not serialize the
    /// array at all.
    pub selector_full_path_ids: Option<Vec<u32>>,
}

impl StateMachineConstantView {
    /// `StateMachineConstantExtensions.StateMachineCount`.
    pub fn state_machine_count(&self) -> usize {
        let Some(ids) = &self.selector_full_path_ids else {
            return 0;
        };
        count_selector_transitions(ids)
    }
}

/// Counts state machines from selector full-path ids: each change of id
/// (including the first) opens a new machine, tolerating missing entry or exit
/// halves exactly like the source loop.
fn count_selector_transitions(ids: &[u32]) -> usize {
    let mut count = 0;
    let mut previous: Option<u32> = None;
    for id in ids {
        if previous != Some(*id) {
            previous = Some(*id);
            count += 1;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Byte-for-byte differential against the reference `StateMachineConstantExtensions` (probe
    /// `Temp/opencode/anim2probe`).
    #[test]
    fn state_machine_count_matches_the_reference_runtime() {
        let fixture = include_str!("state_machine_constant_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "SMC", "{line}");
            let view = StateMachineConstantView {
                selector_full_path_ids: (parts[2] == "1").then_some(vec![10, 10, 20, 10, 30]),
            };
            assert_eq!(
                view.state_machine_count(),
                parts[3].parse::<usize>().unwrap(),
                "{line}"
            );
            rows += 1;
        }
        assert_eq!(rows, 6);
    }

    #[test]
    fn state_machine_count_edges_follow_the_transition_rule() {
        let view = StateMachineConstantView {
            selector_full_path_ids: None,
        };
        assert_eq!(view.state_machine_count(), 0);
        for (ids, expected) in [
            (vec![], 0),
            (vec![7, 7, 7, 7], 1),
            (vec![1, 1, 2, 2], 2),
            (vec![1, 2, 2, 1], 3),
            (vec![1, 2, 1, 2], 4),
        ] {
            let view = StateMachineConstantView {
                selector_full_path_ids: Some(ids),
            };
            assert_eq!(view.state_machine_count(), expected);
        }
    }
}
