#![cfg(unix)]
mod common;
use ahjo_node_kit::{Listing, NodeClient};
use ahjo_testkit::app::{Served, Started, click, serve, until};
use ahjo_testkit::{node::Node, sessions};
use gpui::{TestAppContext, VisualTestContext};
use serde_json::json;
const AREAS: &[Listing] = &[
    sessions::LISTING,
    ahjo_issue::LISTING,
    ahjo_missions::LISTING,
];
fn start() -> Served<()> {
    serve(move || async move {
        let node = Node::start(AREAS, "").await.unwrap();
        Started::new(node, ())
    })
}
fn has(cx: &mut VisualTestContext, selector: &str) -> bool {
    cx.debug_bounds(Box::leak(selector.to_owned().into_boxed_str()))
        .is_some()
}
#[gpui::test]
fn creates_and_renames_a_mission_with_cancel_and_conflict_guard(cx: &mut TestAppContext) {
    let running = start();
    let settings = std::env::temp_dir()
        .join(format!("ahjo-manage-{}", std::process::id()))
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
    until(cx, "new mission", |cx| has(cx, "new-mission"));
    click(cx, "new-mission");
    until(cx, "create form", |cx| has(cx, "mission-form-name"));
    cx.simulate_input("Abandoned");
    // A draft is not lost to Escape or a stray press: the form says so.
    cx.simulate_keystrokes("escape");
    until(cx, "draft kept", |cx| has(cx, "mission-form-error"));
    let form = cx.debug_bounds("mission-form").unwrap();
    let outside = gpui::point(form.right() + gpui::px(40.), form.bottom() + gpui::px(40.));
    cx.simulate_click(outside, gpui::Modifiers::none());
    assert!(has(cx, "mission-form"), "a press outside keeps the draft");
    click(cx, "mission-form-cancel");
    until(cx, "form canceled", |cx| !has(cx, "mission-form"));
    click(cx, "new-mission");
    until(cx, "fresh create form", |cx| has(cx, "mission-form-name"));
    cx.simulate_input("@@@");
    click(cx, "mission-form-submit");
    until(cx, "invalid title", |cx| has(cx, "mission-form-error"));
    click(cx, "mission-form-name");
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("Release");
    click(cx, "mission-form-submit");
    until(cx, "created", |cx| !has(cx, "mission-form"));
    // Rename and Delete wait in the mission head's "…" menu.
    let menu = |cx: &mut VisualTestContext, label: &str| {
        until(cx, "mission menu", |cx| has(cx, "mission-more"));
        click(cx, "mission-more");
        let row = format!("menu-label-{label}");
        until(cx, "menu row", |cx| has(cx, &row));
        click(cx, &row);
    };
    until(cx, "release shown", |cx| has(cx, "pane-title-Release"));
    // The name is edited where it stands, in the head.
    menu(cx, "Rename…");
    until(cx, "rename in the head", |cx| has(cx, "mission-form-name"));
    let field = cx.debug_bounds("mission-form-name").unwrap();
    let head = cx.debug_bounds("mission-more").unwrap();
    assert!(
        (field.center().y - head.center().y).abs() < gpui::px(12.),
        "{field:?} on the head's line with {head:?}"
    );
    assert!(!has(cx, "mission-form"), "no form for a rename");
    // Esc lets it go, keeping the name.
    cx.simulate_input(" draft");
    cx.simulate_keystrokes("escape");
    until(cx, "rename let go", |cx| !has(cx, "mission-form-name"));
    until(cx, "name kept", |cx| has(cx, "pane-title-Release"));
    menu(cx, "Rename…");
    until(cx, "rename in the head again", |cx| {
        has(cx, "mission-form-name")
    });
    cx.simulate_keystrokes("ctrl-a backspace enter");
    until(cx, "name required", |cx| has(cx, "mission-form-error"));
    cx.simulate_input("Renamed");
    cx.simulate_keystrokes("enter");
    until(cx, "renamed", |cx| !has(cx, "mission-form-name"));
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let read = || {
        runtime.block_on(async {
            let client = NodeClient::new(Some(&format!("{}/", running.url))).unwrap();
            common::call(
                &client,
                "mission_show",
                serde_json::json!({"mission":"release","full":true}),
            )
            .await
            .unwrap()
        })
    };
    assert_eq!(read()["mission"]["title"], "Renamed");
    menu(cx, "Rename…");
    until(cx, "rename again", |cx| has(cx, "mission-form-name"));
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("Draft name");
    runtime.block_on(async {
        let client = NodeClient::new(Some(&format!("{}/", running.url))).unwrap();
        let doc = common::call(&client, "mission_show", serde_json::json!({"mission":"release","full":true})).await.unwrap();
        common::call(&client, "mission_edit", serde_json::json!({"mission":"release", "change":json!({"title":"External", "expected_rev":doc["mission"]["rev"]})}))
        .await
        .unwrap();
    });
    cx.simulate_keystrokes("enter");
    until(cx, "stale rename refused", |cx| {
        has(cx, "mission-form-error")
    });
    click(cx, "mission-form-name");
    cx.simulate_keystrokes("ctrl-a ctrl-c");
    assert_eq!(
        cx.read_from_clipboard()
            .and_then(|item| item.text())
            .as_deref(),
        Some("Draft name")
    );
    assert_eq!(read()["mission"]["title"], "External");
    cx.simulate_keystrokes("escape");
    until(cx, "stale rename let go", |cx| {
        !has(cx, "mission-form-name")
    });
    click(cx, "new-mission");
    until(cx, "second create form", |cx| has(cx, "mission-form-name"));
    cx.simulate_input("Scratch");
    click(cx, "mission-form-submit");
    until(cx, "second mission created", |cx| !has(cx, "mission-form"));
    let listed = format!("mission-{}-scratch", running.id);
    until(cx, "scratch shown", |cx| has(cx, "pane-title-Scratch"));
    menu(cx, "Delete…");
    until(cx, "delete asked", |cx| {
        has(cx, "menu-label-Delete mission")
    });
    // Beside the "…" it was asked for from, not across the window.
    let asked = cx.debug_bounds("popup").unwrap();
    let more = cx.debug_bounds("mission-more").unwrap();
    assert!(
        (asked.top() - more.bottom()).abs() < gpui::px(12.)
            && (asked.right() - more.right()).abs() < gpui::px(12.),
        "{asked:?} under {more:?}, at its right edge"
    );
    cx.simulate_keystrokes("escape");
    until(cx, "delete let go", |cx| !has(cx, "popup"));
    assert!(has(cx, &listed), "Escape deletes nothing");
    menu(cx, "Delete…");
    until(cx, "delete asked again", |cx| {
        has(cx, "menu-label-Delete mission")
    });
    click(cx, "menu-label-Delete mission");
    until(cx, "mission deleted", |cx| {
        !has(cx, "popup") && !has(cx, &listed)
    });
    runtime.block_on(async {
        let client = NodeClient::new(Some(&format!("{}/", running.url))).unwrap();
        assert!(
            common::call(
                &client,
                "mission_show",
                serde_json::json!({"mission":"scratch","full":true})
            )
            .await
            .is_err()
        );
    });
}

#[gpui::test]
fn designs_a_multi_gate_mission_and_shows_findings_on_the_canvas(cx: &mut TestAppContext) {
    let running = start();
    let settings = std::env::temp_dir()
        .join(format!("ahjo-design-{}", std::process::id()))
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
    cx.simulate_resize(gpui::size(gpui::px(1600.), gpui::px(1000.)));
    until(cx, "new mission", |cx| has(cx, "new-mission"));
    click(cx, "new-mission");
    until(cx, "new mission form", |cx| has(cx, "mission-form-name"));
    cx.simulate_input("Design journey");
    click(cx, "mission-form-submit");
    let mission = format!("mission-{}-design-journey", running.id);
    until(cx, "mission listing", |cx| has(cx, &mission));
    click(cx, &mission);
    // An empty mission opens on its board, which says to add its Input.
    until(cx, "how to begin", |cx| has(cx, "canvas-empty-input"));
    click(cx, "canvas-empty-input");
    let rt = tokio::runtime::Runtime::new().unwrap();
    let client = NodeClient::new(Some(&format!("{}/", running.url))).unwrap();
    let read = || {
        rt.block_on(common::call(
            &client,
            "mission_show",
            serde_json::json!({"mission":"design-journey","full":true}),
        ))
        .unwrap()["mission"]
            .clone()
    };
    until(cx, "input saved", |_| {
        read()["input_card"].as_array().unwrap().len() == 1
    });
    // The + beside the Input adds the next step, wired from it and open
    // for editing.
    let input = read()["input_card"][0]["id"].as_str().unwrap().to_owned();
    let next = format!("canvas-next-{input}");
    until(cx, "next-step button", |cx| has(cx, &next));
    click(cx, &next);
    until(cx, "next step wired", |_| {
        let m = read();
        m["step"].as_array().unwrap().len() == 1
            && m["connection"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["from"].as_str().unwrap().starts_with(&input))
    });
    until(cx, "next step open", |cx| has(cx, "step-title"));
    // Once something follows the Input, its + is gone: no wire runs past it.
    until(cx, "+ gone once wired", |cx| !has(cx, &next));
    for (button, field, count) in [("new-step", "step", 2), ("menu-label-Add gate", "gate", 1)] {
        until(cx, "board ready for edit", |cx| has(cx, "board-ready"));
        if button != "new-step" {
            click(cx, "mission-more");
            until(cx, "the rarer cards", |cx| has(cx, button));
        }
        click(cx, button);
        until(cx, "card saved", |_| {
            read()[field].as_array().unwrap().len() == count
        });
    }
    let gate_id = read()["gate"][0]["id"].as_str().unwrap().to_owned();
    until(cx, "board ready to arrange", |cx| has(cx, "board-ready"));
    // Arrange waits in the mission head's "…" menu.
    click(cx, "mission-more");
    until(cx, "the arrange row", |cx| has(cx, "menu-label-Arrange"));
    click(cx, "menu-label-Arrange");
    until(cx, "arranged layout saved", |_| {
        let layout = rt
            .block_on(common::call(
                &client,
                "layout_show",
                serde_json::json!({"mission":"design-journey"}),
            ))
            .unwrap();
        layout["layout"]["card"]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| node["id"] == gate_id && (node["x"] != 200 || node["y"] != 200))
    });
    // What it lacks is counted in the head and listed under the board when
    // asked for.
    until(cx, "the count of what it lacks", |cx| {
        has(cx, "mission-todo")
    });
    click(cx, "mission-todo");
    until(cx, "arranged canvas", |cx| {
        has(cx, "canvas-fit") && has(cx, "canvas-findings")
    });
    click(cx, "canvas-fit");
    let gate = format!("mission-card-{gate_id}");
    until(cx, "judge gate card", |cx| has(cx, &gate));
    click(cx, &gate);
    cx.run_until_parked();
    // Who decides is chosen on the gate's panel, and saved as chosen.
    until(cx, "gate panel", |cx| has(cx, "decider-step"));
    click(cx, "decider-step");
    until(cx, "judge gate saved", |_| {
        read()["gate"][0]["kinds"][0] == "step"
    });
    click(cx, "step-save");
    until(cx, "gate panel closed", |cx| !has(cx, "decider-step"));
    for (button, field, count) in [
        ("menu-label-Add gate", "gate", 2),
        ("menu-label-Add end", "end", 1),
    ] {
        until(cx, "board ready for edit", |cx| has(cx, "board-ready"));
        if button != "new-step" {
            click(cx, "mission-more");
            until(cx, "the rarer cards", |cx| has(cx, button));
        }
        click(cx, button);
        until(cx, "card saved", |_| {
            read()[field].as_array().unwrap().len() == count
        });
    }
    let doc = read();
    assert_eq!(doc["gate"][1]["kinds"][0], "human");
    let selector = format!("validation-card-{}", doc["step"][0]["id"].as_str().unwrap());
    until(cx, "finding on a step card", |cx| has(cx, &selector));
}
