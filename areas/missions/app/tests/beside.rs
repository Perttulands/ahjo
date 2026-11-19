#![cfg(unix)]
//! A mission opens on its board with Flow beside it: readable, and tied
//! to Flow both ways.
use ahjo_app_kit::link::client::LinkState;
use ahjo_node_kit::{Listing, NodeClient};
use ahjo_testkit::{node::Node, sessions};
use gpui::{Bounds, Modifiers, Pixels, TestAppContext, VisualTestContext, px};
use serde_json::json;
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
    steps: Vec<String>,
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
/// A node with a ten-step sketch: longer than its pane shows whole.
fn start() -> Running {
    let (ready, wait) = mpsc::channel();
    let (stop, stopped) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async move {
                let node = Node::start(AREAS, "").await.unwrap();
                let client = NodeClient::new(Some(&format!("{}/", node.url()))).unwrap();
                let steps: Vec<String> = (1..=10).map(|n| format!("Step {n}")).collect();
                let (doc, _) = client
                    .request(
                        http::Method::POST,
                        "/api/missions",
                        Some(json!({"title": "Long", "steps": steps.join("\n")})),
                        None,
                    )
                    .await
                    .unwrap();
                let ids = doc["mission"]["formations"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|f| f["id"].as_str().unwrap().to_owned())
                    .collect();
                ready.send((node.url(), node.id(), ids)).unwrap();
                while stopped.try_recv().is_err() {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                node.stop().await;
            });
    });
    let (url, id, steps) = wait.recv().unwrap();
    Running {
        url,
        id,
        steps,
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
fn at(cx: &mut VisualTestContext, selector: String) -> Option<Bounds<Pixels>> {
    cx.debug_bounds(Box::leak(selector.into_boxed_str()))
}
fn inside(inner: Bounds<Pixels>, outer: Bounds<Pixels>) -> bool {
    inner.origin.x >= outer.origin.x
        && inner.origin.y >= outer.origin.y
        && inner.bottom_right().x <= outer.bottom_right().x
        && inner.bottom_right().y <= outer.bottom_right().y
}

#[gpui::test]
fn a_long_mission_opens_on_its_board_beside_flow_and_follows_the_choice(cx: &mut TestAppContext) {
    let running = start();
    let settings = std::env::temp_dir()
        .join(format!("ahjo-missions-beside-{}", std::process::id()))
        .join("app.json");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let (url, path) = (running.url.clone(), settings.clone());
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(
            &[ahjo_missions_app::build],
            Some(url),
            Some(path),
            window,
            cx,
        )
    });
    // A short window: ten steps cannot all show there at a readable size.
    cx.simulate_resize(gpui::size(px(1920.), px(560.)));
    until(cx, "link", |cx| {
        shell.read_with(cx, |s, cx| {
            s.hub().read(cx).fleet.here().state == LinkState::Open
        })
    });
    let listing = format!("mission-{}-long", running.id);
    until(cx, "mission listing", |cx| {
        at(cx, listing.clone()).is_some()
    });
    let row = at(cx, listing).unwrap().center();
    cx.simulate_click(row, Modifiers::none());
    let first = running.steps[0].clone();
    until(cx, "the board beside Flow", |cx| {
        cx.debug_bounds("missions-board").is_some()
            && cx.debug_bounds("missions-flow").is_some()
            && at(cx, format!("mission-card-{first}")).is_some()
    });
    let pane = cx.debug_bounds("missions-board").unwrap();
    // It opens on its start, a card drawn no smaller than its words read
    // at (a step card is 300 wide on the board).
    let card = at(cx, format!("mission-card-{first}")).unwrap();
    assert!(card.size.width >= px(300. * 0.8), "{card:?}");
    assert!(inside(card, pane));
    // Too long to show whole at that size: the last step is out of view
    // (not drawn, or drawn past the pane's edge) until it is chosen in
    // Flow, which brings it into view on the board.
    let last = running.steps.last().unwrap().clone();
    assert!(at(cx, format!("mission-card-{last}")).is_none_or(|c| !inside(c, pane)));
    let window = cx.debug_bounds("missions-flow").unwrap();
    until(cx, "the last step in view in Flow", |cx| {
        let seen = at(cx, format!("flow-title-{last}")).is_some_and(|t| inside(t, window));
        if !seen {
            cx.simulate_event(gpui::ScrollEvent {
                position: window.center(),
                delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-200.))),
                modifiers: Modifiers::none(),
                touch_phase: gpui::TouchPhase::Moved,
                source: gpui::ScrollSource::Pointer,
            });
        }
        seen
    });
    assert!(
        at(cx, format!("mission-card-{last}")).is_none_or(|c| !inside(c, pane)),
        "scrolling Flow leaves the board where it was"
    );
    let title = at(cx, format!("flow-title-{last}")).unwrap().center();
    cx.simulate_click(title, Modifiers::none());
    until(cx, "the step brought into view", |cx| {
        at(cx, format!("mission-card-{last}")).is_some_and(|c| inside(c, pane))
    });
    // Chosen on the board, a step opens in the editor in Flow's place,
    // and is marked in Flow once the editor closes.
    let card = at(cx, format!("mission-card-{last}")).unwrap().center();
    cx.simulate_click(card, Modifiers::none());
    until(cx, "the step's editor in Flow's place", |cx| {
        cx.debug_bounds("step-save").is_some() && cx.debug_bounds("missions-flow").is_none()
    });
    cx.simulate_keystrokes("escape");
    until(cx, "the step marked in Flow", |cx| {
        at(cx, format!("flow-chosen-{last}")).is_some()
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}
