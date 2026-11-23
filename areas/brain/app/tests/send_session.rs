//! A page sent from the Brain panel travels through the real Send drawer
//! into a scratch tmux session.
#![cfg(unix)]
use ahjo_app_kit::{Host, Mounted, link::client::LinkState, mount};
use ahjo_terminals_app::Terminals;
use ahjo_testkit::app::{Served, Started, click, serve, until};
use ahjo_testkit::{fresh_dir, node::Node, tmux::Server};
use gpui::{Entity, TestAppContext, VisualTestContext, Window};
use std::cell::RefCell;

thread_local! { static TERMINALS: RefCell<Option<Entity<Terminals>>> = const { RefCell::new(None) }; }
fn terminals(host: Host, window: &mut Window, cx: &mut gpui::App) -> Mounted {
    mount(host, window, cx, |host, window, cx| {
        TERMINALS.with(|p| *p.borrow_mut() = Some(cx.entity()));
        Terminals::new(host, window, cx)
    })
}
fn start() -> Option<Served<()>> {
    let tmux = Server::start_in("brain-send-journey")?;
    Some(serve(move || async move {
        tmux.new_session("alpha", 100, 30);
        let corpus = fresh_dir("brain-send-corpus");
        std::fs::create_dir(corpus.join("notes")).unwrap();
        std::fs::write(corpus.join("notes/page.md"), "# Page\n").unwrap();
        // A page arrives by a commit.
        for args in [
            &["init", "-q"][..],
            &["config", "user.name", "Fixture"],
            &["config", "user.email", "fixture@example.org"],
            &["add", "."],
            &["commit", "-qm", "Fixture"],
        ] {
            let done = std::process::Command::new("git")
                .arg("-C")
                .arg(&corpus)
                .args(args)
                .output()
                .unwrap();
            assert!(
                done.status.success(),
                "{}",
                String::from_utf8_lossy(&done.stderr)
            );
        }
        let config = format!(
            "[brain]\ncorpus = {:?}\n[terminals]\nsocket = {:?}\n",
            corpus,
            tmux.socket_path()
        );
        let node = Node::start(&[ahjo_terminals::LISTING, ahjo_brain::LISTING], &config)
            .await
            .unwrap();
        Started::new(node, ()).then(move || {
            std::fs::remove_dir_all(corpus).unwrap();
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
fn brain_send_reaches_a_real_session(cx: &mut TestAppContext) {
    let Some(running) = start() else {
        return;
    };
    let settings = fresh_dir("brain-send-settings");
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let (shell, cx) = cx.add_window_view(|window, cx| {
        ahjo_app_shell::start(
            &[terminals, ahjo_brain_app::build],
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
    let row = format!("row-{}::alpha", running.id);
    let selector = Box::leak(row.clone().into_boxed_str());
    until(cx, "session", |cx| cx.debug_bounds(selector).is_some());
    click(cx, &row);
    until(cx, "attach", |cx| cx.debug_bounds("peek-attach").is_some());
    click(cx, "peek-attach");
    until(cx, "session screen", |cx| !shown(cx).trim().is_empty());
    cx.simulate_keystrokes("alt-l");
    until(cx, "the page among the arrivals", |cx| {
        cx.debug_bounds("brain-arrived-notes/page.md").is_some()
    });
    click(cx, "brain-arrived-notes/page.md");
    until(cx, "reader", |cx| {
        cx.debug_bounds("brain-reading-notes/page.md").is_some()
    });
    click(cx, "brain-send");
    until(cx, "send drawer", |cx| cx.debug_bounds("send-go").is_some());
    click(cx, "send-go");
    until(cx, "reference in session", |cx| {
        let text = shown(cx);
        text.contains("brain page /") && text.contains("/notes/page.md")
    });
    std::fs::remove_dir_all(settings).unwrap();
}
