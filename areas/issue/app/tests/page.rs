//! The Issue page's list, card and handoff against a node with a fixture store.
#![cfg(unix)]

use ahjo_app_kit::ask::Ask;
use ahjo_app_kit::link::client::LinkState;
use ahjo_app_kit::{Host, Mounted, Page, mount};
use ahjo_node_kit::{Built, Listing, Setup};
use ahjo_terminals_app::Terminals;
use ahjo_testkit::app::{Served, Started, click, serve, until};
use ahjo_testkit::node::Node;
use ahjo_testkit::tmux::Server;
use gpui::{App, Context, IntoElement, Render, div, prelude::*};
use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, Window};
use std::cell::RefCell;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

static BD: OnceLock<String> = OnceLock::new();
static GH: OnceLock<String> = OnceLock::new();
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
const AREAS: &[Listing] = &[ahjo_terminals::LISTING, WORK];
thread_local! { static TERMINALS: RefCell<Option<Entity<Terminals>>> = const { RefCell::new(None) }; }
fn terminals(host: Host, window: &mut Window, cx: &mut gpui::App) -> Mounted {
    mount(host, window, cx, |host, window, cx| {
        TERMINALS.with(|p| *p.borrow_mut() = Some(cx.entity()));
        Terminals::new(host, window, cx)
    })
}
/// A page beside which the item opens in the panel.
struct Toy;
impl Page for Toy {
    const ID: &'static str = "toy";
    const TITLE: &'static str = "Toy";
}
impl Render for Toy {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("toy-page")
            .debug_selector(|| "toy-page".into())
            .size_full()
            .flex()
            .child("Toy page")
    }
}
fn toy(host: Host, window: &mut Window, cx: &mut App) -> Mounted {
    mount(host, window, cx, |_, _, _| Toy)
}
struct Fixture {
    store: String,
    socket: std::path::PathBuf,
}
fn start() -> Option<Served<Fixture>> {
    let tmux = Server::start_in("issue-page-journey")?;
    Some(serve(move || async move {
        tmux.new_session("alpha", 100, 30);
        let dir = ahjo_testkit::fresh_dir("issue-page-store");
        std::fs::create_dir_all(dir.join(".beads")).unwrap();
        let script = dir.join("fixture-bd");
        ahjo_testkit::tmux::write_script(
            &script,
            "#!/bin/sh\nif [ \"$2\" = config ]; then echo '{\"value\":\"fixture\"}'; exit 0; fi\nif [ -f slow ]; then sleep 6; fi\nif [ -f fail ]; then echo 'tracker unavailable' >&2; exit 1; fi\ncat fixture.json\n",
        );
        std::fs::write(dir.join("fixture.json"), r#"[
          {"id":"fixture-1","title":"Read the fixture","description":"Its words travel apart from the snapshot.","status":"open","created_at":"2026-11-01T00:00:00Z","updated_at":"2026-11-01T00:00:00Z","parent":"fixture-0","dependencies":[{"type":"blocks","depends_on_id":"fixture-3"}]},
          {"id":"fixture-0","title":"Parent title","status":"open","created_at":"2026-11-01T00:00:00Z","updated_at":"2026-11-01T00:00:00Z"},
          {"id":"fixture-2","title":"Child title","status":"open","created_at":"2026-11-01T00:00:00Z","updated_at":"2026-11-01T00:00:00Z","parent":"fixture-1"},
          {"id":"fixture-3","title":"Blocker title","status":"closed","created_at":"2026-11-01T00:00:00Z","updated_at":"2026-11-01T00:00:00Z"}
        ]"#).unwrap();
        BD.set(script.to_string_lossy().into_owned()).unwrap();
        let gh = dir.join("fixture-gh");
        let base =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../node/tests/fixtures/work");
        ahjo_testkit::tmux::write_script(
            &gh,
            format!(
                "#!/bin/sh\ncase \"$*\" in *number=51*) cat '{}';; *) cat '{}';; esac\n",
                base.join("item.json").display(),
                base.join("ahjo.json").display()
            ),
        );
        GH.set(gh.to_string_lossy().into_owned()).unwrap();
        let launch = dir.join("launch.conf");
        std::fs::write(&launch, format!("folder {}\n", dir.display())).unwrap();
        let config = format!(
            "[terminals]\nsocket = {:?}\nlaunch = {:?}\n[issue]\nprojects = [{:?}, {:?}]\n",
            tmux.socket_path(),
            launch,
            format!("beads {}", dir.display()),
            "github example-owner/ahjo"
        );
        let node = Node::start(AREAS, &config).await.unwrap();
        let fixture = Fixture {
            store: dir.display().to_string(),
            socket: tmux.socket_path(),
        };
        Started::new(node, fixture).then(move || {
            std::fs::remove_dir_all(dir).unwrap();
            drop(tmux);
        })
    }))
}
fn shown(cx: &mut VisualTestContext) -> String {
    TERMINALS
        .with(|p| p.borrow().clone())
        .unwrap()
        .read_with(cx, |p, _| p.window_text(0))
        .unwrap_or_default()
}
#[gpui::test]
fn work_column_and_terminal_links_open_cards_and_hand_off(cx: &mut TestAppContext) {
    let Some(running) = start() else {
        return;
    };
    let settings = ahjo_testkit::fresh_dir("issue-page-settings");
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let url = running.url.clone();
    let path = settings.join("app.json");
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(
            &[toy, terminals, ahjo_issue_app::build],
            Some(url),
            Some(path),
            window,
            cx,
        )
    });
    until(cx, "link", |cx| {
        shell.read_with(cx, |s, cx| {
            s.hub().read(cx).fleet.here().state == LinkState::Open
        })
    });
    let toy_host = shell.read_with(cx, |s, _| Host::new(s.hub()).for_page("toy", &[]));
    let reveal = |id: &str, beside| Ask::Reveal {
        what: "item".into(),
        id: id.into(),
        node: Some(ahjo_core::NodeId(running.id.clone())),
        part: None,
        beside,
    };
    std::fs::write(std::path::Path::new(&running.store).join("slow"), "").unwrap();
    cx.update(|_, cx| toy_host.ask(reveal("fixture-1", true), cx))
        .unwrap();
    until(cx, "loading card", |cx| {
        cx.debug_bounds("issue-beside-loading").is_some()
    });
    std::thread::sleep(Duration::from_millis(5300));
    cx.run_until_parked();
    assert!(cx.debug_bounds("issue-beside-loading").is_some());
    assert!(cx.debug_bounds("issue-beside-failed").is_none());
    std::fs::remove_file(std::path::Path::new(&running.store).join("slow")).unwrap();
    until(cx, "beside card title", |cx| {
        cx.debug_bounds("issue-card-title-Read the fixture")
            .is_some()
    });
    // The family has its titles though the Issue page never read the
    // store: the node answers them with the item.
    // A finished blocker folds into a line that unfolds it.
    for text in [
        "Part of-fixture-0",
        "Children-fixture-2",
        "Blocked by-finished",
    ] {
        assert!(
            cx.debug_bounds(Box::leak(format!("issue-family-{text}").into_boxed_str()))
                .is_some(),
            "missing {text}"
        );
    }
    click(cx, "issue-family-Blocked by-finished");
    until(cx, "the finished blocker", |cx| {
        cx.debug_bounds("issue-family-Blocked by-fixture-3")
            .is_some()
    });
    click(cx, "issue-family-Part of-fixture-0");
    until(cx, "parent card", |cx| {
        cx.debug_bounds("issue-card-title-Parent title").is_some()
    });
    click(cx, "panel-back");
    until(cx, "back to selected item", |cx| {
        cx.debug_bounds("issue-card-title-Read the fixture")
            .is_some()
    });
    assert_eq!(shell.read_with(cx, |s, _| s.shown_page()), Some("toy"));
    assert!(cx.debug_bounds("issue-send").is_some());
    assert!(cx.debug_bounds("issue-more").is_some());
    cx.simulate_keystrokes("alt-i");
    until(cx, "beside closed", |cx| {
        cx.debug_bounds("issue-beside").is_none()
    });
    cx.update(|_, cx| toy_host.ask(reveal("fixture-404", true), cx))
        .unwrap();
    until(cx, "not found", |cx| {
        cx.debug_bounds(Box::leak(
            format!(
                "issue-beside-error-no Bead fixture-404 in {}",
                running.store
            )
            .into_boxed_str(),
        ))
        .is_some()
    });
    assert!(cx.debug_bounds("issue-beside-not-found").is_some());
    cx.simulate_keystrokes("alt-i");
    let failure = std::path::Path::new(&running.store).join("fail");
    std::fs::write(&failure, "").unwrap();
    // An item the page's board does not hold is asked of the node, whose
    // tracker fails now.
    cx.update(|_, cx| toy_host.ask(reveal("fixture-405", true), cx))
        .unwrap();
    until(cx, "tracker failure", |cx| {
        cx.debug_bounds("issue-beside-error-tracker unavailable")
            .is_some()
    });
    assert!(cx.debug_bounds("issue-beside-failed").is_some());
    assert!(cx.debug_bounds("issue-beside-not-found").is_none());
    std::fs::remove_file(failure).unwrap();
    cx.simulate_keystrokes("alt-i");
    // The project is chosen by name on the page, and its tree lists.
    click(cx, "tab-issue");
    until(cx, "Issue page", |cx| {
        cx.debug_bounds("issue-layout").is_some()
    });
    cx.simulate_keystrokes("p");
    cx.simulate_input("fixture");
    until(cx, "the store to choose", |cx| {
        cx.debug_bounds("issue-pick-fixture").is_some()
    });
    cx.simulate_keystrokes("enter");
    until(cx, "the store's tree", |cx| {
        cx.debug_bounds("issue-row-fixture-2").is_some()
    });
    cx.update(|_, cx| toy_host.ask(reveal("fixture-1", false), cx))
        .unwrap();
    until(cx, "Issue page card", |cx| {
        cx.debug_bounds("issue-card-title-Read the fixture")
            .is_some()
    });
    assert_eq!(shell.read_with(cx, |s, _| s.shown_page()), Some("issue"));
    // The snapshot leaves the text out: the card reads it from the node,
    // and a search finds the item by words only its text holds.
    until(cx, "the item's text", |cx| {
        cx.debug_bounds("issue-text").is_some()
    });
    cx.simulate_keystrokes("/");
    cx.simulate_input("travel apart");
    until(cx, "found by its text", |cx| {
        cx.debug_bounds("issue-row-fixture-1").is_some()
            && cx.debug_bounds("issue-row-fixture-2").is_none()
    });
    cx.simulate_keystrokes("escape");
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.show_id("sessions", window, cx)));
    let row = format!("row-{}::alpha", running.id);
    until(cx, "session", |cx| {
        cx.debug_bounds(Box::leak(row.clone().into_boxed_str()))
            .is_some()
    });
    click(cx, &row);
    until(cx, "peek", |cx| cx.debug_bounds("peek-attach").is_some());
    click(cx, "peek-attach");
    until(cx, "terminal", |cx| !shown(cx).trim().is_empty());
    // Both forms printed in the terminal resolve through the real Issue page.
    for (written, title) in [
        ("fixture-1", "Read the fixture"),
        ("example-owner/ahjo#51", "Example work item"),
    ] {
        let command = format!("printf '\\033[2J\\033[H{written}\\n'");
        let output = std::process::Command::new(
            std::env::var("AHJO_TMUX").unwrap_or_else(|_| "tmux".into()),
        )
        .args(["-f", "/dev/null", "-S"])
        .arg(&running.socket)
        .env_remove("TMUX")
        .args(["send-keys", "-t", "alpha", &command, "Enter"])
        .output()
        .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        until(cx, "reference printed in terminal", |cx| {
            shown(cx).lines().any(|line| line.trim() == written)
        });
        let point = TERMINALS
            .with(|p| p.borrow().clone())
            .unwrap()
            .read_with(cx, |p, _| {
                let text = p.window_text(0).unwrap();
                let row = text
                    .lines()
                    .position(|line| line.trim() == written)
                    .unwrap();
                let geo = p.geo.borrow()[0];
                let origin = geo.origin.unwrap();
                let cell = geo.cell.unwrap();
                gpui::point(
                    origin.x + cell.width * 3. + gpui::px(2.),
                    origin.y + cell.height * row as f32 + gpui::px(2.),
                )
            });
        cx.simulate_click(point, Modifiers::none());
        let selector = format!("issue-card-title-{title}");
        until(cx, "linked item card", |cx| {
            cx.debug_bounds(Box::leak(selector.clone().into_boxed_str()))
                .is_some()
        });
        assert_eq!(shell.read_with(cx, |s, _| s.shown_page()), Some("sessions"));
        cx.simulate_keystrokes("alt-i");
        until(cx, "linked card closed", |cx| {
            cx.debug_bounds("issue-beside").is_none()
        });
    }
    // Alt+B opens the Issue panel over the terminals; it alone keeps the
    // Issue channel on, and a row opens its item in the panel.
    cx.simulate_keystrokes("alt-b");
    until(cx, "fixture in the panel", |cx| {
        cx.debug_bounds("issue-panel-row-fixture-1").is_some()
    });
    assert!(cx.debug_bounds("panel-right").is_some());
    click(cx, "issue-panel-row-fixture-1");
    until(cx, "item in the panel", |cx| {
        cx.debug_bounds("issue-card-title-Read the fixture")
            .is_some()
    });
    assert_eq!(shell.read_with(cx, |s, _| s.shown_page()), Some("sessions"));
    // Send in the panel hands the item to a session.
    click(cx, "issue-send");
    until(cx, "send drawer", |cx| cx.debug_bounds("send-go").is_some());
    click(cx, "send-go");
    until(cx, "reference in session", |cx| {
        shown(cx).contains("bead fixture-1: Read the fixture")
    });
    until(cx, "send drawer closed", |cx| {
        cx.debug_bounds("send-go").is_none()
    });
    cx.simulate_keystrokes("alt-b");
    until(cx, "panel closed", |cx| {
        cx.debug_bounds("issue-panel").is_none()
    });
    click(cx, "tab-issue");
    until(cx, "item menu", |cx| {
        cx.debug_bounds("issue-more").is_some()
    });
    let host = shell.read_with(cx, |shell, _| Host::new(shell.hub()));
    let sessions = cx
        .update(|_, cx| host.model::<ahjo_terminals_contract::SessionList>(cx))
        .unwrap();
    // "…" offers to start a session in the item's checkout, first.
    click(cx, "issue-more");
    until(cx, "menu", |cx| cx.debug_bounds("menu-row-0").is_some());
    click(cx, "menu-row-0");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        let found = sessions.read_with(cx, |list, _| {
            list.sessions
                .iter()
                .any(|s| s.node == running.id && s.name == "fixture-1")
        });
        if found {
            break;
        }
        if Instant::now() >= deadline {
            let names = sessions.read_with(cx, |list, _| {
                list.sessions
                    .iter()
                    .map(|s| s.name.clone())
                    .collect::<Vec<_>>()
            });
            let status = shell.read_with(cx, |shell, cx| {
                shell
                    .hub()
                    .read(cx)
                    .frame
                    .read(cx)
                    .status
                    .as_ref()
                    .map(|s| (s.text.clone(), s.failure))
            });
            panic!("item session did not start: sessions {names:?}, status {status:?}");
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    std::fs::remove_dir_all(settings).unwrap();
}
