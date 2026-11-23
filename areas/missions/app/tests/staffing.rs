#![cfg(unix)]
mod common;
use ahjo_app_kit::{Ask, Host};
use ahjo_core::{NodeId, TypedCommand};
use ahjo_harness_contract::agents::{ListAgents, ListedAgents};
use ahjo_missions_contract::commands::MissionDefinition;
use ahjo_node_kit::{Area, Built, Listing, NodeClient, Setup};
use ahjo_testkit::app::{Served, Started, serve, until};
use ahjo_testkit::{node::Node, sessions};
use gpui::{Modifiers, TestAppContext, VisualTestContext, point, px};
use serde_json::json;
use std::sync::Arc;

struct AgentCatalog;
impl Area for AgentCatalog {
    fn commands(&self) -> Vec<ahjo_node_kit::Command> {
        vec![ahjo_node_kit::Command::for_request(
            ahjo_node_kit::Effect::Reads,
            |_: ListAgents| async {
                let catalog: ListedAgents = serde_json::from_value(json!({
                "node": "test",
                "agents": [
                    {"schema":1, "id":"reviewer", "name":"Zed", "instructions":"Review", "harness":"claude-code","model":"","effort":"low","notes":[]},
                    {"schema":1, "id":"planner", "name":"Alpha", "instructions":"Plan", "harness":"openai-codex","model":"","effort":"medium","notes":[]}
                ],
                "unreadable": [],
                "harnesses": [
                    {"id":"openai-codex", "efforts":["low","medium","high","xhigh"], "default_effort":"medium", "models":[{"id":"gpt-6","efforts":["low","high"]}]},
                    {"id":"claude-code", "efforts":["low","medium"], "default_effort":"medium", "models":[{"id":"opus"}]}
                ],
                "effort_policy": []
            })).unwrap();
                Ok(catalog)
            },
        )]
    }
}

fn build(_: &Setup) -> anyhow::Result<Built> {
    Ok(Built::Now(Arc::new(AgentCatalog)))
}
const AGENTS: Listing = Listing {
    name: "test-agents",
    kinds: &[],
    commands: &[ListAgents::NAME],
    build,
};
fn start() -> Served<()> {
    // The node checks a chosen model against this host's Codex models; give
    // it the one the catalog test node offers. Alone in its binary, so the
    // process-wide variable is safe to set.
    let codex = std::env::temp_dir().join(format!("ahjo-staffing-codex-{}", std::process::id()));
    std::fs::create_dir_all(&codex).unwrap();
    std::fs::write(
        codex.join("models_cache.json"),
        r#"{"models":[{"slug":"gpt-6","supported_reasoning_levels":[{"effort":"low"},{"effort":"high"}]}]}"#,
    )
    .unwrap();
    unsafe { std::env::set_var("CODEX_HOME", &codex) };
    serve(move || async move {
        let node = Node::start(
            &[
                sessions::LISTING,
                ahjo_issue::LISTING,
                ahjo_missions::LISTING,
                AGENTS,
            ],
            "",
        )
        .await
        .unwrap();
        let agents = node.dir().join("missions/agents");
        std::fs::create_dir_all(&agents).unwrap();
        std::fs::write(
            agents.join("planner.toml"),
            "schema = 1\nid = \"planner\"\nname = \"Alpha\"\ninstructions = \"Plan\"\nharness = \"openai-codex\"\nmodel = \"\"\neffort = \"medium\"\n",
        ).unwrap();
        let dir = node.dir().join("missions/missions");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("hello.mission.toml"),
            include_str!("fixtures/solo-lab.mission.toml"),
        )
        .unwrap();
        Started::new(node, ())
    })
}
fn click(cx: &mut VisualTestContext, selector: &'static str) {
    if selector.starts_with("seat-") || selector.starts_with("staffing-") {
        for _ in 0..8 {
            let at = cx.debug_bounds(selector).expect(selector).center();
            let y = f32::from(at.y);
            if (90. ..820.).contains(&y) {
                break;
            }
            scroll(cx, (500. - y).clamp(-400., 400.));
        }
    }
    let at = cx.debug_bounds(selector).expect(selector).center();
    cx.simulate_click(at, Modifiers::none());
}
fn scroll(cx: &mut VisualTestContext, delta: f32) {
    cx.simulate_event(gpui::ScrollEvent {
        position: point(px(1100.), px(500.)),
        delta: gpui::ScrollDelta::Pixels(point(px(0.), px(delta))),
        modifiers: Default::default(),
        touch_phase: gpui::TouchPhase::Moved,
        source: gpui::ScrollSource::Pointer,
    });
    cx.run_until_parked();
}
#[gpui::test]
fn catalog_pickers_offer_sorted_agents_and_model_efforts_and_save_the_choice(
    cx: &mut TestAppContext,
) {
    let running = start();
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let settings = std::env::temp_dir()
        .join(format!("ahjo-staffing-{}", std::process::id()))
        .join("app.json");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    let url = running.url.clone();
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(
            &[ahjo_missions_app::build],
            Some(url),
            Some(settings),
            window,
            cx,
        )
    });
    cx.simulate_resize(gpui::size(px(1440.), px(900.)));
    shell.update(cx, |shell, cx| {
        Host::new(shell.hub())
            .for_page("test", &[])
            .ask(
                Ask::Reveal {
                    what: "mission".into(),
                    id: "hello".into(),
                    node: Some(NodeId(running.id.clone())),
                    part: None,
                    beside: false,
                },
                cx,
            )
            .unwrap()
    });
    until(cx, "mission", |cx| {
        cx.debug_bounds("mission-card-fmn_work").is_some()
    });
    click(cx, "mission-card-fmn_work");
    until(cx, "step", |cx| cx.debug_bounds("step-save").is_some());
    scroll(cx, -900.);
    until(cx, "seat", |cx| cx.debug_bounds("seat-seat_work").is_some());
    click(cx, "seat-seat_work");
    until(cx, "catalog", |cx| {
        cx.debug_bounds("staffing-agent-planner").is_some()
    });
    assert!(cx.debug_bounds("staffing-agent-retired").is_none());
    let alpha = cx.debug_bounds("staffing-agent-planner").unwrap();
    let zed = cx.debug_bounds("staffing-agent-reviewer").unwrap();
    assert!(
        alpha.origin.y < zed.origin.y
            || (alpha.origin.y == zed.origin.y && alpha.origin.x < zed.origin.x)
    );
    click(cx, "staffing-agent-planner");
    until(cx, "selected agent", |cx| {
        cx.debug_bounds("staffing-agent-planner-selected").is_some()
    });
    // Clear the explicit overrides. The agent supplies the effective values.
    click(cx, "staffing-harness-default");
    until(cx, "inherited harness", |cx| {
        cx.debug_bounds("staffing-harness-default-selected")
            .is_some()
    });
    click(cx, "staffing-effort-default");
    until(cx, "inherited effort", |cx| {
        cx.debug_bounds("staffing-effort-default-selected")
            .is_some()
    });
    click(cx, "staffing-effort-high");
    until(cx, "selected effort", |cx| {
        cx.debug_bounds("staffing-effort-high-selected").is_some()
    });
    click(cx, "staffing-model-gpt-6");
    until(cx, "selected model", |cx| {
        cx.debug_bounds("staffing-model-gpt-6-selected").is_some()
    });
    assert!(cx.debug_bounds("staffing-effort-low").is_some());
    assert!(cx.debug_bounds("staffing-effort-medium").is_none());
    // Each pick staffs the seat; nothing else needs pressing.
    let rt = tokio::runtime::Runtime::new().unwrap();
    let client = NodeClient::new(Some(&format!("{}/", running.url))).unwrap();
    let read = || {
        let body = rt
            .block_on(common::call(
                &client,
                "mission_show",
                serde_json::json!({"mission":"hello","full":true}),
            ))
            .unwrap();
        let response: MissionDefinition = serde_json::from_value(body).unwrap();
        let mission = response.mission;
        (mission.rev, mission.steps[0].seats[0].clone())
    };
    until(cx, "staffing saved", |_| {
        let (_, seat) = read();
        seat.agent == "planner"
            && seat.model == "gpt-6"
            && seat.effort == ahjo_missions_contract::Effort::High
    });
    let (rev, original) = read();
    until(cx, "history ready after staffing", |cx| {
        cx.debug_bounds("undo").is_some()
    });

    click(cx, "staffing-model-default");
    until(cx, "inherited model saved", |_| {
        let (now, seat) = read();
        now == rev + 1 && seat.model.is_empty()
    });
    until(cx, "undo ready after harness change", |cx| {
        cx.debug_bounds("undo").is_some()
    });
    click(cx, "undo");
    until(cx, "staffing undo saved", |_| read().0 == rev + 2);
    assert_eq!(read().1, original, "undo must restore the exact seat");
    // Harness default is a distinct explicit choice when the agent has a model.
    click(cx, "staffing-model-harness-default");
    until(cx, "harness default saved", |_| {
        read().1.model == ahjo_missions_contract::HARNESS_DEFAULT_MODEL
    });
}
