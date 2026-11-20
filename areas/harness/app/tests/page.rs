//! The Harness page reads an isolated node and offers its tool details.
use ahjo_app_kit::link::client::LinkState;
use ahjo_app_kit::{Ask, Host};
use ahjo_testkit::{fresh_dir, node::Node, sessions};
use gpui::{Modifiers, TestAppContext, VisualTestContext};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

struct Running {
    url: String,
    home: PathBuf,
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
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let home = fresh_dir("ahjo-harness-page-home");
            std::fs::create_dir_all(home.join(".claude")).unwrap();
            std::fs::write(home.join(".claude/CLAUDE.md"), "# Mine\n\nBe brief.\n").unwrap();
            std::fs::create_dir_all(home.join("proj/.git")).unwrap();
            std::fs::write(home.join("proj/.git/HEAD"), "ref: refs/heads/main\n").unwrap();
            std::fs::write(home.join("proj/AGENTS.md"), "# Project\n").unwrap();
            let config = format!("[harness]\nhome = {:?}\npath = []\n", home);
            let node = Node::start(&[sessions::LISTING, ahjo_harness::LISTING], &config)
                .await
                .unwrap();
            ready.send((node.url(), home.clone())).unwrap();
            while stopped.try_recv().is_err() {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            node.stop().await;
            std::fs::remove_dir_all(home).unwrap();
        });
    });
    let (url, home) = wait.recv().unwrap();
    Running {
        url,
        home,
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
#[gpui::test]
fn skills_and_agent_views_remain_accessible(cx: &mut TestAppContext) {
    let running = start();
    let settings = fresh_dir("ahjo-harness-page-settings");
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let url = running.url.clone();
    let path = settings.join("app.json");
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(
            &[ahjo_harness_app::build],
            Some(url),
            Some(path),
            window,
            cx,
        )
    });
    until(cx, "the node connection", |cx| {
        shell.read_with(cx, |s, cx| {
            s.hub().read(cx).fleet.here().state == LinkState::Open
        })
    });
    let at = cx.debug_bounds("harness-sub-skills").unwrap().center();
    cx.simulate_click(at, Modifiers::none());
    until(cx, "the reported tool", |cx| {
        cx.debug_bounds("agents-tool-claude").is_some()
    });
    let at = cx.debug_bounds("agents-tool-claude").unwrap().center();
    cx.simulate_click(at, Modifiers::none());
    let at = cx.debug_bounds("harness-sub-agent").unwrap().center();
    cx.simulate_click(at, Modifiers::none());
    until(cx, "the agent folder", |cx| {
        cx.debug_bounds("shape-folder").is_some()
    });
    assert!(cx.debug_bounds("harness-sub-roles").is_none());
    std::fs::remove_dir_all(settings).unwrap();
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    until(cx, selector, |cx| cx.debug_bounds(selector).is_some());
    let at = cx.debug_bounds(selector).unwrap().center();
    cx.simulate_click(at, Modifiers::none());
}

#[gpui::test]
fn agent_view_reads_a_folder_and_saves_an_instruction_in_place(cx: &mut TestAppContext) {
    let running = start();
    let settings = fresh_dir("ahjo-harness-page-shape");
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let url = running.url.clone();
    let path = settings.join("app.json");
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(
            &[ahjo_harness_app::build],
            Some(url),
            Some(path),
            window,
            cx,
        )
    });
    until(cx, "the node connection", |cx| {
        shell.read_with(cx, |s, cx| {
            s.hub().read(cx).fleet.here().state == LinkState::Open
        })
    });
    click(cx, "shape-folder");
    cx.simulate_input(&running.home.join("proj").to_string_lossy());
    cx.simulate_keystrokes("enter");
    // The operator's own file first, then the project's.
    until(cx, "the instructions", |cx| {
        cx.debug_bounds("shape-instruction-1").is_some()
    });
    assert!(cx.debug_bounds("shape-tokens").is_some());
    click(cx, "shape-instruction-0");
    click(cx, "shape-edit");
    cx.simulate_keystrokes("ctrl-end");
    cx.simulate_input("Say why.\n");
    click(cx, "shape-save");
    let mine = running.home.join(".claude/CLAUDE.md");
    until(cx, "the edit on disk", |_| {
        std::fs::read_to_string(&mine).is_ok_and(|s| s.ends_with("Be brief.\nSay why.\n"))
    });
    // A hook added in place lands in the operator's settings.
    click(cx, "shape-hook-add");
    click(cx, "shape-hook-field-Run");
    cx.simulate_input("echo started");
    click(cx, "shape-hook-save");
    let settings_file = running.home.join(".claude/settings.json");
    until(cx, "the hook on disk", |_| {
        std::fs::read_to_string(&settings_file).is_ok_and(|s| s.contains("echo started"))
    });
    // The folder is remembered for the next start.
    until(cx, "the folder kept in app.json", |cx| {
        cx.executor().advance_clock(Duration::from_millis(300));
        std::fs::read_to_string(settings.join("app.json")).is_ok_and(|s| s.contains("proj"))
    });
    std::fs::remove_dir_all(settings).unwrap();
}

#[gpui::test]
fn a_reveal_of_an_agent_opens_its_folder_by_its_harness(cx: &mut TestAppContext) {
    let running = start();
    let settings = fresh_dir("ahjo-harness-page-reveal");
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let url = running.url.clone();
    let path = settings.join("app.json");
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(
            &[ahjo_harness_app::build],
            Some(url),
            Some(path),
            window,
            cx,
        )
    });
    until(cx, "the node connection", |cx| {
        shell.read_with(cx, |s, cx| {
            s.hub().read(cx).fleet.here().state == LinkState::Open
        })
    });
    // The operator is on another subpage when Sessions asks.
    click(cx, "harness-sub-skills");
    let node = shell.read_with(cx, |s, cx| s.hub().read(cx).live_nodes()[0].id.clone());
    let sessions = shell.read_with(cx, |s, _| Host::new(s.hub()).for_page("sessions", &[]));
    let proj = running.home.join("proj").to_string_lossy().into_owned();
    let ask = Ask::Reveal {
        what: "agent".into(),
        id: proj.clone(),
        node: Some(node),
        part: Some("codex".into()),
        beside: false,
    };
    cx.update(|_, cx| sessions.ask(ask, cx)).unwrap();
    // Codex reads only the project's AGENTS.md; Claude Code would read
    // the operator's CLAUDE.md first.
    until(cx, "the Codex instructions", |cx| {
        cx.debug_bounds("shape-instruction-0").is_some()
    });
    assert!(cx.debug_bounds("shape-instruction-1").is_none());
    until(cx, "the folder and harness kept", |cx| {
        cx.executor().advance_clock(Duration::from_millis(300));
        std::fs::read_to_string(settings.join("app.json"))
            .is_ok_and(|s| s.contains(&proj) && s.contains("\"codex\""))
    });
    std::fs::remove_dir_all(settings).unwrap();
}

#[gpui::test]
fn the_reader_follows_a_switch_of_harness(cx: &mut TestAppContext) {
    let running = start();
    let settings = fresh_dir("ahjo-harness-page-switch");
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let url = running.url.clone();
    let path = settings.join("app.json");
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(
            &[ahjo_harness_app::build],
            Some(url),
            Some(path),
            window,
            cx,
        )
    });
    until(cx, "the node connection", |cx| {
        shell.read_with(cx, |s, cx| {
            s.hub().read(cx).fleet.here().state == LinkState::Open
        })
    });
    let opened = |path: PathBuf| -> &'static str {
        Box::leak(format!("shape-open-{}", path.to_string_lossy()).into_boxed_str())
    };
    let mine = opened(running.home.join(".claude/CLAUDE.md"));
    let project = opened(running.home.join("proj/AGENTS.md"));
    click(cx, "shape-folder");
    cx.simulate_input(&running.home.join("proj").to_string_lossy());
    cx.simulate_keystrokes("enter");
    // Claude Code's own file, which Codex does not read, is opened.
    click(cx, "shape-instruction-0");
    until(cx, "the operator's CLAUDE.md open", |cx| {
        cx.debug_bounds(mine).is_some()
    });
    click(cx, "shape-harness-codex");
    until(cx, "Codex's file open instead", |cx| {
        cx.debug_bounds(project).is_some()
    });
    assert!(cx.debug_bounds(mine).is_none());
    // A file both harnesses read stays open across the switch back.
    click(cx, "shape-harness-claude");
    until(cx, "Claude Code's instructions", |cx| {
        cx.debug_bounds("shape-instruction-1").is_some()
    });
    assert!(cx.debug_bounds(project).is_some());
    std::fs::remove_dir_all(settings).unwrap();
}
