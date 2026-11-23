#![cfg(unix)]
mod common;
use ahjo_app_kit::link::client::LinkState;
use ahjo_node_kit::{Listing, NodeClient};
use ahjo_testkit::app::{Served, Started, serve, until};
use ahjo_testkit::{node::Node, sessions};
use gpui::{Modifiers, TestAppContext, VisualTestContext};
use serde_json::json;

const AREAS: &[Listing] = &[
    sessions::LISTING,
    ahjo_issue::LISTING,
    ahjo_missions::LISTING,
];
fn start() -> Served<()> {
    serve(move || async move {
        let node = Node::start(AREAS, "").await.unwrap();
        let folder = node.dir().join("missions/missions");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("hello.mission.toml"),
            include_str!("fixtures/solo-lab.mission.toml"),
        )
        .unwrap();
        Started::new(node, ())
    })
}
/// Show Flow in the panel, from the mission's "…" menu.
fn show_flow(cx: &mut VisualTestContext) {
    until(cx, "the mission's menu", |cx| {
        cx.debug_bounds("mission-more").is_some()
    });
    let more = cx.debug_bounds("mission-more").unwrap().center();
    cx.simulate_click(more, Modifiers::none());
    until(cx, "Show Flow", |cx| {
        cx.debug_bounds("menu-label-Show Flow").is_some()
    });
    let show = cx.debug_bounds("menu-label-Show Flow").unwrap().center();
    cx.simulate_click(show, Modifiers::none());
}
#[gpui::test]
fn opens_a_fixture_mission_on_the_canvas(cx: &mut TestAppContext) {
    let running = start();
    let settings = std::env::temp_dir()
        .join(format!("ahjo-missions-page-{}", std::process::id()))
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
    until(cx, "link", |cx| {
        shell.read_with(cx, |s, cx| {
            s.hub().read(cx).fleet.here().state == LinkState::Open
        })
    });
    let selector = Box::leak(format!("mission-{}-hello", running.id).into_boxed_str());
    until(cx, "mission listing", |cx| {
        cx.debug_bounds(selector).is_some()
    });
    let at = cx.debug_bounds(selector).unwrap().center();
    cx.simulate_click(at, Modifiers::none());
    show_flow(cx);
    until(cx, "step in flow", |cx| {
        cx.debug_bounds("flow-step-fmn_work").is_some()
    });
    let edit = cx.debug_bounds("flow-edit-fmn_work").unwrap().center();
    cx.simulate_click(edit, Modifiers::none());
    until(cx, "step editor", |cx| {
        cx.debug_bounds("step-goal").is_some()
    });
    let goal = cx.debug_bounds("step-goal").unwrap().center();
    cx.simulate_click(goal, Modifiers::none());
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("Deliver the result");
    let save = cx.debug_bounds("step-save").unwrap().center();
    cx.simulate_click(save, Modifiers::none());
    until(cx, "save", |cx| cx.debug_bounds("step-goal").is_none());
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let client = NodeClient::new(Some(&format!("{}/", running.url))).unwrap();
        let doc = common::call(
            &client,
            "mission_show",
            serde_json::json!({"mission":"hello","full":true}),
        )
        .await
        .unwrap();
        assert_eq!(
            doc["mission"]["step"][0]["brief"]["goal"],
            "Deliver the result"
        );
    });
    // The mission is on its board beside Flow all along.
    until(cx, "step on canvas", |cx| {
        cx.debug_bounds("mission-card-fmn_work").is_some()
    });
    assert!(cx.debug_bounds("mission-card-inp_hello").is_some());
    let card = cx.debug_bounds("mission-card-fmn_work").unwrap().center();
    cx.simulate_click(card, Modifiers::none());
    until(cx, "editor reopened", |cx| {
        cx.debug_bounds("step-goal").is_some()
    });
    let goal = cx.debug_bounds("step-goal").unwrap().center();
    cx.simulate_click(goal, Modifiers::none());
    cx.simulate_keystrokes("ctrl-a");
    cx.simulate_input("Keep this draft");
    rt.block_on(async {
        let client = NodeClient::new(Some(&format!("{}/", running.url))).unwrap();
        let doc = common::call(&client, "mission_show", serde_json::json!({"mission":"hello","full":true})).await.unwrap();
        common::call(&client, "mission_edit", serde_json::json!({"mission":"hello", "change":json!({"expected_rev":doc["mission"]["rev"], "title":"Changed elsewhere"})}))
        .await
        .unwrap();
    });
    let save = cx.debug_bounds("step-save").unwrap().center();
    cx.simulate_click(save, Modifiers::none());
    until(cx, "merged save", |cx| {
        cx.debug_bounds("step-goal").is_none()
    });
    rt.block_on(async {
        let client = NodeClient::new(Some(&format!("{}/", running.url))).unwrap();
        let doc = common::call(
            &client,
            "mission_show",
            serde_json::json!({"mission":"hello","full":true}),
        )
        .await
        .unwrap();
        assert_eq!(doc["mission"]["title"], "Changed elsewhere");
        let step = doc["mission"]["step"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["id"] == "fmn_work")
            .unwrap();
        assert_eq!(step["brief"]["goal"], "Keep this draft");
    });
    let _ = std::fs::remove_dir_all(settings.parent().unwrap());
}

// The node is authored exclusively through the public CLI, then opened by the app.
#[gpui::test]
fn cli_authored_graph_and_layout_open_on_the_canvas(cx: &mut TestAppContext) {
    let running = start();
    let rt = tokio::runtime::Runtime::new().unwrap();
    async fn cli(url: &str, noun: &str, args: &[&str]) -> serde_json::Value {
        let mut argv: Vec<String> = args.iter().map(|s| (*s).to_owned()).collect();
        argv.extend(["--node".into(), format!("{url}/"), "--json".into()]);
        let mut output = Vec::new();
        let exit = ahjo_node_kit::cli::run(noun, &argv, &mut output)
            .await
            .unwrap();
        assert_eq!(
            exit,
            0,
            "{noun} {argv:?}: {}",
            String::from_utf8_lossy(&output)
        );
        serde_json::from_slice(&output).unwrap()
    }
    async fn edit(url: &str, change: serde_json::Value) -> serde_json::Value {
        cli(
            url,
            "mission",
            &["edit", "cli-board", "--change", &change.to_string()],
        )
        .await
    }
    let (input, steps, judge, human, end) = rt.block_on(async {
        let u = &running.url;
        cli(
            u,
            "mission",
            &["create", "cli-board", "--title", "CLI Board"],
        )
        .await;
        let input =
            edit(u, json!({"create_input_card":{"title":"Start"}})).await["input_card"]["id"]
                .as_str()
                .unwrap()
                .to_owned();
        let mut steps = Vec::new();
        for title in ["Draft", "Polish", "Judge"] {
            let step = edit(u, json!({"create_step":{"formation":"solo","title":title}})).await;
            let step = &step["step"];
            let id = step["id"].as_str().unwrap().to_owned();
            let input = format!("{id}:{}", step["input"][0]["id"].as_str().unwrap());
            let output = format!("{id}:{}", step["output"][0]["id"].as_str().unwrap());
            steps.push((id, input, output));
        }
        let judge = edit(
            u,
            json!({"create_gate":{"title":"Judge gate","kinds":["step"]}}),
        )
        .await["gate"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        edit(
            u,
            json!({"set_gate_judge":{"gate_id":judge,"chain":[steps[2].0]}}),
        )
        .await;
        let human = edit(u, json!({"create_gate":{"title":"Human gate"}})).await["gate"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let end = edit(u, json!({"create_end":{"outcome":"done"}})).await["end"]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        for (from, to) in [
            (format!("{input}:out"), steps[0].1.clone()),
            (steps[0].2.clone(), format!("{judge}:in")),
            (format!("{judge}:pass"), steps[1].1.clone()),
            (steps[1].2.clone(), format!("{human}:in")),
            (format!("{human}:pass"), format!("{end}:in")),
        ] {
            edit(u, json!({"wire_connection":{"from":from,"to":to}})).await;
        }
        cli(u, "mission", &["arrange", "cli-board"]).await;
        let change = json!({"cards":[{"id":steps[0].0,"x":123,"y":456}]});
        cli(u, "layout", &["edit", "cli-board", &change.to_string()]).await;
        (input, steps, judge, human, end)
    });
    let settings = std::env::temp_dir()
        .join(format!("ahjo-cli-canvas-{}", std::process::id()))
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
    let selector = Box::leak(format!("mission-{}-cli-board", running.id).into_boxed_str());
    until(cx, "CLI mission listing", |cx| {
        cx.debug_bounds(selector).is_some()
    });
    let at = cx.debug_bounds(selector).unwrap().center();
    cx.simulate_click(at, Modifiers::none());
    // Every card it authored is on the board, but the judge: a judge is
    // its seat on the gate it answers, which opens it.
    for id in std::iter::once(&input)
        .chain(steps[..2].iter().map(|s| &s.0))
        .chain([&judge, &human, &end])
    {
        let selector = Box::leak(format!("mission-card-{id}").into_boxed_str());
        until(cx, "CLI-authored canvas card", |cx| {
            cx.debug_bounds(selector).is_some()
        });
    }
    assert!(
        cx.debug_bounds(Box::leak(
            format!("mission-card-{}", steps[2].0).into_boxed_str()
        ))
        .is_none()
    );
    let seat = Box::leak(format!("canvas-judge-{judge}").into_boxed_str());
    let at = cx.debug_bounds(seat).expect("the judge's seat").center();
    cx.simulate_click(at, Modifiers::none());
    until(cx, "the judge's editor", |cx| {
        cx.debug_bounds("step-title").is_some()
    });
    let client = NodeClient::new(Some(&format!("{}/", running.url))).unwrap();
    let layout = rt
        .block_on(common::call(
            &client,
            "layout_show",
            serde_json::json!({"mission":"cli-board"}),
        ))
        .unwrap();
    assert!(
        layout["layout"]["card"]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| node["id"] == steps[0].0 && node["x"] == 123 && node["y"] == 456)
    );
}

#[gpui::test]
fn opens_a_mission_layout_notes_and_run_written_before_the_wire_upgrade(cx: &mut TestAppContext) {
    const MISSION: &str = include_str!("fixtures/wire-v1/hello.mission.toml");
    let root = ahjo_testkit::fresh_dir("ahjo-legacy-mission-app");
    let state = root.join("missions");
    for (path, contents) in [
        ("missions/hello.mission.toml", MISSION),
        (
            "layout/hello.layout.toml",
            include_str!("fixtures/wire-v1/hello.layout.toml"),
        ),
        (
            "notes/hello.notes.toml",
            include_str!("fixtures/wire-v1/hello.notes.toml"),
        ),
        ("runs/run_legacy.snapshot.toml", MISSION),
        (
            "runs/run_legacy.ndjson",
            include_str!("fixtures/wire-v1/run_legacy.ndjson"),
        ),
        (
            "runs/run_legacy.bindings.toml",
            include_str!("fixtures/wire-v1/run_legacy.bindings.toml"),
        ),
    ] {
        let path = state.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    let dir = root.clone();
    let running = serve(move || async move {
        let node = Node::start_in(dir, AREAS, "").await.unwrap();
        Started::new(node, ())
    });

    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let url = running.url.clone();
    let settings = root.join("app.json");
    let (_, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(
            &[ahjo_missions_app::build],
            Some(url),
            Some(settings),
            window,
            cx,
        )
    });
    let selector = Box::leak(format!("mission-{}-hello", running.id).into_boxed_str());
    until(cx, "migrated mission listing", |cx| {
        cx.debug_bounds(selector).is_some()
    });
    let mission = cx.debug_bounds(selector).unwrap().center();
    cx.simulate_click(mission, Modifiers::none());
    show_flow(cx);
    until(cx, "migrated mission flow", |cx| {
        cx.debug_bounds("flow-step-fmn_work").is_some()
    });
    // On its board, with Flow beside it.
    until(cx, "migrated canvas card", |cx| {
        cx.debug_bounds("mission-card-fmn_work").is_some()
    });
    until(cx, "recorded run", |cx| {
        cx.debug_bounds("mission-run-run_legacy").is_some()
    });
    let run = cx.debug_bounds("mission-run-run_legacy").unwrap().center();
    cx.simulate_click(run, Modifiers::none());
    until(cx, "migrated run details", |cx| {
        cx.debug_bounds("run-detail-run_legacy").is_some()
    });

    for (path, schema, field) in [
        ("missions/hello.mission.toml", 6, "input_card"),
        ("layout/hello.layout.toml", 3, "mission_id"),
        ("notes/hello.notes.toml", 3, "created_at"),
        ("runs/run_legacy.snapshot.toml", 6, "input_card"),
        ("runs/run_legacy.bindings.toml", 6, "seat_id"),
    ] {
        let text = std::fs::read_to_string(state.join(path)).unwrap();
        assert!(
            text.contains(&format!("schema = {schema}")) && text.contains(field),
            "{path}: {text}"
        );
    }
    let replayed = std::fs::read_to_string(state.join("runs/run_legacy.ndjson")).unwrap();
    assert!(replayed.contains("\"run_id\":\"run_legacy\"") && !replayed.contains("runId"));
    assert!(replayed.contains("\"userPort\"") && replayed.contains("\"card_id\":\"fmn_work\""));
    // The node removes its folder, `root`, as it stops.
    drop(running);
}
