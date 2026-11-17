//! The operator's Host journeys, with a fake service manager
//! and controlled readings.
use std::{
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use ahjo_app_kit::link::client::LinkState;
use ahjo_host_contract::{
    self as wire, Component, Facts, Gpu, Heavy, LogLine, Point, Reading, ReadingFrame, Service,
    ServiceList, ServiceLog, State, Usage,
};
use ahjo_node_kit::{Area, BoxFuture, Built, Channel, Client, Listing, Setup};
use ahjo_testkit::node::Node;
use axum::{
    Json, Router,
    extract::Path,
    http::StatusCode,
    routing::{get, post},
};
use gpui::{Modifiers, TestAppContext, VisualTestContext};
use serde_json::json;
use tokio::sync::mpsc::{self as tokio_mpsc, UnboundedReceiver, UnboundedSender};

#[derive(Default)]
struct Control {
    on: AtomicBool,
    forwards: AtomicUsize,
    restarts: AtomicUsize,
    // One app socket: the test owns the sender and its channel takes the receiver.
    sender: Mutex<Option<UnboundedSender<Vec<u8>>>>,
    receiver: Mutex<Option<UnboundedReceiver<Vec<u8>>>>,
}
static CONTROL: OnceLock<Arc<Control>> = OnceLock::new();
const FAKE: Listing = Listing {
    name: "host",
    kinds: &[wire::HOST..=wire::HOST_EVENT],
    commands: &[],
    build,
};
fn build(_: &Setup) -> anyhow::Result<Built> {
    Built::now(FakeArea(CONTROL.get().unwrap().clone()))
}
struct FakeArea(Arc<Control>);
impl Area for FakeArea {
    fn routes(&self) -> Router {
        let control = self.0.clone();
        Router::new()
            .route(
                "/api/services",
                get(|| async {
                    let unit = |unit: &str, state: &str| Component {
                        unit: unit.into(),
                        state: state.into(),
                        ..Component::default()
                    };
                    Json(ServiceList {
                        services: vec![
                            Service {
                                id: "fixture".into(),
                                name: "Fixture service".into(),
                                state: "running".into(),
                                controllable: true,
                                components: vec![unit("fixture.service", "active")],
                                ..Service::default()
                            },
                            Service {
                                id: "other".into(),
                                name: "Other".into(),
                                state: "stopped".into(),
                                controllable: true,
                                link: "https://example.invalid/path".into(),
                                ..Service::default()
                            },
                            Service {
                                id: "broken".into(),
                                name: "Broken".into(),
                                state: "failed".into(),
                                controllable: true,
                                components: vec![unit("broken.service", "failed")],
                                ..Service::default()
                            },
                        ],
                    })
                }),
            )
            .route(
                "/api/services/{id}/log",
                get(|Path(id): Path<String>| async move {
                    Json(ServiceLog {
                        lines: vec![LogLine {
                            at: now(),
                            text: format!("{id} logged this"),
                            error: id == "broken",
                        }],
                        errors: vec![],
                    })
                }),
            )
            .route(
                "/api/services/fixture/restart",
                post(move || {
                    let control = control.clone();
                    async move {
                        control.restarts.fetch_add(1, Ordering::SeqCst);
                        (
                            StatusCode::BAD_REQUEST,
                            Json(json!({"error":{"code":"service_refused", "message":"Restart refused by fake manager"}})),
                        )
                    }
                }),
            )
    }
    fn channel(&self, _: Client) -> Option<Box<dyn Channel>> {
        let receiver = self
            .0
            .receiver
            .lock()
            .unwrap()
            .take()
            .expect("one app socket");
        Some(Box::new(FakeChannel {
            control: self.0.clone(),
            receiver,
            initial: None,
        }))
    }
}
struct FakeChannel {
    control: Arc<Control>,
    receiver: UnboundedReceiver<Vec<u8>>,
    initial: Option<Vec<u8>>,
}
impl Channel for FakeChannel {
    fn receive(&mut self, frame: Vec<u8>) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            if let [wire::HOST, on] = frame.as_slice() {
                self.control.on.store(*on != 0, Ordering::SeqCst);
                self.initial = (*on != 0).then(snapshot);
            }
        })
    }
    fn next(&mut self) -> BoxFuture<'_, Option<Vec<u8>>> {
        Box::pin(async move {
            if let Some(frame) = self.initial.take() {
                return Some(frame);
            }
            loop {
                let frame = self.receiver.recv().await?;
                if self.control.on.load(Ordering::SeqCst) {
                    self.control.forwards.fetch_add(1, Ordering::SeqCst);
                    return Some(frame);
                }
            }
        })
    }
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
fn sample(cpu: f32) -> ReadingFrame {
    let reading = Reading {
        at: now(),
        cpu: Some(cpu),
        memory: Some(Usage { used: 2, total: 4 }),
        swap: Some(Usage { used: 1, total: 4 }),
        swap_activity_bps: Some(2_000_000.),
        gpu_reason: Some("nvidia-smi failed".into()),
        heavy: vec![Heavy {
            name: "rustc".into(),
            cwd: Some("/srv/fixture".into()),
            pids: vec![1, 2],
            cpu: Some(40.),
            memory: 1 << 30,
        }],
        ..Reading::default()
    };
    ReadingFrame {
        point: Point::of(&reading),
        reading,
    }
}
fn frame(kind: u8, body: &impl serde::Serialize) -> Vec<u8> {
    let mut data = vec![kind];
    data.extend(serde_json::to_vec(body).unwrap());
    data
}
fn snapshot() -> Vec<u8> {
    let reading = sample(10.0);
    frame(
        wire::HOST_STATE,
        &State {
            facts: Facts {
                threads: 4,
                ..Facts::default()
            },
            minutes: vec![reading.point],
            latest: Some(reading.reading),
            ..State::default()
        },
    )
}
struct Running {
    url: String,
    node: String,
    control: Arc<Control>,
    stop: mpsc::Sender<()>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn start() -> Running {
    let control = Arc::new(Control::default());
    let (tx, rx) = tokio_mpsc::unbounded_channel();
    *control.sender.lock().unwrap() = Some(tx);
    *control.receiver.lock().unwrap() = Some(rx);
    assert!(CONTROL.set(control.clone()).is_ok());
    let (ready, wait) = mpsc::channel();
    let (stop, stopped) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async move {
                let node = Node::start(&[FAKE], "").await.unwrap();
                ready.send((node.url(), node.id())).unwrap();
                while stopped.try_recv().is_err() {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                node.stop().await;
            });
    });
    let (url, node) = wait.recv().unwrap();
    Running {
        url,
        node,
        control,
        stop,
        thread: Some(thread),
    }
}
fn until(
    cx: &mut VisualTestContext,
    what: &str,
    mut ok: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        if ok(cx) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn selector(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}
fn click(cx: &mut VisualTestContext, selector: &'static str) {
    let position = cx
        .debug_bounds(selector)
        .expect("the control is visible")
        .center();
    cx.simulate_click(position, Modifiers::none());
}
fn status(shell: &gpui::Entity<ahjo_app_shell::Shell>, cx: &mut VisualTestContext) -> String {
    shell.read_with(cx, |s, cx| {
        s.hub()
            .read(cx)
            .status
            .as_ref()
            .map(|s| s.text.clone())
            .unwrap_or_default()
    })
}
#[gpui::test]
fn host_journeys(cx: &mut TestAppContext) {
    let running = start();
    let dir = ahjo_testkit::fresh_dir("ahjo-host-journey");
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let (url, settings) = (running.url.clone(), dir.join("app.json"));
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(
            &[ahjo_host_app::build],
            Some(url),
            Some(settings),
            window,
            cx,
        )
    });
    let node = running.node.clone();
    let at = |what: String| selector(what);
    let shows = |cx: &mut VisualTestContext, s: &'static str| cx.debug_bounds(s).is_some();
    until(cx, "the node link", |cx| {
        shell.read_with(cx, |s, cx| {
            s.hub().read(cx).fleet.here().state == LinkState::Open
        })
    });
    until(cx, "initial host subscription", |_| {
        running.control.on.load(Ordering::SeqCst)
    });

    // Is this machine all right: the list says how much is wrong, the
    // machine names each trouble, and every figure reads now.
    let verdict = at(format!("host-verdict-{node}-2 to look at"));
    until(cx, "the verdict", |cx| shows(cx, verdict));
    for figure in ["cpu-10%", "memory-50%"] {
        let legend = at(format!("host-legend-{figure}"));
        until(cx, figure, |cx| shows(cx, legend));
    }
    assert!(!shows(cx, "host-legend-gpu-0%"), "no GPU was read");
    let swapping = "host-trouble-Swapping 1.9 MiB/s: memory is short";
    until(cx, "swap churn", |cx| shows(cx, swapping));
    until(cx, "the failed service", |cx| {
        shows(cx, "host-trouble-Service Broken failed")
    });
    until(cx, "what is heavy", |cx| shows(cx, "host-heavy-rustc ×2"));
    // Its failed service offers its one action on the row.
    assert!(shows(cx, at(format!("svc-row-start-{node}-broken"))));

    // A reading as it comes.
    let mut gpu_sample = sample(31.0);
    gpu_sample.reading.gpu_reason = None;
    gpu_sample.reading.gpus.push(Gpu {
        name: "GPU fixture".into(),
        util: Some(35.),
        ..Gpu::default()
    });
    gpu_sample.point = Point::of(&gpu_sample.reading);
    let send = |f: Vec<u8>| {
        running
            .control
            .sender
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .send(f)
            .unwrap()
    };
    send(frame(wire::HOST_READING, &gpu_sample));
    until(cx, "the pushed reading", |cx| {
        shows(cx, "host-legend-cpu-31%")
    });
    until(cx, "the GPU reading", |cx| shows(cx, "host-legend-gpu-35%"));
    assert_eq!(running.control.forwards.load(Ordering::SeqCst), 1);

    // The mouse over the figures reads one moment in all of them.
    let chart = cx.debug_bounds("host-chart").expect("the figures").center();
    cx.simulate_mouse_move(chart, None, Modifiers::none());
    until(cx, "the moment under the mouse", |cx| {
        shows(cx, "host-hover")
    });

    // A failed service is a click from its log; the machine's readings
    // keep coming while its services are looked at.
    click(cx, "host-trouble-Service Broken failed");
    until(cx, "the failed service's detail", |cx| {
        shows(cx, "pane-title-Broken") && shows(cx, "svc-log-broken logged this")
    });
    assert!(running.control.on.load(Ordering::SeqCst));

    // One action per state: a failed or stopped service starts; a
    // running one restarts or stops.
    let start = |id: &str| at(format!("svc-start-{node}-{id}"));
    let restart = |id: &str| at(format!("svc-restart-{node}-{id}"));
    assert!(shows(cx, start("broken")));
    assert!(!shows(cx, restart("broken")));
    click(cx, at(format!("svc-row-{node}-other")));
    until(cx, "a stopped service", |cx| {
        shows(cx, start("other")) && shows(cx, "svc-address")
    });
    assert!(
        !shows(cx, restart("other")) && !shows(cx, "svc-more"),
        "a stopped service offers Start"
    );
    assert!(
        !shows(cx, "svc-open"),
        "nothing answers at a stopped service's address"
    );
    // A running one restarts; Stop waits under More.
    click(cx, at(format!("svc-row-{node}-fixture")));
    until(cx, "a running service", |cx| {
        shows(cx, restart("fixture")) && shows(cx, "svc-more")
    });
    assert!(!shows(cx, start("fixture")));
    assert!(!shows(cx, at(format!("svc-stop-{node}-fixture"))));
    click(cx, "svc-more");
    until(cx, "Stop under More", |cx| shows(cx, "menu-label-Stop"));
    cx.simulate_keystrokes("escape");
    until(cx, "the menu closed", |cx| !shows(cx, "menu-label-Stop"));
    assert!(!shows(cx, "svc-address"), "it has none");
    click(cx, restart("fixture"));
    let failure = at(format!("svc-failure-{node}-fixture"));
    until(cx, "restart refusal displayed", |cx| shows(cx, failure));
    assert_eq!(running.control.restarts.load(Ordering::SeqCst), 1);
    assert_eq!(status(&shell, cx), "Restart refused by fake manager");
    let _ = std::fs::remove_dir_all(dir);
}
