use super::*;
use crate::export::settings::ExportDefaults;
use crate::export::settings_file::ExportSettingsFile;
use crate::export::workspace_yaml::tests::load_workspace_named_bytes_with_source_factory;
use crate::serialized_file::object_workspace::{ObjectWorkspace, ObjectWorkspaceLimits};
use crate::serialized_file::reference_graph::{
    MultiFileReferenceGraphLimits, ReferenceGraphLimits,
};

const FIXTURE: &str =
    include_str!("../../../resources/animator_nested_state_machine_source_2022_3_fixture.txt");
const EMPTY_FIXTURE: &str =
    include_str!("../../../resources/animator_empty_state_machine_source_2022_3_fixture.txt");
const BEHAVIOUR_FIXTURE: &str =
    include_str!("../../../resources/animator_behaviour_state_machine_source_2022_3_fixture.txt");

fn fixture_input() -> Vec<u8> {
    fixture_input_from(FIXTURE)
}

fn fixture_input_from(fixture: &str) -> Vec<u8> {
    let hex = fixture
        .lines()
        .find_map(|line| line.strip_prefix("SOURCE_SERIALIZED_FILE_HEX|"))
        .expect("source runtime emitted a complete SerializedFile");
    assert_eq!(hex.len() % 2, 0);
    hex.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn field<'a>(node: &'a UnityYamlNode, name: &str) -> &'a UnityYamlNode {
    node_field(node, name)
        .unwrap()
        .unwrap_or_else(|| panic!("missing {name}"))
}

fn items(node: &UnityYamlNode) -> &[UnityYamlNode] {
    let UnityYamlNodeKind::Sequence { items, .. } = &node.kind else {
        panic!("expected a sequence: {node:?}")
    };
    items
}

fn text(node: &UnityYamlNode) -> String {
    match &node.kind {
        UnityYamlNodeKind::Scalar(UnityYamlScalar::String(value))
        | UnityYamlNodeKind::Scalar(UnityYamlScalar::PlainString(value)) => value.clone(),
        UnityYamlNodeKind::Scalar(UnityYamlScalar::Signed(value)) => value.to_string(),
        UnityYamlNodeKind::Scalar(UnityYamlScalar::Unsigned(value)) => value.to_string(),
        _ => panic!("expected scalar: {node:?}"),
    }
}

fn bits(node: &UnityYamlNode) -> String {
    let value = match node.kind {
        UnityYamlNodeKind::Scalar(UnityYamlScalar::Float32(value)) => value,
        UnityYamlNodeKind::Scalar(UnityYamlScalar::Signed(value)) => value as f32,
        _ => panic!("expected float/position scalar: {node:?}"),
    };
    format!("{:08X}", value.to_bits())
}

fn source_bool(node: &UnityYamlNode) -> &'static str {
    match text(node).as_str() {
        "0" => "False",
        "1" => "True",
        other => panic!("invalid boolean {other}"),
    }
}

fn pointer_id(node: &UnityYamlNode) -> i64 {
    text(field(node, "fileID")).parse().unwrap()
}

fn body(document: &UnityYamlDocument) -> &UnityYamlNode {
    let UnityYamlNodeKind::Mapping { entries, .. } = &document.root.kind else {
        panic!("document root is not a mapping")
    };
    assert_eq!(entries.len(), 1);
    &entries[0].1
}

fn fixture_payload() -> ParsedObjectPayload {
    fixture_payload_from(FIXTURE)
}

fn fixture_payload_from(fixture: &str) -> ParsedObjectPayload {
    let (filesystem, mut workspace) = load_workspace_named_bytes_with_source_factory(
        "NestedController.assets",
        &fixture_input_from(fixture),
    );
    assert!(
        workspace.files()[0]
            .metadata
            .types
            .iter()
            .all(|serialized_type| {
                serialized_type
                    .type_tree
                    .as_ref()
                    .is_none_or(|tree| tree.nodes.is_empty())
            }),
        "frozen input must exercise source TPK recovery"
    );
    let mut objects =
        ObjectWorkspace::load_from_vfs(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .unwrap();
    objects
        .recover_header_only_null_script_behaviours(
            &mut workspace,
            ObjectWorkspaceLimits::default(),
            ReferenceGraphLimits::default(),
        )
        .unwrap();
    assert!(objects.report().is_complete(), "{:?}", objects.report());
    let controller = objects
        .objects()
        .iter()
        .find(|object| object.factory.class_id == 91)
        .unwrap();
    assert!(controller.recovered_type_tree.is_some());
    controller.payload.as_ref().unwrap().clone()
}

fn rebuilt_documents(payload: &ParsedObjectPayload) -> Vec<UnityYamlDocument> {
    rebuilt_documents_with_root(payload, 9100000)
}

fn rebuilt_documents_with_root(
    payload: &ParsedObjectPayload,
    root_file_id: i64,
) -> Vec<UnityYamlDocument> {
    rebuilt_documents_with_behaviours(payload, root_file_id, &[])
}

fn rebuilt_documents_with_behaviours(
    payload: &ParsedObjectPayload,
    root_file_id: i64,
    pointers: &[UnityYamlNode],
) -> Vec<UnityYamlDocument> {
    let mut root = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
    root.insert_field("serializedVersion", signed(5)).unwrap();
    root.insert_field("m_ObjectHideFlags", signed(0)).unwrap();
    root.insert_field("m_Name", UnityYamlNode::string("NestedController"))
        .unwrap();
    if !pointers.is_empty() {
        root.insert_field(
            "m_StateMachineBehaviours",
            behaviour_nodes(pointers).unwrap(),
        )
        .unwrap();
    }
    let document =
        UnityYamlDocument::object(91, root_file_id, "AnimatorController", root, false).unwrap();
    rebuild_animator_controller_documents(
        document,
        payload,
        true,
        crate::processing::scene_naming::SceneUnityVersion::parse("2022.3.0f1").unwrap(),
    )
    .unwrap()
}

fn assign_machine_identities(
    document_id: i64,
    parent_path: &str,
    documents: &HashMap<i64, &UnityYamlDocument>,
    identities: &mut HashMap<i64, String>,
) {
    let machine = body(documents[&document_id]);
    let name = text(field(machine, "m_Name"));
    let path = if parent_path.is_empty() {
        name
    } else {
        format!("{parent_path}.{name}")
    };
    assert!(
        identities.insert(document_id, path.clone()).is_none(),
        "duplicate/cyclic machine identity"
    );
    for child in items(field(machine, "m_ChildStates")) {
        let id = pointer_id(field(child, "m_State"));
        let state = body(documents[&id]);
        let state_path = format!("{path}.{}", text(field(state, "m_Name")));
        assert!(
            identities.insert(id, state_path).is_none(),
            "duplicate state owner"
        );
    }
    for child in items(field(machine, "m_ChildStateMachines")) {
        assign_machine_identities(
            pointer_id(field(child, "m_StateMachine")),
            &path,
            documents,
            identities,
        );
    }
}

fn resolved_identity(pointer: &UnityYamlNode, identities: &HashMap<i64, String>) -> String {
    let id = pointer_id(pointer);
    if id == 0 {
        String::new()
    } else {
        identities
            .get(&id)
            .expect("non-null local pointer resolves")
            .clone()
    }
}

fn transition_projection(
    owner: &str,
    pointer: &UnityYamlNode,
    documents: &HashMap<i64, &UnityYamlDocument>,
    identities: &HashMap<i64, String>,
    rows: &mut Vec<String>,
) {
    let document = documents[&pointer_id(pointer)];
    let transition = body(document);
    rows.push(format!(
        "TRANSITION|{owner}|class|{}",
        document
            .root
            .tag
            .as_deref()
            .unwrap()
            .trim_start_matches("!u!")
    ));
    rows.push(format!(
        "TRANSITION|{owner}|serializedVersion|{}",
        node_field(transition, "serializedVersion")
            .unwrap()
            .map_or_else(|| "1".to_owned(), text)
    ));
    rows.push(format!(
        "TRANSITION|{owner}|name|{}",
        node_field(transition, "m_Name")
            .unwrap()
            .map_or_else(String::new, text)
    ));
    rows.push(format!(
        "TRANSITION|{owner}|destinationState|{}",
        resolved_identity(field(transition, "m_DstState"), identities)
    ));
    rows.push(format!(
        "TRANSITION|{owner}|destinationStateMachine|{}",
        resolved_identity(field(transition, "m_DstStateMachine"), identities)
    ));
    rows.push(format!(
        "TRANSITION|{owner}|isExit|{}",
        source_bool(field(transition, "m_IsExit"))
    ));
    for (index, condition) in items(field(transition, "m_Conditions")).iter().enumerate() {
        rows.push(format!(
            "CONDITION|{owner}|{index}|{}|{}|{}",
            text(field(condition, "m_ConditionMode")),
            text(field(condition, "m_ConditionEvent")),
            bits(field(condition, "m_EventTreshold"))
        ));
    }
    if document.root.tag.as_deref() == Some("!u!1101") {
        for name in ["TransitionDuration", "TransitionOffset", "ExitTime"] {
            rows.push(format!(
                "TRANSITION|{owner}|{name}|{}",
                bits(field(transition, &format!("m_{name}")))
            ));
        }
        for name in [
            "HasExitTime",
            "HasFixedDuration",
            "OrderedInterruption",
            "CanTransitionToSelf",
        ] {
            rows.push(format!(
                "TRANSITION|{owner}|{name}|{}",
                source_bool(field(transition, &format!("m_{name}")))
            ));
        }
        rows.push(format!(
            "TRANSITION|{owner}|InterruptionSource|{}",
            text(field(transition, "m_InterruptionSource"))
        ));
    }
}

fn machine_projection(
    id: i64,
    parent_path: &str,
    documents: &HashMap<i64, &UnityYamlDocument>,
    identities: &HashMap<i64, String>,
    rows: &mut Vec<String>,
) {
    let machine = body(documents[&id]);
    let path = &identities[&id];
    rows.push(format!(
        "MACHINE|{path}|name|{}",
        text(field(machine, "m_Name"))
    ));
    rows.push(format!("MACHINE|{path}|parent|{parent_path}"));
    rows.push(format!(
        "MACHINE|{path}|serializedVersion|{}",
        text(field(machine, "serializedVersion"))
    ));
    rows.push(format!(
        "MACHINE|{path}|default|{}",
        resolved_identity(field(machine, "m_DefaultState"), identities)
    ));
    for (source, yaml) in [
        ("stateCount", "m_ChildStates"),
        ("machineCount", "m_ChildStateMachines"),
        ("entryCount", "m_EntryTransitions"),
        ("anyCount", "m_AnyStateTransitions"),
        ("exitGroupCount", "m_StateMachineTransitions"),
    ] {
        rows.push(format!(
            "MACHINE|{path}|{source}|{}",
            items(field(machine, yaml)).len()
        ));
    }
    for child in items(field(machine, "m_ChildStates")) {
        let state_id = pointer_id(field(child, "m_State"));
        let state_path = &identities[&state_id];
        let position = field(child, "m_Position");
        rows.push(format!(
            "CHILD_STATE|{path}|{state_path}|{}|{}|{}",
            bits(field(position, "x")),
            bits(field(position, "y")),
            bits(field(position, "z"))
        ));
        let state = body(documents[&state_id]);
        assert_eq!(
            field(state, "m_Position"),
            position,
            "state and child position agree"
        );
        rows.push(format!(
            "STATE|{state_path}|speed|{}",
            bits(field(state, "m_Speed"))
        ));
        rows.push(format!(
            "STATE|{state_path}|cycleOffset|{}",
            bits(field(state, "m_CycleOffset"))
        ));
        for (index, transition) in items(field(state, "m_Transitions")).iter().enumerate() {
            transition_projection(
                &format!("STATE:{state_path}:{index}"),
                transition,
                documents,
                identities,
                rows,
            );
        }
    }
    for (kind, yaml) in [
        ("ENTRY", "m_EntryTransitions"),
        ("ANY", "m_AnyStateTransitions"),
    ] {
        for (index, transition) in items(field(machine, yaml)).iter().enumerate() {
            transition_projection(
                &format!("{kind}:{path}:{index}"),
                transition,
                documents,
                identities,
                rows,
            );
        }
    }
    for pair in items(field(machine, "m_StateMachineTransitions")) {
        let child = resolved_identity(field(pair, "first"), identities);
        let transitions = items(field(pair, "second"));
        rows.push(format!("EXIT_GROUP|{path}|{child}|{}", transitions.len()));
        for (index, transition) in transitions.iter().enumerate() {
            transition_projection(
                &format!("EXIT:{path}:{child}:{index}"),
                transition,
                documents,
                identities,
                rows,
            );
        }
    }
    for child in items(field(machine, "m_ChildStateMachines")) {
        let child_id = pointer_id(field(child, "m_StateMachine"));
        let position = field(child, "m_Position");
        rows.push(format!(
            "CHILD_MACHINE|{path}|{}|{}|{}|{}",
            identities[&child_id],
            bits(field(position, "x")),
            bits(field(position, "y")),
            bits(field(position, "z"))
        ));
        machine_projection(child_id, path, documents, identities, rows);
    }
}

fn all_document_identities(documents: &[UnityYamlDocument]) -> HashMap<i64, String> {
    let by_id: HashMap<_, _> = documents
        .iter()
        .map(|document| (document_file_id(document).unwrap(), document))
        .collect();
    let root = &items(field(body(&documents[0]), "m_AnimatorLayers"))[0];
    let mut identities = HashMap::new();
    assign_machine_identities(
        pointer_id(field(root, "m_StateMachine")),
        "",
        &by_id,
        &mut identities,
    );
    let nodes = identities.clone();
    for (id, path) in &nodes {
        let document = by_id[id];
        let node = body(document);
        match document.root.tag.as_deref().unwrap() {
            "!u!1102" => {
                for (index, pointer) in items(field(node, "m_Transitions")).iter().enumerate() {
                    assert!(identities
                        .insert(pointer_id(pointer), format!("STATE:{path}:{index}"))
                        .is_none());
                }
            }
            "!u!1107" => {
                for (kind, yaml) in [
                    ("ENTRY", "m_EntryTransitions"),
                    ("ANY", "m_AnyStateTransitions"),
                ] {
                    for (index, pointer) in items(field(node, yaml)).iter().enumerate() {
                        assert!(identities
                            .insert(pointer_id(pointer), format!("{kind}:{path}:{index}"))
                            .is_none());
                    }
                }
                for pair in items(field(node, "m_StateMachineTransitions")) {
                    let child = resolved_identity(field(pair, "first"), &nodes);
                    for (index, pointer) in items(field(pair, "second")).iter().enumerate() {
                        assert!(identities
                            .insert(pointer_id(pointer), format!("EXIT:{path}:{child}:{index}"))
                            .is_none());
                    }
                }
            }
            _ => panic!("unexpected hierarchy node"),
        }
    }
    identities.insert(
        document_file_id(&documents[0]).unwrap(),
        "controller".to_owned(),
    );
    for document in documents
        .iter()
        .filter(|document| document.root.tag.as_deref() == Some("!u!114"))
    {
        identities.insert(
            document_file_id(document).unwrap(),
            format!("BEHAVIOUR:{}", text(field(body(document), "m_Name"))),
        );
    }
    assert_eq!(
        identities.len(),
        documents.len(),
        "all generated documents are reachable exactly once"
    );
    identities
}

fn yaml_sections(yaml: &str) -> HashMap<i64, String> {
    let mut result = HashMap::new();
    let mut current_id = None;
    let mut section = String::new();
    for line in yaml.lines() {
        if line.starts_with("--- !u!") {
            if let Some(id) = current_id.take() {
                assert!(result.insert(id, std::mem::take(&mut section)).is_none());
            }
            let (_, anchor) = line.split_once(" &").unwrap();
            current_id = Some(anchor.parse().unwrap());
        }
        if current_id.is_some() {
            section.push_str(line);
            section.push('\n');
        }
    }
    if let Some(id) = current_id {
        assert!(result.insert(id, section).is_none());
    }
    result
}

fn normalize_source_pointers(yaml: &str) -> String {
    let mut rest = yaml;
    let mut output = String::new();
    const PREFIX: &str = "{m_FileID: 0, m_PathID: ";
    while let Some(index) = rest.find(PREFIX) {
        output.push_str(&rest[..index]);
        let pointer = &rest[index + PREFIX.len()..];
        let end = pointer.find('}').unwrap();
        let (id, _) = pointer[..end].split_once(", m_TargetClassID: ").unwrap();
        output.push_str(&format!("{{fileID: {id}}}"));
        rest = &pointer[end + 1..];
    }
    output.push_str(rest);
    output
}

fn normalize_output_ids(yaml: &str, ids: &HashMap<i64, i64>) -> String {
    let mut output = String::new();
    for line in yaml.lines() {
        let line = if line.starts_with("--- !u!") {
            let (prefix, anchor) = line.split_once(" &").unwrap();
            format!("{prefix} &{}", ids[&anchor.parse::<i64>().unwrap()])
        } else {
            line.to_owned()
        };
        let mut rest = line.as_str();
        const PREFIX: &str = "{fileID: ";
        while let Some(index) = rest.find(PREFIX) {
            output.push_str(&rest[..index]);
            let pointer = &rest[index + PREFIX.len()..];
            let end = pointer.find('}').unwrap();
            let id: i64 = pointer[..end].parse().unwrap();
            output.push_str(&format!(
                "{{fileID: {}}}",
                if id == 0 { 0 } else { ids[&id] }
            ));
            rest = &pointer[end + 1..];
        }
        output.push_str(rest);
        output.push('\n');
    }
    output
}

fn assert_source_yaml_matches(yaml: &str, documents: &[UnityYamlDocument]) {
    assert_source_yaml_matches_fixture(yaml, documents, FIXTURE);
}

fn assert_source_yaml_matches_fixture(yaml: &str, documents: &[UnityYamlDocument], fixture: &str) {
    let source_objects: HashMap<String, (i64, String)> = fixture
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("SOURCE_OBJECT|")?;
            let cells: Vec<_> = rest.split('|').collect();
            Some((
                cells[2].to_owned(),
                (cells[0].parse().unwrap(), cells[1].to_owned()),
            ))
        })
        .collect();
    let identities = all_document_identities(documents);
    let ids: HashMap<_, _> = identities
        .iter()
        .map(|(&id, name)| (id, source_objects[name].0))
        .collect();
    for document in documents {
        let source = &source_objects[&identities[&document_file_id(document).unwrap()]];
        assert_eq!(
            document.root.tag.as_deref().unwrap(),
            format!("!u!{}", source.1)
        );
    }
    assert_yaml_sections_match_fixture(yaml, &ids, fixture);
}

fn assert_yaml_sections_match_fixture(yaml: &str, ids: &HashMap<i64, i64>, fixture: &str) {
    let normalized_fixture = fixture.replace("\r\n", "\n");
    let source_yaml = normalized_fixture
        .split_once("SOURCE_YAML_BEGIN\n")
        .unwrap()
        .1
        .split_once("SOURCE_YAML_END")
        .unwrap()
        .0;
    let expected = yaml_sections(&normalize_source_pointers(source_yaml));
    let actual = yaml_sections(&normalize_output_ids(yaml, ids));
    assert_eq!(actual.len(), expected.len());
    for (id, expected) in expected {
        assert_eq!(actual[&id], expected, "source YAML object {id}");
    }
}

#[test]
fn nested_state_machines_match_frozen_source_runtime() {
    let documents = rebuilt_documents(&fixture_payload());
    let by_id: HashMap<_, _> = documents
        .iter()
        .map(|document| (document_file_id(document).unwrap(), document))
        .collect();
    assert_eq!(
        by_id.len(),
        documents.len(),
        "every generated fileID is unique"
    );
    let layer = &items(field(body(&documents[0]), "m_AnimatorLayers"))[0];
    let root_id = pointer_id(field(layer, "m_StateMachine"));
    let mut identities = HashMap::new();
    assign_machine_identities(root_id, "", &by_id, &mut identities);
    assert_eq!(identities.len(), 8, "four machines and four states");
    let mut actual = Vec::new();
    machine_projection(root_id, "", &by_id, &identities, &mut actual);
    let mut expected = FIXTURE
        .lines()
        .filter(|line| {
            [
                "MACHINE|",
                "STATE|",
                "TRANSITION|",
                "CONDITION|",
                "CHILD_STATE|",
                "CHILD_MACHINE|",
                "EXIT_GROUP|",
            ]
            .iter()
            .any(|prefix| line.starts_with(prefix))
        })
        .map(str::to_owned)
        .collect::<Vec<_>>();
    // Array indices remain in row identities. Sorting only removes graph traversal order.
    actual.sort();
    expected.sort();
    assert_eq!(actual, expected);
}

#[test]
fn nested_controller_exports_through_the_project_pipeline() {
    let input = fixture_input();
    let (filesystem, workspace) =
        load_workspace_named_bytes_with_source_factory("NestedController.assets", &input);
    let objects =
        ObjectWorkspace::load_from_vfs(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .unwrap();
    assert!(objects.report().is_complete(), "{:?}", objects.report());
    let graph = workspace
        .reference_graph(MultiFileReferenceGraphLimits::default())
        .unwrap();
    assert!(graph.is_complete());
    let output = crate::test_support::unique_test_directory("nested-animator-project");
    let report = super::super::export_workspace_yaml_project_with_settings(
        &output,
        false,
        &filesystem,
        &RootResourceFileCatalog::default(),
        &PlatformResourceProvider::new(None, None),
        &workspace,
        &objects,
        &graph,
        &ExportSettingsFile::from(ExportDefaults::default()),
        WorkspaceProjectExportLimits::default(),
    )
    .unwrap();
    let path = output.join("Assets/AnimatorController/Nested State Machine Source.controller");
    let yaml = std::fs::read_to_string(&path).unwrap();
    assert_eq!(yaml.matches("--- !u!1107 ").count(), 4);
    assert_eq!(yaml.matches("--- !u!1102 ").count(), 4);
    assert_eq!(yaml.matches("--- !u!1101 ").count(), 7);
    assert_eq!(yaml.matches("--- !u!1109 ").count(), 8);
    assert!(path.with_extension("controller.meta").is_file());
    assert_eq!(report.exported_object_count, 24);
    assert_source_yaml_matches(&yaml, &rebuilt_documents(&fixture_payload()));
    assert_eq!(
        filesystem
            .read_all_bytes("/game/NestedController.assets", input.len())
            .unwrap(),
        input
    );
    std::fs::remove_dir_all(output).unwrap();
}

#[test]
fn empty_machine_parent_inference_matches_source_project_export() {
    let input = fixture_input_from(EMPTY_FIXTURE);
    let payload = fixture_payload_from(EMPTY_FIXTURE);
    let documents = rebuilt_documents(&payload);
    let by_id: HashMap<_, _> = documents
        .iter()
        .map(|document| (document_file_id(document).unwrap(), document))
        .collect();
    assert_eq!(by_id.len(), documents.len());
    let layer = &items(field(body(&documents[0]), "m_AnimatorLayers"))[0];
    let root_id = pointer_id(field(layer, "m_StateMachine"));
    let mut identities = HashMap::new();
    assign_machine_identities(root_id, "", &by_id, &mut identities);
    assert_eq!(
        identities.len(),
        17,
        "twelve machines and five states, including source-generated Extra/New State"
    );
    let mut actual = Vec::new();
    machine_projection(root_id, "", &by_id, &identities, &mut actual);
    let mut expected: Vec<_> = EMPTY_FIXTURE
        .lines()
        .filter(|line| {
            [
                "MACHINE|",
                "STATE|",
                "TRANSITION|",
                "CONDITION|",
                "CHILD_STATE|",
                "CHILD_MACHINE|",
                "EXIT_GROUP|",
            ]
            .iter()
            .any(|prefix| line.starts_with(prefix))
        })
        .map(str::to_owned)
        .collect();
    actual.sort();
    expected.sort();
    assert_eq!(actual, expected);

    let controller = required_field(&payload.root, "m_Controller", "fixture").unwrap();
    let tos = read_tos(required_field(&payload.root, "m_TOS", "fixture").unwrap()).unwrap();
    let layers = read_layers(controller, &tos).unwrap();
    let constants = read_state_machine_constants(controller).unwrap();
    let machine = rebuild_state_machine_hierarchy(
        &constants[0],
        &layers[0],
        &tos,
        &mut FileIdAllocator::new(),
    )
    .unwrap();
    assert!(
        transition_destination(&machine, 4).is_err(),
        "the generated default state must not become a valid packed state index"
    );
    for node in &machine.machines {
        if node.synthetic_default_state.is_none() {
            let id = node.file_id;
            // Rebuild allocation starts at the same root/state ordering.
            assert_eq!(crc32_ieee(identities[&id].as_bytes()), node.full_path_id);
        }
    }

    let (filesystem, workspace) =
        load_workspace_named_bytes_with_source_factory("EmptyController.assets", &input);
    let objects =
        ObjectWorkspace::load_from_vfs(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .unwrap();
    assert!(objects.report().is_complete(), "{:?}", objects.report());
    let graph = workspace
        .reference_graph(MultiFileReferenceGraphLimits::default())
        .unwrap();
    assert!(graph.is_complete());
    let output = crate::test_support::unique_test_directory("empty-animator-project");
    let report = super::super::export_workspace_yaml_project_with_settings(
        &output,
        false,
        &filesystem,
        &RootResourceFileCatalog::default(),
        &PlatformResourceProvider::new(None, None),
        &workspace,
        &objects,
        &graph,
        &ExportSettingsFile::from(ExportDefaults::default()),
        WorkspaceProjectExportLimits::default(),
    )
    .unwrap();
    let path = output.join("Assets/AnimatorController/Nested State Machine Source.controller");
    let yaml = std::fs::read_to_string(&path).unwrap();
    assert!(path.with_extension("controller.meta").is_file());
    assert_eq!(report.exported_object_count, documents.len());
    assert_source_yaml_matches_fixture(&yaml, &documents, EMPTY_FIXTURE);
    assert_eq!(
        filesystem
            .read_all_bytes("/game/EmptyController.assets", input.len())
            .unwrap(),
        input
    );
    std::fs::remove_dir_all(output).unwrap();
}

#[test]
fn cyclic_empty_machine_exit_parents_fail_closed() {
    let payload = fixture_payload_from(EMPTY_FIXTURE);
    let controller = required_field(&payload.root, "m_Controller", "fixture").unwrap();
    let tos = read_tos(required_field(&payload.root, "m_TOS", "fixture").unwrap()).unwrap();
    let layers = read_layers(controller, &tos).unwrap();
    let mut constants = read_state_machine_constants(controller).unwrap();
    let selector_array =
        required_field(&constants[0], "m_SelectorStateConstantArray", "fixture").unwrap();
    let mut selectors = as_array(selector_array, "fixture").unwrap().to_vec();
    let mut parent = unwrap_data(&selectors[9], "fixture").unwrap().clone();
    let transition = unwrap_data(
        &array_field(
            unwrap_data(&selectors[11], "fixture").unwrap(),
            "m_TransitionConstantArray",
            "fixture",
        )
        .unwrap()[0],
        "fixture",
    )
    .unwrap()
    .clone();
    let mut transition = transition;
    *transition
        .unique_field_in_base_chain_mut("m_Destination")
        .unwrap()
        .unwrap() = SerializedValue::U32(30011);
    use crate::serialized_file::object::SerializedFieldValue;
    *parent
        .unique_field_in_base_chain_mut("m_TransitionConstantArray")
        .unwrap()
        .unwrap() =
        SerializedValue::Array(vec![SerializedValue::Struct(vec![SerializedFieldValue {
            name: "data".into(),
            value: transition,
        }])]);
    *selectors[9]
        .unique_field_in_base_chain_mut("data")
        .unwrap()
        .unwrap() = parent;
    *constants[0]
        .unique_field_in_base_chain_mut("m_SelectorStateConstantArray")
        .unwrap()
        .unwrap() = SerializedValue::Array(selectors);
    let error = rebuild_state_machine_hierarchy(
        &constants[0],
        &layers[0],
        &tos,
        &mut FileIdAllocator::new(),
    )
    .err()
    .expect("cyclic definitive parents must be rejected");
    assert!(error.to_string().contains("cycle"), "{error}");
}

#[test]
fn invalid_nested_destinations_are_errors_instead_of_null_references() {
    let payload = fixture_payload();
    let controller = required_field(&payload.root, "m_Controller", "fixture").unwrap();
    let tos = read_tos(required_field(&payload.root, "m_TOS", "fixture").unwrap()).unwrap();
    let layers = read_layers(controller, &tos).unwrap();
    let constants = read_state_machine_constants(controller).unwrap();
    let machine = rebuild_state_machine_hierarchy(
        &constants[0],
        &layers[0],
        &tos,
        &mut FileIdAllocator::new(),
    )
    .unwrap();
    assert!(transition_destination(&machine, 4)
        .unwrap_err()
        .to_string()
        .contains("absent state index"));
    assert!(transition_destination(&machine, 30008)
        .unwrap_err()
        .to_string()
        .contains("absent selector index"));
    assert!(transition_destination(&machine, u32::MAX)
        .unwrap()
        .is_none());
}

#[test]
fn generated_hierarchy_ids_skip_the_existing_controller_root() {
    let documents = rebuilt_documents_with_root(&fixture_payload(), GENERATED_FILE_ID_BASE);
    assert_eq!(
        document_file_id(&documents[0]).unwrap(),
        GENERATED_FILE_ID_BASE
    );
    let identities = all_document_identities(&documents);
    assert_eq!(identities.len(), 24);
    assert_eq!(identities[&GENERATED_FILE_ID_BASE], "controller");
}

#[test]
fn synchronized_layer_before_its_owner_uses_the_unsynchronized_machine_binding() {
    let mut payload = fixture_payload();
    let SerializedValue::Struct(root_fields) = &mut payload.root else {
        unreachable!()
    };
    let controller = &mut root_fields
        .iter_mut()
        .find(|field| field.name == "m_Controller")
        .unwrap()
        .value;
    let SerializedValue::Struct(controller_fields) = controller else {
        unreachable!()
    };
    let layers = &mut controller_fields
        .iter_mut()
        .find(|field| field.name == "m_LayerArray")
        .unwrap()
        .value;
    let SerializedValue::Array(layers) = layers else {
        unreachable!()
    };
    let mut synchronized = layers[0].clone();
    let SerializedValue::Struct(wrapper) = &mut synchronized else {
        unreachable!()
    };
    let SerializedValue::Struct(layer_fields) = &mut wrapper
        .iter_mut()
        .find(|field| field.name == "data")
        .unwrap()
        .value
    else {
        unreachable!()
    };
    layer_fields
        .iter_mut()
        .find(|field| field.name == "m_StateMachineSynchronizedLayerIndex")
        .unwrap()
        .value = SerializedValue::U32(1);
    // Speed is a valid TOS key, but does not identify the owning state-machine path.
    layer_fields
        .iter_mut()
        .find(|field| field.name == "m_Binding")
        .unwrap()
        .value = SerializedValue::U32(crc32_ieee(b"Speed"));
    layers.insert(0, synchronized);
    let documents = rebuilt_documents(&payload);
    let output_layers = items(field(body(&documents[0]), "m_AnimatorLayers"));
    assert_eq!(output_layers.len(), 2);
    assert_eq!(text(field(&output_layers[0], "m_Name")), "Speed");
    assert_eq!(text(field(&output_layers[0], "m_SyncedLayerIndex")), "0");
    assert_eq!(text(field(&output_layers[1], "m_Name")), "Base");
    assert_eq!(
        field(&output_layers[0], "m_StateMachine"),
        field(&output_layers[1], "m_StateMachine")
    );
}

#[test]
fn editor_controller_preserves_its_existing_hierarchy_during_project_export() {
    let fixture = include_str!(
        "../../../resources/animator_nested_state_machine_editor_source_2022_3_input.hex"
    );
    let hex = fixture
        .lines()
        .find(|line| !line.starts_with('#') && !line.is_empty())
        .unwrap();
    let input = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect::<Vec<_>>();
    let (filesystem, workspace) =
        load_workspace_named_bytes_with_source_factory("EditorController.assets", &input);
    let source_file = &workspace.files()[0];
    assert!(
        !crate::serialized_file::object::serialized_file_object_read_flags(
            source_file.header.format_version,
            source_file.metadata.prefix.target_platform,
            &source_file.virtual_path,
            &source_file.file_name,
            source_file.metadata.prefix.body_endian,
        )
        .is_release()
    );
    let objects =
        ObjectWorkspace::load_from_vfs(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .unwrap();
    assert!(objects.report().is_complete(), "{:?}", objects.report());
    assert_eq!(objects.objects().len(), 24);
    let graph = workspace
        .reference_graph(MultiFileReferenceGraphLimits::default())
        .unwrap();
    assert!(graph.is_complete());
    let controller = objects
        .objects()
        .iter()
        .find(|object| object.factory.class_id == 91)
        .unwrap();
    let payload = controller.payload.as_ref().unwrap();
    assert!(
        optional_field(&payload.root, "m_Controller")
            .unwrap()
            .is_none(),
        "source editor payload contains the existing graph instead of compiled constants"
    );
    let output = crate::test_support::unique_test_directory("editor-animator-project");
    let report = super::super::export_workspace_yaml_project_with_settings(
        &output,
        false,
        &filesystem,
        &RootResourceFileCatalog::default(),
        &PlatformResourceProvider::new(None, None),
        &workspace,
        &objects,
        &graph,
        &ExportSettingsFile::from(ExportDefaults::default()),
        WorkspaceProjectExportLimits::default(),
    )
    .unwrap();
    assert_eq!(
        report.exported_object_count, 24,
        "editor subassets are preserved without generating duplicates"
    );
    let yaml = std::fs::read_to_string(
        output.join("Assets/AnimatorController/Nested State Machine Source.controller"),
    )
    .unwrap();
    assert_eq!(yaml.matches("--- !u!91 ").count(), 1);
    assert_eq!(yaml.matches("--- !u!1107 ").count(), 4);
    assert!(yaml.contains("m_AnimatorLayers:\n"));
    assert!(yaml.contains("m_Name: Base\n"));
    assert!(yaml.contains("m_DefaultFloat: 0.25\n"));
    assert!(yaml.contains("m_DefaultInt: 2\n"));
    assert!(yaml.contains("m_StateMachine: {fileID: 110700001}"));
    assert!(!output
        .join("Assets/AnimatorStateMachine/Base.statemachine.meta")
        .exists());
    let plans = super::super::animator_collections::prepare_controller_collections(
        &workspace,
        &objects,
        &graph,
        &HashSet::new(),
        crate::export::pointer_map::ExportPointerMapLimits::default(),
    )
    .unwrap();
    let group = &plans.roots[&(controller.file_index, controller.object_index)];
    let mut ids = HashMap::from([(9100000, controller.path_id)]);
    for member in &group.children {
        ids.insert(
            member.file_id,
            objects.get(member.key.0, member.key.1).unwrap().path_id,
        );
    }
    assert_yaml_sections_match_fixture(&yaml, &ids, FIXTURE);
    std::fs::remove_dir_all(output).unwrap();
}

#[test]
fn controller_behaviour_ranges_and_collection_match_source_yaml() {
    let input = fixture_input_from(BEHAVIOUR_FIXTURE);
    let (filesystem, mut workspace) =
        load_workspace_named_bytes_with_source_factory("BehaviourController.assets", &input);
    let mut objects =
        ObjectWorkspace::load_from_vfs(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .unwrap();
    assert_eq!(
        objects
            .recover_header_only_null_script_behaviours(
                &mut workspace,
                ObjectWorkspaceLimits::default(),
                ReferenceGraphLimits::default()
            )
            .unwrap(),
        2
    );
    assert!(objects.report().is_complete(), "{:?}", objects.report());
    assert_eq!(objects.objects().len(), 3);
    let graph = workspace
        .reference_graph(MultiFileReferenceGraphLimits::default())
        .unwrap();
    assert!(graph.is_complete());
    let root = objects
        .objects()
        .iter()
        .find(|o| o.factory.class_id == 91)
        .unwrap();
    let payload = root.payload.as_ref().unwrap();
    let prepared = super::super::animator_collections::prepare_controller_collections(
        &workspace,
        &objects,
        &graph,
        &HashSet::new(),
        crate::export::pointer_map::ExportPointerMapLimits::default(),
    )
    .unwrap();
    let group = &prepared.roots[&(root.file_index, root.object_index)];
    assert_eq!(
        group.children.len(),
        2,
        "shared bindings do not duplicate a behaviour within one collection"
    );
    let pointers = behaviours::BehaviourTable::read(&payload.root)
        .unwrap()
        .pointers
        .iter()
        .map(|pointer| {
            let SerializedValue::PPtr { path_id, .. } = pointer else {
                panic!("source pointer")
            };
            local_pointer(if *path_id == 0 {
                0
            } else {
                group
                    .children
                    .iter()
                    .find(|member| {
                        objects.get(member.key.0, member.key.1).unwrap().path_id == *path_id
                    })
                    .unwrap()
                    .file_id
            })
        })
        .collect::<Vec<_>>();
    let mut documents = rebuilt_documents_with_behaviours(payload, 9100000, &pointers);
    for member in &group.children {
        let object = objects.get(member.key.0, member.key.1).unwrap();
        let mut body = UnityYamlNode::mapping(UnityYamlMappingStyle::Block);
        body.insert_field(
            "m_Name",
            UnityYamlNode::string(
                super::super::object_asset_name(object.payload.as_ref().unwrap()).unwrap(),
            ),
        )
        .unwrap();
        // These nodes supply source identities to the comparison; exported fields are
        // read from the real project YAML and compared in full to frozen source output.
        documents.push(
            UnityYamlDocument::object(114, member.file_id, "MonoBehaviour", body, false).unwrap(),
        );
    }
    let output = crate::test_support::unique_test_directory("controller-behaviour-project");
    let report = super::super::export_workspace_yaml_project_with_settings(
        &output,
        false,
        &filesystem,
        &RootResourceFileCatalog::default(),
        &PlatformResourceProvider::new(None, None),
        &workspace,
        &objects,
        &graph,
        &ExportSettingsFile::from(ExportDefaults::default()),
        WorkspaceProjectExportLimits::default(),
    )
    .unwrap();
    assert_eq!(report.exported_object_count, 26);
    let path = output.join("Assets/AnimatorController/Nested State Machine Source.controller");
    let yaml = std::fs::read_to_string(&path).unwrap();
    assert_eq!(yaml.matches("--- !u!114 ").count(), 2);
    assert!(!output.join("Assets/MonoBehaviour").exists());
    assert_source_yaml_matches_fixture(&yaml, &documents, BEHAVIOUR_FIXTURE);
    assert_eq!(
        filesystem
            .read_all_bytes("/game/BehaviourController.assets", input.len())
            .unwrap(),
        input
    );
    std::fs::remove_dir_all(output).unwrap();
}

#[test]
fn controller_behaviour_range_overflow_fails_instead_of_erasing_bindings() {
    let mut payload = fixture_payload_from(BEHAVIOUR_FIXTURE);
    let description = payload
        .root
        .unique_field_in_base_chain_mut("m_StateMachineBehaviourVectorDescription")
        .unwrap()
        .unwrap();
    let SerializedValue::Array(ranges) = description
        .unique_field_in_base_chain_mut("m_StateMachineBehaviourRanges")
        .unwrap()
        .unwrap()
    else {
        panic!("source ranges")
    };
    let range = ranges[0]
        .unique_field_in_base_chain_mut("second")
        .unwrap()
        .unwrap();
    *range
        .unique_field_in_base_chain_mut("m_StartIndex")
        .unwrap()
        .unwrap() = SerializedValue::U32(u32::MAX);
    let error = release_controller_behaviour_pointers(&payload)
        .err()
        .expect("invalid selected range must fail closed");
    assert!(error.to_string().contains("behaviour"), "{error}");
}

#[test]
fn header_only_null_script_behaviours_are_complete_without_hiding_unread_data() {
    let input = fixture_input_from(BEHAVIOUR_FIXTURE);
    let (filesystem, mut workspace) =
        load_workspace_named_bytes_with_source_factory("HeaderOnlyBehaviours.assets", &input);
    let mut objects =
        ObjectWorkspace::load_from_vfs(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .unwrap();
    assert!(
        !objects.report().is_complete(),
        "materialization must preserve the source lazy structures"
    );
    for object in objects
        .objects()
        .iter()
        .filter(|o| o.factory.class_id == 114)
    {
        assert!(!object.is_fully_materialized());
        let payload = object.payload.as_ref().unwrap();
        assert!(object.factory.has_complete_null_script_header(payload));
        let mut factory = object.factory.clone();
        factory
            .structure
            .as_mut()
            .unwrap()
            .unloaded_remainder
            .push(1);
        assert!(
            !factory.has_complete_null_script_header(payload),
            "unread bytes remain incomplete"
        );
        let mut nonnull = payload.clone();
        *nonnull
            .root
            .unique_field_in_base_chain_mut("m_Script")
            .unwrap()
            .unwrap() = SerializedValue::PPtr {
            file_id: 0,
            path_id: 123,
        };
        assert!(
            !object.factory.has_complete_null_script_header(&nonnull),
            "non-null script still requires recovery"
        );
        let mut trailing = payload.clone();
        trailing.trailing_bytes.push(0);
        assert!(
            !object.factory.has_complete_null_script_header(&trailing),
            "unconsumed payload cannot pass"
        );
    }
    assert_eq!(
        objects
            .recover_header_only_null_script_behaviours(
                &mut workspace,
                ObjectWorkspaceLimits::default(),
                ReferenceGraphLimits::default()
            )
            .unwrap(),
        2
    );
    assert!(objects.report().is_complete(), "{:?}", objects.report());
    assert_eq!(
        objects
            .recover_header_only_null_script_behaviours(
                &mut workspace,
                ObjectWorkspaceLimits::default(),
                ReferenceGraphLimits::default()
            )
            .unwrap(),
        0
    );
}

#[test]
fn empty_script_recovery_preserves_prior_reference_scan_limits() {
    use crate::serialized_file::reference_graph::{
        factory_object_outcome_error, DecodedReferenceObject, ObjectReferenceGraph,
        ReferenceGraphIssue,
    };
    let input = fixture_input_from(BEHAVIOUR_FIXTURE);
    let (filesystem, mut workspace) =
        load_workspace_named_bytes_with_source_factory("BoundedBehaviours.assets", &input);
    let mut objects =
        ObjectWorkspace::load_from_vfs(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .unwrap();
    let graph = ObjectReferenceGraph::build_with_payload_decoder(
        &workspace.files()[0].metadata,
        ReferenceGraphLimits {
            max_field_path_bytes: 1,
            ..Default::default()
        },
        |index| {
            let object = objects.get(0, index).unwrap();
            Ok(DecodedReferenceObject {
                payload: object.payload.clone()?,
                unparsed_tail_bytes: 0,
                incomplete_outcome: (!object.is_fully_materialized())
                    .then(|| factory_object_outcome_error(&object.factory)),
            })
        },
    )
    .unwrap();
    let bound_issues = graph
        .issues()
        .iter()
        .filter(|issue| matches!(issue, ReferenceGraphIssue::FieldPathLimitExceeded { .. }))
        .count();
    assert!(bound_issues > 0);
    assert!(workspace.replace_reference_graph(0, graph));
    assert_eq!(
        objects
            .recover_header_only_null_script_behaviours(
                &mut workspace,
                ObjectWorkspaceLimits::default(),
                ReferenceGraphLimits::default(),
            )
            .unwrap(),
        2
    );
    assert!(objects.report().is_complete());
    let retained = &workspace.files()[0].reference_graph;
    assert!(
        !retained.is_complete(),
        "object recovery must not clear a prior input scan limit"
    );
    assert_eq!(
        retained
            .issues()
            .iter()
            .filter(|issue| matches!(issue, ReferenceGraphIssue::FieldPathLimitExceeded { .. }))
            .count(),
        bound_issues
    );
}

#[test]
fn shared_editor_controller_members_are_copied_with_collection_local_references() {
    let fixture = include_str!(
        "../../../resources/animator_shared_controller_editor_source_2022_3_input.hex"
    );
    let hex = fixture
        .lines()
        .find(|line| !line.starts_with('#') && !line.is_empty())
        .unwrap();
    let input = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect::<Vec<_>>();
    let (filesystem, mut workspace) =
        load_workspace_named_bytes_with_source_factory("SharedControllers.assets", &input);
    let mut objects =
        ObjectWorkspace::load_from_vfs(&filesystem, &workspace, ObjectWorkspaceLimits::default())
            .unwrap();
    assert_eq!(
        objects
            .recover_header_only_null_script_behaviours(
                &mut workspace,
                ObjectWorkspaceLimits::default(),
                ReferenceGraphLimits::default()
            )
            .unwrap(),
        2
    );
    assert!(objects.report().is_complete(), "{:?}", objects.report());
    assert_eq!(objects.objects().len(), 27);
    let graph = workspace
        .reference_graph(MultiFileReferenceGraphLimits::default())
        .unwrap();
    assert!(graph.is_complete());
    let prepared = super::super::animator_collections::prepare_controller_collections(
        &workspace,
        &objects,
        &graph,
        &HashSet::new(),
        crate::export::pointer_map::ExportPointerMapLimits::default(),
    )
    .unwrap();
    assert_eq!(prepared.roots.len(), 2);
    assert_eq!(prepared.member_keys.len(), 25);
    assert!(
        super::super::animator_collections::prepare_controller_collections(
            &workspace,
            &objects,
            &graph,
            &HashSet::new(),
            crate::export::pointer_map::ExportPointerMapLimits {
                max_assets: 51,
                ..Default::default()
            },
        )
        .is_err(),
        "the 52 source/copied members must be bounded during preparation"
    );
    let output = crate::test_support::unique_test_directory("shared-controller-project");
    let report = super::super::export_workspace_yaml_project_with_settings(
        &output,
        false,
        &filesystem,
        &RootResourceFileCatalog::default(),
        &PlatformResourceProvider::new(None, None),
        &workspace,
        &objects,
        &graph,
        &ExportSettingsFile::from(ExportDefaults::default()),
        WorkspaceProjectExportLimits::default(),
    )
    .unwrap();
    assert_eq!(
        report.exported_object_count, 52,
        "both controllers own the complete 26-document graph"
    );
    let mut guids = HashSet::new();
    for controller in objects
        .objects()
        .iter()
        .filter(|o| o.factory.class_id == 91)
    {
        let group = &prepared.roots[&(controller.file_index, controller.object_index)];
        assert!(!group.is_release);
        assert_eq!(group.children.len(), 25);
        let first_owner = controller.path_id == 10000;
        assert!(group
            .children
            .iter()
            .all(|m| m.register_global == first_owner));
        let name = super::super::object_asset_name(controller.payload.as_ref().unwrap()).unwrap();
        let relative = format!("Assets/AnimatorController/{name}.controller");
        let yaml = std::fs::read_to_string(output.join(&relative)).unwrap();
        let meta = std::fs::read_to_string(output.join(format!("{relative}.meta"))).unwrap();
        guids.insert(
            meta.lines()
                .find_map(|l| l.strip_prefix("guid: "))
                .unwrap()
                .to_owned(),
        );
        assert_eq!(yaml_sections(&yaml).len(), 26);
        assert!(
            !yaml.contains("guid:"),
            "all non-null pointers in this input belong to the controller"
        );
        let mut ids = HashMap::from([(9100000, 10000)]);
        for member in &group.children {
            ids.insert(
                member.file_id,
                objects.get(member.key.0, member.key.1).unwrap().path_id,
            );
        }
        // This independently authored second controller shares exactly the original
        // graph, but has its own source name and self references. Only its name is
        // adjusted in the comparison; every hierarchy/behaviour field is retained.
        let expected = BEHAVIOUR_FIXTURE.replace(
            "m_Name: Nested State Machine Source\n",
            &format!("m_Name: {name}\n"),
        );
        assert_yaml_sections_match_fixture(&yaml, &ids, &expected);
    }
    assert_eq!(guids.len(), 2, "each controller owns a distinct .meta GUID");
    assert!(!output.join("Assets/AnimatorStateMachine").exists());
    assert!(!output.join("Assets/MonoBehaviour").exists());
    assert_eq!(
        filesystem
            .read_all_bytes("/game/SharedControllers.assets", input.len())
            .unwrap(),
        input
    );
    std::fs::remove_dir_all(output).unwrap();
}
