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
                    let value = if request.attempt == 1 { "B" } else { "B again" };
                    std::fs::write(request.cwd.join("value"), value)
                        .map_err(|error| error.to_string())?;
                    "B done".to_owned()
                }
                "fmn_judge" => {
                    let value = std::fs::read_to_string(request.cwd.join("value"))
                        .map_err(|error| error.to_string())?;
                    let expected = if request.attempt == 1 { "B" } else { "B again" };
                    if value != expected {
                        return Err(format!("judge saw {value:?}, expected {expected:?}"));
                    }
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

/// A builder whose attempts a gate reviews, and a judge that keeps notes in
/// its own folder as a critic does (`.critic-round-N/`).
#[derive(Clone)]
struct Critic {
    run_folder: PathBuf,
    judge_saw_earlier_notes: Arc<Mutex<Vec<bool>>>,
}

impl Executor for Critic {
    fn execute_step(
        &self,
        request: StepExecution,
        sink: ExecutionSink,
    ) -> Result<StepResult, ahjo_mission_engine::Error> {
        let result = (|| -> Result<StepResult, String> {
            let attempt = request.attempt;
            let text = match request.step.id.as_str() {
                "fmn_work" => {
                    std::fs::write(request.cwd.join("work"), format!("attempt {attempt}"))
                        .map_err(|error| error.to_string())?;
                    "built".to_owned()
                }
                "fmn_judge" => {
                    if request.cwd.canonicalize().ok() == self.run_folder.canonicalize().ok() {
                        return Err("the judge started in the run folder".into());
                    }
                    self.judge_saw_earlier_notes
                        .lock()
                        .unwrap()
                        .push((1..attempt).all(|round| {
                            request
                                .cwd
                                .join(format!(".critic-round-{round}/notes"))
                                .exists()
                        }));
                    let notes = request.cwd.join(format!(".critic-round-{attempt}"));
                    std::fs::create_dir_all(&notes).map_err(|error| error.to_string())?;
                    std::fs::write(notes.join("notes"), "looked")
                        .map_err(|error| error.to_string())?;
                    let verdict = if attempt == 1 { "fail" } else { "pass" };
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
fn a_judges_own_files_never_reach_the_run_folder_or_block_the_attempt_it_passes() {
    let root = Scratch::new();
    let workspace = root.0.join("folder");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::write(workspace.join("work"), "original").unwrap();
    let mut mission = String::from(
        "schema = 6\nid = \"msn_critic\"\nslug = \"hello\"\ntitle = \"Critic\"\nrev = 1\n\n[[input_card]]\nid = \"inp_hello\"\ntitle = \"Start\"\ngoal = \"Start\"\n\n",
    );
    mission += &step("fmn_work");
    mission += &step("fmn_judge");
    mission += "[[gate]]\nid = \"gate_review\"\ntitle = \"Review\"\nkinds = [\"step\"]\ncriterion = \"Review\"\n\n[[end]]\nid = \"end_done\"\ntitle = \"Done\"\noutcome = \"done\"\n\n";
    mission += &connection("edge_start", "inp_hello:out", "fmn_work:port_in");
    mission += &connection("edge_review", "fmn_work:port_out", "gate_review:in");
    mission += &connection("edge_pass", "gate_review:pass", "end_done:in");
    mission += &connection("edge_fail", "gate_review:fail", "fmn_work:port_in");
    mission += &connection("edge_judge", "gate_review:judge", "fmn_judge:port_in");
    mission += &connection("edge_return", "fmn_judge:port_out", "gate_review:judge");
    mission += "[[limit]]\nid = \"lim_cap\"\ntitle = \"Cap\"\nscope = \"inp_hello\"\nrounds = 6\nwhen_spent = \"fail\"\n";
    std::fs::write(root.0.join("missions/hello.mission.toml"), mission).unwrap();
    let executor = Critic {
        run_folder: workspace.clone(),
        judge_saw_earlier_notes: Arc::default(),
    };
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
    assert_eq!(
        std::fs::read_to_string(workspace.join("work")).unwrap(),
        "attempt 2"
    );
    let mut names: Vec<_> = std::fs::read_dir(&workspace)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    names.sort();
    assert_eq!(names, ["work"], "only the builder's work is published");
    // The judge's folder is its own: a later round still finds its notes.
    assert_eq!(
        *executor.judge_saw_earlier_notes.lock().unwrap(),
        [true, true]
    );
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
        "B again"
    );
}
