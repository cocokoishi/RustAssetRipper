use std::collections::{HashMap, HashSet};
use std::error::Error;

use crate::export::unity_yaml::{
    format_unity_guid, UnityYamlDocument, UnityYamlMappingStyle, UnityYamlNode, UnityYamlNodeKind,
    UnityYamlScalar, UnityYamlSequenceStyle,
};
use crate::resources::blend_shape::crc32_ieee;
use crate::resources::blend_tree_node::BlendTreeNodeView;
use crate::serialized_file::object::{ParsedObjectPayload, SerializedValue};

use super::*;

const ANIMATOR_CONTROLLER_CLASS_ID: i32 = 91;
const ANIMATOR_STATE_TRANSITION_CLASS_ID: i32 = 1101;
const ANIMATOR_STATE_CLASS_ID: i32 = 1102;
const ANIMATOR_STATE_MACHINE_CLASS_ID: i32 = 1107;
const BLEND_TREE_CLASS_ID: i32 = 206;
const TRANSITION_DESTINATION_FLAG: u32 = 30_000;
const EXIT_TIME_CONDITION_MODE: i32 =
    crate::resources::transition_constant::CONDITION_MODE_EXIT_TIME;
const GENERATED_FILE_ID_BASE: i64 = 7_000_000_000;

#[path = "animator_controller/behaviours.rs"]
mod behaviours;
#[path = "animator_controller/hierarchy.rs"]
mod hierarchy;

struct FileIdAllocator {
    next: i64,
    reserved_root: Option<i64>,
}

impl FileIdAllocator {
    fn new() -> Self {
        Self {
            next: GENERATED_FILE_ID_BASE,
            reserved_root: None,
        }
    }

    fn allocate(&mut self) -> Result<i64, Box<dyn Error>> {
        if self.reserved_root == Some(self.next) {
            self.next = self
                .next
                .checked_add(1)
                .ok_or_else(|| invalid_project("AnimatorController generated fileID overflows"))?;
        }
        let result = self.next;
        self.next = self
            .next
            .checked_add(1)
            .ok_or_else(|| invalid_project("AnimatorController generated fileID overflows"))?;
        Ok(result)
    }
}

struct ControllerLayer {
    name: String,
    binding_path_id: u32,
    state_machine_index: usize,
    synchronized_layer_index: u32,
    default_weight: f32,
    blending_mode: i32,
    ik_pass: bool,
    synced_layer_affects_timing: bool,
}

struct ControllerParameter {
    name: String,
    parameter_type: i32,
    default_float: f32,
    default_int: i32,
    default_bool: bool,
}

struct ChildMotionFields {
    motion: UnityYamlNode,
    threshold: f32,
    position: crate::numerics::Vec2,
    time_scale: f32,
    cycle_offset: f32,
    direct_blend_parameter: String,
    mirror: bool,
}

#[derive(Clone)]
struct StateConstant {
    raw: SerializedValue,
    name: String,
    state_path_id: u32,
    parent_path_id: u32,
    file_id: i64,
    motion: UnityYamlNode,
    transition_file_ids: Vec<i64>,
    behaviours: Vec<UnityYamlNode>,
}

#[derive(Clone)]
struct StateMachineConstant {
    raw: SerializedValue,
    states: Vec<StateConstant>,
    selectors: Vec<SelectorState>,
    machines: Vec<EditorStateMachine>,
    /// State indexes the source leaves unattached because their owning machines
    /// never resolved names. They export as separate single-asset files instead
    /// of embedding in the controller document.
    dropped_state_indexes: Vec<usize>,
}

/// One editor node within a layer's packed state-machine constant. State and selector
/// destinations continue to index the original layer arrays, never these child lists.
#[derive(Clone)]
struct EditorStateMachine {
    file_id: i64,
    full_path_id: u32,
    name: String,
    parent_index: Option<usize>,
    state_indexes: Vec<usize>,
    child_machine_indexes: Vec<usize>,
    any_transition_file_ids: Vec<i64>,
    entry_transition_file_ids: Vec<i64>,
    child_exit_transitions: Vec<(usize, Vec<i64>)>,
    default_state_file_id: i64,
    // Created only by the source's last-resort Extra StateMachine rule. This is
    // separate from the packed global state array, so invalid indices stay errors.
    synthetic_default_state: Option<StateConstant>,
    behaviours: Vec<UnityYamlNode>,
    /// Mirrors `AnimatorStateMachineContext.AssignStateMachineChildStates`: machines
    /// whose names never resolved (`Name.IsEmpty`, e.g. empty-path TOS entries such
    /// as `.AutoDresserTo0`) stay in the hierarchy but receive no child states.
    /// Only the selector-driven (Unity 5+) reconstruction has unnamed machines;
    /// the legacy path attaches every state directly.
    accepts_states: bool,
}

/// One controller sub-asset the source leaves outside the controller file because
/// its owning machine never resolved a name. Each exports as its own
/// single-document file with a class-main anchor and a freshly minted GUID,
/// matching the frozen exporter's separate `.state`/`.blendtree` files.
#[derive(Clone)]
pub(super) struct DroppedControllerDocument {
    /// 1102 for states, 206 for BlendTrees.
    pub(super) class_id: i32,
    /// Asset file stem (state name, or `Blend Tree`).
    pub(super) name: String,
    /// Class-main anchor (`110200000`/`20600000`) shared by every separate file.
    pub(super) main_file_id: i64,
    /// Freshly minted file GUID for cross-file references.
    pub(super) guid: [u8; 16],
    /// Single YAML document, anchored at `main_file_id`.
    pub(super) document: UnityYamlDocument,
    /// Creation sequence for deterministic separate-file emission order,
    /// matching the source engine's pre-order creation.
    pub(super) emit_order: u64,
}

/// Rebuilt controller file documents plus the sub-assets that fell out of the
/// hierarchy and must export as separate files.
pub(super) struct RebuiltControllerDocuments {
    pub(super) documents: Vec<UnityYamlDocument>,
    pub(super) dropped: Vec<DroppedControllerDocument>,
}

/// Cross-file reference target for one dropped sub-asset, keyed by the
/// allocator file ID the kept documents used before the split.
#[derive(Clone, Copy, Debug)]
struct DroppedTarget {
    main_file_id: i64,
    guid: [u8; 16],
    emit_order: u64,
}

/// Tracks dropped sub-assets while rebuilding one controller file: allocator
/// file IDs map to their separate-file identities, and finished separate-file
/// documents accumulate for the planning pass.
struct DropContext<'m> {
    next_guid_sequence: &'m mut u64,
    used_guids: &'m mut HashSet<[u8; 16]>,
    targets: HashMap<i64, DroppedTarget>,
    dropped: Vec<DroppedControllerDocument>,
    next_emit_order: u64,
}

impl DropContext<'_> {
    fn mint_target(
        &mut self,
        local_file_id: i64,
        class_id: i32,
    ) -> Result<DroppedTarget, Box<dyn Error>> {
        let main_file_id =
            crate::export::ids::get_main_export_id(class_id, 0).map_err(|error| {
                invalid_project(format!(
                    "AnimatorController dropped asset cannot take a class-main file ID: {error}"
                ))
            })?;
        let guid = super::scene_collections::mint_unique_collection_guid(
            self.next_guid_sequence,
            self.used_guids,
        )?;
        let emit_order = self.next_emit_order;
        self.next_emit_order = emit_order
            .checked_add(1)
            .ok_or_else(|| invalid_project("AnimatorController dropped asset order overflows"))?;
        let target = DroppedTarget {
            main_file_id,
            guid,
            emit_order,
        };
        self.targets.try_reserve(1).map_err(|_| {
            invalid_project("could not allocate AnimatorController dropped asset mapping")
        })?;
        self.targets.insert(local_file_id, target);
        Ok(target)
    }
}

/// Attach every state to the first state-accepting machine with a matching
/// path ID, collecting the indexes that stay unattached.
///
/// Mirrors `AnimatorStateMachineContext.AssignStateMachineChildStates`, which
/// skips `Name.IsEmpty` machines: states of unnamed machines (e.g. empty-path
/// TOS entries such as `.AutoDresserTo0`) never attach and export as separate
/// files instead of embedding in the controller document.
fn attach_states_to_machines(
    nodes: &mut [EditorStateMachine],
    states: &[StateConstant],
) -> Result<Vec<usize>, Box<dyn Error>> {
    let mut dropped_state_indexes = Vec::new();
    for (state_index, state) in states.iter().enumerate() {
        let owner_index = nodes
            .iter()
            .position(|node| node.accepts_states && node.full_path_id == state.parent_path_id);
        let Some(owner_index) = owner_index else {
            dropped_state_indexes.try_reserve(1).map_err(|_| {
                invalid_project("could not allocate AnimatorController dropped states")
            })?;
            dropped_state_indexes.push(state_index);
            continue;
        };
        nodes[owner_index]
            .state_indexes
            .try_reserve(1)
            .map_err(|_| invalid_project("could not allocate AnimatorController child states"))?;
        nodes[owner_index].state_indexes.push(state_index);
    }
    Ok(dropped_state_indexes)
}

/// Cross-file `{fileID, guid, type: 2}` pointer matching the frozen exporter's
/// references from controller files into separate `.state`/`.blendtree` files.
fn foreign_pointer(file_id: i64, guid: [u8; 16]) -> UnityYamlNode {
    let mut pointer = UnityYamlNode::mapping(UnityYamlMappingStyle::Flow);
    pointer
        .insert_field("fileID", signed(file_id))
        .expect("foreign pointer mapping is valid");
    pointer
        .insert_field("guid", UnityYamlNode::string(format_unity_guid(guid)))
        .expect("foreign pointer mapping is valid");
    pointer
        .insert_field("type", signed(2))
        .expect("foreign pointer mapping is valid");
    pointer
}

/// Rebuilds Unity's editor-facing controller hierarchy from the packed controller constants.
/// The 2022 source exporter consumes those constants to create actual AnimatorState and
/// AnimatorStateMachine sub-assets; writing the constants directly leaves a valid-looking but
/// unusable `.controller` file.
pub(super) fn rebuild_animator_controller_documents(
    mut root_document: UnityYamlDocument,
    payload: &ParsedObjectPayload,
    little_endian: bool,
    project_version: crate::processing::scene_naming::SceneUnityVersion,
    next_guid_sequence: &mut u64,
    used_guids: &mut std::collections::HashSet<[u8; 16]>,
) -> Result<RebuiltControllerDocuments, Box<dyn Error>> {
    let root_file_id = document_file_id(&root_document)?;
    let controller_constant = required_field(&payload.root, "m_Controller", "AnimatorController")?;
    let mut tos = read_tos(required_field(
        &payload.root,
        "m_TOS",
        "AnimatorController",
    )?)?;
    let clips = read_clip_nodes(&root_document)?;
    let parameters = read_parameters(controller_constant, &tos, little_endian)?;
    let layers = read_layers(controller_constant, &tos)?;
    let constants = read_state_machine_constants(controller_constant)?;
    for raw in &constants {
        if !array_field(raw, "m_StateConstantArray", "StateMachineConstant")?.is_empty() {
            // AnimatorStateContext.Process adds this implicit empty-string entry before
            // resolving optional state parameter/tag names.
            tos.entry(0).or_default();
            break;
        }
    }

    if constants.len()
        != layers
            .iter()
            .map(|layer| layer.state_machine_index)
            .max()
            .map_or(0, |index| index + 1)
    {
        return Err(invalid_project(format!(
            "AnimatorController has {} StateMachineConstants, but its layer references do not cover the source array ({} layers)",
            constants.len(),
            layers.len()
        )));
    }

    let mut allocator = FileIdAllocator::new();
    allocator.reserved_root = Some(root_file_id);
    let mut machines = Vec::new();
    machines
        .try_reserve_exact(constants.len())
        .map_err(|_| invalid_project("could not allocate AnimatorController state machines"))?;

    for (machine_index, raw) in constants.iter().enumerate() {
        let layer = layers
            .iter()
            .find(|layer| {
                layer.state_machine_index == machine_index && layer.synchronized_layer_index == 0
            })
            .ok_or_else(|| {
                invalid_project(format!(
                    "AnimatorController StateMachineArray[{machine_index}] has no owning layer"
                ))
            })?;
        machines.push(rebuild_state_machine_hierarchy(
            raw,
            layer,
            &tos,
            &mut allocator,
        )?);
    }

    validate_layer_state_machines(&layers, &machines)?;
    behaviours::bind_behaviours(
        &root_document,
        &behaviours::BehaviourTable::read(&payload.root)?,
        &layers,
        &mut machines,
    )?;

    // Separate-file identities for dropped states are minted before any
    // document references them, so kept transitions and default states resolve
    // cross-file pointers in a single pass.
    let mut drop = DropContext {
        next_guid_sequence,
        used_guids,
        targets: HashMap::new(),
        dropped: Vec::new(),
        next_emit_order: 0,
    };
    for machine in &machines {
        for &state_index in &machine.dropped_state_indexes {
            let local_file_id = machine.states[state_index].file_id;
            drop.mint_target(local_file_id, ANIMATOR_STATE_CLASS_ID)?;
        }
    }

    let mut motion_documents = Vec::new();
    let mut transition_documents = Vec::new();
    for machine in &mut machines {
        for (state_index, state) in machine.states.iter_mut().enumerate() {
            let is_dropped = machine.dropped_state_indexes.contains(&state_index);
            // Dropped states route their BlendTree documents into the drop
            // context directly; the returned document list stays empty there.
            let drop_context = if is_dropped { Some(&mut drop) } else { None };
            let (motion, documents) = state_motion(
                &state.raw,
                &clips,
                &tos,
                project_version,
                &mut allocator,
                drop_context,
            )?;
            state.motion = motion;
            if !is_dropped {
                motion_documents.try_reserve(documents.len()).map_err(|_| {
                    invalid_project("could not allocate AnimatorController BlendTree documents")
                })?;
                motion_documents.extend(documents);
            }
        }
        rebuild_machine_transitions(
            machine,
            &tos,
            &mut allocator,
            &mut transition_documents,
            &drop.targets,
        )?;
    }

    let mut root_body = take_controller_body(&root_document)?;
    let mut generated_fields = HashMap::new();
    generated_fields.insert(
        "m_AnimatorParameters",
        parameter_nodes(&parameters, root_file_id)?,
    );
    generated_fields.insert(
        "m_AnimatorLayers",
        layer_nodes(&layers, &machines, root_file_id)?,
    );
    insert_generated_controller_fields(&mut root_body, generated_fields)?;
    if project_version.major >= 2022 {
        root_body = normalize_modern_controller_root_body(&root_body)?;
    }
    root_document = replace_controller_body(root_document, root_body)?;

    let mut documents = Vec::new();
    let generated_count = machines
        .iter()
        .try_fold(0usize, |count, machine| {
            count
                .checked_add(machine.machines.len())
                .and_then(|count| count.checked_add(machine.states.len()))
                .and_then(|count| {
                    count.checked_add(
                        machine
                            .machines
                            .iter()
                            .filter(|node| node.synthetic_default_state.is_some())
                            .count(),
                    )
                })
        })
        .and_then(|count| count.checked_add(transition_documents.len()))
        .and_then(|count| count.checked_add(motion_documents.len()))
        .ok_or_else(|| invalid_project("AnimatorController document count overflows"))?;
    documents
        .try_reserve_exact(generated_count.saturating_add(1))
        .map_err(|_| invalid_project("could not allocate AnimatorController YAML documents"))?;
    documents.push(root_document);

    for machine in &machines {
        for node in &machine.machines {
            let child_count = node.state_indexes.len() + node.child_machine_indexes.len();
            for (child_index, &state_index) in node.state_indexes.iter().enumerate() {
                documents.push(state_document(
                    &machine.states[state_index],
                    child_index,
                    child_count,
                    &tos,
                )?);
            }
            if let Some(state) = &node.synthetic_default_state {
                documents.push(state_document(state, 0, 1, &tos)?);
            }
            documents.push(state_machine_document(
                node,
                machine,
                project_version,
                &drop.targets,
            )?);
        }
        // Dropped states export as separate single-asset files with class-main
        // anchors, mirroring the frozen exporter's orphan `.state` files.
        for &state_index in &machine.dropped_state_indexes {
            let state = &machine.states[state_index];
            let target = drop.targets.get(&state.file_id).ok_or_else(|| {
                invalid_project("AnimatorController dropped state has no separate-file identity")
            })?;
            let mut document = state_document(state, 0, 1, &tos)?;
            set_document_anchor(&mut document, target.main_file_id)?;
            drop.dropped.try_reserve(1).map_err(|_| {
                invalid_project("could not allocate AnimatorController dropped state documents")
            })?;
            drop.dropped.push(DroppedControllerDocument {
                class_id: ANIMATOR_STATE_CLASS_ID,
                name: state.name.clone(),
                main_file_id: target.main_file_id,
                guid: target.guid,
                document,
                emit_order: target.emit_order,
            });
        }
    }
    documents.extend(motion_documents);
    documents.extend(transition_documents);

    if documents.first().map(document_file_id).transpose()? != Some(root_file_id) {
        return Err(invalid_project(
            "AnimatorController root document identity changed during reconstruction",
        ));
    }
    // Emit separate files in creation order so identically named assets take
    // the same file stems on both exporters.
    drop.dropped.sort_by_key(|document| document.emit_order);
    Ok(RebuiltControllerDocuments {
        documents,
        dropped: drop.dropped,
    })
}

/// Rewrite a rebuilt document's anchor to its separate-file class-main ID.
fn set_document_anchor(
    document: &mut UnityYamlDocument,
    file_id: i64,
) -> Result<(), Box<dyn Error>> {
    document.root.anchor = Some(file_id.to_string());
    Ok(())
}

fn read_tos(value: &SerializedValue) -> Result<HashMap<u32, String>, Box<dyn Error>> {
    let entries = match value {
        SerializedValue::Array(values) => values.as_slice(),
        SerializedValue::Struct(_) => {
            let pairs = optional_field(value, "m_Pairs")?
                .or(optional_field(value, "Array")?)
                .ok_or_else(|| {
                    invalid_project("AnimatorController TOS dictionary has no pair array")
                })?;
            as_array(pairs, "AnimatorController TOS pair array")?
        }
        _ => {
            return Err(invalid_project(
                "AnimatorController TOS is not a serialized dictionary",
            ))
        }
    };
    let mut tos = HashMap::new();
    tos.try_reserve(entries.len())
        .map_err(|_| invalid_project("could not allocate AnimatorController TOS map"))?;
    for (index, raw_pair) in entries.iter().enumerate() {
        let pair = unwrap_data_or_self(raw_pair, "AnimatorController TOS pair")?;
        let key = required_u32_field(pair, "first", "AnimatorController TOS pair")?;
        let bytes = required_field(pair, "second", "AnimatorController TOS pair")?;
        let SerializedValue::StringBytes(bytes) = bytes else {
            return Err(invalid_project(format!(
                "AnimatorController TOS pair {index} value is not a string"
            )));
        };
        let text = std::str::from_utf8(bytes).map_err(|error| {
            invalid_project(format!(
                "AnimatorController TOS pair {index} is invalid UTF-8: {error}"
            ))
        })?;
        if tos.insert(key, text.to_owned()).is_some() {
            return Err(invalid_project(format!(
                "AnimatorController TOS contains duplicate key {key}"
            )));
        }
    }
    Ok(tos)
}

fn read_parameters(
    controller: &SerializedValue,
    tos: &HashMap<u32, String>,
    little_endian: bool,
) -> Result<Vec<ControllerParameter>, Box<dyn Error>> {
    let value_group = required_field(controller, "m_Values", "ControllerConstant")?;
    let value_data = unwrap_data(value_group, "ControllerConstant Values")?;
    let constants = array_field(value_data, "m_ValueArray", "ValueArray")?;
    let defaults = unwrap_data(
        required_field(controller, "m_DefaultValues", "ControllerConstant")?,
        "DefaultValues",
    )?;
    let floats = required_field(defaults, "m_FloatValues", "DefaultValues")?;
    let ints = required_field(defaults, "m_IntValues", "DefaultValues")?;
    let bools = required_field(defaults, "m_BoolValues", "DefaultValues")?;
    let mut output = Vec::new();
    output
        .try_reserve_exact(constants.len())
        .map_err(|_| invalid_project("could not allocate AnimatorController parameters"))?;
    for (index, raw_value) in constants.iter().enumerate() {
        let value = unwrap_data(raw_value, "ValueConstant")?;
        let type_id = required_u32_field(value, "m_Type", "ValueConstant")? as i32;
        let name_id = required_u32_field(value, "m_ID", "ValueConstant")?;
        let name = tos.get(&name_id).cloned().ok_or_else(|| {
            invalid_project(format!(
                "AnimatorController ValueArray[{index}] has no TOS name for key {name_id}"
            ))
        })?;
        let value_index = usize::try_from(required_u32_field(value, "m_Index", "ValueConstant")?)
            .map_err(|_| {
            invalid_project("AnimatorController parameter index does not fit usize")
        })?;
        let (default_float, default_int, default_bool) = match type_id {
            1 => (
                array_f32_at(floats, value_index, little_endian, "float defaults")?,
                0,
                false,
            ),
            3 => (
                0.0,
                array_i32_at(ints, value_index, little_endian, "integer defaults")?,
                false,
            ),
            4 | 9 => (
                0.0,
                0,
                array_bool_at(bools, value_index, "boolean defaults")?,
            ),
            other => {
                return Err(invalid_project(format!(
                    "AnimatorController parameter {name:?} has unsupported serialized type {other}"
                )))
            }
        };
        output.push(ControllerParameter {
            name,
            parameter_type: type_id,
            default_float,
            default_int,
            default_bool,
        });
    }
    Ok(output)
}

fn read_layers(
    controller: &SerializedValue,
    tos: &HashMap<u32, String>,
) -> Result<Vec<ControllerLayer>, Box<dyn Error>> {
    let raw_layers = array_field(controller, "m_LayerArray", "ControllerConstant")?;
    let mut layers = Vec::new();
    layers
        .try_reserve_exact(raw_layers.len())
        .map_err(|_| invalid_project("could not allocate AnimatorController layers"))?;
    for (index, raw_layer) in raw_layers.iter().enumerate() {
        let layer = unwrap_data(raw_layer, "LayerConstant")?;
        let binding = required_u32_field(layer, "m_Binding", "LayerConstant")?;
        let name = tos.get(&binding).cloned().ok_or_else(|| {
            invalid_project(format!(
                "AnimatorController LayerArray[{index}] has no TOS name for binding {binding}"
            ))
        })?;
        layers.push(ControllerLayer {
            name,
            binding_path_id: binding,
            state_machine_index: usize::try_from(required_u32_field(
                layer,
                "m_StateMachineIndex",
                "LayerConstant",
            )?)
            .map_err(|_| invalid_project("AnimatorController layer index does not fit usize"))?,
            synchronized_layer_index: required_u32_field(
                layer,
                "m_StateMachineSynchronizedLayerIndex",
                "LayerConstant",
            )?,
            default_weight: required_f32_field(layer, "m_DefaultWeight", "LayerConstant")?,
            blending_mode: required_i32_field_alias(
                layer,
                &["m_LayerBlendingMode", "(int&)m_LayerBlendingMode"],
                "LayerConstant",
            )?,
            ik_pass: required_bool_field(layer, "m_IKPass", "LayerConstant")?,
            synced_layer_affects_timing: required_bool_field(
                layer,
                "m_SyncedLayerAffectsTiming",
                "LayerConstant",
            )?,
        });
    }
    Ok(layers)
}

fn read_state_machine_constants(
    controller: &SerializedValue,
) -> Result<Vec<SerializedValue>, Box<dyn Error>> {
    let raw = array_field(controller, "m_StateMachineArray", "ControllerConstant")?;
    let mut result = Vec::new();
    result
        .try_reserve_exact(raw.len())
        .map_err(|_| invalid_project("could not allocate AnimatorController constants"))?;
    for value in raw {
        result.push(unwrap_data(value, "StateMachineConstant")?.clone());
    }
    Ok(result)
}

fn legacy_default_state_index(
    raw: &SerializedValue,
    state_count: usize,
) -> Result<Option<usize>, Box<dyn Error>> {
    if state_count == 0 {
        return Ok(None);
    }
    let serialized_index = optional_u32_field(raw, "m_DefaultState")?.unwrap_or(u32::MAX);
    let index = if serialized_index == u32::MAX {
        0
    } else {
        usize::try_from(serialized_index).map_err(|_| {
            invalid_project("AnimatorController default state index does not fit usize")
        })?
    };
    if index >= state_count {
        return Err(invalid_project(format!(
            "AnimatorController legacy default state index {index} is outside {state_count} states"
        )));
    }
    Ok(Some(index))
}

#[derive(Clone)]
struct SelectorState {
    full_path_id: u32,
    is_entry: bool,
    transitions: Vec<SerializedValue>,
}

fn read_selector_states(
    state_machine: &SerializedValue,
) -> Result<Vec<SelectorState>, Box<dyn Error>> {
    let Some(raw_selectors) = optional_field(state_machine, "m_SelectorStateConstantArray")? else {
        return Ok(Vec::new());
    };
    let raw_selectors = as_array(raw_selectors, "SelectorStateConstantArray")?;
    let mut result = Vec::new();
    result
        .try_reserve_exact(raw_selectors.len())
        .map_err(|_| invalid_project("could not allocate AnimatorController selector states"))?;
    for (index, raw_selector) in raw_selectors.iter().enumerate() {
        let selector = unwrap_data(raw_selector, "SelectorStateConstant")?;
        let transitions = array_field(
            selector,
            "m_TransitionConstantArray",
            "SelectorStateConstant",
        )?;
        let mut decoded = Vec::new();
        decoded.try_reserve_exact(transitions.len()).map_err(|_| {
            invalid_project("could not allocate AnimatorController selector transitions")
        })?;
        for transition in transitions {
            decoded.push(unwrap_data(transition, "SelectorTransitionConstant")?.clone());
        }
        let _ = index;
        result.push(SelectorState {
            full_path_id: required_u32_field(selector, "m_FullPathID", "SelectorStateConstant")?,
            is_entry: required_bool_field(selector, "m_IsEntry", "SelectorStateConstant")?,
            transitions: decoded,
        });
    }
    Ok(result)
}

fn rebuild_state_machine_hierarchy(
    raw: &SerializedValue,
    layer: &ControllerLayer,
    tos: &HashMap<u32, String>,
    allocator: &mut FileIdAllocator,
) -> Result<StateMachineConstant, Box<dyn Error>> {
    let root_file_id = allocator.allocate()?;
    let has_selector_array = optional_field(raw, "m_SelectorStateConstantArray")?.is_some();
    let selectors = read_selector_states(raw)?;
    let raw_states = array_field(raw, "m_StateConstantArray", "StateMachineConstant")?;
    let legacy_default_state_index = if has_selector_array {
        None
    } else {
        legacy_default_state_index(raw, raw_states.len())?
    };
    let mut states = Vec::new();
    states
        .try_reserve_exact(raw_states.len())
        .map_err(|_| invalid_project("could not allocate AnimatorController states"))?;
    let mut paths = HashMap::new();
    paths.try_reserve(raw_states.len()).map_err(|_| {
        invalid_project("could not allocate AnimatorController state-machine paths")
    })?;
    for (state_index, raw_state) in raw_states.iter().enumerate() {
        let state = unwrap_data(raw_state, "StateConstant")?;
        let state_path_id = match optional_u32_field(state, "m_FullPathID")? {
            Some(path_id) => path_id,
            None => match optional_u32_field(state, "m_PathID")? {
                Some(path_id) => path_id,
                None => required_u32_field(state, "m_ID", "StateConstant")?,
            },
        };
        let name = if let Some(name_id) = optional_u32_field(state, "m_NameID")? {
            tos_name(tos, name_id, "StateConstant name")?
        } else {
            let legacy_path = tos_name(
                tos,
                required_u32_field(state, "m_ID", "StateConstant")?,
                "StateConstant legacy name",
            )?;
            legacy_path
                .split_once('.')
                .filter(|(_, name)| !name.is_empty())
                .map_or(legacy_path.clone(), |(_, name)| name.to_owned())
        };
        let path = tos_name(tos, state_path_id, "StateConstant full path")?;
        let parent_path = path.strip_suffix(&format!(".{name}")).ok_or_else(|| {
            invalid_project(format!(
                "AnimatorController StateConstantArray[{state_index}] path {path:?} does not end in its name {name:?}"
            ))
        })?;
        let parent_path_id = crc32_ieee(parent_path.as_bytes());
        // AnimatorStateContext also recovers ancestors of state-owning machines, so an
        // empty intermediate node needs no invented name or special handling.
        let mut ancestor = Some(parent_path);
        while let Some(path) = ancestor {
            let path_id = crc32_ieee(path.as_bytes());
            if let Some(previous) = paths.get(&path_id) {
                if previous != path {
                    return Err(invalid_project(format!(
                        "AnimatorController state-machine paths {previous:?} and {path:?} collide at CRC {path_id}"
                    )));
                }
            } else {
                paths.try_reserve(1).map_err(|_| {
                    invalid_project("could not allocate AnimatorController path index")
                })?;
                paths.insert(path_id, path.to_owned());
            }
            ancestor = path.rsplit_once('.').map(|(parent, _)| parent);
        }
        states.push(StateConstant {
            raw: state.clone(),
            name,
            state_path_id,
            parent_path_id,
            file_id: allocator.allocate()?,
            motion: local_pointer(0),
            transition_file_ids: Vec::new(),
            behaviours: Vec::new(),
        });
    }

    let resolved = if has_selector_array {
        let mut path_ids = Vec::new();
        path_ids
            .try_reserve_exact(selectors.len().max(1))
            .map_err(|_| invalid_project("could not allocate AnimatorController selector paths"))?;
        for selector in &selectors {
            if path_ids.last() != Some(&selector.full_path_id) {
                if path_ids.contains(&selector.full_path_id) {
                    return Err(invalid_project(format!(
                        "AnimatorController selector path {} occurs in non-contiguous groups",
                        selector.full_path_id
                    )));
                }
                path_ids.push(selector.full_path_id);
            }
        }
        if path_ids.is_empty() {
            path_ids.push(layer.binding_path_id);
        }
        // Selector FullPathIDs identify state machines. Layer.Binding supplies the root
        // name only when there are no states; with states, names come from their TOS paths.
        hierarchy::resolve_machine_paths(
            &path_ids,
            &mut paths,
            &states,
            &selectors,
            raw,
            &layer.name,
        )?
    } else {
        hierarchy::resolve_legacy_machine_paths(
            &states,
            &paths,
            layer.binding_path_id,
            &layer.name,
            legacy_default_state_index,
        )?
    };
    let mut nodes = Vec::new();
    nodes.try_reserve_exact(resolved.len()).map_err(|_| {
        invalid_project("could not allocate AnimatorController editor state machines")
    })?;
    for (index, path) in resolved.into_iter().enumerate() {
        let file_id = if index == 0 {
            root_file_id
        } else {
            allocator.allocate()?
        };
        let synthetic_default_state = if path.is_extra {
            Some(source_default_animator_state(allocator.allocate()?)?)
        } else {
            None
        };
        let default_state_file_id = synthetic_default_state
            .as_ref()
            .map_or(0, |state| state.file_id);
        // The legacy reconstruction names every machine from a TOS path and
        // attaches all states; only selector-driven machines can be unnamed.
        let accepts_states = !has_selector_array || !path.name.is_empty();
        nodes.push(EditorStateMachine {
            file_id,
            full_path_id: path.full_path_id,
            name: path.name,
            parent_index: path.parent_index,
            state_indexes: Vec::new(),
            child_machine_indexes: Vec::new(),
            any_transition_file_ids: Vec::new(),
            entry_transition_file_ids: Vec::new(),
            child_exit_transitions: Vec::new(),
            default_state_file_id,
            synthetic_default_state,
            behaviours: Vec::new(),
            accepts_states,
        });
    }
    for index in 1..nodes.len() {
        let parent = nodes[index].parent_index.ok_or_else(|| {
            invalid_project("AnimatorController nested machine has no recovered parent")
        })?;
        nodes[parent]
            .child_machine_indexes
            .try_reserve(1)
            .map_err(|_| invalid_project("could not allocate AnimatorController child machines"))?;
        nodes[parent].child_machine_indexes.push(index);
    }
    // A selector group must describe one rooted hierarchy. Parent paths strictly shorten,
    // and this check also catches a root selector which names a non-root ancestor.
    for index in 1..nodes.len() {
        let mut current = index;
        let mut remaining = nodes.len();
        while current != 0 {
            if remaining == 0 {
                return Err(invalid_project(
                    "AnimatorController machine hierarchy contains a cycle",
                ));
            }
            current = nodes[current].parent_index.ok_or_else(|| {
                invalid_project("AnimatorController machine hierarchy is disconnected")
            })?;
            remaining -= 1;
        }
    }
    let dropped_state_indexes = attach_states_to_machines(&mut nodes, &states)?;
    for node in &mut nodes {
        if node.synthetic_default_state.is_some() {
            continue;
        }
        if !has_selector_array {
            if node.parent_index.is_none() {
                if let Some(index) = legacy_default_state_index {
                    node.default_state_file_id = states[index].file_id;
                }
            }
            continue;
        }
        let mut entries = selectors
            .iter()
            .filter(|selector| selector.full_path_id == node.full_path_id && selector.is_entry);
        let entry = entries.next();
        if entries.next().is_some() {
            return Err(invalid_project(
                "AnimatorController machine has duplicate entry selectors",
            ));
        }
        let mut exits = selectors
            .iter()
            .filter(|selector| selector.full_path_id == node.full_path_id && !selector.is_entry);
        exits.next();
        if exits.next().is_some() {
            return Err(invalid_project(
                "AnimatorController machine has duplicate exit selectors",
            ));
        }
        let destination = match entry {
            Some(entry) => entry
                .transitions
                .last()
                .map(|transition| {
                    required_u32_field(transition, "m_Destination", "SelectorTransitionConstant")
                })
                .transpose()?
                .unwrap_or(u32::MAX),
            None => u32::MAX,
        };
        if destination != u32::MAX {
            node.default_state_file_id = states.get(destination as usize)
                .ok_or_else(|| invalid_project(format!(
                    "AnimatorController machine {:?} default state index {destination} is out of bounds",
                    node.name
                )))?.file_id;
        }
    }
    Ok(StateMachineConstant {
        raw: raw.clone(),
        states,
        selectors,
        machines: nodes,
        dropped_state_indexes,
    })
}

#[derive(Clone, Copy, Debug)]
struct TransitionDestination {
    state_file_id: i64,
    machine_file_id: i64,
    is_exit: bool,
    /// Separate-file identity when the destination state fell out of the
    /// hierarchy; kept documents reference it cross-file.
    state_foreign: Option<DroppedTarget>,
}

fn transition_destination(
    machine: &StateMachineConstant,
    destination: u32,
    drop_targets: &HashMap<i64, DroppedTarget>,
) -> Result<Option<TransitionDestination>, Box<dyn Error>> {
    if destination == u32::MAX {
        return Ok(None);
    }
    let mut result = TransitionDestination {
        state_file_id: 0,
        machine_file_id: 0,
        is_exit: false,
        state_foreign: None,
    };
    if destination < TRANSITION_DESTINATION_FLAG {
        let state = machine.states.get(destination as usize).ok_or_else(|| {
            invalid_project(format!(
                "AnimatorController transition targets absent state index {destination}"
            ))
        })?;
        result.state_file_id = state.file_id;
        // Transitions into dropped states stay embedded but point cross-file,
        // matching the frozen exporter's kept `m_DstState` references.
        result.state_foreign = drop_targets.get(&state.file_id).copied();
    } else {
        let selector_index = (destination % TRANSITION_DESTINATION_FLAG) as usize;
        let selector = machine.selectors.get(selector_index).ok_or_else(|| {
            invalid_project(format!(
                "AnimatorController transition targets absent selector index {selector_index}"
            ))
        })?;
        let node = machine
            .machines
            .iter()
            .find(|node| node.full_path_id == selector.full_path_id)
            .ok_or_else(|| {
                invalid_project(
                    "AnimatorController transition selector has no reconstructed machine",
                )
            })?;
        if selector.is_entry {
            result.machine_file_id = node.file_id;
        } else {
            result.is_exit = true;
        }
    }
    Ok(Some(result))
}

fn rebuild_machine_transitions(
    machine: &mut StateMachineConstant,
    tos: &HashMap<u32, String>,
    allocator: &mut FileIdAllocator,
    documents: &mut Vec<UnityYamlDocument>,
    drop_targets: &HashMap<i64, DroppedTarget>,
) -> Result<(), Box<dyn Error>> {
    // States remain globally indexed even after their owning nodes are determined.
    // Dropped states carry no transitions: the source never attaches them, so
    // their separate `.state` files export with empty transition lists.
    for state_index in 0..machine.states.len() {
        if machine.dropped_state_indexes.contains(&state_index) {
            continue;
        }
        let transitions = array_field(
            &machine.states[state_index].raw,
            "m_TransitionConstantArray",
            "StateConstant",
        )?
        .to_vec();
        for raw in &transitions {
            let transition = unwrap_data(raw, "TransitionConstant")?;
            if let Some(destination) = transition_destination(
                machine,
                required_u32_field(transition, "m_DestinationState", "TransitionConstant")?,
                drop_targets,
            )? {
                let file_id = allocator.allocate()?;
                documents.push(state_transition_document(
                    transition,
                    tos,
                    destination,
                    file_id,
                )?);
                machine.states[state_index]
                    .transition_file_ids
                    .push(file_id);
            }
        }
    }
    if !machine.states.is_empty() {
        for raw in array_field(
            &machine.raw,
            "m_AnyStateTransitionConstantArray",
            "StateMachineConstant",
        )? {
            let transition = unwrap_data(raw, "TransitionConstant")?;
            if let Some(destination) = transition_destination(
                machine,
                required_u32_field(transition, "m_DestinationState", "TransitionConstant")?,
                drop_targets,
            )? {
                let file_id = allocator.allocate()?;
                documents.push(state_transition_document(
                    transition,
                    tos,
                    destination,
                    file_id,
                )?);
                machine.machines[0].any_transition_file_ids.push(file_id);
            }
        }
    }
    for selector_index in 0..machine.selectors.len() {
        let selector = &machine.selectors[selector_index];
        if !selector.is_entry {
            continue;
        }
        let node_index = machine
            .machines
            .iter()
            .position(|node| node.full_path_id == selector.full_path_id)
            .ok_or_else(|| invalid_project("AnimatorController entry selector has no machine"))?;
        for transition in selector
            .transitions
            .iter()
            .take(selector.transitions.len().saturating_sub(1))
        {
            if let Some(destination) = transition_destination(
                machine,
                required_u32_field(transition, "m_Destination", "SelectorTransitionConstant")?,
                drop_targets,
            )? {
                let file_id = allocator.allocate()?;
                documents.push(selector_transition_document(
                    transition,
                    tos,
                    destination,
                    file_id,
                )?);
                machine.machines[node_index]
                    .entry_transition_file_ids
                    .push(file_id);
            }
        }
    }
    for child_index in 1..machine.machines.len() {
        let child = &machine.machines[child_index];
        let parent_index = child
            .parent_index
            .ok_or_else(|| invalid_project("AnimatorController child exit has no parent"))?;
        let Some(selector) = machine
            .selectors
            .iter()
            .find(|selector| selector.full_path_id == child.full_path_id && !selector.is_entry)
        else {
            continue;
        };
        if selector.transitions.len() == 1 {
            let transition = &selector.transitions[0];
            let destination =
                required_u32_field(transition, "m_Destination", "SelectorTransitionConstant")?;
            if destination < TRANSITION_DESTINATION_FLAG
                && array_field(
                    transition,
                    "m_ConditionConstantArray",
                    "SelectorTransitionConstant",
                )?
                .is_empty()
                && machine
                    .states
                    .get(destination as usize)
                    .map(|state| state.file_id)
                    == Some(machine.machines[parent_index].default_state_file_id)
            {
                continue;
            }
        }
        let mut transition_ids = Vec::new();
        transition_ids
            .try_reserve_exact(selector.transitions.len())
            .map_err(|_| {
                invalid_project("could not allocate AnimatorController child exit transitions")
            })?;
        for transition in &selector.transitions {
            if let Some(destination) = transition_destination(
                machine,
                required_u32_field(transition, "m_Destination", "SelectorTransitionConstant")?,
                drop_targets,
            )? {
                let file_id = allocator.allocate()?;
                documents.push(selector_transition_document(
                    transition,
                    tos,
                    destination,
                    file_id,
                )?);
                transition_ids.push(file_id);
            }
        }
        if !transition_ids.is_empty() {
            machine.machines[parent_index]
                .child_exit_transitions
                .push((child_index, transition_ids));
        }
    }
    Ok(())
}

fn validate_layer_state_machines(
    layers: &[ControllerLayer],
    machines: &[StateMachineConstant],
) -> Result<(), Box<dyn Error>> {
    for (index, layer) in layers.iter().enumerate() {
        machines.get(layer.state_machine_index).ok_or_else(|| {
            invalid_project(format!(
                "AnimatorController LayerArray[{index}] references absent StateMachineArray[{}]",
                layer.state_machine_index
            ))
        })?;
        // ID 0 is the CRC32 of the empty root path. AnimatorStateContext deliberately
        // seeds that path when states exist, so zero is a valid root identity here.
    }
    Ok(())
}

fn read_clip_nodes(document: &UnityYamlDocument) -> Result<Vec<UnityYamlNode>, Box<dyn Error>> {
    let body = controller_body(document)?;
    let Some(clips) = node_field(body, "m_AnimationClips")? else {
        return Ok(Vec::new());
    };
    let UnityYamlNodeKind::Sequence { items, .. } = &clips.kind else {
        return Err(invalid_project(
            "AnimatorController m_AnimationClips YAML field is not a sequence",
        ));
    };
    Ok(items.clone())
}

fn parameter_nodes(
    parameters: &[ControllerParameter],
    controller_file_id: i64,
) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut output = UnityYamlNode::sequence(UnityYamlSequenceStyle::Block);
    for parameter in parameters {
        let mut node = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        insert_int(
            &mut node,
            "m_Name",
            UnityYamlNode::string(parameter.name.clone()),
        )?;
        insert_int(
            &mut node,
            "m_Type",
            signed(i64::from(parameter.parameter_type)),
        )?;
        insert_int(
            &mut node,
            "m_DefaultFloat",
            float32(parameter.default_float),
        )?;
        insert_int(
            &mut node,
            "m_DefaultInt",
            signed(i64::from(parameter.default_int)),
        )?;
        insert_int(
            &mut node,
            "m_DefaultBool",
            signed(i64::from(parameter.default_bool)),
        )?;
        insert_int(&mut node, "m_Controller", local_pointer(controller_file_id))?;
        output.push(node)?;
    }
    Ok(output)
}

fn layer_nodes(
    layers: &[ControllerLayer],
    machines: &[StateMachineConstant],
    controller_file_id: i64,
) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut output = UnityYamlNode::sequence(UnityYamlSequenceStyle::Block);
    for layer in layers {
        let machine = machines.get(layer.state_machine_index).ok_or_else(|| {
            invalid_project(format!(
                "AnimatorController layer references absent state machine {}",
                layer.state_machine_index
            ))
        })?;
        let mut node = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        insert_int(&mut node, "serializedVersion", signed(5))?;
        insert_int(
            &mut node,
            "m_Name",
            UnityYamlNode::string(layer.name.clone()),
        )?;
        insert_int(
            &mut node,
            "m_StateMachine",
            local_pointer(machine.machines[0].file_id),
        )?;
        insert_int(&mut node, "m_Mask", local_pointer(0))?;
        insert_int(&mut node, "m_Motions", empty_sequence())?;
        insert_int(&mut node, "m_Behaviours", empty_sequence())?;
        insert_int(
            &mut node,
            "m_BlendingMode",
            signed(i64::from(layer.blending_mode)),
        )?;
        insert_int(
            &mut node,
            "m_SyncedLayerIndex",
            signed(i64::from(
                crate::resources::animator_enums::synced_layer_index(
                    layer.synchronized_layer_index,
                    layer.state_machine_index as u32,
                ),
            )),
        )?;
        insert_int(&mut node, "m_DefaultWeight", float32(layer.default_weight))?;
        insert_int(&mut node, "m_IKPass", signed(i64::from(layer.ik_pass)))?;
        insert_int(
            &mut node,
            "m_SyncedLayerAffectsTiming",
            signed(i64::from(layer.synced_layer_affects_timing)),
        )?;
        insert_int(&mut node, "m_Controller", local_pointer(controller_file_id))?;
        output.push(node)?;
    }
    Ok(output)
}

fn state_document(
    state: &StateConstant,
    state_index: usize,
    child_count: usize,
    tos: &HashMap<u32, String>,
) -> Result<UnityYamlDocument, Box<dyn Error>> {
    let mut body = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    insert_field(&mut body, "serializedVersion", signed(6))?;
    insert_field(&mut body, "m_ObjectHideFlags", signed(1))?;
    insert_field(&mut body, "m_CorrespondingSourceObject", local_pointer(0))?;
    insert_field(&mut body, "m_PrefabInstance", local_pointer(0))?;
    insert_field(&mut body, "m_PrefabAsset", local_pointer(0))?;
    insert_field(
        &mut body,
        "m_Name",
        UnityYamlNode::string(state.name.clone()),
    )?;
    insert_field(
        &mut body,
        "m_Speed",
        float32(required_f32_field(&state.raw, "m_Speed", "StateConstant")?),
    )?;
    insert_field(
        &mut body,
        "m_CycleOffset",
        float32(required_f32_field(
            &state.raw,
            "m_CycleOffset",
            "StateConstant",
        )?),
    )?;
    insert_field(
        &mut body,
        "m_Transitions",
        pointer_sequence(&state.transition_file_ids)?,
    )?;
    insert_field(
        &mut body,
        "m_StateMachineBehaviours",
        behaviour_nodes(&state.behaviours)?,
    )?;
    let side = integer_ceil_sqrt(child_count);
    insert_field(
        &mut body,
        "m_Position",
        position3(
            (state_index % side * 250) as i64,
            (state_index / side * 100) as i64,
            0,
        ),
    )?;
    insert_field(
        &mut body,
        "m_IKOnFeet",
        signed(i64::from(required_bool_field(
            &state.raw,
            "m_IKOnFeet",
            "StateConstant",
        )?)),
    )?;
    let write_default = optional_bool_field(&state.raw, "m_WriteDefaultValues")?.unwrap_or(true);
    insert_field(
        &mut body,
        "m_WriteDefaultValues",
        signed(i64::from(write_default)),
    )?;
    insert_field(
        &mut body,
        "m_Mirror",
        signed(i64::from(required_bool_field(
            &state.raw,
            "m_Mirror",
            "StateConstant",
        )?)),
    )?;
    let speed_parameter = required_u32_field(&state.raw, "m_SpeedParamID", "StateConstant")?;
    let mirror_parameter = required_u32_field(&state.raw, "m_MirrorParamID", "StateConstant")?;
    let cycle_parameter = required_u32_field(&state.raw, "m_CycleOffsetParamID", "StateConstant")?;
    let time_parameter = required_u32_field(&state.raw, "m_TimeParamID", "StateConstant")?;
    insert_field(
        &mut body,
        "m_SpeedParameterActive",
        signed(i64::from(speed_parameter > 0)),
    )?;
    insert_field(
        &mut body,
        "m_MirrorParameterActive",
        signed(i64::from(mirror_parameter > 0)),
    )?;
    insert_field(
        &mut body,
        "m_CycleOffsetParameterActive",
        signed(i64::from(cycle_parameter > 0)),
    )?;
    insert_field(
        &mut body,
        "m_TimeParameterActive",
        signed(i64::from(time_parameter > 0)),
    )?;
    insert_field(&mut body, "m_Motion", state.motion.clone())?;
    let tag = required_u32_field(&state.raw, "m_TagID", "StateConstant")?;
    insert_field(
        &mut body,
        "m_Tag",
        UnityYamlNode::string(tos_name(tos, tag, "StateConstant tag")?),
    )?;
    insert_field(
        &mut body,
        "m_SpeedParameter",
        UnityYamlNode::string(tos_name(
            tos,
            speed_parameter,
            "StateConstant speed parameter",
        )?),
    )?;
    insert_field(
        &mut body,
        "m_MirrorParameter",
        UnityYamlNode::string(tos_name(
            tos,
            mirror_parameter,
            "StateConstant mirror parameter",
        )?),
    )?;
    insert_field(
        &mut body,
        "m_CycleOffsetParameter",
        UnityYamlNode::string(tos_name(
            tos,
            cycle_parameter,
            "StateConstant cycle-offset parameter",
        )?),
    )?;
    insert_field(
        &mut body,
        "m_TimeParameter",
        UnityYamlNode::string(tos_name(
            tos,
            time_parameter,
            "StateConstant time parameter",
        )?),
    )?;
    UnityYamlDocument::object(
        ANIMATOR_STATE_CLASS_ID,
        state.file_id,
        "AnimatorState",
        body,
        false,
    )
    .map_err(|error| Box::new(error) as Box<dyn Error>)
}

/// VirtualAnimationFactory.CreateDefaultAnimatorState, used exclusively by
/// the reference processor's extra-machine fallback (not missing input data).
fn source_default_animator_state(file_id: i64) -> Result<StateConstant, Box<dyn Error>> {
    use crate::serialized_file::object::SerializedFieldValue;
    let mut fields = Vec::new();
    fields.try_reserve_exact(11).map_err(|_| {
        invalid_project("could not allocate AnimatorController extra default state")
    })?;
    for (name, value) in [
        ("m_Speed", SerializedValue::F32(1.0)),
        ("m_CycleOffset", SerializedValue::F32(0.0)),
        ("m_IKOnFeet", SerializedValue::Bool(false)),
        ("m_WriteDefaultValues", SerializedValue::Bool(true)),
        ("m_Mirror", SerializedValue::Bool(false)),
        ("m_SpeedParamID", SerializedValue::U32(0)),
        ("m_MirrorParamID", SerializedValue::U32(0)),
        ("m_CycleOffsetParamID", SerializedValue::U32(0)),
        ("m_TimeParamID", SerializedValue::U32(0)),
        ("m_TagID", SerializedValue::U32(0)),
    ] {
        fields.push(SerializedFieldValue {
            name: name.to_owned(),
            value,
        });
    }
    Ok(StateConstant {
        raw: SerializedValue::Struct(fields),
        name: "New State".to_owned(),
        state_path_id: 0,
        parent_path_id: 0,
        file_id,
        motion: local_pointer(0),
        transition_file_ids: Vec::new(),
        behaviours: Vec::new(),
    })
}

fn state_machine_document(
    node: &EditorStateMachine,
    machine: &StateMachineConstant,
    project_version: crate::processing::scene_naming::SceneUnityVersion,
    drop_targets: &HashMap<i64, DroppedTarget>,
) -> Result<UnityYamlDocument, Box<dyn Error>> {
    let mut body = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    insert_field(&mut body, "serializedVersion", signed(6))?;
    insert_field(&mut body, "m_ObjectHideFlags", signed(1))?;
    insert_field(&mut body, "m_CorrespondingSourceObject", local_pointer(0))?;
    insert_field(&mut body, "m_PrefabInstance", local_pointer(0))?;
    insert_field(&mut body, "m_PrefabAsset", local_pointer(0))?;
    insert_field(
        &mut body,
        "m_Name",
        UnityYamlNode::string(node.name.clone()),
    )?;
    let mut child_states = UnityYamlNode::sequence(UnityYamlSequenceStyle::Block);
    let state_count =
        node.state_indexes.len() + usize::from(node.synthetic_default_state.is_some());
    let side = integer_ceil_sqrt(state_count + node.child_machine_indexes.len());
    for (index, &state_index) in node.state_indexes.iter().enumerate() {
        let state = &machine.states[state_index];
        let mut child = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        insert_field(&mut child, "m_State", local_pointer(state.file_id))?;
        insert_field(
            &mut child,
            "m_Position",
            position3((index % side * 250) as i64, (index / side * 100) as i64, 0),
        )?;
        child_states.push(child)?;
    }
    if let Some(state) = &node.synthetic_default_state {
        let mut child = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        insert_field(&mut child, "m_State", local_pointer(state.file_id))?;
        insert_field(&mut child, "m_Position", position3(0, 0, 0))?;
        child_states.push(child)?;
    }
    insert_field(&mut body, "m_ChildStates", child_states)?;
    let mut child_machines = UnityYamlNode::sequence(UnityYamlSequenceStyle::Block);
    for (child_index, &machine_index) in node.child_machine_indexes.iter().enumerate() {
        let index = state_count + child_index;
        let mut child = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        insert_field(
            &mut child,
            "m_StateMachine",
            local_pointer(machine.machines[machine_index].file_id),
        )?;
        insert_field(
            &mut child,
            "m_Position",
            position3((index % side * 250) as i64, (index / side * 100) as i64, 0),
        )?;
        child_machines.push(child)?;
    }
    insert_field(&mut body, "m_ChildStateMachines", child_machines)?;
    insert_field(
        &mut body,
        "m_AnyStateTransitions",
        pointer_sequence(&node.any_transition_file_ids)?,
    )?;
    insert_field(
        &mut body,
        "m_EntryTransitions",
        pointer_sequence(&node.entry_transition_file_ids)?,
    )?;
    let modern_dictionary =
        project_version.major > 5 || (project_version.major == 5 && project_version.minor >= 4);
    let mut exits = if modern_dictionary {
        UnityYamlNode::sequence(UnityYamlSequenceStyle::BlockCurve)
    } else {
        UnityYamlNode::mapping(UnityYamlMappingStyle::Block)
    };
    for (child_index, transition_ids) in &node.child_exit_transitions {
        let mut pair = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        insert_field(
            &mut pair,
            "first",
            local_pointer(machine.machines[*child_index].file_id),
        )?;
        insert_field(&mut pair, "second", pointer_sequence(transition_ids)?)?;
        if modern_dictionary {
            exits.push(pair)?;
        } else {
            insert_field(&mut exits, "data", pair)?;
        }
    }
    insert_field(&mut body, "m_StateMachineTransitions", exits)?;
    insert_field(
        &mut body,
        "m_StateMachineBehaviours",
        behaviour_nodes(&node.behaviours)?,
    )?;
    insert_field(&mut body, "m_AnyStatePosition", position3(0, -100, 0))?;
    insert_field(&mut body, "m_EntryPosition", position3(250, -100, 0))?;
    insert_field(&mut body, "m_ExitPosition", position3(500, -100, 0))?;
    insert_field(
        &mut body,
        "m_ParentStateMachinePosition",
        position3(0, -200, 0),
    )?;
    let default_state = match drop_targets.get(&node.default_state_file_id) {
        Some(target) => foreign_pointer(target.main_file_id, target.guid),
        None => local_pointer(node.default_state_file_id),
    };
    insert_field(&mut body, "m_DefaultState", default_state)?;
    UnityYamlDocument::object(
        ANIMATOR_STATE_MACHINE_CLASS_ID,
        node.file_id,
        "AnimatorStateMachine",
        body,
        false,
    )
    .map_err(|error| Box::new(error) as Box<dyn Error>)
}

fn state_transition_document(
    transition: &SerializedValue,
    tos: &HashMap<u32, String>,
    destination: TransitionDestination,
    file_id: i64,
) -> Result<UnityYamlDocument, Box<dyn Error>> {
    let conditions = array_field(transition, "m_ConditionConstantArray", "TransitionConstant")?;
    let has_exit_time_field = optional_field(transition, "m_ExitTime")?.is_some();
    let mut condition_nodes = UnityYamlNode::sequence(UnityYamlSequenceStyle::Block);
    for raw_condition in conditions {
        let condition = unwrap_data(raw_condition, "ConditionConstant")?;
        let mode = required_i32_field(condition, "m_ConditionMode", "ConditionConstant")?;
        if has_exit_time_field && mode == EXIT_TIME_CONDITION_MODE {
            continue;
        }
        let event_id = required_u32_field(condition, "m_EventID", "ConditionConstant")?;
        let event_name = tos.get(&event_id).cloned().ok_or_else(|| {
            invalid_project(format!(
                "AnimatorController transition condition has no TOS event {event_id}"
            ))
        })?;
        let mut node = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        insert_field(&mut node, "m_ConditionMode", signed(i64::from(mode)))?;
        insert_field(
            &mut node,
            "m_ConditionEvent",
            UnityYamlNode::string(event_name),
        )?;
        insert_field(
            &mut node,
            "m_EventTreshold",
            float32(required_f32_field(
                condition,
                "m_EventThreshold",
                "ConditionConstant",
            )?),
        )?;
        condition_nodes.push(node)?;
    }

    let atomic = optional_bool_field(transition, "m_Atomic")?.unwrap_or(false);
    let duration = required_f32_field(transition, "m_TransitionDuration", "TransitionConstant")?;
    let offset = required_f32_field(transition, "m_TransitionOffset", "TransitionConstant")?;
    let exit_time = transition_exit_time(transition)?;
    let has_exit_time = transition_has_exit_time(transition)?;
    let has_fixed_duration = optional_bool_field(transition, "m_HasFixedDuration")?.unwrap_or(true);
    let interruption = optional_i32_field(transition, "m_InterruptionSource")?
        .unwrap_or(if atomic { 0 } else { 1 });
    let ordered = optional_bool_field(transition, "m_OrderedInterruption")?.unwrap_or(true);
    let can_transition_to_self =
        optional_bool_field(transition, "m_CanTransitionToSelf")?.unwrap_or(false);
    let name_id = required_u32_field(transition, "m_UserID", "TransitionConstant")?;
    let name = tos.get(&name_id).cloned().ok_or_else(|| {
        invalid_project(format!(
            "AnimatorController transition has no TOS name {name_id}"
        ))
    })?;

    let mut body = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    insert_field(&mut body, "serializedVersion", signed(3))?;
    insert_field(&mut body, "m_ObjectHideFlags", signed(1))?;
    insert_field(&mut body, "m_CorrespondingSourceObject", local_pointer(0))?;
    insert_field(&mut body, "m_PrefabInstance", local_pointer(0))?;
    insert_field(&mut body, "m_PrefabAsset", local_pointer(0))?;
    insert_field(&mut body, "m_Name", UnityYamlNode::string(name))?;
    insert_field(&mut body, "m_Conditions", condition_nodes)?;
    insert_field(
        &mut body,
        "m_DstStateMachine",
        local_pointer(destination.machine_file_id),
    )?;
    let dst_state = match destination.state_foreign {
        Some(target) => foreign_pointer(target.main_file_id, target.guid),
        None => local_pointer(destination.state_file_id),
    };
    insert_field(&mut body, "m_DstState", dst_state)?;
    insert_field(&mut body, "m_Solo", signed(0))?;
    insert_field(&mut body, "m_Mute", signed(0))?;
    insert_field(
        &mut body,
        "m_IsExit",
        signed(i64::from(destination.is_exit)),
    )?;
    insert_field(&mut body, "m_TransitionDuration", float32(duration))?;
    insert_field(&mut body, "m_TransitionOffset", float32(offset))?;
    insert_field(&mut body, "m_ExitTime", float32(exit_time))?;
    insert_field(&mut body, "m_HasExitTime", signed(i64::from(has_exit_time)))?;
    insert_field(
        &mut body,
        "m_HasFixedDuration",
        signed(i64::from(has_fixed_duration)),
    )?;
    insert_field(
        &mut body,
        "m_InterruptionSource",
        signed(i64::from(interruption)),
    )?;
    insert_field(
        &mut body,
        "m_OrderedInterruption",
        signed(i64::from(ordered)),
    )?;
    insert_field(
        &mut body,
        "m_CanTransitionToSelf",
        signed(i64::from(can_transition_to_self)),
    )?;
    UnityYamlDocument::object(
        ANIMATOR_STATE_TRANSITION_CLASS_ID,
        file_id,
        "AnimatorStateTransition",
        body,
        false,
    )
    .map_err(|error| Box::new(error) as Box<dyn Error>)
}

fn selector_transition_document(
    transition: &SerializedValue,
    tos: &HashMap<u32, String>,
    destination: TransitionDestination,
    file_id: i64,
) -> Result<UnityYamlDocument, Box<dyn Error>> {
    let conditions = array_field(
        transition,
        "m_ConditionConstantArray",
        "SelectorTransitionConstant",
    )?;
    let mut condition_nodes = UnityYamlNode::sequence(UnityYamlSequenceStyle::Block);
    for raw_condition in conditions {
        let condition = unwrap_data(raw_condition, "Selector ConditionConstant")?;
        let mode = required_i32_field(condition, "m_ConditionMode", "ConditionConstant")?;
        if mode == EXIT_TIME_CONDITION_MODE {
            continue;
        }
        let mut node = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        insert_field(&mut node, "m_ConditionMode", signed(i64::from(mode)))?;
        insert_field(
            &mut node,
            "m_ConditionEvent",
            UnityYamlNode::string(tos_name(
                tos,
                required_u32_field(condition, "m_EventID", "ConditionConstant")?,
                "SelectorTransitionConstant condition event",
            )?),
        )?;
        insert_field(
            &mut node,
            "m_EventTreshold",
            float32(required_f32_field(
                condition,
                "m_EventThreshold",
                "ConditionConstant",
            )?),
        )?;
        condition_nodes.push(node)?;
    }
    let mut body = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    insert_field(&mut body, "m_ObjectHideFlags", signed(1))?;
    insert_field(&mut body, "m_CorrespondingSourceObject", local_pointer(0))?;
    insert_field(&mut body, "m_PrefabInstance", local_pointer(0))?;
    insert_field(&mut body, "m_PrefabAsset", local_pointer(0))?;
    insert_field(&mut body, "m_Name", UnityYamlNode::string(""))?;
    insert_field(&mut body, "m_Conditions", condition_nodes)?;
    insert_field(
        &mut body,
        "m_DstStateMachine",
        local_pointer(destination.machine_file_id),
    )?;
    let dst_state = match destination.state_foreign {
        Some(target) => foreign_pointer(target.main_file_id, target.guid),
        None => local_pointer(destination.state_file_id),
    };
    insert_field(&mut body, "m_DstState", dst_state)?;
    insert_field(&mut body, "m_Solo", signed(0))?;
    insert_field(&mut body, "m_Mute", signed(0))?;
    insert_field(
        &mut body,
        "m_IsExit",
        signed(i64::from(destination.is_exit)),
    )?;
    UnityYamlDocument::object(1109, file_id, "AnimatorTransition", body, false)
        .map_err(|error| Box::new(error) as Box<dyn Error>)
}

fn state_motion(
    state: &SerializedValue,
    clips: &[UnityYamlNode],
    tos: &HashMap<u32, String>,
    project_version: crate::processing::scene_naming::SceneUnityVersion,
    allocator: &mut FileIdAllocator,
    drop: Option<&mut DropContext<'_>>,
) -> Result<(UnityYamlNode, Vec<UnityYamlDocument>), Box<dyn Error>> {
    let mut documents = Vec::new();
    let Some(raw_trees) = optional_field(state, "m_BlendTreeConstantArray")? else {
        return Ok((local_pointer(0), documents));
    };
    let trees = as_array(raw_trees, "StateConstant BlendTreeConstantArray")?;
    let Some(raw_tree) = trees.first() else {
        return Ok((local_pointer(0), documents));
    };
    let tree = unwrap_data(raw_tree, "BlendTreeConstant")?;
    let nodes = array_field(tree, "m_NodeArray", "BlendTreeConstant")?;
    if nodes.is_empty() {
        return Err(invalid_project(
            "AnimatorController BlendTreeConstant has an empty NodeArray",
        ));
    }
    let mut active_nodes = Vec::new();
    let motion = build_motion_node(
        state,
        nodes,
        0,
        clips,
        tos,
        project_version,
        allocator,
        &mut active_nodes,
        &mut documents,
        drop,
    )?;
    Ok((motion, documents))
}

#[allow(clippy::too_many_arguments)]
fn build_motion_node(
    state: &SerializedValue,
    nodes: &[SerializedValue],
    node_index: usize,
    clips: &[UnityYamlNode],
    tos: &HashMap<u32, String>,
    project_version: crate::processing::scene_naming::SceneUnityVersion,
    allocator: &mut FileIdAllocator,
    active_nodes: &mut Vec<usize>,
    documents: &mut Vec<UnityYamlDocument>,
    mut drop: Option<&mut DropContext<'_>>,
) -> Result<UnityYamlNode, Box<dyn Error>> {
    let raw_node = nodes.get(node_index).ok_or_else(|| {
        invalid_project(format!(
            "AnimatorController BlendTree NodeArray index {node_index} is out of bounds"
        ))
    })?;
    let node = unwrap_data(raw_node, "BlendTreeNodeConstant")?;
    let node_view = blend_tree_node_view(node)?;
    if !node_view.is_blend_tree() {
        return animation_clip_pointer(state, node, clips);
    }
    if project_version.major != 2022 || project_version.minor != 3 {
        return Err(invalid_project(format!(
            "AnimatorController BlendTree reconstruction is not source-verified for Unity {}.{}.{}",
            project_version.major, project_version.minor, project_version.patch
        )));
    }
    if active_nodes.contains(&node_index) {
        return Err(invalid_project(format!(
            "AnimatorController BlendTree contains a cycle through NodeArray[{node_index}]"
        )));
    }
    active_nodes
        .try_reserve(1)
        .map_err(|_| invalid_project("could not allocate AnimatorController BlendTree path"))?;
    active_nodes.push(node_index);

    let file_id = allocator.allocate()?;
    // Dropped BlendTrees take separate-file identities up front so parent
    // documents reference them cross-file in a single pass. The allocator ID
    // is still consumed to keep every kept document's file ID stable.
    let target = drop
        .as_mut()
        .map(|context| context.mint_target(file_id, BLEND_TREE_CLASS_ID))
        .transpose()?;
    let mut children = UnityYamlNode::sequence(UnityYamlSequenceStyle::Block);
    for (child_position, child_index) in node_view.child_indices.iter().copied().enumerate() {
        let child_index = usize::try_from(child_index).map_err(|_| {
            invalid_project("AnimatorController BlendTree child index does not fit usize")
        })?;
        let child_raw = nodes.get(child_index).ok_or_else(|| {
            invalid_project(format!(
                "AnimatorController BlendTree child index {child_index} is outside NodeArray length {}",
                nodes.len()
            ))
        })?;
        let child_node = unwrap_data(child_raw, "BlendTreeNodeConstant")?;
        let child_view = blend_tree_node_view(child_node)?;
        let child_motion = if child_index == node_index {
            local_pointer(0)
        } else {
            build_motion_node(
                state,
                nodes,
                child_index,
                clips,
                tos,
                project_version,
                allocator,
                active_nodes,
                documents,
                drop.as_deref_mut(),
            )?
        };
        let time_scale = if child_view.is_blend_tree() {
            1.0
        } else {
            let duration = required_f32_field(child_node, "m_Duration", "BlendTreeNodeConstant")?;
            1.0 / duration
        };
        let cycle_offset =
            required_f32_field(child_node, "m_CycleOffset", "BlendTreeNodeConstant")?;
        let mirror = if child_view.is_blend_tree() {
            false
        } else {
            required_bool_field(child_node, "m_Mirror", "BlendTreeNodeConstant")?
        };
        let position = node_view.get_position(child_position).map_err(|error| {
            invalid_project(format!("AnimatorController BlendTree position: {error}"))
        })?;
        let threshold = node_view.get_threshold(child_position).map_err(|error| {
            invalid_project(format!("AnimatorController BlendTree threshold: {error}"))
        })?;
        let direct_parameter = node_view
            .try_get_direct_blend_parameter(child_position)
            .map_err(|error| {
                invalid_project(format!(
                    "AnimatorController BlendTree direct parameter: {error}"
                ))
            })?
            .map(|id| tos_name(tos, id, "BlendTree direct parameter"))
            .transpose()?
            .unwrap_or_default();
        children.push(child_motion_node(ChildMotionFields {
            motion: child_motion,
            threshold,
            position,
            time_scale,
            cycle_offset,
            direct_blend_parameter: direct_parameter,
            mirror,
        })?)?;
    }

    let document = blend_tree_document(
        node,
        &node_view,
        tos,
        children,
        target.map_or(file_id, |target| target.main_file_id),
    )?;
    if let (Some(context), Some(target)) = (drop.as_mut(), target) {
        context.dropped.try_reserve(1).map_err(|_| {
            invalid_project("could not allocate AnimatorController dropped BlendTree documents")
        })?;
        context.dropped.push(DroppedControllerDocument {
            class_id: BLEND_TREE_CLASS_ID,
            name: "Blend Tree".to_owned(),
            main_file_id: target.main_file_id,
            guid: target.guid,
            document,
            emit_order: target.emit_order,
        });
    } else {
        documents.try_reserve(1).map_err(|_| {
            invalid_project("could not allocate AnimatorController BlendTree documents")
        })?;
        documents.push(document);
    }
    let removed = active_nodes.pop();
    if removed != Some(node_index) {
        return Err(invalid_project(
            "AnimatorController BlendTree traversal stack became inconsistent",
        ));
    }
    Ok(match target {
        Some(target) => foreign_pointer(target.main_file_id, target.guid),
        None => local_pointer(file_id),
    })
}

fn animation_clip_pointer(
    state: &SerializedValue,
    node: &SerializedValue,
    clips: &[UnityYamlNode],
) -> Result<UnityYamlNode, Box<dyn Error>> {
    let clip_id = required_u32_field(node, "m_ClipID", "BlendTreeNodeConstant")?;
    let clip_index = leaf_info_clip_index(state, clip_id)?.unwrap_or(clip_id as i32);
    if clip_index == -1 {
        return Ok(local_pointer(0));
    }
    let clip_index = usize::try_from(clip_index).map_err(|_| {
        invalid_project(format!(
            "AnimatorController motion clip index {clip_index} is negative"
        ))
    })?;
    clips.get(clip_index).cloned().ok_or_else(|| {
        invalid_project(format!(
            "AnimatorController state references AnimationClips[{clip_index}], but only {} clips are exported",
            clips.len()
        ))
    })
}

fn blend_tree_node_view(node: &SerializedValue) -> Result<BlendTreeNodeView, Box<dyn Error>> {
    let blend_type = match required_field(node, "m_BlendType", "BlendTreeNodeConstant")? {
        SerializedValue::I32(value) => *value,
        SerializedValue::U32(value) => *value as i32,
        _ => {
            return Err(invalid_project(
                "AnimatorController BlendTree blend type has unexpected serialized type",
            ))
        }
    };
    let child_indices = array_field(node, "m_ChildIndices", "BlendTreeNodeConstant")?
        .iter()
        .map(|value| as_u32(value, "BlendTreeNodeConstant child index"))
        .collect::<Result<Vec<_>, _>>()?;
    let blend_1d = optional_data_field(node, "m_Blend1dData")?;
    let blend_2d = optional_data_field(node, "m_Blend2dData")?;
    let blend_direct = optional_data_field(node, "m_BlendDirectData")?;
    let blend1d_thresholds = optional_f32_array(blend_1d, "m_ChildThresholdArray")?;
    let child_thresholds = optional_f32_array(Some(node), "m_ChildThresholdArray")?;
    let blend2d_positions = optional_vec2_array(blend_2d, "m_ChildPositionArray")?;
    let blend_direct_ids = optional_u32_array(blend_direct, "m_ChildBlendEventIDArray")?;
    Ok(BlendTreeNodeView {
        blend_type,
        blend1d_thresholds,
        child_thresholds,
        blend2d_positions,
        blend_direct_ids,
        child_indices,
    })
}

fn optional_data_field<'a>(
    value: &'a SerializedValue,
    field: &str,
) -> Result<Option<&'a SerializedValue>, Box<dyn Error>> {
    optional_field(value, field)?
        .map(|value| unwrap_data(value, field))
        .transpose()
}

fn optional_f32_array(
    value: Option<&SerializedValue>,
    field: &str,
) -> Result<Option<Vec<f32>>, Box<dyn Error>> {
    value
        .map(|value| {
            optional_field(value, field)?
                .map(|array| {
                    as_array(array, field)?
                        .iter()
                        .map(|value| as_f32(value, field))
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()
        })
        .transpose()
        .map(Option::flatten)
}

fn optional_u32_array(
    value: Option<&SerializedValue>,
    field: &str,
) -> Result<Option<Vec<u32>>, Box<dyn Error>> {
    value
        .map(|value| {
            optional_field(value, field)?
                .map(|array| {
                    as_array(array, field)?
                        .iter()
                        .map(|value| as_u32(value, field))
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()
        })
        .transpose()
        .map(Option::flatten)
}

fn optional_vec2_array(
    value: Option<&SerializedValue>,
    field: &str,
) -> Result<Option<Vec<crate::numerics::Vec2>>, Box<dyn Error>> {
    value
        .map(|value| {
            optional_field(value, field)?
                .map(|array| {
                    as_array(array, field)?
                        .iter()
                        .map(|value| {
                            Ok(crate::numerics::Vec2 {
                                x: required_f32_field(value, "x", field)?,
                                y: required_f32_field(value, "y", field)?,
                            })
                        })
                        .collect::<Result<Vec<_>, Box<dyn Error>>>()
                })
                .transpose()
        })
        .transpose()
        .map(Option::flatten)
}

fn blend_tree_document(
    raw_node: &SerializedValue,
    node: &BlendTreeNodeView,
    tos: &HashMap<u32, String>,
    children: UnityYamlNode,
    file_id: i64,
) -> Result<UnityYamlDocument, Box<dyn Error>> {
    let mut body = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    // Unity 2022.3's project YAML omits the runtime default serializedVersion=1.
    insert_field(&mut body, "m_ObjectHideFlags", signed(1))?;
    insert_field(&mut body, "m_CorrespondingSourceObject", local_pointer(0))?;
    insert_field(&mut body, "m_PrefabInstance", local_pointer(0))?;
    insert_field(&mut body, "m_PrefabAsset", local_pointer(0))?;
    insert_field(&mut body, "m_Name", UnityYamlNode::string("Blend Tree"))?;
    insert_field(&mut body, "m_Childs", children)?;
    let blend_event = optional_u32_field(raw_node, "m_BlendEventID")?.unwrap_or(u32::MAX);
    let blend_event_y = optional_u32_field(raw_node, "m_BlendEventYID")?.unwrap_or(u32::MAX);
    insert_field(
        &mut body,
        "m_BlendParameter",
        UnityYamlNode::string(optional_tos_name(tos, blend_event, "BlendTree parameter")?),
    )?;
    insert_field(
        &mut body,
        "m_BlendParameterY",
        UnityYamlNode::string(optional_tos_name(
            tos,
            blend_event_y,
            "BlendTree Y parameter",
        )?),
    )?;
    insert_field(
        &mut body,
        "m_MinThreshold",
        float32(node.get_min_threshold().map_err(|error| {
            invalid_project(format!(
                "AnimatorController BlendTree minimum threshold: {error}"
            ))
        })?),
    )?;
    insert_field(
        &mut body,
        "m_MaxThreshold",
        float32(node.get_max_threshold().map_err(|error| {
            invalid_project(format!(
                "AnimatorController BlendTree maximum threshold: {error}"
            ))
        })?),
    )?;
    insert_field(&mut body, "m_UseAutomaticThresholds", signed(0))?;
    let normalized = optional_data_field(raw_node, "m_BlendDirectData")?
        .map(|value| optional_bool_field(value, "m_NormalizedBlendValues"))
        .transpose()?
        .flatten()
        .unwrap_or(false);
    insert_field(
        &mut body,
        "m_NormalizedBlendValues",
        signed(i64::from(normalized)),
    )?;
    let blend_type = match required_field(raw_node, "m_BlendType", "BlendTreeNodeConstant")? {
        SerializedValue::I32(value) => *value,
        SerializedValue::U32(value) => *value as i32,
        _ => {
            return Err(invalid_project(
                "AnimatorController BlendTree blend type has unexpected serialized type",
            ))
        }
    };
    insert_field(&mut body, "m_BlendType", signed(i64::from(blend_type)))?;
    UnityYamlDocument::object(206, file_id, "BlendTree", body, false)
        .map_err(|error| Box::new(error) as Box<dyn Error>)
}

fn optional_tos_name(
    tos: &HashMap<u32, String>,
    id: u32,
    context: &str,
) -> Result<String, Box<dyn Error>> {
    if id == u32::MAX {
        Ok(String::new())
    } else {
        tos_name(tos, id, context)
    }
}

fn child_motion_node(fields: ChildMotionFields) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut child = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    insert_field(&mut child, "serializedVersion", signed(2))?;
    insert_field(&mut child, "m_Motion", fields.motion)?;
    insert_field(&mut child, "m_Threshold", float32(fields.threshold))?;
    insert_field(
        &mut child,
        "m_Position",
        position2(fields.position.x, fields.position.y),
    )?;
    insert_field(&mut child, "m_TimeScale", float32(fields.time_scale))?;
    insert_field(&mut child, "m_CycleOffset", float32(fields.cycle_offset))?;
    insert_field(
        &mut child,
        "m_DirectBlendParameter",
        UnityYamlNode::string(fields.direct_blend_parameter),
    )?;
    insert_field(&mut child, "m_Mirror", signed(i64::from(fields.mirror)))?;
    // The false runtime IsAnim default is omitted from Unity 2022.3 project YAML.
    Ok(child)
}

fn leaf_info_clip_index(
    state: &SerializedValue,
    clip_id: u32,
) -> Result<Option<i32>, Box<dyn Error>> {
    let Some(values) = optional_field(state, "m_LeafInfoArray")? else {
        return Ok(None);
    };
    for raw_leaf in as_array(values, "StateConstant LeafInfoArray")? {
        let leaf = unwrap_data(raw_leaf, "LeafInfoConstant")?;
        let ids = array_field(leaf, "m_IDArray", "LeafInfoConstant")?;
        let mut matching_index = None;
        for (index, value) in ids.iter().enumerate() {
            if as_u32(value, "LeafInfoConstant ID")? == clip_id {
                matching_index = Some(index);
                break;
            }
        }
        if let Some(index) = matching_index {
            let offset = required_u32_field(leaf, "m_IndexOffset", "LeafInfoConstant")?;
            let index = offset
                .checked_add(
                    u32::try_from(index)
                        .map_err(|_| invalid_project("LeafInfo index does not fit u32"))?,
                )
                .ok_or_else(|| invalid_project("LeafInfo clip index overflows"))?;
            return Ok(Some(i32::try_from(index).map_err(|_| {
                invalid_project("LeafInfo clip index exceeds i32")
            })?));
        }
    }
    Ok(Some(-1))
}

fn transition_exit_time(transition: &SerializedValue) -> Result<f32, Box<dyn Error>> {
    if let Some(value) = optional_field(transition, "m_ExitTime")? {
        return as_f32(value, "TransitionConstant m_ExitTime");
    }
    for condition in array_field(transition, "m_ConditionConstantArray", "TransitionConstant")? {
        let condition = unwrap_data(condition, "ConditionConstant")?;
        if required_i32_field(condition, "m_ConditionMode", "ConditionConstant")?
            == EXIT_TIME_CONDITION_MODE
        {
            return required_f32_field(condition, "m_ExitTime", "ConditionConstant");
        }
    }
    Ok(1.0)
}

fn transition_has_exit_time(transition: &SerializedValue) -> Result<bool, Box<dyn Error>> {
    if let Some(value) = optional_field(transition, "m_HasExitTime")? {
        return as_bool(value, "TransitionConstant m_HasExitTime");
    }
    for condition in array_field(transition, "m_ConditionConstantArray", "TransitionConstant")? {
        let condition = unwrap_data(condition, "ConditionConstant")?;
        if required_i32_field(condition, "m_ConditionMode", "ConditionConstant")?
            == EXIT_TIME_CONDITION_MODE
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn take_controller_body(document: &UnityYamlDocument) -> Result<UnityYamlNode, Box<dyn Error>> {
    controller_body(document).cloned()
}

fn controller_body(document: &UnityYamlDocument) -> Result<&UnityYamlNode, Box<dyn Error>> {
    let UnityYamlNodeKind::Mapping { entries, .. } = &document.root.kind else {
        return Err(invalid_project(
            "AnimatorController document root is not a mapping",
        ));
    };
    entries
        .iter()
        .find_map(|(key, value)| {
            (node_string(key).as_deref() == Some("AnimatorController")).then_some(value)
        })
        .ok_or_else(|| invalid_project("YAML object has no AnimatorController body"))
}

fn replace_controller_body(
    mut document: UnityYamlDocument,
    body: UnityYamlNode,
) -> Result<UnityYamlDocument, Box<dyn Error>> {
    let file_id = document_file_id(&document)?;
    let stripped = document.root.stripped;
    document = UnityYamlDocument::object(
        ANIMATOR_CONTROLLER_CLASS_ID,
        file_id,
        "AnimatorController",
        body,
        stripped,
    )
    .map_err(|error| Box::new(error) as Box<dyn Error>)?;
    Ok(document)
}

fn insert_generated_controller_fields(
    body: &mut UnityYamlNode,
    mut generated: HashMap<&'static str, UnityYamlNode>,
) -> Result<(), Box<dyn Error>> {
    let UnityYamlNodeKind::Mapping { style, entries } = &mut body.kind else {
        return Err(invalid_project("AnimatorController body is not a mapping"));
    };
    let mut rebuilt = Vec::new();
    rebuilt
        .try_reserve(entries.len().saturating_add(2))
        .map_err(|_| invalid_project("could not allocate rebuilt AnimatorController fields"))?;
    for (key, value) in std::mem::take(entries) {
        let Some(name) = node_string(&key) else {
            rebuilt.push((key, value));
            continue;
        };
        if matches!(
            name.as_str(),
            "m_Controller"
                | "m_ControllerSize"
                | "m_TOS"
                | "m_AnimatorParameters"
                | "m_AnimatorLayers"
        ) {
            continue;
        }
        let add_generated = name == "m_Name";
        rebuilt.push((key, value));
        if add_generated {
            for field in ["m_AnimatorParameters", "m_AnimatorLayers"] {
                let node = generated.remove(field).ok_or_else(|| {
                    invalid_project(format!("AnimatorController field {field} was not rebuilt"))
                })?;
                rebuilt.push((UnityYamlNode::plain_string(field), node));
            }
        }
    }
    if !generated.is_empty() {
        return Err(invalid_project(
            "AnimatorController root has no m_Name insertion point",
        ));
    }
    *entries = rebuilt;
    let _ = style;
    Ok(())
}

fn normalize_modern_controller_root_body(
    body: &UnityYamlNode,
) -> Result<UnityYamlNode, Box<dyn Error>> {
    let field = |name: &str| -> Result<UnityYamlNode, Box<dyn Error>> {
        node_field(body, name)?
            .cloned()
            .ok_or_else(|| invalid_project(format!("AnimatorController root is missing {name}")))
    };
    let mut normalized = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    insert_field(
        &mut normalized,
        "serializedVersion",
        field("serializedVersion")?,
    )?;
    insert_field(&mut normalized, "m_ObjectHideFlags", signed(0))?;
    insert_field(
        &mut normalized,
        "m_CorrespondingSourceObject",
        local_pointer(0),
    )?;
    insert_field(&mut normalized, "m_PrefabInstance", local_pointer(0))?;
    insert_field(&mut normalized, "m_PrefabAsset", local_pointer(0))?;
    insert_field(&mut normalized, "m_Name", field("m_Name")?)?;
    insert_field(
        &mut normalized,
        "m_AnimatorParameters",
        field("m_AnimatorParameters")?,
    )?;
    insert_field(
        &mut normalized,
        "m_AnimatorLayers",
        field("m_AnimatorLayers")?,
    )?;
    Ok(normalized)
}

fn behaviour_nodes(pointers: &[UnityYamlNode]) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut sequence = UnityYamlNode::sequence(UnityYamlSequenceStyle::Block);
    for pointer in pointers {
        sequence.push(pointer.clone())?;
    }
    Ok(sequence)
}

/// Source FetchEditorHierarchy includes only slots actually bound to a generated
/// state/machine. Unused controller behaviour pointers remain independent assets.
/// Claimed behaviour pointers plus the bundle PPtr identities selected only
/// by dropped states, which stay unclaimed and route as separate files.
type BehaviourOwnership<'a> = (Vec<&'a SerializedValue>, HashSet<(i32, i64)>);

pub(super) fn release_controller_behaviour_pointers(
    payload: &ParsedObjectPayload,
) -> Result<BehaviourOwnership<'_>, Box<dyn Error>> {
    let table = behaviours::BehaviourTable::read(&payload.root)?;
    if table.pointers.is_empty() {
        return Ok((Vec::new(), HashSet::new()));
    }
    let controller = required_field(&payload.root, "m_Controller", "AnimatorController")?;
    let tos = read_tos(required_field(
        &payload.root,
        "m_TOS",
        "AnimatorController",
    )?)?;
    let layers = read_layers(controller, &tos)?;
    let constants = read_state_machine_constants(controller)?;
    let mut pointers = Vec::new();
    // Bundle PPtr identity per behaviour slot, flagged when any kept owner
    // (kept state or machine) selects it. Slots selected only by dropped
    // states export as separate files, mirroring unclaimed MainAsset objects.
    let mut slot_kept: HashMap<(i32, i64), bool> = HashMap::new();
    let mut allocator = FileIdAllocator::new();
    for (machine_index, raw) in constants.iter().enumerate() {
        let owner = layers
            .iter()
            .position(|layer| {
                layer.state_machine_index == machine_index && layer.synchronized_layer_index == 0
            })
            .ok_or_else(|| {
                invalid_project(
                    "AnimatorController behaviour ownership has no unsynchronized layer",
                )
            })?;
        let machine = rebuild_state_machine_hierarchy(raw, &layers[owner], &tos, &mut allocator)?;
        let mut owner_paths = Vec::new();
        owner_paths.try_reserve(
            machine
                .states
                .len()
                .checked_add(machine.machines.len())
                .ok_or_else(|| {
                    invalid_project("could not allocate AnimatorController behaviour owners")
                })?,
        )?;
        for (state_index, state) in machine.states.iter().enumerate() {
            let kept = !machine.dropped_state_indexes.contains(&state_index);
            owner_paths.push((state.state_path_id, kept));
        }
        for node in &machine.machines {
            // Machines always export (even unnamed and empty), so their slots
            // always keep the selected behaviours.
            owner_paths.push((node.full_path_id, true));
        }
        for (path, kept) in owner_paths {
            let slots = table.slots(owner, path)?;
            pointers.try_reserve(slots.len()).map_err(|_| {
                invalid_project(
                    "could not allocate AnimatorController behaviour ownership pointers",
                )
            })?;
            slot_kept.try_reserve(slots.len()).map_err(|_| {
                invalid_project("could not allocate AnimatorController behaviour ownership flags")
            })?;
            for slot in slots {
                let pointer = &table.pointers[slot];
                pointers.push(pointer);
                if let SerializedValue::PPtr { file_id, path_id } = pointer {
                    if *path_id != 0 {
                        let flag = slot_kept.entry((*file_id, *path_id)).or_insert(false);
                        *flag |= kept;
                    }
                }
            }
        }
    }
    let mut dropped = HashSet::new();
    dropped.try_reserve(slot_kept.len()).map_err(|_| {
        invalid_project("could not allocate AnimatorController dropped behaviour identities")
    })?;
    for (key, kept) in slot_kept {
        if !kept {
            dropped.insert(key);
        }
    }
    Ok((pointers, dropped))
}

fn document_file_id(document: &UnityYamlDocument) -> Result<i64, Box<dyn Error>> {
    document
        .root
        .anchor
        .as_deref()
        .ok_or_else(|| invalid_project("AnimatorController root YAML document has no anchor"))?
        .parse::<i64>()
        .map_err(|error| {
            invalid_project(format!(
                "AnimatorController fileID anchor is invalid: {error}"
            ))
        })
}

fn tos_name(tos: &HashMap<u32, String>, id: u32, context: &str) -> Result<String, Box<dyn Error>> {
    tos.get(&id)
        .cloned()
        .ok_or_else(|| invalid_project(format!("AnimatorController {context} has no TOS key {id}")))
}

fn required_field<'a>(
    value: &'a SerializedValue,
    name: &str,
    context: &str,
) -> Result<&'a SerializedValue, Box<dyn Error>> {
    optional_field(value, name)?
        .ok_or_else(|| invalid_project(format!("{context} is missing field {name}")))
}

fn optional_field<'a>(
    value: &'a SerializedValue,
    name: &str,
) -> Result<Option<&'a SerializedValue>, Box<dyn Error>> {
    value.unique_field(name).map_err(|error| {
        invalid_project(format!(
            "AnimatorController field {name} lookup failed: {error}"
        ))
    })
}

fn required_u32_field(
    value: &SerializedValue,
    name: &str,
    context: &str,
) -> Result<u32, Box<dyn Error>> {
    as_u32(required_field(value, name, context)?, name)
}

fn optional_u32_field(value: &SerializedValue, name: &str) -> Result<Option<u32>, Box<dyn Error>> {
    optional_field(value, name)?
        .map(|value| as_u32(value, name))
        .transpose()
}

fn required_i32_field(
    value: &SerializedValue,
    name: &str,
    context: &str,
) -> Result<i32, Box<dyn Error>> {
    as_i32(required_field(value, name, context)?, name)
}

fn required_i32_field_alias(
    value: &SerializedValue,
    names: &[&str],
    context: &str,
) -> Result<i32, Box<dyn Error>> {
    let mut found = None;
    for &name in names {
        if let Some(field) = optional_field(value, name)? {
            if found.is_some() {
                return Err(invalid_project(format!(
                    "AnimatorController {context} has more than one serialized spelling for a field"
                )));
            }
            found = Some(as_i32(field, name)?);
        }
    }
    found.ok_or_else(|| {
        invalid_project(format!(
            "AnimatorController {context} is missing field {}",
            names.join(" or ")
        ))
    })
}

fn optional_i32_field(value: &SerializedValue, name: &str) -> Result<Option<i32>, Box<dyn Error>> {
    optional_field(value, name)?
        .map(|value| as_i32(value, name))
        .transpose()
}

fn required_f32_field(
    value: &SerializedValue,
    name: &str,
    context: &str,
) -> Result<f32, Box<dyn Error>> {
    as_f32(required_field(value, name, context)?, name)
}

fn required_bool_field(
    value: &SerializedValue,
    name: &str,
    context: &str,
) -> Result<bool, Box<dyn Error>> {
    as_bool(required_field(value, name, context)?, name)
}

fn optional_bool_field(
    value: &SerializedValue,
    name: &str,
) -> Result<Option<bool>, Box<dyn Error>> {
    optional_field(value, name)?
        .map(|value| as_bool(value, name))
        .transpose()
}

fn array_field<'a>(
    value: &'a SerializedValue,
    name: &str,
    context: &str,
) -> Result<&'a [SerializedValue], Box<dyn Error>> {
    as_array(required_field(value, name, context)?, name)
}

fn as_array<'a>(
    value: &'a SerializedValue,
    context: &str,
) -> Result<&'a [SerializedValue], Box<dyn Error>> {
    value
        .array_elements()
        .ok_or_else(|| invalid_project(format!("AnimatorController {context} is not an array")))
}

fn unwrap_data<'a>(
    value: &'a SerializedValue,
    context: &str,
) -> Result<&'a SerializedValue, Box<dyn Error>> {
    unwrap_data_or_self(value, context)
}

fn unwrap_data_or_self<'a>(
    value: &'a SerializedValue,
    context: &str,
) -> Result<&'a SerializedValue, Box<dyn Error>> {
    if matches!(value, SerializedValue::Struct(_)) {
        if let Some(data) = optional_field(value, "data")? {
            return Ok(data);
        }
    }
    if matches!(value, SerializedValue::Struct(_)) {
        Ok(value)
    } else {
        Err(invalid_project(format!(
            "AnimatorController {context} is not a struct"
        )))
    }
}

fn as_u32(value: &SerializedValue, context: &str) -> Result<u32, Box<dyn Error>> {
    match value {
        SerializedValue::U32(value) => Ok(*value),
        SerializedValue::U8(value) => Ok(u32::from(*value)),
        SerializedValue::U16(value) => Ok(u32::from(*value)),
        SerializedValue::I32(value) if *value >= 0 => Ok(*value as u32),
        _ => Err(invalid_project(format!(
            "AnimatorController {context} has unexpected serialized type"
        ))),
    }
}

fn as_i32(value: &SerializedValue, context: &str) -> Result<i32, Box<dyn Error>> {
    match value {
        SerializedValue::I32(value) => Ok(*value),
        SerializedValue::I16(value) => Ok(i32::from(*value)),
        SerializedValue::U32(value) => i32::try_from(*value)
            .map_err(|_| invalid_project(format!("AnimatorController {context} exceeds i32"))),
        SerializedValue::U8(value) => Ok(i32::from(*value)),
        _ => Err(invalid_project(format!(
            "AnimatorController {context} has unexpected serialized type"
        ))),
    }
}

fn as_f32(value: &SerializedValue, context: &str) -> Result<f32, Box<dyn Error>> {
    match value {
        SerializedValue::F32(value) => Ok(*value),
        SerializedValue::I32(value) => Ok(*value as f32),
        SerializedValue::U32(value) => Ok(*value as f32),
        _ => Err(invalid_project(format!(
            "AnimatorController {context} has unexpected serialized type"
        ))),
    }
}

fn as_bool(value: &SerializedValue, context: &str) -> Result<bool, Box<dyn Error>> {
    match value {
        SerializedValue::Bool(value) => Ok(*value),
        SerializedValue::U8(value) => Ok(*value != 0),
        SerializedValue::I32(value) => Ok(*value != 0),
        _ => Err(invalid_project(format!(
            "AnimatorController {context} has unexpected serialized type"
        ))),
    }
}

fn array_f32_at(
    value: &SerializedValue,
    index: usize,
    little_endian: bool,
    context: &str,
) -> Result<f32, Box<dyn Error>> {
    match value {
        SerializedValue::Array(values) => values
            .get(index)
            .ok_or_else(|| {
                invalid_project(format!(
                    "AnimatorController {context} index {index} is out of bounds"
                ))
            })
            .and_then(|value| as_f32(value, context)),
        SerializedValue::Bytes(bytes) => {
            let start = index
                .checked_mul(4)
                .ok_or_else(|| invalid_project("AnimatorController float byte offset overflows"))?;
            let slice = bytes.get(start..start + 4).ok_or_else(|| {
                invalid_project(format!(
                    "AnimatorController {context} index {index} is out of bounds"
                ))
            })?;
            let bits = if little_endian {
                u32::from_le_bytes(slice.try_into().unwrap())
            } else {
                u32::from_be_bytes(slice.try_into().unwrap())
            };
            Ok(f32::from_bits(bits))
        }
        _ => Err(invalid_project(format!(
            "AnimatorController {context} is not a numeric array"
        ))),
    }
}

fn array_i32_at(
    value: &SerializedValue,
    index: usize,
    little_endian: bool,
    context: &str,
) -> Result<i32, Box<dyn Error>> {
    match value {
        SerializedValue::Array(values) => values
            .get(index)
            .ok_or_else(|| {
                invalid_project(format!(
                    "AnimatorController {context} index {index} is out of bounds"
                ))
            })
            .and_then(|value| as_i32(value, context)),
        SerializedValue::Bytes(bytes) => {
            let start = index.checked_mul(4).ok_or_else(|| {
                invalid_project("AnimatorController integer byte offset overflows")
            })?;
            let slice = bytes.get(start..start + 4).ok_or_else(|| {
                invalid_project(format!(
                    "AnimatorController {context} index {index} is out of bounds"
                ))
            })?;
            Ok(if little_endian {
                i32::from_le_bytes(slice.try_into().unwrap())
            } else {
                i32::from_be_bytes(slice.try_into().unwrap())
            })
        }
        _ => Err(invalid_project(format!(
            "AnimatorController {context} is not an integer array"
        ))),
    }
}

fn array_bool_at(
    value: &SerializedValue,
    index: usize,
    context: &str,
) -> Result<bool, Box<dyn Error>> {
    match value {
        SerializedValue::Array(values) => values
            .get(index)
            .ok_or_else(|| {
                invalid_project(format!(
                    "AnimatorController {context} index {index} is out of bounds"
                ))
            })
            .and_then(|value| as_bool(value, context)),
        SerializedValue::Bytes(bytes) => {
            bytes.get(index).map(|value| *value != 0).ok_or_else(|| {
                invalid_project(format!(
                    "AnimatorController {context} index {index} is out of bounds"
                ))
            })
        }
        _ => Err(invalid_project(format!(
            "AnimatorController {context} is not a boolean array"
        ))),
    }
}

fn node_field<'a>(
    value: &'a UnityYamlNode,
    name: &str,
) -> Result<Option<&'a UnityYamlNode>, Box<dyn Error>> {
    let UnityYamlNodeKind::Mapping { entries, .. } = &value.kind else {
        return Err(invalid_project(
            "AnimatorController YAML body field is not a mapping",
        ));
    };
    Ok(entries
        .iter()
        .find_map(|(key, value)| (node_string(key).as_deref() == Some(name)).then_some(value)))
}

fn node_string(value: &UnityYamlNode) -> Option<String> {
    match &value.kind {
        UnityYamlNodeKind::Scalar(
            UnityYamlScalar::String(value) | UnityYamlScalar::PlainString(value),
        ) => Some(value.clone()),
        _ => None,
    }
}

fn insert_field(
    body: &mut UnityYamlNode,
    name: &str,
    value: UnityYamlNode,
) -> Result<(), Box<dyn Error>> {
    body.insert_field(name, value)
        .map_err(|error| Box::new(error) as Box<dyn Error>)
}

fn insert_int(
    body: &mut UnityYamlNode,
    name: &str,
    value: UnityYamlNode,
) -> Result<(), Box<dyn Error>> {
    insert_field(body, name, value)
}

fn signed(value: i64) -> UnityYamlNode {
    UnityYamlNode::scalar(UnityYamlScalar::Signed(value))
}

fn float32(value: f32) -> UnityYamlNode {
    UnityYamlNode::scalar(UnityYamlScalar::Float32(value))
}

fn local_pointer(file_id: i64) -> UnityYamlNode {
    let mut pointer = UnityYamlNode::mapping(UnityYamlMappingStyle::Flow);
    pointer
        .insert_field("fileID", signed(file_id))
        .expect("local pointer mapping is valid");
    pointer
}

fn pointer_sequence(file_ids: &[i64]) -> Result<UnityYamlNode, Box<dyn Error>> {
    let mut output = UnityYamlNode::sequence(if file_ids.is_empty() {
        UnityYamlSequenceStyle::Flow
    } else {
        UnityYamlSequenceStyle::Block
    });
    for &file_id in file_ids {
        output.push(local_pointer(file_id))?;
    }
    Ok(output)
}

fn empty_sequence() -> UnityYamlNode {
    UnityYamlNode::sequence(UnityYamlSequenceStyle::Flow)
}

fn position3(x: i64, y: i64, z: i64) -> UnityYamlNode {
    let mut position = UnityYamlNode::mapping(UnityYamlMappingStyle::Flow);
    position
        .insert_field("x", signed(x))
        .expect("position mapping is valid");
    position
        .insert_field("y", signed(y))
        .expect("position mapping is valid");
    position
        .insert_field("z", signed(z))
        .expect("position mapping is valid");
    position
}

fn position2(x: f32, y: f32) -> UnityYamlNode {
    let mut position = UnityYamlNode::mapping(UnityYamlMappingStyle::Flow);
    position
        .insert_field("x", float32(x))
        .expect("BlendTree position mapping is valid");
    position
        .insert_field("y", float32(y))
        .expect("BlendTree position mapping is valid");
    position
}

fn integer_ceil_sqrt(value: usize) -> usize {
    if value <= 1 {
        return 1;
    }
    let mut side = 1usize;
    while side.saturating_mul(side) < value {
        side += 1;
    }
    side
}

#[cfg(test)]
#[path = "animator_controller/tests.rs"]
mod state_machine_source_tests;

#[cfg(test)]
mod blend_tree_source_tests {
    use super::*;
    use crate::resources::blend_tree_node::BLEND_TREE_SIMPLE_1D;
    use crate::serialized_file::object::SerializedFieldValue;

    fn field(name: &str, value: SerializedValue) -> SerializedFieldValue {
        SerializedFieldValue {
            name: name.to_owned(),
            value,
        }
    }

    fn wrapped(value: SerializedValue) -> SerializedValue {
        SerializedValue::Struct(vec![field("data", value)])
    }

    fn source_node(
        children: &[u32],
        clip_id: u32,
        duration: f32,
        cycle_offset: f32,
        mirror: bool,
    ) -> SerializedValue {
        SerializedValue::Struct(vec![
            field("m_BlendType", SerializedValue::U32(0)),
            field(
                "m_ChildIndices",
                SerializedValue::Array(
                    children.iter().copied().map(SerializedValue::U32).collect(),
                ),
            ),
            field("m_ClipID", SerializedValue::U32(clip_id)),
            field("m_Duration", SerializedValue::F32(duration)),
            field("m_CycleOffset", SerializedValue::F32(cycle_offset)),
            field("m_Mirror", SerializedValue::Bool(mirror)),
        ])
    }

    fn source_simple_1d_tree_node(
        children: &[u32],
        blend_parameter_id: u32,
        thresholds: &[f32],
    ) -> SerializedValue {
        SerializedValue::Struct(vec![
            field(
                "m_BlendType",
                SerializedValue::U32(BLEND_TREE_SIMPLE_1D as u32),
            ),
            field("m_BlendEventID", SerializedValue::U32(blend_parameter_id)),
            field("m_BlendEventYID", SerializedValue::U32(u32::MAX)),
            field("m_CycleOffset", SerializedValue::F32(0.0)),
            field(
                "m_ChildIndices",
                SerializedValue::Array(
                    children.iter().copied().map(SerializedValue::U32).collect(),
                ),
            ),
            field(
                "m_Blend1dData",
                wrapped(SerializedValue::Struct(vec![field(
                    "m_ChildThresholdArray",
                    SerializedValue::Array(
                        thresholds
                            .iter()
                            .copied()
                            .map(SerializedValue::F32)
                            .collect(),
                    ),
                )])),
            ),
        ])
    }

    fn yaml_field<'a>(node: &'a UnityYamlNode, name: &str) -> &'a UnityYamlNode {
        node_field(node, name)
            .expect("fixture YAML node is a mapping")
            .expect("fixture YAML field exists")
    }

    fn yaml_optional_field<'a>(node: &'a UnityYamlNode, name: &str) -> Option<&'a UnityYamlNode> {
        node_field(node, name).expect("fixture YAML node is a mapping")
    }

    fn scalar_default(node: &UnityYamlNode, name: &str, default: &str) -> String {
        yaml_optional_field(node, name)
            .map(scalar)
            .unwrap_or_else(|| default.to_owned())
    }

    fn scalar(node: &UnityYamlNode) -> String {
        match &node.kind {
            UnityYamlNodeKind::Scalar(UnityYamlScalar::Signed(value)) => value.to_string(),
            UnityYamlNodeKind::Scalar(UnityYamlScalar::Unsigned(value)) => value.to_string(),
            UnityYamlNodeKind::Scalar(UnityYamlScalar::String(value)) => value.clone(),
            UnityYamlNodeKind::Scalar(UnityYamlScalar::Boolean(value)) => value.to_string(),
            _ => panic!("expected scalar YAML value, got {node:?}"),
        }
    }

    fn float_bits(node: &UnityYamlNode) -> String {
        match node.kind {
            UnityYamlNodeKind::Scalar(UnityYamlScalar::Float32(value)) => {
                format!("{:08X}", value.to_bits())
            }
            _ => panic!("expected Float32 YAML value, got {node:?}"),
        }
    }

    fn source_bool(node: &UnityYamlNode) -> &'static str {
        if scalar(node) == "1" {
            "True"
        } else {
            "False"
        }
    }

    #[test]
    fn simple_1d_blend_tree_matches_frozen_source_runtime_projection() {
        let speed_id = crc32_ieee(b"Speed");
        let root = SerializedValue::Struct(vec![
            field(
                "m_BlendType",
                SerializedValue::U32(BLEND_TREE_SIMPLE_1D as u32),
            ),
            field("m_BlendEventID", SerializedValue::U32(speed_id)),
            field("m_BlendEventYID", SerializedValue::U32(u32::MAX)),
            field(
                "m_ChildIndices",
                SerializedValue::Array(vec![SerializedValue::U32(1), SerializedValue::U32(2)]),
            ),
            field(
                "m_Blend1dData",
                wrapped(SerializedValue::Struct(vec![field(
                    "m_ChildThresholdArray",
                    SerializedValue::Array(vec![
                        SerializedValue::F32(-0.5),
                        SerializedValue::F32(0.75),
                    ]),
                )])),
            ),
        ]);
        let leaves = [
            source_node(&[], 0, 2.0, 0.125, false),
            source_node(&[], 1, 4.0, 0.25, true),
        ];
        let tree = SerializedValue::Struct(vec![field(
            "m_NodeArray",
            SerializedValue::Array(
                std::iter::once(wrapped(root))
                    .chain(leaves.into_iter().map(wrapped))
                    .collect(),
            ),
        )]);
        let state = SerializedValue::Struct(vec![field(
            "m_BlendTreeConstantArray",
            SerializedValue::Array(vec![wrapped(tree)]),
        )]);
        let clips = [local_pointer(101), local_pointer(102)];
        let tos = HashMap::from([(speed_id, "Speed".to_owned())]);
        let version = crate::processing::scene_naming::SceneUnityVersion {
            major: 2022,
            minor: 3,
            patch: 0,
        };
        let mut allocator = FileIdAllocator::new();
        let (motion, documents) =
            state_motion(&state, &clips, &tos, version, &mut allocator, None).unwrap();

        assert_eq!(
            scalar(yaml_field(&motion, "fileID")),
            GENERATED_FILE_ID_BASE.to_string()
        );
        assert_eq!(documents.len(), 1);
        assert_eq!(documents[0].root.tag.as_deref(), Some("!u!206"));
        assert_eq!(documents[0].root.anchor.as_deref(), Some("7000000000"));
        let body = yaml_field(&documents[0].root, "BlendTree");
        assert!(
            yaml_optional_field(body, "serializedVersion").is_none(),
            "the Unity 2022.3 project exporter omits the runtime default BlendTree version"
        );
        let mut actual = vec![
            format!(
                "TREE|class|{}",
                documents[0]
                    .root
                    .tag
                    .as_deref()
                    .unwrap()
                    .trim_start_matches("!u!")
            ),
            format!(
                "TREE|serializedVersion|{}",
                scalar_default(body, "serializedVersion", "1")
            ),
            format!(
                "TREE|hideFlags|{}",
                scalar(yaml_field(body, "m_ObjectHideFlags"))
            ),
            format!("TREE|name|{}", scalar(yaml_field(body, "m_Name"))),
            format!(
                "TREE|blendParameter|{}",
                scalar(yaml_field(body, "m_BlendParameter"))
            ),
            format!(
                "TREE|blendParameterY|{}",
                scalar(yaml_field(body, "m_BlendParameterY"))
            ),
            format!("TREE|blendType|{}", scalar(yaml_field(body, "m_BlendType"))),
            format!(
                "TREE|minThreshold|{}",
                float_bits(yaml_field(body, "m_MinThreshold"))
            ),
            format!(
                "TREE|maxThreshold|{}",
                float_bits(yaml_field(body, "m_MaxThreshold"))
            ),
            format!(
                "TREE|useAutomaticThresholds|{}",
                source_bool(yaml_field(body, "m_UseAutomaticThresholds"))
            ),
            format!(
                "TREE|normalizedBlendValues|{}",
                source_bool(yaml_field(body, "m_NormalizedBlendValues"))
            ),
        ];
        let UnityYamlNodeKind::Sequence { items, .. } = &yaml_field(body, "m_Childs").kind else {
            panic!("BlendTree m_Childs is not a sequence");
        };
        assert_eq!(items.len(), clips.len());
        for (index, (child, expected_clip)) in items.iter().zip(&clips).enumerate() {
            assert_eq!(yaml_field(child, "m_Motion"), expected_clip);
            assert!(
                yaml_optional_field(child, "m_IsAnim").is_none(),
                "the Unity 2022.3 project exporter omits the false runtime IsAnim default"
            );
            let position = yaml_field(child, "m_Position");
            actual.push(format!(
                "CHILD|{index}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
                scalar(yaml_field(child, "serializedVersion")),
                float_bits(yaml_field(child, "m_Threshold")),
                float_bits(yaml_field(position, "x")),
                float_bits(yaml_field(position, "y")),
                float_bits(yaml_field(child, "m_TimeScale")),
                float_bits(yaml_field(child, "m_CycleOffset")),
                scalar(yaml_field(child, "m_DirectBlendParameter")),
                scalar(yaml_field(child, "m_Mirror")),
                scalar_default(child, "m_IsAnim", "0"),
            ));
        }
        let expected = include_str!("../../resources/animator_blendtree_source_2022_3_fixture.txt")
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }

    #[test]
    fn nested_simple_1d_blend_tree_matches_frozen_source_runtime_projection() {
        let speed_id = crc32_ieee(b"Speed");
        let nested_speed_id = crc32_ieee(b"NestedSpeed");
        let root = source_simple_1d_tree_node(&[1, 2], speed_id, &[-0.5, 0.75]);
        let nested = source_simple_1d_tree_node(&[3, 4], nested_speed_id, &[0.25, 1.25]);
        let nodes = vec![
            wrapped(root),
            wrapped(nested),
            wrapped(source_node(&[], 0, 2.0, 0.125, false)),
            wrapped(source_node(&[], 1, 4.0, 0.25, true)),
            wrapped(source_node(&[], 2, 4.0, 0.375, false)),
        ];
        let tree =
            SerializedValue::Struct(vec![field("m_NodeArray", SerializedValue::Array(nodes))]);
        let state = SerializedValue::Struct(vec![field(
            "m_BlendTreeConstantArray",
            SerializedValue::Array(vec![wrapped(tree)]),
        )]);
        let clips = [local_pointer(101), local_pointer(102), local_pointer(103)];
        let tos = HashMap::from([
            (speed_id, "Speed".to_owned()),
            (nested_speed_id, "NestedSpeed".to_owned()),
        ]);
        let version = crate::processing::scene_naming::SceneUnityVersion {
            major: 2022,
            minor: 3,
            patch: 0,
        };
        let mut allocator = FileIdAllocator::new();
        let (motion, documents) =
            state_motion(&state, &clips, &tos, version, &mut allocator, None).unwrap();

        assert_eq!(documents.len(), 2);
        assert_eq!(scalar(yaml_field(&motion, "fileID")), "7000000000");
        let root_document = documents
            .iter()
            .find(|document| document.root.anchor.as_deref() == Some("7000000000"))
            .expect("root BlendTree document exists");
        let nested_document = documents
            .iter()
            .find(|document| document.root.anchor.as_deref() == Some("7000000001"))
            .expect("nested BlendTree document exists");

        let mut actual = tree_source_projection(root_document, "ROOT");
        actual.extend(tree_source_projection(nested_document, "NESTED"));
        let expected =
            include_str!("../../resources/animator_nested_blendtree_source_2022_3_fixture.txt")
                .lines()
                .map(str::to_owned)
                .collect::<Vec<_>>();
        assert_eq!(actual, expected);

        let root_body = yaml_field(&root_document.root, "BlendTree");
        let UnityYamlNodeKind::Sequence {
            items: root_children,
            ..
        } = &yaml_field(root_body, "m_Childs").kind
        else {
            panic!("root BlendTree m_Childs is not a sequence");
        };
        assert_eq!(
            scalar(yaml_field(
                yaml_field(&root_children[0], "m_Motion"),
                "fileID"
            )),
            "7000000001",
            "the first root child must point to the nested BlendTree subasset"
        );
        assert_eq!(
            scalar(yaml_field(
                yaml_field(&root_children[1], "m_Motion"),
                "fileID"
            )),
            "101",
            "the second root child must preserve the first clip identity"
        );

        let nested_body = yaml_field(&nested_document.root, "BlendTree");
        let UnityYamlNodeKind::Sequence {
            items: nested_children,
            ..
        } = &yaml_field(nested_body, "m_Childs").kind
        else {
            panic!("nested BlendTree m_Childs is not a sequence");
        };
        for (child, expected_clip) in nested_children.iter().zip(&clips[1..]) {
            assert_eq!(yaml_field(child, "m_Motion"), expected_clip);
        }
    }

    fn tree_source_projection(document: &UnityYamlDocument, record: &str) -> Vec<String> {
        let body = yaml_field(&document.root, "BlendTree");
        assert!(
            yaml_optional_field(body, "serializedVersion").is_none(),
            "the Unity 2022.3 project exporter omits the runtime default BlendTree version"
        );
        let mut actual = vec![
            format!(
                "{record}|class|{}",
                document
                    .root
                    .tag
                    .as_deref()
                    .expect("source fixture has a class tag")
                    .trim_start_matches("!u!")
            ),
            format!(
                "{record}|serializedVersion|{}",
                scalar_default(body, "serializedVersion", "1")
            ),
            format!(
                "{record}|hideFlags|{}",
                scalar(yaml_field(body, "m_ObjectHideFlags"))
            ),
            format!("{record}|name|{}", scalar(yaml_field(body, "m_Name"))),
            format!(
                "{record}|blendParameter|{}",
                scalar(yaml_field(body, "m_BlendParameter"))
            ),
            format!(
                "{record}|blendParameterY|{}",
                scalar(yaml_field(body, "m_BlendParameterY"))
            ),
            format!(
                "{record}|blendType|{}",
                scalar(yaml_field(body, "m_BlendType"))
            ),
            format!(
                "{record}|minThreshold|{}",
                float_bits(yaml_field(body, "m_MinThreshold"))
            ),
            format!(
                "{record}|maxThreshold|{}",
                float_bits(yaml_field(body, "m_MaxThreshold"))
            ),
            format!(
                "{record}|useAutomaticThresholds|{}",
                source_bool(yaml_field(body, "m_UseAutomaticThresholds"))
            ),
            format!(
                "{record}|normalizedBlendValues|{}",
                source_bool(yaml_field(body, "m_NormalizedBlendValues"))
            ),
        ];
        let UnityYamlNodeKind::Sequence { items, .. } = &yaml_field(body, "m_Childs").kind else {
            panic!("BlendTree m_Childs is not a sequence");
        };
        for (index, child) in items.iter().enumerate() {
            assert!(
                yaml_optional_field(child, "m_IsAnim").is_none(),
                "the Unity 2022.3 project exporter omits the false runtime IsAnim default"
            );
            let position = yaml_field(child, "m_Position");
            actual.push(format!(
                "CHILD|{record}|{index}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
                scalar(yaml_field(child, "serializedVersion")),
                float_bits(yaml_field(child, "m_Threshold")),
                float_bits(yaml_field(position, "x")),
                float_bits(yaml_field(position, "y")),
                float_bits(yaml_field(child, "m_TimeScale")),
                float_bits(yaml_field(child, "m_CycleOffset")),
                scalar(yaml_field(child, "m_DirectBlendParameter")),
                scalar(yaml_field(child, "m_Mirror")),
                scalar_default(child, "m_IsAnim", "0"),
            ));
        }
        actual
    }
}

#[cfg(test)]
mod real_vrcw_tests {
    use super::*;

    #[test]
    fn unity_2022_controller_root_omits_input_only_fields() {
        let mut body = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        body.insert_field("serializedVersion", signed(5)).unwrap();
        body.insert_field("m_Name", UnityYamlNode::string("moon_doll"))
            .unwrap();
        body.insert_field("m_AnimatorParameters", empty_sequence())
            .unwrap();
        body.insert_field("m_AnimatorLayers", empty_sequence())
            .unwrap();
        body.insert_field("m_AnimationClips", empty_sequence())
            .unwrap();
        body.insert_field("m_MultiThreadedStateMachine", signed(1))
            .unwrap();

        let normalized = normalize_modern_controller_root_body(&body).unwrap();
        let UnityYamlNodeKind::Mapping { entries, .. } = normalized.kind else {
            panic!("normalized AnimatorController body is not a mapping");
        };
        let names = entries
            .iter()
            .map(|(key, _)| node_string(key).expect("controller field key is a string"))
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            [
                "serializedVersion",
                "m_ObjectHideFlags",
                "m_CorrespondingSourceObject",
                "m_PrefabInstance",
                "m_PrefabAsset",
                "m_Name",
                "m_AnimatorParameters",
                "m_AnimatorLayers",
            ]
        );
    }
}
