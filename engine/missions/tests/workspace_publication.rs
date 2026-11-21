#[allow(dead_code)]
mod run_support;
use ahjo_mission_engine::{
    Coordinator, ExecutionSink, Executor, LabExecutor, RunSettings, StepExecution, StepResult,
};
use ahjo_mission_model::RunStart;
use run_support::{Scratch, gated_mission};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
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
fn setup(root: &Scratch) -> RunSettings {
    let checkout = root.0.join("checkout");
    std::fs::create_dir(&checkout).unwrap();
    git(&checkout, &["init", "-b", "main"]);
    git(&checkout, &["config", "user.email", "test@example.invalid"]);
    git(&checkout, &["config", "user.name", "Test"]);
    std::fs::write(checkout.join("base"), "base").unwrap();
    git(&checkout, &["add", "base"]);
    git(&checkout, &["commit", "-m", "base"]);
    RunSettings {
        repositories: BTreeMap::from([("owner/repo".into(), checkout)]),
        ..Default::default()
    }
}
fn request() -> RunStart {
    RunStart {
        mission: "hello".into(),
        input_card_id: "inp_hello".into(),
        expected_rev: 1,
        repository: "owner/repo".into(),
        inputs: BTreeMap::from([("brief".into(), "hello".into())]),
        ..Default::default()
    }
}
fn wait(c: &Coordinator, id: &str) -> ahjo_mission_model::RunProjection {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let p = c.run(id).unwrap();
        if p.r#final || p.status == "blocked" {
            return p;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn turns(root: &Scratch, id: &str) -> Vec<String> {
    std::fs::read_dir(root.0.join("worktrees"))
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|s| s.starts_with(&format!(".turn-{id}-")))
        .collect()
}
struct Committer {
    dirty: bool,
}
impl Executor for Committer {
    fn execute_step(
        &self,
        req: StepExecution,
        sink: ExecutionSink,
    ) -> Result<StepResult, ahjo_mission_engine::Error> {
        {
            std::fs::write(req.cwd.join("agent-file"), "committed by agent").unwrap();
            git(&req.cwd, &["add", "agent-file"]);
            git(&req.cwd, &["commit", "-m", "agent commit"]);
            git(
                &req.cwd,
                &["commit", "--allow-empty", "-m", "agent checkpoint"],
            );
            if self.dirty {
                std::fs::write(req.cwd.join("dirty-file"), "uncommitted").unwrap();
            }
            LabExecutor
                .execute_step(req, sink)
                .map_err(|error| error.message)
        }
        .map_err(|message: String| ahjo_mission_engine::Error::new("executor_error", message))
    }
}
#[test]
fn publishes_agent_commits_with_clean_and_dirty_trees() {
    for dirty in [false, true] {
        let root = Scratch::new();
        let settings = setup(&root);
        let c = Coordinator::open(&root.0, settings, Box::new(Committer { dirty })).unwrap();
        let id = c.start_mission(request()).unwrap();
        let p = wait(&c, &id);
        assert_eq!(p.status, "succeeded");
        let cwd = root.0.join("worktrees").join(&id);
        assert_eq!(
            std::fs::read_to_string(cwd.join("agent-file")).unwrap(),
            "committed by agent"
        );
        if dirty {
            assert_eq!(
                std::fs::read_to_string(cwd.join("dirty-file")).unwrap(),
                "uncommitted"
            );
        }
        assert!(
            git(&cwd, &["log", "--format=%s"])
                .lines()
                .any(|s| s == "agent commit")
        );
        assert!(
            git(&cwd, &["log", "--format=%s"])
                .lines()
                .any(|s| s == "agent checkpoint")
        );
        assert!(turns(&root, &id).is_empty());
    }
}
struct Failure {
    mode: &'static str,
    started: Arc<AtomicBool>,
}
impl Executor for Failure {
    fn execute_step(
        &self,
        req: StepExecution,
        sink: ExecutionSink,
    ) -> Result<StepResult, ahjo_mission_engine::Error> {
        (|| -> Result<StepResult, String> {
            std::fs::write(req.cwd.join("unfinished"), "partial").unwrap();
            self.started.store(true, Ordering::Release);
            if self.mode == "error" {
                return Err("executor failed".into());
            }
            if self.mode == "cancel" {
                while !req.canceled.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(5));
                }
                return Err("canceled".into());
            }
            let id = req.run_id.clone();
            let mut result = LabExecutor
                .execute_step(req, sink)
                .map_err(|error| error.message)?;
            result.text = format!("<<<AHJO-ESCALATE run-id={id} severity=stop reason=blocked>>>");
            Ok(result)
        })()
        .map_err(|message: String| ahjo_mission_engine::Error::new("executor_error", message))
    }
}
/// A blocked step keeps its live checkout. Its partial work also gets a
/// branch of its own, visible at the point where the run stopped.
#[test]
fn blocked_or_canceled_attempts_preserve_their_work_and_only_canceled_removes_the_checkout() {
    for mode in ["error", "cancel", "escalate"] {
        let root = Scratch::new();
        let settings = setup(&root);
        let started = Arc::new(AtomicBool::new(false));
        let c = Coordinator::open(
            &root.0,
            settings,
            Box::new(Failure {
                mode,
                started: started.clone(),
            }),
        )
        .unwrap();
        let id = c.start_mission(request()).unwrap();
        if mode == "cancel" {
            let deadline = Instant::now() + Duration::from_secs(10);
            while !started.load(Ordering::Acquire) {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
            c.abort(&id, "human:test", "stop").unwrap();
        }
        let p = wait(&c, &id);
        assert_eq!(
            p.status,
            if mode == "cancel" {
                "canceled"
            } else {
                "blocked"
            }
        );
        assert_eq!(
            turns(&root, &id).len(),
            usize::from(mode != "cancel"),
            "{mode}: {:?}",
            turns(&root, &id)
        );
        let checkout = root.0.join("checkout");
        let prefix = format!("refs/heads/ahjo/run-{}-kept/", &id[4..12]);
        let branches = git(
            &checkout,
            &["for-each-ref", "--format=%(refname:short)", &prefix],
        );
        let branch = branches.lines().collect::<Vec<_>>();
        assert_eq!(branch.len(), 1, "{mode}: {branches:?}");
        let branch = branch[0];
        assert_eq!(
            git(&checkout, &["show", &format!("{branch}:unfinished")]),
            "partial",
            "{mode}"
        );
        let kept = p
            .events
            .iter()
            .find(|e| e.kind == "workspace_kept")
            .unwrap_or_else(|| panic!("{mode}: no workspace_kept event"));
        assert!(kept.detail.contains(branch), "{mode}: {}", kept.detail);
        if mode != "cancel" {
            let blocked = p
                .events
                .iter()
                .rev()
                .find(|e| e.kind == "run_blocked")
                .unwrap();
            assert!(
                blocked.detail.contains(branch),
                "{mode}: {}",
                blocked.detail
            );
            continue;
        }
        c.land(&id, true).unwrap();
        c.delete_run(&id).unwrap();
        assert_eq!(
            git(
                &checkout,
                &["for-each-ref", "--format=%(refname:short)", &prefix]
            ),
            "",
            "the kept branch goes with the run"
        );
    }
}
struct Locking;
impl Executor for Locking {
    fn execute_step(
        &self,
        req: StepExecution,
        sink: ExecutionSink,
    ) -> Result<StepResult, ahjo_mission_engine::Error> {
        {
            std::fs::write(req.cwd.join("changed"), "value").unwrap();
            let primary = req.cwd.parent().unwrap().join(&req.run_id);
            let lock = git(&primary, &["rev-parse", "--git-path", "index.lock"]);
            std::fs::write(lock, "locked").unwrap();
            LabExecutor
                .execute_step(req, sink)
                .map_err(|error| error.message)
        }
        .map_err(|message: String| ahjo_mission_engine::Error::new("executor_error", message))
    }
}
#[test]
fn failed_publication_does_not_request_a_human_verdict() {
    let root = Scratch::new();
    let settings = setup(&root);
    std::fs::write(
        root.0.join("missions/hello.mission.toml"),
        gated_mission("kinds = [\"human\"]"),
    )
    .unwrap();
    let c = Coordinator::open(&root.0, settings, Box::new(Locking)).unwrap();
    let id = c.start_mission(request()).unwrap();
    let p = wait(&c, &id);
    assert_eq!(p.status, "blocked");
    assert!(p.waiting_gates.is_empty());
    assert!(!p.events.iter().any(|e| e.kind == "human_input_requested"));
    let primary = root.0.join("worktrees").join(&id);
    let lock = git(&primary, &["rev-parse", "--git-path", "index.lock"]);
    std::fs::remove_file(lock).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match c.resume(&id, Default::default()) {
            Ok(_) => break,
            Err(e) if e.code == "conflict" => {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) => panic!("{e}"),
        }
    }
    loop {
        let p = c.run(&id).unwrap();
        if !p.waiting_gates.is_empty() {
            break;
        }
        assert!(!p.r#final && p.status != "blocked" && Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(turns(&root, &id).len(), 1);
    c.abort(&id, "human:test", "stop").unwrap();
    assert!(turns(&root, &id).is_empty());
}

#[test]
fn recovery_removes_a_turn_left_after_settlement() {
    use sha2::{Digest, Sha256};
    let root = Scratch::new();
    let settings = setup(&root);
    let c = Coordinator::open(
        &root.0,
        settings.clone(),
        Box::new(Committer { dirty: false }),
    )
    .unwrap();
    let id = c.start_mission(request()).unwrap();
    assert_eq!(wait(&c, &id).status, "succeeded");
    drop(c);
    let primary = root.0.join("worktrees").join(&id);
    let turn = root.0.join("worktrees").join(format!(
        ".turn-{id}-{}-1",
        hex::encode(Sha256::digest(b"fmn_work"))
    ));
    git(
        &primary,
        &[
            "worktree",
            "add",
            "--detach",
            turn.to_str().unwrap(),
            "HEAD",
        ],
    );
    let ledger = root.0.join("runs").join(format!("{id}.ndjson"));
    let text = std::fs::read_to_string(&ledger).unwrap();
    let lines: Vec<_> = text.lines().collect();
    let settled = lines
        .iter()
        .position(|line| {
            let e: serde_json::Value = serde_json::from_str(line).unwrap();
            e["type"] == "workspace_settled"
        })
        .unwrap();
    let retained: Vec<_> = lines[..=settled]
        .iter()
        .filter(|line| {
            let e: serde_json::Value = serde_json::from_str(line).unwrap();
            e["type"] != "workspace_started"
        })
        .enumerate()
        .map(|(i, line)| {
            let mut e: serde_json::Value = serde_json::from_str(line).unwrap();
            e["seq"] = (i as u64 + 1).into();
            serde_json::to_string(&e).unwrap()
        })
        .collect();
    std::fs::write(&ledger, retained.join("\n") + "\n").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let c = loop {
        match Coordinator::open(
            &root.0,
            settings.clone(),
            Box::new(Committer { dirty: false }),
        ) {
            Ok(c) => break c,
            Err(_) => {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    };
    assert_eq!(wait(&c, &id).status, "succeeded");
    assert!(!turn.exists());
    assert!(turns(&root, &id).is_empty());
    assert!(
        !git(&primary, &["worktree", "list", "--porcelain"])
            .contains(&turn.to_string_lossy().to_string())
    );
}

#[test]
fn recovery_keeps_an_interrupted_stable_workspace_for_retry() {
    use sha2::{Digest, Sha256};
    let root = Scratch::new();
    let settings = setup(&root);
    let c = Coordinator::open(
        &root.0,
        settings.clone(),
        Box::new(Committer { dirty: false }),
    )
    .unwrap();
    let id = c.start_mission(request()).unwrap();
    assert_eq!(wait(&c, &id).status, "succeeded");
    drop(c);
    let primary = root.0.join("worktrees").join(&id);
    git(&primary, &["reset", "--hard", "HEAD~2"]);
    let turn = root.0.join("worktrees").join(format!(
        ".turn-{id}-{}-1",
        hex::encode(Sha256::digest(b"fmn_work"))
    ));
    git(
        &primary,
        &[
            "worktree",
            "add",
            "--detach",
            turn.to_str().unwrap(),
            "HEAD",
        ],
    );
    std::fs::write(turn.join("partial"), "interrupted").unwrap();
    let ledger = root.0.join("runs").join(format!("{id}.ndjson"));
    let text = std::fs::read_to_string(&ledger).unwrap();
    let lines: Vec<_> = text.lines().collect();
    let started = lines
        .iter()
        .position(|line| {
            let e: serde_json::Value = serde_json::from_str(line).unwrap();
            e["type"] == "workspace_started"
        })
        .unwrap();
    std::fs::write(&ledger, lines[..=started].join("\n") + "\n").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let c = loop {
        match Coordinator::open(
            &root.0,
            settings.clone(),
            Box::new(Committer { dirty: false }),
        ) {
            Ok(c) => break c,
            Err(_) => {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    };
    let result = wait(&c, &id);
    assert_eq!(result.status, "succeeded");
    assert!(
        result
            .events
            .iter()
            .any(|e| e.kind == "card_started" && e.card_id == "fmn_work" && e.attempt == 2)
    );
    assert!(!turn.exists());
    assert!(turns(&root, &id).is_empty());
    assert_eq!(
        std::fs::read_to_string(primary.join("partial")).unwrap(),
        "interrupted"
    );
}

#[test]
fn recovery_finishes_settlement_after_worktree_was_removed() {
    let root = Scratch::new();
    let settings = setup(&root);
    let c = Coordinator::open(
        &root.0,
        settings.clone(),
        Box::new(Committer { dirty: false }),
    )
    .unwrap();
    let id = c.start_mission(request()).unwrap();
    assert_eq!(wait(&c, &id).status, "succeeded");
    drop(c);
    let ledger = root.0.join("runs").join(format!("{id}.ndjson"));
    let text = std::fs::read_to_string(&ledger).unwrap();
    let lines: Vec<_> = text.lines().collect();
    let published = lines
        .iter()
        .position(|line| {
            let e: serde_json::Value = serde_json::from_str(line).unwrap();
            e["type"] == "workspace_published"
        })
        .unwrap();
    std::fs::write(&ledger, lines[..=published].join("\n") + "\n").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let c = loop {
        match Coordinator::open(
            &root.0,
            settings.clone(),
            Box::new(Committer { dirty: false }),
        ) {
            Ok(c) => break c,
            Err(_) => {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    };
    let p = wait(&c, &id);
    assert_eq!(p.status, "succeeded");
    assert!(p.events.iter().any(|e| e.kind == "workspace_settled"));
    assert!(turns(&root, &id).is_empty());
}

#[test]
fn recovery_does_not_duplicate_a_published_prefix_of_agent_commits() {
    use sha2::{Digest, Sha256};
    for event_kind in ["workspace_snapshot", "workspace_commit"] {
        let root = Scratch::new();
        let settings = setup(&root);
        let c = Coordinator::open(
            &root.0,
            settings.clone(),
            Box::new(Committer { dirty: true }),
        )
        .unwrap();
        let id = c.start_mission(request()).unwrap();
        assert_eq!(wait(&c, &id).status, "succeeded");
        drop(c);
        let primary = root.0.join("worktrees").join(&id);
        let ledger = root.0.join("runs").join(format!("{id}.ndjson"));
        let text = std::fs::read_to_string(&ledger).unwrap();
        let lines: Vec<_> = text.lines().collect();
        let snapshot = lines
            .iter()
            .position(|line| {
                let e: serde_json::Value = serde_json::from_str(line).unwrap();
                e["type"] == "workspace_snapshot"
            })
            .unwrap();
        let data: serde_json::Value = serde_json::from_str(lines[snapshot]).unwrap();
        let commits = data["data"]["commits"].as_array().unwrap();
        assert_eq!(commits.len(), 3);
        let source_head = commits[2].as_str().unwrap();
        git(&primary, &["reset", "--hard", "HEAD~2"]);
        let turn = root.0.join("worktrees").join(format!(
            ".turn-{id}-{}-1",
            hex::encode(Sha256::digest(b"fmn_work"))
        ));
        git(
            &primary,
            &[
                "worktree",
                "add",
                "--detach",
                turn.to_str().unwrap(),
                source_head,
            ],
        );
        let cut = if event_kind == "workspace_snapshot" {
            snapshot
        } else {
            lines
                .iter()
                .position(|line| {
                    let e: serde_json::Value = serde_json::from_str(line).unwrap();
                    e["type"] == "workspace_commit"
                })
                .unwrap()
        };
        std::fs::write(&ledger, lines[..=cut].join("\n") + "\n").unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let c = loop {
            match Coordinator::open(
                &root.0,
                settings.clone(),
                Box::new(Committer { dirty: true }),
            ) {
                Ok(c) => break c,
                Err(_) => {
                    assert!(Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        };
        assert_eq!(wait(&c, &id).status, "succeeded");
        let history = git(&primary, &["log", "--format=%s"]);
        assert_eq!(
            history
                .lines()
                .filter(|line| *line == "agent commit")
                .count(),
            1
        );
        assert_eq!(
            std::fs::read_to_string(primary.join("agent-file")).unwrap(),
            "committed by agent"
        );
        assert_eq!(
            std::fs::read_to_string(primary.join("dirty-file")).unwrap(),
            "uncommitted"
        );
        assert!(turns(&root, &id).is_empty());
    }
}

#[test]
fn recovery_unregisters_a_turn_whose_directory_was_already_removed() {
    use sha2::{Digest, Sha256};
    let root = Scratch::new();
    let settings = setup(&root);
    let c = Coordinator::open(
        &root.0,
        settings.clone(),
        Box::new(Committer { dirty: false }),
    )
    .unwrap();
    let id = c.start_mission(request()).unwrap();
    assert_eq!(wait(&c, &id).status, "succeeded");
    drop(c);
    let primary = root.0.join("worktrees").join(&id);
    let turn = root.0.join("worktrees").join(format!(
        ".turn-{id}-{}-1",
        hex::encode(Sha256::digest(b"fmn_work"))
    ));
    git(
        &primary,
        &[
            "worktree",
            "add",
            "--detach",
            turn.to_str().unwrap(),
            "HEAD",
        ],
    );
    std::fs::remove_dir_all(&turn).unwrap();
    let ledger = root.0.join("runs").join(format!("{id}.ndjson"));
    let text = std::fs::read_to_string(&ledger).unwrap();
    let lines: Vec<_> = text.lines().collect();
    let settled = lines
        .iter()
        .position(|line| {
            let e: serde_json::Value = serde_json::from_str(line).unwrap();
            e["type"] == "workspace_settled"
        })
        .unwrap();
    std::fs::write(&ledger, lines[..=settled].join("\n") + "\n").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let c = loop {
        match Coordinator::open(
            &root.0,
            settings.clone(),
            Box::new(Committer { dirty: false }),
        ) {
            Ok(c) => break c,
            Err(_) => {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    };
    assert_eq!(wait(&c, &id).status, "succeeded");
    assert!(
        !git(&primary, &["worktree", "list", "--porcelain"])
            .contains(&turn.to_string_lossy().to_string())
    );
}

fn step(id: &str, title: &str, goal: &str) -> String {
    format!(
        "[[step]]\nid = \"{id}\"\nformation = \"solo\"\ntitle = \"{title}\"\n[step.brief]\ngoal = \"{goal}\"\n[[step.input]]\nid = \"port_in\"\nlabel = \"Input\"\n[[step.output]]\nid = \"port_out\"\nlabel = \"Result\"\n[[step.seat]]\nid = \"seat_{id}\"\nlabel = \"Seat\"\nagent = \"builder\"\nharness = \"openai-codex\"\nmodel = \"\"\neffort = \"medium\"\n\n"
    )
}

fn connection(id: &str, from: &str, to: &str) -> String {
    format!("[[connection]]\nid = \"{id}\"\nfrom = \"{from}\"\nto = \"{to}\"\n\n")
}

/// Input -> Work -> Review (pass Ship -> Done, fail back to Work), Review
/// judged by the chain Judge 1 -> Judge 2 when its kinds hold step,
/// with the given Limit cards.
fn pick_mission(kinds: &str, work_goal: &str, cards: &str) -> String {
    let mut mission = String::from(
        "schema = 5\nid = \"msn_pick\"\nslug = \"hello\"\ntitle = \"Pick\"\nrev = 1\n\n[[input_card]]\nid = \"inp_hello\"\ntitle = \"Hello\"\ngoal = \"Return the supplied input\"\n\n",
    );
    mission += &step("fmn_work", "Work", work_goal);
    mission += &step("fmn_ship", "Ship", "Ship it.");
    let judged = kinds.contains("step");
    if judged {
        mission += &step("fmn_j1", "Judge 1", "Review the work.");
        mission += &step("fmn_j2", "Judge 2", "Decide.");
    }
    mission += &format!(
        "[[gate]]\nid = \"gate_review\"\ntitle = \"Review\"\nkinds = {kinds}\ncriterion = \"Judge the work\"\n\n[[end]]\nid = \"end_done\"\ntitle = \"Done\"\noutcome = \"done\"\n\n"
    );
    mission += &connection("edge_start", "inp_hello:out", "fmn_work:port_in");
    mission += &connection("edge_review", "fmn_work:port_out", "gate_review:in");
    mission += &connection("edge_pass", "gate_review:pass", "fmn_ship:port_in");
    mission += &connection("edge_fail", "gate_review:fail", "fmn_work:port_in");
    mission += &connection("edge_done", "fmn_ship:port_out", "end_done:in");
    if judged {
        mission += &connection("edge_judge", "gate_review:judge", "fmn_j1:port_in");
        mission += &connection("edge_chain", "fmn_j1:port_out", "fmn_j2:port_in");
        mission += &connection("edge_return", "fmn_j2:port_out", "gate_review:judge");
    }
    mission + cards
}

fn work_card(knobs: &str) -> String {
    format!("[[limit]]\nid = \"lim_cap\"\ntitle = \"Cap\"\nscope = \"fmn_work\"\n{knobs}\n")
}

fn verdict(verdict: &str, reason: &str) -> String {
    format!(
        "```ahjo-verdict\n{}\n```",
        serde_json::json!({"verdict":verdict,"reason":reason,"evidence":[]})
    )
}

fn pick_block(attempt: i64, reason: &str, evidence: &[&str]) -> String {
    format!(
        "```ahjo-pick\n{}\n```",
        serde_json::json!({"attempt":attempt,"reason":reason,"evidence":evidence})
    )
}

#[derive(Clone)]
struct RetryGit {
    paths: Arc<std::sync::Mutex<Vec<std::path::PathBuf>>>,
    selected: i64,
}

impl Executor for RetryGit {
    fn execute_step(
        &self,
        request: StepExecution,
        events: ExecutionSink,
    ) -> Result<StepResult, ahjo_mission_engine::Error> {
        let run = || -> Result<StepResult, String> {
            let text = match request.step.id.as_str() {
                "fmn_work" => {
                    self.paths.lock().unwrap().push(request.cwd.clone());
                    let previous = std::fs::read_to_string(request.cwd.join("draft.txt"))
                        .map_err(|e| e.to_string())?;
                    let expected = if request.attempt == 1 {
                        "original"
                    } else {
                        "attempt 1"
                    };
                    if previous != expected {
                        return Err(format!("retry read {previous:?}, expected {expected:?}"));
                    }
                    std::fs::write(
                        request.cwd.join("draft.txt"),
                        format!("attempt {}", request.attempt),
                    )
                    .map_err(|e| e.to_string())?;
                    format!("draft {}", request.attempt)
                }
                "fmn_j1" => "review".into(),
                "fmn_j2" if !request.pick_attempts.is_empty() => {
                    let snapshots: Vec<_> = request
                        .input_text
                        .lines()
                        .filter_map(|line| line.strip_prefix("attempt workspace: "))
                        .collect();
                    if snapshots.len() != 2 {
                        return Err(format!("picker got {} snapshots", snapshots.len()));
                    }
                    for (index, path) in snapshots.iter().enumerate() {
                        let text =
                            std::fs::read_to_string(std::path::Path::new(path).join("draft.txt"))
                                .map_err(|e| e.to_string())?;
                        if text != format!("attempt {}", index + 1) {
                            return Err(format!("wrong snapshot {text:?}"));
                        }
                    }
                    pick_block(self.selected, "selected", &[])
                }
                "fmn_j2" => verdict("fail", "try again"),
                "fmn_ship" => {
                    let text = std::fs::read_to_string(request.cwd.join("draft.txt"))
                        .map_err(|e| e.to_string())?;
                    if text != format!("attempt {}", self.selected) {
                        return Err(format!("ship saw {text:?}"));
                    }
                    "shipped".into()
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

#[test]
fn git_retry_uses_one_workspace_and_publishes_the_picked_snapshot() {
    for selected in [1, 2] {
        for keep_sessions in [false, true] {
            let root = Scratch::new();
            let settings = setup(&root);
            let checkout = root.0.join("checkout");
            std::fs::write(checkout.join("draft.txt"), "original").unwrap();
            git(&checkout, &["add", "draft.txt"]);
            git(&checkout, &["commit", "-m", "draft"]);
            let mission = pick_mission(
                "[\"step\"]",
                "Draft.",
                &work_card("rounds = 2\nwhen_spent = \"best\""),
            );
            std::fs::write(root.0.join("missions/hello.mission.toml"), mission).unwrap();
            let paths = Arc::new(std::sync::Mutex::new(Vec::new()));
            let c = Coordinator::open(
                &root.0,
                settings,
                Box::new(RetryGit {
                    paths: paths.clone(),
                    selected,
                }),
            )
            .unwrap();
            let id = c
                .start_mission(RunStart {
                    keep_sessions,
                    ..request()
                })
                .unwrap();
            let result = wait(&c, &id);
            assert_eq!(
                result.status,
                "picked",
                "{}",
                std::fs::read_to_string(root.0.join("runs").join(format!("{id}.ndjson"))).unwrap()
            );
            let paths = paths.lock().unwrap();
            assert_eq!(paths.len(), 2);
            assert_eq!(paths[0], paths[1]);
            assert_eq!(
                std::fs::read_to_string(root.0.join("worktrees").join(&id).join("draft.txt"))
                    .unwrap(),
                format!("attempt {selected}")
            );
            if keep_sessions {
                assert_eq!(
                    std::fs::read_to_string(paths[0].join("draft.txt")).unwrap(),
                    "attempt 2"
                );
                std::fs::write(paths[0].join("after-run.txt"), "usable").unwrap();
            } else {
                assert!(!paths[0].exists());
            }
        }
    }
}
