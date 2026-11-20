use ahjo_mission_engine::{AgentStore, BindingsSnapshot};
use ahjo_mission_model::{Mission, Seat, Step};
#[test]
fn frozen_bindings_reject_mutated_settings_and_agents() {
    let seat = Seat {
        id: "worker".into(),
        agent: "builder".into(),
        harness: ahjo_mission_model::Harness::OpenAiCodex,
        effort: ahjo_mission_model::Effort::Medium,
        ..Default::default()
    };
    let mission = Mission {
        id: "mission".into(),
        slug: "demo".into(),
        rev: 1,
        steps: vec![Step {
            id: "step".into(),
            seats: vec![seat.clone()],
            ..Default::default()
        }],
        ..Default::default()
    };
    let agents = AgentStore::new(ahjo_testkit::fresh_dir("run-agents").join("agents"));
    let mut frozen = BindingsSnapshot::capture("run_a", &mission, "input", &agents).unwrap();
    assert!(
        frozen
            .binding(&mission, "run_a", "input", "step", &seat)
            .is_ok()
    );
    let mut changed = seat.clone();
    changed.effort = ahjo_mission_model::Effort::High;
    assert_eq!(
        frozen
            .binding(&mission, "run_a", "input", "step", &changed)
            .unwrap_err()
            .code,
        "persona_snapshot_invalid"
    );
    frozen.bindings[0].agent_toml.push_str("bad");
    assert_eq!(
        frozen
            .binding(&mission, "run_a", "input", "step", &seat)
            .unwrap_err()
            .code,
        "persona_snapshot_invalid"
    );
    frozen.schema = 2;
    assert!(
        frozen
            .binding(&mission, "run_a", "input", "step", &seat)
            .is_err()
    );
}
