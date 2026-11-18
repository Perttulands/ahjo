use ahjo_node_kit::{Built, Listing, Setup};
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
    "mission_purge_archive",
    "mission_inspect",
    "mission_validate",
    "mission_new",
    "mission_examples",
    "mission_edit",
    "mission_notes",
    "mission_note",
    "mission_reply",
    "mission_resolve",
    "layout_show",
    "layout_edit",
    "mission_run",
    "formation_run",
    "run_list",
    "run_status",
    "run_wait",
    "run_evidence",
    "run_attempts",
    "run_gates",
    "run_seats",
    "run_logs",
    "run_abort",
    "run_resume",
    "run_land",
    "run_delete",
    "gate_request",
    "gate_approve",
    "gate_reject",
    "gate_pick",
    "mission_arrange",
    "mission_rename",
    "mission_prefix",
    "mission_move",
    "mission_create",
    "mission_update",
    "mission_delete",
    "mission_input",
    "mission_output",
    "mission_wire",
    "formation_list",
    "formation_inspect",
    "formation_create",
    "formation_rename",
    "formation_place",
    "formation_set_type",
    "formation_set_slots",
    "formation_assign",
    "formation_unassign",
    "formation_agent",
    "formation_fresh",
    "formation_set_brief",
    "formation_add_input",
    "formation_add_output",
    "formation_set_port",
    "formation_wire",
    "formation_unwire",
    "formation_delete",
    "gate_create",
    "gate_update",
    "gate_judge",
    "gate_delete",
    "end_create",
    "end_update",
    "end_delete",
    "limit_create",
    "limit_update",
    "limit_delete",
];

#[tokio::test(flavor = "multi_thread")]
async fn command_list_and_coded_failures() {
    assert_eq!(ahjo_missions::LISTING.commands, EXPECTED);
    let role_commands: Vec<_> = ahjo_agents::LISTING
        .commands
        .iter()
        .copied()
        .filter(|name| name.starts_with("role_"))
        .collect();
    assert_eq!(
        role_commands,
        [
            "role_list",
            "role_inspect",
            "role_usage",
            "role_new",
            "role_edit",
            "role_retire",
            "role_restore",
            "role_delete",
            "role_characters"
        ]
    );
    let node = Node::start(&[sessions::LISTING, ahjo_work::LISTING, LAB], "")
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
        assert_eq!(error.code, "bad_request", "{command}: {error}");
    }
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn author_and_run_with_tools() {
    let node = Node::start(&[sessions::LISTING, ahjo_work::LISTING, LAB], "")
        .await
        .unwrap();
    assert!(node.call("mission_list", json!({})).await.unwrap()["missions"].is_array());
    node.call("mission_new", json!({"slug":"demo","title":"Demo"}))
        .await
        .unwrap();
    let copied = node
        .call(
            "mission_new",
            json!({"slug":"pair","example":"write-and-review"}),
        )
        .await
        .unwrap();
    assert_eq!(
        copied["steps"],
        json!([{"id":"fmn_write","title":"Write"},{"id":"fmn_review","title":"Review"}])
    );
    assert_eq!(
        node.call("mission_inspect", json!({"mission":"demo"}))
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
            json!({"mission":"demo","change":{"createInputCard":{"title":"Start","goal":"Work"}}}),
        )
        .await
        .unwrap();
    let id = edited["inputCard"]["id"].as_str().unwrap();
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
            .unwrap()["layout"]["nodes"]
            .is_array()
    );
    assert_eq!(
        node.call(
            "layout_edit",
            json!({"mission":"demo","change":{"nodes":[{"id":id,"x":10,"y":20}]}})
        )
        .await
        .unwrap()["layout"]["nodes"][0]["x"],
        10
    );
    let folder = node.dir().join("missions/missions");
    std::fs::write(
        folder.join("hello.mission.toml"),
        include_str!("fixtures/solo-lab.mission.toml"),
    )
    .unwrap();
    std::fs::write(
        folder.join("admission-findings.mission.toml"),
        include_str!("fixtures/admission-findings.mission.toml"),
    )
    .unwrap();
    let rejected = node
        .call("mission_run", json!({"mission":"admission-findings"}))
        .await
        .unwrap_err();
    assert_eq!(rejected.code, "run_admission_failed");
    assert!(
        rejected.message.contains("missing_input")
            && rejected.message.contains("formation_without_slots")
    );
    let started = node
        .call(
            "mission_run",
            json!({"mission":"hello","inputs":{"brief":"hi"}}),
        )
        .await
        .unwrap();
    let run = started["runId"].as_str().unwrap();
    assert!(node.call("run_list", json!({})).await.unwrap()["runs"].is_array());
    let waited = node
        .call("run_wait", json!({"run":run,"until":"final","timeout":5}))
        .await
        .unwrap();
    assert_eq!(waited["outcome"], "final");
    assert!(waited["outputs"].is_array());
    assert_eq!(
        node.call("run_status", json!({"run":run})).await.unwrap()["runId"],
        run
    );
    let logs = node.call("run_logs", json!({"run":run})).await.unwrap();
    let step_event = logs["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["nodeId"] == "fmn_work")
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
    assert!(node.call("run_seats", json!({"run":run})).await.unwrap()["seats"].is_array());
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
        data: json!({"code":"coordinator_shutdown","resumeAllowed":true})
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
    assert_eq!(response["run"]["runId"], resumed);
    assert!(
        ledger
            .read()
            .unwrap()
            .iter()
            .any(|event| event.kind == "run_resumed")
    );
    let formation = node
        .call(
            "formation_run",
            json!({"mission":"hello","formation":"fmn_work","inputs":{"brief":"hi"}}),
        )
        .await
        .unwrap();
    assert!(formation["runId"].is_string());
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn gate_and_abort_commands_act_on_pending_runs() {
    use ahjo_mission_engine::{Ledger, RunEvent};
    let node = Node::start(&[sessions::LISTING, ahjo_work::LISTING, LAB], "")
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
        let mut args: Vec<String> = [verb, run, "review", "--requested-seq", "3"]
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
            ahjo_node_kit::cli::run("gate", &args, &mut output)
                .await
                .unwrap(),
            0,
            "{args:?}"
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&output).unwrap()["runId"],
            run
        );
        let events = ledger.read().unwrap();
        let decision = events
            .iter()
            .find(|e| e.kind == "human_verdict_recorded")
            .unwrap();
        assert_eq!(decision.actor, expected, "{verb}");
        assert_eq!(decision.data["decidedBy"], expected, "{verb}");
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
    let node = Node::start(&[sessions::LISTING, ahjo_work::LISTING, LAB], &config)
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
    let run = start["runId"].as_str().unwrap();
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
    assert!(node.call("run_status", json!({"run":run})).await.is_err());
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
    let node = Node::start(&[sessions::LISTING, ahjo_work::LISTING, LAB], "")
        .await
        .unwrap();
    let call = |name: &'static str, args: serde_json::Value| {
        let node = &node;
        async move { node.call(name, args).await.unwrap() }
    };
    call("mission_new", json!({"slug":"line"})).await;
    let input = call("mission_create", json!({"mission":"line","title":"Start"})).await;
    let input = input["inputCard"]["id"].as_str().unwrap().to_owned();
    let mut steps = Vec::new();
    for title in ["Write", "Review"] {
        let step = call(
            "formation_create",
            json!({"mission":"line","type":"solo","title":title}),
        )
        .await;
        steps.push(step["formation"].clone());
    }
    let port = |n: usize, side: &str| {
        format!(
            "{}:{}",
            steps[n]["id"].as_str().unwrap(),
            steps[n][side][0]["id"].as_str().unwrap()
        )
    };
    call(
        "mission_wire",
        json!({"mission":"line","input":input,"to":port(0, "inputs")}),
    )
    .await;
    call(
        "formation_wire",
        json!({"mission":"line","from":port(0, "outputs"),"to":port(1, "inputs")}),
    )
    .await;
    let end = call("end_create", json!({"mission":"line","title":"Done"})).await;
    let end = end["end"]["id"].as_str().unwrap().to_owned();
    call(
        "formation_wire",
        json!({"mission":"line","from":port(1, "outputs"),"to":format!("{end}:in")}),
    )
    .await;
    let places = |layout: &serde_json::Value| {
        let mut v: Vec<(String, i64, i64)> = layout["layout"]["nodes"]
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
        input.as_str(),
        steps[0]["id"].as_str().unwrap(),
        steps[1]["id"].as_str().unwrap(),
        end.as_str(),
    ];
    assert!(ids.windows(2).all(|w| x(w[0]) < x(w[1])));
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn cli_uses_the_same_mission_commands() {
    let node = Node::start(&[sessions::LISTING, ahjo_work::LISTING, LAB], "")
        .await
        .unwrap();
    let mut output = Vec::new();
    let args = vec![
        "new".to_string(),
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
        format!("ahjo mission inspect --node {}/ demo", node.url())
    );
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn end_create_help_names_the_outcomes_and_default() {
    let node = Node::start(&[sessions::LISTING, ahjo_work::LISTING, LAB], "")
        .await
        .unwrap();
    let args = vec![
        "create".to_owned(),
        "--help".to_owned(),
        "--node".to_owned(),
        format!("{}/", node.url()),
    ];
    let mut output = Vec::new();
    assert_eq!(
        ahjo_node_kit::cli::run("end", &args, &mut output)
            .await
            .unwrap(),
        0
    );
    let help = String::from_utf8(output).unwrap();
    assert!(
        help.contains("--outcome is done (the default) or rejected"),
        "{help}"
    );
    node.stop().await;
}

/// An agent building a one-seat mission names no seat and no port, and a
/// wrong name is answered with the names there are.
#[tokio::test(flavor = "multi_thread")]
async fn a_simple_mission_needs_no_seat_or_port_names() {
    let node = Node::start(&[sessions::LISTING, ahjo_work::LISTING, LAB], "")
        .await
        .unwrap();
    node.call("mission_new", json!({"slug":"plain"}))
        .await
        .unwrap();
    node.call("mission_create", json!({"mission":"plain"}))
        .await
        .unwrap();
    let step = node
        .call(
            "formation_create",
            json!({"mission":"plain","title":"Answer"}),
        )
        .await
        .unwrap();
    let step_id = step["formation"]["id"].as_str().unwrap().to_owned();
    let assigned = node
        .call(
            "formation_assign",
            json!({"mission":"plain","formation":"Answer","harness":"Claude","model":"haiku","effort":"low"}),
        )
        .await
        .unwrap();
    assert_eq!(assigned["changed"][0]["slots"][0]["harness"], "claude-code");
    assert!(assigned.get("formations").is_none() && assigned["mission"] == "plain");
    node.call("end_create", json!({"mission":"plain","title":"Done"}))
        .await
        .unwrap();
    node.call(
        "formation_wire",
        json!({"mission":"plain","from":"Answer","to":"Done"}),
    )
    .await
    .unwrap();
    // Without a wire from its Input card the mission cannot run, and
    // validate says so, with the command that fixes it.
    let unwired = node
        .call("mission_validate", json!({"mission":"plain"}))
        .await
        .unwrap();
    assert_eq!(unwired["errors"], json!([]), "{unwired}");
    assert_eq!(unwired["valid"], false, "{unwired}");
    assert!(
        unwired["summary"]
            .as_str()
            .unwrap()
            .starts_with("not runnable: "),
        "{unwired}"
    );
    assert_eq!(
        unwired["next"],
        format!("ahjo mission wire plain --to {step_id}")
    );
    node.call("mission_wire", json!({"mission":"plain","to":"Answer"}))
        .await
        .unwrap();
    let valid = node
        .call("mission_validate", json!({"mission":"plain"}))
        .await
        .unwrap();
    assert_eq!(
        (&valid["valid"], &valid["summary"], &valid["next"]),
        (
            &json!(true),
            &json!("valid"),
            &json!("ahjo mission run plain --inputs '{\"brief\":\"...\"}'")
        ),
        "{valid}"
    );
    let edges = node
        .call("mission_inspect", json!({"mission":"plain","full":true}))
        .await
        .unwrap()["mission"]["connections"]
        .to_string();
    assert!(edges.matches(&step_id).count() == 2, "{edges}");
    let wrong = node
        .call(
            "mission_run",
            json!({"mission":"plain","input":"topic=fog"}),
        )
        .await
        .unwrap_err()
        .to_string();
    assert!(
        wrong.contains("this mission has: Input") && wrong.contains("takes: brief"),
        "{wrong}"
    );
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn specialized_authoring_commands_build_and_edit_a_graph() {
    let node = Node::start(&[sessions::LISTING, ahjo_work::LISTING, LAB], "")
        .await
        .unwrap();
    node.call("mission_new", json!({"slug":"board"}))
        .await
        .unwrap();
    node.call("mission_rename", json!({"mission":"board","title":"Board"}))
        .await
        .unwrap();
    let input = node
        .call("mission_create", json!({"mission":"board","title":"Start"}))
        .await
        .unwrap();
    let input_id = input["inputCard"]["id"].as_str().unwrap().to_owned();
    node.call("mission_update", json!({"mission":"board","goal":"A goal"}))
        .await
        .unwrap();
    node.call(
        "mission_input",
        json!({"mission":"board","name":"topic","required":true}),
    )
    .await
    .unwrap();
    node.call("mission_output", json!({"mission":"board","name":"result"}))
        .await
        .unwrap();
    let step = node
        .call(
            "formation_create",
            json!({"mission":"board","type":"solo","title":"Work"}),
        )
        .await
        .unwrap();
    let step_id = step["formation"]["id"].as_str().unwrap().to_owned();
    assert!(
        node.call("formation_list", json!({"mission":"board"}))
            .await
            .unwrap()["formations"]
            .is_array()
    );
    assert_eq!(
        node.call(
            "formation_inspect",
            json!({"mission":"board","formation":step_id})
        )
        .await
        .unwrap()["formation"]["id"],
        step_id
    );
    node.call(
        "formation_rename",
        json!({"mission":"board","formation":step_id,"title":"Build"}),
    )
    .await
    .unwrap();
    node.call(
        "formation_place",
        json!({"mission":"board","formation":step_id,"place":"home"}),
    )
    .await
    .unwrap();
    node.call(
        "formation_set_type",
        json!({"mission":"board","formation":step_id,"type":"solo"}),
    )
    .await
    .unwrap();
    let mut cli_out = Vec::new();
    let cli_args = vec![
        "set-type".into(),
        "board".into(),
        step_id.clone(),
        "solo".into(),
        "--node".into(),
        format!("{}/", node.url()),
        "--json".into(),
    ];
    assert_eq!(
        ahjo_node_kit::cli::run("formation", &cli_args, &mut cli_out)
            .await
            .unwrap(),
        0
    );
    node.call(
        "formation_set_slots",
        json!({"mission":"board","formation":step_id,"slot":["seat=Worker"]}),
    )
    .await
    .unwrap();
    node.call("formation_assign",json!({"mission":"board","formation":step_id,"slot":"seat","harness":"claude-code","effort":"medium"})).await.unwrap();
    let client = ahjo_node_kit::NodeClient::new(Some(&format!("{}/", node.url()))).unwrap();
    client
        .request(
            axum::http::Method::POST,
            "/api/characters",
            Some(json!({"name":"Test stamp","harness":"claude-code","effort":"high"})),
            None,
        )
        .await
        .unwrap();
    let stamped = node
        .call(
            "formation_assign",
            json!({"mission":"board","formation":step_id,"slot":"seat","character":"test-stamp"}),
        )
        .await
        .unwrap();
    assert_eq!(stamped["changed"][0]["slots"][0]["effort"], "high");
    node.call(
        "formation_unassign",
        json!({"mission":"board","formation":step_id,"slot":"seat"}),
    )
    .await
    .unwrap();
    node.call(
        "formation_set_brief",
        json!({"mission":"board","formation":step_id,"goal":"Do the work"}),
    )
    .await
    .unwrap();
    let with_input = node
        .call(
            "formation_add_input",
            json!({"mission":"board","formation":step_id,"label":"Extra input"}),
        )
        .await
        .unwrap();
    let input_port = with_input["changed"][0]["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["label"] == "Extra input")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let with_output = node
        .call(
            "formation_add_output",
            json!({"mission":"board","formation":step_id,"label":"Extra output"}),
        )
        .await
        .unwrap();
    let output_port = with_output["changed"][0]["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["label"] == "Extra output")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    node.call(
        "formation_set_port",
        json!({"mission":"board","formation":step_id,"port":output_port,"label":"Result"}),
    )
    .await
    .unwrap();
    node.call(
        "mission_wire",
        json!({"mission":"board","input":input_id,"to":format!("{step_id}:{input_port}")}),
    )
    .await
    .unwrap();
    let end = node
        .call(
            "end_create",
            json!({"mission":"board","title":"Done","outcome":"done"}),
        )
        .await
        .unwrap();
    let end_id = end["end"]["id"].as_str().unwrap().to_owned();
    node.call(
        "end_update",
        json!({"mission":"board","end":end_id,"title":"Finished"}),
    )
    .await
    .unwrap();
    let from = format!("{step_id}:{output_port}");
    let to = format!("{end_id}:in");
    node.call(
        "formation_wire",
        json!({"mission":"board","from":from,"to":to}),
    )
    .await
    .unwrap();
    node.call(
        "formation_unwire",
        json!({"mission":"board","from":from,"to":to}),
    )
    .await
    .unwrap();
    let gate = node
        .call("gate_create", json!({"mission":"board","title":"Review"}))
        .await
        .unwrap();
    let gate_id = gate["gate"]["id"].as_str().unwrap().to_owned();
    node.call(
        "gate_update",
        json!({"mission":"board","gate":gate_id,"title":"Review work"}),
    )
    .await
    .unwrap();
    node.call(
        "gate_judge",
        json!({"mission":"board","gate":gate_id,"detach":true}),
    )
    .await
    .unwrap();
    let limit = node
        .call(
            "limit_create",
            json!({"mission":"board","target":step_id,"rounds":"2"}),
        )
        .await
        .unwrap();
    let limit_id = limit["limit"]["id"].as_str().unwrap().to_owned();
    node.call(
        "limit_update",
        json!({"mission":"board","limit":limit_id,"rounds":"3"}),
    )
    .await
    .unwrap();
    node.call(
        "mission_move",
        json!({"mission":"board","card":step_id,"x":100,"y":200}),
    )
    .await
    .unwrap();
    node.call("mission_arrange", json!({"mission":"board"}))
        .await
        .unwrap();
    node.call("limit_delete", json!({"mission":"board","limit":limit_id}))
        .await
        .unwrap();
    node.call("gate_delete", json!({"mission":"board","gate":gate_id}))
        .await
        .unwrap();
    node.call("end_delete", json!({"mission":"board","end":end_id}))
        .await
        .unwrap();
    node.call(
        "formation_delete",
        json!({"mission":"board","formation":step_id}),
    )
    .await
    .unwrap();
    node.call("mission_delete", json!({"mission":"board"}))
        .await
        .unwrap();
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn cli_lists_and_removes_deleted_mission_archives() {
    let node = Node::start(&[sessions::LISTING, ahjo_work::LISTING, LAB], "")
        .await
        .unwrap();
    node.call("mission_new", json!({"slug":"archived","title":"Archived"}))
        .await
        .unwrap();
    node.call(
        "mission_edit",
        json!({"mission":"archived","change":{"createInputCard":{"title":"Start","goal":"Work"}}}),
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
    assert!(
        help.contains("archive") && help.contains("purge-archive"),
        "{help}"
    );
    let deletion = cli(&node, &["delete", "archived"]).await;
    let archive = deletion["deletion"]["archiveId"].as_str().unwrap();
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
        .find(|entry| entry["archiveId"] == archive)
        .unwrap();
    assert_eq!(record["slug"], "archived");
    assert_eq!(record["mission"], true);
    assert_eq!(record["layout"], true);
    assert_eq!(record["notes"], true);
    // Reusing the slug must not let purging the earlier archive remove the
    // new definition, its notes, or any run history.
    node.call(
        "mission_new",
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
    let removed = cli(&node, &["purge-archive", "archived", archive]).await;
    assert_eq!(removed["removed"]["archiveId"], archive);
    assert!(
        cli(&node, &["archives"]).await["archives"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        node.call("mission_inspect", json!({"mission":"archived"}))
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
    let node = Node::start(&[sessions::LISTING, ahjo_work::LISTING, LAB], "")
        .await
        .unwrap();
    node.call(
        "mission_new",
        json!({"slug":"ship","example":"plan-build-check"}),
    )
    .await
    .unwrap();
    let note = |target: &str, text: &str| json!({"mission":"ship","target":target,"text":text,"author":"human:ui"});
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
        .call("mission_inspect", json!({"mission":"ship"}))
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
        .call("mission_inspect", json!({"mission":"ship"}))
        .await
        .unwrap();
    assert_eq!(read["notes"], "4 threads, 3 open");
    assert_eq!(read["about"][1]["resolves"], true);
    // A later note opens the thread again.
    node.call("mission_note", note("", "One more thing"))
        .await
        .unwrap();
    let read = node
        .call("mission_inspect", json!({"mission":"ship"}))
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
    let node = Node::start(&[sessions::LISTING, ahjo_work::LISTING, LAB], "")
        .await
        .unwrap();
    node.call(
        "mission_new",
        json!({"slug":"digest","steps":"Collect\nGroup; Write\n\n","note":"A Monday digest"}),
    )
    .await
    .unwrap();
    let read = node
        .call("mission_inspect", json!({"mission":"digest"}))
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
            "mission_new",
            json!({"slug":"both","steps":"One","example":"write-and-review"}),
        )
        .await
        .unwrap_err();
    assert_eq!(mixed.code, "bad_request");
    assert!(
        node.call("mission_inspect", json!({"mission":"both"}))
            .await
            .is_err()
    );
    node.stop().await;
}

/// A sketch as long as a person might type is made whole and laid out as
/// Arrange lays it out, every card in a place of its own.
#[tokio::test(flavor = "multi_thread")]
async fn a_long_sketch_is_made_whole_and_arranged() {
    let node = Node::start(&[sessions::LISTING, ahjo_work::LISTING, LAB], "")
        .await
        .unwrap();
    for count in [15, 60] {
        let slug = format!("long{count}");
        let steps: Vec<_> = (1..=count).map(|i| format!("Step {i}")).collect();
        let made = node
            .call("mission_new", json!({"slug":slug,"steps":steps.join("\n")}))
            .await
            .unwrap();
        assert_eq!(made["steps"].as_array().unwrap().len(), count, "{made}");
        let read = node
            .call("mission_inspect", json!({"mission":slug}))
            .await
            .unwrap();
        let last = read["steps"].as_array().unwrap().last().unwrap().clone();
        assert_eq!(last["next"], "end (done)", "{read}");
        let nodes = |layout: serde_json::Value| {
            let mut v: Vec<(String, i64, i64)> = (layout["layout"]["nodes"].as_array())
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
    let node = Node::start(&[sessions::LISTING, ahjo_work::LISTING, LAB], "")
        .await
        .unwrap();
    // The steps are made first; the note, too large to keep, fails last.
    let failed = node
        .call(
            "mission_new",
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
        .call("mission_inspect", json!({"mission":"half"}))
        .await
        .unwrap_err();
    assert_eq!(missing.code, "not_found");
    let archives = node.call("mission_archives", json!({})).await.unwrap();
    assert_eq!(archives["archives"], json!([]), "{archives}");
    // The name is free again.
    node.call("mission_new", json!({"slug":"half","steps":"Collect"}))
        .await
        .unwrap();
    node.stop().await;
}
