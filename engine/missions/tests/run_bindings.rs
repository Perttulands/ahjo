use ahjo_mission_engine::{BindingsSnapshot, RoleStore};
use ahjo_mission_model::{Mission, Slot, Step};
#[test]
fn frozen_bindings_reject_mutated_settings_and_roles() {
    let slot = Slot {
        id: "worker".into(),
        agent_id: "builder".into(),
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
            slots: vec![slot.clone()],
            ..Default::default()
        }],
        ..Default::default()
    };
    let roles = RoleStore::new(std::env::temp_dir().join("ahjo-99-no-such-roles"));
    let mut frozen = BindingsSnapshot::capture("run_a", &mission, "input", &roles).unwrap();
    assert!(
        frozen
            .binding(&mission, "run_a", "input", "step", &slot)
            .is_ok()
    );
    let mut changed = slot.clone();
    changed.effort = ahjo_mission_model::Effort::High;
    assert_eq!(
        frozen
            .binding(&mission, "run_a", "input", "step", &changed)
            .unwrap_err()
            .code,
        "persona_snapshot_invalid"
    );
    frozen.bindings[0].card_toml.push_str("bad");
    assert_eq!(
        frozen
            .binding(&mission, "run_a", "input", "step", &slot)
            .unwrap_err()
            .code,
        "persona_snapshot_invalid"
    );
    frozen.schema = 2;
    assert!(
        frozen
            .binding(&mission, "run_a", "input", "step", &slot)
            .is_err()
    );
}
