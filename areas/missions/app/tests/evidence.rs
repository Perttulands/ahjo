#![cfg(unix)]
use ahjo_app_kit::{Ask, Host, Mounted, Page, ask::Answers, mount};
use ahjo_core::NodeId;
use ahjo_mission_engine::{ExecutionSink, Executor, LabExecutor, StepExecution, StepResult};
use ahjo_node_kit::{Built, Listing, Setup};
use ahjo_testkit::app::{Served, Started, click, serve, until};
use ahjo_testkit::{node::Node, sessions};
use gpui::{
    App, AppContext, Context, InteractiveElement, IntoElement, ParentElement, TestAppContext,
    VisualTestContext, Window, div, point, px,
};
use serde_json::{Value, json};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

static OPENED: Mutex<Vec<Ask>> = Mutex::new(Vec::new());
/// Answers as Files does: a beside ask opens the file in the window's
/// panel, beside the page that asked.
struct ToyFiles(Host);
struct Opened;
impl gpui::Render for Opened {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .debug_selector(|| "opened-file".into())
            .child("Opened file")
    }
}
impl ahjo_app_kit::panel::PanelView for Opened {
    fn head(&mut self, _: &mut Window, _: &mut Context<Self>) -> gpui::AnyElement {
        div().child("Opened file").into_any_element()
    }
}
impl gpui::Render for ToyFiles {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().child("Files")
    }
}
impl Page for ToyFiles {
    const ID: &'static str = "toy-files";
    const TITLE: &'static str = "Files";
    const ANSWERS: &'static [Answers] = &[Answers::OpenPath];
    fn answer(&mut self, ask: Ask, _: &'static str, _: &mut Window, cx: &mut Context<Self>) {
        if ask.beside() {
            let view = cx.new(|_| Opened);
            self.0
                .show_panel(ahjo_app_kit::panel::Panel::new("file", view), cx);
        }
        OPENED.lock().unwrap().push(ask);
    }
}
fn toy_files(host: Host, window: &mut Window, cx: &mut App) -> Mounted {
    mount(host, window, cx, |host, _, _| ToyFiles(host))
}
struct Scripted;
impl Executor for Scripted {
    fn execute_step(
        &self,
        request: StepExecution,
        sink: ExecutionSink,
    ) -> Result<StepResult, ahjo_core::Error> {
        let judge = request.step.id == "fmn_judge";
        let attempt = request.attempt;
        let path = request.artifact_dir.join("result.png");
        let mut result = LabExecutor.execute_step(request, sink)?;
        if judge {
            let verdict = if attempt == 1 { "fail" } else { "pass" };
            let reason = if attempt == 1 {
                "add coverage"
            } else {
                "covered"
            };
            let text = format!(
                "```ahjo-verdict\n{}\n```",
                json!({"verdict":verdict,"reason":reason,"evidence":[]})
            );
            result.text = text.clone();
            for output in result.outputs.values_mut() {
                output["text"] = json!(text);
            }
        } else {
            std::fs::write(&path, include_bytes!("fixtures/output.png")).unwrap();
            result.outputs["port_out"]["ref"] = json!(path);
        }
        Ok(result)
    }
}
fn build(setup: &Setup) -> anyhow::Result<Built> {
    ahjo_missions::build_with_executor(setup, Some(Box::new(Scripted)))
}
const MISSIONS: Listing = Listing {
    build,
    ..ahjo_missions::LISTING
};
fn mission() -> String {
    let solo = include_str!("fixtures/solo-lab.mission.toml");
    let judge = solo
        .split("[[step]]")
        .nth(1)
        .unwrap()
        .split("[[end]]")
        .next()
        .unwrap()
        .replace("fmn_work", "fmn_judge")
        .replace("seat_work", "seat_judge");
    let work = solo
        .replace("label = \"Result\"", "label = \"Result\"\nkind = \"file\"")
        .replace("to = \"end_done:in\"", "to = \"gate_review:in\"");
    format!(
        "{work}\n[[step]]{judge}\n[[gate]]\nid = \"gate_review\"\ntitle = \"Review\"\nkinds = [\"step\"]\n[[connection]]\nid = \"edge_pass\"\nfrom = \"gate_review:pass\"\nto = \"end_done:in\"\n[[connection]]\nid = \"edge_fail\"\nfrom = \"gate_review:fail\"\nto = \"fmn_work:port_in\"\n[[connection]]\nid = \"edge_judge\"\nfrom = \"gate_review:judge\"\nto = \"fmn_judge:port_in\"\n[[connection]]\nid = \"edge_return\"\nfrom = \"fmn_judge:port_out\"\nto = \"gate_review:judge\"\n"
    )
}
struct Fixture {
    run: String,
    evidence: Value,
}
fn start() -> Served<Fixture> {
    serve(move || async move {
        let node = Node::start(
            &[
                sessions::LISTING,
                ahjo_issue::LISTING,
                ahjo_files::LISTING,
                MISSIONS,
            ],
            "[files]\nroots = ['/tmp']\n",
        )
        .await
        .unwrap();
        let dir = node.dir().join("missions/missions");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("hello.mission.toml"), mission()).unwrap();
        let result = node.call("mission_run", json!({"mission":"hello","input":"inp_hello","inputs":{"brief":"ship"},"cwd":node.dir()})).await.unwrap();
        let id = result["run_id"].as_str().unwrap().to_owned();
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let answer = node.call("run_show", json!({"run":id})).await.unwrap();
            let state = &answer["state"];
            if state["final"] == true || state["status"] == "blocked" {
                assert_eq!(state["status"], "succeeded", "{state}");
                break;
            }
            assert!(Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let evidence = node
            .call("run_show", json!({"run":id,"evidence":true}))
            .await
            .unwrap();
        Started::new(node, Fixture { run: id, evidence })
    })
}
fn reveal_control(cx: &mut VisualTestContext, selector: &str) {
    let selector: &'static str = Box::leak(selector.to_owned().into_boxed_str());
    for _ in 0..10 {
        let Some(bounds) = cx.debug_bounds(selector) else {
            cx.run_until_parked();
            continue;
        };
        let y = f32::from(bounds.center().y);
        if (100. ..800.).contains(&y) {
            return;
        }
        cx.simulate_event(gpui::ScrollEvent {
            position: point(px(700.), px(500.)),
            delta: gpui::ScrollDelta::Pixels(point(px(0.), px((500. - y).clamp(-400., 400.)))),
            modifiers: Default::default(),
            touch_phase: gpui::TouchPhase::Moved,
            source: gpui::ScrollSource::Pointer,
        });
        cx.run_until_parked();
    }
}
#[gpui::test]
fn evidence_links_and_attempt_images(cx: &mut TestAppContext) {
    OPENED.lock().unwrap().clear();
    let running = start();
    cx.executor().allow_parking();
    cx.update(|cx| {
        ahjo_app_shell::prepare(cx);
        cx.set_app_identity("io.ahjo.evidence-test", "Ahjo evidence test");
    });
    let settings = std::env::temp_dir()
        .join(format!("ahjo-evidence-{}", std::process::id()))
        .join("app.json");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    let url = running.url.clone();
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        ahjo_app_shell::start(
            &[ahjo_missions_app::build, toy_files],
            Some(url),
            Some(settings),
            window,
            cx,
        )
    });
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
            .unwrap();
    });
    until(cx, "mission on its board", |cx| {
        cx.debug_bounds("mission-card-fmn_work").is_some()
    });
    shell.update(cx, |shell, cx| {
        Host::new(shell.hub())
            .for_page("test", &[])
            .ask(
                Ask::Reveal {
                    what: "run".into(),
                    id: running.run.clone(),
                    node: Some(NodeId(running.id.clone())),
                    part: None,
                    beside: false,
                },
                cx,
            )
            .unwrap();
    });
    let e = &running.evidence["evidence"];
    let problem = e["problems"]
        .as_array()
        .unwrap()
        .iter()
        // A step shows the latest of its problems.
        .rfind(|p| p["reason"]["text"] == "add coverage")
        .unwrap();
    let problem_selector = format!(
        "run-problem-{}-add coverage",
        problem["seq"].as_u64().unwrap()
    );
    until(cx, "failed verdict", |cx| {
        cx.debug_bounds(Box::leak(problem_selector.clone().into_boxed_str()))
            .is_some()
    });
    let files = e["files"].as_array().unwrap();
    let (index, file) = files
        .iter()
        .enumerate()
        .find(|(_, f)| f["path"].as_str().unwrap().ends_with("result.png"))
        .unwrap();
    let file_path = file["path"].as_str().unwrap().to_owned();
    let file_selector = format!("run-file-{index}");
    until(cx, "file", |cx| {
        cx.debug_bounds(Box::leak(file_selector.clone().into_boxed_str()))
            .is_some()
    });
    click(cx, &file_selector);
    until(cx, "file ask", |_| !OPENED.lock().unwrap().is_empty());
    until(cx, "the file shown beside the run", |cx| {
        cx.debug_bounds("opened-file").is_some() && cx.debug_bounds("panel-right").is_some()
    });
    assert_eq!(
        OPENED.lock().unwrap().last().unwrap(),
        &Ask::OpenPath {
            node: NodeId(running.id.clone()),
            path: file_path,
            beside: true
        }
    );
    // A step offers the brief of its latest attempt.
    let briefs = e["briefs"].as_array().unwrap();
    let brief = briefs
        .iter()
        .rfind(|b| b["step_id"] == briefs[0]["step_id"])
        .unwrap();
    let brief_selector = format!("run-brief-{}", brief["dispatch_seq"].as_u64().unwrap());
    click(cx, &brief_selector);
    until(cx, "brief ask", |_| OPENED.lock().unwrap().len() >= 2);
    assert_eq!(
        OPENED.lock().unwrap().last().unwrap(),
        &Ask::OpenPath {
            node: NodeId(running.id.clone()),
            path: brief["path"].as_str().unwrap().into(),
            beside: true
        }
    );
    until(cx, "steps", |cx| {
        cx.debug_bounds("run-step-fmn_work-done").is_some()
    });
    reveal_control(cx, "run-step-attempts-fmn_work");
    click(cx, "run-step-attempts-fmn_work");
    until(cx, "flow attempts window", |cx| {
        cx.debug_bounds("attempts-window").is_some()
    });
    click(cx, "attempts-close");
    until(cx, "flow attempts closed", |cx| {
        cx.debug_bounds("attempts-window").is_none()
    });
    cx.simulate_event(gpui::ScrollEvent {
        position: point(px(700.), px(500.)),
        delta: gpui::ScrollDelta::Pixels(point(px(0.), px(-800.))),
        modifiers: Default::default(),
        touch_phase: gpui::TouchPhase::Moved,
        source: gpui::ScrollSource::Pointer,
    });
    cx.run_until_parked();
    reveal_control(cx, "run-step-attempts-fmn_work");
    click(cx, "run-step-attempts-fmn_work");
    until(cx, "compare button", |cx| {
        cx.debug_bounds("attempt-compare").is_some()
    });
    click(cx, "attempt-compare");
    until(cx, "both decoded images", |cx| {
        cx.debug_bounds("attempt-image-1").is_some() && cx.debug_bounds("attempt-image-2").is_some()
    });
    let a = cx.debug_bounds("attempt-image-1").unwrap();
    let b = cx.debug_bounds("attempt-image-2").unwrap();
    assert!(
        a.origin.x < b.origin.x,
        "images must be side by side: {a:?}, {b:?}"
    );
    cx.simulate_keystrokes("c");
    until(cx, "single attempt", |cx| {
        cx.debug_bounds("attempt-card-1").is_none() && cx.debug_bounds("attempt-card-2").is_some()
    });
    cx.simulate_keystrokes("left");
    until(cx, "previous attempt", |cx| {
        cx.debug_bounds("attempt-card-1").is_some() && cx.debug_bounds("attempt-card-2").is_none()
    });
    cx.simulate_keystrokes("end");
    until(cx, "last attempt", |cx| {
        cx.debug_bounds("attempt-card-2").is_some()
    });
    cx.simulate_keystrokes("home");
    until(cx, "first attempt", |cx| {
        cx.debug_bounds("attempt-card-1").is_some()
    });
    cx.simulate_keystrokes("2");
    until(cx, "numbered attempt", |cx| {
        cx.debug_bounds("attempt-card-2").is_some()
    });
    cx.simulate_keystrokes("shift-left");
    until(cx, "keyboard comparison", |cx| {
        cx.debug_bounds("attempt-card-1").is_some() && cx.debug_bounds("attempt-card-2").is_some()
    });
    cx.simulate_keystrokes("escape");
    until(cx, "closed attempts", |cx| {
        cx.debug_bounds("attempts-window").is_none()
    });
}
