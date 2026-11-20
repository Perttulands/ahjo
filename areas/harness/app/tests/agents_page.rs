//! The Agents page reads and edits the real node's reusable agent library.
#![cfg(unix)]

use ahjo_app_kit::link::client::LinkState;
use ahjo_harness_contract::agents::ShownAgent;
use ahjo_testkit::{fresh_dir, node::Node, sessions};
use gpui::{Modifiers, TestAppContext, VisualTestContext};
use serde_json::json;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

struct Running {
    url: String,
    agent_path: PathBuf,
    show: mpsc::Sender<mpsc::Sender<ShownAgent>>,
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
    let (show, requests) = mpsc::channel::<mpsc::Sender<ShownAgent>>();
    let thread = std::thread::spawn(move || {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async move {
                let home = fresh_dir("ahjo-agents-page-home");
                let config = format!("[harness]\nhome = {:?}\npath = []\n", home);
                let node = Node::start(&[sessions::LISTING, ahjo_harness::LISTING], &config)
                    .await
                    .unwrap();
                let created = node.call("agent_create", json!({
                "id":"custom", "name":"Custom Writer", "instructions":"Original guidance",
                "harness":"openai-codex", "model":"default", "effort":"high"
            })).await.unwrap();
                node.call(
                    "agent_edit",
                    json!({"agent":"custom", "etag":created["etag"], "notes":[
                        {"at":"2026-11-19T12:00:00Z", "by":"human:ui", "text":"Older note"},
                        {"at":"2026-11-20T12:00:00Z", "by":"human:ui", "text":"Newer note"}
                    ]}),
                )
                .await
                .unwrap();
                let root = node.dir().join("missions");
                std::fs::create_dir_all(root.join("missions")).unwrap();
                std::fs::write(
                    root.join("missions/draft.mission.toml"),
                    r#"schema = 6
id = "msn_draft"
slug = "draft"
title = "Draft mission"
rev = 1
[[step]]
id = "step_write"
formation = "solo"
title = "Write"
[[step.seat]]
id = "seat_writer"
label = "Writer"
agent = "custom"
"#,
                )
                .unwrap();
                ready
                    .send((node.url(), root.join("agents/custom.toml")))
                    .unwrap();
                while stopped.try_recv().is_err() {
                    while let Ok(reply) = requests.try_recv() {
                        let result = node
                            .call("agent_show", json!({"agent":"custom"}))
                            .await
                            .unwrap();
                        reply.send(serde_json::from_value(result).unwrap()).unwrap();
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                node.stop().await;
                std::fs::remove_dir_all(home).unwrap();
            });
    });
    let (url, agent_path) = wait.recv_timeout(Duration::from_secs(20)).unwrap();
    Running {
        url,
        agent_path,
        show,
        stop,
        thread: Some(thread),
    }
}
fn until(
    cx: &mut VisualTestContext,
    what: &str,
    mut ok: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        cx.run_until_parked();
        if ok(cx) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn click(cx: &mut VisualTestContext, selector: &'static str) {
    until(cx, selector, |cx| cx.debug_bounds(selector).is_some());
    let at = cx.debug_bounds(selector).unwrap().center();
    cx.simulate_click(at, Modifiers::none());
}

#[gpui::test]
fn agents_can_be_selected_and_edited_without_losing_their_details(cx: &mut TestAppContext) {
    let running = start();
    let settings = fresh_dir("ahjo-agents-page-settings");
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let url = running.url.clone();
    let path = settings.join("app.json");
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(
            &[ahjo_harness_app::build_agents],
            Some(url),
            Some(path),
            window,
            cx,
        )
    });
    until(cx, "node connection", |cx| {
        shell.read_with(cx, |s, cx| {
            s.hub().read(cx).fleet.here().state == LinkState::Open
        })
    });
    for selector in [
        "agents-agent-scout",
        "agents-agent-planner",
        "agents-agent-builder",
        "agents-agent-judge",
        "agents-agent-orchestrator",
        "agents-agent-debugger",
        "agents-agent-reviewer",
        "agents-agent-custom",
    ] {
        until(cx, selector, |cx| cx.debug_bounds(selector).is_some());
    }
    click(cx, "agents-agent-custom");
    until(cx, "custom agent details", |cx| {
        cx.debug_bounds("agents-name").is_some()
            && cx.debug_bounds("agents-instructions").is_some()
            && cx.debug_bounds("agents-note-1").is_some()
            && cx.debug_bounds("agents-usage-draft").is_some()
    });
    let newest = cx
        .debug_bounds("agents-note-at-2026-11-20T12:00:00Z")
        .unwrap();
    let oldest = cx
        .debug_bounds("agents-note-at-2026-11-19T12:00:00Z")
        .unwrap();
    assert!(
        newest.origin.y < oldest.origin.y,
        "newest note is shown first"
    );
    let first = cx.debug_bounds("agents-note-0").unwrap();
    assert_eq!(first.origin.y, newest.origin.y);
    click(cx, "agents-edit");
    click(cx, "agents-name");
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("Custom Editor");
    click(cx, "agents-instructions");
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("Original guidance. Added evidence.");
    click(cx, "agents-model");
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("gpt-6");
    click(cx, "agents-effort-ultra");
    click(cx, "agents-save");
    until(cx, "saved TOML", |_| {
        std::fs::read_to_string(&running.agent_path)
            .is_ok_and(|s| s.contains("Custom Editor") && s.contains("Added evidence."))
    });
    let (reply, answer) = mpsc::channel();
    running.show.send(reply).unwrap();
    let shown = answer.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(shown.agent.name, "Custom Editor");
    assert_eq!(
        shown.agent.instructions,
        "Original guidance. Added evidence."
    );
    assert_eq!(shown.agent.harness.to_string(), "openai-codex");
    assert_eq!(shown.agent.effort.to_string(), "ultra");
    assert_eq!(shown.agent.model, "gpt-6");
    assert_eq!(shown.agent.notes.len(), 2);
    assert_eq!(shown.agent.notes[0].text, "Older note");
    assert_eq!(shown.agent.notes[1].text, "Newer note");
    assert!(shown.usage.complete);
    assert_eq!(shown.usage.usage.len(), 1);
    assert_eq!(shown.usage.usage[0].mission_title, "Draft mission");
    std::fs::remove_dir_all(settings).unwrap();
}
