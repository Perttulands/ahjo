//! The Agents page reads an isolated node and offers its tool details.
use gpui::{Modifiers, TestAppContext, VisualTestContext};
use parvi_app_kit::link::client::LinkState;
use parvi_app_kit::{Ask, Host};
use parvi_testkit::{fresh_dir, node::Node, sessions};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

struct Running {
    url: String,
    role_file: PathBuf,
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
            let home = fresh_dir("parvi-agents-page-home");
            std::fs::create_dir_all(home.join(".claude")).unwrap();
            std::fs::write(home.join(".claude/CLAUDE.md"), "# Mine\n\nBe brief.\n").unwrap();
            std::fs::create_dir_all(home.join("proj/.git")).unwrap();
            std::fs::write(home.join("proj/.git/HEAD"), "ref: refs/heads/main\n").unwrap();
            std::fs::write(home.join("proj/AGENTS.md"), "# Project\n").unwrap();
            let config = format!("[agents]\nhome = {:?}\npath = []\n", home);
            let node = Node::start(&[sessions::LISTING, parvi_agents::LISTING], &config)
                .await
                .unwrap();
            let role_file = node.dir().join("missions/roles/fixture-writer.toml");
            std::fs::create_dir_all(role_file.parent().unwrap()).unwrap();
            std::fs::write(&role_file, "schema = 1\n[card]\nid = \"fixture-writer\"\ndisplay_name = \"Fixture writer\"\nkind = \"specialist\"\nsummary = \"Before edit\"\nstatus = \"active\"\n").unwrap();
            ready.send((node.url(), role_file, home.clone())).unwrap();
            while stopped.try_recv().is_err() {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            node.stop().await;
            std::fs::remove_dir_all(home).unwrap();
        });
    });
    let (url, role_file, home) = wait.recv().unwrap();
    Running {
        url,
        role_file,
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
fn role_library_edits_and_saves_a_fixture_card(cx: &mut TestAppContext) {
    let running = start();
    let settings = fresh_dir("parvi-agents-page-settings");
    cx.executor().allow_parking();
    cx.update(parvi_app_shell::prepare);
    let url = running.url.clone();
    let path = settings.join("app.json");
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        parvi_app_shell::start(
            &[parvi_agents_app::build],
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
    let at = cx.debug_bounds("agents-sub-skills").unwrap().center();
    cx.simulate_click(at, Modifiers::none());
    until(cx, "the reported tool", |cx| {
        cx.debug_bounds("agents-tool-claude").is_some()
    });
    let at = cx.debug_bounds("agents-tool-claude").unwrap().center();
    cx.simulate_click(at, Modifiers::none());
    let at = cx.debug_bounds("agents-sub-roles").unwrap().center();
    cx.simulate_click(at, Modifiers::none());
    // The library opens on its first role, unasked.
    until(cx, "the first role's detail", |cx| {
        cx.debug_bounds("role-edit").is_some()
    });
    let at = cx.debug_bounds("role-fixture-writer").unwrap().center();
    cx.simulate_click(at, Modifiers::none());
    until(cx, "the fixture role's detail", |cx| {
        cx.debug_bounds("role-fixture-writer-shown").is_some()
    });
    until(cx, "the role detail", |cx| {
        cx.debug_bounds("role-edit").is_some()
    });
    let at = cx.debug_bounds("role-edit").unwrap().center();
    cx.simulate_click(at, Modifiers::none());
    until(cx, "the role form", |cx| {
        cx.debug_bounds("role-field-Summary").is_some()
    });
    let at = cx.debug_bounds("role-field-Summary").unwrap().center();
    cx.simulate_click(at, Modifiers::none());
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("Updated from the library");
    let at = cx.debug_bounds("role-save").unwrap().center();
    cx.simulate_click(at, Modifiers::none());
    until(cx, "the edited card to be saved", |_| {
        std::fs::read_to_string(&running.role_file)
            .is_ok_and(|s| s.contains("summary = \"Updated from the library\""))
    });
    let saved = std::fs::read_to_string(&running.role_file).unwrap();
    assert!(saved.contains("id = \"fixture-writer\""));
    assert!(!saved.contains("Before edit"));
    until(cx, "the saved role detail", |cx| {
        cx.debug_bounds("role-edit").is_some()
    });
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
    let settings = fresh_dir("parvi-agents-page-shape");
    cx.executor().allow_parking();
    cx.update(parvi_app_shell::prepare);
    let url = running.url.clone();
    let path = settings.join("app.json");
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        parvi_app_shell::start(
            &[parvi_agents_app::build],
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
    let settings = fresh_dir("parvi-agents-page-reveal");
    cx.executor().allow_parking();
    cx.update(parvi_app_shell::prepare);
    let url = running.url.clone();
    let path = settings.join("app.json");
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        parvi_app_shell::start(
            &[parvi_agents_app::build],
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
    click(cx, "agents-sub-skills");
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
