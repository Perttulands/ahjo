#![cfg(unix)]
use ahjo_node_kit::{Listing, NodeClient};
use ahjo_testkit::{node::Node, sessions};
use gpui::{Modifiers, TestAppContext, VisualTestContext};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

const AREAS: &[Listing] = &[
    sessions::LISTING,
    ahjo_work::LISTING,
    ahjo_missions::LISTING,
];
struct Running {
    url: String,
    node: String,
    stop: mpsc::Sender<()>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
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
                let dir = node.dir().join("missions/missions");
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(
                    dir.join("hello.mission.toml"),
                    include_str!("fixtures/solo-lab.mission.toml"),
                )
                .unwrap();
                ready.send((node.url(), node.id())).unwrap();
                while stopped.try_recv().is_err() {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                node.stop().await;
            })
    });
    let (url, node) = wait.recv_timeout(Duration::from_secs(20)).unwrap();
    Running {
        url,
        node,
        stop,
        thread: Some(thread),
    }
}
fn until(
    cx: &mut VisualTestContext,
    what: &str,
    mut f: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        cx.run_until_parked();
        if f(cx) {
            break;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn has(cx: &mut VisualTestContext, selector: &str) -> bool {
    cx.debug_bounds(Box::leak(selector.to_owned().into_boxed_str()))
        .is_some()
}
fn click(cx: &mut VisualTestContext, selector: &str) {
    let bounds = cx
        .debug_bounds(Box::leak(selector.to_owned().into_boxed_str()))
        .expect(selector);
    cx.simulate_click(bounds.center(), Modifiers::none());
}
#[gpui::test]
fn canvas_note_pin_opens_thread_and_preserves_edits(cx: &mut TestAppContext) {
    let running = start();
    let settings = std::env::temp_dir()
        .join(format!("ahjo-notes-{}", std::process::id()))
        .join("app.json");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let url = running.url.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(
            &[ahjo_missions_app::build],
            Some(url),
            Some(settings),
            window,
            cx,
        )
    });
    let mission = format!("mission-{}-hello", running.node);
    until(cx, "mission", |cx| has(cx, &mission));
    click(cx, &mission);
    until(cx, "canvas mode", |cx| has(cx, "mission-mode-canvas"));
    click(cx, "mission-mode-canvas");
    until(cx, "note pin", |cx| has(cx, "canvas-note-pin-fmn_work"));
    for id in ["inp_hello", "fmn_work", "end_done"] {
        let card = cx
            .debug_bounds(Box::leak(format!("mission-card-{id}").into_boxed_str()))
            .unwrap();
        let pin = cx
            .debug_bounds(Box::leak(format!("canvas-note-pin-{id}").into_boxed_str()))
            .unwrap();
        assert!(pin.origin.x >= card.origin.x && pin.origin.y >= card.origin.y);
        assert!(pin.origin.x + pin.size.width <= card.origin.x + card.size.width);
        assert!(pin.origin.y + pin.size.height <= card.origin.y + card.size.height);
    }
    click(cx, "canvas-note-pin-fmn_work");
    until(cx, "open thread", |cx| has(cx, "canvas-note-field"));
    click(cx, "canvas-note-field");
    cx.simulate_input("First note");
    click(cx, "canvas-note-save-fmn_work");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let read = || {
        runtime.block_on(async {
            let client = NodeClient::new(Some(&format!("{}/", running.url))).unwrap();
            client
                .request(http::Method::GET, "/api/missions/hello/notes", None, None)
                .await
                .unwrap()
                .0
        })
    };
    let mut id = String::new();
    until(cx, "saved note", |_| {
        let doc = read();
        if let Some(note) = doc["notes"]["elements"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|thread| thread["node_id"] == "fmn_work")
            .and_then(|thread| thread["entries"].as_array())
            .and_then(|entries| entries.first())
        {
            id = note["id"].as_str().unwrap().into();
            note["text"] == "First note"
        } else {
            false
        }
    });
    let entry = format!("canvas-note-entry-{id}-operator");
    until(cx, "rendered author and entry", |cx| has(cx, &entry));
    let edit = format!("canvas-note-edit-{id}");
    click(cx, &edit);
    click(cx, "canvas-note-field");
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("Changed note");
    click(cx, "canvas-note-close-fmn_work");
    cx.run_until_parked();
    assert!(has(cx, "canvas-note-field"), "dirty thread must stay open");
    click(cx, "canvas-note-save-fmn_work");
    until(cx, "edited note", |_| {
        read()["notes"]["elements"][0]["entries"][0]["text"] == "Changed note"
    });
    click(cx, "canvas-note-close-fmn_work");
    until(cx, "closed thread", |cx| !has(cx, "canvas-note-field"));
    click(cx, "canvas-note-fmn_work");
    until(cx, "reopened thread", |cx| has(cx, &entry));
    click(cx, &format!("canvas-note-delete-{id}"));
    until(cx, "deleted note", |_| {
        read()["notes"]["elements"]
            .as_array()
            .is_none_or(|threads| {
                threads
                    .iter()
                    .all(|thread| thread["entries"].as_array().is_none_or(Vec::is_empty))
            })
    });
    until(cx, "deleted entry removed", |cx| !has(cx, &entry));
    click(cx, "canvas-note-field");
    cx.simulate_input("Preserve this draft");
    runtime.block_on(async {
        let client = NodeClient::new(Some(&format!("{}/", running.url))).unwrap();
        let (_, etag) = client.request(http::Method::GET, "/api/missions/hello/notes", None, None).await.unwrap();
        client.request(http::Method::PATCH, "/api/missions/hello/notes", Some(serde_json::json!({"target":"fmn_work", "text":"Concurrent note", "author":"agent:reviewer", "action":"append"})), Some(&etag)).await.unwrap();
    });
    click(cx, "canvas-note-save-fmn_work");
    until(cx, "stale note rejected", |cx| has(cx, "canvas-note-error"));
    click(cx, "canvas-note-field");
    cx.simulate_keystrokes("ctrl-a ctrl-c");
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("Preserve this draft")
    );
    let doc = read();
    let entries = doc["notes"]["elements"]
        .as_array()
        .unwrap()
        .iter()
        .find(|thread| thread["node_id"] == "fmn_work")
        .unwrap()["entries"]
        .as_array()
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["text"], "Concurrent note");
}

/// The opening Flow view is where notes are written: Note beside a step
/// opens a field in place, Ctrl+Enter keeps it, and it reads there.
#[gpui::test]
fn a_note_is_written_in_flow_beside_its_step(cx: &mut TestAppContext) {
    let running = start();
    let settings = std::env::temp_dir()
        .join(format!("ahjo-flow-notes-{}", std::process::id()))
        .join("app.json");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let url = running.url.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(
            &[ahjo_missions_app::build],
            Some(url),
            Some(settings),
            window,
            cx,
        )
    });
    let mission = format!("mission-{}-hello", running.node);
    until(cx, "mission", |cx| has(cx, &mission));
    click(cx, &mission);
    until(cx, "Note beside the step", |cx| {
        has(cx, "flow-note-fmn_work")
    });
    click(cx, "flow-note-fmn_work");
    until(cx, "field in place", |cx| has(cx, "flow-note-field"));
    cx.simulate_input("Keep it short");
    cx.simulate_keystrokes("ctrl-enter");
    until(cx, "the note read in place", |cx| {
        has(cx, "flow-thread-fmn_work") && !has(cx, "flow-note-field")
    });
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let doc = runtime.block_on(async {
        let client = NodeClient::new(Some(&format!("{}/", running.url))).unwrap();
        client
            .request(http::Method::GET, "/api/missions/hello/notes", None, None)
            .await
            .unwrap()
            .0
    });
    assert_eq!(
        doc["notes"]["elements"][0]["entries"][0]["text"],
        "Keep it short"
    );
}

#[gpui::test]
fn a_note_on_a_wire_shows_under_its_step_and_on_the_board(cx: &mut TestAppContext) {
    let running = start();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let client = NodeClient::new(Some(&format!("{}/", running.url))).unwrap();
    // As an agent leaves it: on the wire from the step to its end.
    let (_, etag) = runtime
        .block_on(client.request(http::Method::GET, "/api/missions/hello/notes", None, None))
        .unwrap();
    let wrote = runtime.block_on(client.request(
        http::Method::PATCH,
        "/api/missions/hello/notes",
        Some(serde_json::json!({
            "target": "edge_done", "text": "Only when it passed", "author": "agent:test",
            "action": "append"
        })),
        Some(&etag),
    ));
    assert!(wrote.is_ok(), "{wrote:?}");
    let settings = std::env::temp_dir()
        .join(format!("ahjo-wire-notes-{}", std::process::id()))
        .join("app.json");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let url = running.url.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(
            &[ahjo_missions_app::build],
            Some(url),
            Some(settings),
            window,
            cx,
        )
    });
    let mission = format!("mission-{}-hello", running.node);
    until(cx, "mission", |cx| has(cx, &mission));
    click(cx, &mission);
    until(cx, "the wire's note in Flow", |cx| {
        has(cx, "flow-thread-edge_done")
    });
    click(cx, "mission-mode-canvas");
    until(cx, "the wire's pin on the board", |cx| {
        has(cx, "canvas-wire-note-edge_done")
    });
}

#[gpui::test]
fn a_wide_window_shows_flow_beside_the_board(cx: &mut TestAppContext) {
    let running = start();
    let settings = std::env::temp_dir()
        .join(format!("ahjo-beside-{}", std::process::id()))
        .join("app.json");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let url = running.url.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(
            &[ahjo_missions_app::build],
            Some(url),
            Some(settings),
            window,
            cx,
        )
    });
    cx.simulate_resize(gpui::size(gpui::px(1920.), gpui::px(1080.)));
    let mission = format!("mission-{}-hello", running.node);
    until(cx, "mission", |cx| has(cx, &mission));
    click(cx, &mission);
    until(cx, "Flow beside the board", |cx| {
        has(cx, "missions-board-beside") && has(cx, "mission-card-fmn_work")
    });
    // A card chosen on the board is marked in Flow.
    click(cx, "mission-card-fmn_work");
    until(cx, "the step marked in Flow", |cx| {
        has(cx, "flow-chosen-fmn_work")
    });
    // Narrower, one view again, with Edit to switch.
    cx.simulate_resize(gpui::size(gpui::px(1280.), gpui::px(900.)));
    until(cx, "Flow alone", |cx| {
        !has(cx, "missions-board-beside") && has(cx, "flow-step-fmn_work")
    });
}
