//! `AnimatorControllerExtensions`, `AnimatorOverrideControllerExtensions`, and
//! `RuntimeAnimatorControllerExtensions`, plus the state-behaviour lookup,
//! state-machine list-routing, and transition-destination decoding kernels.
//!
//! Mirrors the `ContainsAnimationClip` overloads: an animator controller owns a clip pointer list,
//! an override controller owns original/override pairs plus a base controller, and the runtime
//! dispatch hands off to whichever of the two concrete controller kinds it holds.
//! Also mirrors `GetStateBehaviours` (range-table index resolution), the
//! `AnimatorStateMachineExtensions` child-count/capacity routing (modern
//! `ChildStates`/`ChildStateMachines` versus the legacy `StatesP`/
//! `ChildStateMachine` lists), and the `TryGetDestinationState` destination-id
//! decision (absent/state/port) behind every transition rebuild.
//!
//! The asset-graph traversals stay open: `FetchEditorHierarchy` (controller,
//! state-machine, blend-tree, and state hierarchies) needs collection-backed
//! pointer resolution, as does the `AnimatorStateMachineContext`/
//! `AnimatorStateContext` release rebuild in `AssetRipper.Processing`. The
//! `HideInHierarchy` flag write inside `GetStateBehaviours` is a caller-side
//! mutation; this module only resolves which behaviour slots are selected.
//! Likewise the selector-table `FullPathID` search and the `IsEntry` read behind
//! a decoded port stay caller-side with the generated objects.

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

/// `AnimatorStateMachineContext.StateMachineTransitionFlag`: destination ids at or
/// above this address state-machine entry/exit ports instead of plain states.
pub const STATE_MACHINE_TRANSITION_FLAG: u32 = 30_000;

/// Why a transition destination id resolves to nothing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DestinationRejection {
    /// `uint.MaxValue`: names no destination at all (the `TryGetDestinationState`
    /// head; the same check is available as `has_transition_destination` in
    /// `animator_enums`, and is repeated here so the decision table is complete).
    NoDestination,
    /// A state-machine port id on a layout without a selector table (pre-Unity-5,
    /// where `Has_SelectorStateConstantArray()` is false).
    MissingSelectorTable,
    /// `destination % StateMachineTransitionFlag` runs past the selector table.
    SelectorIndexOutOfBounds {
        /// The rejected selector index.
        selector_index: u32,
        /// The selector table length.
        selector_count: usize,
    },
}

/// A decoded `TryGetDestinationState` outcome.
///
/// Mirrors `AnimatorStateMachineContext.TryGetDestinationState`
/// (`AssetRipper/Source/AssetRipper.Processing/AnimatorControllers/AnimatorStateMachineContext.cs`,
/// lines 445-478) as a decision over caller facts: `has_selector_table` is
/// `StateMachineConstant.Has_SelectorStateConstantArray()` and `selector_count`
/// is the `SelectorStateConstantArray` length. The selector-table read, the
/// `FullPathID` machine search, and the plain-state index lookup stay caller-side
/// with the generated objects.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransitionDestination {
    /// A plain state: the source casts the id to an `int` state index (ids below
    /// the flag are small, so the wrapping cast never changes the value).
    State {
        /// The state index.
        state_index: i32,
    },
    /// A state-machine entry/exit port: index into the selector table.
    StateMachinePort {
        /// The selector table index.
        selector_index: u32,
    },
    /// The id resolves to nothing.
    Rejected(DestinationRejection),
}

/// Decides what a transition destination id addresses.
pub const fn decode_transition_destination(
    destination_state: u32,
    has_selector_table: bool,
    selector_count: usize,
) -> TransitionDestination {
    if destination_state == u32::MAX {
        return TransitionDestination::Rejected(DestinationRejection::NoDestination);
    }
    if destination_state < STATE_MACHINE_TRANSITION_FLAG {
        return TransitionDestination::State {
            state_index: destination_state as i32,
        };
    }
    if !has_selector_table {
        return TransitionDestination::Rejected(DestinationRejection::MissingSelectorTable);
    }
    let selector_index = destination_state % STATE_MACHINE_TRANSITION_FLAG;
    if selector_index as u64 >= selector_count as u64 {
        return TransitionDestination::Rejected(DestinationRejection::SelectorIndexOutOfBounds {
            selector_index,
            selector_count,
        });
    }
    TransitionDestination::StateMachinePort { selector_index }
}

/// One claimed asset in `FetchEditorHierarchy` order.
///
/// Mirrors `AnimatorControllerExtensions.FetchEditorHierarchy`
/// (`AssetRipper/Source/AssetRipper.SourceGenerated.Extensions/AnimatorControllerExtensions.cs`,
/// lines 74-299) as an order kernel over caller facts: every yield is a
/// `(kind, path)` pair where `path` is `None` for an unresolvable pointer
/// (the source yields the null). Pointer resolution, the layer/mask/skeleton
/// ignores, the `DefaultState`/`ParentStateMachine` ignores, the skipped
/// transition-map keys, and the `HideInHierarchy` mutation stay caller-side
/// with the generated objects and the caller collection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FetchKind {
    /// The controller itself, then its layers.
    Controller,
    /// A state machine (modern or legacy layout).
    StateMachine,
    /// An animator state.
    State,
    /// A blend tree (animation clips never yield).
    BlendTree,
    /// An any-state, entry, machine-map, local, ordered, or state transition.
    Transition,
    /// A state-machine behaviour.
    Behaviour,
}

/// One `FetchEditorHierarchy` yield: the asset kind and its resolved path
/// (`None` stands for the source's null yield).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FetchYield {
    /// What the source yielded.
    pub kind: FetchKind,
    /// The resolved path id, or `None` for an unresolvable pointer.
    pub path: Option<i64>,
}

/// A motion edge: only a resolved blend tree yields (and recurses);
/// animation clips, missing motions, and nulls yield nothing.
#[derive(Clone, Debug, PartialEq)]
pub struct FetchMotion {
    /// The resolved child blend tree, if the motion is one.
    pub blend_tree: Option<Box<FetchBlendTree>>,
}

/// A blend tree: itself, then the child blend trees in order.
#[derive(Clone, Debug, PartialEq)]
pub struct FetchBlendTree {
    /// The tree's own path.
    pub path: i64,
    /// The child motions in serialized order.
    pub childs: Vec<FetchMotion>,
}

/// An animator state: itself, then motions (plural list when
/// `has_motions`, else the single `MotionP` blend-tree check), then
/// behaviours, then transitions.
#[derive(Clone, Debug, PartialEq)]
pub struct FetchState {
    /// The state's own path.
    pub path: i64,
    /// `Has_Motions()`: plural `MotionsP` list versus the single `MotionP`.
    pub has_motions: bool,
    /// The plural motions (only read when `has_motions`).
    pub motions: Vec<FetchMotion>,
    /// The single-motion blend tree (only read when `!has_motions`).
    pub motion_single: Option<FetchBlendTree>,
    /// The resolved `StateMachineBehavioursP` in order (`None` yields null).
    pub behaviours: Vec<Option<i64>>,
    /// The resolved `TransitionsP` in order (`None` yields null).
    pub transitions: Vec<Option<i64>>,
}

/// A layer behaviour pair: the state hierarchy, then the behaviours.
#[derive(Clone, Debug, PartialEq)]
pub struct FetchBehaviourPair {
    /// The resolved pair state, if any.
    pub state: Option<FetchState>,
    /// The resolved pair behaviours in order (`None` yields null).
    pub behaviours: Vec<Option<i64>>,
}

/// A layer motion pair: the state hierarchy, then the motion blend tree.
#[derive(Clone, Debug, PartialEq)]
pub struct FetchMotionPair {
    /// The resolved pair state, if any.
    pub state: Option<FetchState>,
    /// The pair motion: a blend tree yields and recurses, anything else
    /// yields nothing.
    pub motion: FetchMotion,
}

/// One controller layer: behaviour pairs, motion pairs (only when
/// `has_behaviours`), then the state machine.
#[derive(Clone, Debug, PartialEq)]
pub struct FetchLayer {
    /// `Has_Behaviours()`.
    pub has_behaviours: bool,
    /// The `Behaviours` pairs in order.
    pub behaviour_pairs: Vec<FetchBehaviourPair>,
    /// The `Motions` pairs in order.
    pub motion_pairs: Vec<FetchMotionPair>,
    /// The resolved layer state machine, if any.
    pub state_machine: Option<FetchStateMachine>,
}

/// A state machine: itself, then the modern (`Has_ChildStateMachines()`)
/// or legacy child layout. Child machines recurse in both layouts; the map
/// keys are skipped; `DefaultState` is ignored as redundant.
#[derive(Clone, Debug, PartialEq)]
pub struct FetchStateMachine {
    /// The machine's own path.
    pub path: i64,
    /// `Has_ChildStateMachines()`: modern child lists versus the legacy
    /// `ChildStateMachineP`/`LocalTransitions`/`OrderedTransitions`/`StatesP`.
    pub modern: bool,
    /// The resolved `AnyStateTransitionsP` (modern only).
    pub any_state_transitions: Vec<Option<i64>>,
    /// Resolved child machines in order (modern `ChildStateMachines`,
    /// legacy `ChildStateMachineP`).
    pub child_machines: Vec<Option<FetchStateMachine>>,
    /// Resolved modern `ChildStates` in order.
    pub child_states: Vec<Option<FetchState>>,
    /// The resolved `EntryTransitionsP` (modern only).
    pub entry_transitions: Vec<Option<i64>>,
    /// The resolved modern `StateMachineBehavioursP` in order.
    pub behaviours: Vec<Option<i64>>,
    /// The modern `StateMachineTransitions` values in order: each entry's
    /// transition list, keys skipped.
    pub machine_transitions: Vec<Vec<Option<i64>>>,
    /// The legacy `LocalTransitions` entries in order: the state hierarchy,
    /// then the entry's transitions.
    pub local_transitions: Vec<(Option<FetchState>, Vec<Option<i64>>)>,
    /// The legacy `OrderedTransitions` entries, same shape as local.
    pub ordered_transitions: Vec<(Option<FetchState>, Vec<Option<i64>>)>,
    /// The resolved legacy `StatesP` in order.
    pub states: Vec<Option<FetchState>>,
}

/// The controller root for the fetch: itself, then its layers.
#[derive(Clone, Debug, PartialEq)]
pub struct FetchController {
    /// The controller's own path.
    pub path: i64,
    /// The `AnimatorLayers` in order.
    pub layers: Vec<FetchLayer>,
}

fn fetch_blend_tree(tree: &FetchBlendTree, yields: &mut Vec<FetchYield>) {
    yields.push(FetchYield {
        kind: FetchKind::BlendTree,
        path: Some(tree.path),
    });
    for child in &tree.childs {
        if let Some(child_tree) = &child.blend_tree {
            fetch_blend_tree(child_tree, &mut *yields);
        }
    }
}

fn fetch_state(state: &FetchState, yields: &mut Vec<FetchYield>) {
    yields.push(FetchYield {
        kind: FetchKind::State,
        path: Some(state.path),
    });
    if state.has_motions {
        for motion in &state.motions {
            if let Some(tree) = &motion.blend_tree {
                fetch_blend_tree(tree, &mut *yields);
            }
        }
    } else if let Some(tree) = &state.motion_single {
        fetch_blend_tree(tree, &mut *yields);
    }
    for behaviour in &state.behaviours {
        yields.push(FetchYield {
            kind: FetchKind::Behaviour,
            path: *behaviour,
        });
    }
    for transition in &state.transitions {
        yields.push(FetchYield {
            kind: FetchKind::Transition,
            path: *transition,
        });
    }
}

fn fetch_state_machine(machine: &FetchStateMachine, yields: &mut Vec<FetchYield>) {
    yields.push(FetchYield {
        kind: FetchKind::StateMachine,
        path: Some(machine.path),
    });
    if machine.modern {
        for transition in &machine.any_state_transitions {
            yields.push(FetchYield {
                kind: FetchKind::Transition,
                path: *transition,
            });
        }
        for child_machine in machine.child_machines.iter().flatten() {
            fetch_state_machine(child_machine, &mut *yields);
        }
        for state in machine.child_states.iter().flatten() {
            fetch_state(state, &mut *yields);
        }
        for transition in &machine.entry_transitions {
            yields.push(FetchYield {
                kind: FetchKind::Transition,
                path: *transition,
            });
        }
        for behaviour in &machine.behaviours {
            yields.push(FetchYield {
                kind: FetchKind::Behaviour,
                path: *behaviour,
            });
        }
        for transitions in &machine.machine_transitions {
            for transition in transitions {
                yields.push(FetchYield {
                    kind: FetchKind::Transition,
                    path: *transition,
                });
            }
        }
    } else {
        for child_machine in machine.child_machines.iter().flatten() {
            fetch_state_machine(child_machine, &mut *yields);
        }
        for (state, transitions) in &machine.local_transitions {
            if let Some(state) = state {
                fetch_state(state, &mut *yields);
            }
            for transition in transitions {
                yields.push(FetchYield {
                    kind: FetchKind::Transition,
                    path: *transition,
                });
            }
        }
        for (state, transitions) in &machine.ordered_transitions {
            if let Some(state) = state {
                fetch_state(state, &mut *yields);
            }
            for transition in transitions {
                yields.push(FetchYield {
                    kind: FetchKind::Transition,
                    path: *transition,
                });
            }
        }
        for state in machine.states.iter().flatten() {
            fetch_state(state, &mut *yields);
        }
    }
}

/// Claims the editor hierarchy in source order: the controller, then per
/// layer the behaviour pairs (state hierarchy, behaviours), the motion
/// pairs (state hierarchy, blend tree), and the state machine — exactly the
/// `AnimatorControllerProcessor`/`AnimatorControllerExportCollection` claim
/// sequence. Unresolvable pointers yield `(kind, None)` like the source's
/// nulls; there is no deduplication.
pub fn fetch_editor_hierarchy(controller: &FetchController) -> Vec<FetchYield> {
    let mut yields = Vec::new();
    yields.push(FetchYield {
        kind: FetchKind::Controller,
        path: Some(controller.path),
    });
    for layer in &controller.layers {
        if layer.has_behaviours {
            for pair in &layer.behaviour_pairs {
                if let Some(state) = &pair.state {
                    fetch_state(state, &mut yields);
                }
                for behaviour in &pair.behaviours {
                    yields.push(FetchYield {
                        kind: FetchKind::Behaviour,
                        path: *behaviour,
                    });
                }
            }
            for pair in &layer.motion_pairs {
                if let Some(state) = &pair.state {
                    fetch_state(state, &mut yields);
                }
                if let Some(tree) = &pair.motion.blend_tree {
                    fetch_blend_tree(tree, &mut yields);
                }
            }
        }
        if let Some(machine) = &layer.state_machine {
            fetch_state_machine(machine, &mut yields);
        }
    }
    yields
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

    #[test]
    fn transition_destination_decoding_follows_the_source_branches() {
        use TransitionDestination as Decoded;
        // The sentinel names no destination, with or without a selector table.
        assert_eq!(
            decode_transition_destination(u32::MAX, true, 4),
            Decoded::Rejected(DestinationRejection::NoDestination)
        );
        assert_eq!(
            decode_transition_destination(u32::MAX, false, 0),
            Decoded::Rejected(DestinationRejection::NoDestination)
        );
        // Ids below the flag address plain states, table or not.
        assert_eq!(
            decode_transition_destination(0, false, 0),
            Decoded::State { state_index: 0 }
        );
        assert_eq!(
            decode_transition_destination(7, true, 4),
            Decoded::State { state_index: 7 }
        );
        assert_eq!(
            decode_transition_destination(29_999, true, 4),
            Decoded::State {
                state_index: 29_999
            }
        );
        // Port ids need the selector table.
        assert_eq!(
            decode_transition_destination(30_000, false, 4),
            Decoded::Rejected(DestinationRejection::MissingSelectorTable)
        );
        // The selector index is the id modulo the flag.
        assert_eq!(
            decode_transition_destination(30_000, true, 1),
            Decoded::StateMachinePort { selector_index: 0 }
        );
        assert_eq!(
            decode_transition_destination(30_001, true, 2),
            Decoded::StateMachinePort { selector_index: 1 }
        );
        // An index past the table is rejected with its witness values.
        assert_eq!(
            decode_transition_destination(30_005, true, 5),
            Decoded::Rejected(DestinationRejection::SelectorIndexOutOfBounds {
                selector_index: 5,
                selector_count: 5,
            })
        );
        assert_eq!(
            decode_transition_destination(30_000, true, 0),
            Decoded::Rejected(DestinationRejection::SelectorIndexOutOfBounds {
                selector_index: 0,
                selector_count: 0,
            })
        );
        // The modulo wraps large ids: 4_294_967_294 % 30_000 == 17_294.
        assert_eq!(
            decode_transition_destination(u32::MAX - 1, true, 20_000),
            Decoded::StateMachinePort {
                selector_index: 17_294
            }
        );
        assert_eq!(STATE_MACHINE_TRANSITION_FLAG, 30_000);
    }

    /// Differential against the shipped hierarchy walk over live generated
    /// objects (probe `Temp/opencode/animhierprobe`, fixture
    /// `animator_hierarchy_fixture.txt`): a 2022.3 controller graph plus 5.0
    /// and 4.0 machines through the shipped `FetchEditorHierarchy` and
    /// `GetStateBehaviours`. The replay feeds the probed gates plus the
    /// probe-design graph (pinned in the fixture header) through
    /// `fetch_editor_hierarchy` and must reproduce all 27 yields in order —
    /// including the layer pair order, the nested blend-tree recursion, the
    /// modern machine order, the 4.0 legacy else-branch order, the
    /// clip-exclusion (116 never yields), and the missing-key empty slots
    /// with the live `[1, 0]` slot order. The `HideInHierarchy` write stays
    /// caller-side; the replay consumes the live before/after bits as the
    /// mutation witness.
    #[test]
    fn editor_hierarchy_matches_the_reference_runtime() {
        fn plain_state(path: i64) -> FetchState {
            FetchState {
                path,
                has_motions: false,
                motions: Vec::new(),
                motion_single: None,
                behaviours: Vec::new(),
                transitions: Vec::new(),
            }
        }

        fn empty_machine(path: i64, modern: bool) -> FetchStateMachine {
            FetchStateMachine {
                path,
                modern,
                any_state_transitions: Vec::new(),
                child_machines: Vec::new(),
                child_states: Vec::new(),
                entry_transitions: Vec::new(),
                behaviours: Vec::new(),
                machine_transitions: Vec::new(),
                local_transitions: Vec::new(),
                ordered_transitions: Vec::new(),
                states: Vec::new(),
            }
        }

        let state_a = FetchState {
            path: 103,
            has_motions: false,
            motions: Vec::new(),
            motion_single: None,
            behaviours: vec![Some(113)],
            transitions: vec![Some(108)],
        };
        let child_blend = FetchBlendTree {
            path: 107,
            childs: Vec::new(),
        };
        let blend = FetchBlendTree {
            path: 106,
            childs: vec![FetchMotion {
                blend_tree: Some(Box::new(child_blend)),
            }],
        };
        let mut root = empty_machine(101, true);
        root.any_state_transitions = vec![Some(108)];
        root.child_machines = vec![Some(empty_machine(102, true))];
        root.child_states = vec![Some(state_a.clone())];
        root.entry_transitions = vec![Some(114)];
        root.behaviours = vec![Some(113)];
        root.machine_transitions = vec![vec![Some(115)]];

        let state_124 = FetchState {
            path: 124,
            has_motions: true,
            motions: vec![
                FetchMotion { blend_tree: None },
                FetchMotion {
                    blend_tree: Some(Box::new(FetchBlendTree {
                        path: 107,
                        childs: Vec::new(),
                    })),
                },
            ],
            motion_single: None,
            behaviours: Vec::new(),
            transitions: Vec::new(),
        };
        let mut ancient = empty_machine(123, false);
        ancient.child_machines = vec![Some(empty_machine(102, true))];
        ancient.local_transitions = vec![(Some(state_a.clone()), vec![Some(122)])];
        ancient.ordered_transitions = vec![(Some(plain_state(121)), vec![Some(122)])];
        ancient.states = vec![Some(plain_state(121)), Some(state_124)];

        let controller = FetchController {
            path: 100,
            layers: vec![
                FetchLayer {
                    has_behaviours: true,
                    behaviour_pairs: vec![FetchBehaviourPair {
                        state: Some(plain_state(104)),
                        behaviours: vec![Some(113)],
                    }],
                    motion_pairs: vec![FetchMotionPair {
                        state: Some(plain_state(105)),
                        motion: FetchMotion {
                            blend_tree: Some(Box::new(blend)),
                        },
                    }],
                    state_machine: Some(root),
                },
                FetchLayer {
                    has_behaviours: true,
                    behaviour_pairs: Vec::new(),
                    motion_pairs: Vec::new(),
                    state_machine: Some(empty_machine(120, true)),
                },
                FetchLayer {
                    has_behaviours: true,
                    behaviour_pairs: Vec::new(),
                    motion_pairs: Vec::new(),
                    state_machine: Some(ancient),
                },
            ],
        };

        let fixture = include_str!("animator_hierarchy_fixture.txt");
        let mut gates: Vec<(String, String, bool)> = Vec::new();
        let mut yields: Vec<FetchYield> = Vec::new();
        let mut failures = 0;
        let mut stepfailed = false;
        let mut hide_before = [0; 2];
        let mut hide_after = [0; 2];
        let mut hit_paths: Vec<i64> = Vec::new();
        let mut miss_count = 0;
        for line in fixture.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut cells = line.split('|');
            let prefix = cells.next().expect("a prefix");
            match prefix {
                "ANIM" => {
                    let kind = cells.next().expect("a row kind");
                    if kind == "gates" {
                        let owner = cells.next().expect("an owner").to_owned();
                        let rest: Vec<&str> = cells.collect();
                        for entry in rest.join(",").split(',') {
                            let (name, flag) = entry.split_once('=').expect("a gate assignment");
                            gates.push((
                                owner.clone(),
                                name.to_owned(),
                                match flag {
                                    "0" => false,
                                    "1" => true,
                                    other => panic!("unexpected gate {other}"),
                                },
                            ));
                        }
                    } else if kind == "failures" {
                        failures = cells.next().expect("a count").parse().expect("count");
                    } else if kind == "stepfail" {
                        stepfailed = true;
                    }
                }
                "FHIER" => {
                    let seq = cells.next().expect("a sequence");
                    if seq == "count" {
                        continue;
                    }
                    let what: Vec<&str> = cells.next().expect("a yield").split(':').collect();
                    let kind = match what[0].split('_').next().expect("a type") {
                        "AnimatorController" => FetchKind::Controller,
                        "AnimatorStateMachine" => FetchKind::StateMachine,
                        "AnimatorState" => FetchKind::State,
                        "AnimatorStateTransition" | "AnimatorTransition" => FetchKind::Transition,
                        "MonoBehaviour" => FetchKind::Behaviour,
                        "BlendTree" => FetchKind::BlendTree,
                        other => panic!("unexpected yield {other}"),
                    };
                    yields.push(FetchYield {
                        kind,
                        path: if what[1] == "null" {
                            None
                        } else {
                            Some(what[1].parse().expect("a path"))
                        },
                    });
                }
                "SBEH" => match cells.next().expect("a row kind") {
                    "before" => {
                        assert_eq!(cells.next().expect("a path"), "111");
                        hide_before[0] = cells.next().expect("bits").parse().expect("bits");
                        assert_eq!(cells.next().expect("a path"), "112");
                        hide_before[1] = cells.next().expect("bits").parse().expect("bits");
                    }
                    "hit" => {
                        assert_eq!(cells.next().expect("a count"), "2");
                        for entry in cells.next().expect("paths").split(',') {
                            let path: Vec<&str> = entry.split(':').collect();
                            hit_paths.push(path[1].parse().expect("a path"));
                        }
                    }
                    "after" => {
                        assert_eq!(cells.next().expect("a path"), "111");
                        hide_after[0] = cells.next().expect("bits").parse().expect("bits");
                        assert_eq!(cells.next().expect("a path"), "112");
                        hide_after[1] = cells.next().expect("bits").parse().expect("bits");
                    }
                    "miss" => {
                        miss_count = cells.next().expect("a count").parse().expect("count");
                    }
                    other => panic!("unexpected sbeh row {other}"),
                },
                other => panic!("unexpected prefix {other}"),
            }
        }
        assert!(!stepfailed, "the probe wired the whole graph");
        assert_eq!(failures, 0, "no probe step failed");

        fn gate(gates: &[(String, String, bool)], owner: &str, name: &str) -> bool {
            gates
                .iter()
                .find(|(gate_owner, gate_name, _)| gate_owner == owner && gate_name == name)
                .map(|(_, _, value)| *value)
                .expect("a probed gate")
        }
        // The probed branch facts: modern layouts on 2022/5.0, the legacy
        // else-branch on 4.0, plural motions only on the 4.0 state.
        assert!(gate(&gates, "machine", "Has_ChildStateMachines"));
        assert!(!gate(&gates, "machine", "Has_LocalTransitions"));
        assert!(!gate(&gates, "state", "Has_Motions"));
        assert!(gate(&gates, "state", "Has_StateMachineBehaviours"));
        assert!(gate(&gates, "state", "Has_Transitions"));
        assert!(gate(&gates, "layer", "Has_Behaviours"));
        assert!(gate(&gates, "legacymachine", "Has_ChildStateMachines"));
        assert!(!gate(&gates, "ancientmachine", "Has_ChildStateMachines"));
        assert!(gate(&gates, "ancientmachine", "Has_LocalTransitions"));
        assert!(gate(&gates, "ancientstate", "Has_Motions"));
        assert!(!gate(&gates, "ancientstate", "Has_StateMachineBehaviours"));
        assert!(!gate(&gates, "ancientstate", "Has_Transitions"));

        assert_eq!(yields.len(), 27, "all probed yields are asserted");
        assert_eq!(fetch_editor_hierarchy(&controller), yields);
        // The excluded clip never yields.
        assert!(
            yields.iter().all(|item| item.path != Some(116)),
            "animation clips stay out of the hierarchy"
        );

        // The live slot order plus the missing-key arm through the slots
        // kernel; the HideFlags write is caller-side and witnessed live.
        assert_eq!(
            state_behaviour_slots(
                true,
                &[(
                    StateBehaviourKey {
                        layer_index: 0,
                        state_id: 7,
                    },
                    StateBehaviourRange {
                        start_index: 0,
                        count: 2,
                    },
                )],
                &[1, 0],
                2,
                0,
                7,
            ),
            Ok(vec![1, 0])
        );
        assert_eq!(hit_paths, vec![112, 111]);
        assert_eq!(
            state_behaviour_slots(
                true,
                &[(
                    StateBehaviourKey {
                        layer_index: 0,
                        state_id: 7,
                    },
                    StateBehaviourRange {
                        start_index: 0,
                        count: 2,
                    },
                )],
                &[1, 0],
                2,
                0,
                999,
            ),
            Ok(Vec::new())
        );
        assert_eq!(miss_count, 0);
        assert_eq!(hide_before, [0, 0]);
        assert_eq!(hide_after, [1, 1]);
    }
}
