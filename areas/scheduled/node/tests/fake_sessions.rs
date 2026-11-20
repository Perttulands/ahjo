//! The scheduled area over the testkit's stand-in sessions: no tmux and no
//! terminals, so these run everywhere. It delivers a run into the right
//! session, follows a rename, says why a session refused, and finds the
//! sessions wherever they are listed.

use ahjo_node_kit::{Listing, NodeClient, cli};
use ahjo_testkit::node::Node;
use ahjo_testkit::sessions;
use axum::http::{Method, StatusCode};
use serde_json::{Value, json};

const AREAS: &[Listing] = &[sessions::LISTING, ahjo_scheduled::LISTING];

#[tokio::test]
async fn mcp_lists_derived_scheduled_schemas_and_returns_the_structured_result() {
    let node = Node::start(AREAS, "").await.unwrap();
    let client = NodeClient::new(Some(&node.url())).unwrap();
    let rpc = |method, params| {
        client.request(
            Method::POST,
            "/mcp/scheduled",
            Some(json!({"jsonrpc":"2.0", "id":1, "method":method, "params":params})),
            None,
        )
    };
    let (answer, _) = rpc("tools/list", json!({})).await.unwrap();
    let tools = answer["result"]["tools"].as_array().unwrap();
    let expected = [
        ("list", "tasks"),
        ("show", "task"),
        ("create", "task"),
        ("edit", "task"),
        ("copy", "task"),
        ("pause", "task"),
        ("resume", "task"),
        ("run", "run"),
        ("delete", "deleted"),
    ];
    assert_eq!(tools.len(), expected.len());
    for (verb, output_field) in expected {
        let name = format!("scheduled_{verb}");
        let tool = tools.iter().find(|tool| tool["name"] == name).unwrap();
        assert!(!tool["description"].as_str().unwrap().is_empty());
        let input = &tool["inputSchema"];
        assert_eq!(input["type"], "object");
        assert_eq!(input["additionalProperties"], false);
        assert!(!input["description"].as_str().unwrap().is_empty());
        for property in input["properties"]
            .as_object()
            .into_iter()
            .flat_map(|p| p.values())
        {
            assert!(!property["description"].as_str().unwrap().is_empty());
        }
        let output = &tool["outputSchema"];
        assert_eq!(output["type"], "object");
        assert!(output["properties"].get(output_field).is_some(), "{output}");
        for schema in [input, output] {
            let schema = schema.to_string();
            assert!(!schema.contains(r#""$ref""#), "{schema}");
            assert!(!schema.contains(r#""$defs""#), "{schema}");
        }
    }

    let (answer, _) = rpc("tools/call", json!({"name":"scheduled_list"}))
        .await
        .unwrap();
    let result = &answer["result"];
    assert_eq!(result["isError"], false);
    assert_eq!(result["structuredContent"]["tasks"], json!([]));
    let text: Value = serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(result["structuredContent"], text);
    node.stop().await;
}

#[tokio::test]
async fn scheduled_create_help_and_argument_validation_come_from_the_request_type() {
    let node = Node::start(AREAS, "").await.unwrap();
    let url = node.url();
    let line = |words: &[&str]| {
        words
            .iter()
            .copied()
            .chain(["--node", &url])
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };

    let mut out = vec![];
    let error = cli::run(
        "scheduled",
        &line(&["create", "--session", "x", "--bogus", "1"]),
        &mut out,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(error.contains("unknown flag --bogus"), "{error}");
    let (_, accepted) = error.split_once("accepted fields: ").unwrap();
    let accepted = accepted.lines().next().unwrap();
    for field in [
        "name", "prompt", "sessions", "every", "cron", "timezone", "paused",
    ] {
        assert!(
            accepted
                .split(", ")
                .any(|flag| flag == format!("--{field}")),
            "{error}"
        );
    }
    node.fake_sessions().add("ops");
    let error = node
        .call(
            "scheduled_create",
            json!({
                "name": "nudge", "prompt": "wake up", "sessions": ["ops"], "every": 60, "bogus": 1
            }),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "invalid_argument");
    assert!(
        error.message.contains("unknown arguments: bogus"),
        "{error}"
    );
    assert!(error.message.contains("accepted fields:"), "{error}");
    assert_eq!(
        node.call("scheduled_list", json!({})).await.unwrap()["tasks"],
        json!([])
    );

    assert_eq!(
        cli::run("scheduled", &line(&["create", "--help"]), &mut out)
            .await
            .unwrap(),
        0
    );
    let help = String::from_utf8(out).unwrap();
    for (field, description) in [
        ("<name>", "The name of the scheduled prompt."),
        ("<prompt>", "Pasted into each session"),
        ("<sessions>", "Session names"),
        ("--every", "Fire every this many minutes."),
        ("--cron", "Five cron fields"),
        ("--timezone", "IANA zone for cron."),
        ("--paused", "Start paused."),
    ] {
        assert!(
            help.lines()
                .any(|line| line.trim_start().starts_with(field) && line.contains(description)),
            "{help}"
        );
    }
    assert!(help.contains("Example: ahjo scheduled create"), "{help}");

    let fake = node.fake_sessions();
    fake.add("alpha");
    fake.add("beta");
    let mut out = vec![];
    assert_eq!(
        cli::run(
            "scheduled",
            &line(&[
                "create", "nudge", "wake up", "alpha", "beta", "--every", "60", "--paused",
                "--json",
            ]),
            &mut out
        )
        .await
        .unwrap(),
        0
    );
    let made: Value = serde_json::from_slice(&out).unwrap();
    let task = &made["task"];
    assert_eq!(task["targets"][0]["session_name"], "alpha");
    assert_eq!(task["targets"][1]["session_name"], "beta");
    assert_eq!(task["schedule"]["every_minutes"], 60);
    assert_eq!(task["paused"], true);
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_run_is_delivered_to_the_session_a_task_names() {
    let node = Node::start(AREAS, "").await.unwrap();
    let fake = node.fake_sessions();
    fake.add("alpha");
    fake.add_of("bob", "alpha");
    let made = node
        .call(
            "scheduled_create",
            json!({ "name": "nudge", "prompt": "wake up", "sessions": ["bob/alpha"], "every": 60 }),
        )
        .await
        .unwrap();
    let id = made["task"]["id"].as_str().unwrap();
    node.call("scheduled_run", json!({ "id": id }))
        .await
        .unwrap();
    assert_eq!(
        fake.delivered(),
        [sessions::Delivered {
            session: "alpha".into(),
            text: "wake up".into(),
            enter: true
        }]
    );
    assert_eq!(fake.delivered().len(), 1, "only bob's alpha got it");

    let unknown = node
        .call(
            "scheduled_create",
            json!({ "name": "x", "prompt": "y", "sessions": ["nobody/alpha"], "every": 5 }),
        )
        .await;
    assert!(unknown.unwrap_err().message.contains("nobody"));
    let client = NodeClient::new(Some(&node.url())).unwrap();
    let error = client
        .request(
            Method::POST,
            "/api/call/scheduled_create",
            Some(json!({ "name": "x", "prompt": "y", "sessions": ["nobody/alpha"], "every": 5 })),
            None,
        )
        .await
        .unwrap_err();
    let error = error.downcast_ref::<ahjo_node_kit::NodeError>().unwrap();
    assert_eq!(error.status, StatusCode::BAD_REQUEST);
    assert_eq!(error.code, ahjo_scheduled_contract::TARGET_NOT_FOUND);
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_renamed_session_keeps_its_tasks_and_a_refusal_is_reported() {
    let node = Node::start(AREAS, "").await.unwrap();
    let fake = node.fake_sessions();
    fake.add("alpha");
    let made = node
        .call(
            "scheduled_create",
            json!({ "name": "nudge", "prompt": "hi", "sessions": ["alpha"], "every": 60 }),
        )
        .await
        .unwrap();
    let id = made["task"]["id"].as_str().unwrap().to_owned();

    fake.rename("alpha", "beta");
    let mut shown = json!(null);
    for _ in 0..100 {
        shown = node.call("scheduled_list", json!({})).await.unwrap();
        if shown["tasks"][0]["sessions"] == json!(["beta"]) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_eq!(shown["tasks"][0]["sessions"], json!(["beta"]), "{shown}");

    fake.refuse("beta");
    node.call("scheduled_run", json!({ "id": id }))
        .await
        .unwrap();
    assert!(fake.delivered().is_empty());
    let task = node
        .call("scheduled_show", json!({ "id": id }))
        .await
        .unwrap();
    assert!(
        task["task"]["recent_runs"].to_string().contains("refused"),
        "{task}"
    );
    node.stop().await;
}

/// A task is aimed at a session, not at its name: when that session is
/// closed and another takes its name, nothing is typed into the newcomer.
/// The task is held, says why, and is aimed again by saving it.
#[tokio::test(flavor = "multi_thread")]
async fn a_task_never_types_into_another_session_that_took_its_name() {
    let node = Node::start(AREAS, "").await.unwrap();
    let fake = node.fake_sessions();
    fake.add("ops");
    let made = node
        .call(
            "scheduled_create",
            json!({ "name": "nudge", "prompt": "deploy now", "sessions": ["ops"], "every": 60 }),
        )
        .await
        .unwrap();
    let id = made["task"]["id"].as_str().unwrap().to_owned();

    fake.close("ops");
    fake.add("ops");
    let refused = node.call("scheduled_run", json!({ "id": id })).await;
    assert!(fake.delivered().is_empty(), "typed into another session");
    let why = refused.unwrap_err().message;
    assert!(why.contains("ops is another session"), "{why}");
    let listed = node.call("scheduled_list", json!({})).await.unwrap();
    let held = listed["tasks"][0]["held"].as_str().unwrap();
    assert!(held.contains("Edit the task"), "{listed}");

    node.call("scheduled_edit", json!({ "id": id, "sessions": ["ops"] }))
        .await
        .unwrap();
    let shown = node
        .call("scheduled_show", json!({ "id": id }))
        .await
        .unwrap();
    assert!(shown["task"].get("held").is_none(), "{shown}");
    node.call("scheduled_run", json!({ "id": id }))
        .await
        .unwrap();
    assert_eq!(
        fake.delivered().len(),
        1,
        "saved again, it is the new ops's"
    );
    node.stop().await;
}

/// After the tmux server restarts (a reboot) every session is new and ids
/// begin again: a task follows its sessions' names onto the new server
/// without being held, and is tied to the new session there.
#[tokio::test(flavor = "multi_thread")]
async fn a_task_follows_its_session_onto_a_restarted_server() {
    let node = Node::start(AREAS, "").await.unwrap();
    let fake = node.fake_sessions();
    fake.add("alpha");
    let before = fake.add("ops");
    let made = node
        .call(
            "scheduled_create",
            json!({ "name": "nudge", "prompt": "morning", "sessions": ["ops"], "every": 60 }),
        )
        .await
        .unwrap();
    let id = made["task"]["id"].as_str().unwrap().to_owned();

    fake.restart();
    let after = fake.add("ops");
    assert_ne!(before, after, "the new server gave ops another id");
    node.call("scheduled_run", json!({ "id": id }))
        .await
        .unwrap();
    assert_eq!(fake.delivered().len(), 1, "sent to ops on the new server");
    let shown = node
        .call("scheduled_show", json!({ "id": id }))
        .await
        .unwrap();
    assert!(shown["task"].get("held").is_none(), "{shown}");

    // Tied to the new ops now: another session taking its name on this
    // same server is still not typed into.
    fake.close("ops");
    fake.add("ops");
    let refused = node.call("scheduled_run", json!({ "id": id })).await;
    assert!(
        refused
            .unwrap_err()
            .message
            .contains("ops is another session")
    );
    assert_eq!(fake.delivered().len(), 1);
    node.stop().await;
}

/// The list's order does not matter: scheduled listed first still reaches
/// the sessions; with no area providing them it is refused, saying so.
#[tokio::test(flavor = "multi_thread")]
async fn scheduled_finds_the_sessions_wherever_they_are_listed_and_says_when_none_are() {
    let first: &[Listing] = &[ahjo_scheduled::LISTING, sessions::LISTING];
    let node = Node::start(first, "").await.unwrap();
    node.fake_sessions().add("alpha");
    let made = node
        .call(
            "scheduled_create",
            json!({ "name": "nudge", "prompt": "wake up", "sessions": ["alpha"], "every": 60 }),
        )
        .await
        .unwrap();
    let id = made["task"]["id"].as_str().unwrap();
    node.call("scheduled_run", json!({ "id": id }))
        .await
        .unwrap();
    assert_eq!(node.fake_sessions().delivered().len(), 1);
    node.stop().await;

    let alone: &[Listing] = &[ahjo_scheduled::LISTING];
    let error = match Node::start(alone, "").await {
        Ok(_) => panic!("scheduled started without sessions"),
        Err(error) => format!("{error:#}"),
    };
    assert!(error.contains("starting scheduled"), "{error}");
    assert!(error.contains("no area on this node provides"), "{error}");
}
