//! A refresh must answer from the current store, even after an earlier read:
//! a write moves the store's marker, and the next refresh reads it.
#![cfg(unix)]

use ahjo_node_kit::{Built, Listing, Setup};
use ahjo_testkit::node::Node;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{sync::OnceLock, time::Duration};
use tokio_tungstenite::tungstenite::Message;

static BD: OnceLock<String> = OnceLock::new();
fn build(setup: &Setup) -> anyhow::Result<Built> {
    ahjo_work::build_with(
        setup,
        ahjo_work::Bd::new(BD.get().unwrap()),
        ahjo_work::Gh::default(),
    )
}
const WORK: Listing = Listing {
    build,
    ..ahjo_work::LISTING
};

fn status_file(path: &std::path::Path, status: &str) {
    std::fs::write(path, format!(r#"[{{"id":"fixture-1","title":"Task","status":"{status}","created_at":"2026-11-01T00:00:00Z","updated_at":"2026-11-01T00:00:00Z"}}]"#)).unwrap();
}
fn status(value: &Value) -> &str {
    value["projects"][0]["items"][0]["status"].as_str().unwrap()
}
async fn next_status(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    expected: &str,
) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let data = ws.next().await.unwrap().unwrap().into_data();
            if data.first() == Some(&ahjo_work_contract::SNAPSHOT) {
                let value: Value = serde_json::from_slice(&data[1..]).unwrap();
                if value["projects"][0]["items"][0]["status"] == expected {
                    return;
                }
            }
        }
    })
    .await
    .expect("the refreshed Work snapshot");
}

#[tokio::test(flavor = "multi_thread")]
async fn cli_list_and_page_refresh_read_the_changed_store() {
    let dir = ahjo_testkit::fresh_dir("ahjo-work-refresh");
    std::fs::create_dir_all(dir.join(".beads/embeddeddolt/fixture/.dolt/noms")).unwrap();
    let manifest = dir.join(".beads/embeddeddolt/fixture/.dolt/noms/manifest");
    std::fs::write(&manifest, "first").unwrap();
    let items = dir.join("items.json");
    status_file(&items, "open");
    let script = dir.join("fake-bd");
    ahjo_testkit::tmux::write_script(&script, "#!/bin/sh\ncat items.json\n");
    BD.set(script.display().to_string()).unwrap();
    let config = format!(
        "[work]\nprojects = [{:?}]\n",
        format!("beads {}", dir.display())
    );
    let node = Node::start(&[WORK], &config).await.unwrap();
    let (mut ws, _) = tokio_tungstenite::connect_async(node.ws()).await.unwrap();
    ws.next().await.unwrap().unwrap();
    ws.send(Message::Binary(vec![ahjo_work_contract::WATCH, 1].into()))
        .await
        .unwrap();
    next_status(&mut ws, "open").await;
    assert_eq!(
        status(&node.call("work_list", json!({"all":true})).await.unwrap()),
        "open"
    );

    // A write through bd moves the store's marker; the next list reads it.
    status_file(&items, "closed");
    std::fs::write(&manifest, "after closing").unwrap();
    assert_eq!(
        status(&node.call("work_list", json!({"all":true})).await.unwrap()),
        "closed"
    );
    next_status(&mut ws, "closed").await;
    status_file(&items, "in_progress");
    std::fs::write(&manifest, "after starting").unwrap();
    ws.send(Message::Binary(vec![ahjo_work_contract::WATCH, 2].into()))
        .await
        .unwrap();
    next_status(&mut ws, "in_progress").await;
    drop(ws);
    node.stop().await;
    std::fs::remove_dir_all(dir).unwrap();
}
