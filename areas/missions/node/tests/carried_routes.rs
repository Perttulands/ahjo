//! Authoring regressions carried across the typed command migration.
use ahjo_testkit::{node::Node, sessions};
use serde_json::{Value, json};

async fn start() -> Node {
    Node::start(
        &[
            sessions::LISTING,
            ahjo_issue::LISTING,
            ahjo_missions::LISTING,
        ],
        "",
    )
    .await
    .unwrap()
}
async fn create(node: &Node, args: Value) -> Value {
    let made = node.call("mission_create", args).await.unwrap();
    node.call(
        "mission_show",
        json!({"mission":made["created"],"full":true}),
    )
    .await
    .unwrap()
}
async fn edit(node: &Node, mission: &str, change: Value, etag: &str) -> Value {
    node.call(
        "mission_edit",
        json!({"mission":mission,"change":change,"etag":etag}),
    )
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn commands_restore_a_deleted_step_and_fence_notes_and_layout() {
    let node = start().await;
    let made = create(&node, json!({"slug":"demo","title":"Demo"})).await;
    let made = edit(
        &node,
        "demo",
        json!({"expected_rev":1,"create_step":{"formation":"solo","title":"Work"}}),
        made["etag"].as_str().unwrap(),
    )
    .await;
    let step = made["step"].clone();
    let id = step["id"].as_str().unwrap();
    let deleted = edit(
        &node,
        "demo",
        json!({"expected_rev":2,"delete_step":{"id":id}}),
        made["etag"].as_str().unwrap(),
    )
    .await;
    assert_eq!(deleted["mission"]["step"], json!([]));
    let restored = edit(
        &node,
        "demo",
        json!({"expected_rev":3,"restore_card":{"step":step,"x":112,"y":112}}),
        deleted["etag"].as_str().unwrap(),
    )
    .await;
    assert_eq!(restored["mission"]["step"][0]["id"], id);
    let notes = node
        .call("mission_notes", json!({"mission":"demo"}))
        .await
        .unwrap();
    let note = json!({"mission":"demo","author":"human:test","text":"Keep this","etag":notes["notes"]["etag"]});
    node.call("mission_note", note.clone()).await.unwrap();
    assert_eq!(
        node.call("mission_note", note).await.unwrap_err().code,
        "conflict"
    );
    let layout = node
        .call("layout_show", json!({"mission":"demo"}))
        .await
        .unwrap();
    let position = json!({"mission":"demo","change":{"cards":[{"id":id,"x":400,"y":500}]},"etag":layout["etag"]});
    node.call("layout_edit", position.clone()).await.unwrap();
    assert_eq!(
        node.call("layout_edit", position).await.unwrap_err().code,
        "conflict"
    );
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn derives_named_slug_and_rejects_duplicate_without_clobbering() {
    let node = start().await;
    let body = json!({"slug":"","title":"  Release Plan!  "});
    let first = create(&node, body.clone()).await;
    assert_eq!(first["mission"]["slug"], "release-plan");
    assert_eq!(first["mission"]["title"], "Release Plan!");
    assert!(!first["etag"].as_str().unwrap().is_empty());
    assert_eq!(
        node.call("mission_create", body).await.unwrap_err().code,
        "already_exists"
    );
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn default_gate_is_human_and_a_code_check_requires_code_kind() {
    let node = start().await;
    let made = create(&node, json!({"slug":"gate","title":"Gate"})).await;
    let tag = made["etag"].as_str().unwrap();
    let bad = node.call("mission_edit", json!({"mission":"gate","etag":tag,"change":{"expected_rev":1,"create_gate":{"check":"output_contains","check_version":"1","check_value":"OK"}}})).await.unwrap_err();
    assert_eq!(bad.code, "invalid_code_gate_profile");
    let saved = edit(
        &node,
        "gate",
        json!({"expected_rev":1,"create_gate":{"title":"Signoff"}}),
        tag,
    )
    .await;
    assert_eq!(saved["gate"]["kinds"], json!(["human"]));
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn edits_are_checked_and_pushed_to_subscribers() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::{connect_async, tungstenite::Message};
    let node = start().await;
    let (mut socket, _) = connect_async(node.ws()).await.unwrap();
    socket
        .send(Message::Binary(
            vec![ahjo_missions_contract::WATCH, 1].into(),
        ))
        .await
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(Ok(message)) = socket.next().await {
            if let Message::Binary(frame) = message
                && frame.first() == Some(&ahjo_missions_contract::SNAPSHOT)
            {
                break;
            }
        }
    })
    .await
    .unwrap();
    let made = create(&node, json!({"slug":"test","title":"Test"})).await;
    let tag = made["etag"].as_str().unwrap();
    let same = edit(&node, "test", json!({"expected_rev":1,"title":"Test"}), tag).await;
    assert_eq!(same["etag"], tag);
    let error = node.call("mission_edit", json!({"mission":"test","etag":tag,"change":{"expected_rev":1,"title":"X","create_end":{"title":"End"}}})).await.unwrap_err();
    assert_eq!(error.code, "invalid_argument");
    let update = json!({"mission":"test","etag":tag,"change":{"expected_rev":1,"title":"Renamed"}});
    node.call("mission_edit", update.clone()).await.unwrap();
    assert_eq!(
        node.call("mission_edit", update).await.unwrap_err().code,
        "conflict"
    );
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while let Some(Ok(message)) = socket.next().await {
            if let Message::Binary(frame) = message
                && frame.first() == Some(&ahjo_missions_contract::SNAPSHOT)
            {
                let data: Value = serde_json::from_slice(&frame[1..]).unwrap();
                if data.to_string().contains("Renamed") {
                    return;
                }
            }
        }
        panic!("subscription ended before the edit");
    })
    .await
    .expect("subscriber observes edit");
    drop(socket);
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn command_patch_uses_the_same_schema_and_stages_a_batch() {
    let node = start().await;
    let made = create(&node, json!({"slug":"batch","title":"Original"})).await;
    let tag = made["etag"].as_str().unwrap();
    let error = node.call("mission_edit", json!({"mission":"batch","etag":tag,"change":{"rev":1,"changes":[{"title":"Changed"},{"create_end":{"bogus":42}}]}})).await.unwrap_err();
    assert_eq!(error.code, "invalid_argument");
    assert!(
        error
            .message
            .contains("change at index 1: create_end: unknown field bogus"),
        "{error}"
    );
    let after = edit(&node, "batch", json!({"rev":1,"updated_by":"human:operator","changes":[{"create_step":{"title":"Work"}},{"title":"Changed"}]}), tag).await;
    assert_eq!(after["mission"]["rev"], 2);
    assert_eq!(after["mission"]["title"], "Changed");
    assert_eq!(after["mission"]["step"][0]["title"], "Work");
    assert_eq!(after["layout"]["mission_rev"], 2);
    assert_eq!(after["mission"]["updated_by"], "human:operator");
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn retry_with_original_request_id_survives_a_removed_step() {
    fn lab(setup: &ahjo_node_kit::Setup) -> anyhow::Result<ahjo_node_kit::Built> {
        ahjo_missions::build_with_executor(setup, Some(Box::new(ahjo_mission_engine::LabExecutor)))
    }
    const LAB: ahjo_node_kit::Listing = ahjo_node_kit::Listing {
        build: lab,
        ..ahjo_missions::LISTING
    };
    let node = Node::start(&[sessions::LISTING, ahjo_issue::LISTING, LAB], "")
        .await
        .unwrap();
    let root = node.dir().join("missions/missions");
    std::fs::create_dir_all(&root).unwrap();
    let fixture =
        include_str!("fixtures/solo-lab.mission.toml").replace("agent = \"builder\"\n", "");
    std::fs::write(root.join("hello.mission.toml"), fixture).unwrap();
    let shown = node
        .call("mission_show", json!({"mission":"hello","full":true}))
        .await
        .unwrap();
    let start = json!({"mission":"hello","from":"fmn_work","inputs":{"brief":"go"},"request_id":"retry-original-step","expected_rev":1,"expected_etag":shown["etag"]});
    let first = node.call("mission_run", start.clone()).await.unwrap();
    let updated = edit(
        &node,
        "hello",
        json!({"expected_rev":1,"delete_step":{"id":"fmn_work"}}),
        shown["etag"].as_str().unwrap(),
    )
    .await;
    assert!(updated["mission"]["step"].as_array().unwrap().is_empty());
    let retry = node.call("mission_run", start).await.unwrap();
    assert_eq!(retry["run_id"], first["run_id"]);
    node.stop().await;
}
