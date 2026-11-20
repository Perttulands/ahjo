#![cfg(unix)]
use ahjo_app_kit::{Ask, Host};
use ahjo_core::NodeId;
use ahjo_missions_contract::MissionResponse;
use ahjo_node_kit::{Area, Built, Listing, NodeClient, Setup};
use ahjo_testkit::{node::Node, sessions};
use gpui::{Modifiers, TestAppContext, VisualTestContext, point, px};
use serde_json::json;
use std::{
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

struct Roles;
impl Area for Roles {
    fn routes(&self) -> axum::Router {
        axum::Router::new().route("/api/roles", axum::routing::get(|| async {
            axum::Json(json!({"roles":[{"id":"reviewer","displayName":"Zed"},{"id":"retired","displayName":"Retired","status":"retired"},{"id":"planner","displayName":"Alpha"}],"harnesses":[{"id":"openai-codex","efforts":["low","medium","high","xhigh"],"models":[{"id":"gpt-6","efforts":["low","high"]}]},{"id":"claude-code","efforts":["low","medium"],"models":[{"id":"opus"}]}]}))
        }))
    }
}
fn build(_: &Setup) -> anyhow::Result<Built> {
    Ok(Built::Now(Arc::new(Roles)))
}
const ROLES: Listing = Listing {
    name: "test-roles",
    kinds: &[],
    commands: &[],
    build,
};
struct Running {
    url: String,
    node: String,
    stop: mpsc::Sender<()>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(t) = self.thread.take() {
            t.join().unwrap();
        }
    }
}
fn start() -> Running {
    // The node checks a chosen model against this host's Codex models; give
    // it the one the roles fake offers. Alone in its binary, so the
    // process-wide variable is safe to set.
    let codex = std::env::temp_dir().join(format!("ahjo-staffing-codex-{}", std::process::id()));
    std::fs::create_dir_all(&codex).unwrap();
    std::fs::write(
        codex.join("models_cache.json"),
        r#"{"models":[{"slug":"gpt-6","supported_reasoning_levels":[{"effort":"low"},{"effort":"high"}]}]}"#,
    )
    .unwrap();
    unsafe { std::env::set_var("CODEX_HOME", &codex) };
    let (ready, wait) = mpsc::channel();
    let (stop, stopped) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async move {
                let node = Node::start(
                    &[
                        sessions::LISTING,
                        ahjo_work::LISTING,
                        ahjo_missions::LISTING,
                        ROLES,
                    ],
                    "",
                )
                .await
                .unwrap();
                let dir = node.dir().join("missions/missions");
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::write(
                    dir.join("hello.mission.toml"),
                    include_str!("fixtures/solo-lab.mission.toml"),
                )
                .unwrap();
                ready.send((node.url(), node.id())).unwrap();
                while stopped.try_recv().is_err() {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                node.stop().await;
            })
    });
    let (url, node) = wait.recv_timeout(Duration::from_secs(20)).unwrap();
    Running {
        url,
        node,
        stop,
        thread: Some(thread),
    }
}
fn until(
    cx: &mut VisualTestContext,
    what: &str,
    mut f: impl FnMut(&mut VisualTestContext) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        cx.run_until_parked();
        if f(cx) {
            break;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn click(cx: &mut VisualTestContext, selector: &'static str) {
    if selector.starts_with("slot-") || selector.starts_with("staffing-") {
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
fn catalog_pickers_offer_sorted_roles_and_model_efforts_and_save_the_choice(
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
                    node: Some(NodeId(running.node.clone())),
                    part: None,
                    beside: false,
                },
                cx,
            )
            .unwrap()
    });
    until(cx, "mission mode", |cx| {
        cx.debug_bounds("mission-mode-canvas").is_some()
    });
    click(cx, "mission-mode-canvas");
    until(cx, "mission", |cx| {
        cx.debug_bounds("mission-card-fmn_work").is_some()
    });
    click(cx, "mission-card-fmn_work");
    until(cx, "step", |cx| cx.debug_bounds("step-save").is_some());
    scroll(cx, -900.);
    until(cx, "slot", |cx| cx.debug_bounds("slot-slot_work").is_some());
    click(cx, "slot-slot_work");
    until(cx, "catalog", |cx| {
        cx.debug_bounds("staffing-role-planner").is_some()
    });
    assert!(cx.debug_bounds("staffing-role-retired").is_none());
    let alpha = cx.debug_bounds("staffing-role-planner").unwrap();
    let zed = cx.debug_bounds("staffing-role-reviewer").unwrap();
    assert!(
        alpha.origin.y < zed.origin.y
            || (alpha.origin.y == zed.origin.y && alpha.origin.x < zed.origin.x)
    );
    click(cx, "staffing-role-planner");
    until(cx, "selected role", |cx| {
        cx.debug_bounds("staffing-role-planner-selected").is_some()
    });
    click(cx, "staffing-model-gpt-6");
    until(cx, "selected model", |cx| {
        cx.debug_bounds("staffing-model-gpt-6-selected").is_some()
    });
    assert!(cx.debug_bounds("staffing-effort-low-selected").is_some());
    assert!(cx.debug_bounds("staffing-effort-medium").is_none());
    click(cx, "staffing-effort-high");
    until(cx, "selected effort", |cx| {
        cx.debug_bounds("staffing-effort-high-selected").is_some()
    });
    click(cx, "staffing-harness-claude-code");
    until(cx, "harness", |cx| {
        cx.debug_bounds("staffing-harness-claude-code-selected")
            .is_some()
    });
    assert!(cx.debug_bounds("staffing-model-default-selected").is_some());
    assert!(cx.debug_bounds("staffing-effort-medium-selected").is_some());
    click(cx, "staffing-harness-openai-codex");
    until(cx, "codex", |cx| {
        cx.debug_bounds("staffing-harness-openai-codex-selected")
            .is_some()
    });
    click(cx, "staffing-model-gpt-6");
    until(cx, "model", |cx| {
        cx.debug_bounds("staffing-model-gpt-6-selected").is_some()
    });
    click(cx, "staffing-effort-high");
    until(cx, "effort", |cx| {
        cx.debug_bounds("staffing-effort-high-selected").is_some()
    });
    // Each pick staffs the slot; nothing else needs pressing.
    let rt = tokio::runtime::Runtime::new().unwrap();
    let client = NodeClient::new(Some(&format!("{}/", running.url))).unwrap();
    let read = || {
        let (body, _) = rt
            .block_on(client.request(http::Method::GET, "/api/missions/hello", None, None))
            .unwrap();
        let response: MissionResponse = serde_json::from_value(body).unwrap();
        let mission = response.mission;
        (mission.rev, mission.steps[0].slots[0].clone())
    };
    until(cx, "staffing saved", |_| {
        let (_, slot) = read();
        slot.agent_id == "planner"
            && slot.model == "gpt-6"
            && slot.effort == ahjo_missions_contract::Effort::High
    });
    let (rev, original) = read();
    until(cx, "history ready after staffing", |cx| {
        cx.debug_bounds("undo").is_some()
    });

    click(cx, "staffing-harness-claude-code");
    until(cx, "harness change saved", |_| {
        let (now, slot) = read();
        now == rev + 1
            && slot.harness == ahjo_missions_contract::Harness::ClaudeCode
            && slot.model.is_empty()
            && slot.effort == ahjo_missions_contract::Effort::Medium
    });
    until(cx, "undo ready after harness change", |cx| {
        cx.debug_bounds("undo").is_some()
    });
    click(cx, "undo");
    until(cx, "staffing undo saved", |_| read().0 == rev + 2);
    assert_eq!(read().1, original, "undo must restore the exact seat");
}
