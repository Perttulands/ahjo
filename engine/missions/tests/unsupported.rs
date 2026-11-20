use ahjo_mission_engine::{Harnesses, Store, run_admission, validate};
use ahjo_mission_model::{Code, Mission};
use std::collections::BTreeMap;

fn gated() -> Mission {
    Mission::parse(include_str!(
        "../../../areas/missions/app/tests/fixtures/gated.mission.toml"
    ))
    .unwrap()
}
#[test]
fn reports_unsupported_capabilities_on_the_nodes_and_refuses_admission() {
    let root = ahjo_testkit::fresh_dir("unsupported-validation");
    let store = Store::new(&root);
    let mut mission = gated();
    let solo = validate(&mission, &store.agents(), &Harnesses::default());
    assert!(!solo.errors.iter().any(|f| matches!(
        f.code,
        Code::UnsupportedStep | Code::UnsupportedHumanChannel
    )));
    let step = &mut mission.steps[0];
    step.formation = ahjo_mission_model::Formation::Peer;
    let mut seat = step.seats[0].clone();
    seat.id = "second".into();
    step.seats.push(seat);
    mission.input_cards[0].human_channel = ahjo_mission_model::HumanChannel::Session;
    let report = validate(&mission, &store.agents(), &Harnesses::default());
    assert!(report.errors.iter().any(|f| f.code == Code::UnsupportedStep
        && f.card_id == mission.steps[0].id
        && f.message.contains("not supported")));
    assert!(
        report
            .errors
            .iter()
            .any(|f| f.code == Code::UnsupportedHumanChannel
                && f.card_id == mission.gates[0].id
                && f.message.contains("not supported"))
    );
    let admitted = run_admission(
        &mission,
        &store.agents(),
        &Harnesses::default(),
        &mission.input_cards[0].id,
        false,
        &BTreeMap::new(),
    );
    assert!(
        admitted
            .errors
            .iter()
            .any(|f| f.code == Code::UnsupportedStep)
    );
    assert!(
        admitted
            .errors
            .iter()
            .any(|f| f.code == Code::UnsupportedHumanChannel)
    );
    mission.input_cards[0].human_channel = ahjo_mission_model::HumanChannel::Unset;
    mission.steps[0].formation = ahjo_mission_model::Formation::Orchestrated;
    mission.steps[0].seats[0].controller = true;
    let report = validate(&mission, &store.agents(), &Harnesses::default());
    assert!(!report.errors.iter().any(|f| matches!(
        f.code,
        Code::UnsupportedStep | Code::UnsupportedHumanChannel | Code::OrchestratedController
    )));
    mission.input_cards[0].human_channel = ahjo_mission_model::HumanChannel::Session;
    mission.connections.clear();
    let report = validate(&mission, &store.agents(), &Harnesses::default());
    assert!(
        !report
            .errors
            .iter()
            .any(|f| f.code == Code::UnsupportedHumanChannel),
        "an unreachable human gate does not use this channel"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn legacy_missions_can_be_repaired_without_allowing_unrelated_saves() {
    use ahjo_mission_model::{InputCardUpdate, MissionChange, Precondition};
    let root = ahjo_testkit::fresh_dir("unsupported-repair");
    let folder = root.join("missions");
    std::fs::create_dir_all(&folder).unwrap();
    let source = include_str!("../../../areas/missions/app/tests/fixtures/gated.mission.toml")
        .replace("formation = \"solo\"", "formation = \"peer\"")
        .replace(
            "id = \"inp_start\"",
            "id = \"inp_start\"\nhuman_channel = \"session\"",
        );
    std::fs::write(folder.join("gated.mission.toml"), source).unwrap();
    let store = Store::new(&root);
    let current = store.mission_edit("gated").unwrap();
    let findings = validate(&current.mission, &store.agents(), &Harnesses::default());
    assert!(
        findings
            .errors
            .iter()
            .any(|f| f.code == Code::UnsupportedStep)
    );
    assert!(
        findings
            .errors
            .iter()
            .any(|f| f.code == Code::UnsupportedHumanChannel)
    );
    let pre = |edit: &ahjo_mission_model::Edit| Precondition {
        rev: edit.mission.rev,
        etag: edit.etag.clone(),
    };
    let denied = store
        .patch(
            "gated",
            pre(&current),
            MissionChange::Title("Unrelated".into()),
            "",
        )
        .unwrap_err();
    assert_eq!(denied.code, "unsupported_step");
    assert_eq!(store.mission_edit("gated").unwrap().etag, current.etag);
    let card = current.mission.input_cards[0].id.clone();
    let step = current.mission.steps[0].id.clone();
    let repaired = store
        .patch(
            "gated",
            pre(&current),
            MissionChange::UpdateInputCard(InputCardUpdate {
                id: card,
                human_channel: Some(ahjo_mission_model::HumanChannel::Notify),
                ..Default::default()
            }),
            "",
        )
        .unwrap();
    let repaired = store
        .patch(
            "gated",
            pre(&repaired),
            MissionChange::SetStepFormation {
                id: step,
                formation: ahjo_mission_model::Formation::Solo,
                keep_seat_id: "".into(),
                seats: None,
            },
            "",
        )
        .unwrap();
    assert_eq!(
        repaired.mission.steps[0].formation,
        ahjo_mission_model::Formation::Solo
    );
    store
        .patch(
            "gated",
            pre(&repaired),
            MissionChange::Title("Repaired".into()),
            "",
        )
        .unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

fn lab(setup: &ahjo_node_kit::Setup) -> anyhow::Result<ahjo_node_kit::Built> {
    ahjo_missions::build_with_executor(setup, Some(Box::new(ahjo_mission_engine::LabExecutor)))
}
const LAB: ahjo_node_kit::Listing = ahjo_node_kit::Listing {
    build: lab,
    ..ahjo_missions::LISTING
};
#[tokio::test(flavor = "multi_thread")]
async fn cli_refuses_to_save_unsupported_step_and_channel() {
    use ahjo_testkit::{node::Node, sessions};
    use serde_json::json;
    let node = Node::start(&[sessions::LISTING, ahjo_work::LISTING, LAB], "")
        .await
        .unwrap();
    let folder = node.dir().join("missions/missions");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("gated.mission.toml"),
        include_str!("../../../areas/missions/app/tests/fixtures/gated.mission.toml"),
    )
    .unwrap();
    let before = std::fs::read(folder.join("gated.mission.toml")).unwrap();
    for (operation, code) in [
        (
            json!({"create_step":{"formation":"peer"}}),
            "unsupported_step",
        ),
        (
            json!({"update_input_card":{"id":"inp_start","human_channel":"session"}}),
            "unsupported_human_channel",
        ),
    ] {
        let mut output = Vec::new();
        let args = vec![
            "edit".into(),
            "gated".into(),
            "--change".into(),
            operation.to_string(),
            "--node".into(),
            format!("{}/", node.url()),
        ];
        assert_eq!(
            ahjo_node_kit::cli::run("mission", &args, &mut output)
                .await
                .unwrap(),
            1
        );
        let error = node
            .call(
                "mission_edit",
                json!({"mission":"gated","change":operation}),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, code);
        assert!(error.message.contains("not supported"));
    }
    assert_eq!(
        std::fs::read(folder.join("gated.mission.toml")).unwrap(),
        before
    );
    node.stop().await;
}
