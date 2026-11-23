#![cfg(unix)]
//! A mission opens on its board with Flow beside it: readable, and tied
//! to Flow both ways.
mod common;
use ahjo_app_kit::link::client::LinkState;
use ahjo_node_kit::{Listing, NodeClient};
use ahjo_testkit::app::{Served, Started, serve, until};
use ahjo_testkit::{node::Node, sessions};
use gpui::{Bounds, Modifiers, Pixels, TestAppContext, VisualTestContext, px};
use serde_json::json;

const AREAS: &[Listing] = &[
    sessions::LISTING,
    ahjo_issue::LISTING,
    ahjo_missions::LISTING,
];
struct Fixture {
    steps: Vec<String>,
}
/// A node with a ten-step sketch: longer than its pane shows whole.
fn start() -> Served<Fixture> {
    serve(move || async move {
        let node = Node::start(AREAS, "").await.unwrap();
        let client = NodeClient::new(Some(&format!("{}/", node.url()))).unwrap();
        let steps: Vec<String> = (1..=10).map(|n| format!("Step {n}")).collect();
        let doc = common::call(
            &client,
            "mission_create",
            json!({"slug":"long", "title":"Long", "steps":steps.join("\n")}),
        )
        .await
        .unwrap();
        let ids = doc["steps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["id"].as_str().unwrap().to_owned())
            .collect();
        Started::new(node, Fixture { steps: ids })
    })
}
fn at(cx: &mut VisualTestContext, selector: String) -> Option<Bounds<Pixels>> {
    cx.debug_bounds(Box::leak(selector.into_boxed_str()))
}
fn inside(inner: Bounds<Pixels>, outer: Bounds<Pixels>) -> bool {
    inner.origin.x >= outer.origin.x
        && inner.origin.y >= outer.origin.y
        && inner.bottom_right().x <= outer.bottom_right().x
        && inner.bottom_right().y <= outer.bottom_right().y
}

#[gpui::test]
fn a_long_mission_opens_on_its_board_beside_flow_and_follows_the_choice(cx: &mut TestAppContext) {
    let running = start();
    let settings = std::env::temp_dir()
        .join(format!("ahjo-missions-beside-{}", std::process::id()))
        .join("app.json");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let (url, path) = (running.url.clone(), settings.clone());
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(
            &[ahjo_missions_app::build],
            Some(url),
            Some(path),
            window,
            cx,
        )
    });
    // A short window: ten steps cannot all show there at a readable size.
    cx.simulate_resize(gpui::size(px(1920.), px(420.)));
    until(cx, "link", |cx| {
        shell.read_with(cx, |s, cx| {
            s.hub().read(cx).fleet.here().state == LinkState::Open
        })
    });
    let listing = format!("mission-{}-long", running.id);
    until(cx, "mission listing", |cx| {
        at(cx, listing.clone()).is_some()
    });
    let row = at(cx, listing).unwrap().center();
    cx.simulate_click(row, Modifiers::none());
    let first = running.steps[0].clone();
    until(cx, "the board beside its panel", |cx| {
        cx.debug_bounds("missions-board").is_some()
            && cx.debug_bounds("missions-flow").is_some()
            && at(cx, format!("mission-card-{first}")).is_some()
    });
    // Flow, the mission in words, takes Activity's place from the menu.
    let more = cx.debug_bounds("mission-more").unwrap().center();
    cx.simulate_click(more, Modifiers::none());
    until(cx, "Show Flow", |cx| {
        cx.debug_bounds("menu-label-Show Flow").is_some()
    });
    let show = cx.debug_bounds("menu-label-Show Flow").unwrap().center();
    cx.simulate_click(show, Modifiers::none());
    until(cx, "Flow beside the board", |cx| {
        at(cx, format!("flow-title-{first}")).is_some()
    });
    let pane = cx.debug_bounds("missions-board").unwrap();
    // It opens on its start, a card drawn no smaller than its words read
    // at (four fifths of a step card's width on the board).
    let card = at(cx, format!("mission-card-{first}")).unwrap();
    let readable = ahjo_missions_app::store::canvas::STEP_W * 0.8;
    assert!(card.size.width >= px(readable), "{card:?}");
    assert!(inside(card, pane));
    // Too long to show whole at that size: the last step is out of view
    // (not drawn, or drawn past the pane's edge) until it is chosen in
    // Flow, which brings it into view on the board.
    let last = running.steps.last().unwrap().clone();
    assert!(at(cx, format!("mission-card-{last}")).is_none_or(|c| !inside(c, pane)));
    let window = cx.debug_bounds("missions-flow").unwrap();
    until(cx, "the last step in view in Flow", |cx| {
        let seen = at(cx, format!("flow-title-{last}")).is_some_and(|t| inside(t, window));
        if !seen {
            cx.simulate_event(gpui::ScrollEvent {
                position: window.center(),
                delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-200.))),
                modifiers: Modifiers::none(),
                touch_phase: gpui::TouchPhase::Moved,
                source: gpui::ScrollSource::Pointer,
            });
        }
        seen
    });
    assert!(
        at(cx, format!("mission-card-{last}")).is_none_or(|c| !inside(c, pane)),
        "scrolling Flow leaves the board where it was"
    );
    let title = at(cx, format!("flow-title-{last}")).unwrap().center();
    cx.simulate_click(title, Modifiers::none());
    until(cx, "the step brought into view", |cx| {
        at(cx, format!("mission-card-{last}")).is_some_and(|c| inside(c, pane))
    });
    // Chosen on the board, a step opens in the editor in Flow's place,
    // and is marked in Flow once the editor closes.
    let card = at(cx, format!("mission-card-{last}")).unwrap().center();
    cx.simulate_click(card, Modifiers::none());
    until(cx, "the step's editor in Flow's place", |cx| {
        cx.debug_bounds("step-save").is_some() && cx.debug_bounds("missions-flow").is_none()
    });
    cx.simulate_keystrokes("escape");
    until(cx, "the step marked in Flow", |cx| {
        at(cx, format!("flow-chosen-{last}")).is_some()
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}
