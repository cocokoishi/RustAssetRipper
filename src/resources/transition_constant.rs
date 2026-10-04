//! `TransitionConstantExtensions`.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/TransitionConstantExtensions.cs`. Modern
//! versions serialize the exit-time, has-exit-time, interrupted and interruption-source fields
//! directly; older versions store an exit time only as a condition and derive the interruption
//! source from a single `Atomic` flag.

/// `AnimatorConditionMode.ExitTime`.
pub const CONDITION_MODE_EXIT_TIME: i32 = 5;
/// `TransitionInterruptionSource.None`.
pub const INTERRUPTION_SOURCE_NONE: i32 = 0;
/// `TransitionInterruptionSource.Destination`.
pub const INTERRUPTION_SOURCE_DESTINATION: i32 = 2;

/// One transition condition.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TransitionConditionView {
    /// `ConditionMode`.
    pub condition_mode: i32,
    /// `ExitTime`.
    pub exit_time: f32,
}

/// A transition-constant view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TransitionConstantView {
    /// `Atomic`: present only on versions without an explicit interruption source.
    pub atomic: Option<bool>,
    /// `ExitTime`: present only on versions that serialize it.
    pub exit_time: Option<f32>,
    /// `HasExitTime`: present only on versions that serialize it.
    pub has_exit_time: Option<bool>,
    /// `HasFixedDuration`: present only on versions that serialize it.
    pub has_fixed_duration: Option<bool>,
    /// `InterruptionSource`: present only on versions that serialize it.
    pub interruption_source: Option<i32>,
    /// `ConditionConstantArray`.
    pub conditions: Vec<TransitionConditionView>,
}

impl TransitionConstantView {
    /// `TransitionConstantExtensions.GetHasFixedDuration`.
    pub fn get_has_fixed_duration(&self) -> bool {
        self.has_fixed_duration.unwrap_or(true)
    }

    /// `TransitionConstantExtensions.GetInterruptionSource`.
    pub fn get_interruption_source(&self) -> i32 {
        match self.interruption_source {
            Some(value) => value,
            None => {
                if self.atomic.unwrap_or(false) {
                    INTERRUPTION_SOURCE_NONE
                } else {
                    INTERRUPTION_SOURCE_DESTINATION
                }
            }
        }
    }

    /// `TransitionConstantExtensions.GetExitTime`.
    pub fn get_exit_time(&self) -> f32 {
        match self.exit_time {
            Some(value) => value,
            None => self
                .conditions
                .iter()
                .find(|condition| condition.condition_mode == CONDITION_MODE_EXIT_TIME)
                .map(|condition| condition.exit_time)
                .unwrap_or(1.0),
        }
    }

    /// `TransitionConstantExtensions.GetHasExitTime`.
    pub fn get_has_exit_time(&self) -> bool {
        match self.has_exit_time {
            Some(value) => value,
            None => self
                .conditions
                .iter()
                .any(|condition| condition.condition_mode == CONDITION_MODE_EXIT_TIME),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn condition(exit_time: f32) -> TransitionConditionView {
        TransitionConditionView {
            condition_mode: CONDITION_MODE_EXIT_TIME,
            exit_time,
        }
    }

    fn view_for(kind: &str, flags: &[&str]) -> TransitionConstantView {
        let has = |present: &str| present == "1";
        let has_exit = has(flags[0]);
        let has_has_exit = has(flags[1]);
        let has_has_fixed = has(flags[2]);
        let has_interruption = has(flags[3]);
        let has_atomic = has(flags[4]);
        match kind {
            "primary" => TransitionConstantView {
                atomic: has_atomic.then_some(true),
                exit_time: has_exit.then_some(0.75),
                has_exit_time: has_has_exit.then_some(true),
                has_fixed_duration: has_has_fixed.then_some(false),
                interruption_source: has_interruption.then_some(3),
                conditions: vec![condition(0.4)],
            },
            "atomic" => TransitionConstantView {
                atomic: has_atomic.then_some(false),
                exit_time: has_exit.then_some(0.9),
                has_exit_time: has_has_exit.then_some(false),
                has_fixed_duration: has_has_fixed.then_some(false),
                interruption_source: has_interruption.then_some(0),
                conditions: vec![condition(0.4)],
            },
            "empty" => TransitionConstantView {
                atomic: has_atomic.then_some(true),
                exit_time: has_exit.then_some(0.0),
                has_exit_time: has_has_exit.then_some(false),
                has_fixed_duration: has_has_fixed.then_some(false),
                interruption_source: has_interruption.then_some(0),
                conditions: Vec::new(),
            },
            other => panic!("unknown transition kind {other}"),
        }
    }

    /// Byte-for-byte differential against the reference `TransitionConstantExtensions` (probe
    /// `Temp/opencode/trprobe`).
    #[test]
    fn transition_constant_matches_the_reference_runtime() {
        let fixture = include_str!("transition_constant_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "TR", "{line}");
            let view = view_for(parts[2], &parts[3..8]);

            assert_eq!(
                view.get_exit_time().to_bits(),
                parts[8].parse::<u32>().unwrap(),
                "{line}"
            );
            match parts[9] {
                "-" => {}
                value => {
                    assert_eq!(view.get_has_exit_time(), value == "1", "{line}");
                }
            }
            assert_eq!(
                view.get_interruption_source(),
                parts[10].parse::<i32>().unwrap(),
                "{line}"
            );
            match parts[11] {
                "-" => {}
                value => {
                    assert_eq!(view.get_has_fixed_duration(), value == "1", "{line}");
                }
            }
            rows += 1;
        }
        assert_eq!(rows, 18);
    }
}
