use ahjo_mission_engine::{AgentStore, Harnesses, run_admission, validate};
use ahjo_mission_model::{Mission, Port, Seat, Step};
use std::collections::BTreeMap;

#[test]
fn admission_checks_inputs_and_frozen_staffing_across_the_mission() {
    let mut mission =
        Mission::parse(include_str!("parity/runs/missions/solo-lab.mission.toml")).unwrap();
    let error = serde_json::from_value::<Seat>(serde_json::json!({
        "id": "bad", "harness": "unknown", "effort": "medium"
    }))
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("unknown")
            && error.contains("claude-code")
            && error.contains("openai-codex"),
        "{error}"
    );
    mission.steps.push(Step {
        id: "unreachable".into(),
        formation: ahjo_mission_model::Formation::Solo,
        seats: vec![Seat {
            id: "bad".into(),
            harness: ahjo_mission_model::Harness::ClaudeCode,
            effort: ahjo_mission_model::Effort::Ultra,
            ..Default::default()
        }],
        ..Default::default()
    });
    mission.steps.push(Step {
        id: "unreachable_unstaffed".into(),
        formation: ahjo_mission_model::Formation::Solo,
        seats: vec![Seat {
            id: "empty".into(),
            ..Default::default()
        }],
        ..Default::default()
    });
    let agents = AgentStore::new(ahjo_testkit::fresh_dir("run-agents").join("agents"));
    let report = run_admission(
        &mission,
        &agents,
        &Harnesses::default(),
        "inp_hello",
        false,
        &BTreeMap::new(),
    );
    assert!(
        report
            .errors
            .iter()
            .any(|f| f.code.as_str() == "missing_input" && f.card_id == "inp_hello")
    );
    assert!(
        report
            .errors
            .iter()
            .any(|f| f.code.as_str() == "invalid_seat_settings" && f.card_id == "unreachable")
    );
    assert!(
        report
            .errors
            .iter()
            .any(|f| f.code.as_str() == "unstaffed_seat" && f.card_id == "unreachable_unstaffed")
    );
}

/// A step a run reaches runs once each of its inputs holds work, so an input
/// no wire feeds would stop the run there: validation says so before a run
/// starts, and admission refuses it, but a single step's run, which is handed
/// its inputs, still starts.
#[test]
fn a_reached_step_with_an_input_nothing_feeds_is_refused_before_the_run() {
    let mut mission =
        Mission::parse(include_str!("parity/runs/missions/solo-lab.mission.toml")).unwrap();
    let agents = AgentStore::new(ahjo_testkit::fresh_dir("run-agents").join("agents"));
    let inputs = BTreeMap::from([("brief".to_owned(), "hello".to_owned())]);
    let admit = |mission: &Mission, root: &str, single: bool| {
        run_admission(
            mission,
            &agents,
            &Harnesses::default(),
            root,
            single,
            &inputs,
        )
    };
    let unfed = |report: &ahjo_mission_model::Report| {
        report
            .errors
            .iter()
            .filter(|f| f.code.as_str() == "input_without_producer")
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(unfed(&admit(&mission, "inp_hello", false)), vec![]);
    mission.steps[0].inputs.push(Port {
        id: "port_more".into(),
        label: "More".into(),
        ..Default::default()
    });
    let validation = validate(&mission, &agents, &Harnesses::default());
    let found = unfed(&validation);
    assert_eq!(found.len(), 1, "{validation:?}");
    assert_eq!(found[0].card_id, "fmn_work");
    assert!(
        found[0].message.contains("\"More\""),
        "{}",
        found[0].message
    );
    assert_eq!(unfed(&admit(&mission, "inp_hello", false)), found);
    assert_eq!(unfed(&admit(&mission, "fmn_work", true)), vec![]);
    // A step no run reaches is only warned of as unreachable.
    mission.connections.retain(|c| c.id != "edge_start");
    assert_eq!(
        unfed(&validate(&mission, &agents, &Harnesses::default())),
        vec![]
    );
}
