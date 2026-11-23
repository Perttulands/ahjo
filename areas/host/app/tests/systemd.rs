//! A disposable user unit exercised through the app and CLI on a testkit node.
#![cfg(target_os = "linux")]
use ahjo_app_kit::link::client::LinkState;
use ahjo_node_kit::{NodeClient, cli};
use ahjo_testkit::app::{Served, Started, click, serve, until};
use ahjo_testkit::node::Node;
use gpui::TestAppContext;
use serde_json::{Value, json};
use std::{path::PathBuf, process::Command};

struct Unit {
    name: String,
    dir: PathBuf,
}
impl Unit {
    fn new() -> Self {
        let dir = ahjo_testkit::fresh_dir("host-service-unit");
        let name = format!(
            "ahjo-test-{}.service",
            dir.file_name().unwrap().to_string_lossy()
        );
        let file = dir.join(&name);
        std::fs::write(
            &file,
            "[Service]\nType=exec\nExecStart=/usr/bin/sleep 3600\n",
        )
        .unwrap();
        let out = Command::new("systemctl")
            .args(["--user", "link", "--runtime"])
            .arg(file)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        Self { name, dir }
    }
}
impl Drop for Unit {
    fn drop(&mut self) {
        for args in [
            vec!["stop", self.name.as_str()],
            vec!["disable", "--runtime", self.name.as_str()],
            vec!["reset-failed", self.name.as_str()],
        ] {
            let _ = Command::new("systemctl").arg("--user").args(args).output();
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
struct Fixture {
    config: PathBuf,
}
fn start(unit: &Unit) -> Served<Fixture> {
    let name = unit.name.clone();
    serve(move || async move {
        let node = Node::start(&[ahjo_host::LISTING], "[host]\nenabled = true\n")
            .await
            .unwrap();
        let user = std::env::var("USER").unwrap();
        let component = json!({"kind":"systemd-user", "user":user, "unit":name});
        let missing = json!({"kind":"systemd-user", "user":user, "unit":format!("missing-{name}")});
        std::fs::write(node.dir().join("services.json"), json!({"services":[
            {"id":"single", "name":"Disposable", "components":[component.clone()]},
            {"id":"group", "name":"Disposable group", "components":[component.clone(), component]},
            {"id":"missing", "name":"Missing unit", "components":[missing]}
        ]}).to_string()).unwrap();
        let config = node.dir().join("config.toml");
        Started::new(node, Fixture { config })
    })
}
fn selector(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}
fn command(url: &str, noun: &str, args: &[&str]) -> Value {
    let url = url.to_owned();
    let noun = noun.to_owned();
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    std::thread::spawn(move || {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async move {
                let mut args = args;
                args.extend(["--node".into(), url, "--json".into()]);
                let mut output = Vec::new();
                assert_eq!(cli::run(&noun, &args, &mut output).await.unwrap(), 0);
                serde_json::from_slice(&output).unwrap()
            })
    })
    .join()
    .unwrap()
}
/// An action the node refuses: its words.
fn refused(url: &str, path: &str) -> String {
    let url = url.to_owned();
    let path = path.to_owned();
    std::thread::spawn(move || {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async move {
                let client = NodeClient::new(Some(&url)).unwrap();
                let answer = client
                    .request(
                        axum::http::Method::POST,
                        &path,
                        Some(json!({"id":"missing"})),
                        None,
                    )
                    .await;
                format!("{:#}", answer.expect_err("refused"))
            })
    })
    .join()
    .unwrap()
}
#[gpui::test]
fn disposable_systemd_service_and_host_health_journey(cx: &mut TestAppContext) {
    let unit = Unit::new();
    let node = start(&unit);
    let temp = ahjo_testkit::fresh_dir("host-service-app");
    cx.executor().allow_parking();
    cx.update(ahjo_app_shell::prepare);
    let url = node.url.clone();
    let settings = temp.join("app.json");
    let (shell, cx) = cx.add_window_view(move |w, cx| {
        ahjo_app_shell::start(&[ahjo_host_app::build], Some(url), Some(settings), w, cx)
    });
    until(cx, "node link", |cx| {
        shell.read_with(cx, |s, cx| {
            s.hub().read(cx).fleet.here().state == LinkState::Open
        })
    });
    let state = |s| selector(format!("svc-state-{}-single-{s}", node.id));
    let action = |s| selector(format!("svc-{s}-{}-single", node.id));
    until(cx, "stopped unit and group", |cx| {
        cx.debug_bounds(state("stopped")).is_some()
            && cx
                .debug_bounds(selector(format!("svc-row-{}-group", node.id)))
                .is_some()
    });
    let listed = command(&node.url, "service", &["list"]);
    assert_eq!(listed["services"].as_array().unwrap().len(), 3);
    assert_eq!(
        listed["services"][1]["components"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    click(cx, selector(format!("svc-row-{}-single", node.id)));
    until(cx, "a stopped unit offers Start", |cx| {
        cx.debug_bounds(action("start")).is_some()
    });
    assert!(
        cx.debug_bounds(action("restart")).is_none(),
        "and only Start"
    );
    click(cx, action("start"));
    until(cx, "app start", |cx| {
        cx.debug_bounds(state("running")).is_some() && cx.debug_bounds(action("restart")).is_some()
    });
    click(cx, action("restart"));
    // The node's event of it says so on the status line.
    until(cx, "app restart", |cx| {
        shell.read_with(cx, |s, cx| {
            s.hub()
                .read(cx)
                .status
                .as_ref()
                .is_some_and(|v| v.text.contains("restarted"))
        })
    });
    // A running unit keeps Stop under More.
    until(cx, "More after the restart", |cx| {
        cx.debug_bounds("svc-more").is_some()
    });
    assert!(cx.debug_bounds(action("stop")).is_none());
    click(cx, "svc-more");
    until(cx, "Stop under More", |cx| {
        cx.debug_bounds("menu-label-Stop").is_some()
    });
    click(cx, "menu-label-Stop");
    until(cx, "app stop", |cx| {
        cx.debug_bounds(state("stopped")).is_some()
    });
    for verb in ["start", "restart", "stop"] {
        let answer = command(&node.url, "service", &[verb, "single"]);
        assert_eq!(answer["outcome"], "done", "{answer}");
    }
    // A unit systemd does not know is not known: listed as such, with
    // nothing to press, and systemd's refusal reaches whoever asks.
    click(cx, selector(format!("svc-row-{}-missing", node.id)));
    until(cx, "the missing unit, unknown", |cx| {
        cx.debug_bounds(selector(format!("svc-state-{}-missing-unknown", node.id)))
            .is_some()
            && cx.debug_bounds("pane-title-Missing unit").is_some()
    });
    for verb in ["start", "stop", "restart"] {
        let control = selector(format!("svc-{verb}-{}-missing", node.id));
        assert!(cx.debug_bounds(control).is_none(), "{verb}");
    }
    let missing = format!("missing-{}", unit.name);
    let expected = Command::new("systemctl")
        .args(["--user", "stop", &missing])
        .output()
        .unwrap();
    assert!(!expected.status.success());
    let words = String::from_utf8(expected.stderr)
        .unwrap()
        .trim()
        .to_string();
    let answer = refused(&node.url, "/api/call/service_restart");
    assert!(answer.contains(&words), "{answer}");
    let host = command(&node.url, "host", &["show", "--full"]);
    assert!(
        host["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == "service")
    );
    let url = node.url.clone();
    std::thread::spawn(move || {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(async move {
                let client = NodeClient::new(Some(&url)).unwrap();
                for path in ["/api/health", "/api/version"] {
                    let (body, _) = client
                        .request(axum::http::Method::GET, path, None, None)
                        .await
                        .unwrap();
                    assert!(body.is_object(), "{path}: {body}");
                }
            })
    })
    .join()
    .unwrap();
    click(cx, selector(format!("host-row-{}-machine", node.id)));
    until(cx, "service action in Host events", |cx| {
        cx.debug_bounds("host-event-Disposable stopped").is_some()
    });
    let config = command(&node.url, "config", &["show"]);
    assert_eq!(config["sections"]["host"]["enabled"], true);
    assert_eq!(config["path"], node.config.to_string_lossy().as_ref());
    click(cx, "tab-settings");
    click(cx, "settings-group-node");
    let config_row = selector("node-config:host:enabled:on".to_owned());
    until(cx, "host config on Settings", |cx| {
        cx.debug_bounds(config_row).is_some()
    });
    let settings = temp.join("app.json");
    let mut edited = shell.read_with(cx, |s, cx| s.hub().read(cx).settings.clone());
    edited.keys = false;
    std::fs::write(&settings, edited.text()).unwrap();
    until(cx, "external settings edit", |cx| {
        shell.read_with(cx, |s, cx| !s.hub().read(cx).settings.keys)
    });
    click(cx, "settings-group-app");
    let toggle = "setting-app-keys";
    until(cx, "settings toggle", |cx| {
        cx.debug_bounds(toggle).is_some()
    });
    click(cx, toggle);
    until(cx, "toggle reflects external edit", |cx| {
        shell.read_with(cx, |s, cx| s.hub().read(cx).settings.keys)
    });
    let _ = std::fs::remove_dir_all(temp);
}
