//! The Harness commands and channel on an isolated node and scratch home.
use std::time::Duration;

use ahjo_core::Error;
use ahjo_harness_contract::{FAILED, REPORT, SYNC, SYNCED, WATCH};
use ahjo_node_kit::{NodeClient, NodeError, cli};
use ahjo_testkit::{fresh_dir, node::Node, sessions};
use axum::http::Method;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

#[tokio::test(flavor = "multi_thread")]
async fn commands_and_channel_work_over_the_node() {
    let home = fresh_dir("ahjo-harness-node-home");
    let skill = home.join(".agents/skills/example/SKILL.md");
    std::fs::create_dir_all(skill.parent().unwrap()).unwrap();
    std::fs::write(
        &skill,
        "---\nname: example\ndescription: A fixture skill.\n---\n",
    )
    .unwrap();
    let config = format!("[harness]\nhome = {:?}\npath = []\n", home);
    let node = Node::start(&[sessions::LISTING, ahjo_harness::LISTING], &config)
        .await
        .unwrap();
    let client = NodeClient::new(Some(&node.url())).unwrap();

    for path in [
        "/api/harness/shape",
        "/api/harness/file",
        "/api/harness/hook",
        "/api/harness/setting",
    ] {
        let error = client
            .request(Method::GET, path, None, None)
            .await
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<NodeError>().unwrap().status,
            404,
            "{path}"
        );
    }

    let (health, _) = client
        .request(Method::GET, "/api/health", None, None)
        .await
        .unwrap();
    assert_eq!(health["status"], "ok");
    assert!(
        !health["off"]
            .as_array()
            .unwrap()
            .iter()
            .any(|area| area == "harness")
    );
    let request = json!({"jsonrpc":"2.0", "id":1, "method":"tools/list", "params":{}});
    let (list, _) = client
        .request(Method::POST, "/mcp/harness", Some(request.clone()), None)
        .await
        .unwrap();
    let tools = list["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    let old_path = client
        .request(Method::POST, "/mcp/agents", Some(request), None)
        .await
        .unwrap_err();
    assert_eq!(old_path.downcast_ref::<NodeError>().unwrap().status, 404);
    for tool in tools.iter().filter(|tool| {
        let name = tool["name"].as_str().unwrap();
        name.starts_with("harness_") || name.starts_with("agent_")
    }) {
        for key in ["inputSchema", "outputSchema"] {
            assert_eq!(tool[key]["type"], "object", "{tool}");
            let schema = tool[key].to_string();
            assert!(!schema.contains(r#""$ref""#), "{schema}");
        }
    }
    let bad = node
        .call("harness_sync", json!({"unknown": "value"}))
        .await
        .unwrap_err();
    assert_eq!(bad.code, "invalid_argument");
    assert!(
        bad.message.contains("unknown") && bad.message.contains("checkout"),
        "{bad}"
    );

    assert_eq!(
        names,
        [
            "harness_show",
            "harness_sync",
            "harness_link",
            "harness_show_shape",
            "harness_read",
            "harness_write",
            "harness_create_hook",
            "harness_edit_hook",
            "harness_delete_hook",
            "harness_switch_hook",
            "harness_edit_setting",
            "agent_list",
            "agent_show",
            "agent_create",
            "agent_edit",
            "agent_delete"
        ]
    );
    assert_eq!(ahjo_harness::LISTING.commands, names);

    let args = vec!["--node".into(), node.url()];
    let mut output = Vec::new();
    assert_eq!(cli::run("harness", &args, &mut output).await.unwrap(), 0);
    let output = String::from_utf8(output).unwrap();
    assert!(
        output.contains("show") && output.contains("sync") && output.contains("link"),
        "{output}"
    );
    let report = node.call("harness_show", json!({})).await.unwrap();
    assert_eq!(report["node"], node.id());
    assert_eq!(report["skills"][0]["name"], "example");
    let error = node
        .call("harness_sync", json!({"checkout":"~/not-a-checkout"}))
        .await
        .unwrap_err();
    assert_eq!(error.code, FAILED);
    assert!(error.message.contains("not a checkout"));
    // It says where to find the ones there are.
    assert!(
        error.message.contains("ahjo harness show"),
        "{}",
        error.message
    );
    let error = node
        .call("harness_link", json!({"checkout":"~/not-a-checkout"}))
        .await
        .unwrap_err();
    assert_eq!(error.code, FAILED);
    assert!(error.message.contains("not a checkout"));
    let remote = node
        .call("harness_show", json!({"node":"another-node"}))
        .await
        .unwrap_err();
    assert_eq!(remote.code, "unknown_node");

    let (mut ws, _) = tokio_tungstenite::connect_async(node.ws()).await.unwrap();
    ws.send(Message::Binary(vec![WATCH, 1].into()))
        .await
        .unwrap();
    let frame = next_kind(&mut ws, REPORT).await;
    let report: Value = serde_json::from_slice(&frame[1..]).unwrap();
    assert_eq!(report["skills"][0]["name"], "example");
    ws.send(Message::Binary(
        [&[SYNC][..], &42_u32.to_le_bytes(), b"~/not-a-checkout"]
            .concat()
            .into(),
    ))
    .await
    .unwrap();
    let frame = next_kind(&mut ws, SYNCED).await;
    assert_eq!(&frame[1..6], &[42, 0, 0, 0, 0]);
    let error: Error = serde_json::from_slice(&frame[6..]).unwrap();
    assert_eq!(error.code, FAILED);
    assert!(error.message.contains("not a checkout"));
    ws.send(Message::Binary(vec![WATCH, 0].into()))
        .await
        .unwrap();
    drop(ws);
    node.stop().await;
    std::fs::remove_dir_all(home).unwrap();
}

async fn next_kind(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    kind: u8,
) -> Vec<u8> {
    tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(message) = ws.next().await {
            if let Message::Binary(bytes) = message.unwrap()
                && bytes.first() == Some(&kind)
            {
                return bytes.to_vec();
            }
        }
        panic!("socket closed before frame {kind:#x}")
    })
    .await
    .expect("timed out waiting for frame")
}

#[tokio::test(flavor = "multi_thread")]
async fn agent_commands_keep_versions_and_usage_and_archive() {
    let root = fresh_dir("ahjo-agent-node");
    let config = format!("[harness]\nhome = {:?}\npath = []\n", root);
    let node = Node::start(&[sessions::LISTING, ahjo_harness::LISTING], &config)
        .await
        .unwrap();
    let created = node
        .call(
            "agent_create",
            json!({
                "id":"writer", "name":"Writer", "instructions":"Writes",
                "harness":"claude-code", "model":"", "effort":"medium"
            }),
        )
        .await
        .unwrap();
    assert_eq!(created["agent"]["name"], "Writer");
    let etag = created["etag"].as_str().unwrap().to_owned();
    assert!(!etag.is_empty());

    let listed = node.call("agent_list", json!({})).await.unwrap();
    assert!(
        listed["agents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|agent| agent["id"] == "writer")
    );
    let catalog = listed["harnesses"].as_array().unwrap();
    let codex = catalog
        .iter()
        .find(|item| item["id"] == "openai-codex")
        .unwrap();
    assert!(
        codex["efforts"]
            .as_array()
            .unwrap()
            .contains(&json!("ultra"))
    );
    assert!(listed["effort_policy"].is_array());
    let changed = node
        .call(
            "agent_edit",
            json!({
                "agent":"writer", "etag":etag, "instructions":"Revised",
                "notes":[{"at":"2026-11-20T12:00:00Z","by":"human:ui","text":"Keep context"}]
            }),
        )
        .await
        .unwrap();
    assert_eq!(changed["agent"]["instructions"], "Revised");
    assert_eq!(changed["agent"]["notes"][0]["text"], "Keep context");
    let conflict = node
        .call(
            "agent_edit",
            json!({
                "agent":"writer", "etag":etag, "instructions":"stale"
            }),
        )
        .await
        .unwrap_err();
    assert_eq!(conflict.code, "conflict");

    let state = ahjo_mission_engine::Store::root_in(node.dir());
    let mission = state.join("missions/draft.mission.toml");
    std::fs::create_dir_all(mission.parent().unwrap()).unwrap();
    std::fs::write(
        &mission,
        r#"schema = 3
id = "mis_draft"
slug = "draft"
title = "Draft"
rev = 1

[[step]]
id = "step_write"
formation = "solo"
title = "Write"

[[step.seat]]
id = "seat_writer"
label = "Writer"
agent = "writer"
"#,
    )
    .unwrap();
    let shown = node
        .call("agent_show", json!({"agent":"writer"}))
        .await
        .unwrap();
    assert_eq!(shown["etag"], changed["etag"]);
    assert_eq!(shown["usage"]["complete"], true);
    assert_eq!(
        shown["usage"]["usage"],
        json!([{
            "mission_id":"mis_draft", "mission_slug":"draft", "mission_title":"Draft",
            "step_id":"step_write", "step_title":"Write",
            "seat_id":"seat_writer", "seat_label":"Writer"
        }])
    );

    let stale_delete = node
        .call("agent_delete", json!({"agent":"writer","etag":etag}))
        .await
        .unwrap_err();
    assert_eq!(stale_delete.code, "conflict");
    let deleted = node
        .call(
            "agent_delete",
            json!({"agent":"writer","etag":shown["etag"]}),
        )
        .await
        .unwrap();
    assert_eq!(deleted["agent"], "writer");
    assert!(!state.join("agents/writer.toml").exists());
    assert!(state.join("agents/archive/writer.toml").exists());
    assert_eq!(
        node.call("agent_show", json!({"agent":"writer"}))
            .await
            .unwrap_err()
            .code,
        "not_found"
    );
    std::fs::rename(
        state.join("agents/archive/writer.toml"),
        state.join("agents/writer.toml"),
    )
    .unwrap();
    assert_eq!(
        node.call("agent_show", json!({"agent":"writer"}))
            .await
            .unwrap()["agent"]["id"],
        "writer"
    );

    let bad = node
        .call(
            "agent_create",
            json!({
                "id":"editor", "name":"Editor", "instructions":"Edit",
                "harness":"claude-code", "effort":"high", "unknown":true
            }),
        )
        .await
        .unwrap_err();
    assert_eq!(bad.code, "invalid_argument");
    let client = NodeClient::new(Some(&node.url())).unwrap();
    let old = client
        .request(Method::GET, "/api/roles", None, None)
        .await
        .unwrap_err();
    assert_eq!(old.downcast_ref::<NodeError>().unwrap().status, 404);
    let mut help = Vec::new();
    let args = vec!["--node".into(), format!("{}/", node.url())];
    assert_eq!(cli::run("agent", &args, &mut help).await.unwrap(), 0);
    let help = String::from_utf8(help).unwrap();
    for verb in ["list", "show", "create", "edit", "delete"] {
        assert!(help.contains(verb), "{help}");
    }
    let index = cli::index(Some(&format!("{}/", node.url()))).await.unwrap();
    assert!(
        !index
            .lines()
            .any(|line| line.split_whitespace().next() == Some("role")),
        "{index}"
    );
    node.stop().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn startup_upgrades_roles_and_characters_before_agent_list() {
    use ahjo_node_kit::{Config, Options};

    let dir = fresh_dir("ahjo-agent-upgrade-node");
    let roles = dir.join("missions/roles");
    std::fs::create_dir_all(&roles).unwrap();
    std::fs::write(
        roles.join("writer.toml"),
        r#"schema = 1
[card]
id = "writer"
display_name = "Writer"
kind = "specialist"
summary = "Writes careful drafts"
status = "active"
[[note]]
ts = "2026-11-20T12:00:00Z"
actor = "human:ui"
text = "Keep context"
"#,
    )
    .unwrap();
    std::fs::write(
        roles.join("reviewer.toml"),
        r#"schema = 1
[card]
id = "reviewer"
display_name = "Reviewer"
kind = "reviewer"
summary = "Retired reviewer"
status = "retired"
"#,
    )
    .unwrap();
    let characters = r#"{"schema":2,"characters":[{"id":"night-writer","name":"Night writer","agent_id":"writer","harness":"openai-codex","model":"","effort":"ultra"}]}"#;
    std::fs::write(roles.join("characters.json"), characters).unwrap();

    let config = Config::parse("", dir.clone()).unwrap();
    let running = ahjo_node_kit::start(
        Options {
            dir: dir.clone(),
            config,
            listen: Some("127.0.0.1:0".into()),
        },
        &[sessions::LISTING, ahjo_harness::LISTING],
    )
    .await
    .unwrap();
    let url = format!("http://{}/", running.address);
    let client = NodeClient::new(Some(&url)).unwrap();
    let (listed, _) = client
        .request(Method::POST, "/api/call/agent_list", Some(json!({})), None)
        .await
        .unwrap();
    let agents = listed["agents"].as_array().unwrap();
    assert_eq!(agents.len(), 8, "{listed}");
    for id in [
        "scout",
        "planner",
        "builder",
        "judge",
        "orchestrator",
        "debugger",
        "writer",
        "night-writer",
    ] {
        assert!(
            agents.iter().any(|agent| agent["id"] == id),
            "{id}: {listed}"
        );
    }
    assert!(!agents.iter().any(|agent| agent["id"] == "reviewer"));
    let writer = agents.iter().find(|agent| agent["id"] == "writer").unwrap();
    assert!(
        writer["instructions"]
            .as_str()
            .unwrap()
            .contains("Writes careful drafts")
    );
    assert_eq!(writer["notes"][0]["text"], "Keep context");
    let night = agents
        .iter()
        .find(|agent| agent["id"] == "night-writer")
        .unwrap();
    assert_eq!(night["harness"], "openai-codex");
    assert_eq!(night["effort"], "ultra");
    assert!(
        night["instructions"]
            .as_str()
            .unwrap()
            .contains("Writes careful drafts")
    );
    assert!(dir.join("missions/agents/archive/reviewer.toml").exists());
    assert!(!roles.join("characters.json").exists());
    assert_eq!(
        std::fs::read_to_string(roles.join("characters.json.bak")).unwrap(),
        characters
    );
    let mut output = Vec::new();
    let args = vec!["list".into(), "--node".into(), url, "--json".into()];
    assert_eq!(cli::run("agent", &args, &mut output).await.unwrap(), 0);
    let from_cli: Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(from_cli["agents"], listed["agents"]);
    running.stop().await.unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn watching_does_not_fetch_and_sync_failures_survive_refresh_and_other_watchers() {
    use std::os::unix::fs::symlink;
    let home = fresh_dir("agents-no-fetch-watch");
    let bin = home.join("bin");
    let links = home.join(".agents/skills");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::create_dir_all(&links).unwrap();
    let git = bin.join("git");
    ahjo_testkit::tmux::write_script(
        &git,
        "#!/bin/sh\nif [ \"$3\" = fetch ]; then printf '%s\\n' \"$2\" >> \"$HOME/fetch-log\"; case \"$2\" in */first) if [ -f \"$HOME/block-fetch\" ]; then : > \"$HOME/fetch-started\"; while [ ! -f \"$HOME/continue-fetch\" ]; do :; done; fi;; esac; echo 'fatal: credentials unavailable' >&2; exit 1; fi\nexit 0\n",
    );
    for name in ["first", "second"] {
        let checkout = home.join(name);
        std::fs::create_dir_all(checkout.join(".git")).unwrap();
        let skill = checkout.join("skills").join(name);
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(
            skill.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: Fixture\n---\n"),
        )
        .unwrap();
        symlink(&skill, links.join(name)).unwrap();
    }
    let config = format!("[harness]\nhome = {:?}\npath = [{:?}]\n", home, bin);
    let node = Node::start(&[sessions::LISTING, ahjo_harness::LISTING], &config)
        .await
        .unwrap();
    let (mut first, _) = tokio_tungstenite::connect_async(node.ws()).await.unwrap();
    let (mut second, _) = tokio_tungstenite::connect_async(node.ws()).await.unwrap();
    for ws in [&mut first, &mut second] {
        ws.send(Message::Binary(vec![WATCH, 1].into()))
            .await
            .unwrap();
        let report: Value = serde_json::from_slice(&next_kind(ws, REPORT).await[1..]).unwrap();
        assert_eq!(report["checkouts"].as_array().unwrap().len(), 2);
        assert!(
            report["checkouts"]
                .as_array()
                .unwrap()
                .iter()
                .all(|item| item["fetch"] == "local")
        );
    }
    let show = node.call("harness_show", json!({})).await.unwrap();
    assert!(
        show["checkouts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["fetch"] == "local")
    );
    assert!(
        !home.join("fetch-log").exists(),
        "reading or watching must not fetch"
    );
    first
        .send(Message::Binary(
            [&[SYNC][..], &17_u32.to_le_bytes(), b"~/first"]
                .concat()
                .into(),
        ))
        .await
        .unwrap();
    let answer = next_kind(&mut first, SYNCED).await;
    assert_eq!(answer[5], 0);
    let error: Error = serde_json::from_slice(&answer[6..]).unwrap();
    assert_eq!(error.code, "harness_fetch_failed");
    assert!(error.message.contains("credentials unavailable"));
    let other = next_report_with_failure(&mut second, "first", 1).await;
    assert_eq!(
        other["checkouts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == "first")
            .unwrap()["fetch_error"],
        "fatal: credentials unavailable"
    );
    let error = node
        .call("harness_sync", json!({"checkout":"~/second"}))
        .await
        .unwrap_err();
    assert_eq!(error.code, "harness_fetch_failed");
    let both = next_report_with_failure(&mut second, "second", 2).await;
    assert_eq!(
        both["checkouts"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["fetch"] == "failed")
            .count(),
        2
    );
    first
        .send(Message::Binary(vec![WATCH, 0].into()))
        .await
        .unwrap();
    first
        .send(Message::Binary(vec![WATCH, 1].into()))
        .await
        .unwrap();
    let refreshed = next_report_with_failure(&mut first, "second", 2).await;
    assert_eq!(
        refreshed["checkouts"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["fetch"] == "failed")
            .count(),
        2
    );
    // A second Sync can arrive while a first is in flight, and the first
    // watcher can disconnect without losing either checkout's result.
    std::fs::write(home.join("block-fetch"), "").unwrap();
    first
        .send(Message::Binary(
            [&[SYNC][..], &18_u32.to_le_bytes(), b"~/first"]
                .concat()
                .into(),
        ))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while !home.join("fetch-started").exists() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("first fetch started");
    second
        .send(Message::Binary(
            [&[SYNC][..], &19_u32.to_le_bytes(), b"~/second"]
                .concat()
                .into(),
        ))
        .await
        .unwrap();
    drop(first);
    std::fs::write(home.join("continue-fetch"), "").unwrap();
    let answer = next_kind(&mut second, SYNCED).await;
    assert_eq!(&answer[1..5], &19_u32.to_le_bytes());
    assert_eq!(answer[5], 0);
    let after_disconnect = node.call("harness_show", json!({})).await.unwrap();
    assert_eq!(
        after_disconnect["checkouts"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["fetch"] == "failed")
            .count(),
        2
    );
    let log = std::fs::read_to_string(home.join("fetch-log")).unwrap();
    assert_eq!(
        log.lines().count(),
        4,
        "only explicit Sync commands fetch: {log}"
    );
    drop(second);
    node.stop().await;
    std::fs::remove_dir_all(home).unwrap();
}

#[cfg(unix)]
async fn next_report_with_failure(
    ws: &mut tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    name: &str,
    count: usize,
) -> Value {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let frame = next_kind(ws, REPORT).await;
            let report: Value = serde_json::from_slice(&frame[1..]).unwrap();
            let checkouts = report["checkouts"].as_array().unwrap();
            if checkouts
                .iter()
                .any(|c| c["name"] == name && c["fetch"] == "failed")
                && checkouts.iter().filter(|c| c["fetch"] == "failed").count() == count
            {
                return report;
            }
        }
    })
    .await
    .expect("report with retained sync failures")
}
