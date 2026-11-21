#[allow(dead_code)]
mod run_support;

use ahjo_mission_engine::{
    Coordinator, ExecutionSink, Executor, LabExecutor, RunSettings, StepExecution, StepResult,
};
use ahjo_mission_model::RunStart;
use run_support::Scratch;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

fn step(id: &str) -> String {
    format!(
        "[[step]]\nid = \"{id}\"\nformation = \"solo\"\ntitle = \"{id}\"\n[step.brief]\ngoal = \"Work\"\n[[step.input]]\nid = \"port_in\"\nlabel = \"Input\"\n[[step.output]]\nid = \"port_out\"\nlabel = \"Result\"\n[[step.seat]]\nid = \"seat_{id}\"\nlabel = \"Seat\"\nagent = \"builder\"\nharness = \"openai-codex\"\nmodel = \"\"\neffort = \"medium\"\n\n"
    )
}

fn connection(id: &str, from: &str, to: &str) -> String {
    format!("[[connection]]\nid = \"{id}\"\nfrom = \"{from}\"\nto = \"{to}\"\n\n")
}

#[derive(Clone, Default)]
struct Cycle {
    a_paths: Arc<Mutex<Vec<PathBuf>>>,
}

impl Executor for Cycle {
    fn execute_step(
        &self,
        request: StepExecution,
        sink: ExecutionSink,
    ) -> Result<StepResult, ahjo_mission_engine::Error> {
        let result = (|| -> Result<StepResult, String> {
            let text = match request.step.id.as_str() {
                "fmn_a" => {
                    self.a_paths.lock().unwrap().push(request.cwd.clone());
                    if request.attempt == 1 {
                        std::fs::write(request.cwd.join("value"), "A")
                            .map_err(|error| error.to_string())?;
                    } else {
                        let value = std::fs::read_to_string(request.cwd.join("value"))
                            .map_err(|error| error.to_string())?;
                        if value != "B" {
                            return Err(format!("A saw {value:?}, expected B"));
                        }
                    }
                    "A done".to_owned()
                }
                "fmn_b" => {
                    if request.attempt == 1 {
                        std::fs::write(request.cwd.join("value"), "B")
                            .map_err(|error| error.to_string())?;
                    }
                    "B done".to_owned()
                }
                "fmn_judge" => {
                    let verdict = if request.attempt == 1 { "fail" } else { "pass" };
                    format!(
                        "```ahjo-verdict\n{}\n```",
                        serde_json::json!({"verdict":verdict,"reason":"checked","evidence":[]})
                    )
                }
                other => return Err(format!("unexpected step {other}")),
            };
            let mut result = LabExecutor
                .execute_step(request, sink)
                .map_err(|error| error.message)?;
            result.text.clone_from(&text);
            for output in result.outputs.values_mut() {
                output["text"] = text.clone().into();
            }
            Ok(result)
        })();
        result.map_err(|message| ahjo_mission_engine::Error::new("executor_error", message))
    }
}

#[test]
fn returning_to_a_step_keeps_later_steps_changes() {
    let root = Scratch::new();
    let workspace = root.0.join("folder");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::write(workspace.join("value"), "original").unwrap();
    let mut mission = String::from(
        "schema = 6\nid = \"msn_cycle\"\nslug = \"hello\"\ntitle = \"Cycle\"\nrev = 1\n\n[[input_card]]\nid = \"inp_hello\"\ntitle = \"Start\"\ngoal = \"Start\"\n\n",
    );
    mission += &step("fmn_a");
    mission += &step("fmn_b");
    mission += &step("fmn_judge");
    mission += "[[gate]]\nid = \"gate_review\"\ntitle = \"Review\"\nkinds = [\"step\"]\ncriterion = \"Review\"\n\n[[end]]\nid = \"end_done\"\ntitle = \"Done\"\noutcome = \"done\"\n\n";
    mission += &connection("edge_start", "inp_hello:out", "fmn_a:port_in");
    mission += &connection("edge_ab", "fmn_a:port_out", "fmn_b:port_in");
    mission += &connection("edge_review", "fmn_b:port_out", "gate_review:in");
    mission += &connection("edge_pass", "gate_review:pass", "end_done:in");
    mission += &connection("edge_fail", "gate_review:fail", "fmn_a:port_in");
    mission += &connection("edge_judge", "gate_review:judge", "fmn_judge:port_in");
    mission += &connection("edge_return", "fmn_judge:port_out", "gate_review:judge");
    mission += "[[limit]]\nid = \"lim_cap\"\ntitle = \"Cap\"\nscope = \"inp_hello\"\nrounds = 6\nwhen_spent = \"fail\"\n";
    std::fs::write(root.0.join("missions/hello.mission.toml"), mission).unwrap();
    let executor = Cycle::default();
    let c = Coordinator::open(&root.0, RunSettings::default(), Box::new(executor.clone())).unwrap();
    let id = c
        .start_mission(RunStart {
            mission: "hello".into(),
            input_card_id: "inp_hello".into(),
            expected_rev: 1,
            cwd: workspace.to_string_lossy().into_owned(),
            inputs: BTreeMap::from([("brief".into(), "Work".into())]),
            ..Default::default()
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let projection = loop {
        let projection = c.run(&id).unwrap();
        if projection.r#final || projection.status == "blocked" {
            break projection;
        }
        assert!(Instant::now() < deadline, "run did not finish");
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(
        projection.status,
        "succeeded",
        "{}",
        std::fs::read_to_string(root.0.join("runs").join(format!("{id}.ndjson"))).unwrap()
    );
    let paths = executor.a_paths.lock().unwrap();
    assert_eq!(paths.len(), 2);
    assert_eq!(paths[0], paths[1]);
    assert_eq!(
        std::fs::read_to_string(workspace.join("value")).unwrap(),
        "B"
    );
}
