#![cfg(unix)]
use ahjo_app_kit::{Ask, Host, Mounted, Page, ask::Answers, mount};
use ahjo_core::NodeId;
use ahjo_core::TypedCommand;
use ahjo_mission_engine::{
    ExecutionEvent, ExecutionSink, Executor, LabExecutor, StepExecution, StepResult,
};
use ahjo_node_kit::{Area, Built, Command, Effect, Listing, Setup};
use ahjo_terminals_contract::commands::{SessionRead, SessionReadout, SessionSend, SessionSent};
use ahjo_terminals_contract::{Session, SessionList};
use ahjo_testkit::app::{click, until};
use ahjo_testkit::{
    node::Node,
    sessions::{self, FakeSessions},
};
use gpui::{App, Context, IntoElement, ParentElement, TestAppContext, Window, div};
use serde_json::json;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

static OPENED: Mutex<Vec<Ask>> = Mutex::new(Vec::new());
static SESSIONS: Mutex<Option<Arc<FakeSessions>>> = Mutex::new(None);
static NODE: Mutex<String> = Mutex::new(String::new());
static RELEASE: Mutex<Option<mpsc::Receiver<()>>> = Mutex::new(None);
/// What was written into a seat's session, as `(session, text)`.
static WRITTEN: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
/// How often a seat's screen was read.
static READS: AtomicUsize = AtomicUsize::new(0);
/// What a seat's session shows, as the terminals area would read it.
const SCREEN: &str =
    "● Read(src/drafts/autosave.rs)\n  ⎿  Read 131 lines\n\n> fixing the save order";
/// The terminals area's read and send, as a seat's session answers them.
struct Screens;
impl Area for Screens {
    fn commands(&self) -> Vec<Command> {
        vec![
            Command::for_request(Effect::Reads, |read: SessionRead| async move {
                READS.fetch_add(1, Ordering::SeqCst);
                Ok(SessionReadout {
                    node: NodeId(NODE.lock().unwrap().clone()),
                    session: read.session,
                    pane: 1,
                    cut: None,
                    text: SCREEN.into(),
                })
            }),
            Command::for_request(Effect::Writes, |send: SessionSend| async move {
                WRITTEN
                    .lock()
                    .unwrap()
                    .push((send.session.clone(), send.text));
                Ok(SessionSent {
                    node: NodeId(NODE.lock().unwrap().clone()),
                    session: send.session,
                    pane: 1,
                    sent: true,
                })
            }),
        ]
    }
}
const SCREENS: Listing = Listing {
    name: "test-screens",
    kinds: &[],
    commands: &[SessionRead::NAME, SessionSend::NAME],
    build: |_| Ok(Built::Now(Arc::new(Screens))),
};
struct ToyTerminal;
impl gpui::Render for ToyTerminal {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().child("Terminal")
    }
}
impl Page for ToyTerminal {
    const ID: &'static str = "toy-terminal";
    const TITLE: &'static str = "Terminal";
    const ANSWERS: &'static [Answers] = &[Answers::Reveal("session")];
    fn answer(&mut self, ask: Ask, _: &'static str, _: &mut Window, _: &mut Context<Self>) {
        OPENED.lock().unwrap().push(ask);
    }
}
fn terminal(host: Host, window: &mut Window, cx: &mut App) -> Mounted {
    mount(host, window, cx, |_, _, _| ToyTerminal)
}
struct SeatLab;
impl Executor for SeatLab {
    fn execute_step(
        &self,
        request: StepExecution,
        sink: ExecutionSink,
    ) -> Result<StepResult, ahjo_core::Error> {
        let sessions = SESSIONS.lock().unwrap().clone().unwrap();
        let name = format!("seat-{}", request.step.id);
        let session = sessions.add(&name);
        let seat = request.step.seats[0].id.clone();
        let node = NODE.lock().unwrap().clone();
        sink.emit(ExecutionEvent {
            kind: "seat_created".into(),
            seat_id: seat.clone(),
            data:
                json!({"node":node,"session_name":name,"session_id":session,"dispatch_id":"lab-seat"})
                    .as_object()
                    .unwrap()
                    .clone(),
        })?;
        let receiver = RELEASE.lock().unwrap().take().unwrap();
        receiver
            .recv_timeout(Duration::from_secs(120))
            .map_err(|e| ahjo_core::Error::new("test", e.to_string()))?;
        let result = LabExecutor.execute_step(request, sink.clone())?;
        sink.emit(ExecutionEvent {
            kind: "seat_cleanup".into(),
            seat_id: seat,
            data: json!({"session_name":name,"outcome":"ended","cause":"run_final"})
                .as_object()
                .unwrap()
                .clone(),
        })?;
        Ok(result)
    }
}
fn build(setup: &Setup) -> anyhow::Result<Built> {
    ahjo_missions::build_with_executor(setup, Some(Box::new(SeatLab)))
}
const MISSIONS: Listing = Listing {
    build,
    ..ahjo_missions::LISTING
};
struct Running {
    url: String,
    node: String,
    run: String,
    session: String,
    seq: u64,
    release: Option<mpsc::Sender<()>>,
    stop: mpsc::Sender<()>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Running {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}
fn start() -> Running {
    let (ready, wait) = mpsc::channel();
    let (stop, stopped) = mpsc::channel();
    let (release, receiver) = mpsc::channel();
    *RELEASE.lock().unwrap() = Some(receiver);
    let thread = std::thread::spawn(move || {
        tokio::runtime::Runtime::new().unwrap().block_on(async move {
        let node = Node::start(&[sessions::LISTING, ahjo_issue::LISTING, MISSIONS, SCREENS], "").await.unwrap();
        *SESSIONS.lock().unwrap() = Some(node.fake_sessions());
        *NODE.lock().unwrap() = node.id();
        let dir = node.dir().join("missions/missions");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("hello.mission.toml"), include_str!("fixtures/solo-lab.mission.toml")).unwrap();
        let start = node.call("mission_run", json!({"mission":"hello","input":"inp_hello","inputs":{"brief":"ship"},"cwd":node.dir()})).await.unwrap();
        let run = start["run_id"].as_str().unwrap().to_owned();
        let deadline = Instant::now() + Duration::from_secs(15);
        let (seq, session) = loop {
            let status = node.call("run_show", json!({"run":run,"events":true})).await.unwrap();
            if let Some(event) = status["events"].as_array().unwrap().iter().find(|event| event["type"] == "seat_created") {
                break (event["seq"].as_u64().unwrap(), event["session_id"].as_str().unwrap().to_owned());
            }
            assert!(Instant::now() < deadline, "seat not recorded: {status}");
            tokio::time::sleep(Duration::from_millis(10)).await;
        };
        ready.send((node.url(), node.id(), run, session, seq)).unwrap();
        while stopped.try_recv().is_err() { tokio::time::sleep(Duration::from_millis(20)).await; }
        node.stop().await;
        *SESSIONS.lock().unwrap() = None;
    })
    });
    let (url, node, run, session, seq) = wait.recv_timeout(Duration::from_secs(20)).unwrap();
    Running {
        url,
        node,
        run,
        session,
        seq,
        release: Some(release),
        stop,
        thread: Some(thread),
    }
}
#[gpui::test]
fn step_and_run_bar_open_the_owned_session_and_disable_ended_seats(cx: &mut TestAppContext) {
    OPENED.lock().unwrap().clear();
    let mut running = start();
    cx.executor().allow_parking();
    cx.update(|cx| {
        ahjo_app_shell::prepare(cx);
        cx.set_app_identity("io.ahjo.seat-test", "Ahjo seat test");
    });
    let settings = std::env::temp_dir()
        .join(format!("ahjo-seat-{}", std::process::id()))
        .join("app.json");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    let url = running.url.clone();
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(
            &[ahjo_missions_app::build, terminal],
            Some(url),
            Some(settings),
            window,
            cx,
        )
    });
    shell.update(cx, |shell, cx| {
        Host::new(shell.hub())
            .for_page("test", &[])
            .ask(
                Ask::Reveal {
                    what: "mission".into(),
                    id: "hello".into(),
                    node: Some(NodeId(running.node.clone())),
                    part: None,
                    beside: false,
                },
                cx,
            )
            .unwrap()
    });
    until(cx, "mission on its board", |cx| {
        cx.debug_bounds("mission-card-fmn_work").is_some()
    });
    click(cx, "mission-card-fmn_work");
    let step = format!(
        "step-seat-{}-{}-live-seat-fmn_work",
        running.run, running.seq
    );
    until(cx, "live step seat", |cx| {
        cx.debug_bounds(Box::leak(step.clone().into_boxed_str()))
            .is_some()
    });
    click(cx, &step);
    until(cx, "step terminal ask", |_| {
        !OPENED.lock().unwrap().is_empty()
    });
    let expected = Ask::Reveal {
        what: "session".into(),
        id: running.session.clone(),
        node: Some(NodeId(running.node.clone())),
        part: None,
        beside: false,
    };
    assert_eq!(OPENED.lock().unwrap().last().unwrap(), &expected);
    shell.update(cx, |shell, cx| {
        Host::new(shell.hub())
            .for_page("test", &[])
            .ask(
                Ask::Reveal {
                    what: "run".into(),
                    id: running.run.clone(),
                    node: Some(NodeId(running.node.clone())),
                    part: None,
                    beside: false,
                },
                cx,
            )
            .unwrap()
    });
    let bar = format!(
        "flow-fmn_work-seat-{}-{}-live-seat-fmn_work",
        running.run, running.seq
    );
    until(cx, "live run seat", |cx| {
        cx.debug_bounds(Box::leak(bar.clone().into_boxed_str()))
            .is_some()
    });
    click(cx, &bar);
    until(cx, "bar terminal ask", |_| {
        OPENED.lock().unwrap().len() == 2
    });
    assert_eq!(OPENED.lock().unwrap().last().unwrap(), &expected);
    running.release.take().unwrap().send(()).unwrap();
    shell.update(cx, |shell, cx| {
        Host::new(shell.hub())
            .for_page("test", &[])
            .ask(
                Ask::Reveal {
                    what: "run".into(),
                    id: running.run.clone(),
                    node: Some(NodeId(running.node.clone())),
                    part: None,
                    beside: false,
                },
                cx,
            )
            .unwrap();
    });
    // An ended seat offers no session to open.
    until(cx, "ended seat", |cx| {
        cx.debug_bounds(Box::leak(bar.clone().into_boxed_str()))
            .is_none()
            && cx.debug_bounds("run-step-fmn_work-done").is_some()
    });
    cx.run_until_parked();
    assert_eq!(
        OPENED.lock().unwrap().len(),
        2,
        "ended seat must not open a terminal"
    );
    shell.update(cx, |shell, cx| {
        Host::new(shell.hub())
            .for_page("test", &[])
            .ask(
                Ask::Reveal {
                    what: "mission".into(),
                    id: "hello".into(),
                    node: Some(NodeId(running.node.clone())),
                    part: None,
                    beside: false,
                },
                cx,
            )
            .unwrap();
    });
    until(cx, "mission after run", |cx| {
        cx.debug_bounds("mission-card-fmn_work").is_some()
    });
    click(cx, "mission-card-fmn_work");
    let step_ended = format!(
        "step-seat-{}-{}-ended-seat-fmn_work",
        running.run, running.seq
    );
    until(cx, "step editor", |cx| {
        cx.debug_bounds("step-save").is_some()
    });
    assert!(
        cx.debug_bounds(Box::leak(step_ended.into_boxed_str()))
            .is_none(),
        "the step editor offers only seats that can still be opened"
    );
}

#[gpui::test]
fn a_running_steps_panel_shows_its_seat_live_and_writes_to_it(cx: &mut TestAppContext) {
    OPENED.lock().unwrap().clear();
    let mut running = start();
    cx.executor().allow_parking();
    cx.update(|cx| {
        ahjo_app_shell::prepare(cx);
        cx.set_app_identity("io.ahjo.seat-panel-test", "Ahjo seat panel test");
    });
    let settings = std::env::temp_dir()
        .join(format!("ahjo-seat-panel-{}", std::process::id()))
        .join("app.json");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    let url = running.url.clone();
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(
            &[ahjo_missions_app::build, terminal],
            Some(url),
            Some(settings),
            window,
            cx,
        )
    });
    shell.update(cx, |shell, cx| {
        Host::new(shell.hub())
            .for_page("test", &[])
            .ask(
                Ask::Reveal {
                    what: "run".into(),
                    id: running.run.clone(),
                    node: Some(NodeId(running.node.clone())),
                    part: None,
                    beside: false,
                },
                cx,
            )
            .unwrap()
    });
    until(cx, "the run's view", |cx| {
        cx.debug_bounds("run-graph").is_some()
    });
    click(cx, "run-graph");
    // The input says where the run came in, with its ▶.
    until(cx, "the run's board", |cx| {
        cx.debug_bounds("canvas-play-inp_hello").is_some()
    });
    // The step opens its panel at the window's edge: its seat, what its
    // session shows, and its attempt under way.
    click(cx, "mission-card-fmn_work");
    until(cx, "the step's panel", |cx| {
        cx.debug_bounds("run-seat-screen-seat_work").is_some()
            && cx.debug_bounds("run-attempt-1").is_some()
    });
    assert!(cx.debug_bounds("card-panel").is_some());
    assert!(
        cx.debug_bounds("attempts-window").is_none(),
        "the step no longer opens the attempts window over the board"
    );
    // What it is told goes into its session.
    click(cx, "run-seat-message-seat_work");
    cx.simulate_input("Check the save order first");
    click(cx, "run-seat-send-seat_work");
    until(cx, "the message written", |_| {
        !WRITTEN.lock().unwrap().is_empty()
    });
    assert_eq!(
        WRITTEN.lock().unwrap().last().unwrap(),
        &(running.session.clone(), "Check the save order first".into())
    );
    click(cx, "run-seat-open-seat_work");
    until(cx, "the session shown", |_| {
        !OPENED.lock().unwrap().is_empty()
    });
    assert_eq!(
        OPENED.lock().unwrap().last().unwrap(),
        &Ask::Reveal {
            what: "session".into(),
            id: running.session.clone(),
            node: Some(NodeId(running.node.clone())),
            part: None,
            beside: false,
        }
    );
    // Back on the run's board, the step opens its panel again.
    shell.update(cx, |shell, cx| {
        Host::new(shell.hub())
            .for_page("test", &[])
            .ask(
                Ask::Reveal {
                    what: "run".into(),
                    id: running.run.clone(),
                    node: Some(NodeId(running.node.clone())),
                    part: None,
                    beside: false,
                },
                cx,
            )
            .unwrap()
    });
    until(cx, "the run's view again", |cx| {
        cx.debug_bounds("run-graph").is_some()
    });
    click(cx, "run-graph");
    until(cx, "the board again", |cx| {
        cx.debug_bounds("mission-card-fmn_work").is_some()
    });
    click(cx, "mission-card-fmn_work");
    until(cx, "the step's panel again", |cx| {
        cx.debug_bounds("run-seat-seat_work").is_some()
    });
    // While the Sessions page hears the session writing, its screen is
    // read again and again; once it is quiet, no more.
    let hears = |active: bool, cx: &mut gpui::VisualTestContext| {
        let session = Session {
            node: running.node.clone(),
            id: running.session.clone(),
            name: "seat-fmn_work".into(),
            active: Some(active),
            ..Default::default()
        };
        shell.update(cx, |shell, cx| {
            Host::new(shell.hub())
                .for_page("test-sessions", &[])
                .publish(
                    SessionList {
                        sessions: vec![session],
                        focused: None,
                    },
                    cx,
                )
                .unwrap()
        });
    };
    let before = READS.load(Ordering::SeqCst);
    hears(true, cx);
    for _ in 0..3 {
        cx.executor().advance_clock(Duration::from_secs(2));
        cx.run_until_parked();
    }
    until(cx, "the screen read while it writes", |_| {
        READS.load(Ordering::SeqCst) >= before + 3
    });
    hears(false, cx);
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    let quiet = READS.load(Ordering::SeqCst);
    for _ in 0..3 {
        cx.executor().advance_clock(Duration::from_secs(2));
        cx.run_until_parked();
    }
    assert_eq!(
        READS.load(Ordering::SeqCst),
        quiet,
        "a quiet session's screen is not read on a timer"
    );
    // Once the seat's session closes, nothing is followed and nothing can
    // be written to it.
    running.release.take().unwrap().send(()).unwrap();
    until(cx, "the seat closed", |cx| {
        cx.debug_bounds("run-seat-screen-seat_work").is_none()
            && cx.debug_bounds("run-seat-seat_work").is_some()
    });
    assert!(cx.debug_bounds("run-seat-send-seat_work").is_none());
}
