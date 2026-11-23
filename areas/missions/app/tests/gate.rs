#![cfg(unix)]
//! A gate is one element on the board: a click opens its panel, where its
//! loop's tries are set, and in a run it says which try the work is on.
use ahjo_node_kit::{Built, Listing, Setup};
use ahjo_testkit::app::{Served, Started, click, serve, until};
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
    ahjo_testkit::app::bounds(cx, selector).is_some()
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
    click(cx, "panel-gate-fail");
    until(cx, "the second try", |cx| {
        has(cx, "gate-tries-gate_review: ↺ try 2 of 3")
    });
}
