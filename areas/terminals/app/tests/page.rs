//! The Sessions page in the app against a real node (terminals over a
//! scratch tmux, started through the testkit): a session attached to a
//! window takes what is typed and shows its output, and a toy page hands a
//! reference into that session through the Send ask and starts sessions
//! through the Launch ask; other pages read the sessions the page publishes
//! and open views beside it. Linux and macOS only;
//! skips without tmux unless `AHJO_REQUIRE_TMUX=1`.
#![cfg(unix)]

use std::cell::RefCell;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use ahjo_app_kit::ask::SessionTarget;
use ahjo_app_kit::link::client::LinkState;
use ahjo_app_kit::panel::{Panel, PanelView};
use ahjo_app_kit::{Answers, Ask, Host, Mounted, Page, mount};
use ahjo_app_shell::Shell;
use ahjo_core::NodeId;
use ahjo_node_kit::Listing;
use ahjo_terminals_app::Terminals;
use ahjo_terminals_app::store::sessions::Key;
use ahjo_terminals_app::store::workspace::MAX_WINDOWS;
use ahjo_terminals_contract::SessionList;
use ahjo_testkit::node::Node;
use ahjo_testkit::tmux::Server;
use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, Window, div, prelude::*, px};

const AREAS: &[Listing] = &[ahjo_terminals::LISTING];
const AREAS_WITH_FILES: &[Listing] = &[ahjo_terminals::LISTING, ahjo_files::LISTING];

thread_local! {
    static PAGE: RefCell<Option<Entity<Terminals>>> = const { RefCell::new(None) };
    static ITEM_ASK: RefCell<Option<Ask>> = const { RefCell::new(None) };
}

/// The Sessions page, kept where the test can read what it shows.
fn terminals(host: Host, window: &mut Window, cx: &mut gpui::App) -> Mounted {
    mount(host, window, cx, |host, window, cx| {
        PAGE.with(|p| *p.borrow_mut() = Some(cx.entity()));
        Terminals::new(host, window, cx)
    })
}

/// A page that hands a reference to a session, as any page may.
struct Toy {
    host: Host,
}

/// What the toy hands over: a command, so the session's output shows it
/// arrived and ran.
const REFERENCE: &str = "echo sent-$((40+2))";

impl Page for Toy {
    const ID: &'static str = "toy";
    const TITLE: &'static str = "Toy";
    const ANSWERS: &'static [Answers] = &[
        Answers::Reveal("item"),
        Answers::Reveal("agent"),
        Answers::OpenPath,
    ];

    fn answer(&mut self, ask: Ask, _: &'static str, _: &mut Window, cx: &mut Context<Self>) {
        if ask.beside() {
            let note = cx.new(|cx| Note(cx.focus_handle()));
            self.host.show_panel(Panel::new("note", note), cx);
        }
        ITEM_ASK.with(|slot| *slot.borrow_mut() = Some(ask));
    }
}

impl Render for Toy {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let host = self.host.clone();
        div()
            .size_full()
            .child(
                div()
                    .id("toy-send")
                    .debug_selector(|| "toy-send".into())
                    .w(px(80.))
                    .h(px(30.))
                    .child("Send")
                    .on_click({
                        let host = host.clone();
                        move |_, _, cx| {
                            let ask = Ask::Send {
                                reference: REFERENCE.into(),
                                target: None,
                            };
                            host.ask(ask, cx).expect("the Sessions page answers it");
                        }
                    }),
            )
            .child(
                div()
                    .id("toy-send-beta")
                    .debug_selector(|| "toy-send-beta".into())
                    .w(px(80.))
                    .h(px(30.))
                    .child("Send to beta")
                    .on_click(move |_, _, cx| {
                        let node = host.nodes(cx).remove(0).id;
                        let target = SessionTarget {
                            node,
                            user: String::new(),
                            name: "beta".into(),
                        };
                        let ask = Ask::Send {
                            reference: REFERENCE.into(),
                            target: Some(target),
                        };
                        host.ask(ask, cx).expect("the Sessions page answers it");
                    }),
            )
    }
}

fn toy(host: Host, window: &mut Window, cx: &mut gpui::App) -> Mounted {
    mount(host, window, cx, |host, _, _| Toy { host })
}

/// A running node, on its own thread and runtime.
struct Running {
    url: String,
    node: String,
    socket: std::path::PathBuf,
    fixture: std::path::PathBuf,
    stop: mpsc::Sender<()>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            let _waiting = waiting("the node and its tmux to stop");
            let _ = thread.join();
        }
        SERVERS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|s| *s != self.socket);
    }
}

/// The sockets of the tmux servers the running tests started, which the
/// watchdog stops before it ends a stuck run: nothing would stop them then.
static SERVERS: std::sync::Mutex<Vec<std::path::PathBuf>> = std::sync::Mutex::new(Vec::new());

/// A node following a scratch tmux server with one session, `alpha`.
fn start(label: &str) -> Option<Running> {
    start_with_harness(label, false)
}

fn start_with_harness(label: &str, harness: bool) -> Option<Running> {
    start_with_restart_harness(label, harness, false)
}

fn start_with_restart_harness(label: &str, harness: bool, restart: bool) -> Option<Running> {
    start_config(label, harness, restart, false, false)
}

fn start_config(
    label: &str,
    harness: bool,
    restart: bool,
    files: bool,
    bash: bool,
) -> Option<Running> {
    let label = label.to_string();
    let (ready, wait) = mpsc::channel();
    let (stop, stopped) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("a runtime");
        rt.block_on(async move {
            let Some(tmux) = Server::start_in(&label) else {
                let _ = ready.send(None);
                return;
            };
            tmux.new_session("alpha", 100, 30);
            tmux.new_session("beta", 100, 30);
            if bash {
                for session in ["alpha", "beta"] {
                    tmux.run(&["respawn-pane", "-k", "-t", session, "env PS1='$ ' /bin/bash --noprofile --norc -i"]);
                }
            }
            let dir = ahjo_testkit::fresh_dir("flags-ui");
            let binary = dir.join("fake");
            let launch = dir.join("launch.conf");
            if harness {
                ahjo_testkit::tmux::write_script(
                    &binary,
                    format!("#!/bin/sh\nif [ \"$1\" = --help ]; then printf 'Options:\\n  -v, --verbose  Verbose output\\n'; exit; fi\nprintf '%s|%s\\n' \"$PWD\" \"$*\" >> '{}'\nwhile :; do sleep 1; done\n", dir.join("runs").display()),
                );
                let config = if restart {
                    format!("harness fake {} --marker\nfolder {}\n", binary.display(), dir.display())
                } else {
                    format!("harness fake {}\n", binary.display())
                };
                std::fs::write(&launch, config).unwrap();
            }
            let config = format!(
                "[terminals]\nsocket = {:?}\nlaunch = {:?}\n",
                tmux.socket_path(),
                launch
            );
            let areas = if files { AREAS_WITH_FILES } else { AREAS };
            let config = if files {
                format!("{config}\n[files]\nroots = [{:?}]\n", dir.display().to_string())
            } else {
                config
            };
            let node = Node::start(areas, &config).await.expect("a node");
            let _ = ready.send(Some((node.url(), node.id(), tmux.socket_path(), dir.clone())));
            while stopped.try_recv().is_err() {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            node.stop().await;
            let _ = std::fs::remove_dir_all(dir);
        });
    });
    let started = {
        let _waiting = waiting("the node to start");
        wait.recv().expect("the node thread")
    };
    match started {
        Some((url, node, socket, fixture)) => Some(Running {
            socket: {
                SERVERS
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(socket.clone());
                socket
            },
            url,
            node,
            fixture,
            stop,
            thread: Some(thread),
        }),
        None => {
            let _ = thread.join();
            assert!(
                std::env::var_os("AHJO_REQUIRE_TMUX").is_none(),
                "AHJO_REQUIRE_TMUX is set but tmux did not start"
            );
            None
        }
    }
}

/// A test still in one wait this long is stuck where its own deadline cannot
/// fire: in a call that blocks, a scheduler that never parks, a node that
/// never stops.
const STUCK: Duration = Duration::from_secs(60);

/// What each test is waiting for, and since when, by test name.
static WAITS: std::sync::Mutex<std::collections::BTreeMap<String, (String, Instant)>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

/// The calling test waits for `what` until this is dropped. A watchdog ends
/// the whole run, naming the test and the wait, once any wait is [`STUCK`]:
/// a hang becomes a failure that says where it was.
struct Waiting(String);

fn waiting(what: &str) -> Waiting {
    static WATCHDOG: std::sync::Once = std::sync::Once::new();
    WATCHDOG.call_once(|| {
        std::thread::spawn(|| {
            loop {
                std::thread::sleep(Duration::from_secs(1));
                let waits = WAITS.lock().unwrap_or_else(|e| e.into_inner());
                if let Some((test, (what, since))) =
                    waits.iter().find(|(_, (_, since))| since.elapsed() > STUCK)
                {
                    // Past the test harness's capture, which shows output
                    // only once a test ends.
                    use std::io::Write;
                    let _ = writeln!(
                        std::io::stderr(),
                        "{test} is stuck: waiting for {what} for {:?}; ending the run",
                        since.elapsed()
                    );
                    let servers = SERVERS.lock().unwrap_or_else(|e| e.into_inner());
                    for socket in servers.iter() {
                        let _ = std::process::Command::new(
                            std::env::var_os("AHJO_TMUX").unwrap_or_else(|| "tmux".into()),
                        )
                        .arg("-S")
                        .arg(socket)
                        .arg("kill-server")
                        .output();
                    }
                    std::process::abort();
                }
            }
        });
    });
    let test = std::thread::current()
        .name()
        .unwrap_or("a test")
        .to_string();
    WAITS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(test.clone(), (what.to_string(), Instant::now()));
    Waiting(test)
}

impl Drop for Waiting {
    fn drop(&mut self) {
        WAITS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.0);
    }
}

fn until(
    cx: &mut VisualTestContext,
    what: &str,
    mut ok: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let _waiting = waiting(what);
    let deadline = Instant::now() + ahjo_testkit::timeout(Duration::from_secs(15));
    loop {
        // A frame, as a platform delivers one: what slides moves on.
        cx.update(|window, cx| {
            if window.simulate_next_frame(cx) > 0 {
                window.refresh();
            }
        });
        cx.run_until_parked();
        if ok(cx) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn click(cx: &mut VisualTestContext, selector: String) {
    ahjo_testkit::app::click(cx, &selector);
}

/// What the page's first window shows.
fn shown(cx: &mut VisualTestContext) -> String {
    let page = PAGE.with(|p| p.borrow().clone()).expect("the page");
    page.read_with(cx, |p, _| p.window_text(0))
        .unwrap_or_default()
}

/// The app with the Sessions page and the toy against `running`, the
/// page shown and `alpha` attached to its first window.
fn open_with_alpha<'a>(
    running: &Running,
    name: &str,
    cx: &'a mut TestAppContext,
) -> (Entity<Shell>, &'a mut VisualTestContext, std::path::PathBuf) {
    open_pages_with_alpha(running, name, &[terminals, toy], cx)
}

/// [`open_with_alpha`] with `pages` mounted instead of the page and the toy.
fn open_pages_with_alpha<'a>(
    running: &Running,
    name: &str,
    pages: &'static [ahjo_app_kit::Build],
    cx: &'a mut TestAppContext,
) -> (Entity<Shell>, &'a mut VisualTestContext, std::path::PathBuf) {
    let settings = std::env::temp_dir()
        .join(format!("ahjo-terminals-{name}-{}", std::process::id()))
        .join("app.json");
    std::fs::create_dir_all(settings.parent().expect("its folder")).expect("a folder");
    // The node and the link run on their own threads.
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let (url, path) = (running.url.clone(), settings.clone());
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(pages, Some(url), Some(path), window, cx)
    });
    until(cx, "the link to open", |cx| {
        shell.read_with(cx, |s, cx| {
            s.hub().read(cx).fleet.here().state == LinkState::Open
        })
    });
    until(cx, "the Sessions page to show", |cx| {
        shell.read_with(cx, |s, _| s.shown_page()) == Some("sessions")
    });
    // The rail lists alpha; a click peeks, and Attach puts it in a window.
    let row = format!("row-{}::alpha", running.node);
    let leaked: &'static str = Box::leak(row.clone().into_boxed_str());
    until(cx, "alpha in the rail", |cx| {
        cx.debug_bounds(leaked).is_some()
    });
    click(cx, row);
    until(cx, "the peek", |cx| {
        cx.debug_bounds("peek-attach").is_some()
    });
    click(cx, "peek-attach".into());
    until(cx, "alpha's screen in the window", |cx| {
        !shown(cx).trim().is_empty()
    });
    // The window then sizes alpha to its grid, and the screen tmux sends
    // for the new size starts the window's terminal over: a selection made
    // before it arrives is gone, and rows can move.
    let page = PAGE.with(|p| p.borrow().clone()).expect("the page");
    until(cx, "alpha's screen at the grid its window claimed", |cx| {
        page.read_with(cx, |p, _| {
            let Some(at) = p.slot_pane(0) else {
                return false;
            };
            let (Some(claimed), Some(screen)) = (
                p.fleet.claimed(&at.node, at.pane),
                p.fleet.screen(&at.node, at.pane),
            ) else {
                return false;
            };
            let s = screen.lock();
            (s.cols, s.rows) == (claimed.cols as usize, claimed.rows as usize)
        })
    });
    (shell, cx, settings)
}

#[gpui::test]
fn a_session_in_a_window_takes_typing_and_shows_its_output(cx: &mut TestAppContext) {
    let Some(running) = start("terminals-page-typing") else {
        return;
    };
    let (_shell, cx, settings) = open_with_alpha(&running, "typing", cx);
    cx.simulate_input("echo typed-$((6*7))");
    cx.simulate_keystrokes("enter");
    until(cx, "the output in the window", |cx| {
        shown(cx).contains("typed-42")
    });
    let _ = std::fs::remove_dir_all(settings.parent().expect("its folder"));
}

#[gpui::test]
fn a_page_hands_a_reference_into_a_session_through_send(cx: &mut TestAppContext) {
    let Some(running) = start("terminals-page-send") else {
        return;
    };
    let (shell, cx, settings) = open_with_alpha(&running, "send", cx);
    click(cx, "tab-toy".into());
    until(cx, "the toy page to show", |cx| {
        shell.read_with(cx, |s, _| s.shown_page()) == Some("toy")
    });
    click(cx, "toy-send".into());
    // Send opens in the panel beside the page that asked, which stays
    // shown (sending an item keeps the operator on Work), aimed at the
    // session of the window that last had focus, named at its top.
    until(cx, "Send in the panel", |cx| {
        cx.debug_bounds("send-go").is_some() && cx.debug_bounds("panel-right").is_some()
    });
    assert_eq!(shell.read_with(cx, |s, _| s.shown_page()), Some("toy"));
    let page = PAGE.with(|p| p.borrow().clone()).expect("the page");
    let aimed = page.read_with(cx, |p, _| p.send_targets.first().cloned());
    assert_eq!(aimed.map(|k| k.name), Some("alpha".to_string()));
    assert!(
        cx.debug_bounds("send-aimed-alpha").is_some(),
        "named at the top"
    );
    click(cx, "send-go".into());
    until(cx, "the panel closed once sent", |cx| {
        cx.debug_bounds("send-go").is_none() && cx.debug_bounds("panel-right").is_none()
    });
    assert_eq!(shell.read_with(cx, |s, _| s.shown_page()), Some("toy"));
    click(cx, "tab-sessions-0".into());
    until(cx, "the reference's output in the session", |cx| {
        shown(cx).contains("sent-42")
    });
    let _ = std::fs::remove_dir_all(settings.parent().expect("its folder"));
}

/// Send left open in the panel stays there as the pages change: it is the
/// window's, not the page's that asked.
#[gpui::test]
fn send_left_open_stays_beside_every_page(cx: &mut TestAppContext) {
    let Some(running) = start("terminals-page-send-stays") else {
        return;
    };
    let (shell, cx, settings) = open_with_alpha(&running, "stays", cx);
    click(cx, "tab-toy".into());
    until(cx, "the toy page to show", |cx| {
        shell.read_with(cx, |s, _| s.shown_page()) == Some("toy")
    });
    click(cx, "toy-send".into());
    until(cx, "Send in the panel", |cx| {
        cx.debug_bounds("send-go").is_some()
    });
    click(cx, "tab-sessions-0".into());
    until(cx, "the Sessions page", |cx| {
        shell.read_with(cx, |s, _| s.shown_page()) == Some("sessions")
    });
    assert!(cx.debug_bounds("send-go").is_some());
    let _ = std::fs::remove_dir_all(settings.parent().expect("its folder"));
}

#[gpui::test]
fn an_ask_that_names_a_session_aims_send_at_it_not_at_the_focused_one(cx: &mut TestAppContext) {
    let Some(running) = start("terminals-page-target") else {
        return;
    };
    let (shell, cx, settings) = open_with_alpha(&running, "target", cx);
    click(cx, "tab-toy".into());
    until(cx, "the toy page to show", |cx| {
        shell.read_with(cx, |s, _| s.shown_page()) == Some("toy")
    });
    click(cx, "toy-send-beta".into());
    until(cx, "the Send drawer", |cx| {
        cx.debug_bounds("send-go").is_some()
    });
    let page = PAGE.with(|p| p.borrow().clone()).expect("the page");
    let aimed = page.read_with(cx, |p, _| p.send_targets.first().cloned());
    assert_eq!(
        aimed,
        Some(Key {
            node: running.node.clone(),
            user: String::new(),
            name: "beta".into()
        }),
        "alpha has the focus; the ask names beta"
    );
    let _ = std::fs::remove_dir_all(settings.parent().expect("its folder"));
}

/// What another page shows in the window's panel.
struct Note(gpui::FocusHandle);

impl Render for Note {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .debug_selector(|| "toy-note".into())
            .track_focus(&self.0)
            .size_full()
            .child("beside")
    }
}

impl PanelView for Note {
    fn head(&mut self, _: &mut Window, _: &mut Context<Self>) -> gpui::AnyElement {
        div().child("Note").into_any_element()
    }
    fn focus(&self, _: &gpui::App) -> Option<gpui::FocusHandle> {
        Some(self.0.clone())
    }
}

#[gpui::test]
fn other_pages_read_the_sessions_and_open_views_beside_them(cx: &mut TestAppContext) {
    let Some(running) = start("terminals-page-shared") else {
        return;
    };
    let (shell, cx, settings) = open_with_alpha(&running, "shared", cx);
    let toy = shell.read_with(cx, |s, _| Host::new(s.hub()).for_page("toy", &[]));
    let sessions = cx
        .update(|_, cx| toy.model::<SessionList>(cx))
        .expect("the hub");
    let listed = sessions.read_with(cx, |l, _| {
        l.sessions
            .iter()
            .map(|s| format!("{}/{}", s.node, s.name))
            .collect::<Vec<_>>()
    });
    assert!(
        listed.contains(&format!("{}/alpha", running.node)),
        "{listed:?}"
    );
    assert!(cx.debug_bounds("panel-right").is_none());
    let note = cx.update(|_, cx| cx.new(|cx| Note(cx.focus_handle())));
    cx.update(|_, cx| toy.show_panel(Panel::new("note", note.clone()), cx));
    cx.run_until_parked();
    assert!(cx.debug_bounds("toy-note").is_some(), "drawn in the panel");
    cx.update(|window, cx| window.focus(&note.read(cx).0.clone(), cx));
    cx.update(|_, cx| toy.close_panel("note", cx));
    cx.run_until_parked();
    assert!(cx.debug_bounds("panel-right").is_none(), "closed");
    // The view lives on, holding the focus; the terminal takes it back.
    let page = PAGE.with(|p| p.borrow().clone()).expect("the page");
    assert!(cx.update(|window, cx| page.read(cx).term_focus.is_focused(window)));
    let _ = std::fs::remove_dir_all(settings.parent().expect("its folder"));
}

/// Every way into the window's one panel makes the windows squeeze, tmux
/// windows included, and gives them their width back as it closes by its
/// Close, Escape or its key; the terminal has the keyboard back. Only one
/// content shows at a time, and a picture shows in it. What lies over the
/// windows (Overview) and what an agent's `ahjo view open` brings forward
/// leave every session at the size it had.
#[gpui::test]
fn every_way_into_the_panel_squeezes_the_windows_and_gives_them_back(cx: &mut TestAppContext) {
    let Some(running) = start_config("terminals-opened-size", false, false, true, false) else {
        return;
    };
    let picture = running.fixture.join("plan.svg");
    std::fs::write(
        &picture,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="#c33"/></svg>"##,
    )
    .unwrap();
    let (shell, cx, settings) = open_pages_with_alpha(
        &running,
        "opened-size",
        // Files and Work answer what a session prints, before the toy.
        &[terminals, ahjo_files_app::build, ahjo_issue_app::build, toy],
        cx,
    );
    cx.simulate_resize(gpui::size(px(1440.), px(900.)));
    let toy = shell.read_with(cx, |s, _| Host::new(s.hub()).for_page("toy", &[]));
    let page = PAGE.with(|p| p.borrow().clone()).expect("the page");
    let grid = |cx: &mut VisualTestContext| {
        page.read_with(cx, |p, _| {
            let g = p.geo.borrow()[0];
            (g.cols, g.rows)
        })
    };
    let width = || -> usize {
        tmux(
            &running,
            &["display-message", "-p", "-t", "alpha", "#{window_width}"],
        )
        .trim()
        .parse()
        .expect("a width")
    };
    // A new grid is sent once it has held a moment, on the test's clock.
    let sized = |cx: &mut VisualTestContext, what: &str| {
        until(cx, what, |cx| {
            cx.executor().advance_clock(Duration::from_millis(100));
            let (cols, _) = grid(cx);
            page.read_with(cx, |p, _| p.resizing.is_empty()) && width() == cols
        });
    };
    sized(cx, "alpha sized to its window");
    let whole = width();
    let term_focused = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| page.read(cx).term_focus.is_focused(window))
    };
    let focus_term = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            let focus = page.read(cx).term_focus.clone();
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
    };
    // `content` opens in the one panel, alone, and alpha squeezes.
    let opened = |cx: &mut VisualTestContext, content: &'static str, what: &str| {
        until(cx, what, |cx| {
            cx.debug_bounds(content).is_some() && cx.debug_bounds("panel-right").is_some()
        });
        assert!(cx.debug_bounds("panel-left").is_none(), "one panel: {what}");
        let panel = cx.debug_bounds("panel-right").expect("the panel");
        let window = cx.debug_bounds("window-0").expect("alpha's window");
        assert!(
            window.right() <= panel.left() + px(1.),
            "{what} covers no window: {window:?} {panel:?}"
        );
        until(cx, &format!("alpha squeezed beside {what}"), |cx| {
            cx.executor().advance_clock(Duration::from_millis(100));
            width() < whole
        });
        sized(cx, &format!("alpha sized beside {what}"));
    };
    let closed = |cx: &mut VisualTestContext, what: &str| {
        until(cx, &format!("the panel closed: {what}"), |cx| {
            cx.debug_bounds("panel-right").is_none()
        });
        until(cx, &format!("alpha's width back after {what}"), |cx| {
            cx.executor().advance_clock(Duration::from_millis(100));
            width() == whole
        });
        sized(cx, &format!("alpha sized after {what}"));
        assert!(
            term_focused(cx),
            "the terminal has the keyboard after {what}"
        );
    };

    // Overview lies over the windows and changes no size.
    cx.simulate_keystrokes("alt-v");
    cx.run_until_parked();
    assert!(cx.debug_bounds("overview-grid").is_some(), "Overview drawn");
    sized(cx, "with Overview open");
    assert_eq!(width(), whole, "Overview resized alpha");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("overview-grid").is_none(),
        "Esc closed Overview"
    );

    // Another page's view, shown from elsewhere: the keyboard stays in the
    // terminal; its Close takes it away.
    assert!(term_focused(cx));
    let note = cx.update(|_, cx| cx.new(|cx| Note(cx.focus_handle())));
    cx.update(|_, cx| toy.show_panel(Panel::new("note", note), cx));
    opened(cx, "toy-note", "a view from another page");
    assert!(
        term_focused(cx),
        "a view from elsewhere leaves the keyboard"
    );
    click(cx, "panel-close".into());
    closed(cx, "its Close");

    // Send (Alt+S): Escape closes it.
    cx.simulate_keystrokes("alt-s");
    opened(cx, "send-drawer", "Send");
    cx.simulate_keystrokes("escape");
    closed(cx, "Escape in Send");

    // The output search (Alt+H): its key closes it.
    cx.simulate_keystrokes("alt-h");
    opened(cx, "find-drawer", "the output search");
    cx.simulate_keystrokes("alt-h");
    closed(cx, "Alt+H again");

    // Files (Alt+E), and Send opened over it: one shows, Back goes back.
    cx.simulate_keystrokes("alt-e");
    opened(cx, "files-list", "Files");
    cx.simulate_keystrokes("alt-s");
    opened(cx, "send-drawer", "Send over Files");
    assert!(cx.debug_bounds("files-list").is_none(), "only Send shows");
    click(cx, "panel-back".into());
    opened(cx, "files-list", "Files back from Send");
    assert!(cx.debug_bounds("send-drawer").is_none());
    cx.simulate_keystrokes("ctrl-b");
    closed(cx, "Ctrl+B in Files");

    // A picture the session printed, clicked: it shows in Files.
    let command = format!("printf '\\033[2J\\033[Hmade {}\\n'", picture.display());
    tmux(&running, &["send-keys", "-t", "alpha", &command, "Enter"]);
    let mut was = None;
    until(cx, "the printed picture drawn small, holding still", |cx| {
        let now = page.read_with(cx, |p, _| p.chips.borrow()[0].first().map(|c| c.bounds));
        now.is_some() && std::mem::replace(&mut was, now) == now
    });
    let chip = was.expect("the picture drawn small");
    cx.simulate_click(chip.center(), Modifiers::none());
    until(cx, "the picture asked for", |cx| {
        cx.debug_bounds("file-view-plan.svg").is_some()
    });
    opened(cx, "file-view-plan.svg", "a printed picture");
    until(cx, "the picture drawn", |cx| {
        cx.debug_bounds("files-picture")
            .is_some_and(|b| b.size.width > px(10.) && b.size.height > px(5.))
    });
    // Back in the terminal, Esc is the terminal's (agents need it) and the
    // picture stays.
    let window = cx.debug_bounds("window-0").expect("the window");
    cx.simulate_click(
        gpui::point(window.left() + px(20.), window.bottom() - px(20.)),
        Modifiers::none(),
    );
    cx.run_until_parked();
    assert!(term_focused(cx));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("files-picture").is_some(),
        "Esc in the terminal leaves the picture"
    );
    // The shell took the Esc as a prefix; a fresh line for what follows.
    cx.simulate_keystrokes("ctrl-c");
    cx.simulate_keystrokes("alt-e");
    closed(cx, "Alt+E");

    // Work (Alt+B): its key closes it.
    focus_term(cx);
    cx.simulate_keystrokes("alt-b");
    opened(cx, "issue-panel", "the Work panel");
    cx.simulate_keystrokes("alt-b");
    closed(cx, "Alt+B again");

    // A work item the session printed, clicked: Alt+I closes it.
    tmux(
        &running,
        &[
            "send-keys",
            "-t",
            "alpha",
            "printf '\\033[2J\\033[Hparvi-pnu.7\\n'",
            "Enter",
        ],
    );
    let item = |cx: &mut VisualTestContext| {
        page.read_with(cx, |p, _| {
            let text = p.window_text(0)?;
            let row = text.lines().position(|l| l.trim() == "parvi-pnu.7")?;
            let geo = p.geo.borrow()[0];
            let (origin, cell) = (geo.origin?, geo.cell?);
            Some(gpui::point(
                origin.x + cell.width * 3. + px(2.),
                origin.y + cell.height * row as f32 + px(2.),
            ))
        })
    };
    until(cx, "the work item printed", |cx| item(cx).is_some());
    let at = item(cx).expect("printed");
    cx.simulate_click(at, Modifiers::none());
    opened(cx, "issue-beside", "a printed work item");
    cx.simulate_keystrokes("alt-i");
    closed(cx, "Alt+I");

    // A diagram the session printed, clicked: its chip takes it away.
    let command = "printf '\\033[2J\\033[Hflowchart LR\\n    A[Build] --> B[Test]\\n\\ndone\\n'";
    tmux(&running, &["send-keys", "-t", "alpha", command, "Enter"]);
    until(cx, "the diagram drawn small", |cx| {
        page.read_with(cx, |p, _| {
            p.chips.borrow()[0].len() == 1
                && p.window_text(0).is_some_and(|t| t.contains("flowchart"))
        })
    });
    let chip = |cx: &mut VisualTestContext| {
        let mut was = None;
        until(cx, "the diagram's chip holding still", |cx| {
            let now = page.read_with(cx, |p, _| p.chips.borrow()[0].first().map(|c| c.bounds));
            now.is_some() && std::mem::replace(&mut was, now) == now
        });
        was.expect("the chip").center()
    };
    let at = chip(cx);
    cx.simulate_click(at, Modifiers::none());
    opened(cx, "diagram-beside", "a printed diagram");
    // The window narrowed beside the diagram: its chip moved with it.
    let at = chip(cx);
    cx.simulate_click(at, Modifiers::none());
    closed(cx, "the diagram's chip");

    // An agent's `ahjo view open` brings the page forward; the windows
    // are as they were when the terminals come back.
    let rt = tokio::runtime::Runtime::new().expect("a runtime");
    let path = picture.display().to_string();
    for (page_id, thing) in [
        ("files", vec!["--path", path.as_str()]),
        ("issue", vec!["--item", "parvi-pnu.7"]),
    ] {
        let args: Vec<String> = ["open", "--page", page_id]
            .into_iter()
            .chain(thing)
            .chain(["--json", "--node", &running.url])
            .map(str::to_string)
            .collect();
        let opened = rt.spawn(async move {
            let mut out = Vec::new();
            ahjo_node_kit::cli::run("view", &args, &mut out)
                .await
                .expect("the command runs")
        });
        until(cx, "ahjo view open answered", |_| opened.is_finished());
        assert_eq!(rt.block_on(opened).expect("its task"), 0, "{page_id}");
        assert_eq!(shell.read_with(cx, |s, _| s.shown_page()), Some(page_id));
        assert_eq!(width(), whole, "with {page_id} shown by ahjo view open");
        let terminals = shell.read_with(cx, |s, _| Host::new(s.hub()).for_page("sessions", &[]));
        cx.update(|_, cx| terminals.show(cx));
        until(cx, "the terminals back", |cx| {
            shell.read_with(cx, |s, _| s.shown_page()) == Some("sessions")
        });
        sized(cx, &format!("the terminals back from {page_id}"));
        assert_eq!(width(), whole, "with the terminals back from {page_id}");
    }
    let _ = std::fs::remove_dir_all(settings.parent().expect("its folder"));
}

/// What lies over the windows takes every pointer event inside it: a
/// click, a middle click (a paste in a terminal), a wheel and the keys
/// typed after them reach no session under it, no window under it takes
/// the focus or starts reading back, and Esc after such a click closes
/// what was clicked. Held for the panel beside them (a view from another
/// page, Files, Work, Send, the output search), Overview, Peek and a
/// launcher too big for its window, each beside or over a live session. A
/// view opened from elsewhere leaves the keyboard in the terminal; what
/// opens by its key (Files, Work, Send) has it in its field, and closed
/// with Esc gives it back to the terminal.
#[gpui::test]
fn what_lies_over_the_windows_takes_every_click_inside_it(cx: &mut TestAppContext) {
    let Some(running) = start_config("terminals-over", false, false, true, false) else {
        return;
    };
    let (shell, cx, settings) = open_pages_with_alpha(
        &running,
        "over",
        &[terminals, toy, ahjo_files_app::build, ahjo_issue_app::build],
        cx,
    );
    cx.simulate_resize(gpui::size(px(1440.), px(900.)));
    let page = PAGE.with(|p| p.borrow().clone()).expect("the page");
    let beta = Key {
        node: running.node.clone(),
        user: String::new(),
        name: "beta".into(),
    };
    // Alpha focused on the left, beta on the right where things open.
    cx.update(|_, cx| {
        page.update(cx, |p, cx| {
            p.settings.workspace.set_windows(0, 2);
            p.settings.workspace.bind_in(0, 1, beta.clone(), None);
            p.settings.workspace.focus = 0;
            cx.notify();
        })
    });
    until(cx, "beta's screen in the right window", |cx| {
        page.read_with(cx, |p, _| {
            p.window_text(1).is_some_and(|t| !t.trim().is_empty())
        })
    });
    let toy = shell.read_with(cx, |s, _| Host::new(s.hub()).for_page("toy", &[]));
    const MARK: &str = "overmark";
    cx.write_to_clipboard(gpui::ClipboardItem::new_string(format!("{MARK}-paste")));
    let untouched = |cx: &mut VisualTestContext, what: &str, focus: usize| {
        cx.run_until_parked();
        for session in ["alpha", "beta"] {
            let screen = tmux(&running, &["capture-pane", "-p", "-t", session]);
            assert!(
                !screen.contains(MARK),
                "{session} took input through {what}:\n{screen}"
            );
        }
        page.read_with(cx, |p, _| {
            assert_eq!(
                p.settings.workspace.focus, focus,
                "focused window after {what}"
            );
            assert!(p.histories.is_empty(), "no window reads back after {what}");
        });
        assert!(
            !cx.update(|window, cx| page.read(cx).term_focus.is_focused(window)),
            "the terminals took the keyboard through {what}"
        );
    };
    // Every kind of pointer event at `at`, then typing.
    let poke = |cx: &mut VisualTestContext, at: gpui::Point<gpui::Pixels>, under: &'static str| {
        let over = cx.debug_bounds(under).expect(under);
        assert!(over.contains(&at), "{at:?} lies over {under} {over:?}");
        cx.simulate_click(at, Modifiers::none());
        cx.simulate_mouse_down(at, gpui::MouseButton::Middle, Modifiers::none());
        cx.simulate_mouse_up(at, gpui::MouseButton::Middle, Modifiers::none());
        cx.simulate_event(gpui::ScrollEvent {
            position: at,
            delta: gpui::ScrollDelta::Lines(gpui::point(0., 5.)),
            modifiers: Modifiers::none(),
            touch_phase: gpui::TouchPhase::Moved,
            source: gpui::ScrollSource::Pointer,
        });
        cx.simulate_input(MARK);
    };
    let inside =
        |b: gpui::Bounds<gpui::Pixels>| gpui::point(b.right() - px(24.), b.bottom() - px(24.));

    // The panel: a view opened from elsewhere leaves the keyboard in the
    // terminal; a click gives it the keyboard, and Esc closes it.
    let term_focused = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| page.read(cx).term_focus.is_focused(window))
    };
    assert!(term_focused(cx), "alpha's terminal has the keyboard");
    let note = cx.update(|_, cx| cx.new(|cx| Note(cx.focus_handle())));
    cx.update(|_, cx| toy.show_panel(Panel::new("note", note.clone()), cx));
    cx.run_until_parked();
    assert!(
        term_focused(cx),
        "a view opened from elsewhere leaves the keyboard in the terminal"
    );
    let beside = ahjo_testkit::app::held_still(cx, "toy-note");
    let window = cx.debug_bounds("window-1").expect("window 1");
    assert!(window.right() <= beside.left(), "the view covers no window");
    poke(cx, beside.center(), "toy-note");
    untouched(cx, "the panel", 0);
    assert!(
        cx.update(|window, cx| note.read(cx).0.is_focused(window)),
        "a click in the view gives it the keyboard"
    );
    cx.simulate_keystrokes("escape");
    until(
        cx,
        "Esc after a click in the view puts the panel away",
        |cx| cx.debug_bounds("panel-right").is_none(),
    );
    assert!(
        term_focused(cx),
        "closed, the terminal has the keyboard back"
    );

    // A panel opened by its key has the keyboard in its find. Files: Alt+O,
    // type, Enter opens the file found; Esc closes it.
    std::fs::write(running.fixture.join(format!("{MARK}-note.txt")), "found\n").unwrap();
    let cd = format!("cd {}", running.fixture.display());
    tmux(&running, &["send-keys", "-t", "alpha", &cd, "Enter"]);
    let sessions = cx
        .update(|_, cx| toy.model::<SessionList>(cx))
        .expect("the hub");
    until(cx, "alpha in the fixture folder", |cx| {
        sessions.read_with(cx, |l, _| {
            l.sessions.iter().any(|s| {
                s.name == "alpha" && s.cwd.as_deref() == Some(running.fixture.to_str().unwrap())
            })
        })
    });
    cx.simulate_keystrokes("alt-o");
    until(cx, "the Files panel", |cx| {
        cx.debug_bounds("files-search").is_some()
    });
    cx.simulate_input(MARK);
    // The find waits for typing to pause.
    cx.executor().advance_clock(Duration::from_millis(300));
    until(cx, "the file found", |cx| {
        cx.debug_bounds(Box::leak(format!("hit-{MARK}-note.txt").into_boxed_str()))
            .is_some()
    });
    cx.simulate_keystrokes("enter");
    until(cx, "the file open in the panel", |cx| {
        cx.debug_bounds(Box::leak(
            format!("file-view-{MARK}-note.txt").into_boxed_str(),
        ))
        .is_some()
    });
    untouched(cx, "the Files panel's find", 0);
    cx.simulate_keystrokes("escape");
    until(cx, "Esc closed Files", |cx| {
        cx.debug_bounds("panel-right").is_none()
    });
    assert!(
        term_focused(cx),
        "Esc from Files gives the terminal the keyboard"
    );
    // Work: what is typed goes to its find, so the first Esc clears it and
    // the second closes the panel.
    {
        let (keys, drawn) = ("alt-b", "issue-panel-search");
        cx.simulate_keystrokes(keys);
        until(cx, drawn, |cx| cx.debug_bounds(drawn).is_some());
        cx.simulate_input(MARK);
        untouched(cx, drawn, 0);
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(
            shell.read_with(cx, |s, cx| s.hub().read(cx).panel.open),
            "the first Esc cleared {drawn}, the panel stays"
        );
        cx.simulate_keystrokes("escape");
        until(cx, &format!("Esc closed {drawn}"), |cx| {
            cx.debug_bounds("panel-right").is_none()
        });
        assert!(
            term_focused(cx),
            "Esc from {drawn} gives the terminal the keyboard"
        );
    }

    // The layers, opened from the keyboard with the terminal focused.
    cx.update(|window, cx| {
        let focus = page.read(cx).term_focus.clone();
        window.focus(&focus, cx);
    });
    for (keys, drawn) in [
        ("alt-s", "send-drawer"),
        ("alt-h", "find-drawer"),
        ("alt-v", "overview-grid"),
    ] {
        cx.simulate_keystrokes(keys);
        cx.run_until_parked();
        let over = cx.debug_bounds(drawn).expect(drawn);
        // Overview's cards fill it and a click on one opens it: poke its
        // margin, which is the layer's own.
        let at = if drawn == "overview-grid" {
            let under = cx.debug_bounds("window-1").expect("window 1");
            let at = gpui::point(under.right() - px(2.), under.center().y);
            assert!(over.contains(&at), "{at:?} lies in Overview {over:?}");
            at
        } else {
            inside(over)
        };
        // Send and the search lie over no window: they made room for them.
        let under = if drawn != "overview-grid" {
            let window = cx.debug_bounds("window-1").expect("window 1");
            assert!(
                window.right() <= over.left(),
                "{drawn} {over:?} beside window 1 {window:?}, not over it"
            );
            drawn
        } else {
            "window-1"
        };
        poke(cx, at, under);
        untouched(cx, drawn, 0);
        cx.simulate_keystrokes("escape");
        until(cx, &format!("Esc closed {drawn}"), |cx| {
            cx.debug_bounds(drawn).is_none()
        });
        cx.update(|window, cx| {
            let focus = page.read(cx).term_focus.clone();
            window.focus(&focus, cx);
        });
    }

    // Peek is a terminal: what is done in it reaches the session it shows
    // (beta) and no window under it, and Escape is its program's.
    click(cx, format!("row-{}::beta", running.node));
    until(cx, "the peek", |cx| cx.debug_bounds("peek").is_some());
    let over = cx.debug_bounds("peek-body").expect("Peek's terminal");
    poke(cx, inside(over), "window-1");
    until(cx, "beta to take what was typed in Peek", |_| {
        tmux(&running, &["capture-pane", "-p", "-t", "beta"]).contains(MARK)
    });
    let alpha = tmux(&running, &["capture-pane", "-p", "-t", "alpha"]);
    assert!(!alpha.contains(MARK), "alpha took input through Peek");
    page.read_with(cx, |p, _| {
        assert_eq!(p.settings.workspace.focus, 0, "focused window after Peek");
        let windows = p.settings.workspace.windows().len();
        assert!(
            p.histories.keys().all(|w| *w >= windows),
            "no window reads back after Peek"
        );
    });
    assert!(
        !term_focused(cx),
        "the windows took the keyboard through Peek"
    );
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("peek").is_some(),
        "Esc is beta's, Peek stays"
    );
    cx.simulate_keystrokes("shift-escape");
    cx.run_until_parked();
    assert!(cx.debug_bounds("peek").is_none(), "Shift+Esc closed Peek");
    // Beta's prompt is clean again for what follows.
    tmux(&running, &["send-keys", "-t", "beta", "C-u"]);

    // A launcher too big for its window stays in it: its fields scroll
    // under its button, and beta's window beside it stays whole.
    cx.update(|_, cx| {
        page.update(cx, |p, cx| {
            p.settings.workspace.set_windows(0, 4);
            p.settings.workspace.focus = 3;
            cx.notify();
        })
    });
    until(cx, "the launcher in its window", |cx| {
        cx.debug_bounds("launch-3").is_some()
    });
    let window = cx.debug_bounds("window-3").expect("its window");
    let under = cx.debug_bounds("window-1").expect("beta's window");
    for part in ["launcher-fields-3", "launch-3"] {
        let b = cx.debug_bounds(part).expect(part);
        assert!(
            window.contains(&b.origin) && window.contains(&b.bottom_right()),
            "{part} {b:?} lies in its window {window:?}"
        );
        assert!(
            !b.intersects(&under),
            "{part} {b:?} leaves beta's window {under:?}"
        );
    }
    let _ = std::fs::remove_dir_all(settings.parent().expect("its folder"));
}

#[gpui::test]
fn a_page_starts_a_session_in_a_folder_through_launch(cx: &mut TestAppContext) {
    let Some(running) = start("terminals-page-launch") else {
        return;
    };
    let (shell, cx, settings) = open_with_alpha(&running, "launch", cx);
    let toy = shell.read_with(cx, |s, _| Host::new(s.hub()).for_page("toy", &[]));
    let sessions = cx
        .update(|_, cx| toy.model::<SessionList>(cx))
        .expect("the hub");
    let folder = settings.parent().expect("its folder").to_string_lossy();
    // The suggested name is taken, so the session takes the next free one.
    let ask = Ask::Launch {
        node: NodeId(running.node.clone()),
        folder: folder.to_string(),
        name: "alpha".into(),
    };
    cx.update(|_, cx| toy.ask(ask, cx))
        .expect("the Sessions page answers it");
    until(cx, "alpha-2 to be listed in the folder", |cx| {
        sessions.read_with(cx, |l, _| {
            l.sessions.iter().any(|s| {
                s.node == running.node
                    && s.name == "alpha-2"
                    && s.cwd.as_deref().is_some_and(|c| c.ends_with(&*folder))
            })
        })
    });
    // Its row's menu asks what shapes an agent in its folder; alpha-2 is a
    // shell, so the row says an agent started there.
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    let key = Key {
        node: running.node.clone(),
        user: String::new(),
        name: "alpha-2".into(),
    };
    until(cx, "alpha-2's folder known to the page", |cx| {
        page.read_with(cx, |p, _| {
            p.fleet
                .session(&key)
                .is_some_and(|s| s.cwd.as_deref().is_some_and(|c| c.ends_with(&*folder)))
        })
    });
    let sees = page.read_with(cx, |p, _| {
        p.row_menu(&key)
            .into_iter()
            .find(|r| r.label == "What an agent here sees")
            .expect("the row menu offers it")
            .act
            .unwrap()
    });
    ITEM_ASK.with(|slot| *slot.borrow_mut() = None);
    cx.update(|window, cx| page.update(cx, |p, cx| p.act(sees, window, cx)));
    cx.run_until_parked();
    match ITEM_ASK.with(|slot| slot.borrow().clone()) {
        Some(Ask::Reveal { what, id, node, .. }) => {
            assert_eq!(what, "agent");
            assert!(id.ends_with(&*folder), "{id} is alpha-2's folder");
            assert_eq!(node, Some(NodeId(running.node.clone())));
        }
        other => panic!("asked {other:?}"),
    }
    // A node the app does not know: the status line says why.
    let ask = Ask::Launch {
        node: NodeId("nowhere".into()),
        folder: "/".into(),
        name: "lost".into(),
    };
    cx.update(|_, cx| toy.ask(ask, cx))
        .expect("the Sessions page answers it");
    cx.run_until_parked();
    let said = shell.read_with(cx, |s, cx| {
        let status = s.hub().read(cx).status.clone();
        status.map(|s| (s.text, s.failure))
    });
    assert_eq!(
        said,
        Some((
            "Could not start lost: its node is not in the fleet".into(),
            true
        ))
    );
    let _ = std::fs::remove_dir_all(settings.parent().expect("its folder"));
}

fn tmux(running: &Running, args: &[&str]) -> String {
    let out =
        std::process::Command::new(std::env::var_os("AHJO_TMUX").unwrap_or_else(|| "tmux".into()))
            .args(["-S", running.socket.to_str().unwrap()])
            .args(args)
            .output()
            .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Waits for a launched session's window to take `grid`: tmux lists the
/// session the moment it exists, at its default size, and the launch sizes
/// it after that.
fn sized_to(cx: &mut VisualTestContext, running: &Running, session: &str, grid: (u16, u16)) {
    let want = format!("{}x{}", grid.0, grid.1);
    until(cx, &format!("{session} at {want}"), |_| {
        tmux(
            running,
            &[
                "display-message",
                "-p",
                "-t",
                session,
                "#{window_width}x#{window_height}",
            ],
        ) == want
    });
}

fn scroll_to_launch(cx: &mut VisualTestContext, scroll: &str) {
    let selector: &'static str = Box::leak(scroll.to_string().into_boxed_str());
    let point = cx.debug_bounds(selector).unwrap().center();
    cx.simulate_event(gpui::ScrollEvent {
        position: point,
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-1000.))),
        modifiers: Modifiers::none(),
        touch_phase: gpui::TouchPhase::Moved,
        source: gpui::ScrollSource::Pointer,
    });
    cx.run_until_parked();
}

#[gpui::test]
fn legacy_completion_hook_settings_are_rewritten_on_load(cx: &mut TestAppContext) {
    let Some(running) = start("terminals-hook-settings") else {
        return;
    };
    let path = ahjo_testkit::fresh_dir("hook-settings").join("app.json");
    std::fs::write(
        &path,
        serde_json::json!({
            "sessions": {
                "notify": false,
                "restarts": {
                    "old": {"id": "old", "folder": "/tmp", "harness": "codex",
                            "flags": "--search", "notify": true}
                }
            }
        })
        .to_string(),
    )
    .unwrap();
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let url = running.url.clone();
    let settings_path = path.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(&[terminals], Some(url), Some(settings_path), window, cx)
    });
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    page.read_with(cx, |p, _| {
        assert!(!p.settings.install_completion_hooks);
        assert!(p.settings.restarts["old"].install_completion_hooks);
    });
    until(cx, "the old hook preferences rewritten", |cx| {
        cx.executor().advance_clock(Duration::from_millis(300));
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let section = &saved["sessions"];
        section.get("notify").is_none()
            && section["install_completion_hooks"] == false
            && section["restarts"]["old"]["install_completion_hooks"] == true
            && section["restarts"]["old"].get("notify").is_none()
            && section["restarts"]["old"]["flags"] == "--search"
    });
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[gpui::test]
fn launches_use_each_tile_grid(cx: &mut TestAppContext) {
    let Some(running) = start("terminals-sized-launches") else {
        return;
    };
    let settings = ahjo_testkit::fresh_dir("sized-settings").join("app.json");
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let url = running.url.clone();
    let path = settings.clone();
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(&[terminals, toy], Some(url), Some(path), window, cx)
    });
    cx.simulate_resize(gpui::size(px(1440.), px(900.)));
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    cx.update(|_, cx| {
        page.update(cx, |p, cx| {
            p.settings.workspace.set_windows(0, 4);
            cx.notify();
        })
    });
    until(cx, "four tile grids", |cx| {
        page.read_with(cx, |p, _| {
            p.launch_grids.borrow().iter().take(4).all(Option::is_some)
        })
    });
    for i in 0..4 {
        let grid = page.read_with(cx, |p, _| p.launch_grids.borrow()[i].unwrap());
        let button = format!("launch-{i}");
        let selector: &'static str = Box::leak(button.clone().into_boxed_str());
        // Only the focused empty window offers the launcher: a click on
        // another one brings it there.
        if i > 0 {
            click(cx, format!("empty-window-{i}"));
        }
        until(cx, &format!("launcher ready in tile {i}"), |cx| {
            cx.debug_bounds(selector).is_some()
        });
        // A quarter of the window is too short for the whole launcher: it
        // stays in its window, its button in sight there.
        let tile: &'static str = Box::leak(format!("window-{i}").into_boxed_str());
        until(cx, &format!("the launch button inside tile {i}"), |cx| {
            let (Some(tile), Some(button)) = (cx.debug_bounds(tile), cx.debug_bounds(selector))
            else {
                return false;
            };
            tile.contains(&button.origin) && tile.contains(&button.bottom_right())
        });
        click(cx, button);
        until(cx, &format!("session bound in tile {i}"), |cx| {
            page.read_with(cx, |p, _| p.settings.workspace.shown(i).is_some())
        });
        let name = page.read_with(cx, |p, _| {
            p.settings.workspace.shown(i).unwrap().key.name.clone()
        });
        assert_eq!(
            tmux(
                &running,
                &[
                    "display-message",
                    "-p",
                    "-t",
                    &name,
                    "#{window_width}x#{window_height}"
                ]
            ),
            format!("{}x{}", grid.0, grid.1)
        );
        until(cx, "terminal painted", |cx| {
            page.read_with(cx, |p, _| p.geo.borrow()[i].body.is_some())
        });
        let font = cx.update(|window, cx| {
            let p = page.read(cx);
            let geo = p.geo.borrow()[i];
            ahjo_terminals_app::term::paint::fit(
                window,
                geo.body.unwrap().size,
                grid.0 as usize,
                grid.1 as usize,
                p.settings.font_size,
            )
            .font_size
        });
        assert!(f32::from(font) >= 11.);
    }
    // All four panes must receive their screens without clicking a tile.
    until(cx, "all four newly launched screens", |cx| {
        page.read_with(cx, |p, _| {
            (0..4).all(|i| p.window_text(i).is_some_and(|text| !text.trim().is_empty()))
        })
    });
    let grid = page.read_with(cx, |p, _| {
        p.launch_grids.borrow()[p.settings.workspace.focus].unwrap()
    });
    let host = shell.read_with(cx, |s, _| Host::new(s.hub()).for_page("toy", &[]));
    let folder = settings.parent().unwrap().to_string_lossy().to_string();
    cx.update(|_, cx| {
        host.ask(
            Ask::Launch {
                node: NodeId(running.node.clone()),
                folder,
                name: "asked-size".into(),
            },
            cx,
        )
    })
    .unwrap();
    until(cx, "asked session", |_cx| {
        tmux(&running, &["list-sessions", "-F", "#{session_name}"])
            .lines()
            .any(|s| s == "asked-size")
    });
    sized_to(cx, &running, "asked-size", grid);
    let before = tmux(&running, &["list-sessions", "-F", "#{session_name}"]);
    let submitted = running.fixture.join("send-was-submitted");
    let reference = format!("touch {}", submitted.display());
    cx.update(|_, cx| {
        host.ask(
            Ask::Send {
                reference: reference.clone(),
                target: None,
            },
            cx,
        )
    })
    .unwrap();
    until(cx, "new agent choice", |cx| {
        cx.debug_bounds("send-target-first").is_some()
    });
    click(cx, "send-target-first".into());
    until(cx, "send launcher", |cx| {
        cx.debug_bounds("send-body").is_some()
    });
    scroll_to_launch(cx, "send-body");
    // Send's launcher comes after one per window.
    let send_launch: &'static str = Box::leak(format!("launch-{MAX_WINDOWS}").into_boxed_str());
    until(cx, "visible send launch button", |cx| {
        cx.debug_bounds(send_launch)
            .zip(cx.debug_bounds("send-body"))
            .is_some_and(|(button, viewport)| viewport.contains(&button.center()))
    });
    click(cx, send_launch.into());
    let mut new_name = String::new();
    until(cx, "send session", |_cx| {
        let now = tmux(&running, &["list-sessions", "-F", "#{session_name}"]);
        if let Some(name) = now
            .lines()
            .find(|name| !before.lines().any(|old| old == *name))
        {
            new_name = name.to_string();
            true
        } else {
            false
        }
    });
    sized_to(cx, &running, &new_name, grid);
    until(cx, "new agent paste completed", |cx| {
        cx.debug_bounds("send-body").is_none()
    });
    // The new session runs the login shell, whose prompt and the reference
    // can be wider than the tile's grid: read the wrapped line joined.
    until(cx, "reference pasted into the new agent", |_cx| {
        tmux(&running, &["capture-pane", "-p", "-J", "-t", &new_name]).contains(&reference)
    });
    assert!(
        !submitted.exists(),
        "new agent must not submit the pasted command"
    );
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

#[gpui::test]
fn launcher_browses_and_selects_node_flags(cx: &mut TestAppContext) {
    let Some(running) = start_with_harness("terminals-flags", true) else {
        return;
    };
    let settings = ahjo_testkit::fresh_dir("flags-ui-settings").join("app.json");
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let url = running.url.clone();
    let path = settings.clone();
    let (_shell, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(&[terminals], Some(url), Some(path), window, cx)
    });
    until(cx, "fake harness", |cx| {
        cx.debug_bounds("harness-0-fake").is_some()
    });
    click(cx, "harness-0-fake".into());
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    until(cx, "fake harness selected", |cx| {
        page.read_with(cx, |p, cx| {
            p.launchers[0].read(cx).harness.as_deref() == Some("fake")
        })
    });
    until(cx, "visible Browse control", |cx| {
        cx.debug_bounds("flags-browse-0")
            .zip(cx.debug_bounds("launcher-scroll-0"))
            .is_some_and(|(button, viewport)| viewport.contains(&button.center()))
    });
    click(cx, "flags-browse-0".into());
    cx.run_until_parked();
    scroll_to_launch(cx, "launcher-scroll-0");
    until(cx, "verbose flag", |cx| {
        cx.debug_bounds("flag-0---verbose").is_some()
    });
    click(cx, "flag-0---verbose".into());
    until(cx, "flag in preview", |cx| {
        cx.debug_bounds("flag-line-0---verbose").is_some()
    });
    scroll_to_launch(cx, "launcher-scroll-0");
    until(cx, "visible launch button", |cx| {
        cx.debug_bounds("launch-0")
            .zip(cx.debug_bounds("launcher-scroll-0"))
            .is_some_and(|(button, viewport)| viewport.contains(&button.center()))
    });
    click(cx, "launch-0".into());
    until(cx, "selected flag reaches the harness", |_| {
        std::fs::read_to_string(running.fixture.join("runs"))
            .is_ok_and(|runs| runs.contains("--verbose"))
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

#[gpui::test]
fn restart_recovers_the_launched_harness_and_folder(cx: &mut TestAppContext) {
    let Some(running) = start_with_restart_harness("terminals-restart-harness", true, true) else {
        return;
    };
    let settings = ahjo_testkit::fresh_dir("restart-harness-settings").join("app.json");
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let url = running.url.clone();
    let path = settings.clone();
    let (_shell, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(&[terminals], Some(url), Some(path), window, cx)
    });
    until(cx, "fake harness", |cx| {
        cx.debug_bounds("harness-0-fake").is_some()
    });
    click(cx, "harness-0-fake".into());
    scroll_to_launch(cx, "launcher-scroll-0");
    until(cx, "launch button", |cx| {
        cx.debug_bounds("launch-0").is_some()
    });
    click(cx, "launch-0".into());
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    until(cx, "session bound", |cx| {
        page.read_with(cx, |p, _| p.settings.workspace.shown(0).is_some())
    });
    let name = page.read_with(cx, |p, _| {
        p.settings.workspace.shown(0).unwrap().key.name.clone()
    });
    // The node starts its shell through a launch gate and types the harness
    // into it: tmux's start command therefore identifies the gate, while the
    // fake's run records below prove which harness and flags actually ran.
    assert!(
        tmux(
            &running,
            &[
                "display-message",
                "-p",
                "-t",
                &name,
                "#{pane_start_command}"
            ]
        )
        .contains("wait-for")
    );
    let runs = running.fixture.join("runs");
    until(cx, "fake harness started", |_| {
        std::fs::read_to_string(&runs).is_ok_and(|s| s.lines().count() == 1)
    });
    assert_eq!(
        tmux(
            &running,
            &["display-message", "-p", "-t", &name, "#{pane_current_path}"]
        ),
        running.fixture.display().to_string()
    );
    tmux(&running, &["kill-session", "-t", &name]);
    until(cx, "ended tile", |cx| {
        cx.debug_bounds("ended-0-0").is_some()
    });
    click(cx, "ended-0-0".into());
    until(cx, "fake harness restarted", |_| {
        std::fs::read_to_string(&runs).is_ok_and(|s| s.lines().count() == 2)
    });
    assert_eq!(
        tmux(
            &running,
            &["display-message", "-p", "-t", &name, "#{pane_current_path}"]
        ),
        running.fixture.display().to_string()
    );
    assert!(
        tmux(
            &running,
            &[
                "display-message",
                "-p",
                "-t",
                &name,
                "#{pane_start_command}"
            ]
        )
        .contains("wait-for")
    );
    let records = std::fs::read_to_string(runs).unwrap();
    assert!(
        records
            .lines()
            .all(|line| line == format!("{}|--marker", running.fixture.display()))
    );
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

#[gpui::test]
fn restart_of_external_session_uses_last_folder_and_says_shell(cx: &mut TestAppContext) {
    let Some(running) = start("terminals-restart-external") else {
        return;
    };
    let (_shell, cx, settings) = open_with_alpha(&running, "restart-external", cx);
    let folder = settings.parent().unwrap().to_path_buf();
    tmux(
        &running,
        &[
            "send-keys",
            "-t",
            "alpha",
            &format!("cd '{}'", folder.display()),
            "Enter",
        ],
    );
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    let key = Key {
        node: running.node.clone(),
        user: String::new(),
        name: "alpha".into(),
    };
    until(cx, "last folder observed", |cx| {
        page.read_with(cx, |p, _| {
            p.settings
                .restarts
                .get(&key.text())
                .is_some_and(|r| r.folder == folder.to_string_lossy())
        })
    });
    tmux(&running, &["kill-session", "-t", "alpha"]);
    until(cx, "ended tile", |cx| {
        cx.debug_bounds("ended-0-0").is_some()
    });
    let note = format!(
        "ended-note-0-alpha has ended. Restart opens a shell in {}.",
        folder.display()
    );
    let selector: &'static str = Box::leak(note.into_boxed_str());
    assert!(cx.debug_bounds(selector).is_some());
    click(cx, "ended-0-0".into());
    until(cx, "alpha restarted", |_| {
        tmux(&running, &["list-sessions", "-F", "#{session_name}"])
            .lines()
            .any(|name| name == "alpha")
    });
    // tmux lists the session as soon as it forks the pane's child, which
    // has the server's folder until it changes to the restart's and execs.
    let want = folder.display().to_string();
    until(cx, &format!("alpha's shell in {want}"), |_| {
        tmux(
            &running,
            &[
                "display-message",
                "-p",
                "-t",
                "alpha",
                "#{pane_current_path}",
            ],
        ) == want
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

#[gpui::test]
fn send_chooses_a_pane_and_defaults_to_the_focused_pane(cx: &mut TestAppContext) {
    let Some(running) = start("terminals-send-panes") else {
        return;
    };
    let (shell, cx, settings) = open_with_alpha(&running, "send-panes", cx);
    tmux(
        &running,
        &["split-window", "-d", "-t", "alpha:0", "env PS1='$ ' sh"],
    );
    let ids = tmux(
        &running,
        &["list-panes", "-t", "alpha:0", "-F", "#{pane_id}"],
    );
    let panes: Vec<String> = ids.lines().map(str::to_owned).collect();
    assert_eq!(panes.len(), 2);
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    let key = Key {
        node: running.node.clone(),
        user: String::new(),
        name: "alpha".into(),
    };
    until(cx, "two panes listed", |cx| {
        page.read_with(cx, |p, _| {
            p.fleet.session(&key).is_some_and(|s| s.panes.len() == 2)
        })
    });
    let host = shell.read_with(cx, |s, _| Host::new(s.hub()).for_page("toy", &[]));
    let target = SessionTarget {
        node: NodeId(running.node.clone()),
        user: String::new(),
        name: "alpha".into(),
    };
    cx.update(|_, cx| {
        host.ask(
            Ask::Send {
                reference: "echo pane-second-$((40+2))".into(),
                target: Some(target.clone()),
            },
            cx,
        )
    })
    .unwrap();
    until(cx, "pane chooser", |cx| {
        cx.debug_bounds("send-pane-1").is_some()
    });
    assert!(cx.debug_bounds("send-pane-selected-0").is_some());
    click(cx, "send-pane-1".into());
    until(cx, "second pane selected", |cx| {
        cx.debug_bounds("send-pane-selected-1").is_some()
    });
    click(cx, "send-go".into());
    until(cx, "text in second pane", |_| {
        tmux(&running, &["capture-pane", "-p", "-t", &panes[1]]).contains("pane-second-42")
    });
    assert!(!tmux(&running, &["capture-pane", "-p", "-t", &panes[0]]).contains("pane-second-42"));
    until(cx, "drawer closed", |cx| {
        cx.debug_bounds("send-go").is_none()
    });
    let second = panes[1].trim_start_matches('%').parse::<u32>().unwrap();
    cx.update(|_, cx| {
        page.update(cx, |p, cx| {
            p.settings.workspace.choose_pane(0, second);
            cx.notify();
        })
    });
    cx.update(|_, cx| {
        host.ask(
            Ask::Send {
                reference: "echo next".into(),
                target: Some(target.clone()),
            },
            cx,
        )
    })
    .unwrap();
    until(cx, "focused pane selected by default", |cx| {
        cx.debug_bounds("send-pane-selected-1").is_some()
    });
    // The target can also be shown in a different tile; its displayed pane
    // remains the default even when another tile has focus.
    cx.update(|_, cx| {
        page.update(cx, |p, cx| {
            p.settings.workspace.set_windows(0, 2);
            p.settings
                .workspace
                .bind_in(0, 1, key.clone(), Some(second));
            p.settings.workspace.focus = 0;
            cx.notify();
        })
    });
    cx.update(|_, cx| {
        host.ask(
            Ask::Send {
                reference: "echo next".into(),
                target: Some(target),
            },
            cx,
        )
    })
    .unwrap();
    until(cx, "other tile's pane selected", |cx| {
        cx.debug_bounds("send-pane-selected-1").is_some()
    });
    // Explicitly select that pane, then let it disappear: the drawer must
    // report the loss instead of silently sending into the other pane.
    click(cx, "send-pane-selected-1".into());
    tmux(&running, &["kill-pane", "-t", &panes[1]]);
    // The other pane was offered all along: the chosen one going from the
    // choices is what says the page has heard of the loss.
    until(cx, "only the remaining pane offered", |cx| {
        cx.debug_bounds("send-pane-0").is_some()
            && cx.debug_bounds("send-pane-selected-1").is_none()
    });
    click(cx, "send-go".into());
    until(cx, "pane loss reported", |cx| {
        page.read_with(cx, |p, _| {
            p.send
                .missed
                .iter()
                .any(|(_, s)| s.contains("selected pane") && s.contains("gone"))
        })
    });
    assert!(!tmux(&running, &["capture-pane", "-p", "-t", &panes[0]]).contains("echo next"));
    click(cx, "send-pane-0".into());
    let first = panes[0].trim_start_matches('%').parse::<u32>().unwrap();
    until(cx, "remaining pane selected", |cx| {
        page.read_with(cx, |p, _| p.send.pane == Some(first))
    });
    cx.update(|_, cx| {
        host.ask(
            Ask::Send {
                reference: "echo single".into(),
                target: Some(SessionTarget {
                    node: NodeId(running.node.clone()),
                    user: String::new(),
                    name: "beta".into(),
                }),
            },
            cx,
        )
    })
    .unwrap();
    until(cx, "single-pane target", |cx| {
        page.read_with(cx, |p, _| {
            p.send_targets.iter().map(|k| k.name.as_str()).eq(["beta"])
        })
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("send-pane-0").is_none());
    assert!(cx.debug_bounds("send-pane-selected-0").is_none());
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn cli_send_accepts_an_explicit_pane_id() {
    let Some(server) = Server::start_in("terminals-cli-send-pane") else {
        return;
    };
    server.new_session("cli-alpha", 100, 30);
    server.run(&["split-window", "-d", "-t", "cli-alpha:0", "env PS1='$ ' sh"]);
    let ids = server.run(&["list-panes", "-t", "cli-alpha:0", "-F", "#{pane_id}"]);
    let panes: Vec<&str> = ids.lines().collect();
    assert_eq!(panes, ["%0", "%1"]);
    let config = format!("[terminals]\nsocket = {:?}\n", server.socket_path());
    let node = Node::start(AREAS, &config).await.unwrap();
    ahjo_testkit::tmux::eventually("both panes listed", || {
        node.sessions()
            .list()
            .iter()
            .find(|s| s.name == "cli-alpha" && s.panes.len() == 2)
            .map(|_| ())
    })
    .await;
    let url = node.url();
    let args = [
        "send",
        "cli-alpha",
        "--pane",
        "1",
        "--text",
        "echo cli-pane-42",
        "--node",
        &url,
    ]
    .map(str::to_owned);
    assert_eq!(
        ahjo_node_kit::cli::run("session", &args, &mut Vec::new())
            .await
            .unwrap(),
        0
    );
    ahjo_testkit::tmux::eventually("CLI text in pane 1", || {
        server
            .run(&["capture-pane", "-p", "-t", "%1"])
            .contains("cli-pane-42")
            .then_some(())
    })
    .await;
    assert!(
        !server
            .run(&["capture-pane", "-p", "-t", "%0"])
            .contains("cli-pane-42")
    );
    node.stop().await;
}

#[gpui::test]
fn reveal_session_shows_the_requested_session_and_pane(cx: &mut TestAppContext) {
    let Some(running) = start("terminals-reveal") else {
        return;
    };
    let (shell, cx, settings) = open_with_alpha(&running, "reveal", cx);
    tmux(
        &running,
        &["split-window", "-d", "-t", "beta:0", "env PS1='$ ' sh"],
    );
    let ids = tmux(
        &running,
        &["list-panes", "-t", "beta:0", "-F", "#{pane_id}"],
    );
    let panes: Vec<u32> = ids
        .lines()
        .map(|s| s.trim_start_matches('%').parse().unwrap())
        .collect();
    assert_eq!(panes.len(), 2);
    assert_eq!(panes[0], 1, "the test's first beta pane is %1");
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    let beta = Key {
        node: running.node.clone(),
        user: String::new(),
        name: "beta".into(),
    };
    until(cx, "beta's two panes", |cx| {
        page.read_with(cx, |p, _| {
            p.fleet.session(&beta).is_some_and(|s| s.panes.len() == 2)
        })
    });
    let beta_id = page.read_with(cx, |p, _| p.fleet.session(&beta).unwrap().id.clone());
    click(cx, "tab-toy".into());
    until(cx, "toy page", |cx| {
        shell.read_with(cx, |s, _| s.shown_page()) == Some("toy")
    });
    let host = shell.read_with(cx, |s, _| Host::new(s.hub()).for_page("toy", &[]));
    cx.update(|_, cx| {
        host.ask(
            Ask::Reveal {
                what: "session".into(),
                beside: false,
                id: "beta".into(),
                node: None,
                part: None,
            },
            cx,
        )
    })
    .unwrap();
    until(cx, "beta revealed without a pane", |cx| {
        shell.read_with(cx, |s, _| s.shown_page()) == Some("sessions")
            && page.read_with(cx, |p, _| {
                p.settings.workspace.shown(0).is_some_and(|b| b.key == beta)
            })
    });
    cx.update(|_, cx| {
        host.ask(
            Ask::Reveal {
                what: "session".into(),
                beside: false,
                id: "beta".into(),
                node: None,
                part: Some("1".into()),
            },
            cx,
        )
    })
    .unwrap();
    until(cx, "beta on the Sessions page", |cx| {
        shell.read_with(cx, |s, _| s.shown_page()) == Some("sessions")
            && page.read_with(cx, |p, _| {
                p.settings
                    .workspace
                    .shown(0)
                    .is_some_and(|b| b.key == beta && b.pane == Some(1))
            })
    });
    until(cx, "beta pane painted", |cx| {
        page.read_with(cx, |p, _| p.window_text(0).is_some())
    });
    assert!(cx.update(|window, cx| page.read(cx).term_focus.is_focused(window)));
    cx.simulate_input("echo revealed-pane-$((40+2))");
    cx.simulate_keystrokes("enter");
    until(cx, "typing reaches pane 1", |_| {
        tmux(&running, &["capture-pane", "-p", "-t", "%1"]).contains("revealed-pane-42")
    });
    assert!(
        !tmux(
            &running,
            &["capture-pane", "-p", "-t", &format!("%{}", panes[1])]
        )
        .contains("revealed-pane-42")
    );

    // A bound session in a hidden tile is restored there, not moved into
    // the focused tile; an id and a %pane work as well as a name and number.
    cx.update(|_, cx| {
        page.update(cx, |p, cx| {
            p.settings.workspace.bind_in(0, 1, beta.clone(), Some(1));
            p.settings.workspace.set_windows(0, 1);
            p.settings.workspace.focus = 0;
            cx.notify();
        })
    });
    cx.update(|_, cx| {
        host.ask(
            Ask::Reveal {
                what: "session".into(),
                beside: false,
                id: beta_id,
                node: Some(NodeId(running.node.clone())),
                part: Some(format!("%{}", panes[1])),
            },
            cx,
        )
    })
    .unwrap();
    until(cx, "hidden bound tile restored", |cx| {
        page.read_with(cx, |p, _| {
            p.settings.workspace.focus == 1
                && p.settings
                    .workspace
                    .shown(1)
                    .is_some_and(|b| b.key == beta && b.pane == Some(panes[1]))
        })
    });
    let before = page.read_with(cx, |p, _| p.settings.workspace.clone());
    cx.update(|_, cx| {
        host.ask(
            Ask::Reveal {
                what: "session".into(),
                beside: false,
                id: "missing".into(),
                node: Some(NodeId(running.node.clone())),
                part: None,
            },
            cx,
        )
    })
    .unwrap();
    until(cx, "missing session reported", |cx| {
        shell.read_with(cx, |s, cx| {
            s.hub().read(cx).status.as_ref().is_some_and(|status| {
                status.failure && status.text == format!("no session missing on {}", running.node)
            })
        })
    });
    assert_eq!(
        page.read_with(cx, |p, _| p.settings.workspace.clone()),
        before
    );
    cx.update(|_, cx| {
        host.ask(
            Ask::Reveal {
                what: "session".into(),
                beside: false,
                id: "beta".into(),
                node: Some(NodeId(running.node.clone())),
                part: Some("99999".into()),
            },
            cx,
        )
    })
    .unwrap();
    until(cx, "missing pane reported", |cx| {
        shell.read_with(cx, |s, cx| {
            s.hub()
                .read(cx)
                .status
                .as_ref()
                .is_some_and(|status| status.failure && status.text.contains("no pane 99999"))
        })
    });
    assert_eq!(
        page.read_with(cx, |p, _| p.settings.workspace.clone()),
        before
    );
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

#[gpui::test]
fn work_item_in_terminal_opens_beside_and_focus_is_published(cx: &mut TestAppContext) {
    let Some(running) = start("terminals-work-link") else {
        return;
    };
    let (shell, cx, settings) = open_with_alpha(&running, "issue-link", cx);
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    let toy = shell.read_with(cx, |s, _| Host::new(s.hub()).for_page("toy", &[]));
    let sessions = cx.update(|_, cx| toy.model::<SessionList>(cx)).unwrap();
    until(cx, "alpha focused in the shared model", |cx| {
        sessions.read_with(cx, |list, _| {
            list.focused
                .as_ref()
                .is_some_and(|f| f.node == running.node && f.user.is_empty() && f.name == "alpha")
        })
    });
    tmux(
        &running,
        &[
            "send-keys",
            "-t",
            "alpha",
            "printf '\\033[2J\\033[Hparvi-pnu.7.3\\n'",
            "Enter",
        ],
    );
    until(cx, "work id printed", |cx| {
        page.read_with(cx, |p, _| {
            p.window_text(0)
                .is_some_and(|t| t.lines().any(|l| l.trim() == "parvi-pnu.7.3"))
        })
    });
    let pos = page.read_with(cx, |p, _| {
        let text = p.window_text(0).unwrap();
        let row = text
            .lines()
            .position(|line| line.trim() == "parvi-pnu.7.3")
            .unwrap();
        let geo = p.geo.borrow()[0];
        let origin = geo.origin.unwrap();
        let cell = geo.cell.unwrap();
        gpui::point(
            origin.x + cell.width * 3. + px(2.),
            origin.y + cell.height * row as f32 + px(2.),
        )
    });
    ITEM_ASK.with(|slot| *slot.borrow_mut() = None);
    cx.simulate_click(pos, Modifiers::none());
    until(cx, "work item asked", |_| {
        ITEM_ASK.with(|slot| slot.borrow().is_some())
    });
    let ask = ITEM_ASK.with(|slot| slot.borrow().clone()).unwrap();
    assert_eq!(
        ask,
        Ask::Reveal {
            what: "item".into(),
            id: "parvi-pnu.7.3".into(),
            node: Some(NodeId(running.node.clone())),
            part: None,
            beside: true
        }
    );
    until(cx, "item beside terminals", |cx| {
        cx.debug_bounds("toy-note").is_some()
    });
    assert_eq!(shell.read_with(cx, |s, _| s.shown_page()), Some("sessions"));
    let beta = Key {
        node: running.node.clone(),
        user: String::new(),
        name: "beta".into(),
    };
    cx.update(|window, cx| {
        page.update(cx, |p, cx| {
            p.settings.workspace.set_windows(0, 2);
            p.attach(beta, 1, window, cx);
        })
    });
    until(cx, "beta focused in shared model", |cx| {
        sessions.read_with(cx, |list, _| {
            list.focused
                .as_ref()
                .is_some_and(|f| f.node == running.node && f.user.is_empty() && f.name == "beta")
        })
    });
    until(cx, "alpha tile painted", |cx| {
        page.read_with(cx, |p, _| p.geo.borrow()[0].body.is_some())
    });
    let alpha = page.read_with(cx, |p, _| p.geo.borrow()[0].body.unwrap().center());
    cx.simulate_click(alpha, Modifiers::none());
    until(cx, "click focuses alpha in shared model", |cx| {
        sessions.read_with(cx, |list, _| {
            list.focused
                .as_ref()
                .is_some_and(|f| f.node == running.node && f.user.is_empty() && f.name == "alpha")
        })
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

/// Keep the first launch's shell from becoming ready until after a later
/// unknown-node ask has reported its failure. The node has already listed
/// the new session, but has not answered the launch yet.
#[gpui::test]
fn late_launch_success_does_not_replace_a_newer_failure(cx: &mut TestAppContext) {
    let Some(running) = start_with_harness("terminals-launch-status-order", true) else {
        return;
    };
    let (shell, cx, settings) = open_with_alpha(&running, "launch-status-order", cx);
    let gate_shell = running.fixture.join("gate-shell");
    let gate_ready = running.fixture.join("gate-ready");
    let plain_shell = tmux(&running, &["show-options", "-gv", "default-shell"]);
    let tmux_program = std::env::var_os("AHJO_TMUX").unwrap_or_else(|| "tmux".into());
    // Preserve tmux's -c invocation of the node's launch gate. Starting
    // bash before the engine attaches can lose its initial
    // bracketed-paste signal, which a capture of the screen cannot recover.
    ahjo_testkit::tmux::write_script(
        &gate_shell,
        format!(
            "#!/bin/sh\nif [ \"$1\" = -c ]; then exec '{plain_shell}' \"$@\"; fi\nprintf '%s\\n' \"$1\" > '{}'\n'{}' -S '{}' wait-for launch-status-release || exit\nexec '{plain_shell}' \"$@\"\n",
            gate_ready.display(),
            std::path::PathBuf::from(tmux_program).display(),
            running.socket.display()
        ),
    );
    tmux(
        &running,
        &[
            "set-option",
            "-g",
            "default-shell",
            gate_shell.to_str().unwrap(),
        ],
    );
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    cx.update(|_, cx| {
        page.update(cx, |p, cx| {
            p.settings.last_harness = Some("fake".into());
            cx.notify();
        })
    });
    let toy = shell.read_with(cx, |s, _| Host::new(s.hub()).for_page("toy", &[]));
    let folder = settings.parent().unwrap().to_string_lossy().to_string();
    cx.update(|_, cx| {
        toy.ask(
            Ask::Launch {
                node: NodeId(running.node.clone()),
                folder,
                name: "alpha".into(),
            },
            cx,
        )
    })
    .unwrap();
    until(
        cx,
        "first launch listed and waiting to start its shell",
        |cx| {
            std::fs::read_to_string(&gate_ready).is_ok_and(|args| args == "-l\n")
                && page.read_with(cx, |p, _| {
                    !p.launches.is_empty()
                        && p.fleet
                            .all
                            .iter()
                            .any(|s| s.node == running.node && s.name == "alpha-2")
                })
        },
    );
    cx.update(|_, cx| {
        toy.ask(
            Ask::Launch {
                node: NodeId("nowhere".into()),
                folder: "/".into(),
                name: "lost".into(),
            },
            cx,
        )
    })
    .unwrap();
    until(cx, "newer failure reported", |cx| {
        shell.read_with(cx, |s, cx| {
            s.hub().read(cx).status.as_ref().is_some_and(|status| {
                status.failure
                    && status.text == "Could not start lost: its node is not in the fleet"
            })
        })
    });
    tmux(&running, &["wait-for", "-S", "launch-status-release"]);
    until(cx, "first launch answered", |cx| {
        page.read_with(cx, |p, _| p.launches.is_empty())
    });
    until(cx, "the first launch's harness ran", |_| {
        std::fs::read_to_string(running.fixture.join("runs"))
            .is_ok_and(|runs| runs.lines().count() == 1)
    });
    let status = shell.read_with(cx, |s, cx| s.hub().read(cx).status.clone());
    assert_eq!(
        status.map(|s| (s.text, s.failure)),
        Some((
            "Could not start lost: its node is not in the fleet".into(),
            true
        ))
    );
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

/// The shell routes the leader and Escape while a real terminal is attached.
#[gpui::test]
fn leader_and_escape_route_around_an_attached_terminal(cx: &mut TestAppContext) {
    let Some(running) = start("terminals-keys-journey") else {
        return;
    };
    let (shell, cx, settings) = open_with_alpha(&running, "keys-journey", cx);
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    cx.simulate_keystrokes("ctrl-shift-space s");
    until(cx, "leader opens Send", |cx| {
        cx.debug_bounds("send-body").is_some()
    });
    cx.simulate_keystrokes("escape");
    until(cx, "Escape closes Send", |cx| {
        cx.debug_bounds("send-body").is_none()
    });
    assert!(!page.read_with(cx, |p, _| p.is_open("send")));
    let escape_file = running.fixture.join("escape-byte");
    tmux(
        &running,
        &[
            "send-keys",
            "-t",
            "alpha",
            &format!(
                "stty raw -echo; dd bs=1 count=1 of='{}' 2>/dev/null; stty sane",
                escape_file.display()
            ),
            "Enter",
        ],
    );
    until(cx, "terminal waiting for a byte", |_| {
        tmux(
            &running,
            &[
                "display-message",
                "-p",
                "-t",
                "alpha",
                "#{pane_current_command}",
            ],
        )
        .trim()
            == "dd"
    });
    cx.simulate_keystrokes("escape");
    until(cx, "Escape reaches the terminal", |_| {
        std::fs::read(&escape_file).is_ok_and(|b| b == [27])
    });
    // A Finnish keyboard produces plus without Shift.
    cx.simulate_event(gpui::KeyDownEvent {
        keystroke: gpui::Keystroke {
            key: "+".into(),
            key_char: Some("+".into()),
            modifiers: Modifiers {
                alt: true,
                ..Modifiers::none()
            },
        },
        is_held: false,
        prefer_character_input: false,
    });
    until(cx, "Finnish Alt-plus adds a window", |cx| {
        page.read_with(cx, |p, _| p.settings.workspace.windows().len() == 2)
    });
    cx.simulate_keystrokes("ctrl-shift-space k");
    until(cx, "keys off", |cx| {
        shell.read_with(cx, |s, cx| !s.hub().read(cx).settings.keys)
    });
    cx.simulate_keystrokes("alt-s");
    cx.run_until_parked();
    assert!(cx.debug_bounds("send-body").is_none());
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

#[gpui::test]
fn peek_keyboard_dismisses_and_restores_focus_after_a_leader_action(cx: &mut TestAppContext) {
    let Some(running) = start("peek-keyboard-focus") else {
        return;
    };
    let (_shell, cx, settings) = open_with_alpha(&running, "peek-keyboard-focus", cx);
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    until(cx, "beta listed", |cx| {
        page.read_with(cx, |p, _| p.fleet.all.iter().any(|s| s.name == "beta"))
    });
    cx.simulate_keystrokes("ctrl-shift-space /");
    cx.simulate_input("beta");
    cx.simulate_keystrokes("enter");
    until(cx, "keyboard opens Peek", |cx| {
        cx.debug_bounds("peek-attach").is_some()
    });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("peek-attach").is_some(),
        "Escape is the session's in Peek"
    );
    cx.simulate_keystrokes("shift-escape");
    until(cx, "Shift+Escape closes Peek", |cx| {
        cx.debug_bounds("peek-attach").is_none()
    });
    cx.simulate_input("echo peek-escape-restored");
    cx.simulate_keystrokes("enter");
    until(cx, "typing returns to the terminal", |cx| {
        shown(cx).contains("peek-escape-restored")
    });

    cx.simulate_keystrokes("ctrl-shift-space /");
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("beta");
    cx.simulate_keystrokes("enter");
    until(cx, "keyboard reopens Peek", |cx| {
        cx.debug_bounds("peek-attach").is_some()
    });
    cx.simulate_keystrokes("ctrl-shift-space s");
    until(cx, "leader opens Send over Peek", |cx| {
        cx.debug_bounds("send-drawer").is_some()
    });
    cx.simulate_keystrokes("escape");
    until(cx, "Escape returns to Peek", |cx| {
        cx.debug_bounds("send-drawer").is_none() && cx.debug_bounds("peek-attach").is_some()
    });
    cx.simulate_keystrokes("ctrl-shift-space v");
    until(cx, "leader opens Overview over Peek", |cx| {
        cx.debug_bounds("overview-grid").is_some()
    });
    cx.simulate_keystrokes("escape");
    until(cx, "Overview returns focus to Peek", |cx| {
        cx.debug_bounds("overview-grid").is_none() && cx.debug_bounds("peek-attach").is_some()
    });
    cx.simulate_keystrokes("shift-escape");
    until(cx, "Shift+Escape then closes Peek", |cx| {
        cx.debug_bounds("peek-attach").is_none()
    });
    cx.simulate_input("echo peek-leader-restored");
    cx.simulate_keystrokes("enter");
    until(cx, "typing returns after nested layer", |cx| {
        shown(cx).contains("peek-leader-restored")
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

#[gpui::test]
fn a_mouse_down_anywhere_outside_peek_closes_it(cx: &mut TestAppContext) {
    let Some(running) = start("peek-click-outside") else {
        return;
    };
    let (_shell, cx, settings) = open_with_alpha(&running, "peek-click-outside", cx);
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    let (alpha, beta) = (
        format!("row-{}::alpha", running.node),
        format!("row-{}::beta", running.node),
    );
    let peeked = |cx: &mut VisualTestContext| {
        page.read_with(cx, |p, _| p.peek.as_ref().map(|p| p.key.name.clone()))
    };
    until(cx, "beta in the rail", |cx| {
        page.read_with(cx, |p, _| p.fleet.all.iter().any(|s| s.name == "beta"))
    });
    click(cx, beta.clone());
    until(cx, "beta peeked", |cx| {
        cx.debug_bounds("peek-body").is_some()
    });

    // What is done inside stays inside: a click, a selection dragged out
    // of the card, a menu opened in its terminal and its item picked.
    let card = cx.debug_bounds("peek").expect("Peek");
    let body = cx.debug_bounds("peek-body").expect("Peek's terminal");
    click(cx, "peek-body".into());
    let from = body.center();
    cx.simulate_mouse_down(from, gpui::MouseButton::Left, Modifiers::none());
    let out = gpui::point(card.left() - px(20.), from.y);
    cx.simulate_mouse_move(out, gpui::MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(out, gpui::MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(peeked(cx).as_deref(), Some("beta"), "a drag out keeps Peek");
    cx.simulate_mouse_down(from, gpui::MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(from, gpui::MouseButton::Right, Modifiers::none());
    until(cx, "Peek's terminal menu", |cx| {
        cx.debug_bounds("menu-label-Scroll back").is_some()
    });
    click(cx, "menu-label-Scroll back".into());
    until(cx, "the menu to close", |cx| {
        cx.debug_bounds("menu-label-Scroll back").is_none()
    });
    assert_eq!(peeked(cx).as_deref(), Some("beta"), "its menu keeps Peek");

    // Another session in the rail switches Peek to it; the one peeked
    // closes it.
    click(cx, alpha.clone());
    until(cx, "alpha peeked", |cx| {
        peeked(cx).as_deref() == Some("alpha")
    });
    click(cx, alpha.clone());
    until(cx, "a click on the peeked row closes Peek", |cx| {
        peeked(cx).is_none() && cx.debug_bounds("peek").is_none()
    });

    // The app's top bar, outside the page, closes it too, and a right
    // click outside does.
    click(cx, beta.clone());
    until(cx, "beta peeked again", |cx| {
        cx.debug_bounds("peek").is_some()
    });
    click(cx, "app-mark".into());
    until(cx, "the top bar closes Peek", |cx| {
        cx.debug_bounds("peek").is_none()
    });
    click(cx, beta.clone());
    until(cx, "beta peeked again", |cx| {
        cx.debug_bounds("peek").is_some()
    });
    let mark = cx.debug_bounds("app-mark").expect("the mark").center();
    cx.simulate_mouse_down(mark, gpui::MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(mark, gpui::MouseButton::Right, Modifiers::none());
    until(cx, "a right click outside closes Peek", |cx| {
        cx.debug_bounds("peek").is_none()
    });

    // What the click lands on still does its thing: a page's tab shows it.
    click(cx, beta);
    until(cx, "beta peeked again", |cx| {
        cx.debug_bounds("peek").is_some()
    });
    click(cx, "tab-toy".into());
    until(cx, "the toy shown and Peek closed", |cx| {
        cx.debug_bounds("toy-send").is_some() && cx.debug_bounds("peek").is_none()
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

#[gpui::test]
fn overview_keyboard_dismisses_selects_and_routes_leader(cx: &mut TestAppContext) {
    let Some(running) = start("terminals-overview-keys") else {
        return;
    };
    let (_shell, cx, settings) = open_with_alpha(&running, "overview-keys", cx);
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    until(cx, "both sessions in the overview", |cx| {
        page.read_with(cx, |p, _| p.fleet.all.len() == 2)
    });

    cx.simulate_keystrokes("alt-v");
    until(cx, "overview open", |cx| {
        cx.debug_bounds("overview-grid").is_some()
    });
    cx.simulate_keystrokes("escape");
    until(cx, "Escape closes overview", |cx| {
        cx.debug_bounds("overview-grid").is_none()
    });

    cx.simulate_keystrokes("alt-v");
    until(cx, "overview reopened", |cx| {
        cx.debug_bounds("overview-grid").is_some()
    });
    cx.simulate_keystrokes("ctrl-shift-space v");
    until(cx, "leader closes overview", |cx| {
        cx.debug_bounds("overview-grid").is_none()
    });

    cx.simulate_keystrokes("alt-v");
    until(cx, "overview reopened for selection", |cx| {
        cx.debug_bounds("overview-grid").is_some()
    });
    let (index, wanted) = page.read_with(cx, |p, _| {
        let index = p.fleet.all.iter().position(|s| s.name == "beta").unwrap();
        (index, Key::of(&p.fleet.all[index]))
    });
    cx.simulate_keystrokes("right");
    until(cx, "arrow selects second session", |cx| {
        page.read_with(cx, |p, _| p.overview_cursor == 1)
    });
    cx.simulate_keystrokes("left");
    until(cx, "left selects the first session", |cx| {
        page.read_with(cx, |p, _| p.overview_cursor == 0)
    });
    cx.simulate_keystrokes("down");
    until(cx, "down selects the next row", |cx| {
        page.read_with(cx, |p, _| p.overview_cursor == 1)
    });
    cx.simulate_keystrokes("up");
    until(cx, "up selects the previous row", |cx| {
        page.read_with(cx, |p, _| p.overview_cursor == 0)
    });
    if index == 1 {
        cx.simulate_keystrokes("right");
    }
    cx.simulate_keystrokes("enter");
    until(cx, "Enter opens selected session", |cx| {
        cx.debug_bounds("overview-grid").is_none()
            && page.read_with(cx, |p, _| p.settings.workspace.bound(&wanted).is_some())
    });
    until(cx, "selected session is ready", |cx| {
        page.read_with(cx, |p, _| {
            p.window_text(p.settings.workspace.focus)
                .is_some_and(|text| text.contains('$'))
        })
    });
    cx.simulate_input("echo overview-keyboard-restored");
    cx.simulate_keystrokes("enter");
    until(cx, "typing reaches selected session", |_| {
        tmux(&running, &["capture-pane", "-p", "-t", "beta"]).contains("overview-keyboard-restored")
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

/// A clipboard image travels through Send to the target node before its path
/// and the originating reference are delivered to the session.
#[gpui::test]
fn pasted_attachment_reaches_the_node_and_session(cx: &mut TestAppContext) {
    let Some(running) = start_config("terminals-pasted-attachment", false, false, true, false)
    else {
        return;
    };
    let received = running.fixture.join("received");
    tmux(
        &running,
        &[
            "send-keys",
            "-t",
            "alpha",
            &format!("cat > '{}'", received.display()),
            "Enter",
        ],
    );
    let (shell, cx, settings) = open_with_alpha(&running, "pasted-attachment", cx);
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    let host = shell.read_with(cx, |s, _| Host::new(s.hub()).for_page("toy", &[]));
    cx.update(|_, cx| {
        host.ask(
            Ask::Send {
                reference: "file /example.txt".into(),
                target: None,
            },
            cx,
        )
    })
    .unwrap();
    until(cx, "Send drawer", |cx| cx.debug_bounds("send-go").is_some());
    let bytes = vec![1, 2, 3, 4, 5];
    let image = gpui::Image::from_bytes(gpui::ImageFormat::Png, bytes.clone());
    cx.write_to_clipboard(gpui::ClipboardItem::new_image(&image));
    cx.update(|window, cx| {
        page.update(cx, |p, cx| {
            ahjo_app_kit::widgets::field::focus(&p.send_note, window, cx);
        })
    });
    cx.simulate_keystrokes("ctrl-v");
    until(cx, "clipboard attachment", |cx| {
        page.read_with(cx, |p, _| !p.send.files.is_empty())
    });
    click(cx, "send-go".into());
    let mut path = std::path::PathBuf::new();
    until(cx, "reference and drop path delivered", |_| {
        let Ok(text) = std::fs::read_to_string(&received) else {
            return false;
        };
        if !text.contains("file /example.txt") {
            return false;
        }
        let Some(drop) = text.lines().find_map(|line| line.strip_prefix("- ")) else {
            return false;
        };
        path = drop.into();
        path.exists()
    });
    assert!(path.to_string_lossy().contains("drops"));
    assert_eq!(std::fs::read(path).unwrap(), bytes);
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

/// Hooks mark both the session and its tab until a CLI acknowledgement.
#[gpui::test]
fn hook_reports_mark_session_and_tab_until_cli_seen(cx: &mut TestAppContext) {
    struct ToneLog;
    static TONES: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    static LOGGER: ToneLog = ToneLog;
    impl log::Log for ToneLog {
        fn enabled(&self, _: &log::Metadata<'_>) -> bool {
            true
        }
        fn log(&self, record: &log::Record<'_>) {
            if record.target() == "ahjo_terminals_app::sound" {
                TONES.lock().unwrap().push(record.args().to_string());
            }
        }
        fn flush(&self) {}
    }
    log::set_logger(&LOGGER).expect("install the tone observer");
    log::set_max_level(log::LevelFilter::Warn);
    let Some(running) = start("terminals-hook-journey") else {
        return;
    };
    let (_shell, cx, settings) = open_with_alpha(&running, "hook-journey", cx);
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    cx.update(|_, cx| page.update(cx, |p, _| p.settings.event_tone = true));
    let key = Key {
        node: running.node.clone(),
        user: String::new(),
        name: "alpha".into(),
    };
    let id = page.read_with(cx, |p, _| p.fleet.session(&key).unwrap().id.clone());
    // The terminal is not being looked at, so a report remains news.
    click(cx, "tab-toy".into());
    for (event, kind) in [
        ("finished", ahjo_terminals_contract::EventKind::Finished),
        (
            "needs-input",
            ahjo_terminals_contract::EventKind::NeedsInput,
        ),
    ] {
        let url = format!(
            "{}/api/agent/event?session=%24{}&event={event}",
            running.url,
            &id[1..]
        );
        ahjo_node_kit::hook::report(&[url, "{\"message\":\"ready\"}".into()]);
        until(cx, "hook marks the row and tab", |cx| {
            page.read_with(cx, |p, _| {
                p.fleet
                    .session(&key)
                    .and_then(|s| p.marked(s))
                    .is_some_and(|e| e.kind == kind)
                    && p.tab_marked(0)
            })
        });
        until(cx, "tone requested on the test platform", |_| {
            TONES
                .lock()
                .unwrap()
                .iter()
                .any(|line| line.contains(&format!("tone {event} not played")))
        });
        let args = ["mark-seen", "alpha", "--node", &running.url].map(str::to_owned);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        assert_eq!(
            runtime
                .block_on(ahjo_node_kit::cli::run("session", &args, &mut Vec::new()))
                .unwrap(),
            0
        );
        until(cx, "seen clears the row and tab", |cx| {
            page.read_with(cx, |p, _| {
                p.fleet.session(&key).is_some_and(|s| p.marked(s).is_none()) && !p.tab_marked(0)
            })
        });
    }
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

/// Showing a session is looking at it, whether or not a window manager says
/// the app's window is active: the test window never is, like a bare display.
#[gpui::test]
fn clicking_a_marked_session_clears_its_mark(cx: &mut TestAppContext) {
    let Some(running) = start("terminals-mark-click") else {
        return;
    };
    let (_shell, cx, settings) = open_with_alpha(&running, "mark-click", cx);
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    let key = Key {
        node: running.node.clone(),
        user: String::new(),
        name: "alpha".into(),
    };
    let id = page.read_with(cx, |p, _| p.fleet.session(&key).unwrap().id.clone());
    let url = format!(
        "{}/api/agent/event?session=%24{}&event=needs-input",
        running.url,
        &id[1..]
    );
    ahjo_node_kit::hook::report(&[url, "{\"message\":\"ready\"}".into()]);
    until(cx, "the report marks the row", |cx| {
        page.read_with(cx, |p, _| {
            p.fleet.session(&key).is_some_and(|s| p.marked(s).is_some())
        })
    });
    click(cx, format!("row-{}::alpha", running.node));
    until(cx, "showing the session clears its mark", |cx| {
        page.read_with(cx, |p, _| {
            p.fleet.session(&key).is_some_and(|s| p.marked(s).is_none())
        })
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

/// Alt+N with all eight windows of the tab holding a session opens the
/// launcher in a new tab and leaves those windows as they were.
#[gpui::test]
fn the_launcher_opens_a_new_tab_when_the_windows_are_full(cx: &mut TestAppContext) {
    let Some(running) = start("terminals-launch-full") else {
        return;
    };
    let (_shell, cx, settings) = open_with_alpha(&running, "launch-full", cx);
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    let full: Vec<Key> = (0..MAX_WINDOWS)
        .map(|i| Key {
            node: running.node.clone(),
            user: String::new(),
            name: format!("held-{i}"),
        })
        .collect();
    cx.update(|_, cx| {
        page.update(cx, |p, _| {
            let ws = &mut p.settings.workspace;
            ws.set_windows(0, MAX_WINDOWS);
            for (w, key) in full.iter().enumerate() {
                ws.bind_in(0, w, key.clone(), None);
            }
        })
    });
    cx.simulate_keystrokes("alt-n");
    page.read_with(cx, |p, _| {
        let ws = &p.settings.workspace;
        assert_eq!((ws.count, ws.active), (2, 1), "a new tab is shown");
        assert!(ws.windows()[ws.focus].is_empty(), "its window is free");
        assert!(
            ws.tabs[0].windows.iter().all(|w| !w.is_empty()),
            "the full tab is kept"
        );
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

/// Links emitted by a scratch tmux session reach the platform and the page
/// that answers paths, including a path wrapped by the terminal grid.
#[gpui::test]
fn terminal_links_open_url_and_wrapped_quoted_path(cx: &mut TestAppContext) {
    let Some(running) = start("terminals-link-journey") else {
        return;
    };
    let (_shell, cx, settings) = open_with_alpha(&running, "link-journey", cx);
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    let path = format!("/tmp/{}/leaf.txt", "long".repeat(55));
    let command =
        format!("printf '\\033[2J\\033[H\"{path}\"\\nhttps://example.test/safe\\n🙂X\\n'");
    tmux(&running, &["send-keys", "-t", "alpha", &command, "Enter"]);
    until(cx, "terminal links and emoji printed", |cx| {
        let text = shown(cx);
        text.lines()
            .any(|line| line.trim() == "https://example.test/safe")
            && text.lines().any(|line| line.trim() == "🙂 X")
    });
    let position = |cx: &mut VisualTestContext, needle: &str| {
        page.read_with(cx, |p, _| {
            let text = p.window_text(0).unwrap();
            let (row, line) = text
                .lines()
                .enumerate()
                .find(|(_, line)| line.contains(needle))
                .unwrap();
            let col = line.find(needle).unwrap();
            let geo = p.geo.borrow()[0];
            let origin = geo.origin.unwrap();
            let cell = geo.cell.unwrap();
            gpui::point(
                origin.x + cell.width * col as f32 + px(2.),
                origin.y + cell.height * row as f32 + px(2.),
            )
        })
    };
    let url = position(cx, "https://example.test/safe");
    cx.simulate_click(url, Modifiers::none());
    until(cx, "URL opened", |cx| {
        cx.opened_url().as_deref() == Some("https://example.test/safe")
    });
    ITEM_ASK.with(|slot| *slot.borrow_mut() = None);
    let path_pos = position(cx, "/tmp/");
    cx.simulate_click(path_pos, Modifiers::none());
    until(cx, "wrapped path opened", |_| {
        ITEM_ASK.with(|slot| matches!(&*slot.borrow(), Some(Ask::OpenPath { path: got, beside: true, .. }) if got == &path))
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

#[gpui::test]
fn a_picture_named_in_a_session_shows_small_and_opens_beside(cx: &mut TestAppContext) {
    let Some(running) = start_config("terminals-pictures", false, false, true, false) else {
        return;
    };
    let picture = running.fixture.join("plan.svg");
    std::fs::write(
        &picture,
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="#c33"/></svg>"##,
    )
    .unwrap();
    let (_shell, cx, settings) = open_with_alpha(&running, "pictures", cx);
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    let path = picture.display().to_string();
    let command = format!("printf '\\033[2J\\033[Hmade {path}\\n'");
    tmux(&running, &["send-keys", "-t", "alpha", &command, "Enter"]);
    until(cx, "the picture drawn small at its row", |cx| {
        page.read_with(cx, |p, _| !p.chips.borrow()[0].is_empty())
    });
    let chip = page.read_with(cx, |p, _| p.chips.borrow()[0][0].bounds);
    ITEM_ASK.with(|slot| *slot.borrow_mut() = None);
    cx.simulate_click(chip.center(), Modifiers::none());
    until(cx, "the picture opened beside", |_| {
        ITEM_ASK.with(|slot| matches!(&*slot.borrow(), Some(Ask::OpenPath { path: got, beside: true, .. }) if got == &path))
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

#[gpui::test]
fn a_mermaid_diagram_a_session_prints_draws_small_and_opens_beside(cx: &mut TestAppContext) {
    let Some(running) = start("terminals-diagram") else {
        return;
    };
    let (_shell, cx, settings) = open_with_alpha(&running, "diagram", cx);
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    // As Claude Code shows a mermaid block: no fences, the body indented.
    let command = "printf '\\033[2J\\033[Hflowchart LR\\n    A[Build] --> B[Test]\\n\\ndone\\n'";
    tmux(&running, &["send-keys", "-t", "alpha", command, "Enter"]);
    until(cx, "the diagram drawn small at its header", |cx| {
        page.read_with(cx, |p, _| !p.chips.borrow()[0].is_empty())
    });
    let chip = page.read_with(cx, |p, _| p.chips.borrow()[0][0].bounds);
    assert!(cx.debug_bounds("diagram-beside").is_none());
    cx.simulate_click(chip.center(), Modifiers::none());
    until(cx, "the diagram drawn beside", |cx| {
        cx.debug_bounds("diagram-beside").is_some()
    });
    // It opens in the window's panel, and Send hands it to a session as
    // its mermaid block.
    assert!(cx.debug_bounds("panel-right").is_some(), "in the panel");
    click(cx, "diagram-send".into());
    until(cx, "Send with the diagram", |cx| {
        page.read_with(cx, |p, _| {
            p.send.reference.as_deref().is_some_and(|r| {
                r.starts_with("```mermaid\n") && r.contains("A[Build] --> B[Test]")
            })
        })
    });
    // Back goes from Send to the diagram it was opened over.
    click(cx, "panel-back".into());
    until(cx, "Send closed, the diagram back", |cx| {
        cx.debug_bounds("send-drawer").is_none() && cx.debug_bounds("diagram-beside").is_some()
    });
    // Its chip, clicked again, takes it away.
    cx.simulate_click(chip.center(), Modifiers::none());
    until(cx, "the diagram closed by its chip", |cx| {
        cx.debug_bounds("diagram-beside").is_none()
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

#[gpui::test]
fn selection_copies_when_mouse_releases_outside_the_tile(cx: &mut TestAppContext) {
    let Some(running) = start("terminals-selection-journey") else {
        return;
    };
    let (_shell, cx, settings) = open_with_alpha(&running, "selection-journey", cx);
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    tmux(
        &running,
        &[
            "send-keys",
            "-t",
            "alpha",
            "printf '\\033[2J\\033[Hcopy-marker\\n'",
            "Enter",
        ],
    );
    until(cx, "selection text", |cx| {
        shown(cx).lines().any(|line| line.trim() == "copy-marker")
    });
    let (start, end) = page.read_with(cx, |p, _| {
        let text = p.window_text(0).unwrap();
        let (row, line) = text
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains("copy-marker"))
            .unwrap();
        let col = line.find("copy-marker").unwrap();
        let geo = p.geo.borrow()[0];
        let origin = geo.origin.unwrap();
        let cell = geo.cell.unwrap();
        (
            gpui::point(
                origin.x + cell.width * col as f32 + px(2.),
                origin.y + cell.height * row as f32 + px(2.),
            ),
            gpui::point(
                origin.x + cell.width * (col + 11) as f32 + px(2.),
                origin.y + cell.height * row as f32 + px(2.),
            ),
        )
    });
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(end, gpui::MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(
        gpui::point(px(0.), px(0.)),
        gpui::MouseButton::Left,
        Modifiers::none(),
    );
    until(cx, "selection copied", |cx| {
        cx.read_from_clipboard()
            .and_then(|c| c.text())
            .is_some_and(|s| s.contains("copy-marker"))
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

/// Arrange a real session using the same page actions as the tab menus and
/// drag another session from the rail into an empty window.
#[gpui::test]
fn arrange_tabs_view_drag_and_sizing_on_a_live_node(cx: &mut TestAppContext) {
    let Some(running) = start("terminals-arrange-journey") else {
        return;
    };
    let (_shell, cx, settings) = open_with_alpha(&running, "arrange-journey", cx);
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    let run_item = |cx: &mut VisualTestContext, tab: usize, label: &str| {
        let action = page.read_with(cx, |p, _| {
            p.tab_menu(tab)
                .into_iter()
                .find(|r| r.label == label)
                .unwrap()
                .act
                .unwrap()
        });
        cx.update(|window, cx| page.update(cx, |p, cx| p.act(action, window, cx)));
    };
    let two = page.read_with(cx, |p, _| {
        p.tab_menu(0)
            .into_iter()
            .find(|r| r.label == "Windows")
            .unwrap()
            .sub
            .into_iter()
            .find(|r| r.label == "2")
            .unwrap()
            .act
            .unwrap()
    });
    cx.update(|window, cx| page.update(cx, |p, cx| p.act(two, window, cx)));
    // The new window is empty and not focused: it says what it is for.
    until(cx, "second window", |cx| {
        cx.debug_bounds("empty-window-1").is_some()
    });
    let beta: &'static str = Box::leak(format!("row-{}::beta", running.node).into_boxed_str());
    until(cx, "beta row", |cx| cx.debug_bounds(beta).is_some());
    let from = cx.debug_bounds(beta).unwrap().center();
    let to = cx.debug_bounds("empty-window-1").unwrap().center();
    cx.simulate_mouse_down(from, gpui::MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        gpui::point(from.x + px(10.), from.y),
        gpui::MouseButton::Left,
        Modifiers::none(),
    );
    cx.simulate_mouse_move(to, gpui::MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(to, gpui::MouseButton::Left, Modifiers::none());
    until(cx, "beta bound by dragging", |cx| {
        page.read_with(cx, |p, _| {
            p.settings
                .workspace
                .shown(1)
                .is_some_and(|b| b.key.name == "beta")
        })
    });
    // Kept as view 1 from the tab menu, under a name typed over the one
    // its sessions gave it.
    let keep = page.read_with(cx, |p, _| {
        let menu = p.tab_menu(0);
        let views = &menu.iter().find(|r| r.label == "Views").unwrap().sub;
        let keep = &views
            .iter()
            .find(|r| r.label == "Keep this tab as view")
            .unwrap()
            .sub;
        keep[0].act.clone().unwrap()
    });
    cx.update(|window, cx| page.update(cx, |p, cx| p.act(keep, window, cx)));
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("Morning");
    cx.simulate_keystrokes("enter");
    until(cx, "view kept", |cx| {
        page.read_with(cx, |p, _| {
            p.settings
                .workspace
                .view(1)
                .is_some_and(|v| v.name == "Morning" && v.tab.count == 2)
        })
    });
    until(cx, "tab add control", |cx| {
        cx.debug_bounds("tab-add").is_some()
    });
    click(cx, "tab-add".into());
    until(cx, "second tab", |cx| {
        cx.debug_bounds("tab-sessions-1").is_some()
    });
    run_item(cx, 1, "Rename tab");
    cx.simulate_input("Ops");
    cx.simulate_keystrokes("enter");
    until(cx, "tab renamed", |cx| {
        page.read_with(cx, |p, _| {
            p.settings.workspace.tabs[1].name.as_deref() == Some("Ops")
        })
    });
    run_item(cx, 1, "Close tab");
    until(cx, "tab removed", |cx| {
        page.read_with(cx, |p, _| p.settings.workspace.shown_tabs().len() == 1)
    });
    // Down to one window, then the view lays the two out again.
    let sub_act = |cx: &mut VisualTestContext, menu: &str, label: &str| {
        let action = page.read_with(cx, |p, _| {
            p.tab_menu(0)
                .into_iter()
                .find(|r| r.label == menu)
                .unwrap()
                .sub
                .into_iter()
                .find(|r| r.label == label)
                .unwrap()
                .act
                .unwrap()
        });
        cx.update(|window, cx| page.update(cx, |p, cx| p.act(action, window, cx)));
    };
    sub_act(cx, "Windows", "1");
    until(cx, "one window", |cx| {
        page.read_with(cx, |p, _| p.settings.workspace.windows().len() == 1)
    });
    sub_act(cx, "Views", "1  Morning");
    until(cx, "the view restores beta", |cx| {
        page.read_with(cx, |p, _| {
            let ws = &p.settings.workspace;
            ws.windows().len() == 2 && ws.shown(1).is_some_and(|b| b.key.name == "beta")
        })
    });
    until(cx, "every shown window sizes its session", |cx| {
        page.read_with(cx, |p, _| {
            p.fleet
                .all
                .iter()
                .filter(|s| s.name == "alpha" || s.name == "beta")
                .all(|s| s.seat == Some(ahjo_terminals_contract::SizingPane::Mine))
        })
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

/// The offered recent folder is completed by Tab and used by the fake harness.
#[gpui::test]
fn launcher_completes_recent_folder_before_starting_agent(cx: &mut TestAppContext) {
    let Some(running) = start_with_restart_harness("terminals-folder-journey", true, true) else {
        return;
    };
    let settings = ahjo_testkit::fresh_dir("folder-journey-settings").join("app.json");
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let (url, path) = (running.url.clone(), settings.clone());
    let (_shell, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(&[terminals], Some(url), Some(path), window, cx)
    });
    until(cx, "fake harness offered", |cx| {
        cx.debug_bounds("harness-0-fake").is_some()
    });
    click(cx, "harness-0-fake".into());
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    cx.update(|window, cx| {
        page.update(cx, |p, cx| {
            p.launchers[0].update(cx, |l, cx| l.focus_first(window, cx))
        })
    });
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("flags-ui-");
    cx.simulate_keystrokes("tab");
    scroll_to_launch(cx, "launcher-scroll-0");
    until(cx, "launch available", |cx| {
        cx.debug_bounds("launch-0")
            .zip(cx.debug_bounds("launcher-scroll-0"))
            .is_some_and(|(button, viewport)| viewport.contains(&button.center()))
    });
    click(cx, "launch-0".into());
    let expected = format!("{}|--marker", running.fixture.display());
    until(cx, "agent started in the completed folder", |_| {
        std::fs::read_to_string(running.fixture.join("runs")).is_ok_and(|s| s.contains(&expected))
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

/// Send opened from alpha carries its identity when redirected to beta.
#[gpui::test]
fn terminal_send_carries_source_reference_to_another_session(cx: &mut TestAppContext) {
    let Some(running) = start("terminals-send-source") else {
        return;
    };
    let received = running.fixture.join("source-reference");
    tmux(
        &running,
        &[
            "send-keys",
            "-t",
            "beta",
            &format!("cat > '{}'", received.display()),
            "Enter",
        ],
    );
    let (_shell, cx, settings) = open_with_alpha(&running, "send-source", cx);
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    until(cx, "beta listed", |cx| {
        page.read_with(cx, |p, _| p.fleet.all.iter().any(|s| s.name == "beta"))
    });
    cx.simulate_keystrokes("alt-s");
    until(cx, "Send opened from terminal", |cx| {
        cx.debug_bounds("send-go").is_some()
    });
    let beta = format!("send-target-{}::beta", running.node);
    let selector: &'static str = Box::leak(beta.clone().into_boxed_str());
    until(cx, "beta target", |cx| cx.debug_bounds(selector).is_some());
    click(cx, beta);
    click(cx, "send-go".into());
    // Named by its name and where it runs, never by the node's id.
    until(cx, "source reference delivered to beta", |_| {
        std::fs::read_to_string(&received)
            .is_ok_and(|s| s.contains("session alpha on ") && !s.contains("::alpha"))
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

/// A window's Send hands its session over to another session picked
/// instead of it: the line naming it goes there.
#[gpui::test]
fn window_send_hands_its_session_to_another_picked_instead(cx: &mut TestAppContext) {
    let Some(running) = start("window-send-source") else {
        return;
    };
    let received = running.fixture.join("window-source");
    tmux(
        &running,
        &[
            "send-keys",
            "-t",
            "beta",
            &format!("cat > '{}'", received.display()),
            "Enter",
        ],
    );
    let (_shell, cx, settings) = open_with_alpha(&running, "window-send-source", cx);
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    until(cx, "beta is listed", |cx| {
        page.read_with(cx, |p, _| p.fleet.all.iter().any(|s| s.name == "beta"))
    });
    click(cx, "window-send-0".into());
    until(cx, "Send aimed at alpha", |cx| {
        cx.debug_bounds("send-aimed-alpha").is_some()
    });
    let target = format!("send-target-{}::beta", running.node);
    click(cx, target);
    until(cx, "Send aimed at beta instead", |cx| {
        cx.debug_bounds("send-aimed-beta").is_some()
    });
    assert!(
        cx.debug_bounds("send-reference").is_some(),
        "the line naming alpha goes to beta"
    );
    click(cx, "send-go".into());
    until(cx, "source reference reaches beta", |_| {
        std::fs::read_to_string(&received).is_ok_and(|s| s.contains("session alpha on "))
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

/// What is on alpha's screen now, read from tmux.
fn alpha_screen(running: &Running) -> String {
    tmux(running, &["capture-pane", "-p", "-t", "alpha"])
}

/// Send opened from a session (its window's Send, Alt+S in its terminal)
/// is aimed at that session, lists it with the others, and sends into its
/// pane: the session it came from never has to be picked.
#[gpui::test]
fn send_opened_from_a_session_is_aimed_at_it_and_delivers_there(cx: &mut TestAppContext) {
    let Some(running) = start("send-from-session") else {
        return;
    };
    let (_shell, cx, settings) = open_with_alpha(&running, "send-from-session", cx);
    let alpha: &'static str =
        Box::leak(format!("send-target-{}::alpha", running.node).into_boxed_str());
    click(cx, "window-send-0".into());
    until(cx, "Send aimed at alpha", |cx| {
        cx.debug_bounds("send-aimed-alpha").is_some()
    });
    assert!(cx.debug_bounds(alpha).is_some(), "alpha is listed");
    assert!(
        cx.debug_bounds("send-reference").is_none(),
        "alpha is not sent its own name"
    );
    cx.simulate_input("echo window-sent-$((6*7))");
    cx.simulate_keystrokes("enter");
    until(cx, "the message run in alpha's pane", |_| {
        alpha_screen(&running).contains("window-sent-42")
    });
    until(cx, "the drawer to close once sent", |cx| {
        cx.debug_bounds("send-go").is_none()
    });
    // Alt+S in alpha's terminal opens it the same way.
    cx.simulate_keystrokes("alt-s");
    until(cx, "Send aimed at alpha again", |cx| {
        cx.debug_bounds("send-aimed-alpha").is_some()
    });
    cx.simulate_input("echo chord-sent-$((6*7))");
    cx.simulate_keystrokes("enter");
    until(cx, "the second message run in alpha's pane", |_| {
        alpha_screen(&running).contains("chord-sent-42")
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

/// A point in the Send drawer where nothing but the drawer is: below what
/// its body holds.
fn send_blank(cx: &mut VisualTestContext) -> gpui::Point<gpui::Pixels> {
    let body = ahjo_testkit::app::held_still(cx, "send-body");
    gpui::point(body.center().x, body.bottom() - px(24.))
}

/// Send lies beside the windows and never traps the keyboard: a click on a
/// terminal gives it every key, Escape too, while Send stays open with its
/// draft; a click back in Send goes on with the draft.
#[gpui::test]
fn a_terminal_clicked_beside_send_takes_the_keys_and_send_keeps_its_draft(cx: &mut TestAppContext) {
    let Some(running) = start("send-beside") else {
        return;
    };
    let (_shell, cx, settings) = open_with_alpha(&running, "send-beside", cx);
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    let note =
        |cx: &mut VisualTestContext| page.read_with(cx, |p, cx| p.send_note.read(cx).text.clone());
    click(cx, "window-send-0".into());
    until(cx, "the Send drawer", |cx| {
        cx.debug_bounds("send-go").is_some()
    });
    cx.update(|window, cx| {
        page.update(cx, |p, cx| {
            ahjo_app_kit::widgets::field::focus(&p.send_note, window, cx)
        })
    });
    cx.simulate_input("half a thought");
    assert_eq!(note(cx), "half a thought");
    let body = page
        .read_with(cx, |p, _| p.geo.borrow()[0].body)
        .expect("alpha's window");
    cx.simulate_click(body.center(), Modifiers::none());
    cx.simulate_input("echo beside-$((6*7))");
    cx.simulate_keystrokes("enter");
    until(cx, "what was typed run in alpha", |_| {
        alpha_screen(&running).contains("beside-42")
    });
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("send-go").is_some(),
        "Escape in the terminal is the program's: Send stays open"
    );
    assert_eq!(note(cx), "half a thought", "the draft is kept");
    let blank = send_blank(cx);
    cx.simulate_click(blank, Modifiers::none());
    cx.simulate_input(", finished");
    assert_eq!(note(cx), "half a thought, finished");
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

/// Files dropped on Send are attached: one from this computer (the
/// system's drop) is copied to the target's node, one dragged from the
/// Files page goes as its path there; both paths follow into the session.
#[gpui::test]
fn files_dropped_on_send_are_attached_and_reach_the_session(cx: &mut TestAppContext) {
    let Some(running) = start_config("send-drops", false, false, true, false) else {
        return;
    };
    let received = running.fixture.join("received");
    let on_node = running.fixture.join("on-node.txt");
    std::fs::write(&on_node, "on the node").unwrap();
    let computer = std::env::temp_dir().join(format!("ahjo-send-drops-{}", std::process::id()));
    std::fs::create_dir_all(&computer).unwrap();
    let local = computer.join("from-computer.txt");
    std::fs::write(&local, "from the computer").unwrap();
    tmux(
        &running,
        &[
            "send-keys",
            "-t",
            "alpha",
            &format!("cat > '{}'", received.display()),
            "Enter",
        ],
    );
    let (_shell, cx, settings) = open_pages_with_alpha(
        &running,
        "send-drops",
        &[terminals, toy, ahjo_files_app::build],
        cx,
    );
    cx.simulate_resize(gpui::size(px(1440.), px(900.)));
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    let attached = |cx: &mut VisualTestContext| page.read_with(cx, |p, _| p.send.files.len());
    click(cx, "window-send-0".into());
    until(cx, "the Send drawer", |cx| {
        cx.debug_bounds("send-go").is_some()
    });
    let at = send_blank(cx);
    cx.simulate_event(gpui::FileDropEvent::Entered {
        position: at,
        paths: gpui::ExternalPaths(std::iter::once(local.clone()).collect()),
    });
    cx.simulate_event(gpui::FileDropEvent::Submit { position: at });
    until(cx, "the computer's file attached", |cx| attached(cx) == 1);
    // One panel: Send stays beside the Files page, whose file is dragged.
    let files = _shell.read_with(cx, |s, _| Host::new(s.hub()).for_page("files", &[]));
    cx.update(|_, cx| files.show(cx));
    until(cx, "the node's file on the Files page", |cx| {
        cx.debug_bounds("file-row-on-node.txt").is_some()
    });
    let from = cx.debug_bounds("file-row-on-node.txt").unwrap().center();
    let at = send_blank(cx);
    cx.simulate_mouse_down(from, gpui::MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        gpui::point(from.x - px(40.), from.y + px(10.)),
        gpui::MouseButton::Left,
        Modifiers::none(),
    );
    cx.simulate_mouse_move(at, gpui::MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(at, gpui::MouseButton::Left, Modifiers::none());
    until(cx, "the node's file attached", |cx| attached(cx) == 2);
    click(cx, "send-go".into());
    let on_node_line = format!("- {}", on_node.display());
    let mut copied = std::path::PathBuf::new();
    until(cx, "both paths in the session", |_| {
        let Ok(text) = std::fs::read_to_string(&received) else {
            return false;
        };
        let Some(path) = text
            .lines()
            .filter_map(|line| line.strip_prefix("- "))
            .find(|p| {
                p.ends_with("from-computer.txt") && !p.starts_with(&*computer.to_string_lossy())
            })
        else {
            return false;
        };
        copied = path.into();
        text.lines().any(|line| line == on_node_line)
    });
    assert_eq!(
        std::fs::read_to_string(&copied).unwrap(),
        "from the computer",
        "the computer's file copied to the node"
    );
    let _ = std::fs::remove_dir_all(computer);
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

#[gpui::test]
fn absent_tab_chord_does_not_reach_the_terminal(cx: &mut TestAppContext) {
    let Some(running) = start("absent-tab-key") else {
        return;
    };
    let (shell, cx, settings) = open_with_alpha(&running, "absent-tab-key", cx);
    let received = running.fixture.join("key-byte");
    tmux(
        &running,
        &[
            "send-keys",
            "-t",
            "alpha",
            &format!(
                "stty raw -echo; dd bs=1 count=1 of='{}' 2>/dev/null; stty sane",
                received.display()
            ),
            "Enter",
        ],
    );
    until(cx, "terminal waiting for one byte", |_| {
        tmux(
            &running,
            &[
                "display-message",
                "-p",
                "-t",
                "alpha",
                "#{pane_current_command}",
            ],
        )
        .trim()
            == "dd"
    });
    cx.simulate_keystrokes("alt-2");
    cx.simulate_input("x");
    until(cx, "ordinary input reaches terminal", |_| {
        std::fs::read(&received).is_ok_and(|b| b == b"x")
    });
    until(cx, "shell restored", |_| {
        tmux(
            &running,
            &[
                "display-message",
                "-p",
                "-t",
                "alpha",
                "#{pane_current_command}",
            ],
        )
        .trim()
            == "sh"
    });
    let raw = running.fixture.join("keys-off-byte");
    tmux(
        &running,
        &[
            "send-keys",
            "-t",
            "alpha",
            &format!(
                "stty raw -echo; dd bs=1 count=1 of='{}' 2>/dev/null; stty sane",
                raw.display()
            ),
            "Enter",
        ],
    );
    until(cx, "terminal waiting again", |_| {
        tmux(
            &running,
            &[
                "display-message",
                "-p",
                "-t",
                "alpha",
                "#{pane_current_command}",
            ],
        )
        .trim()
            == "dd"
    });
    cx.simulate_keystrokes("ctrl-shift-space k");
    until(cx, "keys off", |cx| {
        shell.read_with(cx, |s, cx| !s.hub().read(cx).settings.keys)
    });
    cx.simulate_keystrokes("alt-2");
    until(cx, "keys off passes Alt to terminal", |_| {
        std::fs::read(&raw).is_ok_and(|b| b == [27])
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

#[gpui::test]
fn search_shows_and_marks_a_wrapped_history_match(cx: &mut TestAppContext) {
    let Some(running) = start("search-marked-history") else {
        return;
    };
    let (_shell, cx, settings) = open_with_alpha(&running, "search-marked-history", cx);
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    tmux(
        &running,
        &[
            "send-keys",
            "-t",
            "alpha",
            "printf '%0300d\\n' 0; printf '%0300dmatch%s\\n' 0 'needle'",
            "Enter",
        ],
    );
    until(cx, "output reached the terminal", |cx| {
        shown(cx).contains("matchneedle")
    });
    cx.simulate_resize(gpui::size(px(1440.), px(900.)));
    cx.simulate_keystrokes("alt-h");
    until(cx, "search drawer opened over the windows", |cx| {
        cx.debug_bounds("find-drawer").is_some()
    });
    cx.simulate_input("matchneedle");
    until(cx, "search result", |cx| {
        cx.debug_bounds("find-match-0").is_some()
    });
    click(cx, "find-match-0".into());
    until(cx, "matched line selected in the terminal", |cx| {
        page.read_with(cx, |p, _| {
            p.histories
                .get(&p.settings.workspace.focus)
                .is_some_and(|h| {
                    p.window_text(p.settings.workspace.focus)
                        .is_some_and(|text| text.contains("matchneedle"))
                        && h.screen
                            .lock()
                            .term
                            .selection_to_string()
                            .is_some_and(|text| text.contains("matchneedle"))
                })
        })
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

/// Reading back is live: what arrives goes on below the lines being read,
/// which stay where they are, and the input stays docked below them.
#[gpui::test]
fn typing_while_reading_history_reaches_the_program_and_keeps_the_history(cx: &mut TestAppContext) {
    let Some(running) = start("history-dock") else {
        return;
    };
    let (_shell, cx, settings) = open_with_alpha(&running, "history-dock", cx);
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    cx.simulate_input("seq 1 300");
    cx.simulate_keystrokes("enter");
    until(cx, "output reached the terminal", |cx| {
        shown(cx).contains("300")
    });
    cx.simulate_keystrokes("shift-pageup");
    until(cx, "the history open, the input docked below it", |cx| {
        page.read_with(cx, |p, _| {
            p.histories.contains_key(&0) && p.geo.borrow()[0].dock.is_some()
        })
    });
    // The lines being read: the view's top, which the prompt being typed
    // on does not reach.
    let read = |cx: &mut VisualTestContext| {
        page.read_with(cx, |p, _| {
            let text = p.window_text(0).unwrap();
            text.lines().take(5).collect::<Vec<_>>().join("\n")
        })
    };
    let before = read(cx);
    cx.simulate_input("echo docked-$((6*7))");
    cx.simulate_keystrokes("enter");
    until(cx, "the program ran what was typed", |_| {
        tmux(&running, &["capture-pane", "-p", "-t", "alpha"]).contains("docked-42")
    });
    until(cx, "the output arrived below the history, live", |cx| {
        page.read_with(cx, |p, _| {
            p.histories
                .get(&0)
                .is_some_and(|h| h.screen.lock().text().contains("docked-42"))
        })
    });
    assert_eq!(read(cx), before, "the lines being read moved");
    cx.simulate_keystrokes("shift-end");
    until(cx, "back at the live screen", |cx| {
        page.read_with(cx, |p, _| p.histories.is_empty()) && shown(cx).contains("docked-42")
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

/// Send stands in the panel at the right of the windows, which make room
/// for it: it covers none of a lone window, and the window has its width
/// back once it closes.
#[gpui::test]
fn send_makes_room_beside_a_lone_window(cx: &mut TestAppContext) {
    let Some(running) = start("terminals-send-layout") else {
        return;
    };
    let (_shell, cx, settings) = open_with_alpha(&running, "send-layout", cx);
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    cx.simulate_resize(gpui::size(px(1440.), px(900.)));
    let body = |cx: &mut VisualTestContext| page.read_with(cx, |p, _| p.geo.borrow()[0].body);
    until(cx, "terminal geometry", |cx| body(cx).is_some());
    let whole = body(cx).unwrap().size.width;
    cx.simulate_keystrokes("alt-s");
    until(cx, "the window narrowed left of the drawer", |cx| {
        let Some(drawer) = cx.debug_bounds("send-drawer") else {
            return false;
        };
        body(cx)
            .is_some_and(|b| b.origin.x + b.size.width <= drawer.origin.x && b.size.width < whole)
    });
    assert!(!shown(cx).trim().is_empty());
    cx.simulate_keystrokes("escape");
    until(cx, "the window's width back", |cx| {
        cx.debug_bounds("send-drawer").is_none() && body(cx).is_some_and(|b| b.size.width == whole)
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

/// The Files panel stands at the right of the windows, which give it room.
/// A focused terminal keeps Ctrl+B (tmux's prefix, an agent's background
/// key), so it stays; Alt+E puts it away and the window widens, and Ctrl+B
/// where nothing takes it brings it back.
#[gpui::test]
fn the_files_panel_goes_and_comes_with_its_keys_and_a_terminal_keeps_ctrl_b(
    cx: &mut TestAppContext,
) {
    let Some(running) = start("terminals-files-panel") else {
        return;
    };
    let (shell, cx, settings) = open_pages_with_alpha(
        &running,
        "files-panel",
        &[terminals, toy, ahjo_files_app::build],
        cx,
    );
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    cx.simulate_resize(gpui::size(px(1440.), px(900.)));
    let body = |cx: &mut VisualTestContext| page.read_with(cx, |p, _| p.geo.borrow()[0].body);
    until(cx, "terminal geometry", |cx| body(cx).is_some());
    cx.simulate_keystrokes("alt-e");
    until(cx, "the panel at the windows' right", |cx| {
        let Some(panel) = cx.debug_bounds("panel-right") else {
            return false;
        };
        body(cx).is_some_and(|b| b.origin.x + b.size.width <= panel.origin.x)
    });
    let beside = body(cx).unwrap().size.width;
    let window = cx.debug_bounds("window-0").expect("the window");
    cx.simulate_click(
        gpui::point(window.left() + px(20.), window.bottom() - px(20.)),
        Modifiers::none(),
    );
    cx.run_until_parked();
    assert!(
        cx.update(|window, cx| page.read(cx).term_focus.is_focused(window)),
        "alpha's terminal has the keyboard"
    );
    cx.simulate_keystrokes("ctrl-b");
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("panel-right").is_some(),
        "the terminal took Ctrl+B"
    );
    cx.simulate_keystrokes("alt-e");
    until(cx, "the panel gone and the window wider", |cx| {
        cx.debug_bounds("panel-right").is_none() && body(cx).is_some_and(|b| b.size.width > beside)
    });
    // Where nothing takes it, Ctrl+B is the app's.
    cx.update(|window, cx| {
        let focus = gpui::Focusable::focus_handle(shell.read(cx), cx);
        window.focus(&focus, cx);
    });
    cx.simulate_keystrokes("ctrl-b");
    until(cx, "the panel back", |cx| {
        cx.debug_bounds("panel-right").is_some()
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

/// One message to several sessions: each marked in the finder (a click on
/// its box, or Tab), named under To, and sent to all with one Send; the
/// drawer closes once every one has it.
#[gpui::test]
fn send_goes_to_every_session_marked(cx: &mut TestAppContext) {
    let Some(running) = start("terminals-send-several") else {
        return;
    };
    let (shell, cx, settings) = open_with_alpha(&running, "send-several", cx);
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    until(cx, "beta listed", |cx| {
        page.read_with(cx, |p, _| p.fleet.all.iter().any(|s| s.name == "beta"))
    });
    let host = shell.read_with(cx, |s, _| Host::new(s.hub()).for_page("toy", &[]));
    cx.update(|_, cx| {
        host.ask(
            Ask::Send {
                reference: "echo several-$((40+2))".into(),
                target: None,
            },
            cx,
        )
    })
    .unwrap();
    until(cx, "the Send drawer", |cx| {
        cx.debug_bounds("send-go").is_some()
    });
    // Aimed at the focused window's alpha; in the finder, typing finds
    // beta and Tab marks it too.
    let aimed = |cx: &mut VisualTestContext| -> Vec<String> {
        page.read_with(cx, |p, _| {
            p.send_targets.iter().map(|k| k.name.clone()).collect()
        })
    };
    until(cx, "aimed at alpha", |cx| aimed(cx) == ["alpha"]);
    // The note has the keyboard; Tab goes to the finder.
    cx.simulate_keystrokes("tab");
    cx.simulate_input("bta");
    cx.simulate_keystrokes("tab");
    until(cx, "beta marked", |cx| aimed(cx) == ["alpha", "beta"]);
    // With the words gone every session shows again; a click on a row's
    // box unmarks it, and marks it again.
    cx.simulate_keystrokes("ctrl-a backspace");
    let alpha = format!("send-target-mark-{}::alpha", running.node);
    click(cx, alpha.clone());
    until(cx, "alpha unmarked", |cx| aimed(cx) == ["beta"]);
    click(cx, alpha);
    until(cx, "both named under To", |cx| {
        cx.debug_bounds("send-aimed-beta,alpha").is_some()
    });
    click(cx, "send-go".into());
    for session in ["alpha", "beta"] {
        until(cx, &format!("{session} ran it"), |_| {
            tmux(&running, &["capture-pane", "-p", "-t", session])
                .lines()
                .any(|l| l.trim() == "several-42")
        });
    }
    until(cx, "the drawer closed", |cx| {
        cx.debug_bounds("send-drawer").is_none()
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

/// A tab rename open over Peek keeps what was typed when Attach in Peek
/// closes both, as it would on blur (ahjo-dee); Esc still drops it.
#[gpui::test]
fn a_tab_rename_over_peek_is_kept_when_peek_attaches(cx: &mut TestAppContext) {
    let Some(running) = start("terminals-rename-peek") else {
        return;
    };
    let (_shell, cx, settings) = open_with_alpha(&running, "rename-peek", cx);
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    let row = format!("row-{}::beta", running.node);
    let leaked: &'static str = Box::leak(row.clone().into_boxed_str());
    until(cx, "beta in the rail", |cx| {
        cx.debug_bounds(leaked).is_some()
    });
    let rename = |cx: &mut VisualTestContext, typed: &str| {
        click(cx, row.clone());
        until(cx, "the peek", |cx| {
            cx.debug_bounds("peek-attach").is_some()
        });
        let act = page.read_with(cx, |p, _| {
            p.tab_menu(0)
                .into_iter()
                .find(|r| r.label == "Rename tab")
                .unwrap()
                .act
                .unwrap()
        });
        cx.update(|window, cx| page.update(cx, |p, cx| p.act(act, window, cx)));
        cx.run_until_parked();
        cx.simulate_keystrokes("ctrl-a");
        cx.simulate_input(typed);
    };
    let name = |cx: &mut VisualTestContext| {
        page.read_with(cx, |p, _| p.settings.workspace.tabs[0].name.clone())
    };
    // Esc closes the rename first, keeping the old name.
    rename(cx, "Dropped");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(name(cx), None, "Esc keeps the old name");
    click(cx, "peek-attach".into());
    until(cx, "peek closed", |cx| {
        cx.debug_bounds("peek-attach").is_none()
    });
    assert_eq!(name(cx), None);
    // Attach with the rename open over Peek keeps what was typed.
    rename(cx, "Ops");
    click(cx, "peek-attach".into());
    until(cx, "peek closed", |cx| {
        cx.debug_bounds("peek-attach").is_none()
    });
    assert_eq!(name(cx).as_deref(), Some("Ops"), "the typed name is kept");
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

/// Send asked from what shows in the panel (the Brain, a file) opens over
/// it, and gives it the keyboard back when it closes, sent or gone back
/// from, not to the terminal (ahjo-4y1); Escape puts the panel away and
/// gives the terminal the keyboard.
#[gpui::test]
fn send_from_the_panel_gives_the_keyboard_back_to_what_it_opened_over(cx: &mut TestAppContext) {
    let Some(running) = start("terminals-send-beside") else {
        return;
    };
    let (shell, cx, settings) = open_with_alpha(&running, "send-beside", cx);
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    let toy = shell.read_with(cx, |s, _| Host::new(s.hub()).for_page("toy", &[]));
    let note = cx.update(|_, cx| cx.new(|cx| Note(cx.focus_handle())));
    cx.update(|_, cx| toy.show_panel(Panel::new("note", note.clone()), cx));
    cx.run_until_parked();
    let note_focused =
        |cx: &mut VisualTestContext| cx.update(|window, cx| note.read(cx).0.is_focused(window));
    let ask = |cx: &mut VisualTestContext, reference: &str| {
        cx.update(|window, cx| window.focus(&note.read(cx).0.clone(), cx));
        cx.run_until_parked();
        assert!(note_focused(cx), "the view beside has the keyboard");
        cx.update(|_, cx| {
            toy.ask(
                Ask::Send {
                    reference: reference.into(),
                    target: None,
                },
                cx,
            )
        })
        .unwrap();
        until(cx, "the Send drawer", |cx| {
            cx.debug_bounds("send-go").is_some()
        });
    };
    // Gone back from.
    ask(cx, "echo beside-back");
    click(cx, "panel-back".into());
    until(cx, "Send closed", |cx| {
        cx.debug_bounds("send-drawer").is_none()
    });
    assert!(cx.debug_bounds("toy-note").is_some(), "the view back");
    assert!(note_focused(cx), "Back gives the view the keyboard back");
    // Sent: the drawer closes once it arrived.
    ask(cx, "echo beside-$((40+2))");
    click(cx, "send-go".into());
    until(cx, "alpha ran it", |_| {
        tmux(&running, &["capture-pane", "-p", "-t", "alpha"])
            .lines()
            .any(|l| l.trim() == "beside-42")
    });
    until(cx, "the drawer closed", |cx| {
        cx.debug_bounds("send-drawer").is_none()
    });
    assert!(
        note_focused(cx),
        "a send gives the view beside the keyboard back"
    );
    // Escape puts the panel away; the terminal takes the keyboard.
    ask(cx, "echo beside-esc");
    cx.simulate_keystrokes("escape");
    until(cx, "the panel away", |cx| {
        cx.debug_bounds("panel-right").is_none()
    });
    assert!(cx.update(|window, cx| page.read(cx).term_focus.is_focused(window)));
    // With the view gone, Send alone; Escape gives the terminal the keys.
    cx.update(|_, cx| toy.close_panel("note", cx));
    cx.run_until_parked();
    cx.update(|_, cx| {
        toy.ask(
            Ask::Send {
                reference: "echo after".into(),
                target: None,
            },
            cx,
        )
    })
    .unwrap();
    until(cx, "the Send drawer", |cx| {
        cx.debug_bounds("send-go").is_some()
    });
    cx.simulate_keystrokes("escape");
    until(cx, "the drawer closed", |cx| {
        cx.debug_bounds("send-drawer").is_none()
    });
    assert!(cx.update(|window, cx| page.read(cx).term_focus.is_focused(window)));
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

/// A shell would run the line naming a session as a command, so it is left
/// out for one, said so, and Send anyway sends it all the same.
#[gpui::test]
fn send_anyway_gives_a_shell_the_line_naming_a_session(cx: &mut TestAppContext) {
    let Some(running) = start("terminals-send-anyway") else {
        return;
    };
    let (_shell, cx, settings) = open_with_alpha(&running, "send-anyway", cx);
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    until(cx, "beta listed", |cx| {
        page.read_with(cx, |p, _| p.fleet.all.iter().any(|s| s.name == "beta"))
    });
    cx.simulate_keystrokes("alt-s");
    let beta = format!("send-target-{}::beta", running.node);
    let row: &'static str = Box::leak(beta.clone().into_boxed_str());
    until(cx, "beta offered", |cx| cx.debug_bounds(row).is_some());
    click(cx, beta);
    until(cx, "left out for the shell, with Send anyway", |cx| {
        cx.debug_bounds("send-anyway").is_some()
    });
    click(cx, "send-anyway".into());
    until(cx, "and the way back", |cx| {
        cx.debug_bounds("send-leave-out").is_some()
    });
    click(cx, "send-go".into());
    until(cx, "the shell got the line naming alpha", |_| {
        tmux(&running, &["capture-pane", "-p", "-t", "beta"]).contains("session alpha on ")
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

/// The keyboard reaches Send anyway and Leave it out: Tab from the message
/// goes to the sessions, so Alt+Enter is their key, from the message too.
#[gpui::test]
fn alt_enter_sends_anyway_from_the_message(cx: &mut TestAppContext) {
    let Some(running) = start("terminals-send-anyway-key") else {
        return;
    };
    let (_shell, cx, settings) = open_with_alpha(&running, "send-anyway-key", cx);
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    until(cx, "beta listed", |cx| {
        page.read_with(cx, |p, _| p.fleet.all.iter().any(|s| s.name == "beta"))
    });
    cx.simulate_keystrokes("alt-s");
    let beta = format!("send-target-{}::beta", running.node);
    let row: &'static str = Box::leak(beta.clone().into_boxed_str());
    until(cx, "beta offered", |cx| cx.debug_bounds(row).is_some());
    click(cx, beta);
    until(cx, "left out for the shell", |cx| {
        cx.debug_bounds("send-anyway").is_some()
    });
    let typing = |cx: &mut VisualTestContext| {
        page.update_in(cx, |p, window, cx| p.send_note.read(cx).is_focused(window))
    };
    assert!(typing(cx), "the message has the keyboard");
    cx.simulate_keystrokes("alt-enter");
    until(cx, "sent anyway, with the way back", |cx| {
        cx.debug_bounds("send-leave-out").is_some()
    });
    cx.simulate_keystrokes("alt-enter");
    until(cx, "left out again", |cx| {
        cx.debug_bounds("send-anyway").is_some()
    });
    cx.simulate_keystrokes("alt-enter");
    until(cx, "and anyway once more", |cx| {
        cx.debug_bounds("send-leave-out").is_some()
    });
    assert!(typing(cx), "the keyboard stays in the message");
    cx.simulate_keystrokes("enter");
    until(cx, "the shell got the line naming alpha", |_| {
        tmux(&running, &["capture-pane", "-p", "-t", "beta"]).contains("session alpha on ")
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

/// A second node following the same scratch server is not a sizing viewer.
#[gpui::test]
fn opening_a_session_sizes_it_with_another_node_following(cx: &mut TestAppContext) {
    let Some(running) = start("terminals-second-node") else {
        return;
    };
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let config = format!(
        "[terminals]\nsocket = {:?}\nlaunch = {:?}\n",
        running.socket,
        running.fixture.join("no-launch.conf")
    );
    let second = runtime.block_on(Node::start(AREAS, &config)).unwrap();
    runtime.block_on(ahjo_testkit::tmux::eventually(
        "second node follows alpha",
        || {
            second
                .sessions()
                .list()
                .iter()
                .find(|s| s.name == "alpha" && !s.panes.is_empty())
                .map(|_| ())
        },
    ));
    let second_sessions = second.sessions();
    runtime.block_on(ahjo_testkit::tmux::eventually(
        "second node attaches only to alpha",
        || {
            let sessions = second_sessions.list();
            let pane = sessions
                .iter()
                .find(|s| s.name == "alpha")?
                .panes
                .first()?
                .0;
            second_sessions.screen(pane).map(|_| ())
        },
    ));
    let (_shell, cx, settings) = open_with_alpha(&running, "second-node", cx);
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    until(cx, "session claims the tile at its grid", |cx| {
        page.read_with(cx, |p, _| {
            let Some(binding) = p.settings.workspace.shown(0) else {
                return false;
            };
            let Some(session) = p.fleet.session(&binding.key) else {
                return false;
            };
            let Some(grid) = p.launch_grids.borrow()[0] else {
                return false;
            };
            session.foreign == Some(0)
                && session.seat == Some(ahjo_terminals_contract::SizingPane::Mine)
                && tmux(
                    &running,
                    &[
                        "display-message",
                        "-p",
                        "-t",
                        "alpha",
                        "#{window_width}x#{window_height}",
                    ],
                ) == format!("{}x{}", grid.0, grid.1)
        })
    });
    runtime.block_on(second.stop());
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

/// Completion reads the launch user's filesystem even outside Files roots.
#[gpui::test]
fn launcher_completes_a_folder_outside_files_roots(cx: &mut TestAppContext) {
    let Some(running) = start_with_restart_harness("terminals-folder-outside", true, true) else {
        return;
    };
    let chosen = running.fixture.join("subfolder");
    std::fs::create_dir(&chosen).unwrap();
    let settings = ahjo_testkit::fresh_dir("folder-outside-settings").join("app.json");
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let (url, path) = (running.url.clone(), settings.clone());
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(&[terminals], Some(url), Some(path), window, cx)
    });
    until(cx, "fake harness offered", |cx| {
        cx.debug_bounds("harness-0-fake").is_some()
    });
    click(cx, "harness-0-fake".into());
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    cx.update(|window, cx| {
        page.update(cx, |p, cx| {
            p.launchers[0].update(cx, |l, cx| l.focus_first(window, cx))
        })
    });
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input(&format!("{}/sub", running.fixture.display()));
    let row: &'static str =
        Box::leak(format!("folder-row-0-{}", chosen.display()).into_boxed_str());
    until(cx, "folder completion", |cx| cx.debug_bounds(row).is_some());
    until(cx, "button names the typed folder", |cx| {
        cx.debug_bounds("launch-label-0-Launch fake in sub")
            .is_some()
    });
    cx.simulate_keystrokes("tab");
    until(cx, "button names the completed folder", |cx| {
        cx.debug_bounds("launch-label-0-Launch fake in subfolder")
            .is_some()
    });
    // With nothing left to complete, Tab goes on to the flags.
    cx.simulate_keystrokes("tab end");
    cx.simulate_input(" --tabbed");
    until(cx, "typed into the flags", |cx| {
        cx.debug_bounds("flag-line-0---marker --tabbed").is_some()
    });
    scroll_to_launch(cx, "launcher-scroll-0");
    until(cx, "launch available", |cx| {
        cx.debug_bounds("launch-0")
            .zip(cx.debug_bounds("launcher-scroll-0"))
            .is_some_and(|(button, viewport)| viewport.contains(&button.center()))
    });
    click(cx, "launch-0".into());
    let expected = format!("{}|--marker --tabbed", chosen.display());
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        cx.run_until_parked();
        let runs = std::fs::read_to_string(running.fixture.join("runs")).unwrap_or_default();
        if runs.lines().any(|line| {
            line.split_once('|').is_some_and(|(folder, flags)| {
                std::path::Path::new(folder).canonicalize().ok() == chosen.canonicalize().ok()
                    && flags == "--marker --tabbed"
            })
        }) {
            break;
        }
        let status = shell.read_with(cx, |s, cx| {
            s.hub().read(cx).status.as_ref().map(|s| s.text.clone())
        });
        assert!(
            Instant::now() < deadline,
            "expected {expected:?}, runs {runs:?}, status {status:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    // A folder that started a session is offered next time.
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    until(cx, "the folder joins the recent ones", |cx| {
        page.read_with(cx, |p, _| {
            p.settings
                .launched_folders
                .values()
                .flatten()
                .any(|f| f.contains("subfolder"))
        })
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

/// A launch the node refused leaves its folder out of the recent ones, so
/// the next launch does not start from a folder that is not there.
#[gpui::test]
fn a_failed_launch_is_not_offered_again(cx: &mut TestAppContext) {
    let Some(running) = start_with_restart_harness("terminals-failed-folder", true, true) else {
        return;
    };
    let settings = ahjo_testkit::fresh_dir("failed-folder-settings").join("app.json");
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let (url, path) = (running.url.clone(), settings.clone());
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(&[terminals], Some(url), Some(path), window, cx)
    });
    until(cx, "fake harness offered", |cx| {
        cx.debug_bounds("harness-0-fake").is_some()
    });
    let missing = running.fixture.join("not-there");
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    cx.update(|window, cx| {
        page.update(cx, |p, cx| {
            p.launchers[0].update(cx, |l, cx| l.focus_first(window, cx))
        })
    });
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input(&missing.display().to_string());
    cx.simulate_keystrokes("enter");
    until(cx, "the node refused", |cx| {
        shell.read_with(cx, |s, cx| {
            s.hub()
                .read(cx)
                .status
                .as_ref()
                .is_some_and(|s| s.text.starts_with("Could not launch"))
        })
    });
    page.read_with(cx, |p, _| {
        let recent: Vec<&String> = p.settings.launched_folders.values().flatten().collect();
        assert!(
            !recent.iter().any(|f| f.contains("not-there")),
            "{recent:?}"
        );
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

/// Opening the app and following idle sessions must not make them look busy.
#[gpui::test]
fn connecting_the_app_preserves_idle_session_activity(cx: &mut TestAppContext) {
    let Some(running) = start_config("terminals-idle-journey", false, false, false, true) else {
        return;
    };
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let read = || {
        let args = ["list", "--json", "--node", &running.url].map(str::to_owned);
        let mut output = Vec::new();
        assert_eq!(
            runtime
                .block_on(ahjo_node_kit::cli::run("session", &args, &mut output))
                .unwrap(),
            0
        );
        serde_json::from_slice::<serde_json::Value>(&output).unwrap()
    };
    let deadline = Instant::now() + Duration::from_secs(15);
    let baseline = loop {
        let value = read();
        let sessions = value["sessions"].as_array().unwrap();
        if sessions.len() == 2
            && sessions
                .iter()
                .all(|s| s["active"] == false && s["last_output"].is_number())
        {
            break sessions
                .iter()
                .map(|s| {
                    (
                        s["name"].as_str().unwrap().to_string(),
                        s["last_output"].clone(),
                    )
                })
                .collect::<std::collections::BTreeMap<_, _>>();
        }
        assert!(
            Instant::now() < deadline,
            "sessions did not become idle: {value}"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    // Make a changed activity timestamp observable even though the CLI
    // reports whole seconds.
    let newest = baseline
        .values()
        .filter_map(serde_json::Value::as_u64)
        .max()
        .unwrap();
    while std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        <= newest + 1
    {
        std::thread::sleep(Duration::from_millis(20));
    }
    let config = format!(
        "[terminals]\nsocket = {:?}\nlaunch = {:?}\n",
        running.socket,
        running.fixture.join("no-launch.conf")
    );
    let second = runtime.block_on(Node::start(AREAS, &config)).unwrap();
    // Nobody looks at the second node, so it lists when asked.
    let second_lists = |what: &str, check: &dyn Fn(&[ahjo_term::engine::SessionInfo]) -> bool| {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let sessions = runtime.block_on(second.sessions().listed()).unwrap();
            if check(&sessions) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "second node: {what}: {sessions:?}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    };
    let seconds = |s: &ahjo_term::engine::SessionInfo| {
        s.activity
            .last_output
            .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|age| age.as_secs())
    };
    second_lists(
        "lists both sessions with their old output times",
        &|sessions| {
            sessions.len() == 2
                && sessions
                    .iter()
                    .all(|s| seconds(s) == baseline[&s.name].as_u64())
        },
    );
    let (_shell, cx, settings) = open_with_alpha(&running, "idle-journey", cx);
    // The window sizes its session even while another follower watches.
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    until(cx, "app follows the session", |cx| {
        page.read_with(cx, |p, _| {
            p.fleet.all.iter().any(|s| {
                s.name == "alpha" && s.seat == Some(ahjo_terminals_contract::SizingPane::Mine)
            })
        })
    });
    let after = read();
    for session in after["sessions"].as_array().unwrap() {
        let name = session["name"].as_str().unwrap();
        assert_eq!(
            session["last_output"], baseline[name],
            "{name} changed idle time on connect: {after}"
        );
        assert_eq!(
            session["active"], false,
            "{name} became active on connect: {after}"
        );
    }
    for session in second.sessions().list() {
        assert_eq!(
            session
                .activity
                .last_output
                .unwrap()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            baseline[&session.name].as_u64().unwrap(),
            "second node changed {} on app connection",
            session.name
        );
    }
    tmux(
        &running,
        &[
            "send-keys",
            "-t",
            "alpha",
            "printf 'real-output\\n'",
            "Enter",
        ],
    );
    until(cx, "actual output marks the session active", |cx| {
        page.read_with(cx, |p, _| {
            p.fleet
                .all
                .iter()
                .any(|s| s.name == "alpha" && s.active == Some(true))
        })
    });
    let after_output = read();
    let alpha = after_output["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["name"] == "alpha")
        .unwrap();
    assert!(alpha["last_output"].as_u64().unwrap() > baseline["alpha"].as_u64().unwrap());
    second_lists("observes actual output", &|sessions| {
        let of = |name: &str| sessions.iter().find(|s| s.name == name).and_then(seconds);
        of("alpha") > baseline["alpha"].as_u64() && of("beta") == baseline["beta"].as_u64()
    });
    runtime.block_on(second.stop());
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

/// Ctrl+wheel and Ctrl+= over a terminal zoom that one terminal; Ctrl+0
/// puts it back on the Settings size. Peek zooms its own text, takes what
/// is typed, and never sizes the session it shows.
#[gpui::test]
fn ctrl_wheel_zooms_the_terminal_under_it_and_peek_is_typed_into(cx: &mut TestAppContext) {
    let Some(running) = start("zoom-and-peek") else {
        return;
    };
    let (_shell, cx, settings) = open_with_alpha(&running, "zoom-and-peek", cx);
    let page = PAGE.with(|p| p.borrow().clone()).unwrap();
    // A notch of the wheel, as the platforms report it: three lines.
    let ctrl_wheel = |cx: &mut VisualTestContext, at: gpui::Point<gpui::Pixels>, notches: f32| {
        cx.simulate_event(gpui::ScrollEvent {
            position: at,
            delta: gpui::ScrollDelta::Lines(gpui::point(0., 3. * notches)),
            modifiers: Modifiers::control(),
            touch_phase: gpui::TouchPhase::Moved,
            source: gpui::ScrollSource::Pointer,
        });
        cx.run_until_parked();
    };
    let window_font = |cx: &mut VisualTestContext| {
        page.read_with(cx, |p, _| p.settings.workspace.windows()[0].font)
    };
    let at = cx.debug_bounds("window-0").expect("window 0").center();
    ctrl_wheel(cx, at, 2.);
    assert_eq!(window_font(cx), Some(16), "two notches in");
    page.read_with(cx, |p, _| {
        assert!(p.histories.is_empty(), "a zoom does not read back");
        assert_eq!(p.settings.font_size, 14., "Settings keeps its size");
    });
    cx.simulate_keystrokes("ctrl--");
    assert_eq!(window_font(cx), Some(15));
    cx.simulate_keystrokes("ctrl-0");
    assert_eq!(window_font(cx), None, "Ctrl+0 back on the Settings size");

    until(cx, "beta listed", |cx| {
        page.read_with(cx, |p, _| p.fleet.all.iter().any(|s| s.name == "beta"))
    });
    click(cx, format!("row-{}::beta", running.node));
    until(cx, "Peek's terminal", |cx| {
        cx.debug_bounds("peek-body").is_some()
    });
    cx.simulate_input("echo peek-$((6*7))");
    cx.simulate_keystrokes("enter");
    until(cx, "beta to run what was typed in Peek", |_| {
        tmux(&running, &["capture-pane", "-p", "-t", "beta"]).contains("peek-42")
    });
    let over = cx.debug_bounds("peek-body").expect("Peek").center();
    // Peek is drawn in the last place the page lays terminals out.
    let filled = page.read_with(cx, |p, _| p.geo.borrow().last().unwrap().font);
    let filled = filled.expect("Peek drawn") as u8;
    assert!(
        filled > 14,
        "unzoomed, Peek fills its room past the Settings size"
    );
    // Peek zooms past the fitted size like every other view: its text
    // grows and the view follows the cursor, the session keeps its size.
    ctrl_wheel(cx, over, 1.);
    page.read_with(cx, |p, _| {
        assert_eq!(
            p.settings.peek_font,
            Some(filled + 1),
            "Peek grows past the fit"
        );
        assert_eq!(
            p.geo.borrow().last().unwrap().font,
            Some(f32::from(filled + 1))
        );
    });
    ctrl_wheel(cx, over, -2.);
    page.read_with(cx, |p, _| {
        assert_eq!(
            p.settings.peek_font,
            Some(filled - 1),
            "Peek zooms its own text"
        );
        assert_eq!(p.settings.workspace.windows()[0].font, None);
        let beta = p.fleet.all.iter().find(|s| s.name == "beta").unwrap();
        let pane = beta.panes[0].id;
        assert!(
            p.fleet.claimed(&beta.node, pane).is_none(),
            "Peek asked tmux for no size"
        );
    });
    cx.simulate_keystrokes("ctrl-0");
    page.read_with(cx, |p, _| {
        assert_eq!(p.settings.peek_font, None, "Ctrl+0 fills Peek again")
    });
    cx.simulate_keystrokes("shift-escape");
    until(cx, "Shift+Escape closes Peek", |cx| {
        cx.debug_bounds("peek").is_none()
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

/// A session's name is edited where it was asked for: in its window's tag
/// from the window's menu, in its rail row from the row's. Esc and a click
/// elsewhere keep the name; Enter renames it.
#[gpui::test]
fn a_session_is_renamed_where_its_name_is_shown(cx: &mut TestAppContext) {
    let Some(running) = start("terminals-rename-in-place") else {
        return;
    };
    let (_shell, cx, settings) = open_with_alpha(&running, "rename-in-place", cx);
    // Focus leaving a field is seen only in an active window.
    cx.update(|window, _| window.activate_window());
    let page = PAGE.with(|p| p.borrow().clone()).expect("the page");
    let key = Key {
        node: running.node.clone(),
        user: String::new(),
        name: "alpha".into(),
    };
    let row: &'static str = Box::leak(format!("row-{}::alpha", running.node).into_boxed_str());
    let rename = |cx: &mut VisualTestContext, from_rail: bool| {
        let act = page.read_with(cx, |p, _| {
            let items = if from_rail {
                p.row_menu(&key)
            } else {
                p.session_menu(0, &key)
            };
            items
                .into_iter()
                .find(|r| r.label.starts_with("Rename"))
                .expect("the menu renames")
                .act
                .unwrap()
        });
        cx.update(|window, cx| page.update(cx, |p, cx| p.act(act, window, cx)));
        until(cx, "the name's field", |cx| {
            cx.debug_bounds("session-name-field").is_some()
        });
        cx.debug_bounds("session-name-field").unwrap()
    };

    // From the window's menu: in its tag, over the window, not in the rail.
    let field = rename(cx, false);
    let window = cx.debug_bounds("window-0").expect("the window");
    let rail_row = cx.debug_bounds(row).expect("alpha's row");
    assert!(
        field.left() >= window.left() && field.right() <= window.right(),
        "{field:?} in window {window:?}"
    );
    assert!(
        field.bottom() <= window.top() + px(40.),
        "{field:?} in the head"
    );
    assert!(!rail_row.intersects(&field), "not in the rail");
    cx.simulate_keystrokes("escape");
    until(cx, "Esc closes it", |cx| {
        cx.debug_bounds("session-name-field").is_none()
    });

    // From the rail row's menu: in the row. A click elsewhere keeps the name.
    let field = rename(cx, true);
    let rail_row = cx.debug_bounds(row).expect("alpha's row");
    assert!(
        rail_row.contains(&field.center()),
        "{field:?} in {rail_row:?}"
    );
    cx.simulate_input("lost");
    cx.simulate_click(window.center(), Modifiers::none());
    until(cx, "a click elsewhere closes it", |cx| {
        cx.debug_bounds("session-name-field").is_none()
    });
    assert!(page.read_with(cx, |p, _| p.fleet.session(&key).is_some()));

    // Enter renames it.
    rename(cx, true);
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("omega");
    cx.simulate_keystrokes("enter");
    let renamed = Key {
        name: "omega".into(),
        ..key.clone()
    };
    until(cx, "alpha renamed omega", |cx| {
        page.read_with(cx, |p, _| p.fleet.session(&renamed).is_some())
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}
