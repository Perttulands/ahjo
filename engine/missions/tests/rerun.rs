//! A step of an earlier run started again with something changed: the same
//! input and the same starting commit, side by side with the original.
use ahjo_mission_engine::{
    AdmissionFailure, Coordinator, ExecutionSink, Executor, LabExecutor, RunSettings,
    StepExecution, StepResult,
};
use ahjo_mission_model::RunStart;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let mut b = [0; 8];
        getrandom::fill(&mut b).unwrap();
        let p = std::env::temp_dir().join(format!("ahjo-rerun-{}", u64::from_ne_bytes(b)));
        std::fs::create_dir_all(p.join("missions")).unwrap();
        Self(p)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

/// Two steps in a row, `fmn_second`'s seat on `second_model`.
fn mission(rev: i64, second_model: &str) -> String {
    let lab = include_str!("parity/runs/missions/solo-lab.mission.toml");
    let step = lab
        .split("[[step]]")
        .nth(1)
        .unwrap()
        .split("[[end]]")
        .next()
        .unwrap();
    let second = format!(
        "[[step]]{}",
        step.replace("fmn_work", "fmn_second")
            .replace("seat_work", "seat_second")
            .replace(
                "model = \"\" # harness default",
                &format!("model = \"{second_model}\"")
            )
    );
    let mission = lab
        .replace("rev = 1", &format!("rev = {rev}"))
        .replace("[[end]]", &format!("{second}[[end]]"))
        .replace("to = \"end_done:in\"", "to = \"fmn_second:port_in\"");
    format!(
        "{mission}\n[[connection]]\nid = \"edge_second\"\nfrom = \"fmn_second:port_out\"\nto = \"end_done:in\"\n"
    )
}

#[derive(Clone, Debug, PartialEq)]
struct Seen {
    run_id: String,
    step_id: String,
    input_text: String,
    head: String,
    model: String,
}

/// Records what each step was given and where it started, then writes a
/// file so each step leaves a commit.
struct Recording(Arc<Mutex<Vec<Seen>>>);
impl Executor for Recording {
    fn execute_step(
        &self,
        req: StepExecution,
        sink: ExecutionSink,
    ) -> Result<StepResult, ahjo_mission_engine::Error> {
        self.0.lock().unwrap().push(Seen {
            run_id: req.run_id.clone(),
            step_id: req.step.id.clone(),
            input_text: req.input_text.clone(),
            head: git(&req.cwd, &["rev-parse", "HEAD"]),
            model: req.step.seats[0].model.clone(),
        });
        std::fs::write(req.cwd.join(&req.step.id), &req.run_id)
            .map_err(|e| ahjo_mission_engine::Error::new("executor_error", e.to_string()))?;
        LabExecutor.execute_step(req, sink)
    }
}

fn wait_final(c: &Coordinator, id: &str) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !c.run(id).unwrap().r#final {
        assert!(Instant::now() < deadline, "run {id} did not end");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn checkout(dir: &Path) -> PathBuf {
    let checkout = dir.join("checkout");
    std::fs::create_dir(&checkout).unwrap();
    git(&checkout, &["init", "-b", "main"]);
    git(&checkout, &["config", "user.email", "test@example.invalid"]);
    git(&checkout, &["config", "user.name", "Test"]);
    std::fs::write(checkout.join("base"), "base").unwrap();
    git(&checkout, &["add", "base"]);
    git(&checkout, &["commit", "-m", "base"]);
    checkout
}

#[test]
fn a_step_runs_again_with_another_model_from_the_same_input_and_commit() {
    let dir = Scratch::new();
    let mission_path = dir.0.join("missions/hello.mission.toml");
    std::fs::write(&mission_path, mission(1, "model-a")).unwrap();
    let checkout = checkout(&dir.0);
    let log = Arc::new(Mutex::new(Vec::new()));
    let c = Coordinator::open(
        &dir.0,
        RunSettings {
            repositories: BTreeMap::from([("owner/repo".into(), checkout.clone())]),
            ..Default::default()
        },
        Box::new(Recording(log.clone())),
    )
    .unwrap();
    let earlier = c
        .start_mission(RunStart {
            mission: "hello".into(),
            input_card_id: "inp_hello".into(),
            expected_rev: 1,
            repository: "owner/repo".into(),
            issue: "gh:owner/repo#7".into(),
            inputs: BTreeMap::from([("brief".into(), "hello".into())]),
            ..Default::default()
        })
        .unwrap();
    wait_final(&c, &earlier);
    assert_eq!(c.run(&earlier).unwrap().status, "succeeded");

    // The base branch moves on, and the seat gets another model.
    std::fs::write(checkout.join("later"), "later").unwrap();
    git(&checkout, &["add", "later"]);
    git(&checkout, &["commit", "-m", "later"]);
    std::fs::write(&mission_path, mission(2, "model-b")).unwrap();

    let rerun = RunStart {
        mission: "hello".into(),
        step_id: "fmn_second".into(),
        expected_rev: 2,
        rerun_of: earlier.clone(),
        ..Default::default()
    };
    let again = c.start_step(rerun.clone()).unwrap();
    wait_final(&c, &again);
    let run = c.run(&again).unwrap();
    assert_eq!(run.status, "succeeded");
    assert_eq!(run.rerun_of, earlier);
    assert_eq!(run.repository, "owner/repo");
    assert_eq!(run.base, "main");
    assert_eq!(run.issue, "gh:owner/repo#7");
    assert_eq!(run.inputs, c.run(&earlier).unwrap().inputs);

    let second = |run: &str| {
        let seen = log.lock().unwrap();
        let found: Vec<_> = seen
            .iter()
            .filter(|s| s.run_id == run && s.step_id == "fmn_second")
            .collect();
        assert_eq!(found.len(), 1, "{seen:?}");
        found[0].clone()
    };
    let (before, after) = (second(&earlier), second(&again));
    assert!(!before.input_text.is_empty());
    assert_eq!(after.input_text, before.input_text);
    assert_eq!(after.head, before.head);
    assert_ne!(after.head, git(&checkout, &["rev-parse", "main"]));
    assert_eq!(
        (before.model.as_str(), after.model.as_str()),
        ("model-a", "model-b")
    );
    // The earlier run's first step is not run again.
    assert!(
        !log.lock()
            .unwrap()
            .iter()
            .any(|s| s.run_id == again && s.step_id != "fmn_second")
    );

    // Both runs' attempts at the step, each under its own run id.
    for id in [&earlier, &again] {
        let attempts = c.attempts(id, Some("fmn_second")).unwrap();
        assert_eq!(&attempts.run_id, id);
        assert_eq!(attempts.steps.len(), 1);
        assert_eq!(attempts.steps[0].attempts.len(), 1);
    }

    // A rerun of the rerun starts where both did, with the same input.
    let third = c
        .start_step(RunStart {
            rerun_of: again.clone(),
            ..rerun.clone()
        })
        .unwrap();
    wait_final(&c, &third);
    assert_eq!(c.run(&third).unwrap().status, "succeeded");
    let third = second(&third);
    assert_eq!(
        (third.input_text.as_str(), third.head.as_str()),
        (before.input_text.as_str(), before.head.as_str())
    );

    // What a rerun takes from the earlier run is not given again.
    let given = c.start_step(RunStart {
        inputs: BTreeMap::from([("brief".into(), "other".into())]),
        ..rerun.clone()
    });
    assert!(
        matches!(given, Err(AdmissionFailure::Invalid(_))),
        "{given:?}"
    );
    let unknown_step = c.start_step(RunStart {
        step_id: "fmn_work".into(),
        rerun_of: again.clone(),
        ..rerun.clone()
    });
    assert!(
        matches!(unknown_step, Err(AdmissionFailure::NotFound(_))),
        "{unknown_step:?}"
    );
    let no_run = c.start_step(RunStart {
        rerun_of: "run_none".into(),
        ..rerun
    });
    assert!(
        matches!(no_run, Err(AdmissionFailure::NotFound(_))),
        "{no_run:?}"
    );
}

#[test]
fn a_folder_run_cannot_start_a_step_again() {
    let dir = Scratch::new();
    std::fs::write(
        dir.0.join("missions/hello.mission.toml"),
        mission(1, "model-a"),
    )
    .unwrap();
    let c = Coordinator::open(&dir.0, RunSettings::default(), Box::new(LabExecutor)).unwrap();
    let earlier = c
        .start_mission(RunStart {
            mission: "hello".into(),
            input_card_id: "inp_hello".into(),
            expected_rev: 1,
            inputs: BTreeMap::from([("brief".into(), "hello".into())]),
            ..Default::default()
        })
        .unwrap();
    wait_final(&c, &earlier);
    let again = c.start_step(RunStart {
        mission: "hello".into(),
        step_id: "fmn_second".into(),
        expected_rev: 1,
        rerun_of: earlier,
        ..Default::default()
    });
    match again {
        Err(AdmissionFailure::Invalid(message)) => assert!(message.contains("folder"), "{message}"),
        other => panic!("{other:?}"),
    }
}
