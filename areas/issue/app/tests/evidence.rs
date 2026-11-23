//! A report carries what the window showed: the operator opens a page
//! and a panel with a session focused, presses Alt+R, types a title and
//! files; the issue names the page, the panel and the session with its
//! last lines, and points to the window's picture on the node. Removed,
//! nothing goes with it. Linux and macOS only; skips without tmux unless
//! AHJO_REQUIRE_TMUX is set.
#![cfg(unix)]
use ahjo_app_kit::capture::Picture;
use ahjo_app_kit::panel::{Panel, PanelView};
use ahjo_app_kit::{Error, Host, Mounted, Page, mount};
use ahjo_node_kit::{Built, Listing, Setup};
use ahjo_testkit::app::{Served, Started, click, serve, until};
use ahjo_testkit::node::Node;
use ahjo_testkit::tmux::{Server, eventually, session_text};
use gpui::{
    AnyElement, App, Context, IntoElement, Render, TestAppContext, Window, div, prelude::*,
};
use std::path::PathBuf;
use std::sync::OnceLock;

static BD: OnceLock<String> = OnceLock::new();
fn build_work(setup: &Setup) -> anyhow::Result<Built> {
    ahjo_issue::build_with(
        setup,
        ahjo_issue::Bd::new(BD.get().unwrap()),
        ahjo_issue::Gh::new("gh-is-not-used"),
    )
}
const WORK: Listing = Listing {
    build: build_work,
    ..ahjo_issue::LISTING
};
const AREAS: &[Listing] = &[ahjo_terminals::LISTING, ahjo_files::LISTING, WORK];

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

/// What the toy page shows in the panel.
struct Notes;
impl Render for Notes {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().child("Notes")
    }
}
impl PanelView for Notes {
    fn head(&mut self, _: &mut Window, _: &mut Context<Self>) -> AnyElement {
        div().child("Notes").into_any_element()
    }
}

/// gpui's test platform has no pixels: the window is a small picture.
fn picture(_: &Window) -> Result<Picture, Error> {
    Ok(Picture {
        rgba: [40, 44, 52, 255].repeat(4 * 3),
        width: 4,
        height: 3,
    })
}

struct Fixture {
    /// The Beads store the fake bd files into.
    store: PathBuf,
}

/// A node with a session `alpha` that printed a line, files for drops,
/// and a project whose bd is a fake writing each issue into its store.
fn start() -> Option<Served<Fixture>> {
    let tmux = Server::start_in("issue-report-evidence")?;
    Some(serve(move || async move {
        let store = tmux.dir().join("project");
        std::fs::create_dir_all(store.join(".beads")).unwrap();
        std::fs::write(store.join(".beads/prefix"), "evidence").unwrap();
        let bd = tmux.dir().join("fake-bd");
        ahjo_testkit::tmux::write_script(
            &bd,
            include_str!("../../node/tests/fixtures/work/fake-bd.sh"),
        );
        BD.set(bd.display().to_string()).unwrap();
        tmux.new_session("alpha", 80, 24);
        tmux.run(&[
            "send-keys",
            "-t",
            "alpha",
            "echo the-panel-will-not-close",
            "Enter",
        ]);
        let config = format!(
            "[terminals]\nsocket = {:?}\n[files]\nroots = [{:?}]\ndrops = {:?}\n[issue]\nprojects = [{:?}]\n",
            tmux.socket_path(),
            tmux.dir(),
            tmux.dir().join("drops"),
            format!("beads {}", store.display()),
        );
        let node = Node::start(AREAS, &config).await.expect("a node");
        let sessions = node.sessions();
        eventually("alpha's echo", || {
            session_text(sessions.as_ref(), "alpha")?
                .contains("\nthe-panel-will-not-close")
                .then_some(())
        })
        .await;
        Started::new(node, Fixture { store }).then(move || drop(tmux))
    }))
}

fn filed(store: &std::path::Path, n: usize) -> String {
    std::fs::read_to_string(store.join(format!(".beads/evidence-{n}/description")))
        .expect("the issue was filed")
}

fn file_report(
    cx: &mut gpui::VisualTestContext,
    shell: &gpui::Entity<ahjo_app_shell::Shell>,
    input: &str,
) {
    cx.simulate_input(input);
    cx.simulate_keystrokes("ctrl-enter");
    until(cx, "filed", |cx| {
        shell.read_with(cx, |s, cx| {
            s.hub()
                .read(cx)
                .frame
                .read(cx)
                .status
                .as_ref()
                .is_some_and(|s| s.text.starts_with("Filed evidence-"))
        })
    });
    shell.update(cx, |s, cx| {
        s.hub().update(cx, |h, cx| {
            h.frame.update(cx, |f, cx| {
                f.status = None;
                cx.notify();
            })
        })
    });
}

#[gpui::test]
fn a_report_names_the_page_panel_and_session_and_points_to_the_picture(cx: &mut TestAppContext) {
    let Some(running) = start() else {
        assert!(
            std::env::var_os("AHJO_REQUIRE_TMUX").is_none(),
            "AHJO_REQUIRE_TMUX is set but tmux did not start"
        );
        return;
    };
    let settings = ahjo_testkit::fresh_dir("ahjo-report-evidence-settings");
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let url = running.url.clone();
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        let shell = ahjo_app_shell::start(
            &[toy, ahjo_issue_app::build],
            Some(url),
            Some(settings.join("app.json")),
            window,
            cx,
        );
        shell.hub().update(cx, |hub, _| {
            hub.build = "0.0.1+test";
            hub.capture = picture;
        });
        shell
    });
    until(cx, "link", |cx| {
        shell.read_with(cx, |s, cx| {
            s.hub().read(cx).fleet.here().state == ahjo_app_kit::link::client::LinkState::Open
        })
    });
    // The toy page shows, with its notes in the panel and alpha focused
    // as the Sessions page leaves it.
    let host = shell.read_with(cx, |s, _| Host::new(s.hub()));
    let toy = host.for_page("toy", &[]);
    cx.update(|_, cx| {
        let notes = cx.new(|_| Notes);
        toy.show_panel(Panel::new("notes", notes), cx);
    });
    let sessions = cx
        .update(|_, cx| host.model::<ahjo_terminals_contract::SessionList>(cx))
        .unwrap();
    let node = running.id.clone();
    sessions.update(cx, |s, cx| {
        s.focused = Some(ahjo_terminals_contract::FocusedSession {
            node,
            user: String::new(),
            name: "alpha".into(),
        });
        cx.notify();
    });
    until(cx, "the panel", |cx| {
        cx.debug_bounds("panel-close").is_some()
    });

    cx.simulate_keystrokes("alt-r");
    until(cx, "what goes with it", |cx| {
        cx.debug_bounds("report-evidence").is_some()
    });
    until(cx, "the project", |cx| {
        cx.debug_bounds("report-project-0").is_some()
    });
    file_report(cx, &shell, "This panel will not close");

    let text = filed(&running.store, 1);
    for named in [
        "Page: toy",
        "Panel: toy/notes (open)",
        "App: ahjo-swarm 0.0.1+test",
        &format!("Node: {} at {}", running.id, running.url),
        "Focused session: alpha",
        "the-panel-will-not-close",
    ] {
        assert!(text.contains(named), "the report names {named:?}:\n{text}");
    }
    let picture = text
        .lines()
        .find_map(|l| l.strip_prefix("Screenshot: "))
        .and_then(|l| l.split(" (").next())
        .unwrap_or_else(|| panic!("the report points to a picture:\n{text}"));
    let bytes = std::fs::read(picture).expect("the picture is on the node");
    assert!(
        bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "{picture} is a PNG"
    );
    assert!(
        picture.starts_with(
            running
                .store
                .parent()
                .unwrap()
                .join("drops")
                .to_str()
                .unwrap()
        ),
        "{picture} is among the node's drops"
    );

    // Removed, nothing goes with the next one.
    cx.simulate_keystrokes("alt-r");
    until(cx, "what goes with it", |cx| {
        cx.debug_bounds("report-evidence").is_some()
    });
    until(cx, "the project", |cx| {
        cx.debug_bounds("report-project-0").is_some()
    });
    click(cx, "report-evidence-remove");
    until(cx, "nothing goes with it", |cx| {
        cx.debug_bounds("report-evidence").is_none()
    });
    let point = cx.debug_bounds("report-field").unwrap().center();
    cx.simulate_click(point, gpui::Modifiers::none());
    cx.simulate_input("Plain");
    cx.simulate_keystrokes("enter");
    file_report(cx, &shell, "Body");
    assert_eq!(filed(&running.store, 2), "Body");
}
