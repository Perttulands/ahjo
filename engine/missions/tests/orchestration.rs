//! A lab seat side makes real dispatch records and uses testkit's fake Sessions.
use ahjo_mission_engine::{
    AgentSeat, Coordinator, Dispatch, DispatchResult, PublishedTurn, RunSettings, SeatEvent,
    SeatExecutor, SeatFuture, SeatId, SeatSide, TurnWorkspace,
};
use ahjo_mission_model::RunStart;
use ahjo_term::{Sessions, Start};
use ahjo_testkit::{node::Node, sessions};
use serde_json::json;
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::mpsc;

#[derive(Clone)]
struct LabSeats {
    sessions: Arc<dyn Sessions>,
    root: PathBuf,
    entries: Arc<Mutex<HashMap<String, Entry>>>,
}
struct Entry {
    dispatch: Dispatch,
    seat: SeatId,
    path: PathBuf,
    events: Vec<SeatEvent>,
    staged: bool,
    result: Option<DispatchResult>,
    alive: bool,
    notifications: Vec<String>,
}
impl SeatSide for LabSeats {
    fn dispatch(&self, request: Dispatch) -> SeatFuture<'_, String> {
        Box::pin(async move {
            let name = if request.name.is_empty() {
                format!("seat-{}", request.seat_id)
            } else {
                request.name.clone()
            };
            let id = self
                .sessions
                .start(Start {
                    name: name.clone(),
                    folder: request.cwd.to_string_lossy().into(),
                    command: String::new(),
                    user: String::new(),
                })
                .await
                .map_err(|e| ahjo_mission_engine::Error::new("fake", e.to_string()))?;
            let pane = self
                .sessions
                .active_pane(&id)
                .await
                .map_err(|e| ahjo_mission_engine::Error::new("fake", e.to_string()))?;
            let seat = SeatId {
                node: "test-node".into(),
                session_name: name,
                session: ahjo_term::SessionRef {
                    session_id: id,
                    pane_id: pane,
                    generation: ahjo_term::Generation {
                        pid: 1,
                        start_time: 1,
                    },
                },
                cwd: None,
            };
            let path = self.root.join(format!("{}.md", request.seat_id));
            std::fs::write(&path, &request.brief)
                .map_err(|e| ahjo_mission_engine::Error::new("fake", e.to_string()))?;
            let created = json!({"dispatch_id":request.id,"node":seat.node,"session_name":seat.session_name,"session_id":seat.session.session_id,"pane_id":seat.session.pane_id,"generation":seat.session.generation});
            let prepared = json!({"seat":seat,"brief_path":path});
            let events = vec![
                SeatEvent {
                    key: "created".into(),
                    seq: 1,
                    kind: "seat_created".into(),
                    data: created.as_object().unwrap().clone(),
                },
                SeatEvent {
                    key: "prepared".into(),
                    seq: 2,
                    kind: "seat_prepared".into(),
                    data: prepared.as_object().unwrap().clone(),
                },
            ];
            let dispatch_id = request.id.clone();
            self.entries.lock().unwrap().insert(
                dispatch_id.clone(),
                Entry {
                    dispatch: request,
                    seat,
                    path,
                    events,
                    staged: false,
                    result: None,
                    alive: true,
                    notifications: Vec::new(),
                },
            );
            Ok(dispatch_id)
        })
    }
    fn events(
        &self,
        id: &str,
        since: u64,
    ) -> Result<
        mpsc::Receiver<Result<SeatEvent, ahjo_mission_engine::Error>>,
        ahjo_mission_engine::Error,
    > {
        let entries = self.entries.clone();
        let id = id.to_owned();
        let (tx, rx) = mpsc::channel(32);
        tokio::spawn(async move {
            let mut cursor = since;
            loop {
                let events = entries.lock().unwrap().get(&id).unwrap().events.clone();
                for event in events
                    .into_iter()
                    .filter(|event| event.seq > cursor)
                    .collect::<Vec<_>>()
                {
                    cursor = event.seq;
                    if tx.send(Ok(event)).await.is_err() {
                        return;
                    }
                }
                tokio::select! { _ = tx.closed() => return, _ = tokio::time::sleep(Duration::from_millis(10)) => {} }
            }
        });
        Ok(rx)
    }
    fn stage(&self, id: &str, _: &str) -> SeatFuture<'_, ()> {
        let id = id.to_owned();
        Box::pin(async move {
            self.entries.lock().unwrap().get_mut(&id).unwrap().staged = true;
            Ok(())
        })
    }
    fn dispatch_info(&self, id: &str) -> Result<Dispatch, ahjo_mission_engine::Error> {
        Ok(self
            .entries
            .lock()
            .unwrap()
            .get(id)
            .unwrap()
            .dispatch
            .clone())
    }
    fn result(&self, id: &str) -> Result<Option<DispatchResult>, ahjo_mission_engine::Error> {
        Ok(self.entries.lock().unwrap().get(id).unwrap().result.clone())
    }
    fn seat_alive(&self, seat: &SeatId) -> SeatFuture<'_, bool> {
        let alive = self
            .entries
            .lock()
            .unwrap()
            .values()
            .find(|e| e.seat == *seat)
            .unwrap()
            .alive;
        Box::pin(async move { Ok(alive) })
    }
    fn notify(&self, id: &str, text: &str) -> SeatFuture<'_, ()> {
        self.entries
            .lock()
            .unwrap()
            .get_mut(id)
            .unwrap()
            .notifications
            .push(text.into());
        Box::pin(async { Ok(()) })
    }
    fn end_seat(&self, seat: &SeatId, cause: &str) -> SeatFuture<'_, String> {
        let seat = seat.clone();
        let cause = cause.to_owned();
        Box::pin(async move {
            let mut entries = self.entries.lock().unwrap();
            let entry = entries
                .values_mut()
                .find(|entry| entry.seat == seat)
                .unwrap();
            let data = json!({"session_name":seat.session_name,"outcome":if cause == "shutdown" {"left_shutdown"} else {"ended"},"cause":cause});
            entry.alive = false;
            entry.events.push(SeatEvent {
                key: "cleanup".into(),
                seq: entry.events.len() as u64 + 1,
                kind: "seat_cleanup".into(),
                data: data.as_object().unwrap().clone(),
            });
            Ok("ended".into())
        })
    }
    fn seats(&self, run_id: &str) -> Result<Vec<AgentSeat>, ahjo_mission_engine::Error> {
        let entries = self.entries.lock().unwrap();
        Ok(entries
            .values()
            .filter(|entry| entry.dispatch.run_id == run_id)
            .map(|entry| AgentSeat {
                dispatch_id: entry.dispatch.id.clone(),
                seat_id: entry.dispatch.seat_id.clone(),
                node: entry.seat.node.clone(),
                session: entry.seat.session.session_id.clone(),
                pane: Some(entry.seat.session.pane_id),
                session_name: entry.seat.session_name.clone(),
                outcome: if entry.alive { "" } else { "ended" }.into(),
                available: entry.alive,
                waiting: None,
            })
            .collect())
    }
    fn prepare_turn(&self, _: &str, _: &TurnWorkspace) -> SeatFuture<'_, PathBuf> {
        Box::pin(async { unreachable!() })
    }
    fn publish_turn(&self, _: &str, _: &TurnWorkspace) -> SeatFuture<'_, PublishedTurn> {
        Box::pin(async { unreachable!() })
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn controller_output_reaches_gate_and_lost_workers_are_reported() {
    let node = Node::start(&[sessions::LISTING], "").await.unwrap();
    let root = ahjo_testkit::fresh_dir("orchestrated-seats");
    std::fs::create_dir_all(root.join("missions")).unwrap();
    let mission = include_str!("parity/runs/missions/solo-lab.mission.toml")
        .replace("formation = \"solo\"", "formation = \"orchestrated\"")
        .replace("id = \"seat_work\"", "id = \"seat_lead\"\ncontroller = true")
        .replace("[[end]]", "[[step.seat]]\nid = \"seat_a\"\nlabel = \"Worker A\"\nharness = \"openai-codex\"\neffort = \"medium\"\n[[step.seat]]\nid = \"seat_b\"\nlabel = \"Worker B\"\nharness = \"claude-code\"\neffort = \"medium\"\n\n[[end]]");
    let mission = mission.replace("to = \"end_done:in\"", "to = \"gate_review:in\"");
    let mission = format!(
        "{mission}\n[[gate]]\nid = \"gate_review\"\ntitle = \"Review\"\nkinds = [\"code\"]\ncheck = \"output_contains\"\ncheck_version = \"1\"\ncheck_value = \"controller-result\"\n[[end]]\nid = \"end_rejected\"\ntitle = \"Rejected\"\noutcome = \"rejected\"\n[[connection]]\nid = \"edge_pass\"\nfrom = \"gate_review:pass\"\nto = \"end_done:in\"\n[[connection]]\nid = \"edge_fail\"\nfrom = \"gate_review:fail\"\nto = \"end_rejected:in\"\n"
    );
    std::fs::write(root.join("missions/hello.mission.toml"), mission).unwrap();
    let seats = LabSeats {
        sessions: node.sessions(),
        root: root.clone(),
        entries: Default::default(),
    };
    let coordinator = Coordinator::open(
        &root,
        RunSettings::default(),
        Box::new(SeatExecutor::new(
            Arc::new(seats.clone()),
            tokio::runtime::Handle::current(),
        )),
    )
    .unwrap();
    let workspace = root.join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let run = coordinator
        .start_mission(RunStart {
            mission: "hello".into(),
            input_card_id: "inp_hello".into(),
            expected_rev: 1,
            cwd: workspace.to_string_lossy().into(),
            inputs: BTreeMap::from([("brief".into(), "ship".into())]),
            ..Default::default()
        })
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let projection = coordinator.run(&run).unwrap();
            if projection
                .events
                .iter()
                .any(|event| event.kind == "seat_dispatch" && event.seat_id == "seat_lead")
                && seats
                    .entries
                    .lock()
                    .unwrap()
                    .values()
                    .any(|entry| entry.dispatch.seat_id == "seat_lead" && entry.staged)
            {
                break;
            }
            assert!(
                !projection.r#final && projection.status != "blocked",
                "{}",
                projection.status
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("controller dispatched");
    let sessions = node.sessions().list();
    assert_eq!(sessions.len(), 3);
    {
        let entries = seats.entries.lock().unwrap();
        let lead = entries
            .values()
            .find(|entry| entry.dispatch.seat_id == "seat_lead")
            .unwrap();
        let brief = std::fs::read_to_string(&lead.path).unwrap();
        assert!(lead.staged);
        let attempt = brief
            .lines()
            .find(|line| line.starts_with("Attempt directory: "))
            .unwrap();
        for worker in ["seat_a", "seat_b"] {
            let entry = entries
                .values()
                .find(|entry| entry.dispatch.seat_id == worker)
                .unwrap();
            assert!(
                !entry.staged,
                "the controller delegates the worker's first task"
            );
            assert!(brief.contains(&entry.seat.session_name));
            assert!(brief.contains(&format!(
                "ahjo session send {} --text",
                entry.seat.session_name
            )));
            assert!(brief.contains(entry.path.to_str().unwrap()));
            assert!(
                std::fs::read_to_string(&entry.path)
                    .unwrap()
                    .lines()
                    .any(|line| line == attempt)
            );
        }
    }
    let ledger =
        ahjo_mission_engine::Ledger::new(root.join("runs").join(format!("{run}.ndjson")), &run)
            .read()
            .unwrap();
    let team = ledger
        .iter()
        .find(|event| event.kind == "orchestration_team")
        .unwrap();
    assert_eq!(team.data["controller_seat"], "seat_lead");
    assert_eq!(team.data["workers"].as_array().unwrap().len(), 2);
    assert_eq!(
        ledger
            .iter()
            .filter(|event| event.kind == "worker_observation")
            .count(),
        2
    );
    // Simulate one worker disappearing while the controller is still working.
    {
        let mut entries = seats.entries.lock().unwrap();
        entries
            .values_mut()
            .find(|e| e.dispatch.seat_id == "seat_a")
            .unwrap()
            .alive = false;
    }
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if seats.entries.lock().unwrap().values().any(|e| {
                e.dispatch.seat_id == "seat_lead"
                    && e.notifications
                        .iter()
                        .any(|text| text.contains("seat_a") && text.contains("gone"))
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("controller notified of lost worker");
    // The worker saves its own artifact before the controller reports its result.
    let attempt = {
        let entries = seats.entries.lock().unwrap();
        let worker = entries
            .values()
            .find(|e| e.dispatch.seat_id == "seat_b")
            .unwrap();
        let brief = std::fs::read_to_string(&worker.path).unwrap();
        PathBuf::from(
            brief
                .lines()
                .find_map(|l| l.strip_prefix("Attempt directory: "))
                .unwrap(),
        )
    };
    std::fs::write(attempt.join("worker-seat_b.md"), "worker-result").unwrap();
    {
        let mut entries = seats.entries.lock().unwrap();
        let lead = entries
            .values_mut()
            .find(|e| e.dispatch.seat_id == "seat_lead")
            .unwrap();
        lead.result = Some(DispatchResult {
            status: "ok".into(),
            text: "```ahjo-outputs\n{\"port_out\":{\"text\":\"controller-result\"}}\n```".into(),
        });
        let data = json!({"dispatch_id":lead.dispatch.id,"sentinel":{"run_id":run}});
        lead.events.push(SeatEvent {
            key: "result".into(),
            seq: 3,
            kind: "seat_result".into(),
            data: data.as_object().unwrap().clone(),
        });
    }
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let projection = coordinator.run(&run).unwrap();
            if projection.r#final || projection.status == "blocked" {
                assert_eq!(
                    projection.status,
                    "succeeded",
                    "{}",
                    std::fs::read_to_string(root.join("runs").join(format!("{run}.ndjson")))
                        .unwrap()
                );
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("run reaches End");
    assert_eq!(
        std::fs::read_to_string(attempt.join("worker-seat_b.md")).unwrap(),
        "worker-result"
    );
    let ledger =
        ahjo_mission_engine::Ledger::new(root.join("runs").join(format!("{run}.ndjson")), &run)
            .read()
            .unwrap();
    assert!(ledger.iter().any(|e| e.kind == "worker_observation"
        && e.data["outcome"] == "gone"
        && e.seat_id == "seat_a"));
    assert!(
        ledger
            .iter()
            .any(|e| e.kind == "gate_kind_result" && e.data["verdict"] == "pass")
    );
    for seat in ["seat_a", "seat_b"] {
        assert!(
            ledger
                .iter()
                .any(|e| e.kind == "seat_cleanup" && e.seat_id == seat)
        );
    }
    drop(coordinator);
    node.stop().await;
    std::fs::remove_dir_all(root).unwrap();
}

// The same real SeatExecutor is driven through the node's public CLI here.
static CLI_SEATS: Mutex<Option<Arc<LabSeats>>> = Mutex::new(None);
struct CliSeatExecutor(tokio::runtime::Handle);
impl ahjo_mission_engine::Executor for CliSeatExecutor {
    fn seats(&self, run_id: &str) -> Result<Vec<AgentSeat>, ahjo_mission_engine::Error> {
        CLI_SEATS.lock().unwrap().as_ref().unwrap().seats(run_id)
    }

    fn execute_step(
        &self,
        execution: ahjo_mission_engine::StepExecution,
        sink: ahjo_mission_engine::ExecutionSink,
    ) -> Result<ahjo_mission_engine::StepResult, ahjo_mission_engine::Error> {
        let seats = CLI_SEATS.lock().unwrap().as_ref().unwrap().clone();
        ahjo_mission_engine::Executor::execute_step(
            &SeatExecutor::new(seats, self.0.clone()),
            execution,
            sink,
        )
    }
}
fn cli_build(setup: &ahjo_node_kit::Setup) -> anyhow::Result<ahjo_node_kit::Built> {
    ahjo_missions::build_with_executor(
        setup,
        Some(Box::new(CliSeatExecutor(tokio::runtime::Handle::current()))),
    )
}
const CLI_MISSIONS: ahjo_node_kit::Listing = ahjo_node_kit::Listing {
    build: cli_build,
    ..ahjo_missions::LISTING
};
async fn cli(node: &Node, noun: &str, args: &[&str]) -> serde_json::Value {
    let mut argv: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
    argv.extend(["--node".into(), format!("{}/", node.url()), "--json".into()]);
    let mut output = Vec::new();
    let exit = ahjo_node_kit::cli::run(noun, &argv, &mut output)
        .await
        .unwrap();
    assert_eq!(
        exit,
        0,
        "{noun} {argv:?}: {}",
        String::from_utf8_lossy(&output)
    );
    serde_json::from_slice(&output).unwrap()
}
#[tokio::test(flavor = "multi_thread")]
async fn cli_runs_orchestration_to_end_with_controller_and_worker_seats() {
    let node = Node::start(&[sessions::LISTING, ahjo_issue::LISTING, CLI_MISSIONS], "")
        .await
        .unwrap();
    let root = node.dir().join("missions");
    let seats = Arc::new(LabSeats {
        sessions: node.sessions(),
        root: root.clone(),
        entries: Default::default(),
    });
    *CLI_SEATS.lock().unwrap() = Some(seats.clone());
    let mission = include_str!("parity/runs/missions/solo-lab.mission.toml")
        .replace("formation = \"solo\"", "formation = \"orchestrated\"")
        .replace("id = \"seat_work\"", "id = \"seat_lead\"\ncontroller = true")
        .replace("[[end]]", "[[step.seat]]\nid = \"seat_worker\"\nlabel = \"Worker\"\nharness = \"openai-codex\"\neffort = \"medium\"\n\n[[end]]");
    std::fs::create_dir_all(root.join("missions")).unwrap();
    std::fs::write(root.join("missions/hello.mission.toml"), mission).unwrap();
    let inspect = cli(&node, "mission", &["show", "hello"]).await;
    let started = cli(
        &node,
        "mission",
        &["run", "hello", "--inputs", r#"{"brief":"ship"}"#],
    )
    .await;
    let run = started["run_id"].as_str().unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let ready = {
                let entries = seats.entries.lock().unwrap();
                entries.len() == 2
                    && entries
                        .values()
                        .any(|e| e.dispatch.seat_id == "seat_lead" && e.staged)
            };
            if ready {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("controller and worker prepared");
    let shown_run = cli(&node, "run", &["show", run, "--seats"]).await;
    {
        let entries = seats.entries.lock().unwrap();
        let shown_seats = shown_run["seats"].as_array().unwrap();
        assert_eq!(shown_seats.len(), entries.len());
        for entry in entries.values() {
            let shown = shown_seats
                .iter()
                .find(|shown| shown["seat_id"] == entry.dispatch.seat_id)
                .unwrap();
            assert_eq!(shown["session"], entry.seat.session_name);
            assert_eq!(shown["session_id"], entry.seat.session.session_id);
        }
    }
    {
        let mut entries = seats.entries.lock().unwrap();
        assert!(
            entries
                .values()
                .any(|e| e.dispatch.seat_id == "seat_worker" && !e.staged)
        );
        let seats = inspect["steps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|step| step["id"] == "fmn_work")
            .unwrap()["seats"]
            .as_array()
            .unwrap();
        assert_eq!(seats.len(), entries.len());
        for entry in entries.values() {
            let seat = seats
                .iter()
                .find(|seat| seat["id"] == entry.dispatch.seat_id)
                .unwrap();
            assert!(seat.get("session").is_none());
            assert_eq!(entry.dispatch.name, entry.seat.session_name);
        }
        let lead = entries
            .values_mut()
            .find(|e| e.dispatch.seat_id == "seat_lead")
            .unwrap();
        let brief = std::fs::read_to_string(&lead.path).unwrap();
        assert!(brief.contains("hello-work-worker"));
        lead.result = Some(DispatchResult {
            status: "ok".into(),
            text: "```ahjo-outputs\n{\"port_out\":{\"text\":\"controller-result\"}}\n```".into(),
        });
        let data = json!({"dispatch_id":lead.dispatch.id,"sentinel":{"run_id":run}});
        lead.events.push(SeatEvent {
            key: "result".into(),
            seq: 3,
            kind: "seat_result".into(),
            data: data.as_object().unwrap().clone(),
        });
    }
    let final_run = cli(
        &node,
        "run",
        &["wait", run, "--until", "final", "--timeout", "10"],
    )
    .await;
    assert_eq!(final_run["status"], "succeeded", "{final_run}");
    let ledger =
        ahjo_mission_engine::Ledger::new(root.join("runs").join(format!("{run}.ndjson")), run)
            .read()
            .unwrap();
    {
        let entries = seats.entries.lock().unwrap();
        for event in ledger.iter().filter(|event| event.kind == "seat_dispatch") {
            let id = event.data["dispatch_id"].as_str().unwrap();
            let session = &entries[id].seat.session_name;
            assert_eq!(event.data["session"], *session);
            assert_eq!(event.data["session_ref"], format!("terminal:{session}"));
        }
    }
    assert!(
        ledger
            .iter()
            .any(|e| e.kind == "orchestration_team" && e.data["controller_seat"] == "seat_lead")
    );
    assert!(
        ledger
            .iter()
            .any(|e| e.kind == "card_output" && e.card_id == "fmn_work")
    );
    node.stop().await;
    *CLI_SEATS.lock().unwrap() = None;
}
