use ahjo_missions_contract::{Layout, Mission};
use ahjo_node_kit::{Listing, NodeClient};
use ahjo_testkit::{node::Node, sessions};
use axum::http::Method;
use serde_json::json;

const AREAS: &[Listing] = &[
    sessions::LISTING,
    ahjo_issue::LISTING,
    ahjo_missions::LISTING,
];

#[tokio::test(flavor = "multi_thread")]
async fn mission_commands_on_a_node_with_fake_sessions() {
    let node = Node::start(AREAS, "").await.unwrap();
    let created = node
        .call("mission_create", json!({"slug":"", "title":"Example"}))
        .await
        .unwrap();
    assert_eq!(created["title"], "Example");
    assert_eq!(created["created"], "example");
    let initial = node
        .call("mission_show", json!({"mission":"example", "full":true}))
        .await
        .unwrap();
    assert_eq!(initial["mission"]["title"], "Example");
    let listed = node.call("mission_list", json!({})).await.unwrap();
    assert_eq!(listed["missions"][0]["slug"], "example");
    let edited = node.call("mission_edit", json!({"mission":"example", "change":{"title":"Renamed"}, "rev":1, "etag":initial["etag"]})).await.unwrap();
    assert_eq!(edited["mission"]["title"], "Renamed");
    let wire = node
        .call("mission_show", json!({"mission":"example", "full":true}))
        .await
        .unwrap();
    let stored =
        std::fs::read_to_string(node.dir().join("missions/missions/example.mission.toml")).unwrap();
    let stored = Mission::parse(&stored).unwrap();
    assert_eq!(wire["mission"], json!(stored));
    assert_eq!(edited["mission"], wire["mission"]);
    assert_eq!(edited["etag"], wire["etag"]);
    let notes = node
        .call("mission_notes", json!({"mission":"example"}))
        .await
        .unwrap();
    assert!(notes["notes"]["mission"].is_array());
    let note = node.call("mission_note", json!({"mission":"example", "text":"intent", "author":"human:test", "etag":notes["notes"]["etag"]})).await.unwrap();
    assert_eq!(note["thread"][0]["text"], "intent");
    let validation = node
        .call("mission_validate", json!({"mission":"example"}))
        .await
        .unwrap();
    assert!(validation["errors"].is_array());
    let layout = node
        .call("layout_show", json!({"mission":"example"}))
        .await
        .unwrap();
    assert!(layout["layout"]["card"].is_array());
    let changed = node.call("layout_edit", json!({"mission":"example", "change":{"cards":[{"id":"a","x":10,"y":20}]}, "etag":layout["etag"]})).await.unwrap();
    assert_eq!(changed["layout"]["card"][0]["x"], 10);
    let stored =
        std::fs::read_to_string(node.dir().join("missions/layout/example.layout.toml")).unwrap();
    assert_eq!(changed["layout"], json!(Layout::parse(&stored).unwrap()));
    let read_layout = node
        .call("layout_show", json!({"mission":"example"}))
        .await
        .unwrap();
    assert_eq!(read_layout["layout"], changed["layout"]);
    assert_eq!(read_layout["etag"], changed["etag"]);
    for (command, body) in [
        ("run_resume", json!({"run":"run_missing"})),
        ("run_abort", json!({"run":"run_missing", "reason":"stop"})),
        ("run_land", json!({"run":"run_missing", "force":false})),
        (
            "run_approve",
            json!({"run":"run_missing", "gate":"g", "seq":1}),
        ),
        (
            "run_pick",
            json!({"run":"run_missing", "gate":"g", "seq":1, "attempt":1}),
        ),
    ] {
        let error = node.call(command, body).await.unwrap_err();
        assert_eq!(error.code, "not_found", "{command}: {error}");
        let error = node.call(command, json!("wrong type")).await.unwrap_err();
        assert_eq!(error.code, "invalid_argument", "{command}");
    }
    assert_eq!(
        node.call(
            "mission_run",
            json!({"mission":"example", "expected_rev":"wrong"})
        )
        .await
        .unwrap_err()
        .code,
        "invalid_argument"
    );
    assert_eq!(
        node.call("run_list", json!({"mission":["a","b"]}))
            .await
            .unwrap_err()
            .code,
        "invalid_argument"
    );
    assert_eq!(
        node.call("mission_run", json!({"issue":"missing-1"}))
            .await
            .unwrap_err()
            .code,
        "invalid_argument"
    );
    let rev = wire["mission"]["rev"].as_i64().unwrap();
    let deleted = node
        .call(
            "mission_delete",
            json!({"mission":"example", "rev":rev, "etag":wire["etag"]}),
        )
        .await
        .unwrap();
    assert_eq!(deleted["deletion"]["slug"], "example");
    let archive = deleted["deletion"]["archive_id"].as_str().unwrap();
    assert_eq!(
        node.call("mission_show", json!({"mission":"example"}))
            .await
            .unwrap_err()
            .code,
        "not_found"
    );
    let archives = node.call("mission_archives", json!({})).await.unwrap();
    let entry = archives["archives"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["archive_id"] == archive)
        .unwrap();
    assert_eq!(entry["slug"], "example");
    assert_eq!(entry["mission"], true);
    let removed = node
        .call(
            "mission_purge",
            json!({"slug":"example", "archive_id":archive}),
        )
        .await
        .unwrap();
    assert_eq!(removed["removed"]["archive_id"], archive);
    assert!(
        node.call("mission_archives", json!({})).await.unwrap()["archives"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let left: Vec<_> = std::fs::read_dir(node.dir().join("missions/missions"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert!(left.is_empty(), "{left:?}");
    assert_eq!(
        node.call("mission_show", json!({"mission":"missing"}))
            .await
            .unwrap_err()
            .code,
        "not_found"
    );
    let other = NodeClient::new(Some(&format!("{}/?node=elsewhere", node.url()))).unwrap();
    let error = other
        .request(
            Method::POST,
            "/api/call/mission_list",
            Some(json!({})),
            None,
        )
        .await
        .unwrap_err();
    assert!(format!("{error:#}").contains("unknown_node"));
    node.stop().await;
}

fn lab(setup: &ahjo_node_kit::Setup) -> anyhow::Result<ahjo_node_kit::Built> {
    ahjo_missions::build_with_executor(setup, Some(Box::new(ahjo_mission_engine::LabExecutor)))
}
const LAB: Listing = Listing {
    build: lab,
    ..ahjo_missions::LISTING
};

#[tokio::test(flavor = "multi_thread")]
async fn run_changes_arrive_on_the_socket_through_completion() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::{connect_async, tungstenite::Message};
    let node = Node::start(&[sessions::LISTING, ahjo_issue::LISTING, LAB], "")
        .await
        .unwrap();
    let folder = node.dir().join("missions/missions");
    std::fs::create_dir_all(&folder).unwrap();
    let mission = include_str!("fixtures/solo-lab.mission.toml")
        .replace("to = \"end_done:in\"", "to = \"gate_review:in\"")
        + "\n[[gate]]\nid = \"gate_review\"\ntitle = \"Review\"\nkinds = [\"human\"]\n\n[[end]]\nid = \"end_rejected\"\ntitle = \"Rejected\"\noutcome = \"rejected\"\n\n[[connection]]\nid = \"edge_pass\"\nfrom = \"gate_review:pass\"\nto = \"end_done:in\"\n\n[[connection]]\nid = \"edge_fail\"\nfrom = \"gate_review:fail\"\nto = \"end_rejected:in\"\n";
    std::fs::write(folder.join("hello.mission.toml"), mission).unwrap();
    let client = NodeClient::new(Some(&format!("{}/", node.url()))).unwrap();
    std::fs::write(
        folder.join("admission-findings.mission.toml"),
        include_str!("fixtures/admission-findings.mission.toml"),
    )
    .unwrap();
    let rejected = node
        .call(
            "mission_run",
            json!({"mission":"admission-findings", "input":"inp", "expected_rev":1}),
        )
        .await
        .unwrap_err();
    assert_eq!(rejected.code, "run_admission_failed");
    let findings = ahjo_missions_contract::admission_findings(&rejected).unwrap();
    assert!(findings.iter().any(|f| f.code.as_str() == "missing_input"));
    assert!(
        findings
            .iter()
            .any(|f| f.code.as_str() == "step_without_seats")
    );
    let remote = include_str!("fixtures/solo-lab.mission.toml")
        .replace("slug = \"hello\"", "slug = \"remote\"")
        .replace("id = \"msn_hello\"", "id = \"msn_remote\"")
        .replace(
            "id = \"fmn_work\"",
            "id = \"fmn_work\"\nenvironment = \"remote/linux\"",
        );
    std::fs::write(folder.join("remote.mission.toml"), remote).unwrap();
    let error = node.call("mission_run", json!({"mission":"remote", "input":"inp_hello", "expected_rev":1, "inputs":{"brief":"hello"}})).await.unwrap_err();
    assert_eq!(error.code, "run_admission_failed");
    let (health, _) = client
        .request(Method::GET, "/api/health", None, None)
        .await
        .unwrap();
    let place = format!(
        "{}/{}",
        health["device"].as_str().unwrap(),
        health["environment"].as_str().unwrap()
    );
    let local = include_str!("fixtures/solo-lab.mission.toml")
        .replace("slug = \"hello\"", "slug = \"local\"")
        .replace("id = \"msn_hello\"", "id = \"msn_local\"")
        .replace(
            "id = \"fmn_work\"",
            &format!("id = \"fmn_work\"\nenvironment = \"{place}\""),
        );
    std::fs::write(folder.join("local.mission.toml"), local).unwrap();
    let local_run = node.call("mission_run", json!({"mission":"local", "input":"inp_hello", "expected_rev":1, "inputs":{"brief":"hello"}})).await.unwrap();
    assert!(local_run["run_id"].is_string());
    let (mut socket, _) = connect_async(node.ws()).await.unwrap();
    socket
        .send(Message::Binary(
            vec![ahjo_missions_contract::WATCH_RUNS, 1].into(),
        ))
        .await
        .unwrap();
    loop {
        let Message::Binary(frame) = socket.next().await.unwrap().unwrap() else {
            continue;
        };
        if frame.first() == Some(&ahjo_missions_contract::RUNS_SNAPSHOT) {
            break;
        }
    }
    let legacy = node.call("mission_run", json!({"mission":"hello", "input":"inp_hello", "expected_rev":1, "inputs":{"brief":"hello"}, "work":"gh:example/project#1"})).await.unwrap_err();
    assert_eq!(legacy.code, "invalid_argument");
    assert!(legacy.message.contains("work"));
    let started = node.call("mission_run", json!({"mission":"hello", "input":"inp_hello", "expected_rev":1, "inputs":{"brief":"hello"}, "issue":"gh:example/project#1"})).await.unwrap();
    let id = started["run_id"].as_str().unwrap();
    let mut answered = false;
    let final_run = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        'stream: loop {
            let Message::Binary(frame) = socket.next().await.unwrap().unwrap() else {
                continue;
            };
            if frame.first() != Some(&ahjo_missions_contract::RUNS_SNAPSHOT) {
                continue;
            }
            let runs: Vec<ahjo_missions_contract::RunProjection> =
                serde_json::from_slice(&frame[1..]).unwrap();
            for run in runs.into_iter().filter(|r| r.run_id == id) {
                if let Some(gate) = run.waiting_gates.first() && !answered {
                    let shown = node.call("run_show", json!({"run":id})).await.unwrap();
                    let request = shown["asks"].as_array().unwrap().iter().find(|ask| ask["gate"] == gate.gate_id).unwrap();
                    assert_eq!(request["request"]["requested_seq"], gate.requested_seq);
                    for (body, code) in [
                        (json!({"run":id,"gate":gate.gate_id,"seq":gate.requested_seq,"relayed_by":"-invalid"}), "invalid_input"),
                        (json!({"run":id,"gate":gate.gate_id,"seq":1}), "conflict"),
                    ] {
                        assert_eq!(node.call("run_approve", body).await.unwrap_err().code, code);
                    }
                    node.call("run_approve", json!({"run":id,"gate":gate.gate_id,"seq":gate.requested_seq,"reason":"  accepted\n","actor":"agent:http"})).await.unwrap();
                    let duplicate = node.call("run_approve", json!({"run":id,"gate":gate.gate_id,"seq":gate.requested_seq})).await.unwrap_err();
                    assert_eq!(duplicate.code, "conflict");
                    answered = true;
                }
                if run.r#final { break 'stream run; }
            }

        }
    })
    .await
    .expect("run reaches a final projection");
    assert!(answered);
    let ledger = ahjo_mission_engine::Ledger::new(
        ahjo_mission_engine::Store::root_in(node.dir()).join(format!("runs/{id}.ndjson")),
        id,
    );
    let events = ledger.read().unwrap();
    assert!(
        events
            .iter()
            .any(|event| event.kind == "human_verdict_recorded"
                && event.data["reason"] == "  accepted\n"
                && event.actor == "agent:http"
                && event.data["decided_by"] == "agent:http")
    );
    let unknown = node
        .call(
            "run_resume",
            json!({"run":id,"grant":true,"completed_dispatch_id":"dsp_1"}),
        )
        .await
        .unwrap_err();
    assert_eq!(unknown.code, "invalid_argument");
    assert_eq!(final_run.issue, "gh:example/project#1");
    let listed = node.call("run_list", json!({})).await.unwrap();
    let _: Vec<ahjo_missions_contract::RunSummary> =
        serde_json::from_value(listed["runs"].clone()).unwrap();
    let shown = node.call("run_show", json!({"run":id,"events":true,"mission":true,"seats":true,"attempts":true,"evidence":true})).await.unwrap();
    assert!(shown["events"].is_array());
    assert!(shown["mission"].is_object());
    assert!(shown["seats"].is_array());
    assert!(shown["attempts"].is_object());
    assert!(shown["evidence"].is_object());
    drop(socket);
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn default_agent_store_is_shared_and_isolated() {
    let areas = &[
        sessions::LISTING,
        ahjo_issue::LISTING,
        ahjo_harness::LISTING,
        LAB,
    ];
    let node = Node::start(areas, "").await.unwrap();
    let other = Node::start(areas, "").await.unwrap();
    let root = ahjo_mission_engine::Store::root_in(node.dir());
    node.call(
        "agent_create",
        json!({"id":"fixture-writer","name":"Reviewer","instructions":"Write a review.","harness":"claude-code","effort":"medium"}),
    ).await.unwrap();
    std::fs::create_dir_all(root.join("missions")).unwrap();
    let mission = include_str!("fixtures/solo-lab.mission.toml")
        .replace("agent = \"builder\"", "agent = \"fixture-writer\"");
    std::fs::write(root.join("missions/hello.mission.toml"), mission).unwrap();
    let report = node
        .call("mission_validate", json!({"mission":"hello"}))
        .await
        .unwrap();
    assert!(
        !report["errors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["code"] == "unavailable_persona"),
        "{report}"
    );
    let started = node.call("mission_run", json!({"mission":"hello", "input":"inp_hello", "expected_rev":1, "inputs":{"brief":"hello"}})).await.unwrap();
    assert!(started["run_id"].is_string());
    assert_eq!(
        other
            .call("agent_show", json!({"agent":"fixture-writer"}))
            .await
            .unwrap_err()
            .code,
        "not_found"
    );
    assert!(
        other
            .call("mission_show", json!({"mission":"hello"}))
            .await
            .is_err()
    );
    node.stop().await;
    other.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn lists_the_repositories_a_step_may_name() {
    let node = Node::start(AREAS, "[missions]\nrepositories = { app = \"/tmp\" }\n")
        .await
        .unwrap();
    let listed = node.call("mission_repositories", json!({})).await.unwrap();
    assert_eq!(listed["repositories"], json!(["app"]));
    node.stop().await;
}
