use ahjo_missions_contract::*;
use ahjo_missions_contract::{commands::*, run_commands::*};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

fn round<T: Serialize + DeserializeOwned>(v: Value) {
    let parsed: T = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), v);
}

#[test]
fn requests_and_responses_round_trip() {
    round::<MissionCreate>(
        json!({"title":"Demo","slug":"demo","updated_by":"human:test","example":null,"steps":null,"note":null}),
    );
    round::<MissionDelete>(json!({"mission":"demo","rev":2,"etag":"e"}));
    round::<MissionNote>(
        json!({"mission":"demo","on":"mission","author":"human:test","text":"hi","edit":"n","delete":null,"etag":"e"}),
    );
    round::<EditLayout>(json!({"cards":[{"id":"a","x":1,"y":2}]}));
    round::<RunAbort>(json!({"run":"r","actor":"human:test","reason":"stop"}));
    round::<RunLand>(json!({"run":"r","force":true}));
    round::<RunLanded>(json!({"node":"local","landed":true,"notes":["done"]}));
    round::<Vec<AgentSeat>>(
        json!([{"dispatch_id":"d","seat_id":"s","node":"local","session":"s","session_id":"$1","pane":2,"outcome":"","available":true,"waiting":{"state":"waiting","detail":"detail","since":"now","seq":1}}]),
    );
    round::<MissionValidated>(
        json!({"node":"local","valid":true,"summary":"valid","mission_rev":1,"mission_etag":"e","errors":[],"warnings":[],"next":"run"}),
    );
    round::<MissionNotesListed>(
        json!({"node":"local","notes":{"schema":3,"mission_id":"m","rev":1,"updated_at":"2026-01-01T00:00:00Z","mission":[{"id":"n","author":"human:test","created_at":"2026-01-01T00:00:00Z","text":"hello"}],"elements":[],"etag":"e"}}),
    );
}

#[test]
fn mission_layout_and_edit_responses_carry_the_stored_shapes() {
    let mission = Mission::parse(
        r#"
schema = 6
id = "m"
slug = "demo"
rev = 2
[[step]]
id = "f"
formation = "solo"
[[step.seat]]
id = "s"
label = "Writer"
"#,
    )
    .unwrap();
    let layout = Layout::parse(
        r#"
schema = 3
mission_id = "m"
mission_rev = 2
[[card]]
id = "f"
x = 10
y = 20
"#,
    )
    .unwrap();
    let mission_value = serde_json::to_value(&mission).unwrap();
    let layout_value = serde_json::to_value(&layout).unwrap();
    let response = serde_json::to_value(MissionDefinition {
        node: "local".into(),
        mission: mission.clone(),
        etag: "m2".into(),
    })
    .unwrap();
    assert_eq!(
        response,
        json!({"node":"local","mission":mission_value,"etag":"m2"})
    );
    round::<MissionDefinition>(response);
    let response = serde_json::to_value(LayoutShown {
        node: "local".into(),
        layout: layout.clone(),
        etag: "l2".into(),
    })
    .unwrap();
    assert_eq!(
        response,
        json!({"node":"local","layout":layout_value,"etag":"l2"})
    );
    round::<LayoutShown>(response);
    let edit = MissionEdited {
        definition: MissionDefinition {
            node: "local".into(),
            mission,
            etag: "m2".into(),
        },
        layout,
        layout_etag: "l2".into(),
        added: None,
    };
    let response = serde_json::to_value(edit).unwrap();
    assert_eq!(
        response,
        json!({"node":"local","mission":mission_value,"etag":"m2","layout":layout_value,"layout_etag":"l2"})
    );
    round::<MissionEdited>(response);
}

#[test]
fn patch_and_frames_round_trip() {
    let patch: patch::UniquePatch = serde_json::from_value(
        json!({"expected_rev":2,"create_gate":{"title":"Review","kinds":["human"]}}),
    )
    .unwrap();
    assert!(matches!(
        patch::decode_request(&patch.0).unwrap().changes,
        patch::Changes::Single(change) if matches!(*change, MissionChange::CreateGate { .. })
    ));
    round::<patch::UniquePatch>(serde_json::to_value(patch).unwrap());
    assert!(
        serde_json::from_str::<patch::UniquePatch>(r#"{"title":"one","title":"two"}"#).is_err()
    );
    for value in [false, true] {
        assert_eq!(Watch::decode(&Watch(value).encode()), Some(Watch(value)));
    }
    assert_eq!(Watch::decode(&[]), None);
    round::<MissionSnapshot>(
        json!({"missions":[{"slug":"demo","id":"m","title":"Demo","rev":1,"steps":3}]}),
    );
    round::<RunsSnapshot>(json!([]));
}

#[test]
fn rejected_start_keeps_every_engine_finding() {
    let findings = json!([{"code":"missing_input","card_id":"input","message":"required"},{"code":"missing_file","card_id":"step","message":"missing","path":"brief.md"}]);
    let error = ahjo_core::Error::new(
        error::RUN_ADMISSION_FAILED,
        format!("run admission failed: {findings}"),
    );
    assert_eq!(
        serde_json::to_value(admission_findings(&error).unwrap()).unwrap(),
        findings
    );
    assert!(admission_findings(&ahjo_core::Error::new("not_found", "missing")).is_none());
}

#[test]
fn remaining_envelopes_and_queries_round_trip() {
    round::<RunList>(json!({"mission":"demo"}));
    let show = RunShow {
        run: "r".into(),
        step: Some("f".into()),
        attempts: true,
        mission: true,
        ..Default::default()
    };
    let show_value = serde_json::to_value(show).unwrap();
    assert_eq!(show_value["attempts"], true);
    assert_eq!(show_value["mission"], true);
    assert_eq!(show_value["step"], "f");
    round::<RunShow>(show_value);
    round::<MissionRepositories>(json!({}));
    round::<MissionRepositoriesListed>(json!({"node":"local","repositories":["main"]}));
    round::<EditLayout>(json!({"arrange":true}));
    round::<EditLayout>(json!({"edges":[{"id":"e","lane":"top"}]}));
    round::<LayoutShown>(
        json!({"node":"local","layout":{"schema":3,"mission_id":"m","mission_rev":1,"card":[],"edge":[]},"etag":"e"}),
    );
}

#[test]
fn notes_group_by_target_and_keep_order() {
    let entry = |id: &str, target: &str| NoteEntry {
        id: id.into(),
        target: target.into(),
        author: "human:test".into(),
        created_at: "2026-01-01T00:00:00Z".into(),
        text: id.into(),
        ..Default::default()
    };
    let notes = Notes {
        mission_id: "m".into(),
        entries: vec![entry("1", "f"), entry("2", "mission"), entry("3", "f")],
        ..Default::default()
    };
    let document = document::NotesDocument::from_notes(notes, "e".into());
    assert_eq!(document.mission[0].id, "2");
    assert_eq!(document.elements[0].card_id, "f");
    assert_eq!(
        serde_json::to_value(&document.elements[0]).unwrap()["card_id"],
        "f"
    );
    assert_eq!(
        document.elements[0]
            .entries
            .iter()
            .map(|e| e.id.as_str())
            .collect::<Vec<_>>(),
        ["1", "3"]
    );
    assert_eq!(document.updated_at, "0001-01-01T00:00:00Z");
}
