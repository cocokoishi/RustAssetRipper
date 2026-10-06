//! `GameObjectExtensions` active-state and hierarchy helpers.
//!
//! Mirrors `AssetRipper.SourceGenerated.Extensions/GameObjectExtensions.cs` for the active state.
//! Old versions expose a `Boolean` field, newer ones a `Byte` field; the getter combines both, and
//! the setter writes both. In practice the sampled versions only serialize the boolean field, so
//! the byte contribution reads as zero.
//!
//! Also mirrors the caller-independent parts of the hierarchy helpers: `ShouldBeActive` (the
//! pre-4.0 non-scene force-active rule) and the `GetRootDepth` father walk over a caller-held
//! father table. Component resolution, tag mapping, asset creation, and `GetRoot` (which also
//! stops at transforms without a GameObject) stay caller-side.

use super::vertex_data_blob::UnityVersion;

/// A GameObject active-state view.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GameObjectView {
    /// `Has_IsActive_Byte`.
    pub has_is_active_byte: bool,
    /// `IsActive_Byte`.
    pub is_active_byte: u8,
    /// `IsActive_Boolean`.
    pub is_active_boolean: bool,
}

impl GameObjectView {
    /// `GameObjectExtensions.GetIsActive`.
    pub fn get_is_active(&self) -> bool {
        self.is_active_boolean || (self.has_is_active_byte && self.is_active_byte > 0)
    }

    /// `GameObjectExtensions.SetIsActive`.
    pub fn set_is_active(&mut self, active: bool) {
        if self.has_is_active_byte {
            self.is_active_byte = u8::from(active);
        }
        self.is_active_boolean = active;
    }
}

/// A failure while walking the transform father table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GameObjectError {
    /// A node or father target outside the table.
    UnknownNode {
        /// The offending node index.
        index: usize,
    },
    /// A father cycle; the source loops forever here, so the walk stops instead.
    FatherCycle {
        /// The revisited node index.
        index: usize,
    },
}

impl core::fmt::Display for GameObjectError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnknownNode { index } => {
                write!(
                    formatter,
                    "transform node {index} is outside the father table"
                )
            }
            Self::FatherCycle { index } => {
                write!(formatter, "transform father chain cycles at node {index}")
            }
        }
    }
}

impl std::error::Error for GameObjectError {}

/// `GameObjectExtensions.ShouldBeActive` (`IsActiveInherited` is `version < 4.0`).
///
/// Pre-4.0 non-scene objects are always active; otherwise the stored flag decides.
/// `is_active` is the `GetIsActive` outcome, supplied by the caller with the view above.
pub fn should_be_active(version: UnityVersion, is_scene: bool, is_active: bool) -> bool {
    if version.less_than(4) && !is_scene {
        true
    } else {
        is_active
    }
}

/// `GameObjectExtensions.GetRootDepth` father walk over caller facts.
///
/// `fathers[i]` is the parent slot of node `i` (`None` at the root); the result counts
/// father hops from `start`. Unlike the source, cyclic or out-of-table chains read as
/// errors instead of looping forever; acyclic inputs match the source exactly.
pub fn root_depth(fathers: &[Option<usize>], start: usize) -> Result<usize, GameObjectError> {
    let mut visited = vec![false; fathers.len()];
    let mut depth = 0;
    let mut current = start;
    loop {
        let slot = fathers
            .get(current)
            .ok_or(GameObjectError::UnknownNode { index: current })?;
        if visited[current] {
            return Err(GameObjectError::FatherCycle { index: current });
        }
        visited[current] = true;
        match slot {
            None => return Ok(depth),
            Some(parent) => {
                depth += 1;
                current = *parent;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Byte-for-byte differential against the reference `GameObjectExtensions` (probe
    /// `Temp/opencode/goprobe2`).
    #[test]
    fn game_object_active_matches_the_reference_runtime() {
        let fixture = include_str!("game_object_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "GO", "{line}");
            let has_byte = parts[3] == "1";

            let active = |boolean: bool, byte: u8| {
                GameObjectView {
                    has_is_active_byte: has_byte,
                    is_active_byte: byte,
                    is_active_boolean: boolean,
                }
                .get_is_active()
            };

            assert_eq!(active(false, 0), parts[4] == "1", "{line}");
            assert_eq!(active(false, 1), parts[5] == "1", "{line}");
            assert_eq!(active(true, 0), parts[6] == "1", "{line}");
            assert_eq!(active(true, 5), parts[7] == "1", "{line}");

            let mut set_true = GameObjectView {
                has_is_active_byte: has_byte,
                ..GameObjectView::default()
            };
            set_true.set_is_active(true);
            assert_eq!(set_true.is_active_boolean, parts[8] == "1", "{line}");
            assert_eq!(
                set_true.is_active_byte,
                parts[9].parse::<u8>().unwrap(),
                "{line}"
            );

            let mut set_false = GameObjectView {
                has_is_active_byte: has_byte,
                ..GameObjectView::default()
            };
            set_false.set_is_active(false);
            assert_eq!(set_false.is_active_boolean, parts[10] == "1", "{line}");
            assert_eq!(
                set_false.is_active_byte,
                parts[11].parse::<u8>().unwrap(),
                "{line}"
            );
            rows += 1;
        }
        assert_eq!(rows, 3);
    }

    fn version(major: u32, minor: u32) -> UnityVersion {
        UnityVersion {
            major,
            minor,
            patch: 0,
        }
    }

    /// `ShouldBeActive` force-activates pre-4.0 non-scene objects only.
    #[test]
    fn should_be_active_matches_the_source_branches() {
        // Pre-4.0 non-scene objects read active regardless of the stored flag.
        assert!(should_be_active(version(3, 5), false, false));
        assert!(should_be_active(version(3, 5), false, true));
        // Scene membership keeps the stored flag on old versions.
        assert!(!should_be_active(version(3, 5), true, false));
        assert!(should_be_active(version(3, 5), true, true));
        // Version 4.0 and later always defer to the stored flag.
        assert!(!should_be_active(version(4, 0), false, false));
        assert!(should_be_active(version(4, 0), false, true));
        assert!(!should_be_active(version(2022, 3), false, false));
        assert!(should_be_active(version(2022, 3), true, true));
    }

    /// `GetRootDepth` counts father hops and rejects bad chains.
    #[test]
    fn root_depth_matches_the_source_walk() {
        let fathers = [Some(1), Some(2), None];
        assert_eq!(root_depth(&fathers, 0), Ok(2));
        assert_eq!(root_depth(&fathers, 1), Ok(1));
        assert_eq!(root_depth(&fathers, 2), Ok(0));
        assert_eq!(
            root_depth(&[], 0),
            Err(GameObjectError::UnknownNode { index: 0 })
        );
        assert_eq!(
            root_depth(&fathers, 7),
            Err(GameObjectError::UnknownNode { index: 7 })
        );
        // A hop past the table end and any cycle read as errors, not hangs.
        assert_eq!(
            root_depth(&[Some(4)], 0),
            Err(GameObjectError::UnknownNode { index: 4 })
        );
        assert_eq!(
            root_depth(&[Some(1), Some(0)], 0),
            Err(GameObjectError::FatherCycle { index: 0 })
        );
        assert_eq!(
            root_depth(&[Some(0)], 0),
            Err(GameObjectError::FatherCycle { index: 0 })
        );
    }
}
