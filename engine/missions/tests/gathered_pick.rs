//! Sibling steps started together feed one gate, which picks one output:
//! only the picked step's work reaches the run, and the others stay in the
//! run's attempts.
#[allow(dead_code)]
mod run_support;
use ahjo_mission_engine::{
    Coordinator, ExecutionSink, Executor, LabExecutor, RunSettings, StepExecution, StepResult,
};
use ahjo_mission_model::{HumanPick, RunProjection, RunStart};
use run_support::Scratch;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

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
    String::from_utf8(out.stdout).unwrap().trim().into()
}

fn step(id: &str, title: &str) -> String {
    format!(
        "[[step]]\nid = \"{id}\"\nformation = \"solo\"\ntitle = \"{title}\"\n[step.brief]\ngoal = \"Write the draft.\"\n[[step.input]]\nid = \"port_in\"\nlabel = \"Input\"\n[[step.output]]\nid = \"port_out\"\nlabel = \"Result\"\n[[step.seat]]\nid = \"seat_{id}\"\nlabel = \"Seat\"\nagent = \"builder\"\nharness = \"openai-codex\"\nmodel = \"\"\neffort = \"medium\"\n\n"
    )
}

fn connection(id: &str, from: &str, to: &str) -> String {
    format!("[[connection]]\nid = \"{id}\"\nfrom = \"{from}\"\nto = \"{to}\"\n\n")
}

/// Input -> Draft A and Draft B side by side -> Pick (pass Done, fail
/// Rejected), decided by the given kinds; a step kind has the judge Judge.
fn race(kinds: &str) -> String {
    let mut mission = String::from(
        "schema = 5\nid = \"msn_race\"\nslug = \"hello\"\ntitle = \"Race\"\nrev = 1\n\n[[input_card]]\nid = \"inp_hello\"\ntitle = \"Hello\"\ngoal = \"Draft it\"\n\n",
    );
    mission += &step("fmn_a", "Draft A");
    mission += &step("fmn_b", "Draft B");
    mission += &format!(
        "[[gate]]\nid = \"gate_pick\"\ntitle = \"Pick\"\nkinds = {kinds}\ncriterion = \"The best draft\"\n\n[[end]]\nid = \"end_done\"\ntitle = \"Done\"\noutcome = \"done\"\n\n[[end]]\nid = \"end_rejected\"\ntitle = \"Rejected\"\noutcome = \"rejected\"\n\n"
    );
    mission += &connection("edge_a", "inp_hello:out", "fmn_a:port_in");
    mission += &connection("edge_b", "inp_hello:out", "fmn_b:port_in");
    mission += &connection("edge_a_out", "fmn_a:port_out", "gate_pick:in");
    mission += &connection("edge_b_out", "fmn_b:port_out", "gate_pick:in");
    mission += &connection("edge_pass", "gate_pick:pass", "end_done:in");
    mission += &connection("edge_fail", "gate_pick:fail", "end_rejected:in");
    if kinds.contains("step") {
        mission += &step("fmn_judge", "Judge");
        mission += &connection("edge_judge", "gate_pick:judge", "fmn_judge:port_in");
        mission += &connection("edge_return", "fmn_judge:port_out", "gate_pick:judge");
    }
    mission
}

/// Each draft step writes its own version of the same file (and commits it
/// in a repository run); the judge picks `judge_picks` after reading both
/// kept workspaces.
#[derive(Clone)]
struct Drafts {
    repository: bool,
    judge_picks: &'static str,
    judged: Arc<Mutex<Vec<String>>>,
}

impl Executor for Drafts {
    fn execute_step(
        &self,
        request: StepExecution,
        events: ExecutionSink,
    ) -> Result<StepResult, ahjo_mission_engine::Error> {
        let run = || -> Result<StepResult, String> {
            let text = match request.step.id.as_str() {
                step @ ("fmn_a" | "fmn_b") => {
                    std::fs::write(request.cwd.join("draft.txt"), format!("from {step}"))
                        .map_err(|e| e.to_string())?;
                    if self.repository {
                        git(&request.cwd, &["add", "draft.txt"]);
                        git(&request.cwd, &["commit", "-m", &format!("work of {step}")]);
                    }
                    format!("draft {step}")
                }
                "fmn_judge" => {
                    if request.pick_steps != ["fmn_a", "fmn_b"] || request.pick_attempts != [1, 1] {
                        return Err(format!(
                            "judge saw candidates {:?} {:?}",
                            request.pick_steps, request.pick_attempts
                        ));
                    }
                    let mut judged = self.judged.lock().unwrap();
                    for path in (request.input_text.lines())
                        .filter_map(|line| line.strip_prefix("attempt workspace: "))
                    {
                        judged.push(
                            std::fs::read_to_string(Path::new(path).join("draft.txt"))
                                .map_err(|e| e.to_string())?,
                        );
                    }
                    format!(
                        "```ahjo-pick\n{}\n```",
                        serde_json::json!({"step":self.judge_picks,"attempt":1,"reason":"tighter","evidence":[]})
                    )
                }
                other => return Err(format!("unexpected step {other}")),
            };
            let mut result = LabExecutor
                .execute_step(request, events)
                .map_err(|e| e.message)?;
            result.text.clone_from(&text);
            for value in result.outputs.values_mut() {
                value["text"] = text.clone().into();
            }
            Ok(result)
        };
        run().map_err(|message| ahjo_mission_engine::Error::new("executor_error", message))
    }
}

struct Race {
    root: Scratch,
    c: Coordinator,
    id: String,
    /// The repository checkout, or the run folder.
    place: PathBuf,
    judged: Arc<Mutex<Vec<String>>>,
}

impl Race {
    fn start(kinds: &str, repository: bool, judge_picks: &'static str) -> Self {
        let root = Scratch::new();
        let place = root.0.join(if repository { "checkout" } else { "folder" });
        std::fs::create_dir(&place).unwrap();
        std::fs::write(place.join("draft.txt"), "original").unwrap();
        let mut settings = RunSettings::default();
        if repository {
            git(&place, &["init", "-b", "main"]);
            git(&place, &["config", "user.email", "test@example.invalid"]);
            git(&place, &["config", "user.name", "Test"]);
            git(&place, &["add", "draft.txt"]);
            git(&place, &["commit", "-m", "base"]);
            settings.repositories = BTreeMap::from([("owner/repo".into(), place.clone())]);
        }
        std::fs::write(root.0.join("missions/hello.mission.toml"), race(kinds)).unwrap();
        let judged = Arc::new(Mutex::new(Vec::new()));
        let executor = Drafts {
            repository,
            judge_picks,
            judged: judged.clone(),
        };
        let c = Coordinator::open(&root.0, settings, Box::new(executor)).unwrap();
        let id = c
            .start_mission(RunStart {
                mission: "hello".into(),
                input_card_id: "inp_hello".into(),
                expected_rev: 1,
                repository: if repository {
                    "owner/repo".into()
                } else {
                    String::new()
                },
                cwd: if repository {
                    String::new()
                } else {
                    place.to_string_lossy().into_owned()
                },
                inputs: BTreeMap::from([("brief".into(), "the task".into())]),
                ..Default::default()
            })
            .unwrap_or_else(|e| panic!("start: {e:?}"));
        Race {
            root,
            c,
            id,
            place,
            judged,
        }
    }

    fn ledger(&self) -> String {
        std::fs::read_to_string(self.root.0.join("runs").join(format!("{}.ndjson", self.id)))
            .unwrap()
    }

    /// Wait until the run is final, blocked, or waiting on a gate.
    fn settle(&self) -> RunProjection {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let p = self.c.run(&self.id).unwrap();
            if p.r#final || p.status == "blocked" || !p.waiting_gates.is_empty() {
                return p;
            }
            assert!(
                Instant::now() < deadline,
                "run did not settle:\n{}",
                self.ledger()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// The draft the run ended with: on the run branch, or in the run folder.
    fn draft(&self) -> String {
        let dir = if self.place.ends_with("checkout") {
            self.root.0.join("worktrees").join(&self.id)
        } else {
            self.place.clone()
        };
        std::fs::read_to_string(dir.join("draft.txt")).unwrap()
    }

    /// Each step's attempts as (step, attempt, has an output, picked).
    fn attempts(&self) -> Vec<(String, i64, bool, bool)> {
        let attempts = self.c.attempts(&self.id, None).unwrap();
        (attempts.steps.iter())
            .filter(|step| step.step_id.starts_with("fmn_") && step.step_id != "fmn_judge")
            .flat_map(|step| {
                step.attempts.iter().map(|attempt| {
                    (
                        step.step_id.clone(),
                        attempt.attempt,
                        !attempt.outputs.is_empty(),
                        attempt.picked.is_some(),
                    )
                })
            })
            .collect()
    }
}

fn picked(step: &str) -> Vec<(String, i64, bool, bool)> {
    ["fmn_a", "fmn_b"]
        .map(|id| (id.to_owned(), 1, true, id == step))
        .to_vec()
}

#[test]
fn you_pick_one_of_two_sibling_drafts_and_only_it_reaches_the_run_branch() {
    let race = Race::start("[\"human\"]", true, "");
    let waiting = race.settle();
    let gate = &waiting.waiting_gates[0];
    assert_eq!(gate.gate_id, "gate_pick", "{}", race.ledger());
    let pick = gate.pick.as_ref().unwrap();
    assert_eq!(
        pick["candidates"],
        serde_json::json!([{"step_id":"fmn_a","attempt":1},{"step_id":"fmn_b","attempt":1}])
    );
    // Neither draft reached the run branch while the pick waits.
    assert_eq!(race.draft(), "original");
    let refused = race
        .c
        .record_pick(
            &race.id,
            "gate_pick",
            HumanPick {
                requested_seq: gate.requested_seq,
                attempt: 1,
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(
        refused.message.contains("naming the step"),
        "{}",
        refused.message
    );
    race.c
        .record_pick(
            &race.id,
            "gate_pick",
            HumanPick {
                requested_seq: gate.requested_seq,
                step: "fmn_b".into(),
                attempt: 1,
                reason: "tighter".into(),
                ..Default::default()
            },
        )
        .unwrap();
    let done = race.settle();
    assert_eq!(done.status, "succeeded", "{}", race.ledger());
    assert_eq!(race.draft(), "from fmn_b");
    let log = git(
        &race.root.0.join("worktrees").join(&race.id),
        &["log", "--format=%s"],
    );
    assert!(log.contains("work of fmn_b"), "{log}");
    assert!(!log.contains("work of fmn_a"), "{log}");
    assert_eq!(race.attempts(), picked("fmn_b"));
}

#[test]
fn a_judge_picks_among_sibling_drafts_by_step() {
    let race = Race::start("[\"step\"]", true, "fmn_a");
    let done = race.settle();
    assert_eq!(done.status, "succeeded", "{}", race.ledger());
    let mut judged = race.judged.lock().unwrap().clone();
    judged.sort();
    assert_eq!(judged, ["from fmn_a", "from fmn_b"]);
    assert_eq!(race.draft(), "from fmn_a");
    assert_eq!(race.attempts(), picked("fmn_a"));
}

#[test]
fn a_folder_run_publishes_only_the_picked_siblings_files() {
    let race = Race::start("[\"human\"]", false, "");
    let waiting = race.settle();
    let gate = &waiting.waiting_gates[0];
    assert_eq!(race.draft(), "original", "{}", race.ledger());
    race.c
        .record_pick(
            &race.id,
            "gate_pick",
            HumanPick {
                requested_seq: gate.requested_seq,
                step: "fmn_a".into(),
                attempt: 1,
                ..Default::default()
            },
        )
        .unwrap();
    let done = race.settle();
    assert_eq!(done.status, "succeeded", "{}", race.ledger());
    assert_eq!(race.draft(), "from fmn_a");
    assert_eq!(race.attempts(), picked("fmn_a"));
}
