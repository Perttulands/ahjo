//! A two-step mission authored and driven through the same CLI as an operator.
#![cfg(unix)]
use ahjo_mission_engine::{
    ExecutionSink, Executor, FormationExecution, FormationResult, LabExecutor,
};
use ahjo_node_kit::{Built, Listing, Setup};
use ahjo_term::Sessions;
use ahjo_testkit::{
    node::Node,
    sessions::{self, FakeSessions},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

// The executor delivers each step to the testkit's fake Sessions, then uses
// the deterministic lab answer. It runs no real terminal or model.
static LOG: Mutex<Option<Arc<ahjo_node_kit::events::Log>>> = Mutex::new(None);
static FAKE: Mutex<Option<Arc<FakeSessions>>> = Mutex::new(None);
struct FakeSeatLab {
    runtime: tokio::runtime::Handle,
}
impl Executor for FakeSeatLab {
    fn execute_formation(
        &self,
        execution: FormationExecution,
        sink: ExecutionSink,
    ) -> Result<FormationResult, ahjo_core::Error> {
        if execution.formation.title == "Break" {
            return Err(ahjo_core::Error::new("test_failure", "test block"));
        }
        let sessions = FAKE
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| ahjo_core::Error::new("test", "fake Sessions unavailable"))?;
        let id = sessions.add(&format!("seat-{}", execution.formation.id));
        self.runtime
            .block_on(async {
                let pane = sessions.active_pane(&id).await?;
                sessions
                    .deliver(pane, execution.input_text.clone(), true)
                    .await?;
                Ok::<_, anyhow::Error>(())
            })
            .map_err(|e| ahjo_core::Error::new("test", e.to_string()))?;
        let judge = execution.formation.title == "Judge";
        let mut result = LabExecutor.execute_formation(execution, sink)?;
        if judge {
            let verdict =
                "```ahjo-verdict\n{\"verdict\":\"pass\",\"reason\":\"ready\",\"evidence\":[]}\n```";
            result.text = verdict.into();
            for output in result.outputs.values_mut() {
                output["text"] = verdict.into();
            }
        }
        Ok(result)
    }
}
fn build(setup: &Setup) -> anyhow::Result<Built> {
    *LOG.lock().unwrap() = Some(setup.events.clone());
    ahjo_missions::build_with_executor(
        setup,
        Some(Box::new(FakeSeatLab {
            runtime: tokio::runtime::Handle::current(),
        })),
    )
}
const MISSIONS: Listing = Listing {
    build,
    ..ahjo_missions::LISTING
};
async fn cli(node: &Node, noun: &str, args: &[&str]) -> Value {
    let mut args: Vec<String> = args.iter().map(|s| (*s).into()).collect();
    args.extend(["--node".into(), format!("{}/", node.url()), "--json".into()]);
    let mut output = Vec::new();
    let exit = ahjo_node_kit::cli::run(noun, &args, &mut output)
        .await
        .unwrap();
    assert_eq!(exit, 0, "{noun} {args:?}");
    serde_json::from_slice(&output).unwrap()
}
async fn edit(node: &Node, change: Value) -> Value {
    cli(
        node,
        "mission",
        &["edit", "relay", "--change", &change.to_string()],
    )
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn cli_authors_two_steps_answers_gate_and_finishes_with_fake_sessions() {
    let scratch = ahjo_testkit::fresh_dir("mission-notify-cli");
    let output = scratch.join("sent.jsonl");
    let command = scratch.join("notify");
    ahjo_testkit::tmux::write_script(
        &command,
        format!(
            "#!/bin/sh\ncat >> '{}'\nprintf '\\n' >> '{}'\n",
            output.display(),
            output.display()
        ),
    );
    let config = format!("[missions]\nnotify_command = '{}'\n", command.display());
    let node = Node::start(&[sessions::LISTING, ahjo_work::LISTING, MISSIONS], &config)
        .await
        .unwrap();
    let fake = node.fake_sessions();
    *FAKE.lock().unwrap() = Some(fake.clone());
    cli(&node, "mission", &["create", "relay", "--title", "Relay"]).await;
    let started = edit(
        &node,
        json!({"createInputCard": {"title":"Start","goal":"Relay the brief"}}),
    )
    .await;
    let card = started["input_card"]["id"].as_str().unwrap();
    let mut steps = Vec::new();
    for title in ["Draft", "Polish", "Judge"] {
        let result = edit(
            &node,
            json!({"createFormation":{"type":"solo","title":title}}),
        )
        .await;
        let step = &result["formation"];
        let id = step["id"].as_str().unwrap().to_owned();
        let slot = step["slot"][0]["id"].as_str().unwrap();
        edit(
            &node,
            json!({"assignSlot":{
                "formationId":id, "slotId":slot, "harness":"openai-codex", "effort":"medium"
            }}),
        )
        .await;
        edit(&node, json!({"setBrief":{"formationId":id,"goal":title}})).await;
        steps.push((
            format!("{id}:{}", step["input"][0]["id"].as_str().unwrap()),
            format!("{id}:{}", step["output"][0]["id"].as_str().unwrap()),
            id,
        ));
    }
    let judge_gate = edit(
        &node,
        json!({"createGate":{"title":"Judge review","kinds":["formation"]}}),
    )
    .await["gate"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    edit(
        &node,
        json!({"setGateJudge":{"gateId":judge_gate,"chain":[steps[2].2]}}),
    )
    .await;
    let gate = edit(
        &node,
        json!({"createGate":{"title":"Review","criterion":"Is this ready?"}}),
    )
    .await["gate"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let done = edit(&node, json!({"createEnd":{"outcome":"done"}})).await["end"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let rejected = edit(&node, json!({"createEnd":{"outcome":"rejected"}})).await["end"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    for (from, to) in [
        (format!("{card}:out"), steps[0].0.clone()),
        (steps[0].1.clone(), format!("{judge_gate}:in")),
        (format!("{judge_gate}:pass"), format!("{gate}:in")),
        (format!("{judge_gate}:fail"), format!("{rejected}:in")),
        (format!("{gate}:pass"), steps[1].0.clone()),
        (format!("{gate}:fail"), format!("{rejected}:in")),
        (steps[1].1.clone(), format!("{done}:in")),
    ] {
        edit(&node, json!({"wireConnection":{"from":from,"to":to}})).await;
    }
    let started = cli(
        &node,
        "mission",
        &["run", "relay", "--inputs", r#"{"brief":"hello"}"#],
    )
    .await;
    let run = started["run_id"].as_str().unwrap();
    let waiting = cli(&node, "run", &["wait", run, "--timeout", "10"]).await;
    assert_eq!(waiting["outcome"], "needs-you", "{waiting}");
    assert_eq!(waiting["asks"][0]["gate"], gate);
    let shown = cli(&node, "run", &["show", run]).await;
    let human_gate = shown["gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["gate"] == gate)
        .unwrap();
    assert_eq!(human_gate["state"], "waiting");
    assert!(
        shown["gates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["gate"] == judge_gate && g["state"] == "answered")
    );
    let request = &shown["asks"][0]["request"];
    assert_eq!(shown["asks"][0]["seq"], request["requested_seq"]);
    assert_eq!(shown["next"], waiting["next"]);
    assert!(
        shown["why"]
            .as_str()
            .unwrap()
            .contains("waits for approval")
    );
    for section in ["events", "seats", "attempts", "evidence"] {
        assert!(shown.get(section).is_none(), "{shown}");
    }
    assert_eq!(request["criterion"], "Is this ready?");
    assert_eq!(request["inputs"][0]["from_node_id"], steps[0].2);
    assert_eq!(request["routes"][0]["targets"][0]["node_id"], steps[1].2);
    assert_eq!(request["routes"][0]["ends_run"], false);
    assert_eq!(request["routes"][1]["targets"][0]["node_id"], rejected);
    assert_eq!(request["routes"][1]["ends_run"], true);
    assert_eq!(request["ends_run"], true);
    assert_eq!(request["picks_best"], false);
    let message = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Ok(text) = std::fs::read_to_string(&output)
                && let Some(line) = text.lines().find(|line| !line.is_empty())
                && let Ok(value) = serde_json::from_str::<Value>(line)
            {
                break value;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("notification delivered");
    assert_eq!(message["runId"], run);
    assert_eq!(message["missionSlug"], "relay");
    assert_eq!(message["gateId"], gate);
    assert_eq!(message["ask"], "Is this ready?");
    assert_eq!(std::fs::read_to_string(&output).unwrap().lines().count(), 1);
    assert!(
        LOG.lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .listen()
            .0
            .iter()
            .any(|event| event.text.starts_with("run needs you:"))
    );
    let cursor = waiting["cursor"].as_u64().unwrap();
    let seq = request["requested_seq"].as_u64().unwrap();
    // The suggestion reaches the node the command named.
    let expected = format!(
        "ahjo run approve --node {}/ {run} {gate} --seq {seq}",
        node.url()
    );
    assert!(
        waiting["next"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == &expected)
    );
    let mut output = Vec::new();
    let args = vec![
        "wait".into(),
        run.into(),
        "--until".into(),
        "needs-you".into(),
        "--node".into(),
        format!("{}/", node.url()),
    ];
    assert_eq!(
        ahjo_node_kit::cli::run("run", &args, &mut output)
            .await
            .unwrap(),
        0
    );
    assert!(String::from_utf8(output).unwrap().contains(&expected));
    let timed = cli(
        &node,
        "run",
        &[
            "wait",
            run,
            "--since",
            &cursor.to_string(),
            "--timeout",
            "0",
        ],
    )
    .await;
    assert_eq!(timed["outcome"], "timeout");
    assert_eq!(timed["status"], "waiting_human");
    let mut help = Vec::new();
    let address = format!("{}/", node.url());
    let args = ["approve", "--help", "--node", &address].map(str::to_owned);
    assert_eq!(
        ahjo_node_kit::cli::run("run", &args, &mut help)
            .await
            .unwrap(),
        0
    );
    let help = String::from_utf8(help).unwrap();
    assert!(help.contains("--reason") && help.contains("seq"), "{help}");
    assert!(!help.contains("--response"), "{help}");
    let args = [
        "approve",
        run,
        &gate,
        &seq.to_string(),
        "--response",
        "old flag",
        "--node",
        &address,
    ]
    .map(str::to_owned);
    let error = ahjo_node_kit::cli::run("run", &args, &mut Vec::new())
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("unknown flag --response"),
        "{error}"
    );

    cli(
        &node,
        "run",
        &[
            "approve",
            run,
            &gate,
            "--seq",
            &seq.to_string(),
            "--reason",
            "approved",
            "--actor",
            "agent:journey",
        ],
    )
    .await;
    let shown = cli(&node, "run", &["show", run]).await;
    let human_gate = shown["gates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|g| g["gate"] == gate)
        .unwrap();
    assert_eq!(human_gate["state"], "answered");
    assert_eq!(human_gate["verdict"], "pass");
    let ledger = ahjo_mission_engine::Store::root_in(node.dir())
        .join("runs")
        .join(format!("{run}.ndjson"));
    let events = ahjo_mission_engine::Ledger::new(ledger, run)
        .read()
        .unwrap();
    let verdict = events
        .iter()
        .find(|event| event.kind == "human_verdict_recorded" && event.gate_id == gate)
        .unwrap();
    assert_eq!(verdict.data["reason"], "approved");
    assert_eq!(human_gate["by"], "agent:journey");
    assert!(
        shown["asks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|ask| ask["gate"] != gate)
    );
    let stale = node
        .call("run_approve", json!({"run":run,"gate":gate,"seq":seq}))
        .await
        .unwrap_err();
    assert_eq!(stale.code, "conflict");
    let finished = cli(
        &node,
        "run",
        &["wait", run, "--until", "final", "--timeout", "10"],
    )
    .await;
    assert_eq!(finished["outcome"], "final");
    assert_eq!(finished["status"], "succeeded", "{finished}");
    let again = cli(
        &node,
        "run",
        &["wait", run, "--until", "final", "--timeout", "0"],
    )
    .await;
    assert_eq!(again["outcome"], "final");
    let status = cli(&node, "run", &["show", run, "--events"]).await;
    for (_, _, id) in &steps {
        assert!(
            status["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["type"] == "node_output" && e["nodeId"] == *id),
            "{status}"
        );
    }
    let folder = node.dir().join("missions/missions");
    std::fs::write(
        folder.join("broken.mission.toml"),
        include_str!(
            "../../../../engine/missions/tests/parity/runs/missions/solo-lab.mission.toml"
        )
        .replace("slug = \"hello\"", "slug = \"broken\"")
        .replace("id = \"msn_hello\"", "id = \"msn_broken\"")
        .replace("title = \"Work\"", "title = \"Break\""),
    )
    .unwrap();
    let broken = cli(
        &node,
        "mission",
        &["run", "broken", "--inputs", r#"{"brief":"hi"}"#],
    )
    .await;
    let broken_id = broken["run_id"].as_str().unwrap();
    let block = cli(&node, "run", &["wait", broken_id, "--timeout", "10"]).await;
    assert_eq!(block["status"], "blocked");
    assert!(
        LOG.lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .listen()
            .0
            .iter()
            .any(|event| event.text.starts_with("run blocked:"))
    );
    let delivered = fake.delivered();
    assert_eq!(delivered.len(), 3);
    assert!(delivered.iter().all(|d| d.enter));
    *FAKE.lock().unwrap() = None;
    node.stop().await;
    *LOG.lock().unwrap() = None;
    std::fs::remove_dir_all(scratch).unwrap();
}
