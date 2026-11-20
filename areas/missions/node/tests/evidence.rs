//! Evidence and attempts through the same CLI and HTTP node as an operator.
use ahjo_mission_engine::{ExecutionSink, Executor, LabExecutor, StepExecution, StepResult};
use ahjo_node_kit::{Built, Listing, Setup};
use ahjo_testkit::{node::Node, sessions};
use serde_json::{Value, json};
use std::time::Duration;

struct Scripted;
impl Executor for Scripted {
    fn execute_step(
        &self,
        request: StepExecution,
        sink: ExecutionSink,
    ) -> Result<StepResult, ahjo_core::Error> {
        let judge = request.step.id == "fmn_judge";
        let attempt = request.attempt;
        let pick = !request.pick_attempts.is_empty();
        let best = request
            .step
            .brief
            .as_ref()
            .is_some_and(|b| b.goal.contains("alwaysfail"));
        let mut result = LabExecutor.execute_step(request, sink)?;
        if judge {
            let text = if pick {
                "```ahjo-pick\n{\"attempt\":2,\"reason\":\"best\",\"evidence\":[]}\n```"
            } else if attempt == 1 || best {
                "```ahjo-verdict\n{\"verdict\":\"fail\",\"reason\":\"add coverage\",\"evidence\":[]}\n```"
            } else {
                "```ahjo-verdict\n{\"verdict\":\"pass\",\"reason\":\"covered\",\"evidence\":[]}\n```"
            };
            result.text = text.into();
            for output in result.outputs.values_mut() {
                output["text"] = json!(text);
            }
        }
        Ok(result)
    }
}
fn build(setup: &Setup) -> anyhow::Result<Built> {
    ahjo_missions::build_with_executor(setup, Some(Box::new(Scripted)))
}
const LAB: Listing = Listing {
    build,
    ..ahjo_missions::LISTING
};
fn mission(best: bool) -> String {
    let solo = include_str!(
        "../../../../engine/missions/tests/parity/runs/missions/solo-lab.mission.toml"
    );
    let judge = solo
        .split("[[step]]")
        .nth(1)
        .unwrap()
        .split("[[end]]")
        .next()
        .unwrap()
        .replace("fmn_work", "fmn_judge")
        .replace("slot_work", "slot_judge")
        .replace(
            "Return a short result using the supplied output contract.",
            if best { "alwaysfail" } else { "judge" },
        );
    let work = solo
        .replace("label = \"Result\"", "label = \"Result\"\nkind = \"file\"")
        .replace("to = \"end_done:in\"", "to = \"gate_review:in\"");
    format!(
        "{work}\n[[step]]{judge}\n[[gate]]\nid = \"gate_review\"\ntitle = \"Review\"\nkinds = [\"step\"]\n[[connection]]\nid = \"edge_pass\"\nfrom = \"gate_review:pass\"\nto = \"end_done:in\"\n[[connection]]\nid = \"edge_fail\"\nfrom = \"gate_review:fail\"\nto = \"fmn_work:port_in\"\n[[connection]]\nid = \"edge_judge\"\nfrom = \"gate_review:judge\"\nto = \"fmn_judge:port_in\"\n[[connection]]\nid = \"edge_return\"\nfrom = \"fmn_judge:port_out\"\nto = \"gate_review:judge\"\n{}",
        if best {
            "[[limit]]\nid = \"lim_cap\"\ntitle = \"Cap\"\ntarget = \"fmn_work\"\nrounds = 2\nwhen_spent = \"best\"\n"
        } else {
            ""
        }
    )
}
async fn cli(node: &Node, args: &[&str]) -> Value {
    let mut args: Vec<String> = args.iter().map(|s| (*s).into()).collect();
    args.extend(["--node".into(), format!("{}/", node.url()), "--json".into()]);
    let mut out = Vec::new();
    assert_eq!(
        ahjo_node_kit::cli::run("run", &args, &mut out)
            .await
            .unwrap(),
        0
    );
    serde_json::from_slice(&out).unwrap()
}
async fn start(node: &Node, mission: String) -> String {
    let root = node.dir().join("missions/missions");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("hello.mission.toml"), mission).unwrap();
    let answer = node
        .call(
            "mission_run",
            json!({"mission":"hello","input":"inp_hello","inputs":{"brief":"ship"},"cwd":node.dir()}),
        )
        .await
        .unwrap();
    answer["run_id"].as_str().unwrap().to_owned()
}

async fn run(node: &Node, best: bool) -> String {
    let id = start(node, mission(best)).await;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let answer = node.call("run_show", json!({"run":id})).await.unwrap();
            let status = &answer["state"];
            if status["final"] == true || status["status"] == "blocked" {
                assert!(
                    status["status"] == "succeeded" || status["status"] == "picked",
                    "{status}"
                );
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    id
}
#[tokio::test(flavor = "multi_thread")]
async fn cli_and_http_report_problems_briefs_files_and_picked_attempts() {
    let node = Node::start(&[sessions::LISTING, ahjo_work::LISTING, LAB], "")
        .await
        .unwrap();
    let id = run(&node, false).await;
    let evidence = cli(&node, &["show", &id, "--evidence"]).await;
    let evidence = &evidence["evidence"];
    assert!(
        evidence["problems"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["reason"]["text"] == "add coverage")
    );
    for step in ["fmn_work", "fmn_judge"] {
        assert!(
            evidence["briefs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|b| b["node_id"] == step
                    && std::path::Path::new(b["path"].as_str().unwrap()).is_absolute())
        );
    }
    assert!(evidence["files"].as_array().unwrap().iter().any(|f| {
        let path = std::path::Path::new(f["path"].as_str().unwrap());
        f["node_id"] == "fmn_work"
            && path.is_absolute()
            && path.metadata().unwrap().len() == f["size"].as_u64().unwrap()
    }));
    let filtered = cli(&node, &["show", &id, "--evidence", "--step", "fmn_judge"]).await;
    assert!(
        filtered["evidence"]["briefs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|b| b["node_id"] == "fmn_judge")
    );
    assert!(
        filtered["evidence"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .all(|b| b["node_id"] == "fmn_judge")
    );
    let client = ahjo_node_kit::NodeClient::new(Some(&format!("{}/", node.url()))).unwrap();
    let (http, _) = client
        .request(
            axum::http::Method::GET,
            &format!("/api/runs/{id}/evidence?step=fmn_judge"),
            None,
            None,
        )
        .await
        .unwrap();
    assert_eq!(http["evidence"], filtered["evidence"]);
    let missing = node
        .call("run_show", json!({"run":"run_missing","evidence":true}))
        .await
        .unwrap_err();
    assert_eq!(missing.code, "unknown_run");
    let mut out = Vec::new();
    let args = vec![
        "show".into(),
        "run_missing".into(),
        "--evidence".into(),
        "--node".into(),
        format!("{}/", node.url()),
        "--json".into(),
    ];
    assert_eq!(
        ahjo_node_kit::cli::run("run", &args, &mut out)
            .await
            .unwrap(),
        1
    );
    let pick = run(&node, true).await;
    let attempts = cli(&node, &["show", &pick, "--attempts", "--step", "fmn_work"]).await;
    let steps = attempts["attempts"]["steps"].as_array().unwrap();
    assert_eq!(steps.len(), 1);
    let attempts = steps[0]["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 2);
    assert!(
        attempts
            .iter()
            .all(|a| a["verdicts"][0]["verdict"] == "fail")
    );
    assert!(attempts[0]["picked"].is_null());
    assert_eq!(attempts[1]["picked"]["trigger"], "when_spent");
    node.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn show_puts_a_spent_limit_and_its_resume_command_first() {
    let node = Node::start(&[sessions::LISTING, ahjo_work::LISTING, LAB], "")
        .await
        .unwrap();
    let draft = format!(
        "{}\n[[limit]]\nid = \"lim_stop\"\ntitle = \"Try once\"\ntarget = \"fmn_work\"\nrounds = 1\n",
        mission(false)
    );
    let id = start(&node, draft).await;
    let waiting = cli(&node, &["wait", &id, "--timeout", "10"]).await;
    assert_eq!(waiting["status"], "blocked", "{waiting}");
    let shown = cli(&node, &["show", &id]).await;
    assert_eq!(shown["state"]["block_code"], "limit_reached");
    assert_eq!(shown["next"], waiting["next"]);
    let reason = shown["why"].as_str().unwrap();
    assert!(reason.contains("round"), "{shown}");
    let address = format!("{}/", node.url());
    let expected = format!("ahjo run resume --node {address} {id} --grant --reason '<reason>'");
    assert_eq!(shown["next"][0], expected);
    let args = ["show", &id, "--node", &address].map(str::to_owned);
    let mut output = Vec::new();
    assert_eq!(
        ahjo_node_kit::cli::run("run", &args, &mut output)
            .await
            .unwrap(),
        0
    );
    let output = String::from_utf8(output).unwrap();
    assert_eq!(
        output.lines().next(),
        Some(format!("why: {reason}").as_str())
    );
    assert!(
        output.find(&expected).unwrap() < output.find("state").unwrap(),
        "{output}"
    );
    let resumed = cli(
        &node,
        &["resume", &id, "--grant", "--reason", "one more review"],
    )
    .await;
    assert_eq!(resumed["run"]["run_id"], id);
    let finished = cli(&node, &["wait", &id, "--until", "final", "--timeout", "10"]).await;
    assert_eq!(finished["status"], "succeeded", "{finished}");
    node.stop().await;
}
