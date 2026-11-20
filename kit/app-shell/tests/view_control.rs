//! View control end to end: `ahjo view show`, `open` and `send` against a
//! testkit node, with the headless shell connected to it showing two toy
//! pages defined here.

use ahjo_app_kit::{Answers, Ask, Host, Mounted, Page, mount};
use gpui::{Context, TestAppContext, Window, div, prelude::*};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::time::{Duration, Instant};

thread_local! {
    static REVEALED: RefCell<Vec<Ask>> = const { RefCell::new(vec![]) };
}

/// Shown first.
struct Other;

impl Page for Other {
    const ID: &'static str = "other";
    const TITLE: &'static str = "Other";
}

impl Render for Other {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

/// Answers reveals of paths and sends, keeping what it was asked.
struct Files;

impl Page for Files {
    const ID: &'static str = "files";
    const TITLE: &'static str = "Files";
    const ANSWERS: &'static [Answers] = &[Answers::Reveal("path"), Answers::Send];

    fn answer(&mut self, ask: Ask, _: &'static str, _: &mut Window, _: &mut Context<Self>) {
        REVEALED.with(|r| r.borrow_mut().push(ask));
    }
}

impl Render for Files {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

fn other(host: Host, window: &mut Window, cx: &mut gpui::App) -> Mounted {
    mount(host, window, cx, |_, _, _| Other)
}

fn files(host: Host, window: &mut Window, cx: &mut gpui::App) -> Mounted {
    mount(host, window, cx, |_, _, _| Files)
}

/// `ahjo view <args> --json` against `url`, run on `rt` while the app
/// runs: its exit code and what it printed.
fn ahjo(
    rt: &tokio::runtime::Runtime,
    cx: &mut gpui::VisualTestContext,
    url: &str,
    args: &[&str],
) -> (i32, Value) {
    let mut args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    args.extend(["--json".into(), "--node".into(), url.into()]);
    let running = rt.spawn(async move {
        let mut out = Vec::new();
        let code = ahjo_node_kit::cli::run("view", &args, &mut out)
            .await
            .expect("the command runs");
        (code, serde_json::from_slice(&out).unwrap_or(Value::Null))
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while !running.is_finished() {
        cx.run_until_parked();
        assert!(Instant::now() < deadline, "ahjo view did not finish");
        std::thread::sleep(Duration::from_millis(10));
    }
    rt.block_on(running).expect("the command's task")
}

#[gpui::test]
fn an_agent_opens_a_page_and_a_path_on_it_in_the_app(cx: &mut TestAppContext) {
    let rt = tokio::runtime::Runtime::new().expect("a runtime");
    let node = rt
        .block_on(ahjo_testkit::node::Node::start(&[], ""))
        .expect("the node starts");
    let url = node.url();
    let settings = ahjo_testkit::fresh_dir("ahjo-app-view").join("app.json");
    // The node, the link and the command run on their own threads.
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let path = settings.clone();
    let dial = url.clone();
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(&[other, files], Some(dial), Some(path), window, cx)
    });

    // The app says what it shows once its link opens.
    let deadline = Instant::now() + Duration::from_secs(10);
    let shown = loop {
        let (code, shown) = ahjo(&rt, cx, &url, &["show"]);
        assert_eq!(code, 0);
        if shown != json!({"apps": []}) {
            break shown;
        }
        assert!(
            Instant::now() < deadline,
            "the app never said what it shows"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(shown["apps"][0]["page"], "other");

    // Opened by an agent: the page comes forward and is asked to reveal
    // the path.
    let (code, opened) = ahjo(
        &rt,
        cx,
        &url,
        &["open", "--page", "files", "--path", "/tmp/x"],
    );
    assert_eq!(code, 0);
    assert_eq!(opened, json!({ "page": "files" }));
    assert_eq!(shell.read_with(cx, |s, _| s.shown_page()), Some("files"));
    let revealed = REVEALED.with(|r| r.borrow().clone());
    assert!(
        matches!(
            revealed.as_slice(),
            [Ask::Reveal { what, id, .. }] if what == "path" && id == "/tmp/x"
        ),
        "{revealed:?}"
    );

    // Another app's Send, through the node: the page that sends comes
    // forward holding the text.
    REVEALED.with(|r| r.borrow_mut().clear());
    let (code, _) = ahjo(&rt, cx, &url, &["open", "--page", "other"]);
    assert_eq!(code, 0);
    let (code, sent) = ahjo(&rt, cx, &url, &["send", "--text", "brain page a.md"]);
    assert_eq!(code, 0);
    assert_eq!(sent, json!({ "page": "files" }));
    let asked = REVEALED.with(|r| r.borrow().clone());
    assert_eq!(
        asked,
        [Ask::Send {
            reference: "brain page a.md".into(),
            target: None
        }]
    );

    rt.block_on(node.stop());
    let _ = std::fs::remove_dir_all(settings.parent().expect("its folder"));
}
