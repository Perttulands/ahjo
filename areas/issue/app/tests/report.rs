//! Reporting from another page against an isolated Beads store.
#![cfg(unix)]
use ahjo_app_kit::{Host, Mounted, Page, mount};
use ahjo_node_kit::{Built, Listing, Setup};
use ahjo_testkit::app::until;
use ahjo_testkit::node::Node;
use gpui::{App, Context, IntoElement, Render, TestAppContext, Window, div, prelude::*};
use std::sync::{OnceLock, mpsc};
static GH: OnceLock<String> = OnceLock::new();
static BD: OnceLock<String> = OnceLock::new();
fn build_work(setup: &Setup) -> anyhow::Result<Built> {
    ahjo_issue::build_with(
        setup,
        ahjo_issue::Bd::new(BD.get().unwrap()),
        ahjo_issue::Gh::new(GH.get().unwrap()),
    )
}
const WORK: Listing = Listing {
    build: build_work,
    ..ahjo_issue::LISTING
};
use std::time::Duration;

struct Toy;
impl Page for Toy {
    const ID: &'static str = "toy";
    const TITLE: &'static str = "Toy";
}
impl Render for Toy {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child("Toy")
    }
}
fn toy(host: Host, window: &mut Window, cx: &mut App) -> Mounted {
    mount(host, window, cx, |_, _, _| Toy)
}

#[gpui::test]
fn reports_file_to_beads_and_github_with_context_and_escape_discards(cx: &mut TestAppContext) {
    let dir = ahjo_testkit::fresh_dir("ahjo-report-store");
    std::fs::create_dir(dir.join(".beads")).unwrap();
    std::fs::write(dir.join(".beads/prefix"), "reporttest").unwrap();
    let bd = dir.join("fake-bd");
    ahjo_testkit::tmux::write_script(
        &bd,
        include_str!("../../node/tests/fixtures/work/fake-bd.sh"),
    );
    BD.set(bd.display().to_string()).unwrap();
    let gh = dir.join("fake-gh");
    let args = dir.join("gh-args");
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../node/tests/fixtures/work/ahjo.json");
    let item = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../node/tests/fixtures/work/item.json");
    ahjo_testkit::tmux::write_script(
        &gh,
        format!(
            "#!/bin/sh\nif [ \"$1\" = issue ]; then printf '%s\\n' \"$@\" > '{}'; echo https://github.com/o/r/issues/51; else case \"$*\" in *number=51*) cat '{}';; *) cat '{}';; esac; fi\n",
            args.display(),
            item.display(),
            fixture.display()
        ),
    );
    GH.set(gh.display().to_string()).unwrap();
    let (ready, wait) = mpsc::channel();
    let (stop, stopped) = mpsc::channel();
    let store = dir.clone();
    let thread = std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let config = format!(
                "[issue]\nprojects = [{:?}, {:?}]\n",
                format!("beads {}", store.display()),
                "github o/r"
            );
            let node = Node::start(&[WORK], &config).await.unwrap();
            let list = vec!["list".to_owned(), "--node".into(), node.url()];
            let mut output = Vec::new();
            assert_eq!(
                ahjo_node_kit::cli::run("issue", &list, &mut output)
                    .await
                    .unwrap(),
                0
            );
            assert!(String::from_utf8(output).unwrap().contains("id=o/r#70"));
            let show = vec![
                "show".to_owned(),
                "--node".into(),
                node.url(),
                "--ref".into(),
                "gh:o/r#51".into(),
            ];
            let mut output = Vec::new();
            assert_eq!(
                ahjo_node_kit::cli::run("issue", &show, &mut output)
                    .await
                    .unwrap(),
                0
            );
            let output = String::from_utf8(output).unwrap();
            assert!(output.contains("gh:o/r#51") && output.contains("Example work item"));
            ready.send(node.url()).unwrap();
            while stopped.try_recv().is_err() {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            node.stop().await;
        });
    });
    let url = wait.recv().unwrap();
    let settings = ahjo_testkit::fresh_dir("ahjo-report-settings");
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(
            &[toy, ahjo_issue_app::build],
            Some(url),
            Some(settings.join("app.json")),
            window,
            cx,
        )
    });
    until(cx, "link", |cx| {
        shell.read_with(cx, |s, cx| {
            s.hub().read(cx).fleet.here().state == ahjo_app_kit::link::client::LinkState::Open
        })
    });
    let host = shell.read_with(cx, |s, _| Host::new(s.hub()));
    let sessions = cx
        .update(|_, cx| host.model::<ahjo_terminals_contract::SessionList>(cx))
        .unwrap();
    sessions.update(cx, |s, cx| {
        s.focused = Some(ahjo_terminals_contract::FocusedSession {
            node: "node".into(),
            user: "user".into(),
            name: "alpha".into(),
        });
        cx.notify();
    });
    cx.simulate_keystrokes("alt-r");
    until(cx, "what goes with it", |cx| {
        cx.debug_bounds("report-evidence").is_some()
    });
    assert_eq!(shell.read_with(cx, |s, _| s.shown_page()), Some("toy"));
    until(cx, "project", |cx| {
        cx.debug_bounds("report-project-0").is_some()
    });
    cx.simulate_input("Title");
    cx.simulate_keystrokes("enter");
    cx.simulate_input("Body");
    cx.simulate_keystrokes("ctrl-enter");
    until(cx, "filed", |cx| {
        shell.read_with(cx, |s, cx| {
            s.hub()
                .read(cx)
                .frame
                .read(cx)
                .status
                .as_ref()
                .is_some_and(|s| s.text.starts_with("Filed reporttest-"))
        })
    });
    assert_eq!(shell.read_with(cx, |s, _| s.shown_page()), Some("toy"));
    let filed = dir.join(".beads/reporttest-1");
    assert_eq!(
        std::fs::read_to_string(filed.join("title")).unwrap(),
        "Title"
    );
    // gpui's test platform has no pixels to capture, and the node has no
    // terminals: the report says so rather than failing.
    let description = std::fs::read_to_string(filed.join("description")).unwrap();
    assert!(
        description.starts_with("Body\n\nFiled from Ahjo Swarm's report.\nScreenshot: not taken: "),
        "{description}"
    );
    assert!(description.contains("\nFocused session: alpha on node node, user user\n"));
    assert!(description.contains("\nIts screen was not read: "));
    assert_eq!(std::fs::read_to_string(filed.join("type")).unwrap(), "bug");
    cx.simulate_keystrokes("alt-r");
    until(cx, "GitHub project", |cx| {
        cx.debug_bounds("report-project-1").is_some()
    });
    let point = cx.debug_bounds("report-project-1").unwrap().center();
    cx.simulate_click(point, gpui::Modifiers::none());
    let point = cx.debug_bounds("report-field").unwrap().center();
    cx.simulate_click(point, gpui::Modifiers::none());
    cx.simulate_input("GitHub title");
    cx.simulate_keystrokes("enter");
    cx.simulate_input("GitHub body");
    cx.simulate_keystrokes("ctrl-enter");
    until(cx, "GitHub filed", |cx| {
        shell.read_with(cx, |s, cx| {
            s.hub()
                .read(cx)
                .frame
                .read(cx)
                .status
                .as_ref()
                .is_some_and(|s| s.text == "Filed 51")
        })
    });
    let args = std::fs::read_to_string(&args).unwrap();
    assert!(
        args.starts_with(
            "issue\ncreate\n--repo\no/r\n--title\nGitHub title\n--body\nGitHub body\n\nFiled from Ahjo Swarm's report.\n"
        ),
        "{args}"
    );
    assert_eq!(shell.read_with(cx, |s, _| s.shown_page()), Some("toy"));
    cx.simulate_keystrokes("alt-r");
    until(cx, "report reopened", |cx| {
        cx.debug_bounds("report-box").is_some()
    });
    cx.simulate_input("Discard me");
    cx.simulate_keystrokes("escape");
    until(cx, "report closed", |cx| {
        cx.debug_bounds("report-box").is_none()
    });
    assert_eq!(
        std::fs::read_to_string(dir.join(".beads/count")).unwrap(),
        "1"
    );
    stop.send(()).unwrap();
    thread.join().unwrap();
    std::fs::remove_dir_all(dir).unwrap();
}
