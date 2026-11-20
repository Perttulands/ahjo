#![cfg(unix)]
use ahjo_app_kit::link::client::LinkState;
use ahjo_node_kit::{Listing, NodeClient};
use ahjo_testkit::{node::Node, sessions};
use gpui::{Modifiers, TestAppContext, VisualTestContext};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const AREAS: &[Listing] = &[
    sessions::LISTING,
    ahjo_work::LISTING,
    ahjo_missions::LISTING,
];
struct Running {
    url: String,
    id: String,
    stop: mpsc::Sender<()>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(t) = self.thread.take() {
            t.join().unwrap();
        }
    }
}
fn start() -> Running {
    let (ready, wait) = mpsc::channel();
    let (stop, stopped) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async move {
                let node = Node::start(AREAS, "").await.unwrap();
                let folder = node.dir().join("missions/missions");
                std::fs::create_dir_all(&folder).unwrap();
                let clean = include_str!("fixtures/validation-clean.mission.toml");
                std::fs::write(folder.join("clean.mission.toml"), clean).unwrap();
                let invalid = clean.replace("msn_clean", "msn_findings")
                    .replace("slug = \"clean\"", "slug = \"findings\"")
                    .replace("Clean delivery", "Delivery with findings")
                    .replace("harness = \"openai-codex\"", "harness = \"\"")
                    .replace("kinds = [\"human\"]", "kinds = [\"code\"]")
                    + "\n[[connection]]\nid = \"edge_duplicate\"\nfrom = \"inp_start:out\"\nto = \"gate_review:in\"\n[[connection]]\nid = \"edge_dangling\"\nfrom = \"step_work:out\"\nto = \"missing:in\"\n";
                std::fs::write(folder.join("findings.mission.toml"), invalid).unwrap();
                ready.send((node.url(), node.id())).unwrap();
                while stopped.try_recv().is_err() {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                node.stop().await;
            });
    });
    let (url, id) = wait.recv().unwrap();
    Running {
        url,
        id,
        stop,
        thread: Some(thread),
    }
}
fn until(
    cx: &mut VisualTestContext,
    what: &str,
    mut f: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        if f(cx) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}
#[gpui::test]
fn canvas_shows_exact_findings_on_cards_and_edges_and_clears_them(cx: &mut TestAppContext) {
    let running = start();
    let settings = std::env::temp_dir()
        .join(format!("ahjo-canvas-findings-{}", std::process::id()))
        .join("app.json");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let url = running.url.clone();
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(
            &[ahjo_missions_app::build],
            Some(url),
            Some(settings),
            window,
            cx,
        )
    });
    cx.simulate_resize(gpui::size(gpui::px(1600.), gpui::px(1000.)));
    until(cx, "link", |cx| {
        shell.read_with(cx, |s, cx| {
            s.hub().read(cx).fleet.here().state == LinkState::Open
        })
    });
    let invalid = Box::leak(format!("mission-{}-findings", running.id).into_boxed_str());
    until(cx, "findings mission", |cx| {
        cx.debug_bounds(invalid).is_some()
    });
    let position = cx.debug_bounds(invalid).unwrap().center();
    cx.simulate_click(position, Modifiers::none());
    // The head counts what it lacks; asked, the board marks each card and
    // wire it is about and lists it under the board.
    until(cx, "the count of what it lacks", |cx| {
        cx.debug_bounds("mission-todo").is_some()
    });
    let todo = cx.debug_bounds("mission-todo").unwrap().center();
    cx.simulate_click(todo, Modifiers::none());
    until(cx, "step, gate and edge marks", |cx| {
        cx.debug_bounds("validation-card-step_work").is_some()
            && cx.debug_bounds("validation-card-gate_review").is_some()
            && cx.debug_bounds("validation-edge-edge_duplicate").is_some()
            && cx.debug_bounds("validation-edge-edge_dangling").is_some()
    });
    until(cx, "gate note control", |cx| {
        cx.debug_bounds("canvas-note-pin-gate_review").is_some()
    });
    // The gate's finding mark and its note mark cover neither each other
    // nor the gate's ports.
    let pin = cx.debug_bounds("canvas-note-pin-gate_review").unwrap();
    let mark = cx.debug_bounds("validation-card-gate_review").unwrap();
    let apart = |a: gpui::Bounds<gpui::Pixels>, b: gpui::Bounds<gpui::Pixels>| {
        a.origin.x + a.size.width <= b.origin.x
            || b.origin.x + b.size.width <= a.origin.x
            || a.origin.y + a.size.height <= b.origin.y
            || b.origin.y + b.size.height <= a.origin.y
    };
    assert!(apart(pin, mark), "{pin:?} {mark:?}");
    for port in ["in", "pass", "fail"] {
        let selector = Box::leak(format!("mission-port-gate_review:{port}").into_boxed_str());
        if let Some(port) = cx.debug_bounds(selector) {
            assert!(apart(pin, port) && apart(mark, port), "{selector}");
        }
    }
    // What the mission lacks is listed below the canvas, never over a card.
    until(cx, "findings list", |cx| {
        cx.debug_bounds("canvas-findings-list").is_some()
    });
    let band = cx.debug_bounds("canvas-findings").unwrap();
    let board = cx.debug_bounds("mission-canvas").unwrap();
    assert!(
        board.origin.y + board.size.height <= band.origin.y + gpui::px(1.),
        "the to-do {band:?} lies over the board {board:?}"
    );
    let rt = tokio::runtime::Runtime::new().unwrap();
    let client = NodeClient::new(Some(&format!("{}/", running.url))).unwrap();
    let (report, _) = rt
        .block_on(client.request(
            http::Method::GET,
            "/api/missions/findings/validation",
            None,
            None,
        ))
        .unwrap();
    let mission = ahjo_missions_contract::Mission::parse(include_str!(
        "fixtures/validation-clean.mission.toml"
    ))
    .unwrap();
    let finding = report["errors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| {
            finding["node_id"] == "step_work"
                && finding["message"].as_str().unwrap().contains("step_work")
        })
        .unwrap();
    let finding: ahjo_missions_contract::Finding = serde_json::from_value(finding.clone()).unwrap();
    let text = ahjo_missions_app::store::validation::todo_line(&mission, &finding);
    assert!(!text.contains("step_work") && !text.contains(finding.code.as_str()));
    let selector = Box::leak(format!("validation-display-{text}").into_boxed_str());
    assert!(cx.debug_bounds(selector).is_some(), "{selector}");
    for id in [
        "step_work",
        "gate_review",
        "edge_duplicate",
        "edge_dangling",
    ] {
        let finding = report["errors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["node_id"] == id)
            .unwrap();
        let selector = Box::leak(
            format!(
                "validation-message-{id}-{}",
                finding["message"].as_str().unwrap()
            )
            .into_boxed_str(),
        );
        assert!(
            cx.debug_bounds(selector).is_some(),
            "the exact validation message must be visible for {id}"
        );
    }
    let clean = Box::leak(format!("mission-{}-clean", running.id).into_boxed_str());
    until(cx, "clean listing", |cx| cx.debug_bounds(clean).is_some());
    let position = cx.debug_bounds(clean).unwrap().center();
    cx.simulate_click(position, Modifiers::none());
    // Another mission opens on its board too.
    until(cx, "clean validation", |cx| {
        cx.debug_bounds("mission-validation-complete").is_some()
            && cx.debug_bounds("mission-card-step_work").is_some()
    });
    let (report, _) = rt
        .block_on(client.request(
            http::Method::GET,
            "/api/missions/clean/validation",
            None,
            None,
        ))
        .unwrap();
    assert!(report["errors"].as_array().unwrap().is_empty());
    assert!(report["warnings"].as_array().unwrap().is_empty());
    assert!(cx.debug_bounds("canvas-findings").is_none());
    assert!(cx.debug_bounds("validation-card-step_work").is_none());
    assert!(cx.debug_bounds("validation-card-gate_review").is_none());
    assert!(cx.debug_bounds("validation-edge-edge_duplicate").is_none());
    assert!(cx.debug_bounds("validation-edge-edge_dangling").is_none());
}
