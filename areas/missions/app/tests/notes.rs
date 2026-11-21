#![cfg(unix)]
mod common;
use ahjo_node_kit::{Listing, NodeClient};
use ahjo_testkit::{node::Node, sessions};
use gpui::{Modifiers, TestAppContext, VisualTestContext};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

const AREAS: &[Listing] = &[
    sessions::LISTING,
    ahjo_issue::LISTING,
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
/// Show Flow in the panel, from the mission's "…" menu.
fn show_flow(cx: &mut VisualTestContext) {
    until(cx, "the mission's menu", |cx| has(cx, "mission-more"));
    click(cx, "mission-more");
    until(cx, "Show Flow", |cx| has(cx, "menu-label-Show Flow"));
    click(cx, "menu-label-Show Flow");
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
    until(cx, "note pin", |cx| has(cx, "canvas-note-pin-fmn_work"));
    // Each card's note mark sits astride its top-right corner, as a count
    // on a corner does: on the card, over none of its words.
    for id in ["inp_hello", "fmn_work", "end_done"] {
        let card = cx
            .debug_bounds(Box::leak(format!("mission-card-{id}").into_boxed_str()))
            .unwrap();
        let pin = cx
            .debug_bounds(Box::leak(format!("canvas-note-pin-{id}").into_boxed_str()))
            .unwrap();
        let corner = (
            f32::from(card.origin.x + card.size.width),
            f32::from(card.origin.y),
        );
        let middle = (f32::from(pin.center().x), f32::from(pin.center().y));
        assert!(
            (middle.0 - corner.0).abs() <= 1. && (middle.1 - corner.1).abs() <= 1.,
            "{id}: the mark {pin:?} is not on the corner of {card:?}"
        );
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
            common::call(
                &client,
                "mission_notes",
                serde_json::json!({"mission":"hello"}),
            )
            .await
            .unwrap()
        })
    };
    let mut id = String::new();
    until(cx, "saved note", |_| {
        let doc = read();
        if let Some(note) = doc["notes"]["elements"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|thread| thread["card_id"] == "fmn_work")
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
    // Closed, the thread is its count on the mark, which opens it again.
    click(cx, "canvas-note-pin-fmn_work");
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
        common::call(&client, "mission_note", serde_json::json!({"mission":"hello", "on":"fmn_work", "text":"Concurrent note", "author":"reviewer"})).await.unwrap();
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
        .find(|thread| thread["card_id"] == "fmn_work")
        .unwrap()["entries"]
        .as_array()
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["text"], "Concurrent note");
}

/// Flow, the mission in words, shown from the mission's menu in place of
/// Activity, is a place notes are written too: Note beside a step opens a
/// field in place, Ctrl+Enter keeps it, and it reads there.
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
    show_flow(cx);
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
        common::call(
            &client,
            "mission_notes",
            serde_json::json!({"mission":"hello"}),
        )
        .await
        .unwrap()
    });
    assert_eq!(
        doc["notes"]["elements"][0]["entries"][0]["text"],
        "Keep it short"
    );
}

/// A note an agent left on a wire is read in Activity, named by the cards
/// the wire joins, and opens on the wire on the board when chosen there.
#[gpui::test]
fn a_note_on_a_wire_shows_in_activity_and_opens_on_the_board(cx: &mut TestAppContext) {
    let running = start();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let client = NodeClient::new(Some(&format!("{}/", running.url))).unwrap();
    // As an agent leaves it: on the wire from the step to its end.
    let wrote = runtime.block_on(common::call(
        &client,
        "mission_note",
        serde_json::json!({
            "mission":"hello", "on":"edge_done", "text":"Only when it passed", "author":"test"
        }),
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
    let doc = runtime
        .block_on(common::call(
            &client,
            "mission_notes",
            serde_json::json!({"mission":"hello"}),
        ))
        .unwrap();
    let id = doc["notes"]["elements"][0]["entries"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let entry = format!("activity-note-edge_done-{id}");
    until(cx, "the wire's note in Activity", |cx| has(cx, &entry));
    // Kept off the board until asked for.
    assert!(!has(cx, "canvas-note-edge_done"));
    click(cx, &entry);
    until(cx, "the wire's thread on the board", |cx| {
        has(cx, "canvas-note-edge_done") && has(cx, "canvas-note-field")
    });
}

#[gpui::test]
fn a_wide_window_shows_its_panel_beside_the_board_and_a_narrow_one_behind_its_toggle(
    cx: &mut TestAppContext,
) {
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
    until(cx, "Activity beside the board", |cx| {
        has(cx, "missions-flow") && has(cx, "activity-write") && has(cx, "mission-card-fmn_work")
    });
    // Flow takes Activity's place; a step chosen in it is marked on both.
    show_flow(cx);
    until(cx, "Flow beside the board", |cx| {
        has(cx, "flow-title-fmn_work")
    });
    click(cx, "flow-title-fmn_work");
    until(cx, "the step marked in Flow", |cx| {
        has(cx, "flow-chosen-fmn_work")
    });
    // Narrower, the board alone, the panel behind its toggle.
    cx.simulate_resize(gpui::size(gpui::px(1280.), gpui::px(900.)));
    until(cx, "the board alone", |cx| {
        !has(cx, "missions-flow") && has(cx, "mission-card-fmn_work")
    });
    click(cx, "mission-flow");
    until(cx, "Flow opened by hand", |cx| {
        has(cx, "missions-flow") && has(cx, "flow-step-fmn_work")
    });
    // Kept as left, wider again.
    click(cx, "mission-flow");
    until(cx, "Flow closed by hand", |cx| !has(cx, "missions-flow"));
    cx.simulate_resize(gpui::size(gpui::px(1920.), gpui::px(1080.)));
    cx.run_until_parked();
    assert!(!has(cx, "missions-flow"));
}
