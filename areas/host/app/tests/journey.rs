//! The operator's Host journeys, with a fake service manager
//! and controlled readings.
use ahjo_testkit::app::{Served, Started, click, serve, until};
use std::{
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use ahjo_app_kit::link::client::LinkState;
use ahjo_host_contract::{
    self as wire, Component, Facts, Gpu, Heavy, LogLine, Point, Reading, ReadingFrame, Service,
    State, Usage,
    commands::{ServiceList, ServiceLogged, ServiceRead, ServiceRestart, ServicesListed},
};
use ahjo_node_kit::{Area, BoxFuture, Built, Channel, Client, Command, Effect, Listing, Setup};
use ahjo_testkit::node::Node;
use gpui::{Modifiers, TestAppContext, VisualTestContext};
use tokio::sync::mpsc::{self as tokio_mpsc, UnboundedReceiver, UnboundedSender};

#[derive(Default)]
struct Control {
    on: AtomicBool,
    /// The catalog stops reading.
    unread: AtomicBool,
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
    commands: &["service_list", "service_read", "service_restart"],
    build,
};
fn build(setup: &Setup) -> anyhow::Result<Built> {
    Built::now(FakeArea(
        CONTROL.get().unwrap().clone(),
        setup.node.id.to_string(),
    ))
}
struct FakeArea(Arc<Control>, String);
impl Area for FakeArea {
    fn commands(&self) -> Vec<Command> {
        let reading = self.0.clone();
        let control = self.0.clone();
        let list_node = self.1.clone();
        let log_node = self.1.clone();
        vec![
            Command::typed("service_list", Effect::Reads, move |_: ServiceList| {
                let reading = reading.clone();
                let node = list_node.clone();
                async move {
                    if reading.unread.load(Ordering::SeqCst) {
                        return Err(ahjo_core::Error::new(
                            "unavailable",
                            "The catalog is not JSON",
                        ));
                    }
                    let unit = |unit: &str, state: &str| Component {
                        unit: unit.into(),
                        state: state.into(),
                        ..Component::default()
                    };
                    Ok(ServicesListed {
                        node,
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
                }
            }),
            Command::typed("service_read", Effect::Reads, move |args: ServiceRead| {
                let node = log_node.clone();
                async move {
                    Ok(ServiceLogged {
                        node,
                        log: wire::ServiceLog {
                            lines: vec![LogLine {
                                at: now(),
                                text: format!("{} logged this", args.id),
                                error: args.id == "broken",
                            }],
                            errors: vec![],
                        },
                    })
                }
            }),
            Command::typed(
                "service_restart",
                Effect::Writes,
                move |_: ServiceRestart| {
                    let control = control.clone();
                    async move {
                        control.restarts.fetch_add(1, Ordering::SeqCst);
                        Err::<wire::commands::ServiceActed, _>(ahjo_core::Error::new(
                            "service_refused",
                            "Restart refused by fake manager",
                        ))
                    }
                },
            ),
        ]
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
        heavy: Some(vec![Heavy {
            name: "rustc".into(),
            cwd: Some("/srv/fixture".into()),
            pids: vec![1, 2],
            cpu: Some(40.),
            memory: 1 << 30,
        }]),
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
struct Fixture {
    control: Arc<Control>,
}
fn start() -> Served<Fixture> {
    let control = Arc::new(Control::default());
    let (tx, rx) = tokio_mpsc::unbounded_channel();
    *control.sender.lock().unwrap() = Some(tx);
    *control.receiver.lock().unwrap() = Some(rx);
    assert!(CONTROL.set(control.clone()).is_ok());
    serve(move || async move {
        let node = Node::start(&[FAKE], "").await.unwrap();
        Started::new(node, Fixture { control })
    })
}
fn selector(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
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
    let node = running.id.clone();
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

    // Wide, each figure stands beside the chart; in a narrow window the
    // figures stand above it and the chart takes the whole width.
    let beside = |cx: &mut VisualTestContext| {
        let legend = cx.debug_bounds("host-legend-cpu-31%");
        let chart = cx.debug_bounds("host-chart");
        legend.zip(chart).map(|(l, c)| l.right() <= c.left())
    };
    cx.simulate_resize(gpui::size(gpui::px(1600.), gpui::px(1000.)));
    until(cx, "the figures beside the chart", |cx| {
        beside(cx) == Some(true)
    });
    cx.simulate_resize(gpui::size(gpui::px(640.), gpui::px(900.)));
    until(cx, "the figures above the chart", |cx| {
        let legend = cx.debug_bounds("host-legend-cpu-31%");
        let chart = cx.debug_bounds("host-chart");
        legend
            .zip(chart)
            .is_some_and(|(l, c)| l.bottom() <= c.top() && l.left() <= c.left())
    });
    cx.simulate_resize(gpui::size(gpui::px(1600.), gpui::px(1000.)));
    until(cx, "the figures beside the chart again", |cx| {
        beside(cx) == Some(true)
    });

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

    // Services that could not be read are not all right: the machine says
    // so, as one more thing to look at.
    running.control.unread.store(true, Ordering::SeqCst);
    click(cx, at(format!("host-row-{node}-machine")));
    click(cx, "services-refresh");
    until(cx, "the unread services in the verdict", |cx| {
        shows(
            cx,
            "host-trouble-Services could not be read: The catalog is not JSON",
        )
    });
    let _ = std::fs::remove_dir_all(dir);
}
