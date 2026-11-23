//! The Brain panel over the terminals (Alt+L): it searches a testkit
//! node's brain as you type, reads a page in place, follows its link,
//! sends the page as `ahjo brain send` words it, and closes with the
//! keyboard back where it was.
use ahjo_app_kit::link::client::LinkState;
use ahjo_testkit::app::{Served, Started, click, serve, until};
use ahjo_testkit::{fresh_dir, node::Node};
use axum::{Json, routing::post};
use gpui::{FocusHandle, Modifiers, TestAppContext, VisualTestContext, point, px};
use serde_json::{Value, json};
use std::{cell::RefCell, time::Duration};

/// A node serving a corpus of a few pages in git, its search answered by
/// a stand-in Citadel.
fn start() -> Served<()> {
    serve(move || async move {
        let corpus = fresh_dir("brain-panel-corpus");
        std::fs::create_dir(corpus.join("notes")).unwrap();
        std::fs::create_dir(corpus.join("other")).unwrap();
        std::fs::write(corpus.join("other/third.md"), "# Third\n").unwrap();
        std::fs::write(
            corpus.join("notes/first.md"),
            "# First\n\n[[second|Second]]\n",
        )
        .unwrap();
        std::fs::write(corpus.join("notes/second.md"), "# Second\n\nLinked page.\n").unwrap();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(&corpus)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "-q"]);
        git(&["config", "user.name", "Fixture"]);
        git(&["config", "user.email", "fixture@example.org"]);
        git(&["add", "."]);
        git(&["commit", "-qm", "Fixture"]);
        let citadel = axum::Router::new().route(
            "/v1/library/search",
            post(|Json(query): Json<Value>| async move {
                let results = if query["query"] == "first" {
                    json!([
                        { "path": "notes/first.md", "title": "First", "collection": "notes", "kind": "note" },
                        { "path": "notes/second.md", "title": "Second", "collection": "notes" },
                        { "path": "other/third.md", "title": "Third", "collection": "other" }
                    ])
                } else {
                    json!([])
                };
                Json(json!({"results": results}))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, citadel).await.unwrap() });
        let config = format!(
            "[brain]\ncorpus = {:?}\ncitadel = {:?}\n",
            corpus.to_str().unwrap(),
            format!("http://{addr}")
        );
        let node = Node::start(&[ahjo_brain::LISTING], &config).await.unwrap();
        Started::new(node, ()).then(move || {
            server.abort();
            std::fs::remove_dir_all(corpus).unwrap();
        })
    })
}

thread_local! {
    static SENT: RefCell<Option<ahjo_app_kit::Ask>> = const { RefCell::new(None) };
    static TOY_FOCUS: RefCell<Option<FocusHandle>> = const { RefCell::new(None) };
}

/// A stand-in for the terminals: it holds the keyboard, draws what is
/// beside it and answers Send.
struct Toy {
    host: ahjo_app_kit::Host,
    focus: FocusHandle,
}
impl ahjo_app_kit::Page for Toy {
    const ID: &'static str = "sessions";
    const TITLE: &'static str = "Terminals";
    const ANSWERS: &'static [ahjo_app_kit::Answers] = &[ahjo_app_kit::Answers::Send];
    fn answer(
        &mut self,
        ask: ahjo_app_kit::Ask,
        _: &'static str,
        _: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) {
        SENT.with(|sent| *sent.borrow_mut() = Some(ask));
    }
    fn focus(&self, _: &gpui::App) -> Option<FocusHandle> {
        Some(self.focus.clone())
    }
}
impl gpui::Render for Toy {
    fn render(
        &mut self,
        _: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl gpui::IntoElement {
        use gpui::prelude::*;
        gpui::div()
            .size_full()
            .flex()
            .track_focus(&self.focus)
            .debug_selector(|| "toy-page".into())
            .child("Toy")
            .children(self.host.beside(cx).map(|view| {
                gpui::div()
                    .debug_selector(|| "toy-beside".into())
                    .size_full()
                    .child(view)
            }))
    }
}
fn toy(
    host: ahjo_app_kit::Host,
    window: &mut gpui::Window,
    cx: &mut gpui::App,
) -> ahjo_app_kit::Mounted {
    ahjo_app_kit::mount(host, window, cx, |host, _, cx| {
        let focus = cx.focus_handle();
        TOY_FOCUS.with(|f| *f.borrow_mut() = Some(focus.clone()));
        Toy { host, focus }
    })
}

fn toy_focused(cx: &mut VisualTestContext) -> bool {
    let focus = TOY_FOCUS.with(|f| f.borrow().clone()).unwrap();
    cx.update(|window, _| focus.is_focused(window))
}

#[gpui::test]
fn the_panel_searches_reads_follows_sends_and_gives_the_keyboard_back(cx: &mut TestAppContext) {
    let running = start();
    let settings = fresh_dir("brain-panel-settings");
    SENT.with(|sent| *sent.borrow_mut() = None);
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let (shell, cx) = cx.add_window_view(|window, cx| {
        ahjo_app_shell::start(
            &[toy, ahjo_brain_app::build],
            Some(running.url.clone()),
            Some(settings.join("app.json")),
            window,
            cx,
        )
    });
    until(cx, "node link", |cx| {
        shell.read_with(cx, |s, cx| {
            s.hub().read(cx).fleet.here().state == LinkState::Open
        })
    });
    assert_eq!(shell.read_with(cx, |s, _| s.shown_page()), Some("sessions"));
    assert!(
        cx.debug_bounds("tab-brain").is_none(),
        "the brain has no tab"
    );
    until(cx, "the terminals hold the keyboard", toy_focused);
    let chords = shell.update_in(cx, |shell, _, cx| shell.chords(cx));
    assert!(
        ahjo_app_kit::chords::rows(&chords)
            .iter()
            .any(|row| row.label == "Brain"),
        "Alt+L is listed among the keys"
    );

    // Alt+L opens the panel beside the terminals, what arrived lately
    // listed and the search taking the keys at once.
    cx.simulate_keystrokes("alt-l");
    until(cx, "the Brain panel", |cx| {
        cx.debug_bounds("toy-beside").is_some() && cx.debug_bounds("brain-arrival-0").is_some()
    });
    assert!(!toy_focused(cx));
    cx.simulate_input("first");
    cx.executor().advance_clock(Duration::from_millis(250));
    until(cx, "the search answers", |cx| {
        cx.debug_bounds("brain-result-0").is_some()
    });
    assert!(cx.debug_bounds("brain-count-3 pages").is_some());
    assert!(cx.debug_bounds("brain-result-kind-note").is_some());
    assert!(cx.debug_bounds("brain-result-selected-0").is_some());

    // Down marks the next result; Enter reads it in the list's place.
    cx.simulate_keystrokes("down");
    until(cx, "the mark moves", |cx| {
        cx.debug_bounds("brain-result-selected-1").is_some()
    });
    assert!(
        cx.debug_bounds("brain-page-title").is_none(),
        "still the list"
    );
    cx.simulate_keystrokes("enter");
    until(cx, "the second page read", |cx| {
        cx.debug_bounds("brain-reading-notes/second.md").is_some()
    });
    assert!(cx.debug_bounds("brain-result-0").is_none(), "one column");
    click(cx, "brain-back");
    until(cx, "Back to the list", |cx| {
        cx.debug_bounds("brain-result-selected-1").is_some()
    });

    // A result clicked reads; its link followed joins the trail.
    click(cx, "brain-result-0");
    until(cx, "the first page read", |cx| {
        cx.debug_bounds("brain-reading-notes/first.md").is_some()
    });
    let prose = cx.debug_bounds("brain-prose").unwrap();
    cx.simulate_click(prose.origin + point(px(8.), px(8.)), Modifiers::none());
    until(cx, "the linked page", |cx| {
        cx.debug_bounds("brain-reading-notes/second.md").is_some()
    });
    cx.simulate_keystrokes("alt-left");
    until(cx, "back along the trail by Alt+Left", |cx| {
        cx.debug_bounds("brain-reading-notes/first.md").is_some()
    });

    // Send asks the terminals to send the page's file, as `ahjo brain
    // send` words it.
    click(cx, "brain-send");
    let sent = SENT.with(|sent| sent.borrow().clone());
    assert!(
        matches!(&sent, Some(ahjo_app_kit::Ask::Send { reference, target: None })
            if reference.starts_with("brain page /") && reference.ends_with("/notes/first.md")),
        "{sent:?}"
    );

    // Esc from the page read closes the panel; the keyboard goes back.
    cx.simulate_keystrokes("escape");
    until(cx, "the panel closed by Esc", |cx| {
        cx.debug_bounds("toy-beside").is_none()
    });
    assert!(toy_focused(cx), "the terminals have the keyboard again");

    // Opened again it is as it was left; in the search, Esc clears what
    // is typed, then closes.
    cx.simulate_keystrokes("alt-l");
    until(cx, "the panel again, on the page read", |cx| {
        cx.debug_bounds("brain-reading-notes/first.md").is_some()
    });
    click(cx, "brain-back");
    until(cx, "the list", |cx| {
        cx.debug_bounds("brain-result-0").is_some()
    });
    cx.simulate_keystrokes("escape");
    until(cx, "the search cleared", |cx| {
        cx.debug_bounds("brain-result-0").is_none() && cx.debug_bounds("toy-beside").is_some()
    });
    cx.simulate_keystrokes("escape");
    until(cx, "the panel closed", |cx| {
        cx.debug_bounds("toy-beside").is_none()
    });
    assert!(toy_focused(cx));

    // Alt+L toggles it, the keyboard following.
    cx.simulate_keystrokes("alt-l");
    until(cx, "the panel reopened", |cx| {
        cx.debug_bounds("toy-beside").is_some()
    });
    assert!(!toy_focused(cx));
    cx.simulate_keystrokes("alt-l");
    until(cx, "the panel closed by Alt+L", |cx| {
        cx.debug_bounds("toy-beside").is_none()
    });
    assert!(toy_focused(cx));
    assert_eq!(shell.read_with(cx, |s, _| s.shown_page()), Some("sessions"));
    std::fs::remove_dir_all(settings).unwrap();
}
