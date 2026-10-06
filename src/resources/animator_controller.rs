//! `AnimatorControllerExtensions`, `AnimatorOverrideControllerExtensions`, and
//! `RuntimeAnimatorControllerExtensions`, plus the state-behaviour lookup and
//! state-machine list-routing kernels.
//!
//! Mirrors the `ContainsAnimationClip` overloads: an animator controller owns a clip pointer list,
//! an override controller owns original/override pairs plus a base controller, and the runtime
//! dispatch hands off to whichever of the two concrete controller kinds it holds.
//! Also mirrors `GetStateBehaviours` (range-table index resolution) and the
//! `AnimatorStateMachineExtensions` child-count/capacity routing (modern
//! `ChildStates`/`ChildStateMachines` versus the legacy `StatesP`/
//! `ChildStateMachine` lists).
//!
//! The asset-graph traversals stay open: `FetchEditorHierarchy` (controller,
//! state-machine, blend-tree, and state hierarchies) needs collection-backed
//! pointer resolution, as does the `AnimatorStateMachineContext`/
//! `AnimatorStateContext` release rebuild in `AssetRipper.Processing`. The
//! `HideInHierarchy` flag write inside `GetStateBehaviours` is a caller-side
//! mutation; this module only resolves which behaviour slots are selected.

use super::pptr::{is_asset, AssetRef, PPtrView};

/// A failure while resolving a runtime animator controller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControllerError {
    /// The runtime controller was neither an animator controller nor an override controller.
    UnsupportedRuntimeAnimatorController,
}

impl core::fmt::Display for ControllerError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::UnsupportedRuntimeAnimatorController => {
                write!(formatter, "unsupported runtime animator controller type")
            }
        }
    }
}

impl std::error::Error for ControllerError {}

/// An animator-controller view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnimatorControllerView {
    /// `AnimationClips`.
    pub animation_clips: Vec<PPtrView>,
}

impl AnimatorControllerView {
    /// `AnimatorControllerExtensions.ContainsAnimationClip`.
    pub fn contains_animation_clip(
        &self,
        file: usize,
        dependencies: &[usize],
        clip: AssetRef,
    ) -> bool {
        self.animation_clips
            .iter()
            .any(|pointer| is_asset(*pointer, file, dependencies, clip))
    }
}

/// One original/override clip pair.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClipOverrideView {
    /// `OriginalClip`.
    pub original_clip: PPtrView,
    /// `OverrideClip`.
    pub override_clip: PPtrView,
}

/// An animator-override-controller view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnimatorOverrideControllerView {
    /// `Clips`.
    pub clips: Vec<ClipOverrideView>,
    /// `Controller`.
    pub controller: Option<RuntimeAnimatorControllerRef>,
}

impl AnimatorOverrideControllerView {
    /// `AnimatorOverrideControllerExtensions.ContainsAnimationClip`.
    pub fn contains_animation_clip(
        &self,
        file: usize,
        dependencies: &[usize],
        clip: AssetRef,
    ) -> Result<bool, ControllerError> {
        for pair in &self.clips {
            if is_asset(pair.original_clip, file, dependencies, clip)
                || is_asset(pair.override_clip, file, dependencies, clip)
            {
                return Ok(true);
            }
        }
        match &self.controller {
            Some(base) => runtime_contains(base, file, dependencies, clip),
            None => Ok(false),
        }
    }
}

/// A runtime animator controller the dispatch can hold.
#[derive(Clone, Debug, PartialEq)]
pub enum RuntimeAnimatorControllerRef {
    /// An `AnimatorController`.
    Animator(AnimatorControllerView),
    /// An `AnimatorOverrideController`.
    Override(Box<AnimatorOverrideControllerView>),
    /// Any other runtime controller type, which the reference cannot handle.
    Other,
}

/// `RuntimeAnimatorControllerExtensions.ContainsAnimationClip`.
pub fn runtime_contains(
    controller: &RuntimeAnimatorControllerRef,
    file: usize,
    dependencies: &[usize],
    clip: AssetRef,
) -> Result<bool, ControllerError> {
    match controller {
        RuntimeAnimatorControllerRef::Animator(animator) => {
            Ok(animator.contains_animation_clip(file, dependencies, clip))
        }
        RuntimeAnimatorControllerRef::Override(override_controller) => {
            override_controller.contains_animation_clip(file, dependencies, clip)
        }
        RuntimeAnimatorControllerRef::Other => {
            Err(ControllerError::UnsupportedRuntimeAnimatorController)
        }
    }
}

/// Key into the state-machine behaviour range table.
///
/// Mirrors `StateKey` after `SetValues(layerIndex, stateID)`: the layer index is
/// signed and the state id is unsigned at the `GetStateBehaviours` call site
/// (`int layerIndex, uint stateID`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StateBehaviourKey {
    /// `layerIndex`.
    pub layer_index: i32,
    /// `stateID`.
    pub state_id: u32,
}

/// One `StateRange` entry: the half-open `[start_index, start_index + count)`
/// window into the behaviour-index array.
///
/// The generated subclass is not part of the frozen snapshot; the widths are
/// inferred from the casts at the use site (`(int)range.StartIndex` and the
/// unsigned index array), so both fields are `u32` and every address is checked
/// before use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StateBehaviourRange {
    /// `StateRange.StartIndex`.
    pub start_index: u32,
    /// `StateRange.Count`.
    pub count: u32,
}

/// A failure while resolving state-machine behaviours.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateBehaviourError {
    /// The `u32` range count does not fit in a `usize` on this platform.
    CountOverflow { count: u32 },
    /// Reserving the result list failed.
    AllocationFailed { count: usize },
    /// `start_index + count` overflows the index address space.
    RangeAddressOverflow { start_index: u32, count: u32 },
    /// A range position falls outside the behaviour-index array.
    BehaviourIndexOutOfBounds { position: usize, indices_len: usize },
    /// A resolved slot falls outside the behaviour pointer list.
    BehaviourSlotOutOfBounds { slot: u32, behaviour_count: usize },
}

impl core::fmt::Display for StateBehaviourError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::CountOverflow { count } => {
                write!(formatter, "behaviour range count {count} overflows usize")
            }
            Self::AllocationFailed { count } => {
                write!(formatter, "could not reserve {count} behaviour slots")
            }
            Self::RangeAddressOverflow { start_index, count } => write!(
                formatter,
                "behaviour range [{start_index}, {start_index} + {count}) overflows"
            ),
            Self::BehaviourIndexOutOfBounds {
                position,
                indices_len,
            } => write!(
                formatter,
                "behaviour index position {position} is outside the {indices_len}-entry index array"
            ),
            Self::BehaviourSlotOutOfBounds {
                slot,
                behaviour_count,
            } => write!(
                formatter,
                "behaviour slot {slot} is outside the {behaviour_count}-entry behaviour list"
            ),
        }
    }
}

impl std::error::Error for StateBehaviourError {}

/// Resolves the behaviour slots for one `(layer, state)` pair.
///
/// Mirrors `AnimatorControllerExtensions.GetStateBehaviours`
/// (`AssetRipper/Source/AssetRipper.SourceGenerated.Extensions/AnimatorControllerExtensions.cs`,
/// lines 44-72) as an index kernel over caller facts: `has_description` is
/// `Has_StateMachineBehaviourVectorDescription()`; `ranges` holds the
/// `StateMachineBehaviourRanges` entries; `indices` holds
/// `StateMachineBehaviourIndices`; `behaviour_count` is the length of the
/// `StateMachineBehavioursP` pointer list; `layer_index`/`state_id` are the
/// lookup key. A missing description or a missing key yields an empty list
/// (the source returns `[]`); a present range yields the behaviour slots in
/// order. Out-of-range positions and slots are errors, mirroring the source
/// indexer throws. The `HideInHierarchy` flag write and the pointer resolution
/// through the caller collection stay caller-side.
pub fn state_behaviour_slots(
    has_description: bool,
    ranges: &[(StateBehaviourKey, StateBehaviourRange)],
    indices: &[u32],
    behaviour_count: usize,
    layer_index: i32,
    state_id: u32,
) -> Result<Vec<usize>, StateBehaviourError> {
    if !has_description {
        return Ok(Vec::new());
    }
    let key = StateBehaviourKey {
        layer_index,
        state_id,
    };
    let Some(range) = ranges
        .iter()
        .find(|(candidate, _)| *candidate == key)
        .map(|(_, range)| *range)
    else {
        return Ok(Vec::new());
    };
    let count = usize::try_from(range.count)
        .map_err(|_| StateBehaviourError::CountOverflow { count: range.count })?;
    if count > 0 {
        range.start_index.checked_add(range.count - 1).ok_or(
            StateBehaviourError::RangeAddressOverflow {
                start_index: range.start_index,
                count: range.count,
            },
        )?;
    }
    let mut slots = Vec::new();
    slots
        .try_reserve_exact(count)
        .map_err(|_| StateBehaviourError::AllocationFailed { count })?;
    for offset in 0..count {
        let position = (range.start_index as usize).checked_add(offset).ok_or(
            StateBehaviourError::RangeAddressOverflow {
                start_index: range.start_index,
                count: range.count,
            },
        )?;
        let Some(slot) = indices.get(position) else {
            return Err(StateBehaviourError::BehaviourIndexOutOfBounds {
                position,
                indices_len: indices.len(),
            });
        };
        let slot_index = usize::try_from(*slot)
            .map_err(|_| StateBehaviourError::CountOverflow { count: *slot })?;
        if slot_index >= behaviour_count {
            return Err(StateBehaviourError::BehaviourSlotOutOfBounds {
                slot: *slot,
                behaviour_count,
            });
        }
        slots.push(slot_index);
    }
    Ok(slots)
}

/// Which child list a state machine stores.
///
/// Mirrors the `AnimatorStateMachineExtensions` routing
/// (`AssetRipper/Source/AssetRipper.SourceGenerated.Extensions/AnimatorStateMachineExtensions.cs`):
/// the modern layout keeps `ChildStates`/`ChildStateMachines` while the legacy
/// layout keeps `StatesP`/`ChildStateMachine` (plus `ChildStateMachinePosition`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StateMachineListSlot {
    /// The modern `ChildStates`/`ChildStateMachines` list.
    Modern,
    /// The legacy `StatesP`/`ChildStateMachine` list.
    Legacy,
}

/// Selects the child-state list for capacity writes (`SetChildStateCapacity`).
pub fn child_state_slot(has_child_states: bool) -> StateMachineListSlot {
    if has_child_states {
        StateMachineListSlot::Modern
    } else {
        StateMachineListSlot::Legacy
    }
}

/// Selects the child-state-machine list for capacity writes
/// (`SetChildStateMachineCapacity`; the legacy slot additionally covers
/// `ChildStateMachinePosition`, which shares the same capacity).
pub fn child_state_machine_slot(has_child_state_machines: bool) -> StateMachineListSlot {
    if has_child_state_machines {
        StateMachineListSlot::Modern
    } else {
        StateMachineListSlot::Legacy
    }
}

/// Counts child states (`ChildStatesCount`): `ChildStates` when present, else `StatesP`.
pub fn child_state_count(
    has_child_states: bool,
    child_states_len: usize,
    states_p_len: usize,
) -> usize {
    if has_child_states {
        child_states_len
    } else {
        states_p_len
    }
}

/// Counts child state machines (`ChildStateMachinesCount`): `ChildStateMachines`
/// when present, else `ChildStateMachine`.
pub fn child_state_machine_count(
    has_child_state_machines: bool,
    child_state_machines_len: usize,
    child_state_machine_len: usize,
) -> usize {
    if has_child_state_machines {
        child_state_machines_len
    } else {
        child_state_machine_len
    }
}

/// Capacity for the entry-transition list (`SetEntryTransitionsCapacity`): set
/// only when `Has_EntryTransitions()` holds, otherwise there is no list to size.
pub fn entry_transition_capacity(has_entry_transitions: bool, requested: usize) -> Option<usize> {
    if has_entry_transitions {
        Some(requested)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pointer(path_id: i64) -> PPtrView {
        PPtrView {
            path_id,
            file_id: 0,
        }
    }

    /// Byte-for-byte differential against the reference controller extensions (probe
    /// `Temp/opencode/animctrlprobe`).
    #[test]
    fn controller_contains_animation_clip_matches_the_reference_runtime() {
        let clip_a = AssetRef {
            collection: 1,
            path_id: 1,
        };
        let clip_b = AssetRef {
            collection: 1,
            path_id: 2,
        };
        let clip_c = AssetRef {
            collection: 1,
            path_id: 3,
        };
        let controller = AnimatorControllerView {
            animation_clips: vec![pointer(1)],
        };
        let base = AnimatorControllerView {
            animation_clips: vec![pointer(3)],
        };
        let override_controller = AnimatorOverrideControllerView {
            clips: vec![ClipOverrideView {
                original_clip: pointer(1),
                override_clip: pointer(2),
            }],
            controller: Some(RuntimeAnimatorControllerRef::Animator(base)),
        };

        let fixture = include_str!("animator_controller_fixture.txt");
        let mut rows = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split('|').collect();
            assert_eq!(parts[0], "ACTC", "{line}");
            assert_eq!(
                controller.contains_animation_clip(1, &[], clip_a),
                parts[2] == "1",
                "{line}"
            );
            assert_eq!(
                controller.contains_animation_clip(1, &[], clip_b),
                parts[3] == "1",
                "{line}"
            );
            assert_eq!(
                override_controller
                    .contains_animation_clip(1, &[], clip_a)
                    .unwrap(),
                parts[4] == "1",
                "{line}"
            );
            assert_eq!(
                override_controller
                    .contains_animation_clip(1, &[], clip_b)
                    .unwrap(),
                parts[5] == "1",
                "{line}"
            );
            assert_eq!(
                override_controller
                    .contains_animation_clip(1, &[], clip_c)
                    .unwrap(),
                parts[6] == "1",
                "{line}"
            );
            assert_eq!(
                runtime_contains(
                    &RuntimeAnimatorControllerRef::Animator(controller.clone()),
                    1,
                    &[],
                    clip_a
                )
                .unwrap(),
                parts[7] == "1",
                "{line}"
            );
            assert_eq!(
                runtime_contains(
                    &RuntimeAnimatorControllerRef::Override(Box::new(override_controller.clone())),
                    1,
                    &[],
                    clip_c
                )
                .unwrap(),
                parts[8] == "1",
                "{line}"
            );
            rows += 1;
        }
        assert_eq!(rows, 3);

        assert_eq!(
            runtime_contains(&RuntimeAnimatorControllerRef::Other, 1, &[], clip_a),
            Err(ControllerError::UnsupportedRuntimeAnimatorController)
        );
    }

    fn behaviour_ranges() -> Vec<(StateBehaviourKey, StateBehaviourRange)> {
        vec![
            (
                StateBehaviourKey {
                    layer_index: 0,
                    state_id: 7,
                },
                StateBehaviourRange {
                    start_index: 1,
                    count: 2,
                },
            ),
            (
                StateBehaviourKey {
                    layer_index: 1,
                    state_id: 9,
                },
                StateBehaviourRange {
                    start_index: 0,
                    count: 0,
                },
            ),
        ]
    }

    #[test]
    fn state_behaviours_resolve_slots_in_order() {
        // The (0, 7) window covers indices[1..3], resolving to slots 3 and 5.
        assert_eq!(
            state_behaviour_slots(true, &behaviour_ranges(), &[9, 3, 5], 10, 0, 7),
            Ok(vec![3, 5])
        );
        // A present-but-empty range resolves to no slots.
        assert_eq!(
            state_behaviour_slots(true, &behaviour_ranges(), &[9, 3, 5], 10, 1, 9),
            Ok(Vec::new())
        );
        // A missing key resolves to no slots, like the source's missed lookup.
        assert_eq!(
            state_behaviour_slots(true, &behaviour_ranges(), &[9, 3, 5], 10, 2, 7),
            Ok(Vec::new())
        );
        // Without the description block there is nothing to look up.
        assert_eq!(
            state_behaviour_slots(false, &behaviour_ranges(), &[9, 3, 5], 10, 0, 7),
            Ok(Vec::new())
        );
        assert_eq!(
            state_behaviour_slots(false, &[], &[], 0, 0, 7),
            Ok(Vec::new())
        );
    }

    #[test]
    fn state_behaviours_reject_bad_addresses() {
        // A range whose end overflows the address space is rejected.
        let overflowing = vec![(
            StateBehaviourKey {
                layer_index: 0,
                state_id: 7,
            },
            StateBehaviourRange {
                start_index: u32::MAX,
                count: 2,
            },
        )];
        assert_eq!(
            state_behaviour_slots(true, &overflowing, &[0], 1, 0, 7),
            Err(StateBehaviourError::RangeAddressOverflow {
                start_index: u32::MAX,
                count: 2,
            })
        );
        // A range running past the index array reports the exact position.
        assert_eq!(
            state_behaviour_slots(true, &behaviour_ranges(), &[9, 3, 5], 10, 0, 7),
            Ok(vec![3, 5])
        );
        let short = vec![(
            StateBehaviourKey {
                layer_index: 0,
                state_id: 7,
            },
            StateBehaviourRange {
                start_index: 2,
                count: 2,
            },
        )];
        assert_eq!(
            state_behaviour_slots(true, &short, &[9, 3, 5], 10, 0, 7),
            Err(StateBehaviourError::BehaviourIndexOutOfBounds {
                position: 3,
                indices_len: 3,
            })
        );
        // A slot outside the behaviour pointer list is rejected.
        assert_eq!(
            state_behaviour_slots(true, &behaviour_ranges(), &[9, 3, 5], 4, 0, 7),
            Err(StateBehaviourError::BehaviourSlotOutOfBounds {
                slot: 5,
                behaviour_count: 4,
            })
        );
    }

    #[test]
    fn state_machine_list_routing_follows_layout_presence() {
        // Modern layouts use the child lists; legacy layouts use the flat lists.
        assert_eq!(child_state_slot(true), StateMachineListSlot::Modern);
        assert_eq!(child_state_slot(false), StateMachineListSlot::Legacy);
        assert_eq!(child_state_machine_slot(true), StateMachineListSlot::Modern);
        assert_eq!(
            child_state_machine_slot(false),
            StateMachineListSlot::Legacy
        );
        assert_eq!(child_state_count(true, 3, 9), 3);
        assert_eq!(child_state_count(false, 3, 9), 9);
        assert_eq!(child_state_machine_count(true, 2, 8), 2);
        assert_eq!(child_state_machine_count(false, 2, 8), 8);
        // Entry transitions are sized only when the list exists; trimming
        // composes the count kernel with the capacity kernel.
        assert_eq!(entry_transition_capacity(true, 4), Some(4));
        assert_eq!(entry_transition_capacity(false, 4), None);
        assert_eq!(
            entry_transition_capacity(true, child_state_machine_count(true, 2, 8)),
            Some(2)
        );
    }
}
