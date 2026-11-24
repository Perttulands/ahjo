use ahjo_mission_engine::Store;
use ahjo_mission_model::{MissionChange, Precondition};
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Scratch(std::path::PathBuf);
impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ahjo-97-mission-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn pre(edit: &ahjo_mission_model::Edit) -> Precondition {
    Precondition {
        rev: edit.mission.rev,
        etag: edit.etag.clone(),
    }
}

// Both formation restoration and node restore reject invalid shapes before
// touching either file.
#[test]
fn invalid_step_shapes_are_refused_by_both_restore_paths() {
    use ahjo_mission_model::{RestoredCard, Seat, Step};
    let dir = Scratch::new();
    let store = Store::new(&dir.0);
    let edit = store.create_mission("shape", "Shape", "").unwrap();
    let edit = store
        .patch(
            "shape",
            pre(&edit),
            MissionChange::CreateStep {
                formation: ahjo_mission_model::Formation::Solo,
                title: "Work".into(),
                x: 3,
                y: 4,
            },
            "",
        )
        .unwrap();
    let id = edit.mission.steps[0].id.clone();
    let mission_path = dir.0.join("missions/shape.mission.toml");
    let layout_path = dir.0.join("layout/shape.layout.toml");
    let mission_before = std::fs::read(&mission_path).unwrap();
    let layout_before = std::fs::read(&layout_path).unwrap();
    let seat = |name: &str, controller| Seat {
        id: name.into(),
        controller,
        ..Default::default()
    };
    let shapes = vec![
        ("solo", vec![]),
        ("solo", vec![seat("seat_a", false), seat("seat_b", false)]),
        ("peer", vec![]),
        ("peer", vec![seat("seat_a", false)]),
        ("peer", vec![seat("seat_a", true), seat("seat_b", false)]),
        ("orchestrated", vec![]),
        ("orchestrated", vec![seat("seat_a", true)]),
        (
            "orchestrated",
            vec![seat("seat_a", false), seat("seat_b", false)],
        ),
        (
            "orchestrated",
            vec![seat("seat_a", true), seat("seat_b", true)],
        ),
    ];
    for (kind, seats) in shapes {
        let err = store
            .patch(
                "shape",
                pre(&edit),
                MissionChange::SetStepFormation {
                    id: id.clone(),
                    formation: kind.parse().unwrap(),
                    keep_seat_id: "".into(),
                    seats: Some(seats.clone()),
                },
                "",
            )
            .unwrap_err();
        assert_eq!(
            err.code.as_str(),
            "invalid_type_change",
            "{kind}: {seats:?}"
        );
        let err = store
            .patch(
                "shape",
                pre(&edit),
                MissionChange::RestoreCard {
                    card: RestoredCard::Step(Step {
                        id: "fmn_bad".into(),
                        formation: kind.parse().unwrap(),
                        seats: seats.clone(),
                        ..Default::default()
                    }),
                    connections: vec![],
                    index: None,
                    x: 0,
                    y: 0,
                },
                "",
            )
            .unwrap_err();
        assert_eq!(
            err.code.as_str(),
            "invalid_card_restore",
            "{kind}: {seats:?}"
        );
        assert_eq!(std::fs::read(&mission_path).unwrap(), mission_before);
        assert_eq!(std::fs::read(&layout_path).unwrap(), layout_before);
    }
}

// Ported from port_restore_test.go: each position in either direction survives
// remove/undo, including wires and original order.
#[test]
fn restore_each_input_and_output_port_at_its_original_index() {
    for (direction, index) in [("input", 0), ("input", 1), ("output", 0), ("output", 1)] {
        let dir = Scratch::new();
        let store = Store::new(&dir.0);
        let mut edit = store.create_mission("ports", "Ports", "").unwrap();
        for title in ["Source", "Target"] {
            edit = store
                .patch(
                    "ports",
                    pre(&edit),
                    MissionChange::CreateStep {
                        formation: ahjo_mission_model::Formation::Solo,
                        title: title.into(),
                        x: 0,
                        y: 0,
                    },
                    "",
                )
                .unwrap();
        }
        let source = edit.mission.steps[0].id.clone();
        let target = edit.mission.steps[1].id.clone();
        let edited_id = if direction == "input" {
            &target
        } else {
            &source
        };
        edit = store
            .patch(
                "ports",
                pre(&edit),
                MissionChange::AddPort {
                    step_id: edited_id.clone(),
                    direction: direction.into(),
                    label: "Second".into(),
                    kind: ahjo_mission_model::PortKind::Unset,
                },
                "",
            )
            .unwrap();
        let before = edit
            .mission
            .steps
            .iter()
            .find(|f| &f.id == edited_id)
            .unwrap()
            .clone();
        let port = if direction == "input" {
            before.inputs[index].clone()
        } else {
            before.outputs[index].clone()
        };
        let from_port = if direction == "output" {
            &port
        } else {
            &edit.mission.steps[0].outputs[0]
        };
        let to_port = if direction == "input" {
            &port
        } else {
            &edit.mission.steps[1].inputs[0]
        };
        edit = store
            .patch(
                "ports",
                pre(&edit),
                MissionChange::WireConnection {
                    id: "edge_restore".into(),
                    from: format!("{source}:{}", from_port.id),
                    to: format!("{target}:{}", to_port.id),
                    join_if_occupied: false,
                },
                "",
            )
            .unwrap();
        let wire = edit.mission.connections[0].clone();
        let rev = edit.mission.rev;
        edit = store
            .patch(
                "ports",
                pre(&edit),
                MissionChange::RemovePort {
                    step_id: edited_id.clone(),
                    port_id: port.id.clone(),
                },
                "",
            )
            .unwrap();
        assert!(edit.mission.connections.is_empty());
        edit = store
            .patch(
                "ports",
                pre(&edit),
                MissionChange::RestorePort {
                    step_id: edited_id.clone(),
                    direction: direction.into(),
                    port: port.clone(),
                    index: Some(index),
                    connections: vec![wire.clone()],
                },
                "",
            )
            .unwrap();
        assert_eq!(edit.mission.rev, rev + 2);
        assert_eq!(
            edit.mission
                .steps
                .iter()
                .find(|f| &f.id == edited_id)
                .unwrap(),
            &before
        );
        assert_eq!(edit.mission.connections, vec![wire]);
        assert_eq!(
            store
                .patch(
                    "ports",
                    pre(&edit),
                    MissionChange::RestorePort {
                        step_id: edited_id.clone(),
                        direction: direction.into(),
                        port,
                        index: Some(index),
                        connections: vec![]
                    },
                    ""
                )
                .unwrap_err()
                .code
                .as_str(),
            "invalid_card_restore"
        );
    }
}

// Exact seat snapshots are undo data.
#[test]
fn step_formation_restores_exact_seat_snapshot_and_rejects_bad_choices() {
    let dir = Scratch::new();
    let store = Store::new(&dir.0);
    let edit = store.create_mission("type", "Type", "").unwrap();
    let mut edit = store
        .patch(
            "type",
            pre(&edit),
            MissionChange::CreateStep {
                formation: ahjo_mission_model::Formation::Orchestrated,
                title: "Desk".into(),
                x: 0,
                y: 0,
            },
            "",
        )
        .unwrap();
    let step = edit.mission.steps[0].clone();
    let snapshot = step.seats.clone();
    edit = store
        .patch(
            "type",
            pre(&edit),
            MissionChange::SetStepFormation {
                id: step.id.clone(),
                formation: ahjo_mission_model::Formation::Solo,
                keep_seat_id: snapshot[1].id.clone(),
                seats: None,
            },
            "",
        )
        .unwrap();
    edit = store
        .patch(
            "type",
            pre(&edit),
            MissionChange::SetStepFormation {
                id: step.id.clone(),
                formation: ahjo_mission_model::Formation::Orchestrated,
                keep_seat_id: "".into(),
                seats: Some(snapshot.clone()),
            },
            "",
        )
        .unwrap();
    assert_eq!(edit.mission.steps[0].seats, snapshot);
    let path = dir.0.join("missions/type.mission.toml");
    let before = std::fs::read(&path).unwrap();
    let duplicate = vec![snapshot[0].clone(), snapshot[0].clone()];
    for (id, kind, keep, seats, code) in [
        (
            step.id.as_str(),
            "flow",
            "",
            None,
            "unsupported_step_formation",
        ),
        (
            step.id.as_str(),
            "swarm",
            "",
            None,
            "unsupported_step_formation",
        ),
        (
            step.id.as_str(),
            "peer",
            snapshot[0].id.as_str(),
            None,
            "invalid_type_change",
        ),
        (
            step.id.as_str(),
            "solo",
            "seat_missing",
            None,
            "invalid_type_change",
        ),
        (
            step.id.as_str(),
            "peer",
            "",
            Some(duplicate),
            "invalid_type_change",
        ),
        ("fmn_missing", "solo", "", None, "not_found"),
    ] {
        if code == "unsupported_step_formation" {
            let error = serde_json::from_value::<MissionChange>(serde_json::json!({
                "set_step_formation": {"id": id, "formation": kind}
            }))
            .unwrap_err()
            .to_string();
            assert!(error.contains(kind), "{error}");
            assert!(
                error.contains("solo") && error.contains("peer") && error.contains("orchestrated"),
                "{error}"
            );
            assert_eq!(std::fs::read(&path).unwrap(), before);
            continue;
        }
        assert_eq!(
            store
                .patch(
                    "type",
                    pre(&edit),
                    MissionChange::SetStepFormation {
                        id: id.into(),
                        formation: kind.parse().unwrap(),
                        keep_seat_id: keep.into(),
                        seats
                    },
                    ""
                )
                .unwrap_err()
                .code
                .as_str(),
            code
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
}

// Ported from gate_update_test.go: dropping step removes only judge wires.
#[test]
fn changing_gate_kinds_detaches_judge_without_touching_layout() {
    let dir = Scratch::new();
    let store = Store::new(&dir.0);
    let mut edit = store.create_mission("judge", "Judge", "").unwrap();
    edit = store
        .patch(
            "judge",
            pre(&edit),
            MissionChange::CreateStep {
                formation: ahjo_mission_model::Formation::Solo,
                title: "Reviewer".into(),
                x: 0,
                y: 0,
            },
            "",
        )
        .unwrap();
    edit = store
        .patch(
            "judge",
            pre(&edit),
            MissionChange::CreateGate {
                title: "Gate".into(),
                kinds: vec![
                    ahjo_mission_model::GateKind::Code,
                    ahjo_mission_model::GateKind::Step,
                ],
                criterion: "Review".into(),
                check: "output_contains".into(),
                check_version: "1".into(),
                check_value: "OK".into(),
                files: vec![],
                x: 2,
                y: 3,
            },
            "",
        )
        .unwrap();
    let judge = edit.mission.steps[0].id.clone();
    let gate = edit.mission.gates[0].id.clone();
    edit = store
        .patch(
            "judge",
            pre(&edit),
            MissionChange::SetGateJudge {
                gate_id: gate.clone(),
                chain: vec![judge],
            },
            "",
        )
        .unwrap();
    assert_eq!(edit.mission.connections.len(), 2);
    let layout = std::fs::read(dir.0.join("layout/judge.layout.toml")).unwrap();
    edit = store
        .patch(
            "judge",
            pre(&edit),
            MissionChange::UpdateGate(ahjo_mission_model::GateUpdate {
                id: gate.clone(),
                kinds: Some(vec![
                    ahjo_mission_model::GateKind::Code,
                    ahjo_mission_model::GateKind::Human,
                ]),
                ..Default::default()
            }),
            "",
        )
        .unwrap();
    assert!(edit.mission.connections.is_empty());
    assert_eq!(edit.mission.gates[0].check_value, "OK");
    assert_eq!(
        std::fs::read(dir.0.join("layout/judge.layout.toml")).unwrap(),
        layout
    );
    edit = store
        .patch(
            "judge",
            pre(&edit),
            MissionChange::UpdateGate(ahjo_mission_model::GateUpdate {
                id: gate,
                kinds: Some(vec![
                    ahjo_mission_model::GateKind::Human,
                    ahjo_mission_model::GateKind::Step,
                ]),
                ..Default::default()
            }),
            "",
        )
        .unwrap();
    assert_eq!(edit.mission.gates[0].check, "");
    assert_eq!(edit.mission.gates[0].check_value, "");
}

// Gates never grow another input: several steps may feed a gate's one
// input, which picks among them, but nothing else joins them there, and
// stale callers are rejected.
#[test]
fn join_feeds_a_gates_one_input_from_steps_only_and_rejects_stale_writes() {
    let dir = Scratch::new();
    let store = Store::new(&dir.0);
    let mut edit = store.create_mission("join-gate", "Join gate", "").unwrap();
    for title in ["One", "Two"] {
        edit = store
            .patch(
                "join-gate",
                pre(&edit),
                MissionChange::CreateStep {
                    formation: ahjo_mission_model::Formation::Solo,
                    title: title.into(),
                    x: 0,
                    y: 0,
                },
                "",
            )
            .unwrap();
    }
    for title in ["Gate", "Other"] {
        edit = store
            .patch(
                "join-gate",
                pre(&edit),
                MissionChange::CreateGate {
                    title: title.into(),
                    kinds: vec![ahjo_mission_model::GateKind::Human],
                    criterion: "".into(),
                    check: "".into(),
                    check_version: "".into(),
                    check_value: "".into(),
                    files: vec![],
                    x: 0,
                    y: 0,
                },
                "",
            )
            .unwrap();
    }
    let from = |i: usize| {
        format!(
            "{}:{}",
            edit.mission.steps[i].id, edit.mission.steps[i].outputs[0].id
        )
    };
    let first = from(0);
    let second = from(1);
    let to = format!("{}:in", edit.mission.gates[0].id);
    let stale = pre(&edit);
    let wire = |from: String| MissionChange::WireConnection {
        id: "".into(),
        from,
        to: to.clone(),
        join_if_occupied: true,
    };
    edit = store
        .patch("join-gate", stale.clone(), wire(first), "")
        .unwrap();
    assert_eq!(
        store
            .patch("join-gate", stale, wire(second.clone()), "")
            .unwrap_err()
            .code
            .as_str(),
        "conflict"
    );
    edit = store
        .patch("join-gate", pre(&edit), wire(second), "")
        .unwrap();
    let feeding: Vec<_> = (edit.mission.connections.iter())
        .filter(|c| c.to == to)
        .collect();
    assert_eq!(feeding.len(), 2);
    let input = format!("{}:pass", edit.mission.gates[1].id);
    assert_eq!(
        store
            .patch("join-gate", pre(&edit), wire(input), "")
            .unwrap_err()
            .code
            .as_str(),
        "input_occupied"
    );
    assert_eq!(store.mission_edit("join-gate").unwrap().etag, edit.etag);
}

// Feedback may share an occupied input
// during rewire and undo without introducing a required extra port.
#[test]
fn feedback_rewire_and_join_undo_preserve_pushback_exception() {
    let dir = Scratch::new();
    let store = Store::new(&dir.0);
    let mut edit = store.create_mission("feedback", "Feedback", "").unwrap();
    for title in ["Frame", "Research", "Ship"] {
        edit = store
            .patch(
                "feedback",
                pre(&edit),
                MissionChange::CreateStep {
                    formation: ahjo_mission_model::Formation::Solo,
                    title: title.into(),
                    x: 0,
                    y: 0,
                },
                "",
            )
            .unwrap();
    }
    edit = store
        .patch(
            "feedback",
            pre(&edit),
            MissionChange::CreateGate {
                title: "Review".into(),
                kinds: vec![ahjo_mission_model::GateKind::Human],
                criterion: "".into(),
                check: "".into(),
                check_version: "".into(),
                check_value: "".into(),
                files: vec![],
                x: 0,
                y: 0,
            },
            "",
        )
        .unwrap();
    let endpoint = |i: usize, output: bool| {
        let f = &edit.mission.steps[i];
        format!(
            "{}:{}",
            f.id,
            if output {
                &f.outputs[0].id
            } else {
                &f.inputs[0].id
            }
        )
    };
    let (frame_out, research_in, ship_in, ship_out) = (
        endpoint(0, true),
        endpoint(1, false),
        endpoint(2, false),
        endpoint(2, true),
    );
    let fail = format!("{}:fail", edit.mission.gates[0].id);
    let wire = |from: &str, to: &str| MissionChange::WireConnection {
        id: "".into(),
        from: from.into(),
        to: to.into(),
        join_if_occupied: false,
    };
    edit = store
        .patch("feedback", pre(&edit), wire(&frame_out, &ship_in), "")
        .unwrap();
    edit = store
        .patch("feedback", pre(&edit), wire(&fail, &research_in), "")
        .unwrap();
    edit = store
        .patch(
            "feedback",
            pre(&edit),
            MissionChange::RewireConnection {
                from: fail.clone(),
                previous_to: research_in.clone(),
                to: ship_in.clone(),
                join_if_occupied: true,
                remove_previous_input: false,
            },
            "",
        )
        .unwrap();
    assert_eq!(edit.mission.steps[2].inputs.len(), 1);
    assert!(
        edit.mission
            .connections
            .iter()
            .any(|c| c.from == fail && c.to == ship_in)
    );
    edit = store
        .patch("feedback", pre(&edit), wire(&ship_out, &research_in), "")
        .unwrap();
    edit = store
        .patch(
            "feedback",
            pre(&edit),
            MissionChange::RewireConnection {
                from: frame_out.clone(),
                previous_to: ship_in.clone(),
                to: research_in,
                join_if_occupied: true,
                remove_previous_input: false,
            },
            "",
        )
        .unwrap();
    let joined = edit
        .mission
        .connections
        .iter()
        .find(|c| c.from == frame_out)
        .unwrap()
        .to
        .clone();
    assert_eq!(edit.mission.steps[1].inputs.len(), 2);
    edit = store
        .patch(
            "feedback",
            pre(&edit),
            MissionChange::RewireConnection {
                from: frame_out.clone(),
                previous_to: joined,
                to: ship_in.clone(),
                join_if_occupied: false,
                remove_previous_input: true,
            },
            "",
        )
        .unwrap();
    assert_eq!(edit.mission.steps[1].inputs.len(), 1);
    assert!(
        edit.mission
            .connections
            .iter()
            .any(|c| c.from == frame_out && c.to == ship_in)
    );
    assert!(
        edit.mission
            .connections
            .iter()
            .any(|c| c.from == fail && c.to == ship_in)
    );
}

// Ported from node_restore_test.go: failed undo does not publish either file.
#[test]
fn restore_refuses_missing_foreign_and_occupied_connections_and_bad_nodes() {
    use ahjo_mission_model::{Connection, Gate, RestoredCard, Step};
    let dir = Scratch::new();
    let store = Store::new(&dir.0);
    let mut edit = store.create_mission("restore", "Restore", "").unwrap();
    for title in ["Build", "Other", "Target"] {
        edit = store
            .patch(
                "restore",
                pre(&edit),
                MissionChange::CreateStep {
                    formation: ahjo_mission_model::Formation::Solo,
                    title: title.into(),
                    x: 0,
                    y: 0,
                },
                "",
            )
            .unwrap();
    }
    let build = edit.mission.steps[0].clone();
    let other = edit.mission.steps[1].clone();
    let target = edit.mission.steps[2].clone();
    let output = |f: &Step| format!("{}:{}", f.id, f.outputs[0].id);
    let input = |f: &Step| format!("{}:{}", f.id, f.inputs[0].id);
    edit = store
        .patch(
            "restore",
            pre(&edit),
            MissionChange::WireConnection {
                id: "edge_occupied".into(),
                from: output(&other),
                to: input(&target),
                join_if_occupied: false,
            },
            "",
        )
        .unwrap();
    edit = store
        .patch(
            "restore",
            pre(&edit),
            MissionChange::DeleteStep {
                id: build.id.clone(),
            },
            "",
        )
        .unwrap();
    let mission_path = dir.0.join("missions/restore.mission.toml");
    let layout_path = dir.0.join("layout/restore.layout.toml");
    let mission_before = std::fs::read(&mission_path).unwrap();
    let layout_before = std::fs::read(&layout_path).unwrap();
    let connection = |from: String, to: String| Connection {
        id: "edge_x".into(),
        from,
        to,
    };
    let cases = vec![
        (
            RestoredCard::Step(other.clone()),
            vec![],
            "invalid_card_restore",
        ),
        (
            RestoredCard::Step(build.clone()),
            vec![connection(output(&build), "gate_missing:in".into())],
            "invalid_card_restore",
        ),
        (
            RestoredCard::Step(build.clone()),
            vec![connection(output(&other), input(&target))],
            "invalid_card_restore",
        ),
        (
            RestoredCard::Step(build.clone()),
            vec![connection(output(&build), input(&target))],
            "invalid_card_restore",
        ),
        (
            RestoredCard::Step(Step {
                id: "fmn_old".into(),
                formation: ahjo_mission_model::Formation::Unset,
                ..Default::default()
            }),
            vec![],
            "unsupported_step_formation",
        ),
        (
            RestoredCard::Gate(Gate {
                id: "gate_new".into(),
                ..Default::default()
            }),
            vec![],
            "invalid_gate_kind",
        ),
        (
            RestoredCard::Gate(Gate {
                id: "gate_new".into(),
                kinds: vec![ahjo_mission_model::GateKind::Human],
                check_value: "PASS".into(),
                ..Default::default()
            }),
            vec![],
            "invalid_code_gate_profile",
        ),
        (
            RestoredCard::Step(Step {
                id: "fmn_copy".into(),
                formation: ahjo_mission_model::Formation::Solo,
                seats: other.seats.clone(),
                ..Default::default()
            }),
            vec![],
            "invalid_card_restore",
        ),
    ];
    let error = serde_json::from_value::<RestoredCard>(serde_json::json!({
        "step": {"id": "fmn_old", "formation": "flow"}
    }))
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("flow") && error.contains("solo") && error.contains("orchestrated"),
        "{error}"
    );
    assert_eq!(std::fs::read(&mission_path).unwrap(), mission_before);
    assert_eq!(std::fs::read(&layout_path).unwrap(), layout_before);
    for (node, connections, code) in cases {
        let error = store
            .patch(
                "restore",
                pre(&edit),
                MissionChange::RestoreCard {
                    card: node,
                    connections,
                    index: None,
                    x: 0,
                    y: 0,
                },
                "",
            )
            .unwrap_err();
        assert_eq!(error.code.as_str(), code);
        assert_eq!(std::fs::read(&mission_path).unwrap(), mission_before);
        assert_eq!(std::fs::read(&layout_path).unwrap(), layout_before);
    }
}

// Ported from store_test.go: judge chains have a single send and return,
// can extend the return, and cannot occupy an already-fed input.
#[test]
fn judge_chain_extends_its_return_and_refuses_occupied_inputs() {
    let dir = Scratch::new();
    let store = Store::new(&dir.0);
    let mut edit = store.create_mission("chain", "Chain", "").unwrap();
    for title in ["Judge 1", "Judge 2", "Judge 3", "Other"] {
        edit = store
            .patch(
                "chain",
                pre(&edit),
                MissionChange::CreateStep {
                    formation: ahjo_mission_model::Formation::Solo,
                    title: title.into(),
                    x: 0,
                    y: 0,
                },
                "",
            )
            .unwrap();
    }
    edit = store
        .patch(
            "chain",
            pre(&edit),
            MissionChange::CreateGate {
                title: "Review".into(),
                kinds: vec![ahjo_mission_model::GateKind::Human],
                criterion: "".into(),
                check: "".into(),
                check_version: "".into(),
                check_value: "".into(),
                files: vec![],
                x: 0,
                y: 0,
            },
            "",
        )
        .unwrap();
    let steps = edit.mission.steps.clone();
    let gate = edit.mission.gates[0].id.clone();
    let endpoint = |i: usize, output: bool| {
        let f = &steps[i];
        format!(
            "{}:{}",
            f.id,
            if output {
                &f.outputs[0].id
            } else {
                &f.inputs[0].id
            }
        )
    };
    edit = store
        .patch(
            "chain",
            pre(&edit),
            MissionChange::WireConnection {
                id: "".into(),
                from: endpoint(3, true),
                to: endpoint(0, false),
                join_if_occupied: false,
            },
            "",
        )
        .unwrap();
    let chain = |count: usize| MissionChange::SetGateJudge {
        gate_id: gate.clone(),
        chain: steps.iter().take(count).map(|f| f.id.clone()).collect(),
    };
    assert_eq!(
        store
            .patch("chain", pre(&edit), chain(1), "")
            .unwrap_err()
            .code
            .as_str(),
        "input_occupied"
    );
    assert_eq!(store.mission_edit("chain").unwrap().etag, edit.etag);
    edit = store
        .patch(
            "chain",
            pre(&edit),
            MissionChange::UnwireConnection {
                from: endpoint(3, true),
                to: endpoint(0, false),
            },
            "",
        )
        .unwrap();
    edit = store.patch("chain", pre(&edit), chain(2), "").unwrap();
    assert_eq!(edit.mission.connections.len(), 3);
    let gate_endpoint = format!("{gate}:judge");
    assert_eq!(
        edit.mission
            .connections
            .iter()
            .filter(|c| c.from == gate_endpoint)
            .count(),
        1
    );
    assert_eq!(
        edit.mission
            .connections
            .iter()
            .filter(|c| c.to == gate_endpoint)
            .count(),
        1
    );
    edit = store.patch("chain", pre(&edit), chain(3), "").unwrap();
    assert!(
        edit.mission
            .connections
            .iter()
            .any(|c| c.from == gate_endpoint && c.to == endpoint(0, false))
    );
    assert!(
        edit.mission
            .connections
            .iter()
            .any(|c| c.from == endpoint(2, true) && c.to == gate_endpoint)
    );
}

// Port of TestS3RewireRejectsOccupiedTargetWithoutDroppingOriginal.
#[test]
fn refused_rewire_retains_original_and_occupied_connections() {
    let dir = Scratch::new();
    let store = Store::new(&dir.0);
    let mut edit = store.create_mission("rewire", "Rewire", "").unwrap();
    for title in ["Frame", "Research", "Ship"] {
        edit = store
            .patch(
                "rewire",
                pre(&edit),
                MissionChange::CreateStep {
                    formation: ahjo_mission_model::Formation::Solo,
                    title: title.into(),
                    x: 0,
                    y: 0,
                },
                "",
            )
            .unwrap();
    }
    let endpoint = |i: usize, output: bool| {
        let f = &edit.mission.steps[i];
        format!(
            "{}:{}",
            f.id,
            if output {
                &f.outputs[0].id
            } else {
                &f.inputs[0].id
            }
        )
    };
    let (frame_out, research_in, research_out, ship_in) = (
        endpoint(0, true),
        endpoint(1, false),
        endpoint(1, true),
        endpoint(2, false),
    );
    for (from, to) in [(&frame_out, &research_in), (&research_out, &ship_in)] {
        edit = store
            .patch(
                "rewire",
                pre(&edit),
                MissionChange::WireConnection {
                    id: "".into(),
                    from: from.clone(),
                    to: to.clone(),
                    join_if_occupied: false,
                },
                "",
            )
            .unwrap();
    }
    let original = edit.mission.connections.clone();
    let error = store
        .patch(
            "rewire",
            pre(&edit),
            MissionChange::RewireConnection {
                from: frame_out,
                previous_to: research_in,
                to: ship_in,
                join_if_occupied: false,
                remove_previous_input: false,
            },
            "",
        )
        .unwrap_err();
    assert_eq!(error.code.as_str(), "input_occupied");
    let after = store.mission_edit("rewire").unwrap();
    assert_eq!(after.etag, edit.etag);
    assert_eq!(after.mission.connections, original);
}

// Port of TestS3DynamicPortsKeepStableIDsAndPruneEdges.
#[test]
fn removing_a_dynamic_port_prunes_only_its_connections() {
    let dir = Scratch::new();
    let store = Store::new(&dir.0);
    let mut edit = store.create_mission("ports", "Ports", "").unwrap();
    for title in ["A", "B", "Target"] {
        edit = store
            .patch(
                "ports",
                pre(&edit),
                MissionChange::CreateStep {
                    formation: ahjo_mission_model::Formation::Solo,
                    title: title.into(),
                    x: 0,
                    y: 0,
                },
                "",
            )
            .unwrap();
    }
    let target = edit.mission.steps[2].id.clone();
    edit = store
        .patch(
            "ports",
            pre(&edit),
            MissionChange::AddPort {
                step_id: target.clone(),
                direction: "input".into(),
                label: "Second".into(),
                kind: ahjo_mission_model::PortKind::Unset,
            },
            "",
        )
        .unwrap();
    let ports = edit.mission.steps[2].inputs.clone();
    assert!(ports[1].id.starts_with("port_"));
    assert_ne!(ports[0].id, ports[1].id);
    let sources: Vec<_> = edit.mission.steps[..2]
        .iter()
        .map(|f| format!("{}:{}", f.id, f.outputs[0].id))
        .collect();
    for i in 0..2 {
        edit = store
            .patch(
                "ports",
                pre(&edit),
                MissionChange::WireConnection {
                    id: format!("edge_{i}"),
                    from: sources[i].clone(),
                    to: format!("{target}:{}", ports[i].id),
                    join_if_occupied: false,
                },
                "",
            )
            .unwrap();
    }
    edit = store
        .patch(
            "ports",
            pre(&edit),
            MissionChange::RemovePort {
                step_id: target,
                port_id: ports[1].id.clone(),
            },
            "",
        )
        .unwrap();
    assert_eq!(edit.mission.steps[2].inputs, vec![ports[0].clone()]);
    assert_eq!(edit.mission.connections.len(), 1);
    assert_eq!(edit.mission.connections[0].id, "edge_0");
}

// Port of TestPeerSnapshotCanRemoveOnlyEmptySeats.
#[test]
fn orchestrated_snapshot_cannot_silently_remove_a_staffed_seat() {
    let dir = Scratch::new();
    let store = Store::new(&dir.0);
    let edit = store.create_mission("peer", "Peer", "").unwrap();
    let mut edit = store
        .patch(
            "peer",
            pre(&edit),
            MissionChange::CreateStep {
                formation: ahjo_mission_model::Formation::Orchestrated,
                title: "Team".into(),
                x: 0,
                y: 0,
            },
            "",
        )
        .unwrap();
    let team = edit.mission.steps[0].clone();
    edit = store
        .patch(
            "peer",
            pre(&edit),
            MissionChange::StaffSeat {
                step_id: team.id.clone(),
                seat_id: team.seats[1].id.clone(),
                agent: "builder".into(),
                harness: ahjo_mission_model::Harness::ClaudeCode,
                model: "".into(),
                effort: ahjo_mission_model::Effort::Medium,
            },
            "",
        )
        .unwrap();
    // Keep a controller and enough workers when removing the staffed seat.
    let mut three = edit.mission.steps[0].seats.clone();
    three.push(ahjo_mission_model::Seat {
        id: "seat_third".into(),
        label: "Third".into(),
        ..Default::default()
    });
    edit = store
        .patch(
            "peer",
            pre(&edit),
            MissionChange::SetStepFormation {
                id: team.id.clone(),
                formation: ahjo_mission_model::Formation::Orchestrated,
                keep_seat_id: "".into(),
                seats: Some(three.clone()),
            },
            "",
        )
        .unwrap();
    let remaining: Vec<_> = three
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != 1)
        .map(|(_, seat)| seat.clone())
        .collect();
    let refused = store
        .patch(
            "peer",
            pre(&edit),
            MissionChange::SetStepFormation {
                id: team.id.clone(),
                formation: ahjo_mission_model::Formation::Orchestrated,
                keep_seat_id: "".into(),
                seats: Some(remaining.clone()),
            },
            "",
        )
        .unwrap_err();
    assert_eq!(refused.code.as_str(), "seat_choice_required");
    assert_eq!(store.mission_edit("peer").unwrap().etag, edit.etag);
    edit = store
        .patch(
            "peer",
            pre(&edit),
            MissionChange::StaffSeat {
                step_id: team.id.clone(),
                seat_id: team.seats[1].id.clone(),
                agent: "".into(),
                harness: ahjo_mission_model::Harness::Unset,
                model: "".into(),
                effort: ahjo_mission_model::Effort::Unset,
            },
            "",
        )
        .unwrap();
    edit = store
        .patch(
            "peer",
            pre(&edit),
            MissionChange::SetStepFormation {
                id: team.id,
                formation: ahjo_mission_model::Formation::Orchestrated,
                keep_seat_id: "".into(),
                seats: Some(remaining.clone()),
            },
            "",
        )
        .unwrap();
    assert_eq!(edit.mission.steps[0].seats, remaining);
}
