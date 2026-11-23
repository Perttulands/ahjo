#![cfg(unix)]
//! Every element on the board opens its panel in the window's edge panel:
//! a gate's sets its loop's tries and, in a run, says which try the work
//! is on; an end's says how a run ends there; an input's starts a run; a
//! wire's names its two cards and takes it off the board. On a run's
//! board each reads what the card did and is, and takes only answers.
use ahjo_node_kit::{Built, Listing, Setup};
use ahjo_testkit::app::{Served, Started, bounds, click, serve, until};
use ahjo_testkit::{node::Node, sessions};
use gpui::{TestAppContext, VisualTestContext};
use serde_json::json;
use std::path::PathBuf;

fn lab(setup: &Setup) -> anyhow::Result<Built> {
    ahjo_missions::build_with_executor(setup, Some(Box::new(ahjo_mission_engine::LabExecutor)))
}
const MISSIONS: Listing = Listing {
    build: lab,
    ..ahjo_missions::LISTING
};

/// A fix the operator reviews, sent back to the fixer up to three times.
const MISSION: &str = r#"schema = 6
id = "msn_fix"
slug = "fix"
title = "Fix"
rev = 1

[[input_card]]
id = "inp_bug"
title = "Bug"
inputs = [{ name = "report", kind = "text", required = true }]

[[step]]
id = "fmn_fix"
formation = "solo"
title = "Fix"
[step.brief]
goal = "Fix the bug."
[[step.input]]
id = "port_in"
label = "Report"
[[step.output]]
id = "port_out"
label = "Change"
[[step.seat]]
id = "seat_fix"
label = "Fixer"
harness = "openai-codex"
effort = "medium"

[[gate]]
id = "gate_review"
title = "Review"
kinds = ["human"]
criterion = "The change fixes the cause."

[[limit]]
id = "lim_tries"
title = "Tries"
scope = "fmn_fix"
rounds = 3
when_spent = "fail"

[[end]]
id = "end_done"
title = "Done"
outcome = "done"

[[connection]]
id = "edge_bug"
from = "inp_bug:out"
to = "fmn_fix:port_in"

[[connection]]
id = "edge_fixed"
from = "fmn_fix:port_out"
to = "gate_review:in"

[[connection]]
id = "edge_again"
from = "gate_review:fail"
to = "fmn_fix:port_in"

[[connection]]
id = "edge_done"
from = "gate_review:pass"
to = "end_done:in"
"#;

struct Fixture {
    mission: PathBuf,
    settings: PathBuf,
    run: Option<String>,
}

fn start(with_run: bool) -> Served<Fixture> {
    serve(move || async move {
        let node = Node::start(&[sessions::LISTING, ahjo_issue::LISTING, MISSIONS], "")
            .await
            .unwrap();
        let dir = node.dir().join("missions/missions");
        std::fs::create_dir_all(&dir).unwrap();
        let mission = dir.join("fix.mission.toml");
        std::fs::write(&mission, MISSION).unwrap();
        let run = if with_run {
            let answer = node
                .call(
                    "mission_run",
                    json!({"mission": "fix", "input": "inp_bug", "inputs": {"report": "it breaks"}, "cwd": node.dir()}),
                )
                .await
                .unwrap();
            Some(answer["run_id"].as_str().unwrap().to_owned())
        } else {
            None
        };
        let settings = node.dir().join("app.json");
        Started::new(
            node,
            Fixture {
                mission,
                settings,
                run,
            },
        )
    })
}

fn open<'a>(cx: &'a mut TestAppContext, running: &Served<Fixture>) -> &'a mut VisualTestContext {
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let (url, settings) = (running.url.clone(), running.settings.clone());
    let (_, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(
            &[ahjo_missions_app::build],
            Some(url),
            Some(settings),
            window,
            cx,
        )
    });
    cx
}

fn has(cx: &mut VisualTestContext, selector: &str) -> bool {
    bounds(cx, selector).is_some()
}

/// Opens the fixture's mission on the board.
fn open_mission(cx: &mut VisualTestContext, running: &Served<Fixture>) {
    let title = format!("mission-{}-fix", running.id);
    until(cx, "the mission listed", |cx| has(cx, &title));
    click(cx, &title);
    until(cx, "the board", |cx| has(cx, "mission-card-end_done"));
}

/// Whether the mission as saved holds `line`.
fn saved(running: &Served<Fixture>, line: &str) -> bool {
    std::fs::read_to_string(&running.mission)
        .unwrap()
        .lines()
        .any(|l| l.trim() == line)
}

#[gpui::test]
fn clicking_a_gate_opens_its_panel_where_its_tries_are_set(cx: &mut TestAppContext) {
    let running = start(false);
    let cx = open(cx, &running);
    let title = format!("mission-{}-fix", running.id);
    until(cx, "the mission listed", |cx| has(cx, &title));
    click(cx, &title);
    // The fail exit carries the loop it starts.
    until(cx, "the gate's tries on the board", |cx| {
        has(cx, "gate-tries-gate_review: ↺ up to 3 tries")
    });
    click(cx, "mission-card-gate_review");
    until(cx, "the gate's panel", |cx| {
        has(cx, "decider-human") && has(cx, "tries-more")
    });
    click(cx, "tries-more");
    until(cx, "four tries saved", |_| saved(&running, "rounds = 4"));
    until(cx, "four tries on the board", |cx| {
        has(cx, "gate-tries-gate_review: ↺ up to 4 tries")
    });
    click(cx, "tries-less");
    until(cx, "back to three", |_| saved(&running, "rounds = 3"));
    click(cx, "spent-best");
    until(cx, "the best goes on when they are spent", |_| {
        saved(&running, r#"when_spent = "best""#)
    });
    // The panel is the window's edge panel; Escape from inside it puts it
    // away, and so does its Close.
    assert!(has(cx, "card-panel"), "the gate's panel stands at the edge");
    click(cx, "step-title");
    cx.simulate_keystrokes("escape");
    until(cx, "Escape puts the panel away", |cx| {
        !has(cx, "card-panel")
    });
    click(cx, "mission-card-gate_review");
    until(cx, "the panel again", |cx| has(cx, "panel-close"));
    click(cx, "panel-close");
    until(cx, "Close puts it away", |cx| !has(cx, "card-panel"));
}

#[gpui::test]
fn a_runs_gate_says_which_try_the_work_is_on(cx: &mut TestAppContext) {
    let running = start(true);
    let run = running.run.clone().unwrap();
    let cx = open(cx, &running);
    let row = format!("run-row-{run}");
    until(cx, "the run listed", |cx| has(cx, &row));
    click(cx, &row);
    until(cx, "the run's view", |cx| has(cx, "run-graph"));
    click(cx, "run-graph");
    until(cx, "the first try waits on the operator", |cx| {
        has(cx, "gate-tries-gate_review: ↺ try 1 of 3")
    });
    // In a run the gate opens its panel too, with the operator's answer.
    click(cx, "mission-card-gate_review");
    until(cx, "send back on the gate's panel", |cx| {
        has(cx, "panel-gate-fail")
    });
    // The run's panel reads the plan and takes only the operator's answer.
    assert!(has(
        cx,
        "run-fact: Goes back to Fix, up to 3 tries; then the run fails"
    ));
    assert!(!has(cx, "tries-more") && !has(cx, "step-save"));
    click(cx, "panel-gate-fail");
    until(cx, "the second try", |cx| {
        has(cx, "gate-tries-gate_review: ↺ try 2 of 3")
    });
    // The input says what the run was given; the end, whether it ended there.
    click(cx, "mission-card-inp_bug");
    until(cx, "what the run was given", |cx| {
        has(cx, "run-given-report: it breaks")
    });
    click(cx, "mission-card-end_done");
    until(cx, "the end's panel", |cx| {
        has(cx, "run-fact: Has not ended here")
    });
    click(cx, "panel-close");
    until(cx, "put away", |cx| !has(cx, "card-panel"));
}

#[gpui::test]
fn an_ends_panel_says_how_a_run_ends_there(cx: &mut TestAppContext) {
    let running = start(false);
    let cx = open(cx, &running);
    open_mission(cx, &running);
    click(cx, "mission-card-end_done");
    until(cx, "the end's panel", |cx| has(cx, "outcome-rejected"));
    assert!(
        !has(cx, "step-goal"),
        "an end has no words beyond its title"
    );
    click(cx, "outcome-rejected");
    until(cx, "rejected saved", |_| {
        saved(&running, r#"outcome = "rejected""#)
    });
}

#[gpui::test]
fn an_inputs_panel_starts_a_run(cx: &mut TestAppContext) {
    let running = start(false);
    let cx = open(cx, &running);
    open_mission(cx, &running);
    // The ▶ starts a run; the names under it open the card's panel.
    click(cx, "canvas-start-inp_bug");
    until(cx, "the input's panel", |cx| has(cx, "input-start"));
    // What a run takes is the card's own: one more, named and saved.
    assert!(has(cx, "run-input-0") && !has(cx, "run-input-1"));
    click(cx, "add-input");
    until(cx, "a blank input", |cx| has(cx, "input-name-1"));
    click(cx, "input-name-1");
    cx.simulate_input("severity");
    click(cx, "step-save");
    until(cx, "the input saved", |_| {
        std::fs::read_to_string(&running.mission)
            .unwrap()
            .contains(r#"name = "severity""#)
    });
    // A notification, the default, is the only way to ask about a gate
    // he decides: no choice is offered.
    click(cx, "canvas-start-inp_bug");
    until(cx, "the panel again", |cx| has(cx, "input-start"));
    assert!(!has(cx, "channel-session"));
    click(cx, "input-start");
    until(cx, "the start form", |cx| has(cx, "run-start-actions"));
}

#[gpui::test]
fn a_click_on_a_wire_opens_its_panel_where_it_comes_off(cx: &mut TestAppContext) {
    let running = start(false);
    let cx = open(cx, &running);
    open_mission(cx, &running);
    // Just short of the end's way in, on the wire from the gate's pass.
    let port = bounds(cx, "mission-port-end_done:in").unwrap();
    let on_wire = gpui::point(port.left() - gpui::px(8.), port.center().y);
    cx.simulate_click(on_wire, gpui::Modifiers::none());
    until(cx, "the wire's panel", |cx| has(cx, "wire-from"));
    assert!(has(cx, "wire-to"));
    // Each end opens its card's panel in its place.
    click(cx, "wire-from");
    until(cx, "the gate's panel", |cx| has(cx, "decider-human"));
    cx.simulate_click(on_wire, gpui::Modifiers::none());
    until(cx, "the wire's panel again", |cx| has(cx, "wire-delete"));
    click(cx, "wire-delete");
    until(cx, "the wire gone", |_| {
        !saved(&running, r#"id = "edge_done""#)
    });
    until(cx, "its panel put away", |cx| !has(cx, "card-panel"));
}
