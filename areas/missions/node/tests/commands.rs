use ahjo_node_kit::{Built, Listing, NodeClient, Setup};
use ahjo_testkit::{node::Node, sessions};
use serde_json::json;

fn lab(setup: &Setup) -> anyhow::Result<Built> {
    ahjo_missions::build_with_executor(setup, Some(Box::new(ahjo_mission_engine::LabExecutor)))
}
const LAB: Listing = Listing {
    build: lab,
    ..ahjo_missions::LISTING
};
const EXPECTED: &[&str] = &[
    "mission_list",
    "mission_archives",
    "mission_purge",
    "mission_show",
    "mission_validate",
    "mission_create",
    "mission_examples",
    "mission_export",
    "mission_apply",
    "mission_edit",
    "mission_notes",
    "mission_note",
    "mission_reply",
    "mission_resolve",
    "layout_show",
    "layout_edit",
    "mission_run",
    "mission_arrange",
    "mission_delete",
    "run_list",
    "run_show",
    "run_wait",
    "run_approve",
    "run_reject",
    "run_pick",
    "run_resume",
    "run_abort",
    "run_land",
    "run_delete",
];

#[tokio::test(flavor = "multi_thread")]
async fn cli_updates_step_environment_and_home_clears_it() {
    let node = Node::start(&[sessions::LISTING, ahjo_issue::LISTING, LAB], "")
        .await
        .unwrap();
    let folder = node.dir().join("missions/missions");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("hello.mission.toml"),
        include_str!("fixtures/solo-lab.mission.toml"),
    )
    .unwrap();

    for (environment, expected) in [("remote/linux", "remote/linux"), ("home", "")] {
        let change = json!({"update_step":{"id":"fmn_work","environment":environment}});
        let args = vec![
            "edit".into(),
            "hello".into(),
            "--change".into(),
            change.to_string(),
            "--node".into(),
            format!("{}/", node.url()),
            "--json".into(),
        ];
        let mut output = Vec::new();
        assert_eq!(
            ahjo_node_kit::cli::run("mission", &args, &mut output)
                .await
                .unwrap(),
            0
        );
        let response: serde_json::Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(
            response["mission"]["step"][0]["environment"]
                .as_str()
                .unwrap_or(""),
            expected
        );
    }
    let error = node
        .call(
            "mission_edit",
            json!({"mission":"hello","change":{
                "update_step":{"id":"fmn_work","device":"remote/linux"}
            }}),
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "invalid_argument");
    assert!(error.message.contains("unknown field device"), "{error}");
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn run_wait_and_show_explain_a_code_gate_send_back() {
    let node = Node::start(&[sessions::LISTING, ahjo_issue::LISTING, LAB], "")
        .await
        .unwrap();
    let mission = include_str!("fixtures/solo-lab.mission.toml")
        .replace("to = \"end_done:in\"", "to = \"gate_review:in\"")
        + r#"
[[gate]]
id = "gate_review"
title = "Noise check"
kinds = ["code"]
check = "output_contains"
check_version = "1"
check_value = "UI-CHECK: CLEAN"

[[end]]
id = "end_rejected"
title = "Sent back"
outcome = "rejected"

[[connection]]
id = "edge_pass"
from = "gate_review:pass"
to = "end_done:in"

[[connection]]
id = "edge_fail"
from = "gate_review:fail"
to = "end_rejected:in"
"#;
    let folder = node.dir().join("missions/missions");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("hello.mission.toml"), mission).unwrap();
    let finding = "Remove the duplicate heading.\nKeep the Save action.";
    let started = node
        .call(
            "mission_run",
            json!({"mission":"hello","inputs":{"brief":finding}}),
        )
        .await
        .unwrap();
    let run = started["run_id"].as_str().unwrap();
    let waited = node
        .call("run_wait", json!({"run":run,"until":"final","timeout":5}))
        .await
        .unwrap();
    let shown = node.call("run_show", json!({"run":run})).await.unwrap();
    assert_eq!(waited["outcome"], "final");
    assert_eq!(waited["status"], "sent_back");
    assert_eq!(shown["state"]["status"], "sent_back");
    assert_eq!(shown["state"]["final"], true);
    assert_eq!(shown["sent_back"], waited["sent_back"]);
    assert_eq!(shown["why"], waited["why"]);
    let sent_back = &shown["sent_back"];
    assert_eq!(sent_back["gate_id"], "gate_review");
    assert_eq!(sent_back["gate_title"], "Noise check");
    assert_eq!(sent_back["end_id"], "end_rejected");
    assert!(
        sent_back["reason"]
            .as_str()
            .unwrap()
            .contains("does not contain")
    );
    assert!(
        sent_back["findings"].as_str().unwrap().contains(finding),
        "{sent_back}"
    );
    for verb in ["wait", "show"] {
        let args = [verb.to_owned(), run.to_owned(), "--node".into(), node.url()];
        let mut output = Vec::new();
        let exit = ahjo_node_kit::cli::run("run", &args, &mut output)
            .await
            .unwrap();
        assert_eq!(exit, 0);
        let output = String::from_utf8(output).unwrap();
        assert!(
            output.contains("Sent back by Noise check (gate_review)"),
            "{output}"
        );
        for line in finding.lines() {
            assert!(output.contains(line), "{output}");
        }
    }
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn command_list_and_coded_failures() {
    assert_eq!(ahjo_missions::LISTING.commands, EXPECTED);
    let agent_commands: Vec<_> = ahjo_harness::LISTING
        .commands
        .iter()
        .copied()
        .filter(|name| name.starts_with("agent_"))
        .collect();
    assert_eq!(
        agent_commands,
        [
            "agent_list",
            "agent_show",
            "agent_create",
            "agent_edit",
            "agent_delete"
        ]
    );
    let node = Node::start(&[sessions::LISTING, ahjo_issue::LISTING, LAB], "")
        .await
        .unwrap();
    for &command in EXPECTED {
        let args = match command {
            "mission_list" | "mission_archives" | "mission_examples" => {
                json!({"unexpected":true})
            }
            "run_list" => json!({"mission":9}),
            _ => json!({}),
        };
        let error = node.call(command, args).await.unwrap_err();
        let code = if command == "mission_export" {
            "bad_request"
        } else {
            "invalid_argument"
        };
        assert_eq!(error.code, code, "{command}: {error}");
    }
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mission_command_schemas_and_cli_help_match_the_document_surface() {
    let node = Node::start(&[sessions::LISTING, ahjo_issue::LISTING, LAB], "")
        .await
        .unwrap();
    let client = NodeClient::new(Some(&node.url())).unwrap();
    let (answer, _) = client
        .request(
            axum::http::Method::POST,
            "/mcp/missions",
            Some(json!({"jsonrpc":"2.0","id":1,"method":"tools/list"})),
            None,
        )
        .await
        .unwrap();
    let tools = answer["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), EXPECTED.len());
    for name in EXPECTED {
        let tool = tools.iter().find(|tool| tool["name"] == *name).unwrap();
        assert_eq!(tool["inputSchema"]["additionalProperties"], false, "{name}");
        assert!(tool["outputSchema"].is_object(), "{tool}");
        let error = node
            .call(name, json!({"unexpected": true}))
            .await
            .unwrap_err();
        assert_eq!(error.code, "invalid_argument", "{name}: {error}");
        assert!(
            error.message.contains("unknown arguments: unexpected"),
            "{error}"
        );
    }

    fn has_field_enum(schema: &serde_json::Value, field: &str, values: &[&str]) -> bool {
        if let Some(property) = schema["properties"].get(field) {
            let values = json!(values);
            if property["enum"] == values || property["items"]["enum"] == values {
                return true;
            }
        }
        match schema {
            serde_json::Value::Object(fields) => fields
                .values()
                .any(|child| has_field_enum(child, field, values)),
            serde_json::Value::Array(items) => items
                .iter()
                .any(|child| has_field_enum(child, field, values)),
            _ => false,
        }
    }

    let command_schema =
        |name| &tools.iter().find(|tool| tool["name"] == name).unwrap()["inputSchema"];
    let note = &command_schema("mission_note")["properties"];
    assert!(note.get("on").is_some());
    assert!(note.get("target").is_none());
    let apply = &command_schema("mission_apply")["properties"]["document"];
    assert_eq!(apply["type"], "string");
    assert_eq!(apply["contentMediaType"], "application/toml");
    assert_eq!(apply["x-ahjo-cli-file"], true);
    let document = &apply["contentSchema"];
    let change = &command_schema("mission_edit")["properties"]["change"];
    let vocabulary: &[(&str, &[&str])] = &[
        ("formation", &["", "solo", "peer", "orchestrated"]),
        ("kinds", &["code", "step", "human"]),
        ("outcome", &["", "done", "rejected"]),
        ("kind", &["", "text", "file", "folder"]),
        ("human_channel", &["", "notify", "session"]),
        ("when_spent", &["", "stop", "best", "fail"]),
        ("harness", &["", "claude-code", "openai-codex", "pi"]),
        (
            "effort",
            &["", "low", "medium", "high", "xhigh", "max", "ultra"],
        ),
    ];
    for (field, values) in vocabulary {
        assert!(
            has_field_enum(document, field, values),
            "mission_apply {field}"
        );
        assert!(
            has_field_enum(change, field, values),
            "mission_edit {field}"
        );
    }

    let address = format!("{}/", node.url());
    let mut output = Vec::new();
    let help = ["--node".into(), address.clone()];
    assert_eq!(
        ahjo_node_kit::cli::run("mission", &help, &mut output)
            .await
            .unwrap(),
        0
    );
    let help = String::from_utf8(output).unwrap();
    for verb in ["create", "show", "edit", "export", "apply", "purge"] {
        assert!(
            help.lines().any(|line| line
                .split_whitespace()
                .take(3)
                .eq(["ahjo", "mission", verb])),
            "{help}"
        );
    }
    let index = ahjo_node_kit::cli::index(Some(&address)).await.unwrap();
    assert!(
        !index
            .lines()
            .any(|line| line.split_whitespace().next() == Some("gate")),
        "{index}"
    );
    let run_verbs: Vec<_> = index
        .lines()
        .find_map(|line| {
            let mut words = line.split_whitespace();
            (words.next() == Some("run")).then(|| words.collect())
        })
        .unwrap();
    assert_eq!(
        run_verbs,
        [
            "list", "show", "wait", "approve", "reject", "pick", "resume", "abort", "land",
            "delete"
        ]
    );
    for (noun, verb) in [
        ("mission", "new"),
        ("mission", "inspect"),
        ("mission", "wire"),
        ("step", "run"),
        ("end", "create"),
        ("limit", "create"),
        ("gate", "create"),
        ("gate", "approve"),
        ("gate", "request"),
        ("run", "status"),
    ] {
        let args = [
            verb.into(),
            "--node".into(),
            address.clone(),
            "--help".into(),
        ];
        let error = ahjo_node_kit::cli::run(noun, &args, &mut Vec::new())
            .await
            .unwrap_err();
        assert!(
            error.downcast_ref::<ahjo_node_kit::cli::Usage>().is_some(),
            "{error}"
        );
    }
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mission_edit_rejects_an_unknown_agent_and_uses_work_in_both_brief_operations() {
    let node = Node::start(&[sessions::LISTING, ahjo_issue::LISTING, LAB], "")
        .await
        .unwrap();
    node.call(
        "mission_create",
        json!({"slug":"pair","example":"write-and-review"}),
    )
    .await
    .unwrap();
    let inspect = || node.call("mission_show", json!({"mission":"pair","full":true}));
    let before = inspect().await.unwrap();
    let step = &before["mission"]["step"][0];
    let id = &step["id"];
    let change = json!({"staff_seat":{
        "step_id": id, "seat_id": step["seat"][0]["id"],
        "role": "planner", "harness": "claude-code", "effort": "high"
    }});
    let error = node
        .call("mission_edit", json!({"mission":"pair","change":change}))
        .await
        .unwrap_err();
    assert_eq!(error.code, "invalid_argument");
    assert_eq!(
        error.message,
        "staff_seat: unknown field role; accepted fields: step_id, seat_id, agent, harness, model, effort; accepted fields: mission, change, rev",
    );
    let args = vec![
        "edit".into(),
        "pair".into(),
        "--change".into(),
        change.to_string(),
        "--node".into(),
        format!("{}/", node.url()),
        "--json".into(),
    ];
    let mut output = Vec::new();
    let exit = ahjo_node_kit::cli::run("mission", &args, &mut output)
        .await
        .unwrap();
    assert_eq!(exit, 1);
    assert!(output.is_empty());
    assert_eq!(inspect().await.unwrap(), before);

    for (change, work) in [
        (
            json!({"set_brief":{"step_id":id,"issue":"ahjo-2"}}),
            "ahjo-2",
        ),
        (
            json!({"edit_step":{"id":id,"title":step["title"],"brief":{"issue":"ahjo-3"}}}),
            "ahjo-3",
        ),
    ] {
        node.call("mission_edit", json!({"mission":"pair","change":change}))
            .await
            .unwrap();
        assert_eq!(
            inspect().await.unwrap()["mission"]["step"][0]["brief"]["issue"],
            work
        );
    }
    for change in [
        json!({"set_brief":{"step_id":id,"bead_id":"ahjo-2"}}),
        json!({"edit_step":{"id":id,"title":step["title"],"brief":{"bead_id":"ahjo-2"}}}),
    ] {
        let error = node
            .call("mission_edit", json!({"mission":"pair","change":change}))
            .await
            .unwrap_err();
        assert_eq!(error.code, "invalid_argument");
        assert!(
            error.message.contains("bead_id") && error.message.contains("issue"),
            "{error}"
        );
    }
    let mut help = Vec::new();
    let args = vec![
        "edit".into(),
        "--help".into(),
        "--node".into(),
        format!("{}/", node.url()),
    ];
    assert_eq!(
        ahjo_node_kit::cli::run("mission", &args, &mut help)
            .await
            .unwrap(),
        0,
    );
    let help = String::from_utf8(help).unwrap();
    assert!(
        help.contains("edit_step {id, title, brief{goal, repository, issue, files[], links[]}}")
    );
    assert!(help.contains("set_brief {step_id, goal, repository, issue, files[], links[]}"));
    assert!(!help.contains("bead_id"));

    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn author_and_run_with_tools() {
    let node = Node::start(&[sessions::LISTING, ahjo_issue::LISTING, LAB], "")
        .await
        .unwrap();
    assert!(node.call("mission_list", json!({})).await.unwrap()["missions"].is_array());
    node.call("mission_create", json!({"slug":"demo","title":"Demo"}))
        .await
        .unwrap();
    let copied = node
        .call(
            "mission_create",
            json!({"slug":"pair","example":"write-and-review"}),
        )
        .await
        .unwrap();
    assert_eq!(
        copied["steps"],
        json!([{"id":"fmn_write","title":"Write"},{"id":"fmn_review","title":"Review"}])
    );
    assert_eq!(
        node.call("mission_show", json!({"mission":"demo"}))
            .await
            .unwrap()["title"],
        "Demo"
    );
    assert!(
        node.call("mission_validate", json!({"mission":"demo"}))
            .await
            .unwrap()["errors"]
            .is_array()
    );
    let edited = node
        .call(
            "mission_edit",
            json!({"mission":"demo","change":{"create_input_card":{"title":"Start","goal":"Work"}}}),
        )
        .await
        .unwrap();
    use ahjo_missions_contract::commands::{AddedCard, MissionEdited};
    let receipt: MissionEdited = serde_json::from_value(edited).unwrap();
    let Some(AddedCard::InputCard(card)) = receipt.added else {
        panic!("missing new Input card")
    };
    let id = &card.id;
    assert!(!id.is_empty());
    let note = node
        .call("mission_note", json!({"mission":"demo","text":"intent"}))
        .await
        .unwrap();
    assert_eq!(
        (&note["on"], &note["thread"][0]["text"]),
        (&json!("mission"), &json!("intent"))
    );
    assert_eq!(
        node.call("mission_notes", json!({"mission":"demo"}))
            .await
            .unwrap()["notes"]["mission"][0]["text"],
        "intent"
    );
    assert!(
        node.call("layout_show", json!({"mission":"demo"}))
            .await
            .unwrap()["layout"]["card"]
            .is_array()
    );
    assert_eq!(
        node.call(
            "layout_edit",
            json!({"mission":"demo","change":{"cards":[{"id":id,"x":10,"y":20}]}})
        )
        .await
        .unwrap()["layout"]["card"][0]["x"],
        10
    );
    let folder = node.dir().join("missions/missions");
    std::fs::write(
        folder.join("hello.mission.toml"),
        include_str!("fixtures/solo-lab.mission.toml"),
    )
    .unwrap();
    let path = folder.join("admission-findings.mission.toml");
    let source = include_str!("fixtures/admission-findings.mission.toml");
    std::fs::write(
        &path,
        source.replacen("harness = \"claude-code\"", "harness = \"wrong\"", 1),
    )
    .unwrap();
    let unknown_harness = node
        .call("mission_run", json!({"mission":"admission-findings"}))
        .await
        .unwrap_err();
    assert_eq!(unknown_harness.code, "invalid_definition_source");
    assert!(
        unknown_harness.message.contains("harness") && unknown_harness.message.contains("wrong")
    );
    std::fs::write(path, source).unwrap();
    let rejected = node
        .call("mission_run", json!({"mission":"admission-findings"}))
        .await
        .unwrap_err();
    assert_eq!(rejected.code, "run_admission_failed");
    assert!(
        rejected.message.contains("missing_input")
            && rejected.message.contains("step_without_seats")
    );
    let started = node
        .call(
            "mission_run",
            json!({"mission":"hello","inputs":{"brief":"hi"}}),
        )
        .await
        .unwrap();
    let run = started["run_id"].as_str().unwrap();
    assert!(node.call("run_list", json!({})).await.unwrap()["runs"].is_array());
    let waited = node
        .call("run_wait", json!({"run":run,"until":"final","timeout":5}))
        .await
        .unwrap();
    assert_eq!(waited["outcome"], "final");
    assert!(waited["outputs"].is_array());
    assert_eq!(
        node.call("run_show", json!({"run":run})).await.unwrap()["state"]["run_id"],
        run
    );
    let logs = node
        .call("run_show", json!({"run":run,"events":true}))
        .await
        .unwrap();
    let step_event = logs["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["card_id"] == "fmn_work")
        .expect("a step's event");
    assert!(
        !step_event["step"].as_str().unwrap_or("").is_empty(),
        "a log line names its step: {step_event}"
    );
    let valid = node
        .call("mission_validate", json!({"mission":"hello"}))
        .await
        .unwrap();
    assert_eq!(
        (&valid["valid"], &valid["summary"]),
        (&json!(true), &json!("valid"))
    );
    assert!(
        node.call("run_show", json!({"run":run,"seats":true}))
            .await
            .unwrap()["seats"]
            .is_array()
    );
    // A durable, resumable run: copy the engine's actual snapshots from the
    // completed run, then record a shutdown block for a fresh run id.
    let runs = node.dir().join("missions/runs");
    let original = ahjo_mission_engine::Ledger::new(runs.join(format!("{run}.ndjson")), run);
    let mut first = original.read().unwrap().remove(0);
    first.run_id.clear();
    let resumed = "run_resume_test";
    for suffix in ["snapshot.toml", "bindings.toml"] {
        std::fs::copy(
            runs.join(format!("{run}.{suffix}")),
            runs.join(format!("{resumed}.{suffix}")),
        )
        .unwrap();
    }
    let ledger = ahjo_mission_engine::Ledger::new(runs.join(format!("{resumed}.ndjson")), resumed);
    ledger.create(first).unwrap();
    let blocked = ahjo_mission_engine::RunEvent {
        kind: "run_blocked".into(),
        timestamp: "2026-01-01T00:00:00Z".into(),
        data: json!({"code":"coordinator_shutdown","resume_allowed":true})
            .as_object()
            .unwrap()
            .clone(),
        ..Default::default()
    };
    ledger.append(blocked).unwrap();
    let response = node
        .call("run_resume", json!({"run":resumed}))
        .await
        .unwrap();
    assert_eq!(response["run"]["run_id"], resumed);
    assert!(
        ledger
            .read()
            .unwrap()
            .iter()
            .any(|event| event.kind == "run_resumed")
    );
    let step = node
        .call(
            "mission_run",
            json!({"mission":"hello","from":"fmn_work","inputs":{"brief":"hi"}}),
        )
        .await
        .unwrap();
    assert!(step["run_id"].is_string());
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn gate_and_abort_commands_act_on_pending_runs() {
    use ahjo_mission_engine::{Ledger, RunEvent};
    let node = Node::start(&[sessions::LISTING, ahjo_issue::LISTING, LAB], "")
        .await
        .unwrap();
    let event = |kind: &str| RunEvent {
        kind: kind.into(),
        actor: "agent:test".into(),
        timestamp: "2026-01-01T00:00:00Z".into(),
        ..Default::default()
    };
    for (run, verb, pick, actor_arg, expected) in [
        (
            "run_approve",
            "approve",
            false,
            Some("agent:alice"),
            "agent:alice",
        ),
        (
            "run_reject",
            "reject",
            false,
            Some("agent:bob"),
            "agent:bob",
        ),
        ("run_pick", "pick", true, Some("agent:carol"), "agent:carol"),
        ("run_approve_default", "approve", false, None, "agent:ahjo"),
        ("run_reject_default", "reject", false, None, "agent:ahjo"),
        ("run_pick_default", "pick", true, None, "agent:ahjo"),
        (
            "run_approve_empty",
            "approve",
            false,
            Some(""),
            "agent:ahjo",
        ),
    ] {
        let ledger = Ledger::new(
            node.dir()
                .join("missions/runs")
                .join(format!("{run}.ndjson")),
            run,
        );
        ledger.create(event("run_started")).unwrap();
        let mut evaluating = event("gate_evaluating");
        evaluating.gate_id = "review".into();
        ledger.append(evaluating).unwrap();
        let mut request = event("human_input_requested");
        request.gate_id = "review".into();
        if pick {
            request.data = json!({"pick":{"candidates":[1,2]}})
                .as_object()
                .unwrap()
                .clone();
        }
        ledger.append(request).unwrap();
        let mut args: Vec<String> = [verb, run, "review", "--seq", "3"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        if pick {
            args.extend(["--attempt".into(), "1".into()]);
        }
        if let Some(actor) = actor_arg {
            args.extend(["--actor".into(), actor.into()]);
        }
        args.extend(["--node".into(), format!("{}/", node.url()), "--json".into()]);
        let mut output = Vec::new();
        assert_eq!(
            ahjo_node_kit::cli::run("run", &args, &mut output)
                .await
                .unwrap(),
            0,
            "{args:?}"
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&output).unwrap()["run_id"],
            run
        );
        let events = ledger.read().unwrap();
        let decision = events
            .iter()
            .find(|e| e.kind == "human_verdict_recorded")
            .unwrap();
        assert_eq!(decision.actor, expected, "{verb}");
        assert_eq!(decision.data["decided_by"], expected, "{verb}");
        if !pick {
            assert_eq!(
                decision.data["verdict"],
                if verb == "approve" { "pass" } else { "fail" }
            );
        }
    }
    let ledger = Ledger::new(
        node.dir().join("missions/runs/run_abort.ndjson"),
        "run_abort",
    );
    ledger.create(event("run_started")).unwrap();
    assert_eq!(
        node.call("run_abort", json!({"run":"run_abort"}))
            .await
            .unwrap()["run"]["status"],
        "canceled"
    );
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_repository_run_can_be_landed() {
    let root = ahjo_testkit::fresh_dir("mission-checkout");
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "-b", "main"]);
    git(&["config", "user.name", "Test"]);
    git(&["config", "user.email", "test@example.invalid"]);
    std::fs::write(root.join("readme"), "test\n").unwrap();
    git(&["add", "readme"]);
    git(&["commit", "-m", "initial"]);
    let config = format!("[missions]\nrepositories = {{ \"example/project\" = {root:?} }}\n");
    let node = Node::start(&[sessions::LISTING, ahjo_issue::LISTING, LAB], &config)
        .await
        .unwrap();
    let folder = node.dir().join("missions/missions");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("hello.mission.toml"),
        include_str!("fixtures/solo-lab.mission.toml"),
    )
    .unwrap();
    let start = node
        .call(
            "mission_run",
            json!({"mission":"hello","repository":"example/project","inputs":{"brief":"hi"}}),
        )
        .await
        .unwrap();
    let run = start["run_id"].as_str().unwrap();
    assert_eq!(
        node.call("run_wait", json!({"run":run,"until":"final","timeout":5}))
            .await
            .unwrap()["outcome"],
        "final"
    );
    // A repository run keeps its branch until it is landed.
    let kept = node
        .call("run_delete", json!({"run":run}))
        .await
        .unwrap_err();
    assert!(kept.message.contains("land it first"), "{}", kept.message);
    assert_eq!(
        node.call("run_land", json!({"run":run,"force":true}))
            .await
            .unwrap()["landed"],
        true
    );
    node.call("run_delete", json!({"run":run})).await.unwrap();
    assert!(node.call("run_show", json!({"run":run})).await.is_err());
    assert!(
        node.call("run_list", json!({})).await.unwrap()["runs"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    node.stop().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_mission_built_through_commands_stays_arranged() {
    let node = Node::start(&[sessions::LISTING, ahjo_issue::LISTING, LAB], "")
        .await
        .unwrap();
    let call = |name: &'static str, args: serde_json::Value| {
        let node = &node;
        async move { node.call(name, args).await.unwrap() }
    };
    let made = call(
        "mission_create",
        json!({"slug":"line","steps":"Write; Review"}),
    )
    .await;
    let shown = call("mission_show", json!({"mission":"line","full":true})).await;
    let shown = &shown["mission"];
    let places = |layout: &serde_json::Value| {
        let mut v: Vec<(String, i64, i64)> = layout["layout"]["card"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| {
                (
                    n["id"].as_str().unwrap().to_owned(),
                    n["x"].as_i64().unwrap(),
                    n["y"].as_i64().unwrap(),
                )
            })
            .collect();
        v.sort();
        v
    };
    let built = places(&call("layout_show", json!({"mission":"line"})).await);
    assert_eq!(built.len(), 4);
    let arranged = places(&call("mission_arrange", json!({"mission":"line"})).await);
    assert_eq!(
        built, arranged,
        "the commands left it as Arrange lays it out"
    );
    // Read left to right: Start, Write, Review, Done.
    let x = |id: &str| built.iter().find(|p| p.0 == id).unwrap().1;
    let ids = [
        shown["input_card"][0]["id"].as_str().unwrap(),
        made["steps"][0]["id"].as_str().unwrap(),
        made["steps"][1]["id"].as_str().unwrap(),
        shown["end"][0]["id"].as_str().unwrap(),
    ];
    assert!(ids.windows(2).all(|w| x(w[0]) < x(w[1])));
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn cli_uses_the_same_mission_commands() {
    let node = Node::start(&[sessions::LISTING, ahjo_issue::LISTING, LAB], "")
        .await
        .unwrap();
    let mut output = Vec::new();
    let args = vec![
        "create".to_string(),
        "demo".to_string(),
        "--node".to_string(),
        format!("{}/", node.url()),
        "--json".to_string(),
    ];
    assert_eq!(
        ahjo_node_kit::cli::run("mission", &args, &mut output)
            .await
            .unwrap(),
        0
    );
    let result: serde_json::Value = serde_json::from_slice(&output).unwrap();
    // A receipt, not the graph, with the commands that read the rest,
    // aimed at the node named.
    assert_eq!(result["created"], "demo");
    assert!(result.get("mission").is_none());
    assert_eq!(
        result["next"][0],
        format!("ahjo mission show --node {}/ demo", node.url())
    );
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn cli_lists_and_removes_deleted_mission_archives() {
    let node = Node::start(&[sessions::LISTING, ahjo_issue::LISTING, LAB], "")
        .await
        .unwrap();
    node.call(
        "mission_create",
        json!({"slug":"archived","title":"Archived"}),
    )
    .await
    .unwrap();
    node.call(
        "mission_edit",
        json!({"mission":"archived","change":{"create_input_card":{"title":"Start","goal":"Work"}}}),
    )
    .await
    .unwrap();
    node.call(
        "mission_note",
        json!({"mission":"archived","text":"Keep this note"}),
    )
    .await
    .unwrap();
    async fn cli(node: &Node, args: &[&str]) -> serde_json::Value {
        let mut args: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
        args.extend(["--node".into(), format!("{}/", node.url()), "--json".into()]);
        let mut output = Vec::new();
        assert_eq!(
            ahjo_node_kit::cli::run("mission", &args, &mut output)
                .await
                .unwrap(),
            0,
            "{args:?}"
        );
        serde_json::from_slice(&output).unwrap()
    }
    let mut help = Vec::new();
    let args = vec![
        "delete".into(),
        "--help".into(),
        "--node".into(),
        format!("{}/", node.url()),
    ];
    assert_eq!(
        ahjo_node_kit::cli::run("mission", &args, &mut help)
            .await
            .unwrap(),
        0
    );
    let help = String::from_utf8(help).unwrap();
    assert!(help.contains("archive") && help.contains("purge"), "{help}");
    let deletion = cli(&node, &["delete", "archived"]).await;
    let archive = deletion["deletion"]["archive_id"].as_str().unwrap();
    assert!(
        cli(&node, &["list"]).await["missions"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let listed = cli(&node, &["archives"]).await;
    let record = listed["archives"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["archive_id"] == archive)
        .unwrap();
    assert_eq!(record["slug"], "archived");
    assert_eq!(record["mission"], true);
    assert_eq!(record["layout"], true);
    assert_eq!(record["notes"], true);
    // Reusing the slug must not let purging the earlier archive remove the
    // new definition, its notes, or any run history.
    node.call(
        "mission_create",
        json!({"slug":"archived","title":"Replacement"}),
    )
    .await
    .unwrap();
    let notes = node.dir().join("missions/notes/archived.notes.toml");
    assert!(!notes.exists());
    node.call(
        "mission_note",
        json!({"mission":"archived","text":"Replacement note"}),
    )
    .await
    .unwrap();
    assert!(notes.exists());
    let run_dir = node.dir().join("missions/runs");
    std::fs::create_dir_all(&run_dir).unwrap();
    std::fs::write(run_dir.join("keep-history"), "unrelated run").unwrap();
    let removed = cli(&node, &["purge", "archived", archive]).await;
    assert_eq!(removed["removed"]["archive_id"], archive);
    assert!(
        cli(&node, &["archives"]).await["archives"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        node.call("mission_show", json!({"mission":"archived"}))
            .await
            .unwrap()["title"],
        "Replacement"
    );
    assert!(
        std::fs::read_to_string(&notes)
            .unwrap()
            .contains("Replacement note")
    );
    assert_eq!(
        std::fs::read_to_string(run_dir.join("keep-history")).unwrap(),
        "unrelated run"
    );
    node.stop().await;
}

/// An agent told to finish a mission reads it in one answer, with every
/// note beside what it is on, then answers and closes the notes it took up.
#[tokio::test(flavor = "multi_thread")]
async fn an_agent_picks_up_a_mission_from_its_notes() {
    let node = Node::start(&[sessions::LISTING, ahjo_issue::LISTING, LAB], "")
        .await
        .unwrap();
    node.call(
        "mission_create",
        json!({"slug":"ship","example":"plan-build-check"}),
    )
    .await
    .unwrap();
    let note = |target: &str, text: &str| json!({"mission":"ship","on":target,"text":text,"author":"human:ui"});
    let intent = node
        .call("mission_note", note("", "Ship it\nby Friday"))
        .await
        .unwrap();
    for (target, text) in [
        ("Build", "Small commits"),
        ("lim_hour", "An hour is plenty"),
        ("Plan->Build", "The plan, as a file"),
    ] {
        node.call("mission_note", note(target, text)).await.unwrap();
    }
    let read = node
        .call("mission_show", json!({"mission":"ship"}))
        .await
        .unwrap();
    assert_eq!(read["notes"], "4 threads, 4 open", "{read}");
    assert_eq!(read["about"][0]["text"], "Ship it\nby Friday");
    let steps: Vec<_> = read["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["n"].clone(), s["id"].as_str().unwrap()))
        .collect();
    assert_eq!(
        steps,
        [
            (json!(1), "fmn_plan"),
            (json!(2), "fmn_build"),
            (json!(3), "gate_checks")
        ]
    );
    assert_eq!(read["steps"][1]["notes"][0]["text"], "Small commits");
    assert_eq!(read["steps"][2]["fail"], "back to 2 Build");
    assert_eq!(read["limits"][0]["notes"][0]["text"], "An hour is plenty");
    assert_eq!(read["wires"][0]["wire"], "Plan -> Build");
    assert_eq!(read["wires"][0]["notes"][0]["by"], "human:ui");

    let build_note = read["steps"][1]["notes"][0]["id"].as_str().unwrap();
    let asked = node
        .call(
            "mission_reply",
            json!({"mission":"ship","note":build_note,"text":"One per step?","author":"claude-7"}),
        )
        .await
        .unwrap();
    assert_eq!(
        (&asked["on"], &asked["resolved"], &asked["thread"][1]["by"]),
        (&json!("fmn_build"), &json!(false), &json!("agent:claude-7"))
    );
    let intent_id = intent["note"].as_str().unwrap();
    node.call(
        "mission_resolve",
        json!({"mission":"ship","note":intent_id,"text":"Staffed and briefed every step","author":"claude-7"}),
    )
    .await
    .unwrap();
    let read = node
        .call("mission_show", json!({"mission":"ship"}))
        .await
        .unwrap();
    assert_eq!(read["notes"], "4 threads, 3 open");
    assert_eq!(read["about"][1]["resolves"], true);
    // A later note opens the thread again.
    node.call("mission_note", note("", "One more thing"))
        .await
        .unwrap();
    let read = node
        .call("mission_show", json!({"mission":"ship"}))
        .await
        .unwrap();
    assert_eq!(read["notes"], "4 threads, 4 open");

    // Only the author changes a note; anyone may answer it.
    let mine = asked["note"].as_str().unwrap();
    node.call(
        "mission_note",
        json!({"mission":"ship","edit":mine,"text":"One commit per step?","author":"claude-7"}),
    )
    .await
    .unwrap();
    let theirs = node
        .call(
            "mission_note",
            json!({"mission":"ship","edit":build_note,"text":"Mine now","author":"claude-7"}),
        )
        .await
        .unwrap_err();
    assert_eq!(theirs.code, "note_author_mismatch");
    node.call(
        "mission_note",
        json!({"mission":"ship","delete":mine,"author":"claude-7"}),
    )
    .await
    .unwrap();
    let missing = node
        .call("mission_note", note("Nowhere", "lost"))
        .await
        .unwrap_err();
    assert_eq!(missing.code, "not_found");
    node.stop().await;
}

/// A mission sketched from rough steps reads in their order, from the
/// Input to Done, with what it is for as its note.
#[tokio::test(flavor = "multi_thread")]
async fn a_mission_is_sketched_from_rough_steps() {
    let node = Node::start(&[sessions::LISTING, ahjo_issue::LISTING, LAB], "")
        .await
        .unwrap();
    node.call(
        "mission_create",
        json!({"slug":"digest","steps":"Collect\nGroup; Write\n\n","note":"A Monday digest"}),
    )
    .await
    .unwrap();
    let read = node
        .call("mission_show", json!({"mission":"digest"}))
        .await
        .unwrap();
    let order: Vec<_> = (read["steps"].as_array().unwrap().iter())
        .map(|s| (s["step"].as_str().unwrap(), s["next"].as_str().unwrap()))
        .collect();
    assert_eq!(
        order,
        [
            ("Collect", "2 Group"),
            ("Group", "3 Write"),
            ("Write", "end (done)")
        ],
        "{read}"
    );
    assert_eq!(read["input"][0]["next"], "1 Collect");
    assert_eq!(read["about"][0]["text"], "A Monday digest");
    let mixed = node
        .call(
            "mission_create",
            json!({"slug":"both","steps":"One","example":"write-and-review"}),
        )
        .await
        .unwrap_err();
    assert_eq!(mixed.code, "bad_request");
    assert!(
        node.call("mission_show", json!({"mission":"both"}))
            .await
            .is_err()
    );
    node.stop().await;
}

/// A sketch as long as a person might type is made whole and laid out as
/// Arrange lays it out, every card in a place of its own.
#[tokio::test(flavor = "multi_thread")]
async fn a_long_sketch_is_made_whole_and_arranged() {
    let node = Node::start(&[sessions::LISTING, ahjo_issue::LISTING, LAB], "")
        .await
        .unwrap();
    for count in [15, 60] {
        let slug = format!("long{count}");
        let steps: Vec<_> = (1..=count).map(|i| format!("Step {i}")).collect();
        let made = node
            .call(
                "mission_create",
                json!({"slug":slug,"steps":steps.join("\n")}),
            )
            .await
            .unwrap();
        assert_eq!(made["steps"].as_array().unwrap().len(), count, "{made}");
        let read = node
            .call("mission_show", json!({"mission":slug}))
            .await
            .unwrap();
        let last = read["steps"].as_array().unwrap().last().unwrap().clone();
        assert_eq!(last["next"], "end (done)", "{read}");
        let nodes = |layout: serde_json::Value| {
            let mut v: Vec<(String, i64, i64)> = (layout["layout"]["card"].as_array())
                .unwrap()
                .iter()
                .map(|n| {
                    let id = n["id"].as_str().unwrap().to_owned();
                    (id, n["x"].as_i64().unwrap(), n["y"].as_i64().unwrap())
                })
                .collect();
            v.sort();
            v
        };
        let built = nodes(
            node.call("layout_show", json!({"mission":slug}))
                .await
                .unwrap(),
        );
        // The Input, every step and Done.
        assert_eq!(built.len(), count + 2);
        let arranged = nodes(
            node.call("mission_arrange", json!({"mission":slug}))
                .await
                .unwrap(),
        );
        assert_eq!(built, arranged, "a sketch is left as Arrange lays it out");
        let mut places: Vec<_> = built.iter().map(|(_, x, y)| (x, y)).collect();
        places.sort();
        places.dedup();
        assert_eq!(places.len(), built.len(), "two cards share a place");
    }
    node.stop().await;
}

/// A sketch that cannot be finished leaves nothing behind, and the error
/// says the mission was not created.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_sketch_leaves_no_mission() {
    let node = Node::start(&[sessions::LISTING, ahjo_issue::LISTING, LAB], "")
        .await
        .unwrap();
    // The steps are made first; the note, too large to keep, fails last.
    let failed = node
        .call(
            "mission_create",
            json!({"slug":"half","steps":"Collect\nWrite","note":"x".repeat(70 * 1024)}),
        )
        .await
        .unwrap_err();
    assert_eq!(failed.code, "note_too_large", "{failed:?}");
    assert!(
        failed.message.contains("the mission was not created"),
        "{failed:?}"
    );
    let missing = node
        .call("mission_show", json!({"mission":"half"}))
        .await
        .unwrap_err();
    assert_eq!(missing.code, "not_found");
    let archives = node.call("mission_archives", json!({})).await.unwrap();
    assert_eq!(archives["archives"], json!([]), "{archives}");
    // The name is free again.
    node.call("mission_create", json!({"slug":"half","steps":"Collect"}))
        .await
        .unwrap();
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mission_edit_batches_commit_once_or_leave_the_mission_and_layout_untouched() {
    let node = Node::start(&[sessions::LISTING, ahjo_issue::LISTING, LAB], "")
        .await
        .unwrap();
    node.call("mission_create", json!({"slug":"batch","title":"Original"}))
        .await
        .unwrap();

    let inspect = || node.call("mission_show", json!({"mission":"batch","full":true}));
    let layout = || node.call("layout_show", json!({"mission":"batch"}));
    let before = inspect().await.unwrap();
    let before_layout = layout().await.unwrap();
    let rev = before["mission"]["rev"].as_i64().unwrap();

    for (second, code, message) in [
        (
            json!({"create_end":{"outcome":"invalid"}}),
            "invalid_argument",
            "create_end: unknown variant `invalid`, expected one of ``, `done`, `rejected`",
        ),
        (
            json!({"create_end":{"unexpected":null}}),
            "invalid_argument",
            "create_end: unknown field unexpected; accepted fields: title, outcome, x, y",
        ),
    ] {
        let error = node
            .call("mission_edit", json!({"mission":"batch","change":{
                "rev":rev, "changes":[{"create_step":{"title":"Work","x":10,"y":20}}, second, {"title":"New"}]
            }}))
            .await
            .unwrap_err();
        assert_eq!(error.code, code);
        let message = format!("change at index 1: {message}");
        if code == "invalid_argument" {
            assert_eq!(
                error.message,
                format!("{message}; accepted fields: mission, change, rev")
            );
        } else {
            assert_eq!(error.message, message);
        }
        assert_eq!(inspect().await.unwrap(), before);
        assert_eq!(layout().await.unwrap(), before_layout);
    }

    let change = json!({"rev":rev,"changes":[
        {"create_step":{"title":"Work","x":10,"y":20}},
        {"create_end":{"title":"Done","x":100,"y":200}},
        {"title":"New"}
    ]});
    let args = [
        "edit".into(),
        "batch".into(),
        "--change".into(),
        change.to_string(),
        "--node".into(),
        format!("{}/", node.url()),
        "--json".into(),
    ];
    let mut output = Vec::new();
    assert_eq!(
        ahjo_node_kit::cli::run("mission", &args, &mut output)
            .await
            .unwrap(),
        0
    );
    let after: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(after["mission"]["rev"], rev + 1);
    assert_eq!(after["mission"]["title"], "New");
    assert_eq!(after["mission"]["step"][0]["title"], "Work");
    assert_eq!(after["mission"]["end"][0]["title"], "Done");
    assert_eq!(inspect().await.unwrap()["mission"], after["mission"]);

    let after_layout = layout().await.unwrap();
    assert_eq!(after_layout["layout"]["mission_rev"], rev + 1);
    let nodes = after_layout["layout"]["card"].as_array().unwrap();
    for (id, x, y) in [
        (&after["mission"]["step"][0]["id"], 10, 20),
        (&after["mission"]["end"][0]["id"], 100, 200),
    ] {
        assert!(
            nodes
                .iter()
                .any(|node| node["id"] == *id && node["x"] == x && node["y"] == y)
        );
    }
    let stale = node
        .call("mission_edit", json!({"mission":"batch","change":change}))
        .await
        .unwrap_err();
    assert_eq!(stale.code, "conflict");
    assert_eq!(layout().await.unwrap(), after_layout);
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn mission_show_resolves_seat_settings_and_names_the_session() {
    let node = Node::start(&[sessions::LISTING, ahjo_issue::LISTING, LAB], "")
        .await
        .unwrap();
    node.call(
        "mission_create",
        json!({"slug":"seat-demo","example":"write-and-review"}),
    )
    .await
    .unwrap();

    node.call(
        "mission_edit",
        json!({"mission":"seat-demo","change":{"staff_seat":{
            "step_id":"fmn_write", "seat_id":"seat_write", "agent":"builder"
        }}}),
    )
    .await
    .unwrap();

    let before = node
        .call("mission_show", json!({"mission":"seat-demo","full":true}))
        .await
        .unwrap();
    for change in [
        json!({"name_agent":{"step_id":"fmn_write","seat_id":"seat_write","agent":"legacy"}}),
        json!({"staff_seat":{"step_id":"fmn_write","seat_id":"seat_write","agent_id":"builder"}}),
    ] {
        let error = node
            .call(
                "mission_edit",
                json!({"mission":"seat-demo","change":change}),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "invalid_argument");
        assert_eq!(
            node.call("mission_show", json!({"mission":"seat-demo","full":true}))
                .await
                .unwrap(),
            before
        );
    }

    let summary = node
        .call("mission_show", json!({"mission":"seat-demo"}))
        .await
        .unwrap();
    let step = summary["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|step| step["id"] == "fmn_write")
        .unwrap();
    let seat = &step["seats"][0];
    assert_eq!(seat["id"], "seat_write");
    assert_eq!(seat["agent"], "builder");
    assert_eq!(seat["harness"], "claude-code");
    assert_eq!(seat["model"], "");
    assert_eq!(seat["effort"], "medium");
    assert_eq!(seat["session"], "seatdemo-write");

    let mut stdout = Vec::new();
    let args = vec![
        "show".into(),
        "seat-demo".into(),
        "--json".into(),
        "--node".into(),
        format!("{}/", node.url()),
    ];
    assert_eq!(
        ahjo_node_kit::cli::run("mission", &args, &mut stdout)
            .await
            .unwrap(),
        0
    );
    let cli: serde_json::Value = serde_json::from_slice(&stdout).unwrap();
    assert_eq!(cli["steps"], summary["steps"]);

    node.call(
        "mission_edit",
        json!({"mission":"seat-demo","change":{"staff_seat":{
            "step_id":"fmn_write", "seat_id":"seat_write", "agent":"builder",
            "harness":"openai-codex", "effort":"ultra"
        }}}),
    )
    .await
    .unwrap();
    node.call(
        "mission_edit",
        json!({"mission":"seat-demo","change":{"update_step":{"id":"fmn_write","fresh":true}}}),
    )
    .await
    .unwrap();
    let summary = node
        .call("mission_show", json!({"mission":"seat-demo"}))
        .await
        .unwrap();
    let step = summary["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|step| step["id"] == "fmn_write")
        .unwrap();
    assert_eq!(step["fresh"], true);
    assert_eq!(step["seats"][0]["agent"], "builder");
    assert_eq!(step["seats"][0]["harness"], "openai-codex");
    assert_eq!(step["seats"][0]["effort"], "ultra");
    assert_eq!(step["seats"][0]["session"], "seatdemo-write");
    node.stop().await;
}
