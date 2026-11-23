#![cfg(unix)]
use ahjo_node_kit::Listing;
use ahjo_testkit::app::{Served, Started, click, serve, until};
use ahjo_testkit::{node::Node, sessions};
use gpui::{TestAppContext, VisualTestContext};
use std::path::PathBuf;
const AREAS: &[Listing] = &[
    sessions::LISTING,
    ahjo_issue::LISTING,
    ahjo_missions::LISTING,
];
struct Fixture {
    bad: PathBuf,
}
fn start() -> Served<Fixture> {
    serve(move || async move {
        let node = Node::start(AREAS, "").await.unwrap();
        let root = node.dir().join("missions");
        std::fs::create_dir_all(root.join("missions")).unwrap();
        std::fs::create_dir_all(root.join("notes")).unwrap();
        std::fs::write(
            root.join("missions/hello.mission.toml"),
            include_str!("fixtures/solo-lab.mission.toml"),
        )
        .unwrap();
        let bad = root.join("notes/hello.notes.toml");
        std::fs::write(&bad, "invalid = [").unwrap();
        Started::new(node, Fixture { bad })
    })
}
fn has(cx: &mut VisualTestContext, id: &str) -> bool {
    cx.debug_bounds(Box::leak(id.to_owned().into_boxed_str()))
        .is_some()
}
#[gpui::test]
fn failed_note_read_can_be_retried(cx: &mut TestAppContext) {
    let running = start();
    let settings = std::env::temp_dir()
        .join(format!("ahjo-note-retry-{}", std::process::id()))
        .join("app.json");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let url = running.url.clone();
    let (_, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(
            &[ahjo_missions_app::build],
            Some(url),
            Some(settings),
            window,
            cx,
        )
    });
    let mission = format!("mission-{}-hello", running.id);
    until(cx, "mission", |cx| has(cx, &mission));
    click(cx, &mission);
    until(cx, "notes read failure", |cx| {
        has(cx, "canvas-notes-error") && has(cx, "canvas-notes-retry")
    });
    std::fs::remove_file(&running.bad).unwrap();
    click(cx, "canvas-notes-retry");
    until(cx, "notes recovered", |cx| {
        !has(cx, "canvas-notes-error") && has(cx, "canvas-note-pin-fmn_work")
    });
}
